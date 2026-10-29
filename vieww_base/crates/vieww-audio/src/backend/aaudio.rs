//! AAudio: Android's low-latency audio API (Android 8.0, API 26+).
//!
//! `libaaudio.so` is loaded at run time (so an APK built for older API
//! levels still starts, and reports the backend unavailable). A stream is
//! built for float samples in low-latency performance mode with a data
//! callback, which AAudio runs on its real-time thread with an
//! interleaved buffer to fill (or read). Signatures and constants follow
//! `<aaudio/AAudio.h>`.
//!
//! What the stream asks for and reports (AU-08):
//!
//! * **Exclusive mode first.** [`AaudioOptions::exclusive`] (default on)
//!   asks for `AAUDIO_SHARING_MODE_EXCLUSIVE`, the MMAP path that bypasses
//!   the system mixer; a device that refuses gets the shared stream it
//!   always got. [`StreamStats::exclusive`](super::StreamStats::exclusive)
//!   says which was granted.
//! * **Usage and content type** (API 28+) from the audio session's focus
//!   request (`crate::session`): media, speech, game or sonification, so
//!   the system routes and ducks the stream as what it is. Capture can be
//!   marked privacy-sensitive (API 30+).
//! * **Buffer tuning.** After opening, the buffer is set to
//!   [`AaudioOptions::buffer_bursts`] bursts (default 2, Oboe's
//!   starting point); each time AAudio reports new underruns it grows by
//!   one burst, up to the capacity, as Oboe's latency tuner does.
//! * **Latency and underruns.** Output latency comes from
//!   `AAudioStream_getTimestamp` against frames written; underruns from
//!   `AAudioStream_getXRunCount`. AAudio was the one backend of nine that
//!   reported neither.
//! * **Disconnects.** The error callback still ends the stream on any
//!   error, and marks `disconnected` when the error is
//!   `AAUDIO_ERROR_DISCONNECTED` (headphones out, a Bluetooth device gone,
//!   the default device changed), which `resilient.rs` reopens from.
//!
//! The API 28 and 30 calls are looked up optionally, so Android 8 still
//! opens a stream.

use std::ffi::{c_char, c_void, CStr};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use super::dl::Library;
use super::{
    isolate_capture, isolate_render, Backend, BackendError, BlockInfo, CaptureFn, Counters,
    DeviceStream, RenderFn, StreamConfig, StreamImpl,
};

type Result_ = i32;
const AAUDIO_OK: Result_ = 0;
const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
const AAUDIO_DIRECTION_INPUT: i32 = 1;
const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
const AAUDIO_SHARING_MODE_EXCLUSIVE: i32 = 0;
const AAUDIO_SHARING_MODE_SHARED: i32 = 1;
const AAUDIO_ERROR_DISCONNECTED: Result_ = -899;
const CLOCK_MONOTONIC: i32 = 1;
/// How often (in callbacks) the data callback samples underruns and latency.
const SAMPLE_EVERY: u32 = 32;

#[repr(C)]
struct Timespec {
    tv_sec: std::ffi::c_long,
    tv_nsec: std::ffi::c_long,
}

extern "C" {
    fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
}

fn now_ns() -> i64 {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid out-pointer; CLOCK_MONOTONIC always exists.
    unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut ts) };
    #[allow(
        clippy::useless_conversion,
        reason = "c_long is i64 on arm64 and x86_64 but i32 on 32-bit Android"
    )]
    let ns = i64::from(ts.tv_sec) * 1_000_000_000 + i64::from(ts.tv_nsec);
    ns
}

/// What an AAudio stream asks for (AU-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AaudioOptions {
    /// Ask for exclusive (MMAP) mode, falling back to shared on refusal.
    pub exclusive: bool,
    /// The starting buffer, in bursts (at least 1).
    pub buffer_bursts: u32,
    /// Mark capture privacy-sensitive (API 30+): no other app may capture
    /// concurrently, and it is excluded from accessibility capture.
    pub privacy_sensitive: bool,
}

impl Default for AaudioOptions {
    fn default() -> Self {
        Self {
            exclusive: true,
            buffer_bursts: 2,
            privacy_sensitive: false,
        }
    }
}
const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;
const AAUDIO_CALLBACK_RESULT_CONTINUE: i32 = 0;

type DataCb = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, i32) -> i32;
type ErrorCb = unsafe extern "C" fn(*mut c_void, *mut c_void, Result_);

