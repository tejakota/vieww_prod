//! The platform escape hatch: decode through the OS what pure Rust refuses.
//!
//! Every decoder in this crate is hand-rolled, and two of them refuse part
//! of their format *by name* — the Opus SILK/CELT payload
//! ([`opus::REFUSED_PAYLOAD`](super::opus::REFUSED_PAYLOAD)) and AAC's
//! object types beyond LC — because writing them correctly is a project,
//! and guessing is worse than refusing. But Android and Apple devices
//! ship decoders for exactly those: `MediaCodec` (Android 10+ for Opus)
//! and `ExtAudioFile`/`AudioConverter` (iOS 11+/macOS 10.13+ for Opus),
//! licensed by the OS vendor. This module is the bridge: an app installs
//! one [`PlatformCodec`] at startup and calls [`decode`] instead of
//! [`codec::decode`](super::decode); when the pure decoder refuses with
//! [`CodecError::Unsupported`], the platform gets the bytes, and the
//! pure side's own container arithmetic finishes the job.
//!
//! What each side owns, so a decode is never double-processed:
//!
//! * **The platform owns the payload and its priming.** It must return
//!   the *playable* stream — encoder lookahead already trimmed (Android's
//!   Opus decoder trims the pre-skip from the `csd-0` the Ogg extractor
//!   hands it; an implementation that cannot trim must say so by
//!   refusing, not by returning untrimmed bytes).
//! * **The pure side owns the container's own arithmetic**, which no
//!   platform applies: the Ogg-Opus output gain (RFC 7845 §5.1.1 puts it
//!   on the *container layer*, and MediaCodec does not) is applied here
//!   from the parsed [`Head`](super::opus::Head) after a platform decode.
//! * A decode the platform returns goes through
//!   [`Decoded::validated`](super::Decoded::validated) like any other:
//!   shape limits and runaway-sample concealment are the crate's contract
//!   with its mixer, whatever decoded the bytes.
//!
//! The shipped implementations:
//!
//! * Android — [`MediaCodec`]: the NDK `AMediaExtractor` +
//!   `AMediaCodec` pipeline, `libmediandk.so` loaded at run time exactly
//!   as `backend::aaudio` loads `libaaudio.so` (an APK on an older device
//!   reports the bridge unavailable instead of failing to start). The
//!   bytes are spooled into a `memfd` — no file on disk, no permission
//!   asked — and handed to the extractor by fd.
//! * macOS/iOS — [`ExtAudioFile`]: the container readers plus
//!   `AudioConverter`, asked for interleaved float32. `ExtAudioFile`
//!   opens by URL, so the bytes are spooled to the sandbox's temporary
//!   directory first.
//!
//! Both are written against the published C headers and type-checked on
//! their targets (`cargo check -p vieww-audio --target
//! aarch64-linux-android`, `… aarch64-apple-darwin`); neither has run on
//! real hardware in CI — the device farm's audio step owns that
//! verification (AU-14). On every other platform [`system`] is `None`;
//! an app that needs the bridge implements the trait itself, which is
//! what the in-process tests here do.
//!
//! The patent story rides the bridge too: an app that turns the `aac`
//! feature off and decodes AAC through the platform puts the OS vendor's
//! licence in charge (see `PATENTS.md`, which described exactly this
//! bridge as a follow-up — this is it).

use std::sync::RwLock;

use super::{CodecError, Decoded, Format};

/// A decoder the operating system provides, consulted when the pure-Rust
/// decoders refuse.
///
/// Implementations must be `Send + Sync`: they are stored globally and
/// called from whatever thread decodes. The contract in the module docs
/// (the platform trims priming and never applies the Ogg-Opus output
/// gain) is part of the trait, not of any one implementation.
pub trait PlatformCodec: Send + Sync + 'static {
    /// The implementation's name, for diagnostics ("media-codec",
    /// "ext-audio-file" — or an app's own).
    fn name(&self) -> &'static str;

    /// Decode the whole byte stream. `mime` is what the pure layer learned
    /// from the container before refusing (`"audio/opus"`,
    /// `"audio/mp4a-latm"`, `"audio/aac"`); an implementation that does
    /// not recognise one should refuse it, not guess.
    ///
    /// # Errors
    /// Whatever the platform said, as a string — it becomes
    /// [`CodecError::Io`], the "reading the stream failed" arm: a
    /// platform refusal during decode is an I/O-shaped failure the
    /// caller can only report.
    fn decode(&self, bytes: &[u8], mime: &str) -> Result<Decoded, String>;
}

/// Where the installed codec lives. `install`/`uninstall` are
/// app-startup (and test) operations; `decode` takes the read lock per
/// call and releases it before any post-processing.
static SLOT: RwLock<Option<Box<dyn PlatformCodec>>> = RwLock::new(None);

/// Install `codec` as the platform decoder, replacing any previous one.
///
/// Call once at startup, before any decode:
///
/// ```ignore
/// #[cfg(target_os = "android")]
/// vieww_audio::codec::platform::install(
///     vieww_audio::codec::platform::system().unwrap());
/// ```
pub fn install(codec: Box<dyn PlatformCodec>) {
    if let Ok(mut slot) = SLOT.write() {
        *slot = Some(codec);
    }
}

/// Remove the installed codec; whether there was one. Tests use this to
/// prove the pure refusal comes back when nothing is installed.
pub fn uninstall() -> bool {
    SLOT.write().ok().and_then(|mut s| s.take()).is_some()
}

