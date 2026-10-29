//! Sound that actually plays: the engine behind [`AudioPlayer`] (AU-01) and
//! a media-player facade over it (AU-11).
//!
//! [`Engine`] owns one output stream — opened on the first working backend
//! in [`backend::backends`] order and wrapped in a
//! [`ResilientStream`] so device changes and lost devices reopen by
//! themselves — and mixes up to [`MAX_VOICES`] voices into it. Each voice is
//! a decoded buffer with a gain that ramps (no clicks on start, stop or
//! volume changes), pause, seek, looping and a position you can read.
//!
//! * [`EnginePlayer`] is the [`AudioPlayer`] a platform crate registers: the
//!   small fire-and-forget seam, now with something behind it.
//! * [`MediaPlayer`] is the rich facade a player UI needs: `load`, `play`,
//!   `pause`, `seek`, `position`, `duration`, `set_volume` with a fade,
//!   looping, and [`PlaybackEvent`]s for "it ended".
//!
//! # Real-time discipline
//!
//! The audio callback never allocates, never frees and never waits. Voices
//! live in a `Vec` whose capacity is reserved up front; the control side
//! pushes into it (no reallocation, capacity is never exceeded) and is the
//! only side that removes from it, so the last reference to a buffer is
//! always dropped off the audio thread. The callback takes the voice list
//! with `try_lock`: if a control call holds it at that instant, the block
//! is rendered silent and counted in [`EngineStats::contended`] rather than
//! blocking the audio thread. Control calls hold the lock for a push or a
//! field write — microseconds — so in practice the count stays at zero; the
//! `realtime_no_alloc` harness covers [`Engine::render`].
//!
//! Large files are decoded whole into memory here; streaming playback of
//! long media is `DiskStream`'s job (AU-05).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vieww_foundation::service::ServiceError;

use crate::backend::resilient::{ResilientStream, Target};
use crate::backend::{self, Backend, BackendError, BlockInfo, StreamConfig};
use crate::focus::FocusRequest;
use crate::player::{AudioPlayer, PlaybackHandle, Sound};
use crate::resample::Quality;
use crate::session::{AudioSession, NowPlaying, RemoteCommand};
use crate::Samples;

/// Most voices mixed at once. A UI plays a handful; a game a few dozen.
pub const MAX_VOICES: usize = 64;

/// A playing (or paused) voice's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoiceId(pub u64);

/// How a voice starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceOptions {
    /// Linear gain, 1.0 = unity.
    pub gain: f32,
    /// Restart from the beginning at the end instead of ending.
    pub looping: bool,
    /// Ramp up from silence over this long.
    pub fade_in: Duration,
    /// Where in the buffer to start.
    pub start_at: Duration,
    /// Start paused (a loaded media file waiting for `play`).
    pub paused: bool,
}

impl Default for VoiceOptions {
    fn default() -> Self {
        Self {
            gain: 1.0,
            looping: false,
            fade_in: DECLICK,
            start_at: Duration::ZERO,
            paused: false,
        }
    }
}

/// The ramp applied to every start, stop and pause, so nothing clicks.
pub const DECLICK: Duration = Duration::from_millis(5);

/// Something that happened to a voice, read with [`Engine::poll_events`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackEvent {
    /// It reached its end (and was not looping).
    Ended(VoiceId),
    /// It was stopped by a call.
    Stopped(VoiceId),
}

/// Why a voice could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayError {
    /// [`MAX_VOICES`] are already playing.
    TooManyVoices,
    /// The buffer has no frames.
    Empty,
    /// The media could not be read or decoded.
    Media(String),
    /// The platform refused audio focus — a call is in progress, for
    /// instance (AU-02). Playing anyway would talk over it.
    FocusDenied,
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyVoices => write!(f, "{MAX_VOICES} voices are already playing"),
            Self::Empty => write!(f, "the sound has no samples"),
            Self::Media(e) => write!(f, "{e}"),
            Self::FocusDenied => write!(f, "another app or a call has the audio focus"),
        }
    }
}

impl std::error::Error for PlayError {}

/// Counters for a running engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineStats {
    /// Frames rendered.
    pub frames: u64,
    /// Blocks rendered silent because a control call held the voice list.
    pub contended: u64,
    /// Voices playing or paused now.
    pub voices: usize,
}

/// A streamed voice's source: a disk stream at the engine's rate and the
/// block it last read, consumed a frame at a time (AU-05).
struct StreamVoice {
    disk: crate::loader::DiskStream,
    scratch: Box<[f32]>,
    len: usize,
    idx: usize,
    ch: usize,
    /// The two frames the rate-matching interpolation sits between.
    a: [f32; 2],
    b: [f32; 2],
    primed: bool,
}

impl std::fmt::Debug for StreamVoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamVoice")
            .field("disk", &self.disk)
            .finish_non_exhaustive()
    }
}

impl StreamVoice {
    /// The next frame (first two channels), or `None` when the stream has
    /// nothing buffered. Never blocks, never allocates.
    fn pull(&mut self) -> Option<[f32; 2]> {
        if self.idx >= self.len {
            let got = self.disk.read(&mut self.scratch);
            if got == 0 {
                return None;
            }
            self.len = got;
            self.idx = 0;
        }
        let f = &self.scratch[self.idx..self.idx + self.ch];
        self.idx += self.ch;
        Some([f[0], f[1.min(self.ch - 1)]])
    }

    fn seek(&mut self, frame: u64) {
        self.disk.seek(frame);
        self.idx = self.len;
        self.primed = false;
    }
}

#[derive(Debug)]
enum VoiceSrc {
    Mem(Arc<Samples>),
    Stream(Box<StreamVoice>),
}

#[derive(Debug)]
struct Voice {
    id: u64,
    src: VoiceSrc,
    frame: usize,
    /// Fraction of a frame past `frame`, when the device runs at another
    /// rate than the engine (AU-09: a reopened stream on a device that
    /// granted a different rate).
    frac: f32,
    gain: f32,
    target: f32,
    step: f32,
    paused: bool,
    /// Pause once the fade to zero lands.
    pause_at_zero: bool,
    /// End once the fade to zero lands.
    stop_at_zero: bool,
    looping: bool,
    finished: Option<Finish>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finish {
    Ended,
    Stopped,
}

impl Voice {
    fn frames(&self) -> usize {
        match &self.src {
            VoiceSrc::Mem(s) => s.data.len() / usize::from(s.channels.max(1)),
            VoiceSrc::Stream(sv) => match sv.disk.frames() {
                0 => usize::MAX,
                n => usize::try_from(n).unwrap_or(usize::MAX),
            },
        }
    }

    /// One frame's gain-ramp step; `true` when the voice stops or pauses
    /// here.
    fn ramp_step(&mut self) -> bool {
        if self.step != 0.0 {
            self.gain += self.step;
            if (self.step > 0.0 && self.gain >= self.target)
                || (self.step < 0.0 && self.gain <= self.target)
            {
                self.gain = self.target;
                self.step = 0.0;
            }
        }
        if self.step == 0.0 && self.gain <= 0.0 {
            if self.stop_at_zero {
                self.finished = Some(Finish::Stopped);
                return true;
            }
            if self.pause_at_zero {
                self.paused = true;
                self.pause_at_zero = false;
                return true;
            }
        }
        false
    }