struct Api {
    _lib: Library,
    create_builder: unsafe extern "C" fn(*mut *mut c_void) -> Result_,
    set_direction: unsafe extern "C" fn(*mut c_void, i32),
    set_sample_rate: unsafe extern "C" fn(*mut c_void, i32),
    set_channel_count: unsafe extern "C" fn(*mut c_void, i32),
    set_format: unsafe extern "C" fn(*mut c_void, i32),
    set_sharing_mode: unsafe extern "C" fn(*mut c_void, i32),
    set_performance_mode: unsafe extern "C" fn(*mut c_void, i32),
    set_frames_per_data_callback: unsafe extern "C" fn(*mut c_void, i32),
    set_data_callback: unsafe extern "C" fn(*mut c_void, DataCb, *mut c_void),
    set_error_callback: unsafe extern "C" fn(*mut c_void, ErrorCb, *mut c_void),
    open_stream: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Result_,
    builder_delete: unsafe extern "C" fn(*mut c_void) -> Result_,
    request_start: unsafe extern "C" fn(*mut c_void) -> Result_,
    request_stop: unsafe extern "C" fn(*mut c_void) -> Result_,
    close: unsafe extern "C" fn(*mut c_void) -> Result_,
    get_sample_rate: unsafe extern "C" fn(*mut c_void) -> i32,
    get_channel_count: unsafe extern "C" fn(*mut c_void) -> i32,
    get_frames_per_burst: unsafe extern "C" fn(*mut c_void) -> i32,
    convert_result_to_text: unsafe extern "C" fn(Result_) -> *const c_char,
    get_sharing_mode: unsafe extern "C" fn(*mut c_void) -> i32,
    set_buffer_size: unsafe extern "C" fn(*mut c_void, i32) -> Result_,
    get_buffer_size: unsafe extern "C" fn(*mut c_void) -> i32,
    get_buffer_capacity: unsafe extern "C" fn(*mut c_void) -> i32,
    get_xrun_count: unsafe extern "C" fn(*mut c_void) -> i32,
    get_frames_written: unsafe extern "C" fn(*mut c_void) -> i64,
    get_timestamp: unsafe extern "C" fn(*mut c_void, i32, *mut i64, *mut i64) -> Result_,
    // API 28+.
    set_usage: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    set_content_type: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    // API 30+.
    set_privacy_sensitive: Option<unsafe extern "C" fn(*mut c_void, bool)>,
}

impl Api {
    fn load() -> Result<Self, String> {
        let lib = Library::open(&["libaaudio.so"])?;
        // SAFETY: the types are <aaudio/AAudio.h>'s signatures.
        unsafe {
            Ok(Self {
                create_builder: lib.get(c"AAudio_createStreamBuilder")?,
                set_direction: lib.get(c"AAudioStreamBuilder_setDirection")?,
                set_sample_rate: lib.get(c"AAudioStreamBuilder_setSampleRate")?,
                set_channel_count: lib.get(c"AAudioStreamBuilder_setChannelCount")?,
                set_format: lib.get(c"AAudioStreamBuilder_setFormat")?,
                set_sharing_mode: lib.get(c"AAudioStreamBuilder_setSharingMode")?,
                set_performance_mode: lib.get(c"AAudioStreamBuilder_setPerformanceMode")?,
                set_frames_per_data_callback: lib
                    .get(c"AAudioStreamBuilder_setFramesPerDataCallback")?,
                set_data_callback: lib.get(c"AAudioStreamBuilder_setDataCallback")?,
                set_error_callback: lib.get(c"AAudioStreamBuilder_setErrorCallback")?,
                open_stream: lib.get(c"AAudioStreamBuilder_openStream")?,
                builder_delete: lib.get(c"AAudioStreamBuilder_delete")?,
                request_start: lib.get(c"AAudioStream_requestStart")?,
                request_stop: lib.get(c"AAudioStream_requestStop")?,
                close: lib.get(c"AAudioStream_close")?,
                get_sample_rate: lib.get(c"AAudioStream_getSampleRate")?,
                get_channel_count: lib.get(c"AAudioStream_getChannelCount")?,
                get_frames_per_burst: lib.get(c"AAudioStream_getFramesPerBurst")?,
                convert_result_to_text: lib.get(c"AAudio_convertResultToText")?,
                get_sharing_mode: lib.get(c"AAudioStream_getSharingMode")?,
                set_buffer_size: lib.get(c"AAudioStream_setBufferSizeInFrames")?,
                get_buffer_size: lib.get(c"AAudioStream_getBufferSizeInFrames")?,
                get_buffer_capacity: lib.get(c"AAudioStream_getBufferCapacityInFrames")?,
                get_xrun_count: lib.get(c"AAudioStream_getXRunCount")?,
                get_frames_written: lib.get(c"AAudioStream_getFramesWritten")?,
                get_timestamp: lib.get(c"AAudioStream_getTimestamp")?,
                set_usage: lib.get(c"AAudioStreamBuilder_setUsage").ok(),
                set_content_type: lib.get(c"AAudioStreamBuilder_setContentType").ok(),
                set_privacy_sensitive: lib.get(c"AAudioStreamBuilder_setPrivacySensitive").ok(),
                _lib: lib,
            })
        }
    }