/// Whether a platform codec is installed.
#[must_use]
pub fn is_installed() -> bool {
    SLOT.read().is_ok_and(|s| s.is_some())
}

/// The OS decoder this crate ships for the platform, if there is one:
/// MediaCodec on Android, `ExtAudioFile` on macOS/iOS, `None` elsewhere
/// (implement [`PlatformCodec`] and [`install`] it).
#[must_use]
pub fn system() -> Option<Box<dyn PlatformCodec>> {
    #[cfg(target_os = "android")]
    return Some(Box::new(media_codec::MediaCodec::new()?));
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return Some(Box::new(ext_audio_file::ExtAudioFile));
    #[cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios")))]
    return None;
}

/// The mime type the pure layer hands a platform for `bytes`, when it
/// knows one. Only refusals that map to a platform mime consult the
/// platform: Opus's payload, and AAC (ADTS or MP4) — the object types the
/// pure decoder refuses by name, including every AAC kind while the
/// `aac` feature is off. Anything else — a container this crate does not
/// sniff, an MP4 track that is not `mp4a`/`alac` — keeps its own refusal:
/// the platform cannot be told what it is being handed.
fn handoff_mime(bytes: &[u8]) -> Option<&'static str> {
    match super::sniff(bytes)? {
        Format::Ogg if super::opus::is_opus(bytes) => Some("audio/opus"),
        Format::Adts => Some("audio/aac"),
        Format::Mp4 => super::mp4::audio_track(bytes)
            .ok()
            .filter(|t| t.codec == *b"mp4a")
            .map(|_| "audio/mp4a-latm"),
        _ => None,
    }
}

/// Decode any supported format, falling back to the installed
/// [`PlatformCodec`] exactly when the pure decoder refuses by name.
///
/// The order is deliberate: the pure path first (it is what is fuzzed and
/// graded), the platform only on [`CodecError::Unsupported`], so a format
/// the pure side decodes never depends on the OS. With nothing installed
/// the pure refusal comes back unchanged — the escape hatch changes
/// nothing until it is opened.
///
/// # Errors
/// [`super::decode`]'s, or the platform's refusal as
/// [`CodecError::Io`], or [`CodecError::Malformed`] if what the platform
/// returned is not a shape this crate's mixer can use.
pub fn decode(bytes: &[u8]) -> Result<Decoded, CodecError> {
    match super::decode(bytes) {
        Ok(d) => Ok(d),
        // Only an Unsupported is a hand-off: Malformed and Truncated are
        // the file's own words, and a checksum failure would be papered
        // over by a platform that is less careful.
        Err(CodecError::Unsupported(what)) => hand_off(bytes, what),
        Err(e) => Err(e),
    }
}

/// Consult the installed codec for `bytes` and finish its output the
/// pure way: the Opus output gain, then validation.
fn hand_off(bytes: &[u8], refused: &'static str) -> Result<Decoded, CodecError> {
    let Some(mime) = handoff_mime(bytes) else {
        return Err(CodecError::Unsupported(refused));
    };
    let decoded = {
        let Ok(slot) = SLOT.read() else {
            return Err(CodecError::Malformed("platform codec lock poisoned"));
        };
        let Some(codec) = slot.as_ref() else {
            return Err(CodecError::Unsupported(refused));
        };
        codec.decode(bytes, mime).map_err(CodecError::Io)?
    };
    let mut d = decoded;
    if mime == "audio/opus" {
        // RFC 7845 §5.1.1: the container layer applies the output gain.
        // MediaCodec and ExtAudioFile do not — this is the container layer.
        if let Ok(s) = super::opus::parse(bytes) {
            if s.head.output_gain != 0 {
                let g = s.head.gain_factor();
                for v in &mut d.data {
                    *v *= g;
                }
            }
        }
    }
    d.validated()
}

/// The same bridge for the streaming side ([`super::stream`]): the
/// container is parsed and validated by the pure code, the payload is
/// handed to the installed codec whole, and the result is served a chunk
/// at a time. Refused by name when nothing is installed.
pub(crate) fn decode_chunked(bytes: &[u8]) -> Result<(Decoded, Option<u64>), CodecError> {
    let refused = super::opus::REFUSED_PAYLOAD;
    let s = super::opus::parse(bytes)?;
    let d = hand_off(bytes, refused)?;
    // The stream's own length: the final granule less the pre-skip — the
    // platform's decode is trimmed, so this is a cross-check the caller
    // (and the tests) can hold it to.
    let total = s.total_pcm;
    Ok((d, total))
}

// ── the shipped implementations ──────────────────────────────────────────

/// Android's `MediaCodec`: the NDK media pipeline in `libmediandk.so`.
///
/// `AMediaExtractor` sniffs the container and finds the audio track
/// (which is why the trait's `mime` is advisory here — the extractor
/// re-derives it and refuses what it does not know); `AMediaCodec`
/// decodes into 16-bit PCM, which this side converts to `f32`. The input
/// bytes are written into a `memfd` so nothing touches the app's storage.
///
/// Written against `<media/NdkMediaExtractor.h>` / `<media/NdkMediaCodec.h>`
/// (API 21+, Opus decode on API 29+); loaded at run time so an APK that
/// runs on anything older still starts, exactly as `backend::aaudio`
/// loads `libaaudio.so`. Compile-checked for `aarch64-linux-android`;
/// never run on a device in CI — AU-14's farm step is where that receipt
/// comes from.
#[cfg(target_os = "android")]
pub mod media_codec {
    use std::ffi::{c_char, c_int, c_long, c_void, CStr, CString};
    use std::sync::OnceLock;

