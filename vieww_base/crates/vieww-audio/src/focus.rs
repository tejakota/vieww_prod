//! Audio focus and interruptions, as one OS-neutral state machine (AU-02,
//! AU-03).
//!
//! A phone has one loudspeaker and many apps. Android arbitrates it with
//! *audio focus* (`AudioManager.requestAudioFocus`), iOS with the
//! *audio session* (`AVAudioSession` interruptions and route changes). The
//! two differ in vocabulary, not in what an app has to do:
//!
//! | what happened | Android | iOS | what the app does |
//! |---|---|---|---|
//! | a call, an alarm, another app's short clip | `AUDIOFOCUS_LOSS_TRANSIENT` | interruption began | pause, and resume afterwards |
//! | a notification sound | `AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK` | (the system ducks) | lower the volume, or pause for speech |
//! | another media app started | `AUDIOFOCUS_LOSS` | interruption began, no "should resume" | pause, and **stay** paused |
//! | it is over | `AUDIOFOCUS_GAIN` | interruption ended, "should resume" | resume what *this* paused |
//! | headphones unplugged | `ACTION_AUDIO_BECOMING_NOISY` | route change, old device unavailable | pause, and stay paused |
//!
//! [`FocusState`] is that table. The platform bridge (in the platform crate)
//! turns OS callbacks into [`FocusEvent`], [`Interruption`] and
//! [`RouteChange`]; the state answers with an [`Action`] for the transport.
//! It is pure state, so every row is unit-tested here without a phone.
//!
//! One rule is worth stating on its own: **only a pause this state machine
//! caused is ever undone by it.** If the user pressed pause during a call,
//! the end of the call must not start the music again.

/// What the app plays: decides the platform's usage/category, and whether a
/// "may duck" loss pauses instead (speech ducked under a notification is
/// unintelligible, so podcasts and audiobooks pause).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Content {
    /// Music and other media (Android `USAGE_MEDIA`, iOS `.playback`).
    #[default]
    Media,
    /// Spoken word: podcasts, audiobooks, navigation prompts. Pauses rather
    /// than ducks.
    Speech,
    /// Game audio (`USAGE_GAME`, iOS `.ambient` mixes with other apps).
    Game,
    /// Short interface sounds (`USAGE_ASSISTANCE_SONIFICATION`).
    Sonification,
}

/// How long the app wants the speaker for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GainType {
    /// Until further notice (`AUDIOFOCUS_GAIN`): media playback.
    #[default]
    Gain,
    /// A short clip; others pause (`AUDIOFOCUS_GAIN_TRANSIENT`).
    Transient,
    /// A short clip; others may duck (`AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK`).
    TransientMayDuck,
    /// A short clip nothing may mix with, such as a voice memo
    /// (`AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE`).
    TransientExclusive,
}

/// What the app asks for when it starts playing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusRequest {
    /// What is being played.
    pub content: Content,
    /// For how long.
    pub gain: GainType,
    /// How far to duck, in dB (negative). Android 8+ ducks an app itself
    /// unless it asked to be told; this is the depth used when the engine
    /// does it.
    pub duck_db: f32,
}

impl Default for FocusRequest {
    fn default() -> Self {
        Self {
            content: Content::Media,
            gain: GainType::Gain,
            duck_db: -12.0,
        }
    }
}

impl FocusRequest {
    /// Whether a "may duck" loss pauses instead of ducking.
    #[must_use]
    pub const fn pauses_when_ducked(&self) -> bool {
        matches!(self.content, Content::Speech)
    }

    /// The Android `AUDIOFOCUS_GAIN*` constant for [`Self::gain`].
    #[must_use]
    pub const fn android_gain(&self) -> i32 {
        match self.gain {
            GainType::Gain => 1,
            GainType::Transient => 2,
            GainType::TransientMayDuck => 3,
            GainType::TransientExclusive => 4,
        }
    }