    fn ramp_to(&mut self, target: f32, over: Duration, rate: u32) {
        self.target = target;
        #[expect(clippy::cast_possible_truncation, reason = "a ramp length in frames")]
        let frames = (over.as_secs_f64() * f64::from(rate)).max(1.0) as f32;
        self.step = (target - self.gain) / frames;
    }
}

#[derive(Debug)]
struct Shared {
    voices: Mutex<Vec<Voice>>,
    frames: AtomicU64,
    contended: AtomicU64,
    master: AtomicU32,
    /// Focus, interruptions, ducking (AU-02/03).
    session: Arc<AudioSession>,
    /// The duck gain the last block ended on, for the ramp.
    applied_duck: AtomicU32,
    /// The rate voices were prepared at (the engine's).
    rate: u32,
}

/// The mixer behind both facades. Cheap to share: clone the `Arc`.
pub struct Engine {
    shared: Arc<Shared>,
    rate: u32,
    channels: u16,
    next: AtomicU64,
    events: Mutex<Vec<PlaybackEvent>>,
    backend: &'static str,
    stream: Option<ResilientStream>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("backend", &self.backend)
            .field("rate", &self.rate)
            .field("channels", &self.channels)
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Open the platform's default output on the first backend that works.
    ///
    /// # Errors
    /// The last backend's refusal, or `Unavailable` when the platform has
    /// no backend at all.
    pub fn open_default() -> Result<Self, BackendError> {
        let mut last = BackendError::Unavailable("no audio backend on this platform".into());
        for b in backend::backends() {
            match Self::with_backend(b, StreamConfig::default()) {
                Ok(e) => return Ok(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Open `backend`'s default output with `config`.
    ///
    /// # Errors
    /// The backend's refusal.
    pub fn with_backend(
        backend: Box<dyn Backend>,
        config: StreamConfig,
    ) -> Result<Self, BackendError> {
        let name = backend.name();
        let mut engine = Self::detached(config.rate, config.channels);
        engine.backend = name;
        let shared = Arc::clone(&engine.shared);
        let session = Arc::clone(&engine.shared.session);
        let stream = ResilientStream::open_with_session(
            backend,
            Target::Default,
            config,
            Box::new(move |out: &mut [f32], info: &BlockInfo| {
                render_into(&shared, out, info.channels.max(1), info.rate);
            }),
            Some(session),
        )?;
        engine.stream = Some(stream);
        Ok(engine)
    }

    /// An engine with no device: blocks are pulled with [`Engine::render`].
    /// For tests and offline rendering.
    #[must_use]
    pub fn detached(rate: u32, channels: u16) -> Self {
        Self::detached_with_session(rate, channels, Arc::clone(AudioSession::shared()))
    }

    /// A detached engine on its own `session` (tests of focus behaviour).
    #[must_use]
    pub fn detached_with_session(rate: u32, channels: u16, session: Arc<AudioSession>) -> Self {
        Self {
            shared: Arc::new(Shared {
                voices: Mutex::new(Vec::with_capacity(MAX_VOICES)),
                frames: AtomicU64::new(0),
                contended: AtomicU64::new(0),
                master: AtomicU32::new(1.0f32.to_bits()),
                session,
                applied_duck: AtomicU32::new(1.0f32.to_bits()),
                rate: rate.max(1),
            }),
            rate: rate.max(1),
            channels: channels.max(1),
            next: AtomicU64::new(1),
            events: Mutex::new(Vec::new()),
            backend: "none",
            stream: None,
        }
    }

    /// Render one interleaved block (`out.len()` = frames × channels) — what
    /// the device callback does. Real-time safe.
    pub fn render(&self, out: &mut [f32]) {
        render_into(&self.shared, out, usize::from(self.channels), self.rate);
    }

    /// Render at a device `rate` other than the engine's — what the device
    /// callback does after a stream reopened on a device that granted
    /// another rate. For tests.
    #[cfg(test)]
    fn render_at(&self, out: &mut [f32], rate: u32) {
        render_into(&self.shared, out, usize::from(self.channels), rate);
    }

    /// The audio session this engine obeys.
    #[must_use]
    pub fn session(&self) -> &Arc<AudioSession> {
        &self.shared.session
    }

    /// The device stream's statistics (latency, underruns, buffer,
    /// exclusive mode), or `None` detached or between streams.
    #[must_use]
    pub fn stream_stats(&self) -> Option<crate::backend::StreamStats> {
        self.stream.as_ref().and_then(ResilientStream::stats)
    }

    /// Route events the device stream's supervisor has received (AU-09):
    /// backend notifications and the audio session's route changes. 0
    /// detached.
    #[must_use]
    pub fn route_events(&self) -> u64 {
        self.stream
            .as_ref()
            .map_or(0, ResilientStream::route_events)
    }

    /// Supervisor polls spent waiting out an interruption (0 detached).
    #[must_use]
    pub fn held_polls(&self) -> u64 {
        self.stream.as_ref().map_or(0, ResilientStream::held_polls)
    }

    /// The output rate.
    #[must_use]
    pub const fn rate(&self) -> u32 {
        self.rate
    }

    /// The backend the stream opened on (`"none"` when detached).
    #[must_use]
    pub const fn backend(&self) -> &'static str {
        self.backend
    }

    /// Start `samples`. Resampled to the engine's rate here, on the caller's
    /// thread.
    ///
    /// # Errors
    /// [`PlayError::Empty`] or [`PlayError::TooManyVoices`].
    pub fn play(&self, samples: &Samples, opts: VoiceOptions) -> Result<VoiceId, PlayError> {
        self.play_shared(self.prepare(samples)?, opts)
    }

    fn prepare(&self, samples: &Samples) -> Result<Arc<Samples>, PlayError> {
        if samples.data.is_empty() {
            return Err(PlayError::Empty);
        }
        Ok(Arc::new(if samples.rate == self.rate {
            samples.clone()
        } else {
            samples.resampled(self.rate, Quality::Sinc(16))
        }))
    }

    fn play_shared(&self, samples: Arc<Samples>, opts: VoiceOptions) -> Result<VoiceId, PlayError> {
        self.play_src(VoiceSrc::Mem(samples), opts)
    }

    /// Stream `path` from disk instead of decoding it whole (AU-05): any
    /// format [`crate::codec::stream`] reads, resampled to the engine's rate
    /// on a reader thread, about half a second buffered. Memory stays
    /// bounded however long the file is.
    ///
    /// # Errors
    /// The file cannot be opened or decoded, or the engine is full.
    pub fn play_streaming(
        &self,
        path: impl AsRef<std::path::Path>,
        opts: VoiceOptions,
    ) -> Result<VoiceId, PlayError> {
        let path = path.as_ref();
        let buffer = usize::try_from(self.rate / 2).unwrap_or(24_000).max(4_096);
        let disk = crate::loader::DiskStream::open_resampled(path, buffer, self.rate)
            .map_err(|e| PlayError::Media(format!("{}: {e}", path.display())))?;
        let ch = usize::from(disk.info().channels.max(1));
        let sv = StreamVoice {
            disk,
            scratch: vec![0.0; STREAM_BLOCK * ch].into_boxed_slice(),
            len: 0,
            idx: 0,
            ch,
            a: [0.0; 2],
            b: [0.0; 2],
            primed: false,
        };
        self.play_src(VoiceSrc::Stream(Box::new(sv)), opts)
    }

    fn play_src(&self, src: VoiceSrc, opts: VoiceOptions) -> Result<VoiceId, PlayError> {
        self.reap();
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a frame index"
        )]
        let start = (opts.start_at.as_secs_f64() * f64::from(self.rate)) as usize;
        let mut v = Voice {
            id,
            src,
            frame: 0,
            frac: 0.0,
            gain: 0.0,
            target: 0.0,
            step: 0.0,
            paused: opts.paused,
            pause_at_zero: false,
            stop_at_zero: false,
            looping: opts.looping,
            finished: None,
        };
        v.frame = start.min(v.frames());
        if let VoiceSrc::Stream(sv) = &mut v.src {
            sv.disk.set_looping(opts.looping);
            if v.frame > 0 {
                sv.seek(v.frame as u64);
            }
            // A first block ready before the voice joins the mix, so it
            // starts without an underrun.
            let _ = sv.disk.prime(STREAM_BLOCK, Duration::from_millis(500));
        }
        if opts.paused {
            v.target = opts.gain.max(0.0);
        } else {
            v.ramp_to(opts.gain.max(0.0), opts.fade_in, self.rate);
        }
        let mut voices = self.lock();
        if voices.len() >= MAX_VOICES {
            return Err(PlayError::TooManyVoices);
        }
        voices.push(v); // within reserved capacity: no reallocation
        Ok(VoiceId(id))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Voice>> {
        self.shared
            .voices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn with_voice<R>(&self, id: VoiceId, f: impl FnOnce(&mut Voice, u32) -> R) -> Option<R> {
        let rate = self.rate;
        let mut voices = self.lock();
        voices
            .iter_mut()
            .find(|v| v.id == id.0 && v.finished.is_none())
            .map(|v| f(v, rate))
    }

    /// Stop a voice, fading out over `fade` (at least [`DECLICK`]). Stopping
    /// one that already ended is fine.
    pub fn stop(&self, id: VoiceId, fade: Duration) {
        self.with_voice(id, |v, rate| {
            if v.paused {
                v.finished = Some(Finish::Stopped);
            } else {
                v.stop_at_zero = true;
                v.ramp_to(0.0, fade.max(DECLICK), rate);
            }
        });
    }

    /// Pause, with a de-click fade. Position is kept.
    pub fn pause(&self, id: VoiceId) {
        self.with_voice(id, |v, rate| {
            if !v.paused {
                v.pause_at_zero = true;
                v.ramp_to(0.0, DECLICK, rate);
            }
        });
    }

    /// Resume a paused voice, ramping back to `gain`.
    pub fn resume(&self, id: VoiceId, gain: f32) {
        self.with_voice(id, |v, rate| {
            v.paused = false;
            v.pause_at_zero = false;
            v.ramp_to(gain.max(0.0), DECLICK, rate);
        });
    }

    /// Jump to `at` (clamped to the end).
    pub fn seek(&self, id: VoiceId, at: Duration) {
        self.with_voice(id, |v, rate| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a frame index"
            )]
            let f = (at.as_secs_f64() * f64::from(rate)) as usize;
            v.frame = f.min(v.frames());
            if let VoiceSrc::Stream(sv) = &mut v.src {
                sv.seek(v.frame as u64);
            }
        });
    }

    /// Change a voice's gain over `fade`.
    pub fn set_gain(&self, id: VoiceId, gain: f32, fade: Duration) {
        self.with_voice(id, |v, rate| {
            if v.paused {
                v.target = gain.max(0.0);
            } else {
                v.ramp_to(gain.max(0.0), fade, rate);
            }
        });
    }

    /// Turn looping on or off.
    pub fn set_looping(&self, id: VoiceId, on: bool) {
        self.with_voice(id, |v, _| {
            v.looping = on;
            if let VoiceSrc::Stream(sv) = &v.src {
                sv.disk.set_looping(on);
            }
        });
    }