    fn text(&self, r: Result_) -> String {
        // SAFETY: AAudio returns a static string.
        let p = unsafe { (self.convert_result_to_text)(r) };
        if p.is_null() {
            format!("AAudio error {r}")
        } else {
            // SAFETY: a static NUL-terminated string.
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    }
}

enum Callback {
    Render(RenderFn),
    Capture(CaptureFn),
}

struct Shared {
    callback: Callback,
    /// Granted values, written by `open` after the stream exists. They are
    /// atomic because the data callback — registered before those values
    /// are known, the builder API allows nothing else — reads them; a
    /// plain store would make the safety of the read depend on AAudio
    /// never invoking the callback before `requestStart`, which is true of
    /// the API's ordering but true of nothing else.
    channels: AtomicUsize,
    rate: AtomicU32,
    counters: Arc<Counters>,
    frames_done: u64,
    /// Copied out of `Api` for the callback (plain function pointers).
    tune: Tuning,
}

/// What the data callback needs to tune the buffer and read latency.
struct Tuning {
    output: bool,
    burst: i32,
    buffer: i32,
    capacity: i32,
    xruns: i32,
    since_sample: u32,
    set_buffer_size: unsafe extern "C" fn(*mut c_void, i32) -> Result_,
    get_xrun_count: unsafe extern "C" fn(*mut c_void) -> i32,
    get_frames_written: unsafe extern "C" fn(*mut c_void) -> i64,
    get_timestamp: unsafe extern "C" fn(*mut c_void, i32, *mut i64, *mut i64) -> Result_,
}

impl Tuning {
    /// Every [`SAMPLE_EVERY`] callbacks: fold new underruns into the
    /// counters and grow the buffer by a burst for them; read the output
    /// latency. Non-blocking calls only, as the callback requires.
    ///
    /// # Safety
    /// `stream` is the live stream this callback belongs to.
    unsafe fn sample(&mut self, stream: *mut c_void, rate: u32, counters: &Counters) {
        self.since_sample += 1;
        if self.since_sample < SAMPLE_EVERY {
            return;
        }
        self.since_sample = 0;
        // SAFETY: the caller's contract.
        let xruns = unsafe { (self.get_xrun_count)(stream) };
        if xruns > self.xruns {
            counters
                .xruns
                .store(u64::try_from(xruns).unwrap_or(0), Ordering::Relaxed);
            self.xruns = xruns;
            if self.buffer + self.burst <= self.capacity {
                // SAFETY: as above; returns the size granted, or an error.
                let granted = unsafe { (self.set_buffer_size)(stream, self.buffer + self.burst) };
                if granted > 0 {
                    self.buffer = granted;
                    counters
                        .buffer_frames
                        .store(u64::try_from(granted).unwrap_or(0), Ordering::Relaxed);
                }
            }
        }
        if !self.output || rate == 0 {
            return;
        }
        let (mut position, mut time) = (0i64, 0i64);
        // SAFETY: as above, with valid out-pointers.
        let ok = unsafe {
            (self.get_timestamp)(stream, CLOCK_MONOTONIC, &raw mut position, &raw mut time)
        };
        if ok != AAUDIO_OK {
            return;
        }
        // SAFETY: as above.
        let written = unsafe { (self.get_frames_written)(stream) };
        // When the next frame written is presented, less now.
        let ahead = (written - position).max(0);
        let presented_at = time + ahead * 1_000_000_000 / i64::from(rate);
        let latency = presented_at - now_ns();
        if latency > 0 {
            counters
                .latency_ns
                .store(u64::try_from(latency).unwrap_or(0), Ordering::Relaxed);
        }
    }
}

unsafe extern "C" fn on_data(
    stream: *mut c_void,
    user: *mut c_void,
    data: *mut c_void,
    frames: i32,
) -> i32 {
    // SAFETY: `user` is the boxed `Shared`, alive until the stream closes;
    // AAudio calls one data callback at a time.
    let s = unsafe { &mut *user.cast::<Shared>() };
    let n = usize::try_from(frames.max(0)).unwrap_or(0);
    let ch = s.channels.load(Ordering::Acquire);
    let rate = s.rate.load(Ordering::Acquire);
    let len = n * ch;
    #[expect(clippy::cast_precision_loss, reason = "stream time")]
    let info = BlockInfo {
        frames: n,
        channels: ch,
        rate,
        time: s.frames_done as f64 / f64::from(rate),
    };
    match &mut s.callback {
        Callback::Render(r) => {
            // SAFETY: AAudio hands `frames` interleaved float frames.
            let out = unsafe { std::slice::from_raw_parts_mut(data.cast::<f32>(), len) };
            out.fill(0.0);
            r(out, &info);
        }
        Callback::Capture(c) => {
            // SAFETY: as above.
            let input = unsafe { std::slice::from_raw_parts(data.cast::<f32>(), len) };
            c(input, &info);
        }
    }
    s.frames_done += n as u64;
    s.counters.block(n);
    // SAFETY: `stream` is this callback's live stream.
    unsafe { s.tune.sample(stream, rate, &s.counters) };
    AAUDIO_CALLBACK_RESULT_CONTINUE
}

unsafe extern "C" fn on_error(_stream: *mut c_void, user: *mut c_void, error: Result_) {
    // SAFETY: as in `on_data`; only atomics are touched.
    let s = unsafe { &*user.cast::<Shared>() };
    if error == AAUDIO_ERROR_DISCONNECTED {
        s.counters.disconnected.store(true, Ordering::Release);
    }
    s.counters.ended.store(true, Ordering::Release);
}

struct AaStream {
    api: Arc<Api>,
    stream: *mut c_void,
    shared: *mut Shared,
}

// SAFETY: AAudio stream handles may be stopped and closed from any thread.
unsafe impl Send for AaStream {}

impl StreamImpl for AaStream {
    fn stop(&mut self) {
        if self.stream.is_null() {
            return;
        }
        // SAFETY: stop then close; no callback runs after close.
        unsafe {
            (self.api.request_stop)(self.stream);
            (self.api.close)(self.stream);
            drop(Box::from_raw(self.shared));
        }
        self.stream = null_mut();
    }
}

fn open(
    config: StreamConfig,
    callback: Callback,
    options: AaudioOptions,
) -> Result<DeviceStream, BackendError> {
    let api = Arc::new(Api::load().map_err(BackendError::Unavailable)?);
    let capture = matches!(callback, Callback::Capture(_));
    let ch = config.channels.clamp(1, 8);
    let counters = Arc::new(Counters::default());
    // The callback crosses an `extern "C"` boundary on AAudio's data
    // thread: isolate it before it is stored, so a panicking closure ends
    // the stream instead of the process.
    let callback = match callback {
        Callback::Render(r) => Callback::Render(isolate_render(r, &counters)),
        Callback::Capture(c) => Callback::Capture(isolate_capture(c, &counters)),
    };
    let shared = Box::into_raw(Box::new(Shared {
        callback,
        channels: AtomicUsize::new(usize::from(ch)),
        rate: AtomicU32::new(config.rate),
        counters: counters.clone(),
        frames_done: 0,
        tune: Tuning {
            output: !capture,
            burst: 0,
            buffer: 0,
            capacity: 0,
            xruns: 0,
            since_sample: 0,
            set_buffer_size: api.set_buffer_size,
            get_xrun_count: api.get_xrun_count,
            get_frames_written: api.get_frames_written,
            get_timestamp: api.get_timestamp,
        },
    }));
    // What the session says is playing (media, speech, game…).
    let request = crate::session::AudioSession::shared().request();
    // SAFETY: the AAudio builder protocol; every result checked.
    unsafe {
        let build = |sharing: i32| -> Result<*mut c_void, Result_> {
            let mut b = null_mut();
            let r = (api.create_builder)(&raw mut b);
            if r != AAUDIO_OK {
                return Err(r);
            }
            (api.set_direction)(
                b,
                if capture {
                    AAUDIO_DIRECTION_INPUT
                } else {
                    AAUDIO_DIRECTION_OUTPUT
                },
            );
            (api.set_sample_rate)(b, i32::try_from(config.rate).unwrap_or(48_000));
            (api.set_channel_count)(b, i32::from(ch));
            (api.set_format)(b, AAUDIO_FORMAT_PCM_FLOAT);
            (api.set_sharing_mode)(b, sharing);
            (api.set_performance_mode)(b, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
            if !capture {
                if let Some(f) = api.set_usage {
                    f(b, request.android_usage());
                }
                if let Some(f) = api.set_content_type {
                    f(b, request.android_content_type());
                }
            } else if options.privacy_sensitive {
                if let Some(f) = api.set_privacy_sensitive {
                    f(b, true);
                }
            }
            (api.set_frames_per_data_callback)(b, i32::try_from(config.block).unwrap_or(0));
            (api.set_data_callback)(b, on_data, shared.cast());
            (api.set_error_callback)(b, on_error, shared.cast());
            let mut stream = null_mut();
            let r = (api.open_stream)(b, &raw mut stream);
            (api.builder_delete)(b);
            if r == AAUDIO_OK {
                Ok(stream)
            } else {
                Err(r)
            }
        };
        // Exclusive first, when asked; a refusal falls back to shared.
        let opened = if options.exclusive {
            build(AAUDIO_SHARING_MODE_EXCLUSIVE).or_else(|_| build(AAUDIO_SHARING_MODE_SHARED))
        } else {
            build(AAUDIO_SHARING_MODE_SHARED)
        };
        let stream = match opened {
            Ok(s) => s,
            Err(r) => {
                drop(Box::from_raw(shared));
                return Err(BackendError::Refused(api.text(r)));
            }
        };
        counters.exclusive.store(
            (api.get_sharing_mode)(stream) == AAUDIO_SHARING_MODE_EXCLUSIVE,
            Ordering::Relaxed,
        );
        // The device may grant another rate or channel count.
        let rate = u32::try_from((api.get_sample_rate)(stream)).unwrap_or(config.rate);
        let channels = u16::try_from((api.get_channel_count)(stream)).unwrap_or(ch);
        (*shared).rate.store(rate, Ordering::Release);
        (*shared)
            .channels
            .store(usize::from(channels), Ordering::Release);
        let burst_frames = (api.get_frames_per_burst)(stream).max(1);
        let burst = u32::try_from(burst_frames).unwrap_or(config.block);
        // Start at `buffer_bursts` bursts; the callback grows it on
        // underruns.
        let capacity = (api.get_buffer_capacity)(stream);
        let want =
            burst_frames.saturating_mul(i32::try_from(options.buffer_bursts.max(1)).unwrap_or(2));
        let granted = (api.set_buffer_size)(stream, want.min(capacity.max(burst_frames)));
        let buffer = if granted > 0 {
            granted
        } else {
            (api.get_buffer_size)(stream)
        };
        counters
            .buffer_frames
            .store(u64::try_from(buffer.max(0)).unwrap_or(0), Ordering::Relaxed);
        if !capture {
            // Until the first timestamp: the buffer is the floor.
            counters.set_latency_frames(u64::try_from(buffer.max(0)).unwrap_or(0), rate);
        }
        (*shared).tune.burst = burst_frames;
        (*shared).tune.buffer = buffer;
        (*shared).tune.capacity = capacity;
        let r = (api.request_start)(stream);
        if r != AAUDIO_OK {
            (api.close)(stream);
            drop(Box::from_raw(shared));
            return Err(BackendError::Refused(api.text(r)));
        }
        Ok(DeviceStream::new(
            "aaudio",
            StreamConfig {
                rate,
                channels,
                block: burst,
            },
            counters,
            Box::new(AaStream {
                api,
                stream,
                shared,
            }),
        ))
    }
}

/// The AAudio backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct AAudio {
    /// What its streams ask for.
    pub options: AaudioOptions,
}

impl Backend for AAudio {
    fn name(&self) -> &'static str {
        "aaudio"
    }

    fn open_output(
        &self,
        config: StreamConfig,
        render: RenderFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Render(render), self.options)
    }

    fn open_input(
        &self,
        config: StreamConfig,
        capture: CaptureFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Capture(capture), self.options)
    }
}