    use super::super::{Decoded, MAX_CHANNELS, RATE_RANGE};
    use super::PlatformCodec;

    type Status = c_int; // media_status_t: AMEDIA_OK == 0
    const AMEDIA_OK: Status = 0;
    const AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED: isize = -3;
    const AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED: isize = -2;
    const AMEDIACODEC_INFO_TRY_AGAIN_LATER: isize = -1;
    /// µs per codec round, so neither loop can hang on a stalled device.
    const TIMEOUT_US: i64 = 100_000;
    const BUFFER_FLAG_END_OF_STREAM: u32 = 4;

    /// `bool` in the NDK headers is one byte (stdbool), not C's `int`.
    type Bool = u8;

    /// The NDK's opaque handles: the headers spell them as pointers to
    /// undeclared structs, which is exactly `*mut c_void` in Rust — a
    /// type alias per name keeps the calls honest without inventing a
    /// struct the C side never defines.
    type MediaFormat = *mut c_void;
    type MediaExtractor = *mut c_void;
    type MediaCodecHandle = *mut c_void;

    /// What `AMediaCodec_dequeueOutputBuffer` reports about a buffer.
    #[repr(C)]
    struct BufferInfo {
        offset: i32,
        size: i32,
        presentation_time_us: i64,
        flags: u32,
    }

    /// The symbols this module needs, resolved once.
    struct Api {
        _lib: crate::backend::dl::Library,
        extractor_new: unsafe extern "C" fn() -> MediaExtractor,
        extractor_set_fd: unsafe extern "C" fn(MediaExtractor, c_int, i64, i64) -> Status,
        extractor_delete: unsafe extern "C" fn(MediaExtractor),
        extractor_track_count: unsafe extern "C" fn(MediaExtractor) -> isize,
        extractor_track_format: unsafe extern "C" fn(MediaExtractor, usize) -> MediaFormat,
        extractor_select: unsafe extern "C" fn(MediaExtractor, usize) -> Status,
        extractor_read: unsafe extern "C" fn(MediaExtractor, *mut u8, usize) -> isize,
        extractor_advance: unsafe extern "C" fn(MediaExtractor) -> Status,
        format_delete: unsafe extern "C" fn(MediaFormat),
        format_get_string: unsafe extern "C" fn(MediaFormat, *const c_char) -> *const c_char,
        format_get_int32: unsafe extern "C" fn(MediaFormat, *const c_char, *mut i32) -> Bool,
        codec_create_decoder: unsafe extern "C" fn(*const c_char) -> MediaCodecHandle,
        codec_configure: unsafe extern "C" fn(
            MediaCodecHandle,
            MediaFormat,
            *mut c_void,
            *mut c_void,
            u32,
        ) -> Status,
        codec_start: unsafe extern "C" fn(MediaCodecHandle) -> Status,
        codec_stop: unsafe extern "C" fn(MediaCodecHandle) -> Status,
        codec_delete: unsafe extern "C" fn(MediaCodecHandle),
        codec_dequeue_input: unsafe extern "C" fn(MediaCodecHandle, i64) -> isize,
        codec_input_buffer: unsafe extern "C" fn(MediaCodecHandle, usize, *mut usize) -> *mut u8,
        codec_queue_input:
            unsafe extern "C" fn(MediaCodecHandle, usize, i32, usize, i64, u32) -> Status,
        codec_dequeue_output: unsafe extern "C" fn(MediaCodecHandle, *mut BufferInfo, i64) -> isize,
        codec_output_buffer: unsafe extern "C" fn(MediaCodecHandle, usize, *mut usize) -> *mut u8,
        codec_release_output: unsafe extern "C" fn(MediaCodecHandle, usize, Bool) -> Status,
        codec_output_format: unsafe extern "C" fn(MediaCodecHandle) -> MediaFormat,
    }

    unsafe extern "C" {
        /// bionic's `syscall` — std links libc, so the symbol exists.
        fn syscall(num: c_long, ...) -> c_long;
        fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        fn lseek64(fd: c_int, offset: i64, whence: c_int) -> i64;
        fn close(fd: c_int) -> c_int;
    }

    // SYS_memfd_create, the one number that differs per architecture.
    #[cfg(target_arch = "aarch64")]
    const SYS_MEMFD_CREATE: c_long = 279;
    #[cfg(target_arch = "arm")]
    const SYS_MEMFD_CREATE: c_long = 385;
    #[cfg(target_arch = "x86_64")]
    const SYS_MEMFD_CREATE: c_long = 319;
    #[cfg(target_arch = "x86")]
    const SYS_MEMFD_CREATE: c_long = 354;
    #[cfg(target_arch = "riscv64")]
    const SYS_MEMFD_CREATE: c_long = 279;

    /// The one-line failure of this module.
    fn err(what: &'static str, detail: impl std::fmt::Display) -> String {
        format!("media-codec: {what} ({detail})")
    }

