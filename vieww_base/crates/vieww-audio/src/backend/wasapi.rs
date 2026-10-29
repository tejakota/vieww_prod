//! WASAPI: Windows' native audio API (Vista and later; the automatic
//! format conversion used here needs Windows 10).
//!
//! Shared mode, event driven: the default endpoint's `IAudioClient` is
//! initialised with our format (32-bit float, the requested rate and
//! channels — `AUTOCONVERTPCM | SRC_DEFAULT_QUALITY` has the audio engine
//! convert to the device), and the stream thread waits on the client's
//! event, then fills `buffer size − padding` frames through
//! `IAudioRenderClient` (or drains `IAudioCaptureClient` packets). Every
//! COM object is created, used and released on that one thread, which
//! initialises COM as multithreaded. The interfaces' vtables, GUIDs and
//! `WAVEFORMATEXTENSIBLE` are transcribed from the Windows SDK headers (as
//! the MinGW-w64 headers reproduce them).
//!
//! [`WasapiExclusive`] is the pro-audio path: the endpoint is taken for
//! this stream alone (no engine mixing, no engine conversion), at the
//! best [`PcmFormat`] and rate `IsFormatSupported` accepts — the requested
//! rate first, then the nearest standard one, converted to with the sinc
//! resampler — with a buffer of one device period (the endpoint's minimum,
//! or the requested block if longer) refilled on each event. Latency is
//! the period, typically 3 ms on a USB interface, against the shared
//! engine's 10 ms plus. Another exclusive stream holding the endpoint is a
//! refusal ("in use"), never a silent fall-back to shared mode.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::device::{decode_samples, encode_samples, negotiate, PcmFormat};
use crate::resample::{Quality, Resampler};

