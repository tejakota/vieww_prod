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
use crate::player::{AudioPlayer, PlaybackHandle, Sound};
use crate::resample::Quality;
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
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyVoices => write!(f, "{MAX_VOICES} voices are already playing"),
            Self::Empty => write!(f, "the sound has no samples"),
            Self::Media(e) => write!(f, "{e}"),
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

#[derive(Debug)]
struct Voice {
    id: u64,
    samples: Arc<Samples>,
    frame: usize,
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
        self.samples.data.len() / usize::from(self.samples.channels.max(1))
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
        let stream = ResilientStream::open(
            backend,
            Target::Default,
            config,
            Box::new(move |out: &mut [f32], info: &BlockInfo| {
                render_into(&shared, out, info.channels.max(1), info.rate);
            }),
        )?;
        engine.stream = Some(stream);
        Ok(engine)
    }

    /// An engine with no device: blocks are pulled with [`Engine::render`].
    /// For tests and offline rendering.
    #[must_use]
    pub fn detached(rate: u32, channels: u16) -> Self {
        Self {
            shared: Arc::new(Shared {
                voices: Mutex::new(Vec::with_capacity(MAX_VOICES)),
                frames: AtomicU64::new(0),
                contended: AtomicU64::new(0),
                master: AtomicU32::new(1.0f32.to_bits()),
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
            samples,
            frame: 0,
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
        self.with_voice(id, |v, _| v.looping = on);
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
fn render_into(shared: &Shared, out: &mut [f32], channels: usize, _rate: u32) {
    out.fill(0.0);
    let frames = out.len() / channels;
    shared.frames.fetch_add(frames as u64, Ordering::Relaxed);
    let Ok(mut voices) = shared.voices.try_lock() else {
        shared.contended.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let master = f32::from_bits(shared.master.load(Ordering::Relaxed));
    for v in voices.iter_mut() {
        if v.finished.is_some() || v.paused {
            continue;
        }
        let src_ch = usize::from(v.samples.channels.max(1));
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
            if v.step != 0.0 {
                v.gain += v.step;
                if (v.step > 0.0 && v.gain >= v.target) || (v.step < 0.0 && v.gain <= v.target) {
                    v.gain = v.target;
                    v.step = 0.0;
                }
            }
            if v.step == 0.0 && v.gain <= 0.0 {
                if v.stop_at_zero {
                    v.finished = Some(Finish::Stopped);
                    break;
                }
                if v.pause_at_zero {
                    v.paused = true;
                    v.pause_at_zero = false;
                    break;
                }
            }
            let base = v.frame * src_ch;
            let g = v.gain * master;
            for c in 0..channels.min(2) {
                let s = v.samples.data[base + c.min(src_ch - 1)];
                out[f * channels + c] += s * g;
            }
            v.frame += 1;
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

/// One track's transport: the shape a player UI binds to.
#[derive(Debug)]
pub struct MediaPlayer {
    engine: Arc<Engine>,
    samples: Option<Arc<Samples>>,
    voice: Option<VoiceId>,
    volume: f32,
    looping: bool,
    /// Position while there is no voice (before play, after end).
    parked: Duration,
    state: MediaState,
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
        }
    }

    /// Load `source`, replacing what was loaded. Starts paused at zero.
    ///
    /// # Errors
    /// The file could not be read or decoded, or it is empty.
    pub fn load(&mut self, source: Source) -> Result<(), PlayError> {
        let samples = match source {
            Source::Samples(s) => s,
            Source::File(p) => {
                let bytes = std::fs::read(&p)
                    .map_err(|e| PlayError::Media(format!("{}: {e}", p.display())))?;
                decode(&bytes)?
            }
            Source::Bytes(b) => decode(&b)?,
        };
        self.unload();
        self.samples = Some(self.engine.prepare(&samples)?);
        self.parked = Duration::ZERO;
        self.state = MediaState::Paused;
        Ok(())
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
        let Some(samples) = self.samples.clone() else {
            return Err(PlayError::Empty);
        };
        match self.voice {
            Some(v) => self.engine.resume(v, self.volume),
            None => {
                let at = if self.state == MediaState::Ended {
                    Duration::ZERO
                } else {
                    self.parked
                };
                self.voice = Some(self.engine.play_shared(
                    samples,
                    VoiceOptions {
                        gain: self.volume,
                        looping: self.looping,
                        start_at: at,
                        ..VoiceOptions::default()
                    },
                )?);
            }
        }
        self.state = MediaState::Playing;
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
    }

    /// Set the volume (linear), fading over `fade`.
    pub fn set_volume(&mut self, volume: f32, fade: Duration) {
        self.volume = volume.max(0.0);
        if let Some(v) = self.voice {
            self.engine.set_gain(v, self.volume, fade);
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
        self.samples.as_ref().map_or(Duration::ZERO, |s| {
            let frames = s.data.len() / usize::from(s.channels.max(1));
            #[expect(clippy::cast_precision_loss, reason = "a frame count")]
            Duration::from_secs_f64(frames as f64 / f64::from(s.rate.max(1)))
        })
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

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        if let Some(v) = self.voice.take() {
            self.engine.stop(v, DECLICK);
        }
    }
}

fn decode(bytes: &[u8]) -> Result<Samples, PlayError> {
    let d = crate::codec::decode(bytes).map_err(|e| PlayError::Media(format!("{e:?}")))?;
    // `Samples` carries at most stereo: fold wider layouts down.
    let ch = usize::from(d.channels.max(1));
    if ch <= 2 {
        return Ok(Samples {
            data: d.data,
            channels: d.channels.max(1),
            rate: d.rate,
        });
    }
    #[expect(clippy::cast_precision_loss, reason = "a channel count")]
    let scale = 1.0 / (ch as f32 / 2.0);
    let mut data = Vec::with_capacity(d.data.len() / ch * 2);
    for frame in d.data.chunks_exact(ch) {
        let (mut l, mut r) = (0.0, 0.0);
        for (i, s) in frame.iter().enumerate() {
            if i % 2 == 0 {
                l += s
            } else {
                r += s
            }
        }
        data.push(l * scale);
        data.push(r * scale);
    }
    Ok(Samples {
        data,
        channels: 2,
        rate: d.rate,
    })
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
}