    impl Api {
        fn load() -> Result<&'static Self, String> {
            static API: OnceLock<Result<Api, String>> = OnceLock::new();
            API.get_or_init(|| {
                let lib = crate::backend::dl::Library::open(&["libmediandk.so"])?;
                macro_rules! sym {
                    ($name:literal) => {
                        match lib.get(CStr::from_bytes_with_nul_unchecked(
                            concat!($name, "\0").as_bytes(),
                        )) {
                            Ok(f) => f,
                            Err(e) => return Err(e),
                        }
                    };
                }
                // SAFETY: every symbol below is looked up by its exact NDK
                // name and transmuted to its exact NDK type, as `dl.rs`
                // does for the audio backends; this runs once.
                unsafe {
                    Ok(Api {
                        extractor_new: sym!("AMediaExtractor_new"),
                        extractor_set_fd: sym!("AMediaExtractor_setDataSourceFd"),
                        extractor_delete: sym!("AMediaExtractor_delete"),
                        extractor_track_count: sym!("AMediaExtractor_getTrackCount"),
                        extractor_track_format: sym!("AMediaExtractor_getTrackFormat"),
                        extractor_select: sym!("AMediaExtractor_selectTrack"),
                        extractor_read: sym!("AMediaExtractor_readSampleData"),
                        extractor_advance: sym!("AMediaExtractor_advance"),
                        format_delete: sym!("AMediaFormat_delete"),
                        format_get_string: sym!("AMediaFormat_getString"),
                        format_get_int32: sym!("AMediaFormat_getInt32"),
                        codec_create_decoder: sym!("AMediaCodec_createDecoderByType"),
                        codec_configure: sym!("AMediaCodec_configure"),
                        codec_start: sym!("AMediaCodec_start"),
                        codec_stop: sym!("AMediaCodec_stop"),
                        codec_delete: sym!("AMediaCodec_delete"),
                        codec_dequeue_input: sym!("AMediaCodec_dequeueInputBuffer"),
                        codec_input_buffer: sym!("AMediaCodec_getInputBuffer"),
                        codec_queue_input: sym!("AMediaCodec_queueInputBuffer"),
                        codec_dequeue_output: sym!("AMediaCodec_dequeueOutputBuffer"),
                        codec_output_buffer: sym!("AMediaCodec_getOutputBuffer"),
                        codec_release_output: sym!("AMediaCodec_releaseOutputBuffer"),
                        codec_output_format: sym!("AMediaCodec_getOutputFormat"),
                        _lib: lib,
                    })
                }
            })
            .as_ref()
            .map_err(Clone::clone)
        }

        /// The format's string value for `key`.
        ///
        /// # Safety
        /// `f` is a live `AMediaFormat*` the caller owns.
        unsafe fn string<'a>(&self, f: MediaFormat, key: &CStr) -> Option<&'a str> {
            // SAFETY: the caller's contract; the NDK keeps the value
            // alive with the format.
            let p = unsafe { (self.format_get_string)(f, key.as_ptr()) };
            (!p.is_null())
                .then(|| unsafe { CStr::from_ptr(p) }.to_str().ok())
                .flatten()
        }

