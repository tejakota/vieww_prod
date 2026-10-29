//! PulseAudio — and every PipeWire desktop, through `pipewire-pulse`.
//!
//! The blocking "simple" API (`libpulse-simple`, loaded at run time): a
//! stream thread renders a block and writes it, and the write blocks until
//! the server has room, so the server's clock paces the callback. The
//! server's buffer is sized from [`StreamConfig::block`] (target length
//! four blocks, minimum request one block), which is what sets latency.
//! Samples are `float32le` at the requested rate and channel count; the
//! server converts to the device.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::dl::Library;
use super::{
    isolate_capture, isolate_render, Backend, BackendError, BlockInfo, CaptureFn, Counters,
    DeviceInfo, DeviceStream, RenderFn, StreamConfig, StreamImpl,
};

const PA_STREAM_PLAYBACK: c_int = 1;
const PA_STREAM_RECORD: c_int = 2;
const PA_SAMPLE_FLOAT32LE: c_int = 5;

#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

struct Api {
    _lib: Library,
    new: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        c_int,
        *const c_char,
        *const c_char,
        *const SampleSpec,
        *const c_void,
        *const BufferAttr,
        *mut c_int,
    ) -> *mut c_void,
    write: unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int,
    read: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *mut c_int) -> c_int,
    flush: unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int,
    free: unsafe extern "C" fn(*mut c_void),
    /// `pa_simple_get_latency`: microseconds from now to the speaker.
    latency: Option<unsafe extern "C" fn(*mut c_void, *mut c_int) -> u64>,
    strerror: Option<unsafe extern "C" fn(c_int) -> *const c_char>,
}

impl Api {
    fn load() -> Result<Self, String> {
        let lib = Library::open(&["libpulse-simple.so.0", "libpulse-simple.so"])?;
        let errlib = Library::open(&["libpulse.so.0", "libpulse.so"]).ok();
        // SAFETY: the types are libpulse-simple's documented signatures.
        unsafe {
            let strerror = errlib.as_ref().and_then(|l| l.get(c"pa_strerror").ok());
            Ok(Self {
                new: lib.get(c"pa_simple_new")?,
                write: lib.get(c"pa_simple_write")?,
                read: lib.get(c"pa_simple_read")?,
                flush: lib.get(c"pa_simple_flush")?,
                free: lib.get(c"pa_simple_free")?,
                latency: lib.get(c"pa_simple_get_latency").ok(),
                strerror,
                _lib: lib,
            })
        }
    }

    fn error(&self, code: c_int) -> String {
        match self.strerror {
            Some(f) => {
                // SAFETY: pa_strerror returns a static string.
                let p = unsafe { f(code) };
                if p.is_null() {
                    format!("error {code}")
                } else {
                    // SAFETY: a static NUL-terminated string.
                    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
                }
            }
            None => format!("error {code}"),
        }
    }
}

/// The PulseAudio backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pulse;

struct Handle(*mut c_void);
// SAFETY: a pa_simple is used by one thread at a time (the stream thread;
// `stop` only touches it after joining that thread).
unsafe impl Send for Handle {}

