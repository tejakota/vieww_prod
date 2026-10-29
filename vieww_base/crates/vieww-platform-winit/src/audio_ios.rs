//! iOS audio session: `AVAudioSession`, interruptions, route changes, the
//! lock screen (AU-03, AU-04).
//!
//! [`install`] runs once, from [`services::platform`](crate::services). It:
//!
//! * registers a [`SessionBridge`] with vieww-audio's process-wide
//!   [`AudioSession`]. `request_focus` sets the category for the request's
//!   content (`.playback`, with `.spokenAudio` mode for speech, or
//!   `.ambient` for game audio, which mixes with the user's music) and
//!   activates the session. `abandon_focus` deactivates it with
//!   `.notifyOthersOnDeactivation`, so the app that was paused resumes;
//! * observes `AVAudioSessionInterruptionNotification` (began/ended, with
//!   the should-resume option), `AVAudioSessionRouteChangeNotification`
//!   (old device unavailable: headphones out) and
//!   `AVAudioSessionMediaServicesWereResetNotification` (the media server
//!   restarted: the category must be set again). An interruption that ends
//!   reactivates the session **before** telling the engine, so the stream
//!   that reopens has an active session to open on;
//! * routes `MPRemoteCommandCenter` play/pause/toggle/stop/next/previous/
//!   seek/skip into [`AudioSession::push_remote`], and publishes
//!   [`NowPlaying`] to `MPNowPlayingInfoCenter`.
//!
//! Playing with the screen locked needs one more thing that is not code:
//! `UIBackgroundModes` = `audio` in the app's `Info.plist`. `vieww-build`'s
//! iOS packaging writes it when the project asks for background audio, and
//! `ci/mobile/lint-background-audio.sh` checks that it is there.
//!
//! Everything goes through `msg_send!` on classes looked up by name, with
//! the framework constants linked as the extern statics Apple declares them
//! as, so this needs no binding crate beyond the `objc2` the platform crate
//! already uses.

use std::sync::Arc;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, NSObject, Sel};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::NSString;

use vieww_audio::focus::{Content, FocusRequest, Interruption, RouteChange};
use vieww_audio::session::{AudioSession, NowPlaying, RemoteCommand, SessionBridge};

#[link(name = "AVFAudio", kind = "framework")]
extern "C" {
    static AVAudioSessionCategoryPlayback: &'static NSString;
    static AVAudioSessionCategoryAmbient: &'static NSString;
    static AVAudioSessionModeDefault: &'static NSString;
    static AVAudioSessionModeSpokenAudio: &'static NSString;
    static AVAudioSessionInterruptionNotification: &'static NSString;
    static AVAudioSessionRouteChangeNotification: &'static NSString;
    static AVAudioSessionMediaServicesWereResetNotification: &'static NSString;
    static AVAudioSessionInterruptionTypeKey: &'static NSString;
    static AVAudioSessionInterruptionOptionKey: &'static NSString;
    static AVAudioSessionRouteChangeReasonKey: &'static NSString;
}

#[link(name = "MediaPlayer", kind = "framework")]
extern "C" {
    static MPMediaItemPropertyTitle: &'static NSString;
    static MPMediaItemPropertyArtist: &'static NSString;
    static MPMediaItemPropertyAlbumTitle: &'static NSString;
    static MPMediaItemPropertyPlaybackDuration: &'static NSString;
    static MPNowPlayingInfoPropertyElapsedPlaybackTime: &'static NSString;
    static MPNowPlayingInfoPropertyPlaybackRate: &'static NSString;
}

/// `AVAudioSessionInterruptionTypeBegan`.
const INTERRUPTION_BEGAN: usize = 1;
/// `AVAudioSessionInterruptionOptionShouldResume`.
const OPTION_SHOULD_RESUME: usize = 1;
/// `AVAudioSessionRouteChangeReasonOldDeviceUnavailable`.
const ROUTE_OLD_DEVICE_UNAVAILABLE: usize = 2;
/// `AVAudioSessionCategoryOptionMixWithOthers`.
const MIX_WITH_OTHERS: usize = 1;
/// `AVAudioSessionSetActiveOptionNotifyOthersOnDeactivation`.
const NOTIFY_OTHERS: usize = 1;
/// `MPRemoteCommandHandlerStatusSuccess`.
const HANDLED: isize = 0;