    /// The Android `AudioAttributes.USAGE_*` constant for [`Self::content`].
    #[must_use]
    pub const fn android_usage(&self) -> i32 {
        match self.content {
            Content::Media | Content::Speech => 1, // USAGE_MEDIA
            Content::Game => 14,                   // USAGE_GAME
            Content::Sonification => 13,           // USAGE_ASSISTANCE_SONIFICATION
        }
    }

    /// The Android `AudioAttributes.CONTENT_TYPE_*` constant.
    #[must_use]
    pub const fn android_content_type(&self) -> i32 {
        match self.content {
            Content::Speech => 1,       // CONTENT_TYPE_SPEECH
            Content::Media => 2,        // CONTENT_TYPE_MUSIC
            Content::Game => 4,         // CONTENT_TYPE_SONIFICATION
            Content::Sonification => 4, // CONTENT_TYPE_SONIFICATION
        }
    }
}

/// A focus change, as Android reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusEvent {
    /// `AUDIOFOCUS_GAIN`: the speaker is ours again.
    Gained,
    /// `AUDIOFOCUS_LOSS`: someone else's for good.
    Lost,
    /// `AUDIOFOCUS_LOSS_TRANSIENT`: someone else's for a moment.
    LostTransient,
    /// `AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK`: we may keep playing, quieter.
    LostCanDuck,
}

impl FocusEvent {
    /// From Android's `onAudioFocusChange(int)` argument.
    #[must_use]
    pub const fn from_android(change: i32) -> Option<Self> {
        match change {
            1..=4 => Some(Self::Gained),
            -1 => Some(Self::Lost),
            -2 => Some(Self::LostTransient),
            -3 => Some(Self::LostCanDuck),
            _ => None,
        }
    }
}

/// An iOS audio-session interruption (or an Android call, seen through
/// `AudioManager.getMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interruption {
    /// It began: the session is inactive, the stream has stopped.
    Began,
    /// It ended. `should_resume` is iOS's
    /// `AVAudioSessionInterruptionOptionShouldResume`.
    Ended {
        /// Whether the system suggests playback resume.
        should_resume: bool,
    },
}

/// An output route change worth acting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteChange {
    /// The device playing went away (headphones out, Bluetooth off):
    /// iOS `oldDeviceUnavailable`, Android `ACTION_AUDIO_BECOMING_NOISY`.
    OldDeviceUnavailable,
    /// Something else: a new device arrived, a category change. Nothing to
    /// do; the resilient stream follows the default device by itself.
    Other,
}

/// What the transport should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing.
    None,
    /// Lower the volume by the request's `duck_db`.
    Duck,
    /// Back to full volume.
    Unduck,
    /// Pause.
    Pause,
    /// Resume (only ever after a [`Action::Pause`] this state issued).
    Resume,
}

/// The focus automaton.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FocusState {
    request: FocusRequest,
    /// The app wants to be playing (it called play, not pause).
    wants_play: bool,
    /// We hold focus.
    holding: bool,
    /// Paused by a transient loss or interruption: resumable.
    paused_by_focus: bool,
    /// Paused by a permanent loss or a route change: not resumable by us.
    stopped_by_focus: bool,
    ducked: bool,
    interrupted: bool,
}

impl FocusState {
    /// The current request.
    #[must_use]
    pub const fn request(&self) -> &FocusRequest {
        &self.request
    }

    /// The app is starting playback with `request`. The caller asks the
    /// platform for focus; [`Self::granted`] records the answer.
    pub fn begin(&mut self, request: FocusRequest) {
        self.request = request;
        self.wants_play = true;
        self.stopped_by_focus = false;
    }

    /// The platform answered a focus request. Returns whether playback may
    /// start: a refused request (Android `AUDIOFOCUS_REQUEST_FAILED`, which
    /// is what a request during a call gets) must stay silent.
    pub fn granted(&mut self, granted: bool) -> bool {
        self.holding = granted;
        if !granted {
            self.wants_play = false;
        }
        granted && !self.interrupted
    }

    /// The app paused or stopped by itself: focus is given back, and no
    /// later event resumes it.
    pub fn end(&mut self) {
        self.wants_play = false;
        self.holding = false;
        self.paused_by_focus = false;
        self.ducked = false;
    }