struct PulseStream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StreamImpl for PulseStream {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

enum Callback {
    Render(RenderFn),
    Capture(CaptureFn),
}

fn open(
    config: StreamConfig,
    callback: Callback,
    device: Option<&str>,
) -> Result<DeviceStream, BackendError> {
    let device = device
        .map(CString::new)
        .transpose()
        .map_err(|_| BackendError::Refused("a device name with a NUL byte".into()))?;
    let api = Arc::new(Api::load().map_err(BackendError::Unavailable)?);
    let ch = config.channels.clamp(1, 32);
    let spec = SampleSpec {
        format: PA_SAMPLE_FLOAT32LE,
        rate: config.rate,
        channels: u8::try_from(ch).unwrap_or(2),
    };
    let block_bytes = config.block.max(16) * u32::from(ch) * 4;
    let capture = matches!(callback, Callback::Capture(_));
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: if capture { u32::MAX } else { block_bytes * 4 },
        prebuf: if capture { u32::MAX } else { block_bytes * 2 },
        minreq: if capture { u32::MAX } else { block_bytes },
        fragsize: if capture { block_bytes } else { u32::MAX },
    };
    let name = CString::new("vieww").unwrap_or_default();
    let stream_name =
        CString::new(if capture { "capture" } else { "playback" }).unwrap_or_default();
    let mut err = 0;
    // SAFETY: valid strings, spec and attributes; null server/device mean
    // the defaults.
    let s = unsafe {
        (api.new)(
            null(),
            name.as_ptr(),
            if capture {
                PA_STREAM_RECORD
            } else {
                PA_STREAM_PLAYBACK
            },
            device.as_ref().map_or(null(), |d| d.as_ptr()),
            stream_name.as_ptr(),
            &raw const spec,
            null(),
            &raw const attr,
            &raw mut err,
        )
    };
    if s.is_null() {
        return Err(BackendError::Unavailable(api.error(err)));
    }
    let counters = Arc::new(Counters::default());
    // Pulse paces a plain thread here, but the callback contract stays the
    // one every backend keeps: a panicking closure is caught once, then the
    // stream is silence and `ended` — not a dead thread that `stop` waits
    // on forever.
    let callback = match callback {
        Callback::Render(r) => Callback::Render(isolate_render(r, &counters)),
        Callback::Capture(c) => Callback::Capture(isolate_capture(c, &counters)),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (c2, s2, a2, handle) = (counters.clone(), stop.clone(), api.clone(), Handle(s));
    let frames = config.block.max(16) as usize;
    let rate = config.rate;
    let thread = std::thread::Builder::new()
        .name("vieww-pulse".into())
        .spawn(move || {
            #[expect(clippy::cast_precision_loss, reason = "a block length")]
            let _rt = crate::rt::promote_audio_thread(std::time::Duration::from_secs_f64(
                frames as f64 / f64::from(rate.max(1)),
            ));
            let h = handle;
            let chn = usize::from(ch);
            let mut buf = vec![0.0f32; frames * chn];
            let mut callback = callback;
            let mut done = 0u64;
            while !s2.load(Ordering::Acquire) {
                #[expect(clippy::cast_precision_loss, reason = "stream time")]
                let info = BlockInfo {
                    frames,
                    channels: chn,
                    rate,
                    time: done as f64 / f64::from(rate),
                };
                let mut err = 0;
                let ok = match &mut callback {
                    Callback::Render(r) => {
                        buf.fill(0.0);
                        r(&mut buf, &info);
                        // SAFETY: a live stream and a buffer of that many bytes.
                        unsafe {
                            (a2.write)(h.0, buf.as_ptr().cast(), buf.len() * 4, &raw mut err) == 0
                        }
                    }
                    Callback::Capture(cap) => {
                        // SAFETY: as above.
                        let ok = unsafe {
                            (a2.read)(h.0, buf.as_mut_ptr().cast(), buf.len() * 4, &raw mut err)
                                == 0
                        };
                        if ok {
                            cap(&buf, &info);
                        }
                        ok
                    }
                };
                if !ok {
                    c2.ended.store(true, Ordering::Release);
                    break;
                }
                done += frames as u64;
                c2.block(frames);
                // The server's own figure, a few times a second (it is a
                // round trip to the server, so not every block) — and only
                // once playback is running: before the prebuffer (two
                // blocks) is full the server has no timing to report, and
                // `pa_simple_get_latency` blocks until its next periodic
                // update — a stall of up to two seconds at start.
                if let (Callback::Render(_), Some(lat)) = (&callback, a2.latency) {
                    let n = c2.callbacks.load(Ordering::Relaxed);
                    if n >= 16 && n % 16 == 0 {
                        let mut e = 0;
                        // SAFETY: a live stream and error pointer.
                        let us = unsafe { lat(h.0, &raw mut e) };
                        if e == 0 && us > 0 && us != u64::MAX {
                            c2.latency_ns
                                .store(us.saturating_mul(1_000), Ordering::Relaxed);
                        }
                    }
                }
            }
            // SAFETY: the stream thread owns the handle; free it once.
            unsafe {
                let mut e = 0;
                (a2.flush)(h.0, &raw mut e);
                (a2.free)(h.0);
            }
        })
        .map_err(|e| BackendError::Refused(e.to_string()))?;
    let _ = null_mut::<c_void>();
    Ok(DeviceStream::new(
        "pulse",
        StreamConfig {
            rate: config.rate,
            channels: ch,
            block: config.block.max(16),
        },
        counters,
        Box::new(PulseStream {
            stop,
            thread: Some(thread),
        }),
    ))
}

impl Backend for Pulse {
    fn name(&self) -> &'static str {
        "pulse"
    }