    /// The engine-wide gain applied after mixing.
    pub fn set_master_gain(&self, gain: f32) {
        self.shared
            .master
            .store(gain.max(0.0).to_bits(), Ordering::Relaxed);
    }

    /// Where a voice is, or `None` once it has finished.
    #[must_use]
    pub fn position(&self, id: VoiceId) -> Option<Duration> {
        self.with_voice(id, |v, rate| {
            #[expect(clippy::cast_precision_loss, reason = "a frame index")]
            Duration::from_secs_f64(v.frame as f64 / f64::from(rate))
        })
    }

    /// Whether a voice exists and is not paused.
    #[must_use]
    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.with_voice(id, |v, _| !v.paused).unwrap_or(false)
    }

    /// Remove finished voices and return what happened since the last call.
    pub fn poll_events(&self) -> Vec<PlaybackEvent> {
        self.reap();
        std::mem::take(
            &mut *self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Events for one voice only; the rest stay queued.
    pub fn take_events_for(&self, id: VoiceId) -> Vec<PlaybackEvent> {
        self.reap();
        let mut all = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mine, rest): (Vec<_>, Vec<_>) = all.drain(..).partition(|e| match e {
            PlaybackEvent::Ended(v) | PlaybackEvent::Stopped(v) => *v == id,
        });
        *all = rest;
        mine
    }

    /// Finished voices leave here, on the control thread, so their buffers
    /// are freed off the audio thread.
    fn reap(&self) {
        let mut done = Vec::new();
        {
            let mut voices = self.lock();
            let mut i = 0;
            while i < voices.len() {
                if let Some(f) = voices[i].finished {
                    let v = voices.swap_remove(i);
                    done.push(match f {
                        Finish::Ended => PlaybackEvent::Ended(VoiceId(v.id)),
                        Finish::Stopped => PlaybackEvent::Stopped(VoiceId(v.id)),
                    });
                    drop(v);
                } else {
                    i += 1;
                }
            }
        }
        if !done.is_empty() {
            self.events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend(done);
        }
    }

    /// Counters.
    #[must_use]
    pub fn stats(&self) -> EngineStats {
        EngineStats {
            frames: self.shared.frames.load(Ordering::Relaxed),
            contended: self.shared.contended.load(Ordering::Relaxed),
            voices: self.lock().iter().filter(|v| v.finished.is_none()).count(),
        }
    }
}

/// The audio callback. No allocation, no blocking.
fn render_into(shared: &Shared, out: &mut [f32], channels: usize, rate: u32) {
    out.fill(0.0);
    let frames = out.len() / channels;
    shared.frames.fetch_add(frames as u64, Ordering::Relaxed);
    let Ok(mut voices) = shared.voices.try_lock() else {
        shared.contended.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if shared.session.suspended() {
        // Paused by focus: silence, and nothing advances.
        return;
    }
    let master = f32::from_bits(shared.master.load(Ordering::Relaxed));
    let duck_to = shared.session.duck_gain();
    let duck_from = f32::from_bits(
        shared
            .applied_duck
            .swap(duck_to.to_bits(), Ordering::Relaxed),
    );
    let duck_step = if frames > 0 {
        (duck_to - duck_from) / frames as f32
    } else {
        0.0
    };
    // Source frames per output frame. 1 unless the device was reopened at
    // another rate than the engine's (a Bluetooth headset switching to its
    // 16 kHz call profile, say): then the voices are stepped through at
    // the ratio, linearly interpolated, so pitch and speed stay right.
    #[expect(clippy::cast_precision_loss, reason = "a rate ratio")]
    let step = if rate == 0 || rate == shared.rate {
        None
    } else {
        Some(shared.rate as f32 / rate as f32)
    };
    for v in voices.iter_mut() {
        if v.finished.is_some() || v.paused {
            continue;
        }
        if matches!(v.src, VoiceSrc::Stream(_)) {
            render_stream(
                v,
                out,
                frames,
                channels,
                master,
                (duck_from, duck_step),
                step,
            );
            continue;
        }
        let VoiceSrc::Mem(samples) = &v.src else {
            continue;
        };
        let samples = Arc::clone(samples);
        let src_ch = usize::from(samples.channels.max(1));
        let total = v.frames();
        for f in 0..frames {
            if v.frame >= total {
                if v.looping && total > 0 {
                    v.frame = 0;
                } else {
                    v.finished = Some(Finish::Ended);
                    break;
                }
            }
            // Gain ramp toward the target.
            if v.ramp_step() {
                break;
            }
            let base = v.frame * src_ch;
            let g = v.gain * master * (duck_from + duck_step * (f + 1) as f32);
            match step {
                None => {
                    for c in 0..channels.min(2) {
                        let s = samples.data[base + c.min(src_ch - 1)];
                        out[f * channels + c] += s * g;
                    }
                    v.frame += 1;
                }
                Some(step) => {
                    let next = (v.frame + 1).min(total - 1) * src_ch;
                    for c in 0..channels.min(2) {
                        let k = c.min(src_ch - 1);
                        let a = samples.data[base + k];
                        let b = samples.data[next + k];
                        out[f * channels + c] += (a + (b - a) * v.frac) * g;
                    }
                    v.frac += step;
                    while v.frac >= 1.0 {
                        v.frac -= 1.0;
                        v.frame += 1;
                    }
                }
            }
        }
    }
}

/// Frames a streamed voice reads from its ring at a time.
const STREAM_BLOCK: usize = 1_024;

/// Mix one streamed voice into `out`. A stream that has nothing buffered
/// yet (an underrun: the disk fell behind) contributes silence and does
/// not advance; one that has played out ends.
fn render_stream(
    v: &mut Voice,
    out: &mut [f32],
    frames: usize,
    channels: usize,
    master: f32,
    (duck_from, duck_step): (f32, f32),
    step: Option<f32>,
) {
    for f in 0..frames {
        if v.ramp_step() {
            return;
        }
        let VoiceSrc::Stream(sv) = &mut v.src else {
            return;
        };
        let frame = match step {
            None => sv.pull(),
            Some(step) => {
                if !sv.primed {
                    if let (Some(a), Some(b)) = (sv.pull(), sv.pull()) {
                        sv.a = a;
                        sv.b = b;
                        sv.primed = true;
                        v.frac = 0.0;
                    }
                }
                if sv.primed {
                    let t = v.frac;
                    let fr = [
                        sv.a[0] + (sv.b[0] - sv.a[0]) * t,
                        sv.a[1] + (sv.b[1] - sv.a[1]) * t,
                    ];
                    v.frac += step;
                    let mut ok = true;
                    while v.frac >= 1.0 {
                        v.frac -= 1.0;
                        sv.a = sv.b;
                        match sv.pull() {
                            Some(b) => sv.b = b,
                            None => {
                                ok = false;
                                sv.primed = false;
                                break;
                            }
                        }
                    }
                    if ok || !sv.disk.is_finished() {
                        Some(fr)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };
        let Some(fr) = frame else {
            if sv.disk.is_finished() {
                v.finished = Some(Finish::Ended);
            }
            return;
        };
        let g = v.gain * master * (duck_from + duck_step * (f + 1) as f32);
        for c in 0..channels.min(2) {
            out[f * channels + c] += fr[c] * g;
        }
        v.frame += 1;
        if sv.disk.frames() > 0 && v.frame as u64 >= sv.disk.frames() && v.looping {
            v.frame = 0;
        }
    }
}

// ── the AudioPlayer seam (AU-01) ────────────────────────────────────────

/// The [`AudioPlayer`] a platform crate registers. The device is opened on
/// the first `play`, not at registration, so an app that never makes a
/// sound never touches the audio system.
pub struct EnginePlayer {
    engine: std::cell::OnceCell<Result<Arc<Engine>, String>>,
    open: Box<dyn Fn() -> Result<Engine, BackendError>>,
}

impl std::fmt::Debug for EnginePlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnginePlayer")
            .field("engine", &self.engine.get())
            .finish_non_exhaustive()
    }
}

impl Default for EnginePlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl EnginePlayer {
    /// The platform default output, opened lazily.
    #[must_use]
    pub fn new() -> Self {
        Self::with_opener(Engine::open_default)
    }

    /// Open with `open` on first use (tests pass a detached engine).
    #[must_use]
    pub fn with_opener(open: impl Fn() -> Result<Engine, BackendError> + 'static) -> Self {
        Self {
            engine: std::cell::OnceCell::new(),
            open: Box::new(open),
        }
    }

    /// The engine, opening it if this is the first call.
    ///
    /// # Errors
    /// Why no output could be opened.
    pub fn engine(&self) -> Result<Arc<Engine>, ServiceError> {
        match self
            .engine
            .get_or_init(|| (self.open)().map(Arc::new).map_err(|e| e.to_string()))
        {
            Ok(e) => Ok(Arc::clone(e)),
            Err(_) => Err(ServiceError::unsupported("AudioPlayer")),
        }
    }

    /// Register a lazily-opened `EnginePlayer` as the app's `AudioPlayer`.
    pub fn register(services: &mut vieww_foundation::service::Services) {
        services.provide::<dyn AudioPlayer>(Rc::new(Self::new()));
    }
}

impl AudioPlayer for EnginePlayer {
    fn play(&self, sound: &Sound) -> Result<PlaybackHandle, ServiceError> {
        let engine = self.engine()?;
        let samples = crate::device::render_sound(sound, engine.rate());
        engine
            .play(&samples, VoiceOptions::default())
            .map(|v| PlaybackHandle(v.0))
            .map_err(|_| ServiceError::unsupported("AudioPlayer"))
    }

    fn stop(&self, handle: PlaybackHandle) -> Result<(), ServiceError> {
        self.engine()?.stop(VoiceId(handle.0), DECLICK);
        Ok(())
    }
}

// ── the media player facade (AU-11) ─────────────────────────────────────

/// What a [`MediaPlayer`] plays.
#[derive(Debug, Clone)]
pub enum Source {
    /// Decoded samples in hand.
    Samples(Samples),
    /// A file, decoded by [`codec::decode`](crate::codec::decode).
    File(PathBuf),
    /// Encoded bytes (WAV, FLAC, MP3, AAC, Ogg Vorbis…).
    Bytes(Vec<u8>),
    /// A file streamed from disk as it plays, whatever its length (AU-05).
    /// [`Source::File`] streams by itself once a file would decode to more
    /// than [`STREAM_ABOVE_BYTES`].
    Stream(PathBuf),
}

/// Decoded size above which [`MediaPlayer::load`] streams a
/// [`Source::File`] instead of holding it in memory: 32 MB of `f32`, about
/// three minutes of 44.1 kHz stereo.
pub const STREAM_ABOVE_BYTES: u64 = 32 << 20;

/// What a [`MediaPlayer`] has loaded.
#[derive(Debug, Clone)]
enum Track {
    Mem(Arc<Samples>),
    Stream {
        path: PathBuf,
        duration: Duration,
        /// Integrated loudness and true peak, measured on first need.
        measured: Option<(f64, f64)>,
    },
}

/// Integrated loudness (LUFS) and true peak (dBTP) of a file, decoded a
/// piece at a time so a long file never sits in memory whole.
fn measure_stream(path: &std::path::Path) -> Option<(f64, f64)> {
    let mut d = crate::codec::stream::open_file(path).ok()?;
    let layout = crate::channels::Layout::for_channels(usize::from(d.channels().max(1)));
    let mut meter = crate::meter::LoudnessMeter::new(layout, d.rate());
    let mut buf = vec![0f32; 8_192 * usize::from(d.channels().max(1))];
    loop {
        match d.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => meter.push(&buf[..n]),
        }
    }
    Some((meter.integrated(), meter.true_peak()))
}

/// Track mode's gain from a measured loudness and peak.
fn gain_from(integrated: f64, peak: f64, target_lufs: f64) -> f64 {
    if !integrated.is_finite() {
        return 0.0;
    }
    let gain = target_lufs - integrated;
    if peak.is_finite() {
        gain.min(PEAK_CEILING_DBTP - peak)
    } else {
        gain
    }
}

/// Where a [`MediaPlayer`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaState {
    /// Nothing loaded.
    Empty,
    /// Loaded, not playing.
    Paused,
    /// Playing.
    Playing,
    /// Reached the end.
    Ended,
}

/// Loudness normalization for a [`MediaPlayer`] (AU-25): tracks mastered
/// at very different levels play at one level, the way Sound Check and
/// streaming services do it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Normalization {
    /// Play as mastered.
    #[default]
    Off,
    /// Measure each track's EBU R128 integrated loudness on load and bring
    /// it to `target_lufs` ([`DEFAULT_TARGET_LUFS`] is a good default).
    /// Gain upward is limited so the true peak stays at or below −1 dBTP.
    Track {
        /// The loudness to play at, in LUFS.
        target_lufs: f64,
    },
    /// One fixed gain for every track — an album's, from [`album_gain_db`],
    /// so the album's own dynamics between tracks survive.
    Album {
        /// The gain, in dB.
        gain_db: f64,
    },
    /// The file's own ReplayGain / R128 tags (album gain when `album` and
    /// the file has one, else track gain), moved from the ReplayGain
    /// reference to `target_lufs`, limited by the tagged peak to −1 dBTP.
    /// A file without tags is measured, as [`Normalization::Track`] does.
    Tags {
        /// Prefer the album gain.
        album: bool,
        /// The loudness to play at, in LUFS.
        target_lufs: f64,
    },
}