    /// Whether we hold focus.
    #[must_use]
    pub const fn holding(&self) -> bool {
        self.holding
    }

    /// Whether the output is currently ducked.
    #[must_use]
    pub const fn ducked(&self) -> bool {
        self.ducked
    }

    /// Whether an interruption is in progress (the stream must not be
    /// reopened: on iOS the session is inactive until it ends).
    #[must_use]
    pub const fn interrupted(&self) -> bool {
        self.interrupted
    }

    /// Whether playback is paused by focus, resumable or not.
    #[must_use]
    pub const fn paused(&self) -> bool {
        self.paused_by_focus || self.stopped_by_focus
    }

    fn pause_transient(&mut self) -> Action {
        if self.wants_play && !self.paused() {
            self.paused_by_focus = true;
            Action::Pause
        } else {
            Action::None
        }
    }

    fn pause_for_good(&mut self) -> Action {
        let was_playing = self.wants_play && !self.paused();
        self.paused_by_focus = false;
        self.stopped_by_focus = self.wants_play;
        self.wants_play = false;
        if was_playing {
            Action::Pause
        } else {
            Action::None
        }
    }

    fn resume_if_ours(&mut self) -> Action {
        if self.paused_by_focus {
            self.paused_by_focus = false;
            Action::Resume
        } else {
            Action::None
        }
    }

    /// A focus change.
    pub fn on_focus(&mut self, event: FocusEvent) -> Action {
        match event {
            FocusEvent::Gained => {
                self.holding = true;
                if self.ducked {
                    self.ducked = false;
                    if !self.paused_by_focus {
                        return Action::Unduck;
                    }
                }
                if self.interrupted {
                    return Action::None;
                }
                self.resume_if_ours()
            }
            FocusEvent::Lost => {
                self.holding = false;
                self.ducked = false;
                self.pause_for_good()
            }
            FocusEvent::LostTransient => self.pause_transient(),
            FocusEvent::LostCanDuck => {
                if self.request.pauses_when_ducked() {
                    self.pause_transient()
                } else if self.wants_play && !self.paused() && !self.ducked {
                    self.ducked = true;
                    Action::Duck
                } else {
                    Action::None
                }
            }
        }
    }

    /// An interruption began or ended.
    pub fn on_interruption(&mut self, i: Interruption) -> Action {
        match i {
            Interruption::Began => {
                self.interrupted = true;
                self.pause_transient()
            }
            Interruption::Ended { should_resume } => {
                self.interrupted = false;
                if should_resume {
                    self.resume_if_ours()
                } else {
                    // iOS says not to: treat it like a permanent loss, but
                    // the stream may be reopened.
                    if self.paused_by_focus {
                        self.paused_by_focus = false;
                        self.stopped_by_focus = true;
                        self.wants_play = false;
                    }
                    Action::None
                }
            }
        }
    }

    /// An output route change.
    pub fn on_route(&mut self, r: RouteChange) -> Action {
        match r {
            RouteChange::OldDeviceUnavailable => self.pause_for_good(),
            RouteChange::Other => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(content: Content) -> FocusState {
        let mut s = FocusState::default();
        s.begin(FocusRequest {
            content,
            ..FocusRequest::default()
        });
        assert!(s.granted(true));
        s
    }

    #[test]
    fn a_call_pauses_and_its_end_resumes() {
        let mut s = playing(Content::Media);
        assert_eq!(s.on_focus(FocusEvent::LostTransient), Action::Pause);
        assert!(s.paused());
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::Resume);
        assert!(!s.paused());
    }

    #[test]
    fn a_notification_ducks_music_and_pauses_speech() {
        let mut music = playing(Content::Media);
        assert_eq!(music.on_focus(FocusEvent::LostCanDuck), Action::Duck);
        assert!(music.ducked());
        assert_eq!(music.on_focus(FocusEvent::Gained), Action::Unduck);
        assert!(!music.ducked());

        let mut podcast = playing(Content::Speech);
        assert_eq!(podcast.on_focus(FocusEvent::LostCanDuck), Action::Pause);
        assert_eq!(podcast.on_focus(FocusEvent::Gained), Action::Resume);
    }

    #[test]
    fn another_media_app_stops_us_for_good() {
        let mut s = playing(Content::Media);
        assert_eq!(s.on_focus(FocusEvent::Lost), Action::Pause);
        assert!(!s.holding());
        // Its focus ending later is not ours to act on.
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::None);
        assert!(s.paused());
    }