fn class(name: &str) -> Option<&'static AnyClass> {
    AnyClass::get(name)
}

fn shared_session() -> Option<*mut AnyObject> {
    let cls = class("AVAudioSession")?;
    let s: *mut AnyObject = unsafe { msg_send![cls, sharedInstance] };
    (!s.is_null()).then_some(s)
}

/// Set the category and mode for `request`. Logged, not fatal: a failed
/// category leaves the default (`.soloAmbient`), which still plays.
fn configure(request: &FocusRequest) -> bool {
    let Some(s) = shared_session() else {
        return false;
    };
    let (category, mode, options) = unsafe {
        match request.content {
            Content::Media | Content::Sonification => {
                (AVAudioSessionCategoryPlayback, AVAudioSessionModeDefault, 0)
            }
            Content::Speech => (
                AVAudioSessionCategoryPlayback,
                AVAudioSessionModeSpokenAudio,
                0,
            ),
            Content::Game => (
                AVAudioSessionCategoryAmbient,
                AVAudioSessionModeDefault,
                MIX_WITH_OTHERS,
            ),
        }
    };
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let ok: bool = unsafe {
        msg_send![s, setCategory: category, mode: mode, options: options, error: &mut error]
    };
    if !ok {
        log::warn!("vieww-audio: AVAudioSession setCategory failed");
    }
    ok
}

fn set_active(active: bool) -> bool {
    let Some(s) = shared_session() else {
        return false;
    };
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let options = if active { 0 } else { NOTIFY_OTHERS };
    let ok: bool =
        unsafe { msg_send![s, setActive: active, withOptions: options, error: &mut error] };
    if !ok {
        log::warn!("vieww-audio: AVAudioSession setActive({active}) failed");
    }
    ok
}

fn user_info_usize(note: &AnyObject, key: &NSString) -> Option<usize> {
    let info: *mut AnyObject = unsafe { msg_send![note, userInfo] };
    if info.is_null() {
        return None;
    }
    let value: *mut AnyObject = unsafe { msg_send![info, objectForKey: key] };
    if value.is_null() {
        return None;
    }
    Some(unsafe { msg_send![value, unsignedIntegerValue] })
}

fn event_double(event: &AnyObject, sel_name: Sel) -> f64 {
    let responds: bool = unsafe { msg_send![event, respondsToSelector: sel_name] };
    if !responds {
        return 0.0;
    }
    // `positionTime` and `interval` are both `NSTimeInterval` properties.
    if sel_name == sel!(positionTime) {
        unsafe { msg_send![event, positionTime] }
    } else {
        unsafe { msg_send![event, interval] }
    }
}

fn secs(s: f64) -> std::time::Duration {
    std::time::Duration::from_secs_f64(s.max(0.0))
}