/// −16 LUFS: the level Apple Music and most podcast guidance play at.
pub const DEFAULT_TARGET_LUFS: f64 = -16.0;

/// The highest true peak normalization may raise a track to, in dBTP.
const PEAK_CEILING_DBTP: f64 = -1.0;

/// The gain, in dB, that brings `tracks` *as one programme* to
/// `target_lufs` — album mode's single gain (AU-25).
#[must_use]
pub fn album_gain_db(tracks: &[&Samples], target_lufs: f64) -> f64 {
    let Some(first) = tracks.first() else {
        return 0.0;
    };
    let layout = crate::channels::Layout::for_channels(usize::from(first.channels.max(1)));
    let mut meter = crate::meter::LoudnessMeter::new(layout, first.rate);
    for t in tracks {
        if t.channels == first.channels && t.rate == first.rate {
            meter.push(&t.data);
        }
    }
    let i = meter.integrated();
    if i.is_finite() {
        target_lufs - i
    } else {
        0.0
    }
}

/// Track mode's gain for `s`: to the target, but never pushing the true
/// peak above the ceiling.
fn track_gain_db(s: &Samples, target_lufs: f64) -> f64 {
    let layout = crate::channels::Layout::for_channels(usize::from(s.channels.max(1)));
    let (integrated, _, peak) = crate::meter::measure(&s.data, layout, s.rate);
    if !integrated.is_finite() {
        return 0.0;
    }
    let gain = target_lufs - integrated;
    if peak.is_finite() {
        gain.min(PEAK_CEILING_DBTP - peak)
    } else {
        gain
    }
}

/// One track's transport: the shape a player UI binds to.
#[derive(Debug)]
pub struct MediaPlayer {
    engine: Arc<Engine>,
    samples: Option<Track>,
    voice: Option<VoiceId>,
    volume: f32,
    looping: bool,
    /// Position while there is no voice (before play, after end).
    parked: Duration,
    state: MediaState,
    normalization: Normalization,
    /// The loaded track's normalization gain, linear (1 when off).
    norm_gain: f32,
    /// The loaded file's loudness tags (empty for decoded samples).
    tagged: crate::codec::tags::LoudnessTags,
    /// What focus `play` asks for (AU-02).
    focus: FocusRequest,
    /// What the lock screen shows (AU-04); empty title publishes nothing.
    now: NowPlaying,
}