    fn open_output(
        &self,
        config: StreamConfig,
        render: RenderFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Render(render), None)
    }

    fn open_input(
        &self,
        config: StreamConfig,
        capture: CaptureFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Capture(capture), None)
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, BackendError> {
        list::sinks()
    }

    /// The server's subscription events (AU-09): the default sink moving,
    /// a sink appearing or going. The server moves a default-following
    /// stream itself, so there is nothing to reopen; the event tells the
    /// supervisor and the app's route listeners at once.
    fn watch(&self, wake: super::RouteWake) -> Option<super::RouteWatch> {
        route_watch(wake)
    }

    fn open_output_on(
        &self,
        device: &str,
        config: StreamConfig,
        render: RenderFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Render(render), Some(device))
    }
}

/// Route events from a PulseAudio-protocol server (PulseAudio itself, or
/// PipeWire's `pipewire-pulse`), for both backends' [`Backend::watch`].
pub(super) fn route_watch(wake: super::RouteWake) -> Option<super::RouteWatch> {
    list::watch(wake).map(|w| Box::new(w) as super::RouteWatch)
}

/// Listing sinks needs libpulse's asynchronous API (the simple API cannot
/// enumerate): a private main loop, a context, one `get_sink_info_list`
/// and one `get_server_info` (for the default sink), iterated to
/// completion. Only the leading fields of `pa_sink_info` and
/// `pa_server_info` are read — the ones every libpulse since 0.9 lays out
/// identically (later versions append fields; they never reorder).
mod list {
    use super::{BackendError, DeviceInfo, Library};
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::ptr::{null, null_mut};

    /// The head of `pa_sink_info`.
    #[repr(C)]
    struct SinkInfoHead {
        name: *const c_char,
        index: u32,
        description: *const c_char,
        sample_format: c_int,
        sample_rate: u32,
        sample_channels: u8,
    }

    /// The head of `pa_server_info`.
    #[repr(C)]
    struct ServerInfoHead {
        user_name: *const c_char,
        host_name: *const c_char,
        server_version: *const c_char,
        server_name: *const c_char,
        sample_format: c_int,
        sample_rate: u32,
        sample_channels: u8,
        default_sink_name: *const c_char,
    }

    type SinkCb = unsafe extern "C" fn(*mut c_void, *const SinkInfoHead, c_int, *mut c_void);
    type ServerCb = unsafe extern "C" fn(*mut c_void, *const ServerInfoHead, *mut c_void);

