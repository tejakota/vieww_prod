//! The app's audio session: focus, interruptions, lock-screen controls
//! (AU-02, AU-03, AU-04).
//!
//! There is one [`AudioSession`] per process, [`AudioSession::shared`], just
//! as there is one `AVAudioSession` and one set of Android focus. The engine
//! reads it on every block; the platform crate feeds it.
//!
//! ```text
//!  OS callbacks ──► SessionBridge impl (platform crate)
//!                      │  on_focus / on_interruption / on_route / push_remote
//!                      ▼
//!                 AudioSession ──── FocusState (focus.rs) ──► Action
//!                      │                                       │
//!   MediaPlayer::play ─┘ begin(): asks the bridge for focus     ▼
//!                                      Engine: suspend / duck / resume,
//!                                      ResilientStream: hold reopen
//! ```
//!
//! What each [`Action`] does to the engine:
//!
//! * **Pause** suspends it: the callback writes silence and no voice
//!   advances, so **Resume** carries on from the exact sample;
//! * **Duck** ramps the master to the request's `duck_db` over one block,
//!   **Unduck** ramps it back;
//! * during an interruption the stream is not reopened. On iOS the session
//!   is inactive until the interruption ends, and every open fails; retrying
//!   once a second was the retry storm AU-03 describes.
//!
//! With no bridge registered (desktop, tests) focus is always granted and
//! nothing ever interrupts, so the session is invisible.
//!
//! # Lock screen and remote controls (AU-04)
//!
//! [`NowPlaying`] is what the lock screen, the notification shade, a car or
//! a watch shows. [`RemoteCommand`] is what comes back from their buttons.
//! [`MediaPlayer`](crate::engine_player::MediaPlayer) publishes the first
//! and applies the second (`poll_remote`). A bridge that has no lock screen
//! ignores both.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use crate::focus::{Action, FocusEvent, FocusRequest, FocusState, Interruption, RouteChange};

/// The platform side of the session. Implemented by the platform crate
/// (Android: `AudioManager` + `MediaSession`; iOS: `AVAudioSession` +
/// `MPNowPlayingInfoCenter`).
pub trait SessionBridge: Send + Sync {
    /// For logs: "android", "ios".
    fn name(&self) -> &'static str;
    /// Ask for focus (and activate the session). `true` when granted.
    fn request_focus(&self, request: &FocusRequest) -> bool;
    /// Give focus back (and deactivate, notifying others).
    fn abandon_focus(&self);
    /// Show `now` on the lock screen and in system media controls.
    fn publish(&self, now: &NowPlaying) {
        let _ = now;
    }
    /// Remove the lock-screen entry.
    fn clear_now_playing(&self) {}
}

/// What the system media controls show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    /// The track title.
    pub title: String,
    /// The artist or show.
    pub artist: String,
    /// The album or series.
    pub album: String,
    /// The track length, when known.
    pub duration: Option<Duration>,
    /// Where playback is.
    pub position: Duration,
    /// Playing (rate 1) or paused (rate 0).
    pub playing: bool,
}

/// A button pressed on the lock screen, headset, car or watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCommand {
    /// Play.
    Play,
    /// Pause.
    Pause,
    /// Play/pause toggle (a headset's single button).
    TogglePlayPause,
    /// Stop.
    Stop,
    /// Next track: the app's to handle.
    Next,
    /// Previous track: the app's to handle.
    Previous,
    /// Seek to a position (the lock screen's scrubber).
    SeekTo(Duration),
    /// Skip forward by an amount.
    SkipForward(Duration),
    /// Skip back by an amount.
    SkipBackward(Duration),
}

/// One entry in the session's log: what the device suites assert on.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// Focus was requested; the answer.
    Requested {
        /// Granted or refused.
        granted: bool,
    },
    /// Focus was given back.
    Abandoned,
    /// A focus change arrived.
    Focus(FocusEvent),
    /// An interruption began or ended.
    Interruption(Interruption),
    /// The route changed.
    Route(RouteChange),
    /// The engine was told to do this.
    Action(Action),
    /// A remote command arrived.
    Remote(RemoteCommand),
}

const LOG_DEPTH: usize = 256;

/// The process's audio session.
pub struct AudioSession {
    state: Mutex<FocusState>,
    bridge: RwLock<Option<Arc<dyn SessionBridge>>>,
    suspended: AtomicBool,
    duck: AtomicU32,
    hold_reopen: AtomicBool,
    generation: AtomicU64,
    log: Mutex<VecDeque<SessionEvent>>,
    remote: Mutex<Vec<RemoteCommand>>,
}

impl std::fmt::Debug for AudioSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioSession")
            .field("bridge", &self.bridge_name())
            .field("suspended", &self.suspended())
            .field("duck_gain", &self.duck_gain())
            .field("hold_reopen", &self.hold_reopen())
            .finish_non_exhaustive()
    }
}