use super::{
    isolate_capture, isolate_render, Backend, BackendError, BlockInfo, CaptureFn, Counters,
    DeviceInfo, DeviceStream, RenderFn, StreamConfig, StreamImpl,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid(u32, u16, u16, [u8; 8]);

const CLSID_MM_DEVICE_ENUMERATOR: Guid = Guid(
    0xbcde_0395,
    0xe52f,
    0x467c,
    [0x8e, 0x3d, 0xc4, 0x57, 0x92, 0x91, 0x69, 0x2e],
);
const IID_IMM_DEVICE_ENUMERATOR: Guid = Guid(
    0xa956_64d2,
    0x9614,
    0x4f35,
    [0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6],
);
const IID_IAUDIO_CLIENT: Guid = Guid(
    0x1cb9_ad4c,
    0xdbfa,
    0x4c32,
    [0xb1, 0x78, 0xc2, 0xf5, 0x68, 0xa7, 0x03, 0xb2],
);
const IID_IAUDIO_RENDER_CLIENT: Guid = Guid(
    0xf294_acfc,
    0x3146,
    0x4483,
    [0xa7, 0xbf, 0xad, 0xdc, 0xa7, 0xc2, 0x60, 0xe2],
);
const IID_IAUDIO_CAPTURE_CLIENT: Guid = Guid(
    0xc8ad_bd64,
    0xe71e,
    0x48a0,
    [0xa4, 0xde, 0x18, 0x5c, 0x39, 0x5c, 0xd3, 0x17],
);
const KSDATAFORMAT_SUBTYPE_PCM: Guid = Guid(
    0x0000_0001,
    0x0000,
    0x0010,
    [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71],
);
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: Guid = Guid(
    0x0000_0003,
    0x0000,
    0x0010,
    [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71],
);

const CLSCTX_ALL: u32 = 0x17;
const COINIT_MULTITHREADED: u32 = 0;
const E_RENDER: i32 = 0;
const E_CAPTURE: i32 = 1;
const E_CONSOLE: i32 = 0;
const SHAREMODE_SHARED: i32 = 0;
const SHAREMODE_EXCLUSIVE: i32 = 1;
/// `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED`: retry with the aligned size.
#[expect(clippy::cast_possible_wrap, reason = "an HRESULT bit pattern")]
const E_BUFFER_SIZE_NOT_ALIGNED: i32 = 0x8889_0019_u32 as i32;
/// `AUDCLNT_E_DEVICE_IN_USE`: another exclusive stream has the endpoint.
#[expect(clippy::cast_possible_wrap, reason = "an HRESULT bit pattern")]
const E_DEVICE_IN_USE: i32 = 0x8889_000A_u32 as i32;
const STREAMFLAGS_EVENTCALLBACK: u32 = 0x0004_0000;
const STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;
const STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
const BUFFERFLAGS_SILENT: u32 = 0x2;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const WAIT_OBJECT_0: u32 = 0;

/// `WAVEFORMATEXTENSIBLE` (byte-packed, 40 bytes).
#[repr(C, packed(1))]
#[derive(Clone, Copy)]
struct WaveFormatExtensible {
    format_tag: u16,
    channels: u16,
    samples_per_sec: u32,
    avg_bytes_per_sec: u32,
    block_align: u16,
    bits_per_sample: u16,
    cb_size: u16,
    valid_bits_per_sample: u16,
    channel_mask: u32,
    sub_format: Guid,
}

/// The `WAVEFORMATEXTENSIBLE` for `fmt`. 24-in-32 is MSB-aligned on
/// Windows (valid bits 24 in a 32-bit container, the low byte ignored), so
/// it is written as [`PcmFormat::S32`] — see [`wire_format`].
fn wave_format(fmt: PcmFormat, rate: u32, ch: u16) -> WaveFormatExtensible {
    let (bits, valid, sub) = match fmt {
        PcmFormat::F32 => (32, 32, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT),
        PcmFormat::S32 => (32, 32, KSDATAFORMAT_SUBTYPE_PCM),
        PcmFormat::S24 => (32, 24, KSDATAFORMAT_SUBTYPE_PCM),
        PcmFormat::S24Packed => (24, 24, KSDATAFORMAT_SUBTYPE_PCM),
        PcmFormat::S16 => (16, 16, KSDATAFORMAT_SUBTYPE_PCM),
    };
    let block = ch * (bits / 8);
    WaveFormatExtensible {
        format_tag: WAVE_FORMAT_EXTENSIBLE,
        channels: ch,
        samples_per_sec: rate,
        avg_bytes_per_sec: rate * u32::from(block),
        block_align: block,
        bits_per_sample: bits,
        cb_size: 22,
        valid_bits_per_sample: valid,
        channel_mask: channel_mask(ch),
        sub_format: sub,
    }
}

/// The encoding the samples are written in for a negotiated format.
const fn wire_format(fmt: PcmFormat) -> PcmFormat {
    match fmt {
        PcmFormat::S24 => PcmFormat::S32,
        f => f,
    }
}

type Hresult = i32;
type This = *mut c_void;

#[repr(C)]
struct UnknownVtbl {
    query_interface: unsafe extern "system" fn(This, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(This) -> u32,
    release: unsafe extern "system" fn(This) -> u32,
}

/// `IMMDeviceEnumerator` (mmdeviceapi.h), methods in declaration order.
#[repr(C)]
struct DeviceEnumeratorVtbl {
    unknown: UnknownVtbl,
    enum_audio_endpoints: unsafe extern "system" fn(This, i32, u32, *mut *mut c_void) -> Hresult,
    get_default_audio_endpoint: unsafe extern "system" fn(This, i32, i32, *mut This) -> Hresult,
    get_device: unsafe extern "system" fn(This, *const u16, *mut This) -> Hresult,
    register_endpoint_notification_callback: unsafe extern "system" fn(This, This) -> Hresult,
    unregister_endpoint_notification_callback: unsafe extern "system" fn(This, This) -> Hresult,
}

/// `IMMDevice`, in declaration order.
#[repr(C)]
struct DeviceVtbl {
    unknown: UnknownVtbl,
    activate: unsafe extern "system" fn(This, *const Guid, u32, *mut c_void, *mut This) -> Hresult,
    open_property_store: unsafe extern "system" fn(This, u32, *mut This) -> Hresult,
    get_id: unsafe extern "system" fn(This, *mut *mut u16) -> Hresult,
}

/// `IMMDeviceCollection`.
#[repr(C)]
struct DeviceCollectionVtbl {
    unknown: UnknownVtbl,
    get_count: unsafe extern "system" fn(This, *mut u32) -> Hresult,
    item: unsafe extern "system" fn(This, u32, *mut This) -> Hresult,
}

/// `IPropertyStore` (propsys.h).
#[repr(C)]
struct PropertyStoreVtbl {
    unknown: UnknownVtbl,
    get_count: unsafe extern "system" fn(This, *mut u32) -> Hresult,
    get_at: unsafe extern "system" fn(This, u32, *mut PropertyKey) -> Hresult,
    get_value: unsafe extern "system" fn(This, *const PropertyKey, *mut PropVariant) -> Hresult,
}

/// `PROPERTYKEY`.
#[repr(C)]
struct PropertyKey {
    fmtid: Guid,
    pid: u32,
}

/// `PROPVARIANT`, sized for 64-bit (24 bytes; over-sized and harmless on
/// 32-bit). Only the string arm is read.
#[repr(C)]
struct PropVariant {
    vt: u16,
    _reserved: [u16; 3],
    pwsz: *mut u16,
    _rest: u64,
}

/// `PKEY_Device_FriendlyName` (functiondiscoverykeys_devpkey.h).
const PKEY_DEVICE_FRIENDLY_NAME: PropertyKey = PropertyKey {
    fmtid: Guid(
        0xa45c_254e,
        0xdf1c,
        0x4efd,
        [0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0],
    ),
    pid: 14,
};
const VT_LPWSTR: u16 = 31;
const DEVICE_STATE_ACTIVE: u32 = 1;
const STGM_READ: u32 = 0;

#[repr(C)]
struct AudioClientVtbl {
    unknown: UnknownVtbl,
    initialize: unsafe extern "system" fn(
        This,
        i32,
        u32,
        i64,
        i64,
        *const WaveFormatExtensible,
        *const Guid,
    ) -> Hresult,
    get_buffer_size: unsafe extern "system" fn(This, *mut u32) -> Hresult,
    get_stream_latency: unsafe extern "system" fn(This, *mut i64) -> Hresult,
    get_current_padding: unsafe extern "system" fn(This, *mut u32) -> Hresult,
    is_format_supported:
        unsafe extern "system" fn(This, i32, *const c_void, *mut *mut c_void) -> Hresult,
    get_mix_format: unsafe extern "system" fn(This, *mut *mut c_void) -> Hresult,
    get_device_period: unsafe extern "system" fn(This, *mut i64, *mut i64) -> Hresult,
    start: unsafe extern "system" fn(This) -> Hresult,
    stop: unsafe extern "system" fn(This) -> Hresult,
    reset: unsafe extern "system" fn(This) -> Hresult,
    set_event_handle: unsafe extern "system" fn(This, *mut c_void) -> Hresult,
    get_service: unsafe extern "system" fn(This, *const Guid, *mut This) -> Hresult,
}

/// `IAudioClient3` (Windows 10+): `IAudioClient`, then `IAudioClient2`'s
/// three methods, then its own three (AU-10).
#[repr(C)]
struct AudioClient3Vtbl {
    base: AudioClientVtbl,
    is_offload_capable: unsafe extern "system" fn(This, i32, *mut i32) -> Hresult,
    set_client_properties: unsafe extern "system" fn(This, *const c_void) -> Hresult,
    get_buffer_size_limits:
        unsafe extern "system" fn(This, *const c_void, i32, *mut i64, *mut i64) -> Hresult,
    get_shared_mode_engine_period: unsafe extern "system" fn(
        This,
        *const c_void,
        *mut u32,
        *mut u32,
        *mut u32,
        *mut u32,
    ) -> Hresult,
    get_current_shared_mode_engine_period:
        unsafe extern "system" fn(This, *mut *mut c_void, *mut u32) -> Hresult,
    initialize_shared_audio_stream:
        unsafe extern "system" fn(This, u32, u32, *const c_void, *const Guid) -> Hresult,
}

const IID_IAUDIO_CLIENT3: Guid = Guid(
    0x7ed4_ee07,
    0x8e67,
    0x4cd4,
    [0x8c, 0x1a, 0x2b, 0x7a, 0x59, 0x87, 0xad, 0x42],
);

/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`'s first field (the rest is the shared
/// tail, `GUID_TAIL` in wav.rs).
const SUBTYPE_IEEE_FLOAT: u32 = 3;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

#[repr(C)]
struct RenderClientVtbl {
    unknown: UnknownVtbl,
    get_buffer: unsafe extern "system" fn(This, u32, *mut *mut u8) -> Hresult,
    release_buffer: unsafe extern "system" fn(This, u32, u32) -> Hresult,
}

#[repr(C)]
struct CaptureClientVtbl {
    unknown: UnknownVtbl,
    get_buffer: unsafe extern "system" fn(
        This,
        *mut *mut u8,
        *mut u32,
        *mut u32,
        *mut u64,
        *mut u64,
    ) -> Hresult,
    release_buffer: unsafe extern "system" fn(This, u32) -> Hresult,
    get_next_packet_size: unsafe extern "system" fn(This, *mut u32) -> Hresult,
}

#[cfg_attr(windows, link(name = "ole32"))]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, co_init: u32) -> Hresult;
    fn CoUninitialize();
    fn CoCreateInstance(
        clsid: *const Guid,
        outer: *mut c_void,
        ctx: u32,
        iid: *const Guid,
        out: *mut This,
    ) -> Hresult;
    fn CoTaskMemFree(p: *mut c_void);
    fn PropVariantClear(v: *mut PropVariant) -> Hresult;
}

#[cfg_attr(windows, link(name = "kernel32"))]
unsafe extern "system" {
    fn CreateEventW(
        attrs: *mut c_void,
        manual_reset: i32,
        initial: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

/// `vtable!(ptr, Vtbl)`: the vtable of a COM pointer.
macro_rules! vt {
    ($p:expr, $t:ty) => {
        &**$p.cast::<*const $t>()
    };
}

/// A COM pointer released on drop.
struct Com(This);

impl Drop for Com {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a live COM pointer we own one reference to.
            unsafe { (vt!(self.0, UnknownVtbl).release)(self.0) };
        }
    }
}

fn check(hr: Hresult, what: &str) -> Result<(), String> {
    if hr < 0 {
        Err(format!(
            "{what} failed (HRESULT {:#010x})",
            hr.cast_unsigned()
        ))
    } else {
        Ok(())
    }
}

fn channel_mask(ch: u16) -> u32 {
    match ch {
        1 => 0x4,   // FC
        2 => 0x3,   // FL FR
        4 => 0x33,  // FL FR BL BR
        6 => 0x3F,  // 5.1
        8 => 0x63F, // 7.1 (side)
        n => (1u32 << n.min(18)) - 1,
    }
}

enum Callback {
    Render(RenderFn),
    Capture(CaptureFn),
}

struct WasapiStream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StreamImpl for WasapiStream {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

/// Everything set up on the stream thread.
struct Session {
    _enumerator: Com,
    _device: Com,
    client: Com,
    service: Com,
    event: *mut c_void,
    buffer_frames: u32,
    /// `GetStreamLatency` plus the buffer, nanoseconds (0 = unknown).
    latency_ns: u64,
    /// Exclusive mode: the wire encoding (`None`: shared float, written
    /// in place).
    format: Option<PcmFormat>,
    /// The rate the endpoint runs at.
    device_rate: u32,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: a live client; stopping an unstarted client is harmless.
        unsafe { (vt!(self.client.0, AudioClientVtbl).stop)(self.client.0) };
        if !self.event.is_null() {
            // SAFETY: an event handle we created.
            unsafe { CloseHandle(self.event) };
        }
    }
}

/// A NUL-terminated UTF-16 string from COM, copied; the original freed.
///
/// # Safety
/// `p` is null or a `CoTaskMemAlloc`ed, NUL-terminated wide string.
unsafe fn take_wide(p: *mut u16) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract.
    unsafe {
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        CoTaskMemFree(p.cast());
        s
    }
}

/// An endpoint's id string.
///
/// # Safety
/// `d` is a live `IMMDevice`.
unsafe fn device_id(d: This) -> String {
    let mut id = null_mut();
    // SAFETY: a live device; the string is ours to free.
    unsafe {
        if (vt!(d, DeviceVtbl).get_id)(d, &raw mut id) < 0 {
            return String::new();
        }
        take_wide(id)
    }
}

/// An endpoint's friendly name ("Speakers (Realtek High Definition
/// Audio)").
///
/// # Safety
/// `d` is a live `IMMDevice`.
unsafe fn device_name(d: This) -> String {
    // SAFETY: a live device; the store and the variant are released.
    unsafe {
        let mut store = null_mut();
        if (vt!(d, DeviceVtbl).open_property_store)(d, STGM_READ, &raw mut store) < 0
            || store.is_null()
        {
            return String::new();
        }
        let store = Com(store);
        let mut v = PropVariant {
            vt: 0,
            _reserved: [0; 3],
            pwsz: null_mut(),
            _rest: 0,
        };
        let mut name = String::new();
        if (vt!(store.0, PropertyStoreVtbl).get_value)(
            store.0,
            &PKEY_DEVICE_FRIENDLY_NAME,
            &raw mut v,
        ) >= 0
            && v.vt == VT_LPWSTR
            && !v.pwsz.is_null()
        {
            let mut n = 0;
            while *v.pwsz.add(n) != 0 {
                n += 1;
            }
            name = String::from_utf16_lossy(std::slice::from_raw_parts(v.pwsz, n));
        }
        PropVariantClear(&raw mut v);
        name
    }
}

/// The active render endpoints, on a private MTA thread (the caller's own
/// COM apartment is left alone).
fn list_devices() -> Result<Vec<DeviceInfo>, BackendError> {
    let worker = std::thread::spawn(|| -> Result<Vec<DeviceInfo>, String> {
        // SAFETY: COM initialised for this thread, used, uninitialised.
        unsafe {
            let hr = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);
            if hr < 0 {
                return Err(format!("CoInitializeEx failed ({hr:#x})"));
            }
            let result = (|| {
                let mut e = null_mut();
                check(
                    CoCreateInstance(
                        &CLSID_MM_DEVICE_ENUMERATOR,
                        null_mut(),
                        CLSCTX_ALL,
                        &IID_IMM_DEVICE_ENUMERATOR,
                        &raw mut e,
                    ),
                    "CoCreateInstance(MMDeviceEnumerator)",
                )?;
                let e = Com(e);
                let default = {
                    let mut d = null_mut();
                    if (vt!(e.0, DeviceEnumeratorVtbl).get_default_audio_endpoint)(
                        e.0, E_RENDER, E_CONSOLE, &raw mut d,
                    ) >= 0
                    {
                        let d = Com(d);
                        device_id(d.0)
                    } else {
                        String::new()
                    }
                };
                let mut c = null_mut();
                check(
                    (vt!(e.0, DeviceEnumeratorVtbl).enum_audio_endpoints)(
                        e.0,
                        E_RENDER,
                        DEVICE_STATE_ACTIVE,
                        &raw mut c,
                    ),
                    "EnumAudioEndpoints",
                )?;
                let c = Com(c);
                let mut n = 0u32;
                check(
                    (vt!(c.0, DeviceCollectionVtbl).get_count)(c.0, &raw mut n),
                    "GetCount",
                )?;
                let mut out = Vec::new();
                for i in 0..n {
                    let mut d = null_mut();
                    if (vt!(c.0, DeviceCollectionVtbl).item)(c.0, i, &raw mut d) < 0 {
                        continue;
                    }
                    let d = Com(d);
                    let id = device_id(d.0);
                    let name = device_name(d.0);
                    out.push(DeviceInfo {
                        is_default: id == default,
                        name: if name.is_empty() { id.clone() } else { name },
                        id,
                        outputs: None,
                        inputs: Some(0),
                    });
                }
                Ok(out)
            })();
            CoUninitialize();
            result
        }
    });
    match worker.join() {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(BackendError::Unavailable(e)),
        Err(_) => Err(BackendError::Unavailable(
            "the WASAPI listing thread panicked".into(),
        )),
    }
}

/// The engine's mix format, as far as low-latency shared mode needs it:
/// 32-bit float, its channel count and rate.
fn mix_is_float(fmt: *const u8) -> Option<(u16, u32)> {
    // SAFETY: a WAVEFORMATEX from GetMixFormat: 18 bytes, plus 22 more when
    // the tag says EXTENSIBLE.
    unsafe {
        let tag = u16::from_le_bytes([*fmt, *fmt.add(1)]);
        let channels = u16::from_le_bytes([*fmt.add(2), *fmt.add(3)]);
        let rate = u32::from_le_bytes([*fmt.add(4), *fmt.add(5), *fmt.add(6), *fmt.add(7)]);
        let bits = u16::from_le_bytes([*fmt.add(14), *fmt.add(15)]);
        let float = match tag {
            WAVE_FORMAT_IEEE_FLOAT => true,
            WAVE_FORMAT_EXTENSIBLE => {
                u32::from_le_bytes([*fmt.add(24), *fmt.add(25), *fmt.add(26), *fmt.add(27)])
                    == SUBTYPE_IEEE_FLOAT
            }
            _ => false,
        };
        (float && bits == 32).then_some((channels, rate))
    }
}

/// Low-latency shared mode (AU-10): `IAudioClient3`, the engine's own
/// format (no auto-conversion), its minimum period. `Some(rate)` when the
/// stream was initialised this way; `None` to fall back to `IAudioClient`.
///
/// # Safety
/// `c` is a fresh, uninitialised `IAudioClient`.
unsafe fn init_low_latency_shared(c: This, ch: u16, block: u32) -> Option<u32> {
    // SAFETY: the caller's contract; every pointer checked, the mix format
    // freed with CoTaskMemFree, the extra interface reference released.
    unsafe {
        let mut c3 = null_mut();
        if (vt!(c, UnknownVtbl).query_interface)(c, &IID_IAUDIO_CLIENT3, &raw mut c3) < 0
            || c3.is_null()
        {
            return None; // before Windows 10
        }
        let c3 = Com(c3);
        let mut mix = null_mut();
        if (vt!(c, AudioClientVtbl).get_mix_format)(c, &raw mut mix) < 0 || mix.is_null() {
            return None;
        }
        let result = (|| {
            let (mix_ch, mix_rate) = mix_is_float(mix.cast::<u8>())?;
            if mix_ch != ch {
                return None; // the shared engine's channels, exactly, or not at all
            }
            let (mut default, mut fundamental, mut min, mut max) = (0u32, 0u32, 0u32, 0u32);
            if (vt!(c3.0, AudioClient3Vtbl).get_shared_mode_engine_period)(
                c3.0,
                mix,
                &raw mut default,
                &raw mut fundamental,
                &raw mut min,
                &raw mut max,
            ) < 0
                || min == 0
            {
                return None;
            }
            // Only worth it when the app asked for blocks no longer than
            // the default period: a large block gains nothing from a short
            // engine period.
            if block > default {
                return None;
            }
            let hr = (vt!(c3.0, AudioClient3Vtbl).initialize_shared_audio_stream)(
                c3.0,
                STREAMFLAGS_EVENTCALLBACK,
                min,
                mix,
                std::ptr::null(),
            );
            (hr >= 0).then_some(mix_rate)
        })();
        CoTaskMemFree(mix);
        result
    }
}

// ── route notifications (AU-09) ─────────────────────────────────────────

/// `IMMNotificationClient`, in declaration order. `PROPERTYKEY` is passed
/// by value in the IDL; on x64 and arm64 a 20-byte struct travels by
/// reference, which is how it is declared here (32-bit x86, where it would
/// be on the stack, is not a target).
#[repr(C)]
struct NotificationClientVtbl {
    unknown: UnknownVtbl,
    on_device_state_changed: unsafe extern "system" fn(This, *const u16, u32) -> Hresult,
    on_device_added: unsafe extern "system" fn(This, *const u16) -> Hresult,
    on_device_removed: unsafe extern "system" fn(This, *const u16) -> Hresult,
    on_default_device_changed: unsafe extern "system" fn(This, i32, i32, *const u16) -> Hresult,
    on_property_value_changed:
        unsafe extern "system" fn(This, *const u16, *const PropertyKey) -> Hresult,
}

const IID_IUNKNOWN: Guid = Guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_IMM_NOTIFICATION_CLIENT: Guid = Guid(
    0x7991_eec9,
    0x7e89,
    0x4d85,
    [0x83, 0x90, 0x6c, 0x70, 0x3c, 0xec, 0x60, 0xc0],
);
const E_NOINTERFACE: Hresult = 0x8000_4002_u32.cast_signed();

/// The COM object: a vtable pointer first, as COM requires.
#[repr(C)]
struct NotificationClient {
    vtbl: *const NotificationClientVtbl,
    refs: std::sync::atomic::AtomicU32,
    wake: super::RouteWake,
}

fn same_guid(a: &Guid, b: &Guid) -> bool {
    a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && a.3 == b.3
}

unsafe extern "system" fn nc_query_interface(
    this: This,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> Hresult {
    // SAFETY: COM passes a valid IID and out-pointer.
    unsafe {
        if out.is_null() {
            return E_NOINTERFACE;
        }
        if same_guid(&*iid, &IID_IUNKNOWN) || same_guid(&*iid, &IID_IMM_NOTIFICATION_CLIENT) {
            nc_add_ref(this);
            *out = this;
            0
        } else {
            *out = null_mut();
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn nc_add_ref(this: This) -> u32 {
    // SAFETY: `this` is a live NotificationClient.
    let c = unsafe { &*this.cast::<NotificationClient>() };
    c.refs.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1
}

unsafe extern "system" fn nc_release(this: This) -> u32 {
    // SAFETY: `this` is a live NotificationClient, freed at zero.
    let left = unsafe { &*this.cast::<NotificationClient>() }
        .refs
        .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
        - 1;
    if left == 0 {
        // SAFETY: the last reference; it was a Box.
        drop(unsafe { Box::from_raw(this.cast::<NotificationClient>()) });
    }
    left
}

fn nc_wake(this: This) {
    // SAFETY: `this` is a live NotificationClient while COM calls it.
    let c = unsafe { &*this.cast::<NotificationClient>() };
    // Never unwind into the MMDevice notification thread.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (c.wake)()));
}

unsafe extern "system" fn nc_state_changed(this: This, _id: *const u16, _state: u32) -> Hresult {
    nc_wake(this);
    0
}

unsafe extern "system" fn nc_added(this: This, _id: *const u16) -> Hresult {
    nc_wake(this);
    0
}

unsafe extern "system" fn nc_removed(this: This, _id: *const u16) -> Hresult {
    nc_wake(this);
    0
}

unsafe extern "system" fn nc_default_changed(
    this: This,
    flow: i32,
    role: i32,
    _id: *const u16,
) -> Hresult {
    // The console render default is what `default_endpoint_id` follows.
    if flow == E_RENDER && role == E_CONSOLE {
        nc_wake(this);
    }
    0
}

unsafe extern "system" fn nc_property_changed(
    _this: This,
    _id: *const u16,
    _key: *const PropertyKey,
) -> Hresult {
    0
}

static NOTIFICATION_CLIENT_VTBL: NotificationClientVtbl = NotificationClientVtbl {
    unknown: UnknownVtbl {
        query_interface: nc_query_interface,
        add_ref: nc_add_ref,
        release: nc_release,
    },
    on_device_state_changed: nc_state_changed,
    on_device_added: nc_added,
    on_device_removed: nc_removed,
    on_default_device_changed: nc_default_changed,
    on_property_value_changed: nc_property_changed,
};

/// The subscription: a thread in the multithreaded apartment that holds
/// the enumerator and the registered client, and unregisters when this is
/// dropped (its channel closes). On a thread of its own because the caller
/// may be a single-threaded apartment (winit's UI thread is), from which
/// the enumerator could not be used again on the supervisor thread.
struct Watch {
    _stop: std::sync::mpsc::Sender<()>,
}

fn watch(wake: super::RouteWake) -> Option<Watch> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("vieww-audio-routes".into())
        .spawn(move || {
            // SAFETY: COM entered and left on this thread; every pointer
            // checked; the client is a Box COM releases through `nc_release`.
            unsafe {
                let hr = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);
                if hr < 0 {
                    let _ = ready_tx.send(false);
                    return;
                }
                let mut e = null_mut();
                if CoCreateInstance(
                    &CLSID_MM_DEVICE_ENUMERATOR,
                    null_mut(),
                    CLSCTX_ALL,
                    &IID_IMM_DEVICE_ENUMERATOR,
                    &raw mut e,
                ) < 0
                {
                    let _ = ready_tx.send(false);
                    CoUninitialize();
                    return;
                }
                let enumerator = Com(e);
                let client: This = Box::into_raw(Box::new(NotificationClient {
                    vtbl: &raw const NOTIFICATION_CLIENT_VTBL,
                    refs: std::sync::atomic::AtomicU32::new(1),
                    wake,
                }))
                .cast();
                let registered = (vt!(enumerator.0, DeviceEnumeratorVtbl)
                    .register_endpoint_notification_callback)(
                    enumerator.0, client
                ) >= 0;
                let _ = ready_tx.send(registered);
                if registered {
                    // Until the guard is dropped.
                    let _ = stop_rx.recv();
                    (vt!(enumerator.0, DeviceEnumeratorVtbl)
                        .unregister_endpoint_notification_callback)(
                        enumerator.0, client
                    );
                }
                nc_release(client);
                drop(enumerator);
                CoUninitialize();
            }
        })
        .ok()?;
    ready_rx.recv().ok()?.then_some(Watch { _stop: stop_tx })
}

/// The default render endpoint's id, from the calling thread (COM joined
/// for the call and left again).
fn default_endpoint_id() -> Option<String> {
    // SAFETY: COM initialised (or found initialised) for the call and the
    // reference released; every pointer checked.
    unsafe {
        let hr = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);
        // S_OK / S_FALSE add a reference to release; RPC_E_CHANGED_MODE
        // (the thread is already single-threaded) adds none but COM works.
        let release = hr >= 0;
        let id = (|| {
            let mut e = null_mut();
            if CoCreateInstance(
                &CLSID_MM_DEVICE_ENUMERATOR,
                null_mut(),
                CLSCTX_ALL,
                &IID_IMM_DEVICE_ENUMERATOR,
                &raw mut e,
            ) < 0
            {
                return None;
            }
            let e = Com(e);
            let mut d = null_mut();
            if (vt!(e.0, DeviceEnumeratorVtbl).get_default_audio_endpoint)(
                e.0, E_RENDER, E_CONSOLE, &raw mut d,
            ) < 0
            {
                return None;
            }
            let d = Com(d);
            Some(device_id(d.0)).filter(|s| !s.is_empty())
        })();
        if release {
            CoUninitialize();
        }
        id
    }
}

unsafe fn session(
    config: StreamConfig,
    capture: bool,
    device: Option<&[u16]>,
    exclusive: bool,
) -> Result<Session, String> {
    // SAFETY: the WASAPI setup sequence, each pointer checked before use.
    unsafe {
        let mut e = null_mut();
        check(
            CoCreateInstance(
                &CLSID_MM_DEVICE_ENUMERATOR,
                null_mut(),
                CLSCTX_ALL,
                &IID_IMM_DEVICE_ENUMERATOR,
                &raw mut e,
            ),
            "CoCreateInstance(MMDeviceEnumerator)",
        )?;
        let enumerator = Com(e);
        let mut d = null_mut();
        match device {
            Some(id) => check(
                (vt!(e, DeviceEnumeratorVtbl).get_device)(e, id.as_ptr(), &raw mut d),
                "GetDevice (no such endpoint)",
            )?,
            None => check(
                (vt!(e, DeviceEnumeratorVtbl).get_default_audio_endpoint)(
                    e,
                    if capture { E_CAPTURE } else { E_RENDER },
                    E_CONSOLE,
                    &raw mut d,
                ),
                "GetDefaultAudioEndpoint",
            )?,
        }
        let device = Com(d);
        let mut c = null_mut();
        check(
            (vt!(d, DeviceVtbl).activate)(
                d,
                &IID_IAUDIO_CLIENT,
                CLSCTX_ALL,
                null_mut(),
                &raw mut c,
            ),
            "Activate(IAudioClient)",
        )?;
        let mut client = Com(c);
        let ch = config.channels.clamp(1, 18);
        let mut format = None;
        let mut device_rate = config.rate;
        if exclusive {
            // The best format at the requested rate, else the nearest rate.
            let supports = |f: PcmFormat, r: u32| {
                let w = wave_format(f, r, ch);
                (vt!(c, AudioClientVtbl).is_format_supported)(
                    c,
                    SHAREMODE_EXCLUSIVE,
                    (&raw const w).cast(),
                    null_mut(),
                ) == 0
            };
            let (f, r) =
                negotiate(config.rate, &PcmFormat::PREFERENCE, supports).ok_or_else(|| {
                    format!(
                        "IsFormatSupported: no exclusive format for {ch} channels at or near {} Hz",
                        config.rate
                    )
                })?;
            // One device period: the endpoint's minimum, or the block
            // asked for if that is longer.
            let (mut default_period, mut min_period) = (0i64, 0i64);
            check(
                (vt!(c, AudioClientVtbl).get_device_period)(
                    c,
                    &raw mut default_period,
                    &raw mut min_period,
                ),
                "GetDevicePeriod",
            )?;
            let want = i64::from(config.block.max(32)) * 10_000_000 / i64::from(r.max(1));
            let mut hns = want.max(min_period).max(1);
            let fmt = wave_format(f, r, ch);
            let mut hr = (vt!(c, AudioClientVtbl).initialize)(
                client.0,
                SHAREMODE_EXCLUSIVE,
                STREAMFLAGS_EVENTCALLBACK,
                hns,
                hns,
                &raw const fmt,
                std::ptr::null(),
            );
            if hr == E_BUFFER_SIZE_NOT_ALIGNED {
                // The documented dance: read the aligned frame count, take
                // a fresh client, initialise with exactly that period.
                let mut frames = 0u32;
                check(
                    (vt!(client.0, AudioClientVtbl).get_buffer_size)(client.0, &raw mut frames),
                    "GetBufferSize (alignment)",
                )?;
                hns = (10_000_000 * i64::from(frames) + i64::from(r) / 2) / i64::from(r.max(1));
                drop(client);
                let mut c2 = null_mut();
                check(
                    (vt!(d, DeviceVtbl).activate)(
                        d,
                        &IID_IAUDIO_CLIENT,
                        CLSCTX_ALL,
                        null_mut(),
                        &raw mut c2,
                    ),
                    "Activate(IAudioClient) after alignment",
                )?;
                client = Com(c2);
                hr = (vt!(c2, AudioClientVtbl).initialize)(
                    c2,
                    SHAREMODE_EXCLUSIVE,
                    STREAMFLAGS_EVENTCALLBACK,
                    hns,
                    hns,
                    &raw const fmt,
                    std::ptr::null(),
                );
            }
            if hr == E_DEVICE_IN_USE {
                return Err(
                    "IAudioClient::Initialize: the endpoint is in use by another exclusive stream"
                        .into(),
                );
            }
            check(hr, "IAudioClient::Initialize (exclusive)")?;
            format = Some(wire_format(f));
            device_rate = r;
        } else if let Some(mix_rate) = init_low_latency_shared(c, ch, config.block.max(1)) {
            // AU-10: low-latency shared mode at the engine's minimum period.
            // Its format is the mix format: float at the engine's rate, so
            // at another rate than ours the conversion path (as exclusive
            // mode at a negotiated rate) resamples.
            device_rate = mix_rate;
            if mix_rate != config.rate {
                format = Some(PcmFormat::F32);
            }
        } else {
            let fmt = wave_format(PcmFormat::F32, config.rate, ch);
            // Buffer: three blocks, in 100 ns units.
            let hns =
                i64::from(config.block.max(64)) * 3 * 10_000_000 / i64::from(config.rate.max(1));
            check(
                (vt!(c, AudioClientVtbl).initialize)(
                    c,
                    SHAREMODE_SHARED,
                    STREAMFLAGS_EVENTCALLBACK
                        | STREAMFLAGS_AUTOCONVERTPCM
                        | STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    hns,
                    0,
                    &raw const fmt,
                    std::ptr::null(),
                ),
                "IAudioClient::Initialize",
            )?;
        }
        let c = client.0;
        let event = CreateEventW(null_mut(), 0, 0, std::ptr::null());
        if event.is_null() {
            return Err("CreateEventW failed".into());
        }
        let mut s = Session {
            _enumerator: enumerator,
            _device: device,
            client,
            service: Com(null_mut()),
            event,
            buffer_frames: 0,
            latency_ns: 0,
            format,
            device_rate,
        };
        check(
            (vt!(c, AudioClientVtbl).set_event_handle)(c, event),
            "SetEventHandle",
        )?;
        check(
            (vt!(c, AudioClientVtbl).get_buffer_size)(c, &raw mut s.buffer_frames),
            "GetBufferSize",
        )?;
        // The engine's own latency (100 ns units) plus our buffer: what a
        // sample written now waits before it is heard.
        let mut hns_latency = 0i64;
        if (vt!(c, AudioClientVtbl).get_stream_latency)(c, &raw mut hns_latency) >= 0 {
            let engine = u64::try_from(hns_latency).unwrap_or(0).saturating_mul(100);
            let buffer = u64::from(s.buffer_frames).saturating_mul(1_000_000_000)
                / u64::from(device_rate.max(1));
            s.latency_ns = engine.saturating_add(buffer);
        }
        let mut svc = null_mut();
        let iid = if capture {
            &IID_IAUDIO_CAPTURE_CLIENT
        } else {
            &IID_IAUDIO_RENDER_CLIENT
        };
        check(
            (vt!(c, AudioClientVtbl).get_service)(c, iid, &raw mut svc),
            "GetService",
        )?;
        s.service = Com(svc);
        Ok(s)
    }
}

/// Per-stream scratch for exclusive mode: float staging, the converter
/// and its FIFO, and dither — all sized before the stream starts.
struct Exclusive {
    fmt: PcmFormat,
    scratch: Vec<f32>,
    conv: Option<Resampler>,
    fifo: Vec<f32>,
    block: Vec<f32>,
    dither: crate::wav::Dither,
}

impl Exclusive {
    fn new(
        fmt: PcmFormat,
        buffer_frames: u32,
        ch: usize,
        block: usize,
        from: u32,
        to: u32,
        capture: bool,
    ) -> Self {
        let buf = buffer_frames as usize * ch;
        let (a, b) = if capture { (to, from) } else { (from, to) };
        let conv = (a != b).then(|| Resampler::new(a, b, ch, Quality::Sinc(16)));
        let ratio = (b / a.max(1)) as usize + 2;
        Self {
            fmt,
            scratch: vec![0.0; buf],
            conv,
            fifo: Vec::with_capacity((buf + block * ch) * ratio * 2 + 256),
            block: vec![0.0; block * ch],
            dither: crate::wav::Dither::new(0x5EED_A15A),
        }
    }
}

#[expect(clippy::too_many_lines, reason = "one COM thread, setup to teardown")]
fn open(
    config: StreamConfig,
    callback: Callback,
    device: Option<&str>,
    exclusive: bool,
) -> Result<DeviceStream, BackendError> {
    let device: Option<Vec<u16>> = device.map(|d| d.encode_utf16().chain(Some(0)).collect());
    let capture = matches!(callback, Callback::Capture(_));
    let counters = Arc::new(Counters::default());
    // The render loop lives on a plain thread here rather than an FFI
    // callback, but a panicking closure still kills that thread silently —
    // the stream stalls with `ended` never set and `wait_for` spins to its
    // timeout. Isolating the callback gives it the same contract as every
    // other backend: one caught panic, then silence and `ended`.
    let callback = match callback {
        Callback::Render(r) => Callback::Render(isolate_render(r, &counters)),
        Callback::Capture(c) => Callback::Capture(isolate_capture(c, &counters)),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Result<u32, String>>();
    let (c2, s2) = (counters.clone(), stop.clone());
    let ch = usize::from(config.channels.clamp(1, 18));
    let rate = config.rate;
    let block_frames = config.block.max(32) as usize;
    let thread = std::thread::Builder::new()
        .name("vieww-wasapi".into())
        .spawn(move || {
            // MMCSS "Pro Audio" — the class Windows' own audio engine uses.
            let _rt = crate::rt::promote_audio_thread(std::time::Duration::from_millis(10));
            // SAFETY: COM initialised for this thread and uninitialised at
            // its end; every object lives inside `session`.
            unsafe {
                let hr = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);
                if hr < 0 {
                    tx.send(Err(format!("CoInitializeEx failed ({hr:#x})")))
                        .ok();
                    return;
                }
                let sess = match session(config, capture, device.as_deref(), exclusive) {
                    Ok(s) => s,
                    Err(e) => {
                        tx.send(Err(e)).ok();
                        CoUninitialize();
                        return;
                    }
                };
                let c = sess.client.0;
                let svc = sess.service.0;
                let mut callback = callback;
                let mut ex = sess.format.map(|f| {
                    Exclusive::new(
                        f,
                        sess.buffer_frames,
                        ch,
                        block_frames,
                        rate,
                        sess.device_rate,
                        capture,
                    )
                });
                let mut done = 0u64;
                if (vt!(c, AudioClientVtbl).start)(c) < 0 {
                    tx.send(Err("IAudioClient::Start failed".into())).ok();
                    drop(sess);
                    CoUninitialize();
                    return;
                }
                c2.latency_ns.store(sess.latency_ns, Ordering::Relaxed);
                c2.buffer_frames
                    .store(u64::from(sess.buffer_frames), Ordering::Relaxed);
                c2.exclusive.store(exclusive, Ordering::Relaxed);

                tx.send(Ok(sess.buffer_frames)).ok();
                while !s2.load(Ordering::Acquire) {
                    if WaitForSingleObject(sess.event, 2_000) != WAIT_OBJECT_0 {
                        c2.xruns.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    // `time` is derived from the running frame count, so it
                    // is computed per callback — one wakeup can drain many
                    // capture packets, and stamping them all with the first
                    // packet's time froze timestamps in chunks.
                    match &mut callback {
                        Callback::Render(r) => {
                            let mut pad = 0u32;
                            if (vt!(c, AudioClientVtbl).get_current_padding)(c, &raw mut pad) < 0 {
                                c2.ended.store(true, Ordering::Release);
                                break;
                            }
                            let avail = sess.buffer_frames.saturating_sub(pad);
                            if avail == 0 {
                                continue;
                            }
                            let mut data = null_mut();
                            if (vt!(svc, RenderClientVtbl).get_buffer)(svc, avail, &raw mut data)
                                < 0
                                || data.is_null()
                            {
                                c2.ended.store(true, Ordering::Release);
                                break;
                            }
                            let n = avail as usize;
                            match &mut ex {
                                // Shared: the engine takes our floats in place.
                                None => {
                                    #[expect(clippy::cast_precision_loss, reason = "stream time")]
                                    let time = done as f64 / f64::from(rate);
                                    let out =
                                        std::slice::from_raw_parts_mut(data.cast::<f32>(), n * ch);
                                    out.fill(0.0);
                                    r(
                                        out,
                                        &BlockInfo {
                                            frames: n,
                                            channels: ch,
                                            rate,
                                            time,
                                        },
                                    );
                                    done += n as u64;
                                    c2.block(n);
                                }
                                // Exclusive at the requested rate: render,
                                // encode into the endpoint's buffer.
                                Some(x) if x.conv.is_none() => {
                                    #[expect(clippy::cast_precision_loss, reason = "stream time")]
                                    let time = done as f64 / f64::from(rate);
                                    let f = &mut x.scratch[..n * ch];
                                    f.fill(0.0);
                                    r(
                                        f,
                                        &BlockInfo {
                                            frames: n,
                                            channels: ch,
                                            rate,
                                            time,
                                        },
                                    );
                                    let bytes = std::slice::from_raw_parts_mut(
                                        data,
                                        n * ch * x.fmt.bytes(),
                                    );
                                    encode_samples(f, x.fmt, bytes, &mut x.dither);
                                    done += n as u64;
                                    c2.block(n);
                                }
                                // Exclusive at another rate: render blocks at
                                // ours, convert into the FIFO until a buffer's
                                // worth is there.
                                Some(x) => {
                                    let need = n * ch;
                                    while x.fifo.len() < need {
                                        #[expect(
                                            clippy::cast_precision_loss,
                                            reason = "stream time"
                                        )]
                                        let time = done as f64 / f64::from(rate);
                                        x.block.fill(0.0);
                                        r(
                                            &mut x.block,
                                            &BlockInfo {
                                                frames: block_frames,
                                                channels: ch,
                                                rate,
                                                time,
                                            },
                                        );
                                        if let Some(cv) = &mut x.conv {
                                            cv.process(&x.block, &mut x.fifo);
                                        }
                                        done += block_frames as u64;
                                        c2.block(block_frames);
                                    }
                                    let bytes =
                                        std::slice::from_raw_parts_mut(data, need * x.fmt.bytes());
                                    encode_samples(&x.fifo[..need], x.fmt, bytes, &mut x.dither);
                                    x.fifo.drain(..need);
                                }
                            }
                            (vt!(svc, RenderClientVtbl).release_buffer)(svc, avail, 0);
                        }
                        Callback::Capture(cap) => loop {
                            let mut packet = 0u32;
                            if (vt!(svc, CaptureClientVtbl).get_next_packet_size)(
                                svc,
                                &raw mut packet,
                            ) < 0
                            {
                                c2.ended.store(true, Ordering::Release);
                                break;
                            }
                            if packet == 0 {
                                break;
                            }
                            let (mut data, mut frames, mut flags) = (null_mut(), 0u32, 0u32);
                            if (vt!(svc, CaptureClientVtbl).get_buffer)(
                                svc,
                                &raw mut data,
                                &raw mut frames,
                                &raw mut flags,
                                null_mut(),
                                null_mut(),
                            ) < 0
                            {
                                c2.ended.store(true, Ordering::Release);
                                break;
                            }
                            let n = frames as usize;
                            let silent = flags & BUFFERFLAGS_SILENT != 0 || data.is_null();
                            match &mut ex {
                                None if silent => {
                                    let zeros = [0.0f32; 4_096];
                                    let mut left = n * ch;
                                    while left > 0 {
                                        #[expect(
                                            clippy::cast_precision_loss,
                                            reason = "stream time"
                                        )]
                                        let time = done as f64 / f64::from(rate);
                                        let k = left.min(zeros.len() - zeros.len() % ch);
                                        cap(
                                            &zeros[..k],
                                            &BlockInfo {
                                                frames: k / ch,
                                                channels: ch,
                                                rate,
                                                time,
                                            },
                                        );
                                        done += (k / ch) as u64;
                                        left -= k;
                                    }
                                    c2.block(n);
                                }
                                None => {
                                    #[expect(clippy::cast_precision_loss, reason = "stream time")]
                                    let time = done as f64 / f64::from(rate);
                                    let input =
                                        std::slice::from_raw_parts(data.cast::<f32>(), n * ch);
                                    cap(
                                        input,
                                        &BlockInfo {
                                            frames: n,
                                            channels: ch,
                                            rate,
                                            time,
                                        },
                                    );
                                    done += n as u64;
                                    c2.block(n);
                                }
                                Some(x) => {
                                    // Decode (or zero) into staging, packet
                                    // by staging-sized piece.
                                    let mut off = 0;
                                    while off < n {
                                        let k = (n - off).min(x.scratch.len() / ch);
                                        let f = &mut x.scratch[..k * ch];
                                        if silent {
                                            f.fill(0.0);
                                        } else {
                                            let w = ch * x.fmt.bytes();
                                            let bytes = std::slice::from_raw_parts(
                                                data.add(off * w),
                                                k * w,
                                            );
                                            decode_samples(bytes, x.fmt, f);
                                        }
                                        match &mut x.conv {
                                            None => {
                                                #[expect(
                                                    clippy::cast_precision_loss,
                                                    reason = "stream time"
                                                )]
                                                let time = done as f64 / f64::from(rate);
                                                cap(
                                                    f,
                                                    &BlockInfo {
                                                        frames: k,
                                                        channels: ch,
                                                        rate,
                                                        time,
                                                    },
                                                );
                                                done += k as u64;
                                                c2.block(k);
                                            }
                                            Some(cv) => {
                                                cv.process(f, &mut x.fifo);
                                                let bn = block_frames * ch;
                                                while x.fifo.len() >= bn {
                                                    #[expect(
                                                        clippy::cast_precision_loss,
                                                        reason = "stream time"
                                                    )]
                                                    let time = done as f64 / f64::from(rate);
                                                    x.block.copy_from_slice(&x.fifo[..bn]);
                                                    x.fifo.drain(..bn);
                                                    cap(
                                                        &x.block,
                                                        &BlockInfo {
                                                            frames: block_frames,
                                                            channels: ch,
                                                            rate,
                                                            time,
                                                        },
                                                    );
                                                    done += block_frames as u64;
                                                    c2.block(block_frames);
                                                }
                                            }
                                        }
                                        off += k;
                                    }
                                }
                            }
                            (vt!(svc, CaptureClientVtbl).release_buffer)(svc, frames);
                        },
                    }
                }
                drop(sess);
                CoUninitialize();
            }
        })
        .map_err(|e| BackendError::Refused(e.to_string()))?;
    match rx.recv() {
        Ok(Ok(buffer)) => Ok(DeviceStream::new(
            if exclusive {
                "wasapi-exclusive"
            } else {
                "wasapi"
            },
            StreamConfig {
                rate: config.rate,
                channels: config.channels.clamp(1, 18),
                block: if exclusive { buffer } else { buffer / 3 },
            },
            counters,
            Box::new(WasapiStream {
                stop,
                thread: Some(thread),
            }),
        )),
        Ok(Err(e)) => {
            thread.join().ok();
            // No endpoint at all (a server with no audio device) is
            // "unavailable"; anything later is a refusal.
            if e.contains("GetDefaultAudioEndpoint") || e.contains("CoCreateInstance") {
                Err(BackendError::Unavailable(e))
            } else {
                // A named endpoint that does not exist ("GetDevice") is a
                // refusal too: never a silent fall-back to the default.
                Err(BackendError::Refused(e))
            }
        }
        Err(_) => Err(BackendError::Refused("the WASAPI thread ended".into())),
    }
}

/// The WASAPI backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct Wasapi;

impl Backend for Wasapi {
    fn name(&self) -> &'static str {
        "wasapi"
    }

    fn open_output(
        &self,
        config: StreamConfig,
        render: RenderFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Render(render), None, false)
    }

    fn open_input(
        &self,
        config: StreamConfig,
        capture: CaptureFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Capture(capture), None, false)
    }

    /// The active render endpoints with their friendly names; ids are
    /// endpoint id strings (`{0.0.0.00000000}.{…}`).
    fn devices(&self) -> Result<Vec<DeviceInfo>, BackendError> {
        list_devices()
    }

    /// WASAPI streams stay on the endpoint they opened; following a new
    /// default (headphones plugged in, an output picked in Settings) is
    /// the supervisor's job, and this is what it watches.
    fn default_device_id(&self) -> Option<String> {
        default_endpoint_id()
    }

    /// `IMMNotificationClient` (AU-09): default and device changes wake the
    /// supervisor at once.
    fn watch(&self, wake: super::RouteWake) -> Option<super::RouteWatch> {
        watch(wake).map(|w| Box::new(w) as super::RouteWatch)
    }

    fn open_output_on(
        &self,
        device: &str,
        config: StreamConfig,
        render: RenderFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Render(render), Some(device), false)
    }
}

/// WASAPI in exclusive mode: the endpoint to this stream alone, at its
/// native format and a one-period buffer (see the module docs).
#[derive(Debug, Clone, Copy, Default)]
pub struct WasapiExclusive;

impl Backend for WasapiExclusive {
    fn name(&self) -> &'static str {
        "wasapi-exclusive"
    }

    fn open_output(
        &self,
        config: StreamConfig,
        render: RenderFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Render(render), None, true)
    }

    fn open_input(
        &self,
        config: StreamConfig,
        capture: CaptureFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Capture(capture), None, true)
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, BackendError> {
        list_devices()
    }

    fn default_device_id(&self) -> Option<String> {
        default_endpoint_id()
    }

    fn open_output_on(
        &self,
        device: &str,
        config: StreamConfig,
        render: RenderFn,
    ) -> Result<DeviceStream, BackendError> {
        open(config, Callback::Render(render), Some(device), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AU-10: the mix format is read as low-latency shared mode needs it.
    #[test]
    fn mix_format_float_detection() {
        // WAVEFORMATEXTENSIBLE: float, 2 channels, 48 kHz, 32 bits.
        let mut ext = [0u8; 40];
        ext[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes());
        ext[2..4].copy_from_slice(&2u16.to_le_bytes());
        ext[4..8].copy_from_slice(&48_000u32.to_le_bytes());
        ext[14..16].copy_from_slice(&32u16.to_le_bytes());
        ext[24..28].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(mix_is_float(ext.as_ptr()), Some((2, 48_000)));
        ext[24..28].copy_from_slice(&1u32.to_le_bytes()); // PCM sub-format
        assert_eq!(mix_is_float(ext.as_ptr()), None);
        // Plain WAVE_FORMAT_IEEE_FLOAT.
        let mut plain = [0u8; 18];
        plain[0..2].copy_from_slice(&3u16.to_le_bytes());
        plain[2..4].copy_from_slice(&1u16.to_le_bytes());
        plain[4..8].copy_from_slice(&44_100u32.to_le_bytes());
        plain[14..16].copy_from_slice(&32u16.to_le_bytes());
        assert_eq!(mix_is_float(plain.as_ptr()), Some((1, 44_100)));
    }

    /// AU-10: a small-block shared stream opens (through `IAudioClient3`
    /// where the endpoint offers it, else `IAudioClient`) and reports its
    /// buffer and latency. Skipped where the machine has no render
    /// endpoint.
    #[cfg(windows)]
    #[test]
    fn a_small_block_shared_stream_reports_buffer_and_latency() {
        let config = StreamConfig {
            rate: 48_000,
            channels: 2,
            block: 128,
        };
        let stream = match Wasapi.open_output(config, Box::new(|out, _| out.fill(0.0))) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("no render endpoint here ({e}); skipped");
                return;
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        let st = stream.stats();
        eprintln!(
            "wasapi shared: buffer {:?} frames, latency {:?}, callbacks {}",
            st.buffer_frames, st.latency, st.callbacks
        );
        assert!(st.buffer_frames.is_some(), "the buffer is reported");
        assert!(st.callbacks > 0, "it runs");
        assert!(!st.exclusive);
    }

    /// AU-09: the endpoint notification client registers with the real
    /// MMDevice enumerator and unregisters when the guard drops.
    #[cfg(windows)]
    #[test]
    fn endpoint_notifications_register_and_unregister() {
        let watch = Wasapi.watch(Arc::new(|| {}));
        assert!(
            watch.is_some(),
            "RegisterEndpointNotificationCallback refused"
        );
        drop(watch);
    }

    #[test]
    fn the_notification_client_answers_its_interfaces_only() {
        let client: This = Box::into_raw(Box::new(NotificationClient {
            vtbl: &raw const NOTIFICATION_CLIENT_VTBL,
            refs: std::sync::atomic::AtomicU32::new(1),
            wake: Arc::new(|| {}),
        }))
        .cast();
        // SAFETY: a live client; references balanced to free it.
        unsafe {
            let mut out = null_mut();
            assert_eq!(
                nc_query_interface(client, &IID_IMM_NOTIFICATION_CLIENT, &raw mut out),
                0
            );
            assert_eq!(out, client);
            assert_eq!(
                nc_query_interface(client, &IID_IAUDIO_CLIENT, &raw mut out),
                E_NOINTERFACE
            );
            assert!(out.is_null());
            assert_eq!(nc_release(client), 1, "the reference QueryInterface added");
            assert_eq!(nc_release(client), 0, "freed");
        }
    }

    #[test]
    fn the_format_struct_has_the_sdk_layout() {
        assert_eq!(size_of::<WaveFormatExtensible>(), 40);
        assert_eq!(std::mem::offset_of!(WaveFormatExtensible, cb_size), 16);
        assert_eq!(std::mem::offset_of!(WaveFormatExtensible, channel_mask), 20);
        assert_eq!(std::mem::offset_of!(WaveFormatExtensible, sub_format), 24);
        assert_eq!(size_of::<Guid>(), 16);
        assert_eq!(channel_mask(2), 3);
        assert_eq!(channel_mask(6), 0x3F);
    }

    #[test]
    fn exclusive_formats_describe_what_is_written() {
        let f = wave_format(PcmFormat::S24, 96_000, 2);
        let (bits, valid, align, rate) = (
            f.bits_per_sample,
            f.valid_bits_per_sample,
            f.block_align,
            f.avg_bytes_per_sec,
        );
        assert_eq!((bits, valid, align, rate), (32, 24, 8, 768_000));
        // 24-in-32 is MSB-aligned on Windows: written as full 32-bit words.
        assert_eq!(wire_format(PcmFormat::S24), PcmFormat::S32);
        let p = wave_format(PcmFormat::S24Packed, 48_000, 2);
        let (bits, align) = (p.bits_per_sample, p.block_align);
        assert_eq!((bits, align), (24, 6));
        let fl = wave_format(PcmFormat::F32, 48_000, 6);
        let sub = fl.sub_format;
        assert_eq!(sub.0, 3, "IEEE float subtype");
        let i = wave_format(PcmFormat::S16, 44_100, 1);
        let (sub, align, mask) = (i.sub_format, i.block_align, i.channel_mask);
        assert_eq!((sub.0, align, mask), (1, 2, 4));
        assert_eq!(E_BUFFER_SIZE_NOT_ALIGNED as u32, 0x8889_0019);
    }

    #[test]
    fn the_property_structs_have_the_sdk_layout() {
        assert_eq!(size_of::<PropertyKey>(), 20);
        assert_eq!(std::mem::offset_of!(PropVariant, pwsz), 8);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(size_of::<PropVariant>(), 24);
        // Vtable slots: IMMDevice::GetId is the 6th method (3 + Activate,
        // OpenPropertyStore, GetId); IMMDeviceEnumerator::GetDevice the 6th.
        let p = size_of::<usize>();
        assert_eq!(std::mem::offset_of!(DeviceVtbl, get_id), 5 * p);
        assert_eq!(
            std::mem::offset_of!(DeviceEnumeratorVtbl, get_device),
            5 * p
        );
        assert_eq!(std::mem::offset_of!(PropertyStoreVtbl, get_value), 5 * p);
        assert_eq!(std::mem::offset_of!(DeviceCollectionVtbl, item), 4 * p);
    }

    #[test]
    #[cfg(windows)]
    fn lists_endpoints_and_opens_one_by_id_or_refuses_cleanly() {
        match Wasapi.devices() {
            Ok(list) => {
                if let Some(d) = list.first() {
                    assert!(!d.id.is_empty());
                    let s = Wasapi.open_output_on(
                        &d.id,
                        StreamConfig::default(),
                        Box::new(|b, _| b.fill(0.0)),
                    );
                    if let Ok(s) = s {
                        assert!(s.wait_for(5, std::time::Duration::from_secs(10)));
                        assert!(s.latency().is_some(), "WASAPI reports its latency");
                    }
                }
                assert!(Wasapi
                    .open_output_on(
                        "{no-such-endpoint}",
                        StreamConfig::default(),
                        Box::new(|_, _| {})
                    )
                    .is_err());
            }
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }

    #[test]
    #[cfg(windows)]
    fn plays_on_the_default_endpoint_or_refuses_cleanly() {
        // CI runners usually have no audio endpoint: then the refusal is
        // the check.
        match Wasapi.open_output(StreamConfig::default(), Box::new(|b, _| b.fill(0.0))) {
            Ok(s) => assert!(s.wait_for(5, std::time::Duration::from_secs(10))),
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}