declare_class!(
    /// Receives the session's notifications and the remote commands.
    struct Observer;

    // SAFETY: NSObject has no subclassing requirements; no ivars, no Drop.
    unsafe impl ClassType for Observer {
        type Super = NSObject;
        type Mutability = mutability::InteriorMutable;
        const NAME: &'static str = "ViewwAudioSessionObserver";
    }

    impl DeclaredClass for Observer {}

    unsafe impl Observer {
        #[method_id(init)]
        fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
            let this = this.set_ivars(());
            unsafe { msg_send_id![super(this), init] }
        }

        #[method(interruption:)]
        fn interruption(&self, note: &AnyObject) {
            let session = AudioSession::shared();
            let kind = user_info_usize(note, unsafe { AVAudioSessionInterruptionTypeKey });
            if kind == Some(INTERRUPTION_BEGAN) {
                session.on_interruption(Interruption::Began);
            } else {
                let options =
                    user_info_usize(note, unsafe { AVAudioSessionInterruptionOptionKey })
                        .unwrap_or(0);
                // Reactivate first: the stream reopens as soon as the engine
                // hears the interruption is over.
                if session.holding() {
                    set_active(true);
                }
                session.on_interruption(Interruption::Ended {
                    should_resume: options & OPTION_SHOULD_RESUME != 0,
                });
            }
        }

        #[method(routeChange:)]
        fn route_change(&self, note: &AnyObject) {
            let reason = user_info_usize(note, unsafe { AVAudioSessionRouteChangeReasonKey });
            let change = if reason == Some(ROUTE_OLD_DEVICE_UNAVAILABLE) {
                RouteChange::OldDeviceUnavailable
            } else {
                RouteChange::Other
            };
            let session = AudioSession::shared();
            session.on_route(change);
            // Any route change: device streams look at the default now
            // (AU-09).
            session.notify_route_change();
        }

        #[method(mediaServicesReset:)]
        fn media_services_reset(&self, _note: &AnyObject) {
            // Every session object is invalid after a reset; set it up again.
            let session = AudioSession::shared();
            if session.holding() {
                configure(&session.request());
                set_active(true);
            }
        }

        #[method(remotePlay:)]
        fn remote_play(&self, _e: &AnyObject) -> isize {
            AudioSession::shared().push_remote(RemoteCommand::Play);
            HANDLED
        }

        #[method(remotePause:)]
        fn remote_pause(&self, _e: &AnyObject) -> isize {
            AudioSession::shared().push_remote(RemoteCommand::Pause);
            HANDLED
        }

        #[method(remoteToggle:)]
        fn remote_toggle(&self, _e: &AnyObject) -> isize {
            AudioSession::shared().push_remote(RemoteCommand::TogglePlayPause);
            HANDLED
        }

        #[method(remoteStop:)]
        fn remote_stop(&self, _e: &AnyObject) -> isize {
            AudioSession::shared().push_remote(RemoteCommand::Stop);
            HANDLED
        }

        #[method(remoteNext:)]
        fn remote_next(&self, _e: &AnyObject) -> isize {
            AudioSession::shared().push_remote(RemoteCommand::Next);
            HANDLED
        }

        #[method(remotePrevious:)]
        fn remote_previous(&self, _e: &AnyObject) -> isize {
            AudioSession::shared().push_remote(RemoteCommand::Previous);
            HANDLED
        }

        #[method(remoteSeek:)]
        fn remote_seek(&self, e: &AnyObject) -> isize {
            let at = event_double(e, sel!(positionTime));
            AudioSession::shared().push_remote(RemoteCommand::SeekTo(secs(at)));
            HANDLED
        }

        #[method(remoteSkipForward:)]
        fn remote_skip_forward(&self, e: &AnyObject) -> isize {
            let by = event_double(e, sel!(interval));
            AudioSession::shared().push_remote(RemoteCommand::SkipForward(secs(by)));
            HANDLED
        }

        #[method(remoteSkipBackward:)]
        fn remote_skip_backward(&self, e: &AnyObject) -> isize {
            let by = event_double(e, sel!(interval));
            AudioSession::shared().push_remote(RemoteCommand::SkipBackward(secs(by)));
            HANDLED
        }
    }
);

/// The bridge vieww-audio calls.
#[derive(Debug, Default)]
struct IosBridge;