impl Default for AudioSession {
    fn default() -> Self {
        Self::new()
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl AudioSession {
    /// A session with no bridge. Engines use [`Self::shared`]; tests make
    /// their own.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FocusState::default()),
            bridge: RwLock::new(None),
            suspended: AtomicBool::new(false),
            duck: AtomicU32::new(1.0f32.to_bits()),
            hold_reopen: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            log: Mutex::new(VecDeque::with_capacity(LOG_DEPTH)),
            remote: Mutex::new(Vec::new()),
        }
    }

    /// The process-wide session.
    pub fn shared() -> &'static Arc<Self> {
        static SHARED: OnceLock<Arc<AudioSession>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(Self::new()))
    }

    /// Register the platform bridge (the platform crate does this at
    /// startup).
    pub fn set_bridge(&self, bridge: Arc<dyn SessionBridge>) {
        if let Ok(mut b) = self.bridge.write() {
            *b = Some(bridge);
        }
    }

    /// The registered bridge's name, if any.
    #[must_use]
    pub fn bridge_name(&self) -> Option<&'static str> {
        self.bridge().map(|b| b.name())
    }

    fn bridge(&self) -> Option<Arc<dyn SessionBridge>> {
        self.bridge.read().ok().and_then(|b| b.clone())
    }

    fn record(&self, e: SessionEvent) {
        if let Ok(mut log) = self.log.lock() {
            if log.len() == LOG_DEPTH {
                log.pop_front();
            }
            log.push_back(e);
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FocusState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn apply(&self, action: Action, duck_db: f32) -> Action {
        match action {
            Action::None => return action,
            Action::Pause => self.suspended.store(true, Ordering::Release),
            Action::Resume => self.suspended.store(false, Ordering::Release),
            Action::Duck => self
                .duck
                .store(db_to_gain(duck_db).to_bits(), Ordering::Release),
            Action::Unduck => self.duck.store(1.0f32.to_bits(), Ordering::Release),
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.record(SessionEvent::Action(action));
        action
    }

    /// The app starts playback. Asks the bridge for focus; returns whether
    /// playback may go ahead (`false` during a call, for instance).
    pub fn begin(&self, request: FocusRequest) -> bool {
        let granted = self.bridge().is_none_or(|b| b.request_focus(&request));
        self.record(SessionEvent::Requested { granted });
        let mut s = self.state();
        s.begin(request);
        let ok = s.granted(granted);
        drop(s);
        if ok {
            self.suspended.store(false, Ordering::Release);
            self.duck.store(1.0f32.to_bits(), Ordering::Release);
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        ok
    }

    /// The app paused or stopped by itself: focus goes back.
    pub fn end(&self) {
        let held = {
            let mut s = self.state();
            let held = s.holding();
            s.end();
            held
        };
        self.suspended.store(false, Ordering::Release);
        self.duck.store(1.0f32.to_bits(), Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel);
        if held {
            if let Some(b) = self.bridge() {
                b.abandon_focus();
            }
            self.record(SessionEvent::Abandoned);
        }
    }

    /// A focus change from the platform.
    pub fn on_focus(&self, event: FocusEvent) -> Action {
        self.record(SessionEvent::Focus(event));
        let (a, db) = {
            let mut s = self.state();
            (s.on_focus(event), s.request().duck_db)
        };
        self.apply(a, db)
    }

    /// An interruption from the platform.
    pub fn on_interruption(&self, i: Interruption) -> Action {
        self.record(SessionEvent::Interruption(i));
        let (a, db, held) = {
            let mut s = self.state();
            let a = s.on_interruption(i);
            (a, s.request().duck_db, s.interrupted())
        };
        self.hold_reopen.store(held, Ordering::Release);
        self.apply(a, db)
    }

    /// A route change from the platform.
    pub fn on_route(&self, r: RouteChange) -> Action {
        self.record(SessionEvent::Route(r));
        let (a, db) = {
            let mut s = self.state();
            (s.on_route(r), s.request().duck_db)
        };
        self.apply(a, db)
    }

    /// A remote command from the platform, queued for the player.
    pub fn push_remote(&self, c: RemoteCommand) {
        self.record(SessionEvent::Remote(c));
        if let Ok(mut q) = self.remote.lock() {
            q.push(c);
        }
    }

    /// Take the queued remote commands.
    pub fn take_remote(&self) -> Vec<RemoteCommand> {
        self.remote
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }

    /// Show `now` in the system media controls.
    pub fn publish(&self, now: &NowPlaying) {
        if let Some(b) = self.bridge() {
            b.publish(now);
        }
    }

    /// Remove the system media controls entry.
    pub fn clear_now_playing(&self) {
        if let Some(b) = self.bridge() {
            b.clear_now_playing();
        }
    }

    /// Whether the engine is suspended by focus (renders silence, frozen).
    #[must_use]
    pub fn suspended(&self) -> bool {
        self.suspended.load(Ordering::Acquire)
    }

    /// The duck gain (1 when not ducked). Real-time safe.
    #[must_use]
    pub fn duck_gain(&self) -> f32 {
        f32::from_bits(self.duck.load(Ordering::Acquire))
    }

    /// Whether streams must not be reopened now (an interruption is in
    /// progress).
    #[must_use]
    pub fn hold_reopen(&self) -> bool {
        self.hold_reopen.load(Ordering::Acquire)
    }

    /// Bumped on every state change, so a player can notice one cheaply.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The last focus request (what to configure again after the platform
    /// resets its media services).
    #[must_use]
    pub fn request(&self) -> FocusRequest {
        *self.state().request()
    }

    /// Whether focus is held.
    #[must_use]
    pub fn holding(&self) -> bool {
        self.state().holding()
    }

    /// Whether playback is paused by focus or an interruption.
    #[must_use]
    pub fn paused_by_focus(&self) -> bool {
        self.state().paused()
    }

    /// The event log, oldest first (at most 256 entries).
    #[must_use]
    pub fn log(&self) -> Vec<SessionEvent> {
        self.log
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Fake {
        refuse: AtomicBool,
        requests: AtomicUsize,
        abandons: AtomicUsize,
        published: Mutex<Vec<NowPlaying>>,
    }

    impl SessionBridge for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn request_focus(&self, _r: &FocusRequest) -> bool {
            self.requests.fetch_add(1, Ordering::Relaxed);
            !self.refuse.load(Ordering::Relaxed)
        }
        fn abandon_focus(&self) {
            self.abandons.fetch_add(1, Ordering::Relaxed);
        }
        fn publish(&self, now: &NowPlaying) {
            self.published.lock().unwrap().push(now.clone());
        }
    }

    #[test]
    fn without_a_bridge_focus_is_always_granted() {
        let s = AudioSession::new();
        assert!(s.begin(FocusRequest::default()));
        assert!(!s.suspended());
        assert_eq!(s.bridge_name(), None);
    }

    #[test]
    fn a_call_suspends_and_holds_reopen_until_it_ends() {
        let s = AudioSession::new();
        let fake = Arc::new(Fake::default());
        s.set_bridge(fake.clone());
        assert!(s.begin(FocusRequest::default()));
        assert_eq!(s.on_interruption(Interruption::Began), Action::Pause);
        assert!(s.suspended() && s.hold_reopen());
        assert_eq!(
            s.on_interruption(Interruption::Ended {
                should_resume: true
            }),
            Action::Resume
        );
        assert!(!s.suspended() && !s.hold_reopen());
        s.end();
        assert_eq!(fake.requests.load(Ordering::Relaxed), 1);
        assert_eq!(fake.abandons.load(Ordering::Relaxed), 1);
        let log = s.log();
        assert_eq!(
            log.first(),
            Some(&SessionEvent::Requested { granted: true })
        );
        assert_eq!(log.last(), Some(&SessionEvent::Abandoned));
    }

    #[test]
    fn a_refused_request_does_not_play() {
        let s = AudioSession::new();
        let fake = Arc::new(Fake::default());
        fake.refuse.store(true, Ordering::Relaxed);
        s.set_bridge(fake.clone());
        assert!(!s.begin(FocusRequest::default()));
        s.end();
        assert_eq!(
            fake.abandons.load(Ordering::Relaxed),
            0,
            "nothing held, nothing abandoned"
        );
    }

    #[test]
    fn ducking_sets_the_gain_and_unducking_restores_it() {
        let s = AudioSession::new();
        assert!(s.begin(FocusRequest {
            duck_db: -20.0,
            ..FocusRequest::default()
        }));
        s.on_focus(FocusEvent::LostCanDuck);
        assert!((s.duck_gain() - 0.1).abs() < 1e-4);
        s.on_focus(FocusEvent::Gained);
        assert_eq!(s.duck_gain(), 1.0);
    }

    #[test]
    fn remote_commands_queue_and_now_playing_reaches_the_bridge() {
        let s = AudioSession::new();
        let fake = Arc::new(Fake::default());
        s.set_bridge(fake.clone());
        s.push_remote(RemoteCommand::Pause);
        s.push_remote(RemoteCommand::SeekTo(Duration::from_secs(3)));
        assert_eq!(
            s.take_remote(),
            vec![
                RemoteCommand::Pause,
                RemoteCommand::SeekTo(Duration::from_secs(3))
            ]
        );
        assert!(s.take_remote().is_empty());
        s.publish(&NowPlaying {
            title: "t".into(),
            ..NowPlaying::default()
        });
        assert_eq!(fake.published.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_log_is_bounded() {
        let s = AudioSession::new();
        for _ in 0..1000 {
            s.push_remote(RemoteCommand::Play);
        }
        assert_eq!(s.log().len(), LOG_DEPTH);
    }
}