impl MediaPlayer {
    /// A player on `engine`.
    #[must_use]
    pub const fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            samples: None,
            voice: None,
            volume: 1.0,
            looping: false,
            parked: Duration::ZERO,
            state: MediaState::Empty,
            normalization: Normalization::Off,
            norm_gain: 1.0,
            tagged: crate::codec::tags::LoudnessTags {
                track_gain_db: None,
                track_peak: None,
                album_gain_db: None,
                album_peak: None,
            },
            focus: FocusRequest {
                content: crate::focus::Content::Media,
                gain: crate::focus::GainType::Gain,
                duck_db: -12.0,
            },
            now: NowPlaying {
                title: String::new(),
                artist: String::new(),
                album: String::new(),
                duration: None,
                position: Duration::ZERO,
                playing: false,
            },
        }
    }

    /// What `play` asks the platform for: content kind (speech pauses
    /// rather than ducks), focus length, duck depth.
    pub fn set_focus_request(&mut self, request: FocusRequest) {
        self.focus = request;
    }

    /// What the lock screen and system media controls show for the loaded
    /// track (AU-04). Published on every play, pause, seek and stop.
    pub fn set_metadata(&mut self, title: &str, artist: &str, album: &str) {
        self.now.title = title.to_owned();
        self.now.artist = artist.to_owned();
        self.now.album = album.to_owned();
        self.publish();
    }

    fn publish(&mut self) {
        if self.now.title.is_empty() {
            return;
        }
        self.now.duration = self.samples.as_ref().map(|_| self.duration());
        self.now.position = self.position_now();
        self.now.playing = self.state == MediaState::Playing;
        self.engine.session().publish(&self.now);
    }

    fn position_now(&self) -> Duration {
        self.voice
            .and_then(|v| self.engine.position(v))
            .unwrap_or(self.parked)
    }

    /// Whether playback is held by the platform: a call, another app's
    /// focus, headphones unplugged. [`Self::state`] still says `Playing`
    /// for a resumable pause, which ends by itself.
    #[must_use]
    pub fn interrupted(&self) -> bool {
        self.engine.session().paused_by_focus()
    }

    /// Apply the lock screen's and headset's commands (AU-04). Play, pause,
    /// toggle, stop and seeks are handled here; next and previous are the
    /// app's and are returned.
    pub fn poll_remote(&mut self) -> Vec<RemoteCommand> {
        let mut rest = Vec::new();
        for c in self.engine.session().take_remote() {
            match c {
                RemoteCommand::Play => {
                    let _ = self.play();
                }
                RemoteCommand::Pause => self.pause(),
                RemoteCommand::TogglePlayPause => {
                    if self.state() == MediaState::Playing && !self.interrupted() {
                        self.pause();
                    } else {
                        let _ = self.play();
                    }
                }
                RemoteCommand::Stop => self.stop(),
                RemoteCommand::SeekTo(at) => self.seek(at),
                RemoteCommand::SkipForward(by) => {
                    let at = self.position() + by;
                    self.seek(at);
                }
                RemoteCommand::SkipBackward(by) => {
                    let at = self.position().saturating_sub(by);
                    self.seek(at);
                }
                RemoteCommand::Next | RemoteCommand::Previous => rest.push(c),
            }
        }
        rest
    }

    /// Turn loudness normalization on or off (AU-25). Applies to the
    /// loaded track at once, and to every track loaded after.
    pub fn set_normalization(&mut self, mode: Normalization) {
        self.normalization = mode;
        self.update_norm_gain();
        if let Some(v) = self.voice {
            self.engine.set_gain(v, self.effective_gain(), DECLICK);
        }
    }

    /// The normalization mode last set.
    #[must_use]
    pub const fn normalization(&self) -> Normalization {
        self.normalization
    }

    /// The gain normalization applies to the loaded track, in dB (0 when
    /// off or nothing is loaded).
    #[must_use]
    pub fn normalization_gain_db(&self) -> f64 {
        20.0 * f64::from(self.norm_gain).log10()
    }

    fn update_norm_gain(&mut self) {
        let loaded = self.samples.is_some();
        let mut measured_gain = |target_lufs: f64| -> f64 {
            match &mut self.samples {
                Some(Track::Mem(s)) => track_gain_db(s, target_lufs),
                Some(Track::Stream { path, measured, .. }) => {
                    if measured.is_none() {
                        *measured = Some(measure_stream(path).unwrap_or((f64::NAN, f64::NAN)));
                    }
                    let (i, p) = measured.unwrap_or((f64::NAN, f64::NAN));
                    gain_from(i, p, target_lufs)
                }
                None => 0.0,
            }
        };
        let db = match (self.normalization, loaded) {
            (Normalization::Off, _) | (_, false) => 0.0,
            (Normalization::Track { target_lufs }, true) => measured_gain(target_lufs),
            (Normalization::Album { gain_db }, true) => gain_db,
            (Normalization::Tags { album, target_lufs }, true) => {
                let t = self.tagged;
                let (gain, peak) = if album && t.album_gain_db.is_some() {
                    (t.album_gain_db, t.album_peak)
                } else {
                    (t.track_gain_db, t.track_peak)
                };
                match gain {
                    Some(g) => {
                        let g = g + (target_lufs - crate::codec::tags::REPLAYGAIN_REFERENCE_LUFS);
                        match peak.filter(|p| *p > 0.0) {
                            Some(p) => g.min(PEAK_CEILING_DBTP - 20.0 * p.log10()),
                            None => g,
                        }
                    }
                    None => measured_gain(target_lufs),
                }
            }
        };
        #[expect(clippy::cast_possible_truncation, reason = "a gain")]
        let lin = 10f64.powf(db / 20.0) as f32;
        self.norm_gain = lin;
    }

    fn effective_gain(&self) -> f32 {
        self.volume * self.norm_gain
    }

    /// Load `source`, replacing what was loaded. Starts paused at zero.
    ///
    /// # Errors
    /// The file could not be read or decoded, or it is empty.
    pub fn load(&mut self, source: Source) -> Result<(), PlayError> {
        let mut tagged = crate::codec::tags::LoudnessTags::default();
        let track = match source {
            Source::Samples(s) => Track::Mem(self.engine.prepare(&s)?),
            Source::Stream(p) => {
                tagged = file_tags(&p);
                stream_track(p)?
            }
            Source::File(p) => {
                if decoded_size(&p).is_some_and(|n| n > STREAM_ABOVE_BYTES) {
                    tagged = file_tags(&p);
                    stream_track(p)?
                } else {
                    let bytes = std::fs::read(&p)
                        .map_err(|e| PlayError::Media(format!("{}: {e}", p.display())))?;
                    tagged = crate::codec::tags::read(&bytes).loudness();
                    Track::Mem(self.engine.prepare(&decode(&bytes)?)?)
                }
            }
            Source::Bytes(b) => {
                tagged = crate::codec::tags::read(&b).loudness();
                Track::Mem(self.engine.prepare(&decode(&b)?)?)
            }
        };
        self.unload();
        self.samples = Some(track);
        self.tagged = tagged;
        self.update_norm_gain();
        self.parked = Duration::ZERO;
        self.state = MediaState::Paused;
        Ok(())
    }

    /// Whether the loaded track streams from disk rather than sitting in
    /// memory.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        matches!(self.samples, Some(Track::Stream { .. }))
    }

    fn unload(&mut self) {
        if let Some(v) = self.voice.take() {
            self.engine.stop(v, DECLICK);
        }
        self.samples = None;
        self.state = MediaState::Empty;
    }

    /// Start or resume.
    ///
    /// # Errors
    /// Nothing loaded, or the engine is full.
    pub fn play(&mut self) -> Result<(), PlayError> {
        self.refresh();
        let Some(track) = self.samples.clone() else {
            return Err(PlayError::Empty);
        };
        if !self.engine.session().begin(self.focus) {
            return Err(PlayError::FocusDenied);
        }
        match self.voice {
            Some(v) => self.engine.resume(v, self.effective_gain()),
            None => {
                let at = if self.state == MediaState::Ended {
                    Duration::ZERO
                } else {
                    self.parked
                };
                let opts = VoiceOptions {
                    gain: self.effective_gain(),
                    looping: self.looping,
                    start_at: at,
                    ..VoiceOptions::default()
                };
                self.voice = Some(match track {
                    Track::Mem(samples) => self.engine.play_shared(samples, opts)?,
                    Track::Stream { path, .. } => self.engine.play_streaming(path, opts)?,
                });
            }
        }
        self.state = MediaState::Playing;
        self.publish();
        Ok(())
    }

    /// Pause, keeping the position.
    pub fn pause(&mut self) {
        self.refresh();
        if let Some(v) = self.voice {
            self.engine.pause(v);
        }
        if self.state == MediaState::Playing {
            self.state = MediaState::Paused;
        }
        self.engine.session().end();
        self.publish();
    }

    /// Stop and rewind to the start.
    pub fn stop(&mut self) {
        if let Some(v) = self.voice.take() {
            self.engine.stop(v, DECLICK);
        }
        self.parked = Duration::ZERO;
        if self.samples.is_some() {
            self.state = MediaState::Paused;
        }
        self.engine.session().end();
        self.publish();
    }

    /// Jump to `at` (clamped to the duration).
    pub fn seek(&mut self, at: Duration) {
        let at = at.min(self.duration());
        match self.voice {
            Some(v) => self.engine.seek(v, at),
            None => self.parked = at,
        }
        if self.state == MediaState::Ended {
            self.state = MediaState::Paused;
            self.parked = at;
        }
        self.publish();
    }

    /// Set the volume (linear), fading over `fade`.
    pub fn set_volume(&mut self, volume: f32, fade: Duration) {
        self.volume = volume.max(0.0);
        if let Some(v) = self.voice {
            self.engine.set_gain(v, self.effective_gain(), fade);
        }
    }

    /// The volume last set.
    #[must_use]
    pub const fn volume(&self) -> f32 {
        self.volume
    }

    /// Loop at the end instead of ending.
    pub fn set_looping(&mut self, on: bool) {
        self.looping = on;
        if let Some(v) = self.voice {
            self.engine.set_looping(v, on);
        }
    }

    /// The loaded track's length.
    #[must_use]
    pub fn duration(&self) -> Duration {
        match &self.samples {
            None => Duration::ZERO,
            Some(Track::Stream { duration, .. }) => *duration,
            Some(Track::Mem(s)) => {
                let frames = s.data.len() / usize::from(s.channels.max(1));
                #[expect(clippy::cast_precision_loss, reason = "a frame count")]
                Duration::from_secs_f64(frames as f64 / f64::from(s.rate.max(1)))
            }
        }
    }

    /// Where playback is.
    #[must_use]
    pub fn position(&mut self) -> Duration {
        self.refresh();
        match (self.voice, self.state) {
            (_, MediaState::Ended) => self.duration(),
            (Some(v), _) => self.engine.position(v).unwrap_or(self.parked),
            (None, _) => self.parked,
        }
    }

    /// The transport state.
    #[must_use]
    pub fn state(&mut self) -> MediaState {
        self.refresh();
        self.state
    }

    /// What happened since the last call (an `Ended` is what a UI waits for).
    pub fn events(&mut self) -> Vec<PlaybackEvent> {
        let Some(v) = self.voice else {
            return Vec::new();
        };
        let ev = self.engine.take_events_for(v);
        self.apply(&ev);
        ev
    }

    fn refresh(&mut self) {
        if let Some(v) = self.voice {
            let ev = self.engine.take_events_for(v);
            self.apply(&ev);
            // Put them back for `events()` to report.
            if !ev.is_empty() {
                self.engine
                    .events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .extend(ev);
                self.voice = Some(v);
            }
        }
    }

    fn apply(&mut self, ev: &[PlaybackEvent]) {
        for e in ev {
            if let PlaybackEvent::Ended(_) = e {
                self.state = MediaState::Ended;
                self.parked = Duration::ZERO;
            }
        }
    }
}