impl SessionBridge for IosBridge {
    fn name(&self) -> &'static str {
        "ios"
    }

    fn request_focus(&self, request: &FocusRequest) -> bool {
        configure(request);
        // A refused activation is what a request during a phone call gets.
        set_active(true)
    }

    fn abandon_focus(&self) {
        set_active(false);
    }

    fn publish(&self, now: &NowPlaying) {
        let (Some(center_cls), Some(dict_cls), Some(num_cls)) = (
            class("MPNowPlayingInfoCenter"),
            class("NSMutableDictionary"),
            class("NSNumber"),
        ) else {
            return;
        };
        unsafe {
            let center: *mut AnyObject = msg_send![center_cls, defaultCenter];
            let dict: *mut AnyObject = msg_send![dict_cls, dictionary];
            if center.is_null() || dict.is_null() {
                return;
            }
            let put_str = |key: &NSString, value: &str| {
                if !value.is_empty() {
                    let v = NSString::from_str(value);
                    let _: () = msg_send![dict, setObject: &*v, forKey: key];
                }
            };
            put_str(MPMediaItemPropertyTitle, &now.title);
            put_str(MPMediaItemPropertyArtist, &now.artist);
            put_str(MPMediaItemPropertyAlbumTitle, &now.album);
            let put_num = |key: &NSString, value: f64| {
                let n: *mut AnyObject = msg_send![num_cls, numberWithDouble: value];
                if !n.is_null() {
                    let _: () = msg_send![dict, setObject: n, forKey: key];
                }
            };
            if let Some(d) = now.duration {
                put_num(MPMediaItemPropertyPlaybackDuration, d.as_secs_f64());
            }
            put_num(
                MPNowPlayingInfoPropertyElapsedPlaybackTime,
                now.position.as_secs_f64(),
            );
            put_num(
                MPNowPlayingInfoPropertyPlaybackRate,
                if now.playing { 1.0 } else { 0.0 },
            );
            let _: () = msg_send![center, setNowPlayingInfo: dict];
        }
    }

    fn clear_now_playing(&self) {
        let Some(center_cls) = class("MPNowPlayingInfoCenter") else {
            return;
        };
        unsafe {
            let center: *mut AnyObject = msg_send![center_cls, defaultCenter];
            if !center.is_null() {
                let nil: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![center, setNowPlayingInfo: nil];
            }
        }
    }
}

fn observe(center: *mut AnyObject, observer: &Observer, selector: Sel, name: &NSString) {
    let nil: *mut AnyObject = std::ptr::null_mut();
    unsafe {
        let _: () =
            msg_send![center, addObserver: observer, selector: selector, name: name, object: nil];
    }
}

fn command(center: *mut AnyObject, getter: Sel, observer: &Observer, action: Sel) {
    unsafe {
        let cmd: *mut AnyObject = msg_send![center, performSelector: getter];
        if cmd.is_null() {
            return;
        }
        let _: () = msg_send![cmd, setEnabled: true];
        let _: *mut AnyObject = msg_send![cmd, addTarget: observer, action: action];
    }
}

/// Register the bridge and the observers. Idempotent.
pub(crate) fn install() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        AudioSession::shared().set_bridge(Arc::new(IosBridge));
        let observer: Retained<Observer> = unsafe { msg_send_id![Observer::alloc(), init] };
        if let Some(nc_cls) = class("NSNotificationCenter") {
            let nc: *mut AnyObject = unsafe { msg_send![nc_cls, defaultCenter] };
            if !nc.is_null() {
                unsafe {
                    observe(
                        nc,
                        &observer,
                        sel!(interruption:),
                        AVAudioSessionInterruptionNotification,
                    );
                    observe(
                        nc,
                        &observer,
                        sel!(routeChange:),
                        AVAudioSessionRouteChangeNotification,
                    );
                    observe(
                        nc,
                        &observer,
                        sel!(mediaServicesReset:),
                        AVAudioSessionMediaServicesWereResetNotification,
                    );
                }
            }
        }
        if let Some(rc_cls) = class("MPRemoteCommandCenter") {
            let rc: *mut AnyObject = unsafe { msg_send![rc_cls, sharedCommandCenter] };
            if !rc.is_null() {
                command(rc, sel!(playCommand), &observer, sel!(remotePlay:));
                command(rc, sel!(pauseCommand), &observer, sel!(remotePause:));
                command(
                    rc,
                    sel!(togglePlayPauseCommand),
                    &observer,
                    sel!(remoteToggle:),
                );
                command(rc, sel!(stopCommand), &observer, sel!(remoteStop:));
                command(rc, sel!(nextTrackCommand), &observer, sel!(remoteNext:));
                command(
                    rc,
                    sel!(previousTrackCommand),
                    &observer,
                    sel!(remotePrevious:),
                );
                command(
                    rc,
                    sel!(changePlaybackPositionCommand),
                    &observer,
                    sel!(remoteSeek:),
                );
                command(
                    rc,
                    sel!(skipForwardCommand),
                    &observer,
                    sel!(remoteSkipForward:),
                );
                command(
                    rc,
                    sel!(skipBackwardCommand),
                    &observer,
                    sel!(remoteSkipBackward:),
                );
            }
        }
        // Process lifetime: the notification center and the command center
        // hold it weakly (or unretained), so it must never be freed.
        std::mem::forget(observer);
    });
}