    struct Fns {
        _lib: Library,
        ml_new: unsafe extern "C" fn() -> *mut c_void,
        ml_api: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        ml_iterate: unsafe extern "C" fn(*mut c_void, c_int, *mut c_int) -> c_int,
        ml_free: unsafe extern "C" fn(*mut c_void),
        ctx_new: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
        ctx_connect:
            unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *const c_void) -> c_int,
        ctx_state: unsafe extern "C" fn(*mut c_void) -> c_int,
        ctx_disconnect: unsafe extern "C" fn(*mut c_void),
        ctx_unref: unsafe extern "C" fn(*mut c_void),
        sinks: unsafe extern "C" fn(*mut c_void, SinkCb, *mut c_void) -> *mut c_void,
        server: unsafe extern "C" fn(*mut c_void, ServerCb, *mut c_void) -> *mut c_void,
        op_state: unsafe extern "C" fn(*mut c_void) -> c_int,
        op_unref: unsafe extern "C" fn(*mut c_void),
        set_subscribe_cb: unsafe extern "C" fn(*mut c_void, SubscribeCb, *mut c_void),
        subscribe:
            unsafe extern "C" fn(*mut c_void, u32, *const c_void, *mut c_void) -> *mut c_void,
        ml_wakeup: unsafe extern "C" fn(*mut c_void),
    }

    type SubscribeCb = unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void);

    const READY: c_int = 4;
    const FAILED: c_int = 5;
    const TERMINATED: c_int = 6;
    const OP_RUNNING: c_int = 0;
    const NOAUTOSPAWN: c_int = 1;

    fn load() -> Result<Fns, String> {
        let lib = Library::open(&["libpulse.so.0", "libpulse.so"])?;
        // SAFETY: libpulse's documented signatures.
        unsafe {
            Ok(Fns {
                ml_new: lib.get(c"pa_mainloop_new")?,
                ml_api: lib.get(c"pa_mainloop_get_api")?,
                ml_iterate: lib.get(c"pa_mainloop_iterate")?,
                ml_free: lib.get(c"pa_mainloop_free")?,
                ctx_new: lib.get(c"pa_context_new")?,
                ctx_connect: lib.get(c"pa_context_connect")?,
                ctx_state: lib.get(c"pa_context_get_state")?,
                ctx_disconnect: lib.get(c"pa_context_disconnect")?,
                ctx_unref: lib.get(c"pa_context_unref")?,
                sinks: lib.get(c"pa_context_get_sink_info_list")?,
                server: lib.get(c"pa_context_get_server_info")?,
                op_state: lib.get(c"pa_operation_get_state")?,
                op_unref: lib.get(c"pa_operation_unref")?,
                set_subscribe_cb: lib.get(c"pa_context_set_subscribe_callback")?,
                subscribe: lib.get(c"pa_context_subscribe")?,
                ml_wakeup: lib.get(c"pa_mainloop_wakeup")?,
                _lib: lib,
            })
        }
    }

    fn text(p: *const c_char) -> String {
        if p.is_null() {
            String::new()
        } else {
            // SAFETY: libpulse hands NUL-terminated strings valid for the
            // callback's duration; copied here.
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    }

    unsafe extern "C" fn on_sink(
        _c: *mut c_void,
        i: *const SinkInfoHead,
        eol: c_int,
        ud: *mut c_void,
    ) {
        if eol != 0 || i.is_null() {
            return;
        }
        // SAFETY: `ud` is the `Vec` passed below, alive until the operation
        // completes; `i` is valid for this call.
        let (out, i) = unsafe { (&mut *ud.cast::<Vec<DeviceInfo>>(), &*i) };
        let name = text(i.description);
        out.push(DeviceInfo {
            id: text(i.name),
            name: if name.is_empty() { text(i.name) } else { name },
            outputs: Some(u16::from(i.sample_channels)),
            inputs: Some(0),
            is_default: false,
        });
    }

    unsafe extern "C" fn on_server(_c: *mut c_void, i: *const ServerInfoHead, ud: *mut c_void) {
        if i.is_null() {
            return;
        }
        // SAFETY: as in `on_sink`.
        let (out, i) = unsafe { (&mut *ud.cast::<String>(), &*i) };
        *out = text(i.default_sink_name);
    }

    pub(super) fn sinks() -> Result<Vec<DeviceInfo>, BackendError> {
        let f = load().map_err(BackendError::Unavailable)?;
        let mut sinks: Vec<DeviceInfo> = Vec::new();
        let mut default = String::new();
        // SAFETY: the documented libpulse sequence on a private main loop;
        // every object is released before return, and the callbacks' user
        // data outlive the operations that use them.
        let result = unsafe {
            let ml = (f.ml_new)();
            if ml.is_null() {
                return Err(BackendError::Unavailable("pa_mainloop_new failed".into()));
            }
            let ctx = (f.ctx_new)((f.ml_api)(ml), c"vieww".as_ptr());
            let mut ok = !ctx.is_null() && (f.ctx_connect)(ctx, null(), NOAUTOSPAWN, null()) >= 0;
            // Connect: a bounded number of blocking iterations.
            let mut spins = 0;
            while ok {
                let st = (f.ctx_state)(ctx);
                if st == READY {
                    break;
                }
                if st == FAILED || st == TERMINATED || spins > 1_000 {
                    ok = false;
                    break;
                }
                (f.ml_iterate)(ml, 1, null_mut());
                spins += 1;
            }
            if ok {
                for op in [
                    (f.sinks)(ctx, on_sink, std::ptr::from_mut(&mut sinks).cast()),
                    (f.server)(ctx, on_server, std::ptr::from_mut(&mut default).cast()),
                ] {
                    if op.is_null() {
                        continue;
                    }
                    let mut n = 0;
                    while (f.op_state)(op) == OP_RUNNING && n < 10_000 {
                        (f.ml_iterate)(ml, 1, null_mut());
                        n += 1;
                    }
                    (f.op_unref)(op);
                }
            }
            if !ctx.is_null() {
                (f.ctx_disconnect)(ctx);
                (f.ctx_unref)(ctx);
            }
            (f.ml_free)(ml);
            ok
        };
        if !result {
            return Err(BackendError::Unavailable("no PulseAudio server".into()));
        }
        for s in &mut sinks {
            s.is_default = s.id == default;
        }
        Ok(sinks)
    }

    /// `pa_subscription_mask`: sinks and the server (whose change event is
    /// the default sink moving).
    const MASK_SINK: u32 = 0x0001;
    const MASK_SERVER: u32 = 0x0080;
    const FACILITY_MASK: u32 = 0x000F;
    const FACILITY_SINK: u32 = 0x0000;
    const FACILITY_SERVER: u32 = 0x0007;
    const TYPE_MASK: u32 = 0x0030;
    const TYPE_CHANGE: u32 = 0x0010;

    unsafe extern "C" fn on_event(_c: *mut c_void, t: u32, _idx: u32, ud: *mut c_void) {
        // SAFETY: `ud` is the watch thread's `RouteWake`, alive while the
        // context is (the thread owns both and frees the context first).
        let wake = unsafe { &*ud.cast::<crate::backend::RouteWake>() };
        let facility = t & FACILITY_MASK;
        let kind = t & TYPE_MASK;
        // The default sink moving (a server change), a sink appearing or
        // going. A sink's own change events (volume, state) are noise.
        if facility == FACILITY_SERVER || (facility == FACILITY_SINK && kind != TYPE_CHANGE) {
            wake();
        }
    }

    /// Stops the watch thread when dropped.
    pub(crate) struct Watch {
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        ml: std::sync::Arc<std::sync::atomic::AtomicPtr<c_void>>,
        wakeup: unsafe extern "C" fn(*mut c_void),
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            use std::sync::atomic::Ordering;
            self.stop.store(true, Ordering::Release);
            let ml = self.ml.load(Ordering::Acquire);
            if !ml.is_null() {
                // SAFETY: pa_mainloop_wakeup is documented as callable from
                // another thread; the loop is freed only after this thread
                // is joined below... by the watch thread itself, which
                // clears the pointer first under the stop flag.
                unsafe { (self.wakeup)(ml) };
            }
            if let Some(t) = self.thread.take() {
                t.join().ok();
            }
        }
    }

    /// Subscribe to the server's sink and default-sink events (AU-09) on a
    /// thread with its own main loop. `None` with no server.
    pub(crate) fn watch(wake: crate::backend::RouteWake) -> Option<Watch> {
        use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
        use std::sync::Arc;
        let f = load().ok()?;
        let wakeup = f.ml_wakeup;
        let stop = Arc::new(AtomicBool::new(false));
        let shared_ml = Arc::new(AtomicPtr::new(null_mut::<c_void>()));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();
        let (stop2, ml2) = (Arc::clone(&stop), Arc::clone(&shared_ml));
        let thread = std::thread::Builder::new()
            .name("vieww-audio-routes".into())
            .spawn(move || {
                let mut wake = wake;
                // SAFETY: the documented libpulse sequence on a private main
                // loop owned by this thread; `wake` outlives the context.
                unsafe {
                    let ml = (f.ml_new)();
                    if ml.is_null() {
                        let _ = ready_tx.send(false);
                        return;
                    }
                    let ctx = (f.ctx_new)((f.ml_api)(ml), c"vieww-routes".as_ptr());
                    let mut ok =
                        !ctx.is_null() && (f.ctx_connect)(ctx, null(), NOAUTOSPAWN, null()) >= 0;
                    let mut spins = 0;
                    while ok {
                        let st = (f.ctx_state)(ctx);
                        if st == READY {
                            break;
                        }
                        if st == FAILED || st == TERMINATED || spins > 1_000 {
                            ok = false;
                            break;
                        }
                        (f.ml_iterate)(ml, 1, null_mut());
                        spins += 1;
                    }
                    if ok {
                        (f.set_subscribe_cb)(ctx, on_event, std::ptr::from_mut(&mut wake).cast());
                        let op = (f.subscribe)(ctx, MASK_SINK | MASK_SERVER, null(), null_mut());
                        if op.is_null() {
                            ok = false;
                        } else {
                            (f.op_unref)(op);
                        }
                    }
                    ml2.store(ml, Ordering::Release);
                    let _ = ready_tx.send(ok);
                    while ok && !stop2.load(Ordering::Acquire) {
                        if (f.ml_iterate)(ml, 1, null_mut()) < 0 {
                            break;
                        }
                        let st = (f.ctx_state)(ctx);
                        if st == FAILED || st == TERMINATED {
                            break;
                        }
                    }
                    // Wait for the stop flag before freeing, so a wakeup
                    // from `Watch::drop` never reaches a freed loop.
                    while !stop2.load(Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    ml2.store(null_mut(), Ordering::Release);
                    if !ctx.is_null() {
                        (f.ctx_disconnect)(ctx);
                        (f.ctx_unref)(ctx);
                    }
                    (f.ml_free)(ml);
                }
            })
            .ok()?;
        let ok = ready_rx.recv().unwrap_or(false);
        let w = Watch {
            stop,
            ml: shared_ml,
            wakeup,
            thread: Some(thread),
        };
        ok.then_some(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // Lists the server's sinks and plays on a chosen one — with CI's null
    // sink, `null` — or, with no server, refuses both cleanly.
    #[test]
    fn lists_and_opens_a_named_sink_or_refuses_cleanly() {
        let cfg = StreamConfig {
            rate: 48_000,
            channels: 2,
            block: 480,
        };
        match Pulse.devices() {
            Ok(sinks) => {
                assert!(!sinks.is_empty(), "a running server has a sink");
                assert_eq!(
                    sinks.iter().filter(|s| s.is_default).count(),
                    1,
                    "{sinks:?}"
                );
                let pick = &sinks[0];
                assert!(!pick.id.is_empty() && !pick.name.is_empty());
                let stream = Pulse
                    .open_output_on(&pick.id, cfg, Box::new(|_, _| {}))
                    .expect("a listed sink opens");
                assert!(
                    stream.wait_for(10, Duration::from_secs(10)),
                    "{:?}",
                    stream.stats()
                );
                drop(stream);
                assert!(
                    Pulse
                        .open_output_on("no-such-sink-vieww", cfg, Box::new(|_, _| {}))
                        .is_err(),
                    "an unknown sink is refused, not swapped for the default"
                );
            }
            Err(e) => {
                assert!(matches!(e, BackendError::Unavailable(_)), "{e:?}");
                assert!(Pulse
                    .open_output_on("null", cfg, Box::new(|_, _| {}))
                    .is_err());
            }
        }
    }

    // Against a real server when one is running (CI starts PulseAudio with
    // a null sink), else the refusal.
    #[test]
    fn plays_at_the_servers_pace_or_refuses_cleanly() {
        let cfg = StreamConfig {
            rate: 48_000,
            channels: 2,
            block: 480,
        };
        match Pulse.open_output(
            cfg,
            Box::new(|buf, _| {
                for (i, v) in buf.iter_mut().enumerate() {
                    *v = (i as f32 * 0.01).sin() * 0.1;
                }
            }),
        ) {
            Ok(stream) => {
                assert_eq!(stream.config(), cfg);
                // Let the initial buffer fill, then measure the pace: the
                // server consumes 48 000 frames a second, so ~100 blocks.
                assert!(
                    stream.wait_for(20, Duration::from_secs(10)),
                    "{:?}",
                    stream.stats()
                );
                let (f0, t0) = (stream.stats().frames, Instant::now());
                std::thread::sleep(Duration::from_millis(1_000));
                let rate = (stream.stats().frames - f0) as f64 / t0.elapsed().as_secs_f64();
                assert!(
                    (rate - 48_000.0).abs() < 4_800.0,
                    "consumed {rate} frames/s"
                );
                assert!(!stream.stats().ended);
            }
            Err(e) => {
                assert!(
                    !super::super::expected("pulse"),
                    "pulse was expected to be running: {e}"
                );
                assert!(matches!(e, BackendError::Unavailable(_)), "{e}");
            }
        }
    }

    #[test]
    fn records_or_refuses_cleanly() {
        match Pulse.open_input(
            StreamConfig {
                rate: 16_000,
                channels: 1,
                block: 160,
            },
            Box::new(|buf, info| assert_eq!(buf.len(), info.frames)),
        ) {
            Ok(stream) => assert!(stream.wait_for(5, Duration::from_secs(10))),
            Err(e) => {
                assert!(
                    !super::super::expected("pulse"),
                    "pulse was expected to be running: {e}"
                );
                assert!(matches!(e, BackendError::Unavailable(_)), "{e}");
            }
        }
    }
}