/// The size `path` would decode to, in bytes of `f32`, when its format
/// says how long it is.
fn decoded_size(path: &std::path::Path) -> Option<u64> {
    let d = crate::codec::stream::open_file(path).ok()?;
    Some(d.total_frames()? * u64::from(d.channels()) * 4)
}

/// A streamed track for `path`: checked to open, with its duration.
fn stream_track(path: PathBuf) -> Result<Track, PlayError> {
    let d = crate::codec::stream::open_file(&path)
        .map_err(|e| PlayError::Media(format!("{}: {e}", path.display())))?;
    let frames = d.total_frames().unwrap_or(0);
    if frames == 0 && d.total_frames().is_some() {
        return Err(PlayError::Empty);
    }
    #[expect(clippy::cast_precision_loss, reason = "a frame count")]
    let duration = Duration::from_secs_f64(frames as f64 / f64::from(d.rate().max(1)));
    Ok(Track::Stream {
        path,
        duration,
        measured: None,
    })
}

/// Loudness tags of a file too long to read whole: the tag blocks live in
/// the first and last few hundred KB (ID3v2, Vorbis comments, FLAC
/// metadata and a front `moov` at the head; ID3v1 and APE at the tail).
fn file_tags(path: &std::path::Path) -> crate::codec::tags::LoudnessTags {
    use std::io::{Read, Seek, SeekFrom};
    const HEAD: u64 = 1 << 20;
    let Ok(mut f) = std::fs::File::open(path) else {
        return crate::codec::tags::LoudnessTags::default();
    };
    let len = f.metadata().map_or(0, |m| m.len());
    let mut head = Vec::new();
    let _ = (&mut f).take(HEAD).read_to_end(&mut head);
    let from_head = crate::codec::tags::read(&head).loudness();
    if from_head.track_gain_db.is_some() || len <= HEAD {
        return from_head;
    }
    let mut tail = Vec::new();
    if f.seek(SeekFrom::Start(len.saturating_sub(HEAD))).is_ok() {
        let _ = f.read_to_end(&mut tail);
    }
    let from_tail = crate::codec::tags::read(&tail).loudness();
    if from_tail.track_gain_db.is_some() {
        from_tail
    } else {
        from_head
    }
}

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        if let Some(v) = self.voice.take() {
            self.engine.stop(v, DECLICK);
        }
    }
}