        /// The format's int32 value for `key`.
        ///
        /// # Safety
        /// `f` is a live `AMediaFormat*` the caller owns.
        unsafe fn int32(&self, f: MediaFormat, key: &CStr) -> Option<i32> {
            let mut v = 0i32;
            // SAFETY: the caller's contract; `v` is the out-parameter. The
            // NDK's `bool` is one byte, so this is a `!= 0` test — `bool`'s
            // combinators do not exist on `u8`.
            (unsafe { (self.format_get_int32)(f, key.as_ptr(), &raw mut v) } != 0).then_some(v)
        }
    }

    /// The pipeline. `Send + Sync` holds: the `Api` is a once-loaded
    /// table of plain function pointers over a `dl.rs` handle (which is
    /// itself `Send + Sync`), and no decode state survives a call.
    pub struct MediaCodec {
        api: &'static Api,
    }

    impl std::fmt::Debug for MediaCodec {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // The Api is a table of fn pointers; there is nothing of it
            // worth printing beyond that it resolved.
            f.write_str("MediaCodec")
        }
    }

    impl MediaCodec {
        /// Load `libmediandk.so`; `None` when it cannot be (an OS without
        /// the NDK media stack), which `system()` reports as "no bridge".
        pub fn new() -> Option<Self> {
            Api::load().ok().map(|api| Self { api })
        }
    }

    impl PlatformCodec for MediaCodec {
        fn name(&self) -> &'static str {
            "media-codec"
        }

        fn decode(&self, bytes: &[u8], _mime: &str) -> Result<Decoded, String> {
            // Spool the bytes into a memfd: an anonymous, seekable file.
            let name = c"vieww-audio";
            // SAFETY: a valid syscall number and arguments; flags 0 —
            // MFD_CLOEXEC would race a fork we do not do.
            let fd = unsafe { syscall(SYS_MEMFD_CREATE, name.as_ptr(), 0usize) };
            if fd < 0 {
                return Err(err("no memfd", std::io::Error::last_os_error()));
            }
            let fd = FdGuard(fd as c_int);
            let mut at = 0usize;
            while at < bytes.len() {
                // SAFETY: our fd, a slice of the caller's bytes.
                let n = unsafe { write(fd.0, bytes[at..].as_ptr().cast(), bytes.len() - at) };
                if n <= 0 {
                    return Err(err("memfd write", std::io::Error::last_os_error()));
                }
                at += n as usize;
            }
            // SAFETY: our fd, to its start (SEEK_SET = 0).
            unsafe { lseek64(fd.0, 0, 0) };

            let a = self.api;
            // SAFETY: from here every call takes handles this function
            // created, with the NDK's own types; the guards close what
            // the NDK does not, and each loop iteration is bounded.
            unsafe {
                let extractor = (a.extractor_new)();
                if extractor.is_null() {
                    return Err(err("extractor", "null handle"));
                }
                let _ex = Guard(extractor, |p| (a.extractor_delete)(p));
                let st = (a.extractor_set_fd)(extractor, fd.0, 0, bytes.len() as i64);
                if st != AMEDIA_OK {
                    return Err(err("set data source", st));
                }
                // Find the audio track: its format's "is-audio" key.
                let mime_key = c"mime";
                let is_audio_key = c"is-audio";
                let rate_key = c"sample-rate";
                let channel_key = c"channel-count";
                let n = (a.extractor_track_count)(extractor);
                let mut track_mime: Option<String> = None;
                let mut track = None;
                for i in 0..usize::try_from(n.max(0)).unwrap_or(0) {
                    let f = (a.extractor_track_format)(extractor, i);
                    if f.is_null() {
                        continue;
                    }
                    let keep = a.string(f, is_audio_key) == Some("true")
                        && a.string(f, mime_key).is_some();
                    if keep {
                        track_mime = a.string(f, mime_key).map(str::to_owned);
                        track = Some(i);
                        // The format stays alive with the extractor; the
                        // codec is configured from it below.
                    } else {
                        (a.format_delete)(f);
                    }
                }
                let Some(track_mime) = track_mime else {
                    return Err(err("no audio track", n));
                };
                let track = track.unwrap_or(0);
                let st = (a.extractor_select)(extractor, track);
                if st != AMEDIA_OK {
                    return Err(err("select track", st));
                }

                let c_track_mime = CString::new(track_mime.clone()).map_err(|e| err("mime", e))?;
                let codec = (a.codec_create_decoder)(c_track_mime.as_ptr());
                if codec.is_null() {
                    return Err(err("no decoder for", track_mime));
                }
                let _cg = Guard(codec, |p| {
                    (a.codec_stop)(p);
                    (a.codec_delete)(p);
                });
                // Configure from the selected track's format, then start.
                let track_format = (a.extractor_track_format)(extractor, track);
                if (a.codec_configure)(
                    codec,
                    track_format,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                ) != AMEDIA_OK
                {
                    (a.format_delete)(track_format);
                    return Err(err("configure", track_mime));
                }
                (a.format_delete)(track_format);
                if (a.codec_start)(codec) != AMEDIA_OK {
                    return Err(err("start", track_mime));
                }

                // The pump: extractor sample → input buffer; output
                // buffer → PCM. An empty read is the extractor's EOS.
                let mut pcm: Vec<i16> = Vec::new();
                let mut sample = [0u8; 64 << 10];
                let mut input_done = false;
                let mut output_done = false;
                let mut rate = 0i32;
                let mut channels = 0i32;
                while !output_done {
                    if !input_done {
                        let n = (a.extractor_read)(extractor, sample.as_mut_ptr(), sample.len());
                        let idx = (a.codec_dequeue_input)(codec, TIMEOUT_US);
                        if idx >= 0 {
                            let idx = idx as usize;
                            let mut size = 0usize;
                            let buf = (a.codec_input_buffer)(codec, idx, &raw mut size);
                            if buf.is_null() {
                                return Err(err("input buffer", idx));
                            }
                            let take = (n.max(0) as usize).min(size).min(sample.len());
                            if take > 0 {
                                std::ptr::copy_nonoverlapping(sample.as_ptr(), buf, take);
                            }
                            let eos = u32::from(n == 0) * BUFFER_FLAG_END_OF_STREAM;
                            let st = (a.codec_queue_input)(codec, idx, 0, take, 0, eos);
                            if st != AMEDIA_OK {
                                return Err(err("queue input", st));
                            }
                            if eos != 0 {
                                input_done = true;
                            } else if (a.extractor_advance)(extractor) != AMEDIA_OK {
                                return Err(err("advance", st));
                            }
                        }
                    }
                    let mut info = BufferInfo {
                        offset: 0,
                        size: 0,
                        presentation_time_us: 0,
                        flags: 0,
                    };
                    let idx = (a.codec_dequeue_output)(codec, &raw mut info, TIMEOUT_US);
                    match idx {
                        AMEDIACODEC_INFO_TRY_AGAIN_LATER
                        | AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED => {}
                        AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED => {
                            let f = (a.codec_output_format)(codec);
                            if !f.is_null() {
                                rate = a.int32(f, rate_key).unwrap_or(0);
                                channels = a.int32(f, channel_key).unwrap_or(0);
                                (a.format_delete)(f);
                            }
                        }
                        i if i >= 0 => {
                            let idx = idx as usize;
                            let mut size = 0usize;
                            let buf = (a.codec_output_buffer)(codec, idx, &raw mut size);
                            if !buf.is_null() {
                                let from = info.offset.max(0) as usize;
                                let len =
                                    (info.size.max(0) as usize).min(size.saturating_sub(from));
                                if from + len <= size {
                                    // `buf` is the NDK's own `size`-byte
                                    // buffer, `from + len` bounded inside
                                    // it; the decode is 16-bit PCM per the
                                    // codec's output. (No inner `unsafe`:
                                    // the whole decode body is one block.)
                                    let as_i16: &[i16] =
                                        std::slice::from_raw_parts(buf.add(from).cast(), len / 2);
                                    pcm.extend_from_slice(as_i16);
                                }
                            }
                            let _ = (a.codec_release_output)(codec, idx, 0);
                            if info.flags & BUFFER_FLAG_END_OF_STREAM != 0 {
                                output_done = true;
                            }
                        }
                        _ => return Err(err("dequeue output", idx)),
                    }
                }
                (a.codec_stop)(codec);
                if !RATE_RANGE.contains(&(u32::try_from(rate).unwrap_or(0))) {
                    return Err(err("no sample rate", rate));
                }
                let channels = usize::try_from(channels.max(1)).unwrap_or(1);
                if channels > usize::from(MAX_CHANNELS) {
                    return Err(err("channel count", channels));
                }
                Ok(Decoded {
                    data: pcm.iter().map(|&s| f32::from(s) / 32_768.0).collect(),
                    channels: u8::try_from(channels).unwrap_or(u8::MAX),
                    rate: u32::try_from(rate).unwrap_or(0),
                })
            }
        }
    }

    /// Close an fd exactly once.
    struct FdGuard(c_int);

    impl Drop for FdGuard {
        fn drop(&mut self) {
            // SAFETY: our fd, closed once.
            unsafe { close(self.0) };
        }
    }

    /// Run a closure at drop. The NDK teardown calls need the `Api` table,
    /// so the closure captures — a bare `fn` pointer would not compile.
    struct Guard<T: Copy, F: Fn(T)>(T, F);

    impl<T: Copy, F: Fn(T)> Drop for Guard<T, F> {
        fn drop(&mut self) {
            (self.1)(self.0);
        }
    }
}