    #[test]
    fn a_pause_the_user_made_is_never_undone() {
        let mut s = playing(Content::Media);
        assert_eq!(s.on_focus(FocusEvent::LostTransient), Action::Pause);
        s.end(); // the user pressed pause during the call
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::None);
        let mut s = playing(Content::Media);
        s.end();
        assert_eq!(s.on_focus(FocusEvent::LostTransient), Action::None);
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::None);
    }

    #[test]
    fn a_refused_request_stays_silent() {
        let mut s = FocusState::default();
        s.begin(FocusRequest::default());
        assert!(!s.granted(false), "a request during a call is refused");
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::None);
    }

    #[test]
    fn ios_interruptions_follow_should_resume() {
        let mut s = playing(Content::Media);
        assert_eq!(s.on_interruption(Interruption::Began), Action::Pause);
        assert!(s.interrupted());
        assert_eq!(
            s.on_interruption(Interruption::Ended {
                should_resume: true
            }),
            Action::Resume
        );
        assert!(!s.interrupted());

        let mut s = playing(Content::Media);
        s.on_interruption(Interruption::Began);
        assert_eq!(
            s.on_interruption(Interruption::Ended {
                should_resume: false
            }),
            Action::None
        );
        assert!(s.paused(), "no should-resume: stays paused");
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::None);
    }

    #[test]
    fn focus_back_during_an_interruption_waits_for_its_end() {
        let mut s = playing(Content::Media);
        s.on_interruption(Interruption::Began);
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::None);
        assert_eq!(
            s.on_interruption(Interruption::Ended {
                should_resume: true
            }),
            Action::Resume
        );
    }

    #[test]
    fn headphones_out_pauses_and_nothing_resumes_it() {
        let mut s = playing(Content::Media);
        assert_eq!(s.on_route(RouteChange::OldDeviceUnavailable), Action::Pause);
        assert_eq!(s.on_route(RouteChange::Other), Action::None);
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::None);
        // Pressing play again starts afresh.
        s.begin(FocusRequest::default());
        assert!(s.granted(true));
        assert!(!s.paused());
    }

    #[test]
    fn a_duck_then_a_transient_loss_resumes_at_full_volume() {
        let mut s = playing(Content::Media);
        assert_eq!(s.on_focus(FocusEvent::LostCanDuck), Action::Duck);
        assert_eq!(s.on_focus(FocusEvent::LostTransient), Action::Pause);
        // Gain clears the duck silently and resumes.
        assert_eq!(s.on_focus(FocusEvent::Gained), Action::Resume);
        assert!(!s.ducked());
    }

    #[test]
    fn android_constants_map_both_ways() {
        assert_eq!(FocusEvent::from_android(1), Some(FocusEvent::Gained));
        assert_eq!(FocusEvent::from_android(-1), Some(FocusEvent::Lost));
        assert_eq!(
            FocusEvent::from_android(-2),
            Some(FocusEvent::LostTransient)
        );
        assert_eq!(FocusEvent::from_android(-3), Some(FocusEvent::LostCanDuck));
        assert_eq!(FocusEvent::from_android(0), None);
        let r = FocusRequest::default();
        assert_eq!((r.android_gain(), r.android_usage()), (1, 1));
        let g = FocusRequest {
            content: Content::Game,
            gain: GainType::TransientMayDuck,
            ..r
        };
        assert_eq!((g.android_gain(), g.android_usage()), (3, 14));
    }
}