fn decode(bytes: &[u8]) -> Result<Samples, PlayError> {
    let d = crate::codec::decode(bytes).map_err(|e| PlayError::Media(format!("{e:?}")))?;
    // `Samples` carries at most stereo: wider layouts fold down with the
    // standard coefficients (AU-23).
    Ok(d.to_samples_downmixed())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000;

    fn dc(level: f32, frames: usize) -> Samples {
        Samples {
            data: vec![level; frames],
            channels: 1,
            rate: RATE,
        }
    }

    fn pull(e: &Engine, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        e.render(&mut out);
        out
    }

    /// AU-25: two tracks mastered 10 dB apart play at the same loudness,
    /// within 0.5 LU of the target; album mode keeps their difference.
    #[test]
    fn normalization_levels_tracks_and_album_mode_keeps_their_difference() {
        use crate::channels::Layout;
        let rate = 48_000u32;
        let tone = |amp: f32| {
            let data: Vec<f32> = (0..rate as usize * 3)
                .map(|i| {
                    #[expect(clippy::cast_precision_loss, reason = "a sample index")]
                    let t = i as f32 / rate as f32;
                    amp * (std::f32::consts::TAU * 997.0 * t).sin()
                })
                .collect();
            Samples {
                data,
                channels: 1,
                rate,
            }
        };
        let (loud, quiet) = (tone(0.5), tone(0.5 * 10f32.powf(-10.0 / 20.0)));
        let played = |s: &Samples, mode: Normalization| {
            let e = Arc::new(Engine::detached(rate, 2));
            let mut p = MediaPlayer::new(Arc::clone(&e));
            p.set_normalization(mode);
            p.load(Source::Samples(s.clone())).unwrap();
            p.play().unwrap();
            let mut out = vec![0.0; rate as usize * 2 * 2];
            e.render(&mut out); // fade-in and settle
            e.render(&mut out);
            crate::meter::measure(&out, Layout::Stereo, rate).0
        };
        let target = -20.0;
        let mode = Normalization::Track {
            target_lufs: target,
        };
        let (a, b) = (played(&loud, mode), played(&quiet, mode));
        let (a_off, b_off) = (
            played(&loud, Normalization::Off),
            played(&quiet, Normalization::Off),
        );
        assert!(
            (a_off - b_off - 10.0).abs() < 0.5,
            "the fixtures are 10 LU apart"
        );
        assert!(
            (a - b).abs() < 0.5,
            "normalized, they play {} LU apart",
            a - b
        );
        // Each moved by exactly what brings its own loudness to the target
        // (the device path's mono-to-stereo step adds the same to both).
        let measured = |s: &Samples| crate::meter::measure(&s.data, Layout::Mono, rate).0;
        assert!(
            (a - a_off - (target - measured(&loud))).abs() < 0.5,
            "loud track moved {}",
            a - a_off
        );
        assert!(
            (b - b_off - (target - measured(&quiet))).abs() < 0.5,
            "quiet track moved {}",
            b - b_off
        );

        let album = album_gain_db(&[&loud, &quiet], target);
        let mode = Normalization::Album { gain_db: album };
        let (a, b) = (played(&loud, mode), played(&quiet, mode));
        assert!(
            (a - b - 10.0).abs() < 0.5,
            "album mode kept {} LU between them",
            a - b
        );

        assert!(
            (b - b_off - album).abs() < 0.5,
            "album mode applied {album} dB"
        );
    }

    /// AU-25: a file's ReplayGain tag decides the gain, moved to the
    /// target; a file without tags is measured instead.
    #[test]
    fn tagged_files_play_at_their_tagged_gain() {
        use crate::codec::tags::{self, Tags};
        let rate = 48_000u32;
        let data: Vec<f32> = (0..rate as usize)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "a sample index")]
                let t = i as f32 / rate as f32;
                0.1 * (std::f32::consts::TAU * 440.0 * t).sin()
            })
            .collect();
        // FLAC: Vorbis comments carry any key, as players store ReplayGain.
        let wav = crate::codec::flac::encode(
            &crate::codec::Decoded {
                channels: 1,
                data,
                rate,
            },
            16,
        )
        .unwrap();
        let mut t = Tags::new();
        t.set_replaygain_track(-4.0, 0.1);
        t.set_replaygain_album(2.0, 0.1);
        let tagged = tags::write_flac(&wav, &t).unwrap();

        let e = Arc::new(Engine::detached(rate, 2));
        let mut p = MediaPlayer::new(Arc::clone(&e));
        p.set_normalization(Normalization::Tags {
            album: false,
            target_lufs: -18.0,
        });
        p.load(Source::Bytes(tagged.clone())).unwrap();
        assert!(
            (p.normalization_gain_db() - -4.0).abs() < 0.01,
            "{}",
            p.normalization_gain_db()
        );
        p.set_normalization(Normalization::Tags {
            album: true,
            target_lufs: -14.0,
        });
        assert!(
            (p.normalization_gain_db() - 6.0).abs() < 0.01,
            "{}",
            p.normalization_gain_db()
        );

        p.load(Source::Bytes(wav)).unwrap();
        let measured = p.normalization_gain_db();
        assert!(
            measured.abs() > 0.5,
            "no tags: measured instead ({measured})"
        );
    }

    /// Upward gain stops short of clipping: a quiet track with one loud
    /// peak is not raised past −1 dBTP.
    #[test]
    fn normalization_never_raises_the_peak_past_the_ceiling() {
        let mut data = vec![0.001f32; 48_000 * 2];
        data[1000] = 0.9;
        let s = Samples {
            data,
            channels: 1,
            rate: 48_000,
        };
        let g = track_gain_db(&s, -10.0);
        let layout = crate::channels::Layout::Mono;
        let peak = crate::meter::measure(&s.data, layout, 48_000).2;
        assert!(
            peak + g <= PEAK_CEILING_DBTP + 1e-9,
            "peak would reach {}",
            peak + g
        );
    }

    /// AU-02/03: a pause by focus freezes the engine on the exact sample,
    /// and the resume carries on from it.
    #[test]
    fn a_focus_pause_freezes_and_resumes_on_the_same_sample() {
        use crate::focus::{FocusEvent, FocusRequest, Interruption};
        let session = Arc::new(AudioSession::new());
        let engine = Arc::new(Engine::detached_with_session(
            48_000,
            2,
            Arc::clone(&session),
        ));
        let mut player = MediaPlayer::new(Arc::clone(&engine));
        let ramp: Vec<f32> = vec![0.5; 48_000];
        player
            .load(Source::Samples(Samples {
                data: ramp,
                channels: 1,
                rate: 48_000,
            }))
            .unwrap();
        player.play().unwrap();
        let mut buf = vec![0.0f32; 2 * 4800];
        engine.render(&mut buf);
        let before = player.position();
        assert!(before >= Duration::from_millis(99));

        assert_eq!(
            session.on_interruption(Interruption::Began),
            crate::focus::Action::Pause
        );
        assert!(player.interrupted());
        for _ in 0..10 {
            engine.render(&mut buf);
            assert!(buf.iter().all(|s| *s == 0.0), "silent while interrupted");
        }
        assert_eq!(player.position(), before, "nothing advanced");

        session.on_interruption(Interruption::Ended {
            should_resume: true,
        });
        engine.render(&mut buf);
        assert!(buf.iter().any(|s| *s != 0.0));
        assert!(player.position() > before);

        // A duck lowers the level; gaining focus restores it.
        let _ = FocusRequest::default();
        engine.render(&mut buf);
        let full = buf[buf.len() - 2].abs();
        session.on_focus(FocusEvent::LostCanDuck);
        engine.render(&mut buf); // the ramp block
        engine.render(&mut buf);
        let ducked = buf[buf.len() - 2].abs();
        assert!(
            ducked < full * 0.3 && ducked > full * 0.2,
            "ducked {ducked} vs {full}"
        );
        session.on_focus(FocusEvent::Gained);
        engine.render(&mut buf);
        engine.render(&mut buf);
        assert!(buf[buf.len() - 2].abs() >= full * 0.99, "back to full");
    }

    /// AU-09: a device reopened at another rate (a Bluetooth headset's
    /// 16 kHz call profile) plays at the right speed: a second of output
    /// at 16 kHz moves the voice a second, not a third of one.
    #[test]
    fn a_device_at_another_rate_keeps_speed() {
        let session = Arc::new(AudioSession::new());
        let engine = Engine::detached_with_session(48_000, 2, session);
        let tone: Vec<f32> = (0..48_000 * 4)
            .map(|i| (i as f32 * 0.01).sin() * 0.5)
            .collect();
        let id = engine
            .play(
                &Samples {
                    data: tone,
                    channels: 1,
                    rate: 48_000,
                },
                VoiceOptions::default(),
            )
            .unwrap();
        let mut buf = vec![0.0f32; 2 * 1_600];
        for _ in 0..10 {
            engine.render_at(&mut buf, 16_000); // 10 x 100 ms
        }
        let pos = engine.position(id).unwrap();
        assert!(
            (pos.as_secs_f64() - 1.0).abs() < 0.01,
            "one second of 16 kHz output is one second of the voice, got {pos:?}"
        );
        assert!(buf.iter().any(|s| *s != 0.0), "and it is not silent");
        // At the engine's own rate, unchanged.
        let before = engine.position(id).unwrap();
        let mut buf = vec![0.0f32; 2 * 4_800];
        engine.render_at(&mut buf, 48_000);
        let moved = engine.position(id).unwrap() - before;
        assert!((moved.as_secs_f64() - 0.1).abs() < 0.001);
    }

    /// AU-02: a refused request does not play.
    #[test]
    fn a_refused_focus_request_fails_play() {
        use crate::focus::FocusRequest;
        use crate::session::SessionBridge;
        struct Busy;
        impl SessionBridge for Busy {
            fn name(&self) -> &'static str {
                "busy"
            }
            fn request_focus(&self, _: &FocusRequest) -> bool {
                false
            }
            fn abandon_focus(&self) {}
        }
        let session = Arc::new(AudioSession::new());
        session.set_bridge(Arc::new(Busy));
        let engine = Arc::new(Engine::detached_with_session(48_000, 2, session));
        let mut player = MediaPlayer::new(engine);
        player
            .load(Source::Samples(Samples {
                data: vec![0.5; 4800],
                channels: 1,
                rate: 48_000,
            }))
            .unwrap();
        assert_eq!(player.play(), Err(PlayError::FocusDenied));
        assert_ne!(player.state(), MediaState::Playing);
    }

    /// AU-04: lock-screen commands drive the player; next/previous are the
    /// app's.
    #[test]
    fn remote_commands_drive_the_player() {
        let session = Arc::new(AudioSession::new());
        let engine = Arc::new(Engine::detached_with_session(
            48_000,
            2,
            Arc::clone(&session),
        ));
        let mut player = MediaPlayer::new(engine);
        player
            .load(Source::Samples(Samples {
                data: vec![0.5; 48_000 * 10],
                channels: 1,
                rate: 48_000,
            }))
            .unwrap();
        session.push_remote(RemoteCommand::Play);
        session.push_remote(RemoteCommand::SeekTo(Duration::from_secs(4)));
        session.push_remote(RemoteCommand::Next);
        assert_eq!(player.poll_remote(), vec![RemoteCommand::Next]);
        assert_eq!(player.state(), MediaState::Playing);
        assert_eq!(player.position(), Duration::from_secs(4));
        session.push_remote(RemoteCommand::TogglePlayPause);
        player.poll_remote();
        assert_eq!(player.state(), MediaState::Paused);
        assert!(!session.holding(), "pausing gave focus back");
    }

    #[test]
    fn plays_mixes_and_ends_with_an_event() {
        let e = Engine::detached(RATE, 2);
        let a = e
            .play(
                &dc(0.25, 100),
                VoiceOptions {
                    fade_in: Duration::ZERO,
                    ..Default::default()
                },
            )
            .unwrap();
        let _b = e
            .play(
                &dc(0.25, 50),
                VoiceOptions {
                    fade_in: Duration::ZERO,
                    ..Default::default()
                },
            )
            .unwrap();
        let block = pull(&e, 10);
        assert!(
            (block[18] - 0.5).abs() < 1e-6,
            "two voices mix: {}",
            block[18]
        );
        pull(&e, 200);
        let ev = e.poll_events();
        assert!(ev.contains(&PlaybackEvent::Ended(a)), "{ev:?}");
        assert_eq!(e.stats().voices, 0);
        assert_eq!(e.position(a), None);
    }

    #[test]
    fn starts_and_stops_without_clicks() {
        let e = Engine::detached(RATE, 2);
        let v = e
            .play(
                &dc(1.0, 1000),
                VoiceOptions {
                    fade_in: Duration::from_millis(10),
                    ..Default::default()
                },
            )
            .unwrap();
        let first = pull(&e, 10);
        // 10 ms at 1 kHz is 10 frames: a ramp, not a step.
        assert!(first[0] < 0.2 && first[18] > 0.8, "{first:?}");
        e.stop(v, Duration::from_millis(10));
        let tail = pull(&e, 20);
        assert!(
            tail[0] > 0.5 && tail[38] == 0.0,
            "fade-out then silence: {tail:?}"
        );
        assert_eq!(e.poll_events(), vec![PlaybackEvent::Stopped(v)]);
    }

    #[test]
    fn pause_resume_keeps_position_within_one_block() {
        let e = Engine::detached(RATE, 2);
        let v = e.play(&dc(1.0, 1000), VoiceOptions::default()).unwrap();
        pull(&e, 100);
        e.pause(v);
        pull(&e, 50);
        let at = e.position(v).unwrap();
        pull(&e, 100); // paused: nothing advances
        assert_eq!(e.position(v).unwrap(), at);
        assert!(pull(&e, 10).iter().all(|s| *s == 0.0));
        let err = at.as_secs_f64() - 0.1;
        assert!(err.abs() <= 0.010, "pause landed {err}s from the request");
        e.resume(v, 1.0);
        pull(&e, 10);
        assert!(e.position(v).unwrap() > at);
    }

    #[test]
    fn seek_gain_loop_and_master() {
        let e = Engine::detached(RATE, 2);
        let v = e
            .play(
                &dc(1.0, 100),
                VoiceOptions {
                    looping: true,
                    fade_in: Duration::ZERO,
                    ..Default::default()
                },
            )
            .unwrap();
        e.seek(v, Duration::from_millis(90));
        pull(&e, 20); // wraps
        assert!(e.position(v).unwrap() < Duration::from_millis(20));
        assert!(e.poll_events().is_empty(), "a loop never ends");
        e.set_gain(v, 0.5, Duration::ZERO);
        e.set_master_gain(0.5);
        let b = pull(&e, 4);
        assert!((b[6] - 0.25).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn voice_limit_and_empty() {
        let e = Engine::detached(RATE, 2);
        for _ in 0..MAX_VOICES {
            e.play(&dc(0.0, 10_000), VoiceOptions::default()).unwrap();
        }
        assert_eq!(
            e.play(&dc(0.0, 10), VoiceOptions::default()),
            Err(PlayError::TooManyVoices)
        );
        assert_eq!(
            e.play(&dc(0.0, 0), VoiceOptions::default()),
            Err(PlayError::Empty)
        );
    }

    #[test]
    fn contention_renders_silence_instead_of_blocking() {
        let e = Engine::detached(RATE, 2);
        e.play(&dc(1.0, 100), VoiceOptions::default()).unwrap();
        let guard = e.shared.voices.lock().unwrap();
        let mut out = vec![1.0; 8];
        render_into(&e.shared, &mut out, 2, RATE);
        drop(guard);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(e.stats().contended, 1);
    }

    #[test]
    fn engine_player_is_the_audio_player() {
        let p = EnginePlayer::with_opener(|| Ok(Engine::detached(RATE, 2)));
        let h = p.play(&Sound::buffer(dc(0.5, 50))).unwrap();
        let engine = p.engine().unwrap();
        assert_eq!(engine.stats().voices, 1);
        p.stop(h).unwrap();
        pull(&engine, 20);
        assert_eq!(
            engine.poll_events(),
            vec![PlaybackEvent::Stopped(VoiceId(h.0))]
        );
    }

    #[test]
    fn engine_player_without_a_device_is_unsupported() {
        let p = EnginePlayer::with_opener(|| Err(BackendError::Unavailable("none".into())));
        assert!(matches!(
            p.play(&Sound::buffer(dc(0.5, 5))),
            Err(ServiceError::Unsupported { .. })
        ));
    }

    #[test]
    fn media_player_transport() {
        let engine = Arc::new(Engine::detached(RATE, 2));
        let mut m = MediaPlayer::new(Arc::clone(&engine));
        assert_eq!(m.state(), MediaState::Empty);
        m.load(Source::Samples(dc(1.0, 500))).unwrap();
        assert_eq!(m.duration(), Duration::from_millis(500));
        m.seek(Duration::from_millis(100));
        m.play().unwrap();
        pull(&engine, 100);
        let p = m.position();
        assert!(
            p >= Duration::from_millis(195) && p <= Duration::from_millis(201),
            "{p:?}"
        );
        m.pause();
        pull(&engine, 20);
        assert_eq!(m.state(), MediaState::Paused);
        m.set_volume(0.5, Duration::ZERO);
        m.play().unwrap();
        pull(&engine, 400);
        assert_eq!(m.events(), vec![PlaybackEvent::Ended(m.voice.unwrap())]);
        assert_eq!(m.state(), MediaState::Ended);
        assert_eq!(m.position(), m.duration());
    }

    #[test]
    fn media_player_decodes_wav_bytes() {
        let wav = crate::wav::write_wav(&Samples {
            data: vec![0.5; 200],
            channels: 2,
            rate: 2000,
        })
        .unwrap();
        let engine = Arc::new(Engine::detached(RATE, 2));
        let mut m = MediaPlayer::new(engine);
        m.load(Source::Bytes(wav)).unwrap();
        assert_eq!(m.duration(), Duration::from_millis(50));
        assert!(m.load(Source::Bytes(b"not audio".to_vec())).is_err());
    }

    /// The device path: a backend that calls the render closure on its own
    /// thread, like a real one.
    #[test]
    fn plays_through_a_backend_stream() {
        use crate::backend::{CaptureFn, Counters, DeviceStream, RenderFn, StreamImpl};
        use std::sync::atomic::AtomicBool;
        struct Thread(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>);
        impl StreamImpl for Thread {
            fn stop(&mut self) {
                self.0.store(true, Ordering::Release);
                if let Some(t) = self.1.take() {
                    t.join().ok();
                }
            }
        }
        struct Fake(Arc<Mutex<f32>>);
        impl Backend for Fake {
            fn name(&self) -> &'static str {
                "fake"
            }
            fn open_output(
                &self,
                config: StreamConfig,
                mut render: RenderFn,
            ) -> Result<DeviceStream, BackendError> {
                let stop = Arc::new(AtomicBool::new(false));
                let (s2, peak) = (Arc::clone(&stop), Arc::clone(&self.0));
                let counters = Arc::new(Counters::default());
                let c2 = Arc::clone(&counters);
                let t = std::thread::spawn(move || {
                    let mut buf = vec![0.0; 64 * usize::from(config.channels)];
                    while !s2.load(Ordering::Acquire) {
                        render(
                            &mut buf,
                            &BlockInfo {
                                frames: 64,
                                channels: usize::from(config.channels),
                                rate: config.rate,
                                time: 0.0,
                            },
                        );
                        c2.block(64);
                        let m = buf.iter().fold(0.0f32, |a, b| a.max(b.abs()));
                        let mut p = peak.lock().unwrap();
                        *p = p.max(m);
                        drop(p);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                });
                Ok(DeviceStream::new(
                    "fake",
                    config,
                    counters,
                    Box::new(Thread(stop, Some(t))),
                ))
            }
            fn open_input(
                &self,
                _: StreamConfig,
                _: CaptureFn,
            ) -> Result<DeviceStream, BackendError> {
                Err(BackendError::Refused("no input".into()))
            }
        }
        let peak = Arc::new(Mutex::new(0.0));
        let e = Engine::with_backend(
            Box::new(Fake(Arc::clone(&peak))),
            StreamConfig {
                rate: 8000,
                channels: 2,
                block: 64,
            },
        )
        .unwrap();
        assert_eq!(e.backend(), "fake");
        let v = e
            .play(
                &Samples {
                    data: vec![0.5; 800],
                    channels: 1,
                    rate: 8000,
                },
                VoiceOptions::default(),
            )
            .unwrap();
        let t0 = std::time::Instant::now();
        while e.poll_events().is_empty() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(*peak.lock().unwrap() > 0.45, "sound reached the device");
        assert_eq!(e.position(v), None, "finished");
        assert!(e.stats().frames >= 800);
    }

    fn long(name: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// Render streamed output, keeping only the frames the voice actually
    /// advanced through (an underrun renders silence without advancing).
    fn drain_stream(e: &Engine, v: VoiceId, max_frames: usize) -> Vec<f32> {
        let mut got = Vec::new();
        let mut block = vec![0f32; 2 * 512];
        let mut idle = 0;
        while got.len() < max_frames * 2 && idle < 2_000 {
            let before = e.with_voice(v, |v, _| v.frame);
            e.render(&mut block);
            let Some(before) = before else { break };
            let after = e.with_voice(v, |v, _| v.frame).unwrap_or(before + 512);
            let n = after.saturating_sub(before).min(512);
            if n == 0 {
                idle += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            got.extend_from_slice(&block[..n * 2]);
        }
        got.truncate(max_frames * 2);
        got
    }

    #[test]
    fn a_streamed_voice_matches_the_same_file_in_memory() {
        // FLAC at 22.05 kHz on a 22.05 kHz engine: no resampling, so the
        // streamed voice must reproduce the in-memory voice exactly.
        let path = long("long.flac");
        let e = Engine::detached(22_050, 2);
        let decoded = crate::codec::decode(&std::fs::read(&path).unwrap()).unwrap();
        let mem = e
            .play(&decoded.to_samples_downmixed(), VoiceOptions::default())
            .unwrap();
        let mut want = vec![0f32; 2 * 22_050];
        e.render(&mut want);
        e.stop(mem, Duration::ZERO);
        let e = Engine::detached(22_050, 2);
        let v = e.play_streaming(&path, VoiceOptions::default()).unwrap();
        let got = drain_stream(&e, v, 22_050);
        assert_eq!(got.len(), want.len());
        let worst = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst < 1e-6, "streamed and in-memory differ by {worst}");
    }

    #[test]
    fn a_media_player_streams_long_files_seeks_and_ends() {
        let e = Arc::new(Engine::detached(48_000, 2));
        let mut p = MediaPlayer::new(Arc::clone(&e));
        p.load(Source::Stream(long("long.ogg"))).unwrap();
        assert!(p.is_streaming());
        let d = p.duration().as_secs_f64();
        assert!((d - 20.0).abs() < 0.05, "duration {d}");
        p.play().unwrap();
        let v = p.voice.unwrap();
        let first = drain_stream(&e, v, 24_000);
        let rms = (first.iter().map(|s| s * s).sum::<f32>() / first.len() as f32).sqrt();
        assert!(rms > 0.1, "streamed audio is there (rms {rms})");
        p.seek(Duration::from_secs(19));
        let at = p.position().as_secs_f64();
        assert!((at - 19.0).abs() < 0.01, "position after seek {at}");
        let mut ended = false;
        for _ in 0..5_000 {
            let mut b = vec![0f32; 2 * 512];
            e.render(&mut b);
            if p.state() == MediaState::Ended {
                ended = true;
                break;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        assert!(ended, "the streamed track played to its end");
    }

    #[test]
    fn short_files_stay_in_memory_and_normalization_measures_streams() {
        let e = Arc::new(Engine::detached(48_000, 2));
        let mut p = MediaPlayer::new(Arc::clone(&e));
        p.load(Source::File(long("long.mp3"))).unwrap();
        assert!(
            !p.is_streaming(),
            "20 s decodes to well under the threshold"
        );
        p.set_normalization(Normalization::Track { target_lufs: -16.0 });
        let in_memory = p.normalization_gain_db();
        p.load(Source::Stream(long("long.mp3"))).unwrap();
        let streamed = p.normalization_gain_db();
        assert!(
            (in_memory - streamed).abs() < 0.5,
            "streamed loudness {streamed} dB vs in-memory {in_memory} dB"
        );
    }
}