#[cfg(target_os = "android")]
pub use media_codec::MediaCodec;

/// Apple's `ExtAudioFile`: the container readers plus `AudioConverter`,
/// asked for interleaved float32.
///
/// `ExtAudioFile` opens by URL, so the bytes are spooled to the sandbox's
/// temporary directory first — the one cost of the simplest correct API
/// (an in-memory `AudioConverter` pipeline is the follow-up if a farm run
/// ever shows the spool matters). The `mime` parameter is advisory only:
/// `ExtAudioFile` sniffs the container itself and refuses what it does
/// not read.
///
/// Written against `<AudioToolbox/ExtAudioFile.h>` and
/// `<CoreFoundation/CFURL.h>`; compile-checked for `aarch64-apple-darwin`,
/// never run on Apple hardware in CI — the farm's audio step owns that
/// receipt (AU-14).
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod ext_audio_file {
    use std::ffi::{c_char, c_int, c_void, CString};

    use super::super::{Decoded, MAX_CHANNELS};
    use super::PlatformCodec;

    type OSStatus = c_int;
    type UInt32 = u32;
    /// Four-char code.
    type FourCC = u32;
    const NO_ERR: OSStatus = 0;
    const FILE_FORMAT: FourCC = u32::from_be_bytes(*b"ffmt");
    const CLIENT_FORMAT: FourCC = u32::from_be_bytes(*b"cfmt");
    /// `kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked`.
    const LPCM_FLOAT_FLAGS: UInt32 = 0x1 | 0x8;

    #[repr(C)]
    struct ExtAudioFileRef(*mut c_void);

    #[repr(C)]
    #[allow(non_snake_case, reason = "the header's own field names")]
    struct AudioStreamBasicDescription {
        mSampleRate: f64,
        mFormatID: FourCC,
        mFormatFlags: UInt32,
        mBytesPerPacket: UInt32,
        mFramesPerPacket: UInt32,
        mBytesPerFrame: UInt32,
        mChannelsPerFrame: UInt32,
        mBitsPerChannel: UInt32,
        mReserved: UInt32,
    }

    #[repr(C)]
    #[allow(non_snake_case, reason = "the header's own field names")]
    struct AudioBuffer {
        mNumberChannels: UInt32,
        mDataByteSize: UInt32,
        mData: *mut c_void,
    }

    #[repr(C)]
    #[allow(non_snake_case, reason = "the header's own field names")]
    struct AudioBufferList {
        mNumberBuffers: UInt32,
        mBuffers: [AudioBuffer; 1],
    }

    unsafe extern "C" {
        // CoreFoundation, linked by std on Apple targets.
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *mut c_void,
            path: *const c_char,
            len: isize,
            is_directory: u8,
        ) -> *mut c_void;
        fn CFRelease(cf: *const c_void);
        // AudioToolbox: present on macOS and iOS alike.
        fn ExtAudioFileOpenURL(url: *mut c_void, out: *mut ExtAudioFileRef) -> OSStatus;
        fn ExtAudioFileDispose(f: ExtAudioFileRef) -> OSStatus;
        fn ExtAudioFileSetProperty(
            f: ExtAudioFileRef,
            prop: FourCC,
            size: UInt32,
            data: *const c_void,
        ) -> OSStatus;
        fn ExtAudioFileGetProperty(
            f: ExtAudioFileRef,
            prop: FourCC,
            size: *mut UInt32,
            data: *mut c_void,
        ) -> OSStatus;
        fn ExtAudioFileRead(
            f: ExtAudioFileRef,
            frames: *mut UInt32,
            data: *mut AudioBufferList,
        ) -> OSStatus;
    }

    fn err(what: &'static str, status: OSStatus) -> String {
        format!("ext-audio-file: {what} (OSStatus {status})")
    }

    /// The one instance — it is stateless; every call opens its own file.
    #[derive(Debug)]
    pub struct ExtAudioFile;

    impl PlatformCodec for ExtAudioFile {
        fn name(&self) -> &'static str {
            "ext-audio-file"
        }

        fn decode(&self, bytes: &[u8], _mime: &str) -> Result<Decoded, String> {
            // Spool to the sandbox's tmp: ExtAudioFile opens by URL.
            let path = std::env::temp_dir().join("vieww-audio-platform-decode");
            std::fs::write(&path, bytes).map_err(|e| format!("ext-audio-file: spool ({e})"))?;
            let _unlink = Unlink(path.clone());
            let c_path = CString::new(path.to_string_lossy().as_bytes().to_vec())
                .map_err(|e| format!("ext-audio-file: path ({e})"))?;
            // SAFETY: from here, every call takes the types above on
            // handles this function creates; the guards release them.
            unsafe {
                let url = CFURLCreateFromFileSystemRepresentation(
                    std::ptr::null_mut(),
                    c_path.as_ptr(),
                    c_path.as_bytes().len() as isize,
                    0,
                );
                if url.is_null() {
                    return Err(err("url", -1));
                }
                let _url = Guard(url, |p| CFRelease(p.cast_const()));
                let mut file = ExtAudioFileRef(std::ptr::null_mut());
                let st = ExtAudioFileOpenURL(url, &raw mut file);
                if st != NO_ERR || file.0.is_null() {
                    return Err(err("open", st));
                }
                let _file = Guard(file.0, |p| {
                    let _ = ExtAudioFileDispose(ExtAudioFileRef(p));
                });

                // The file's own shape, for channels and rate.
                let mut file_format = AudioStreamBasicDescription {
                    mSampleRate: 0.0,
                    mFormatID: 0,
                    mFormatFlags: 0,
                    mBytesPerPacket: 0,
                    mFramesPerPacket: 0,
                    mBytesPerFrame: 0,
                    mChannelsPerFrame: 0,
                    mBitsPerChannel: 0,
                    mReserved: 0,
                };
                let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as UInt32;
                let st = ExtAudioFileGetProperty(
                    ExtAudioFileRef(file.0),
                    FILE_FORMAT,
                    &raw mut size,
                    std::ptr::addr_of_mut!(file_format).cast(),
                );
                if st != NO_ERR {
                    return Err(err("file format", st));
                }
                let channels = file_format
                    .mChannelsPerFrame
                    .clamp(1, UInt32::from(MAX_CHANNELS));
                // Ask for interleaved packed float32 at the same rate:
                // AudioConverter converts the stream, resamples nothing.
                let client = AudioStreamBasicDescription {
                    mSampleRate: file_format.mSampleRate,
                    mFormatID: u32::from_be_bytes(*b"lpcm"),
                    mFormatFlags: LPCM_FLOAT_FLAGS,
                    mBytesPerPacket: 4 * channels,
                    mFramesPerPacket: 1,
                    mBytesPerFrame: 4 * channels,
                    mChannelsPerFrame: channels,
                    mBitsPerChannel: 32,
                    mReserved: 0,
                };
                let st = ExtAudioFileSetProperty(
                    ExtAudioFileRef(file.0),
                    CLIENT_FORMAT,
                    std::mem::size_of::<AudioStreamBasicDescription>() as UInt32,
                    std::ptr::addr_of!(client).cast(),
                );
                if st != NO_ERR {
                    return Err(err("client format", st));
                }

                let mut data: Vec<f32> = Vec::new();
                let ch = channels as usize;
                let mut buf = vec![0f32; 4096 * ch];
                loop {
                    let mut frames = (buf.len() / ch) as UInt32;
                    let mut list = AudioBufferList {
                        mNumberBuffers: 1,
                        mBuffers: [AudioBuffer {
                            mNumberChannels: channels,
                            mDataByteSize: frames * 4 * channels,
                            mData: buf.as_mut_ptr().cast(),
                        }],
                    };
                    let st =
                        ExtAudioFileRead(ExtAudioFileRef(file.0), &raw mut frames, &raw mut list);
                    if st != NO_ERR {
                        return Err(err("read", st));
                    }
                    if frames == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..frames as usize * ch]);
                }
                let rate = file_format.mSampleRate.round();
                if !(rate.is_finite() && (1.0..=768_000.0).contains(&rate)) {
                    return Err(err("sample rate", 0));
                }
                Ok(Decoded {
                    data,
                    channels: u8::try_from(channels).unwrap_or(u8::MAX),
                    rate: rate as u32,
                })
            }
        }
    }

    /// Remove the spooled file at drop, decode or not.
    struct Unlink(std::path::PathBuf);

    impl Drop for Unlink {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Run a closure at drop.
    struct Guard<T: Copy>(T, fn(T));

    impl<T: Copy> Drop for Guard<T> {
        fn drop(&mut self) {
            (self.1)(self.0);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use ext_audio_file::ExtAudioFile;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{opus, stream};

    /// The slot is one global, so the tests that install into it run one
    /// at a time — a hand-off proven against another test's codec is no
    /// proof at all. (Cargo runs tests in parallel within a binary; this
    /// lock is the standard-library answer.)
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A platform codec for the tests: it "decodes" whatever it is asked
    /// for into a deterministic signal, proving the plumbing — the
    /// hand-off, the mime the pure layer derived, the Opus gain applied
    /// on top, the validation — without pretending a decode happened.
    struct Fake {
        mime: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }

    impl PlatformCodec for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn decode(&self, _bytes: &[u8], mime: &str) -> Result<Decoded, String> {
            *self.mime.lock().unwrap() = Some(mime.to_owned());
            // A steady stereo signal at the Opus clock, trimmed like the
            // contract demands (the platform owns the pre-skip).
            Ok(Decoded {
                data: vec![0.25; 480 * 2],
                channels: 2,
                rate: 48_000,
            })
        }
    }

    /// An Ogg page, lacing and CRC included — the opus module's own test
    /// builder, kept local so this module's tests stand alone.
    fn page(granule: i64, packets: &[&[u8]], last: bool) -> Vec<u8> {
        let mut lacing = Vec::new();
        let mut body = Vec::new();
        for p in packets {
            body.extend_from_slice(p);
            let mut left = p.len();
            while left >= 255 {
                lacing.push(255);
                left -= 255;
            }
            lacing.push(u8::try_from(left).unwrap());
        }
        let mut b = Vec::with_capacity(27 + lacing.len() + body.len());
        b.extend_from_slice(b"OggS");
        b.push(0);
        b.push(u8::from(last) << 2 | 2);
        b.extend_from_slice(&granule.to_le_bytes());
        b.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.push(u8::try_from(lacing.len()).unwrap());
        b.extend_from_slice(&lacing);
        b.extend_from_slice(&body);
        let crc = super::super::ogg::crc32(&b);
        b[22..26].copy_from_slice(&crc.to_le_bytes());
        b
    }

    /// A hand-built Ogg-Opus stream with an output gain to apply.
    fn opus_stream(gain: i16) -> Vec<u8> {
        let mut head = b"OpusHead".to_vec();
        head.push(1); // version
        head.push(2); // stereo
        head.extend_from_slice(&312u16.to_le_bytes()); // pre-skip
        head.extend_from_slice(&48_000u32.to_le_bytes());
        head.extend_from_slice(&gain.to_le_bytes());
        head.push(0); // mapping family 0
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&0u32.to_le_bytes());
        tags.extend_from_slice(&0u32.to_le_bytes());
        let audio = [0x9Cu8, 1, 1, 1]; // 20 ms stereo CELT TOC
        let mut bytes = page(0, &[&head], false);
        bytes.extend(page(0, &[&tags], false));
        bytes.extend(page(960, &[&audio], true));
        bytes
    }

    #[test]
    fn without_a_codec_the_pure_refusal_comes_back() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        uninstall(); // whatever a previous test left, start empty
        let e = decode(&opus_stream(0)).unwrap_err();
        assert!(matches!(e, CodecError::Unsupported(w) if w.contains("Opus")));
        assert!(!is_installed());
        // Formats the pure side decodes never ask: FLAC decodes as ever.
        let flac = include_bytes!("../../tests/fixtures/src16.flac");
        assert!(decode(flac).is_ok());
    }

    #[test]
    fn a_platform_decode_takes_the_hand_off_and_finishes_it() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let mime = std::sync::Arc::new(std::sync::Mutex::new(None));
        install(Box::new(Fake {
            mime: std::sync::Arc::clone(&mime),
        }));
        // +3.125 dB of output gain: the pure side applies it on top of
        // the platform's signal, per RFC 7845 — the container's own job.
        let d = decode(&opus_stream(800)).unwrap();
        assert_eq!(mime.lock().unwrap().as_deref(), Some("audio/opus"));
        let g = 10f32.powf(3.125 / 20.0);
        assert!((d.data[0] - 0.25 * g).abs() < 1e-6, "{}", d.data[0]);
        assert_eq!((d.channels, d.rate), (2, 48_000));
        assert!(uninstall());
    }

    #[test]
    fn the_hand_off_mime_is_derived_from_the_container() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        uninstall(); // whatever a previous test left, start empty
                     // The AAC cases the PATENTS.md alternative describes, and the
                     // refusal that must not consult a platform at all.
        let m4a = include_bytes!("../../tests/fixtures/src.m4a");
        assert_eq!(handoff_mime(m4a), Some("audio/mp4a-latm"));
        let adts = include_bytes!("../../tests/fixtures/src_mono.aac");
        assert_eq!(handoff_mime(adts), Some("audio/aac"));
        let vorbis = include_bytes!("../../tests/fixtures/src.ogg");
        assert_eq!(handoff_mime(vorbis), None);
        let alac = include_bytes!("../../tests/fixtures/src_alac.m4a");
        assert_eq!(handoff_mime(alac), None, "ALAC decodes purely — no bridge");
    }

    #[test]
    fn a_platform_decode_is_validated_like_any_other() {
        struct Runaway;
        impl PlatformCodec for Runaway {
            fn name(&self) -> &'static str {
                "runaway"
            }
            fn decode(&self, _bytes: &[u8], _mime: &str) -> Result<Decoded, String> {
                // A hostile platform is treated like a hostile file: the
                // runaway block is concealed, the rest kept.
                let mut d = Decoded {
                    data: vec![0.5; super::super::CONCEAL_FRAMES * 2],
                    channels: 1,
                    rate: 48_000,
                };
                d.data[3] = 1.0e9;
                Ok(d)
            }
        }
        install(Box::new(Runaway));
        let d = decode(&opus_stream(0)).unwrap();
        assert!(d.data[..super::super::CONCEAL_FRAMES]
            .iter()
            .all(|v| *v == 0.0));
        assert!(uninstall());
    }

    #[test]
    fn the_streaming_side_refuses_without_a_platform() {
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        // The AU-05 seam honours the same refusal; with a platform
        // installed it serves chunks — proven in tests/stream_decode.rs,
        // which installs the same kind of fake.
        uninstall(); // whatever a previous test left, start empty
        let bytes = opus_stream(0);
        let cursor = std::io::Cursor::new(bytes.clone());
        let e = stream::open(Box::new(cursor))
            .err()
            .expect("refuses without a platform");
        assert!(matches!(e, CodecError::Unsupported(w) if w.contains("Opus")));
        // The container was still parsed on the way to the refusal: the
        // pure layer knows exactly what it is refusing.
        assert!(opus::is_opus(&bytes));
    }
}