/// For the on-device audio check: whether the session's category is
/// `.playback` now.
pub(crate) fn category_is_playback() -> bool {
    let Some(s) = shared_session() else {
        return false;
    };
    let category: *mut AnyObject = unsafe { msg_send![s, category] };
    if category.is_null() {
        return false;
    }
    let same: bool =
        unsafe { msg_send![category, isEqualToString: AVAudioSessionCategoryPlayback] };
    same
}

/// For the on-device audio check (AU-09): change the session's category to
/// `.ambient` and back to `.playback`. iOS answers each category change
/// with an `AVAudioSessionRouteChangeNotification` (reason
/// `categoryChange`), posted by the system to the same observer a
/// headphone unplug reaches. `true` when both changes were accepted.
pub(crate) fn bounce_category() -> bool {
    let Some(s) = shared_session() else {
        return false;
    };
    let set = |category: &NSString| -> bool {
        let mut error: *mut AnyObject = std::ptr::null_mut();
        unsafe {
            msg_send![s, setCategory: category, mode: AVAudioSessionModeDefault, options: 0usize, error: &mut error]
        }
    };
    let away = set(unsafe { AVAudioSessionCategoryAmbient });
    std::thread::sleep(std::time::Duration::from_millis(300));
    let back = set(unsafe { AVAudioSessionCategoryPlayback });
    away && back
}

/// For the on-device audio check: whether the lock screen has a now-playing
/// entry.
pub(crate) fn now_playing_published() -> bool {
    let Some(center_cls) = class("MPNowPlayingInfoCenter") else {
        return false;
    };
    unsafe {
        let center: *mut AnyObject = msg_send![center_cls, defaultCenter];
        if center.is_null() {
            return false;
        }
        let info: *mut AnyObject = msg_send![center, nowPlayingInfo];
        !info.is_null()
    }
}

/// For the on-device audio check: post the interruption notification iOS
/// posts for a call, so the observer → session → engine path runs on the
/// phone. (A real call cannot be placed on a device cloud.)
pub(crate) fn post_interruption(began: bool, should_resume: bool) {
    let (Some(nc_cls), Some(dict_cls), Some(num_cls)) = (
        class("NSNotificationCenter"),
        class("NSMutableDictionary"),
        class("NSNumber"),
    ) else {
        return;
    };
    unsafe {
        let nc: *mut AnyObject = msg_send![nc_cls, defaultCenter];
        let info: *mut AnyObject = msg_send![dict_cls, dictionary];
        if nc.is_null() || info.is_null() {
            return;
        }
        let kind: usize = if began { INTERRUPTION_BEGAN } else { 0 };
        let n: *mut AnyObject = msg_send![num_cls, numberWithUnsignedInteger: kind];
        let _: () = msg_send![info, setObject: n, forKey: AVAudioSessionInterruptionTypeKey];
        if !began {
            let opt: usize = if should_resume {
                OPTION_SHOULD_RESUME
            } else {
                0
            };
            let o: *mut AnyObject = msg_send![num_cls, numberWithUnsignedInteger: opt];
            let _: () = msg_send![info, setObject: o, forKey: AVAudioSessionInterruptionOptionKey];
        }
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![nc, postNotificationName: AVAudioSessionInterruptionNotification, object: nil, userInfo: info];
    }
}
