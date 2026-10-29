//! Android audio focus, calls, headphones and the lock screen (AU-02,
//! AU-04).
//!
//! [`install`] runs once, from [`services::for_android`](crate::services).
//! It registers a [`SessionBridge`] with vieww-audio's process-wide
//! [`AudioSession`]:
//!
//! * `request_focus` builds `AudioAttributes` (usage and content type from
//!   the request) and an `AudioFocusRequest`, calls
//!   `AudioManager.requestAudioFocus`, and activates the `MediaSession`.
//!   A refusal, which is what a request during a call gets, makes
//!   `MediaPlayer::play` fail with `FocusDenied` instead of talking over
//!   the call;
//! * focus changes arrive through `dev.vieww.audio.FocusListener`, a
//!   four-line Java class whose native method is registered here;
//! * `ACTION_AUDIO_BECOMING_NOISY` (headphones out) arrives through
//!   `dev.vieww.audio.NoisyReceiver`, registered at runtime;
//! * lock-screen, notification and headset buttons arrive through
//!   `dev.vieww.audio.SessionCallback` on a `MediaSession`, and
//!   [`NowPlaying`] is published to it as `MediaMetadata` and
//!   `PlaybackState`;
//! * `dev.vieww.audio.PlaybackService`, a foreground service of type
//!   `mediaPlayback`, is started while media plays so the process is not
//!   frozen when the screen locks.
//!
//! The Java classes live in `android/java/` and are compiled into the APK's
//! `classes.dex` by `ci/mobile/apk-add-code.sh`, which also declares the
//! service in the manifest (`--background-audio`). An APK built without
//! them still gets focus. The request is then made without a listener
//! (Android 8+ ducks the app by itself), and calls are noticed by polling
//! `AudioManager.getMode()` twice a second, so the call case that AU-02 is
//! about holds either way.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jni::objects::{Global, JClass, JObject, JValue};
use jni::{jni_sig, jni_str};

use vieww_audio::focus::{Content, FocusEvent, FocusRequest, Interruption, RouteChange};
use vieww_audio::session::{AudioSession, NowPlaying, RemoteCommand, SessionBridge};

/// `AudioManager.AUDIOFOCUS_REQUEST_GRANTED`.
const GRANTED: i32 = 1;
/// `AudioManager.STREAM_MUSIC`, for the pre-API-26 request.
const STREAM_MUSIC: i32 = 3;
/// `AudioManager.MODE_NORMAL`; anything else is ringing or a call.
const MODE_NORMAL: i32 = 0;
/// `PlaybackState.STATE_PAUSED` / `STATE_PLAYING`.
const STATE_PAUSED: i32 = 2;
const STATE_PLAYING: i32 = 3;
/// `PlaybackState.ACTION_*`: stop, pause, play, rewind, previous, next,
/// fast-forward, seek, play/pause.
const ACTIONS: i64 = 1 | 2 | 4 | 8 | 16 | 32 | 64 | 256 | 512;

/// The Java side, when the APK carries it.
struct Java {
    listener: Option<Global<JObject<'static>>>,
    session: Option<Global<JObject<'static>>>,
    service: Option<Global<JClass<'static>>>,
}

struct AndroidBridge {
    app: crate::AndroidApp,
    java: Java,
    sdk: i32,
    request: Mutex<Option<Global<JObject<'static>>>>,
    content: Mutex<Content>,
    servicing: AtomicBool,
}

impl std::fmt::Debug for AndroidBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidBridge")
            .field("sdk", &self.sdk)
            .field("listener", &self.java.listener.is_some())
            .field("session", &self.java.session.is_some())
            .field("service", &self.java.service.is_some())
            .finish_non_exhaustive()
    }
}

type JResult<T> = Result<T, jni::errors::Error>;

fn with_env<T>(
    app: &crate::AndroidApp,
    act: impl FnOnce(&mut jni::Env<'_>, &JObject<'_>) -> JResult<T>,
) -> JResult<T> {
    // Both pointers belong to the running activity and neither is kept past
    // this call, the same contract as `clipboard.rs`.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let activity_ptr = app.activity_as_ptr();
    vm.attach_current_thread(|env| {
        let activity = unsafe { JObject::from_raw(env, activity_ptr.cast()) };
        let out = act(env, &activity);
        if env.exception_check() {
            env.exception_clear();
        }
        out
    })
}

fn audio_manager<'l>(env: &mut jni::Env<'l>, activity: &JObject<'_>) -> JResult<JObject<'l>> {
    let name = env.new_string("audio")?;
    env.call_method(
        activity,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

/// Load one of our classes through the activity's class loader (a native
/// thread's `FindClass` sees only the system's).
fn load<'l>(env: &mut jni::Env<'l>, activity: &JObject<'_>, name: &str) -> Option<JClass<'l>> {
    let found = (|| -> JResult<JClass<'l>> {
        let loader = env
            .call_method(
                activity,
                jni_str!("getClassLoader"),
                jni_sig!("()Ljava/lang/ClassLoader;"),
                &[],
            )?
            .l()?;
        let jname = env.new_string(name)?;
        let class = env
            .call_method(
                &loader,
                jni_str!("loadClass"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
                &[JValue::Object(&jname)],
            )?
            .l()?;
        env.cast_local::<JClass>(class)
    })();
    if env.exception_check() {
        env.exception_clear();
    }
    found.ok()
}

fn register(env: &mut jni::Env<'_>, class: &JClass<'_>, method: jni::NativeMethod<'_>) -> bool {
    // SAFETY: each NativeMethod pairs a Java signature with an
    // `extern "system"` function of exactly that shape (below).
    let ok = unsafe { env.register_native_methods(class, &[method]) }.is_ok();
    if env.exception_check() {
        env.exception_clear();
    }
    ok
}

fn guard(f: impl FnOnce() + std::panic::UnwindSafe) {
    // Never unwind into the JVM.
    let _ = std::panic::catch_unwind(f);
}

extern "system" fn native_focus_change(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    change: jni::sys::jint,
) {
    guard(|| {
        if let Some(event) = FocusEvent::from_android(change) {
            AudioSession::shared().on_focus(event);
        }
    });
}

extern "system" fn native_becoming_noisy(_env: *mut jni::sys::JNIEnv, _class: jni::sys::jclass) {
    guard(|| {
        AudioSession::shared().on_route(RouteChange::OldDeviceUnavailable);
    });
}

extern "system" fn native_command(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    command: jni::sys::jint,
    argument: jni::sys::jlong,
) {
    guard(|| {
        let ms = Duration::from_millis(u64::try_from(argument).unwrap_or(0));
        // Shared with SessionCallback.java.
        let c = match command {
            0 => RemoteCommand::Play,
            1 => RemoteCommand::Pause,
            3 => RemoteCommand::Stop,
            4 => RemoteCommand::Next,
            5 => RemoteCommand::Previous,
            6 => RemoteCommand::SeekTo(ms),
            7 => RemoteCommand::SkipForward(ms),
            8 => RemoteCommand::SkipBackward(ms),
            _ => return,
        };
        AudioSession::shared().push_remote(c);
    });
}

/// Set up the Java side: listener, receiver, media session, service class.
fn java_side(env: &mut jni::Env<'_>, activity: &JObject<'_>) -> Java {
    let mut java = Java {
        listener: None,
        session: None,
        service: None,
    };

    if let Some(class) = load(env, activity, "dev.vieww.audio.FocusListener") {
        // SAFETY: `(I)V` static ↔ fn(env, class, jint).
        let m = unsafe {
            jni::NativeMethod::from_raw_parts(
                jni_str!("nativeFocusChange"),
                jni_str!("(I)V"),
                native_focus_change as *mut std::ffi::c_void,
            )
        };
        if register(env, &class, m) {
            java.listener = env
                .new_object(&class, jni_sig!("()V"), &[])
                .and_then(|o| env.new_global_ref(&o))
                .ok();
        }
    }

    if let Some(class) = load(env, activity, "dev.vieww.audio.NoisyReceiver") {
        // SAFETY: `()V` static ↔ fn(env, class).
        let m = unsafe {
            jni::NativeMethod::from_raw_parts(
                jni_str!("nativeBecomingNoisy"),
                jni_str!("()V"),
                native_becoming_noisy as *mut std::ffi::c_void,
            )
        };
        if register(env, &class, m) {
            let registered = (|| -> JResult<()> {
                let receiver = env.new_object(&class, jni_sig!("()V"), &[])?;
                let action = env.new_string("android.media.AUDIO_BECOMING_NOISY")?;
                let filter = env.new_object(
                    jni_str!("android/content/IntentFilter"),
                    jni_sig!("(Ljava/lang/String;)V"),
                    &[JValue::Object(&action)],
                )?;
                // On the application context: one registered on the
                // activity leaks (and is logged as leaked) when the activity
                // is destroyed, and this receiver is for the process.
                let context = env
                    .call_method(
                        activity,
                        jni_str!("getApplicationContext"),
                        jni_sig!("()Landroid/content/Context;"),
                        &[],
                    )?
                    .l()?;
                env.call_method(
                    &context,
                    jni_str!("registerReceiver"),
                    jni_sig!(
                        "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)\
                         Landroid/content/Intent;"
                    ),
                    &[JValue::Object(&receiver), JValue::Object(&filter)],
                )?;
                // Process lifetime: the activity holds it.
                std::mem::forget(env.new_global_ref(&receiver)?);
                Ok(())
            })();
            if let Err(e) = registered {
                log::warn!("vieww-audio: becoming-noisy receiver not registered: {e}");
            }
        }
    }

    // The media session exists without the callback class (it still shows
    // metadata); buttons need the callback.
    let session = (|| -> JResult<Global<JObject<'static>>> {
        let tag = env.new_string("vieww")?;
        let session = env.new_object(
            jni_str!("android/media/session/MediaSession"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)V"),
            &[JValue::Object(activity), JValue::Object(&tag)],
        )?;
        env.new_global_ref(&session)
    })();
    if env.exception_check() {
        env.exception_clear();
    }
    if let Ok(session) = session {
        if let Some(class) = load(env, activity, "dev.vieww.audio.SessionCallback") {
            // SAFETY: `(IJ)V` static ↔ fn(env, class, jint, jlong).
            let m = unsafe {
                jni::NativeMethod::from_raw_parts(
                    jni_str!("nativeCommand"),
                    jni_str!("(IJ)V"),
                    native_command as *mut std::ffi::c_void,
                )
            };
            if register(env, &class, m) {
                let set = (|| -> JResult<()> {
                    let callback = env.new_object(&class, jni_sig!("()V"), &[])?;
                    // On the main looper: the one-argument `setCallback`
                    // makes a `Handler` on the calling thread, and this
                    // native thread has no Java `Looper`, so it would throw.
                    let main = env
                        .call_static_method(
                            jni_str!("android/os/Looper"),
                            jni_str!("getMainLooper"),
                            jni_sig!("()Landroid/os/Looper;"),
                            &[],
                        )?
                        .l()?;
                    let handler = env.new_object(
                        jni_str!("android/os/Handler"),
                        jni_sig!("(Landroid/os/Looper;)V"),
                        &[JValue::Object(&main)],
                    )?;
                    env.call_method(
                        session.as_obj(),
                        jni_str!("setCallback"),
                        jni_sig!(
                            "(Landroid/media/session/MediaSession$Callback;Landroid/os/Handler;)V"
                        ),
                        &[JValue::Object(&callback), JValue::Object(&handler)],
                    )?;
                    Ok(())
                })();
                if let Err(e) = set {
                    log::warn!("vieww-audio: media session callback not set: {e}");
                }
            }
        }
        java.session = Some(session);
    }

    if let Some(class) = load(env, activity, "dev.vieww.audio.PlaybackService") {
        java.service = env.new_global_ref(&class).ok();
    }
    if env.exception_check() {
        env.exception_clear();
    }
    java
}

impl AndroidBridge {
    fn set_session_active(&self, env: &mut jni::Env<'_>, active: bool) -> JResult<()> {
        if let Some(s) = &self.java.session {
            env.call_method(
                s.as_obj(),
                jni_str!("setActive"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(active)],
            )?;
        }
        Ok(())
    }

    fn start_service(&self, env: &mut jni::Env<'_>, activity: &JObject<'_>, now: &NowPlaying) {
        let (Some(service), Some(session)) = (&self.java.service, &self.java.session) else {
            return;
        };
        let started = (|| -> JResult<()> {
            let token = env
                .call_method(
                    session.as_obj(),
                    jni_str!("getSessionToken"),
                    jni_sig!("()Landroid/media/session/MediaSession$Token;"),
                    &[],
                )?
                .l()?;
            let title = env.new_string(&now.title)?;
            let artist = env.new_string(&now.artist)?;
            env.call_static_method(
                service,
                jni_str!("start"),
                jni_sig!(
                    "(Landroid/content/Context;Landroid/media/session/MediaSession$Token;\
                     Ljava/lang/String;Ljava/lang/String;)V"
                ),
                &[
                    JValue::Object(activity),
                    JValue::Object(&token),
                    JValue::Object(&title),
                    JValue::Object(&artist),
                ],
            )?;
            Ok(())
        })();
        match started {
            Ok(()) => self.servicing.store(true, Ordering::Release),
            Err(e) => log::warn!("vieww-audio: playback service not started: {e}"),
        }
    }

    fn stop_service(&self, env: &mut jni::Env<'_>, activity: &JObject<'_>) {
        let Some(service) = &self.java.service else {
            return;
        };
        if !self.servicing.swap(false, Ordering::AcqRel) {
            return;
        }
        let _ = env.call_static_method(
            service,
            jni_str!("stop"),
            jni_sig!("(Landroid/content/Context;)V"),
            &[JValue::Object(activity)],
        );
    }

    fn backgroundable(&self) -> bool {
        matches!(
            *self
                .content
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            Content::Media | Content::Speech
        )
    }
}

impl SessionBridge for AndroidBridge {
    fn name(&self) -> &'static str {
        "android"
    }

    fn request_focus(&self, request: &FocusRequest) -> bool {
        if let Ok(mut c) = self.content.lock() {
            *c = request.content;
        }
        let result = with_env(&self.app, |env, activity| {
            let am = audio_manager(env, activity)?;
            let listener = self.java.listener.as_ref().map(Global::as_obj);
            let answer = if self.sdk >= 26 {
                let builder = env.new_object(
                    jni_str!("android/media/AudioAttributes$Builder"),
                    jni_sig!("()V"),
                    &[],
                )?;
                env.call_method(
                    &builder,
                    jni_str!("setUsage"),
                    jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                    &[JValue::Int(request.android_usage())],
                )?;
                env.call_method(
                    &builder,
                    jni_str!("setContentType"),
                    jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                    &[JValue::Int(request.android_content_type())],
                )?;
                let attrs = env
                    .call_method(
                        &builder,
                        jni_str!("build"),
                        jni_sig!("()Landroid/media/AudioAttributes;"),
                        &[],
                    )?
                    .l()?;
                let fb = env.new_object(
                    jni_str!("android/media/AudioFocusRequest$Builder"),
                    jni_sig!("(I)V"),
                    &[JValue::Int(request.android_gain())],
                )?;
                env.call_method(
                    &fb,
                    jni_str!("setAudioAttributes"),
                    jni_sig!(
                        "(Landroid/media/AudioAttributes;)Landroid/media/AudioFocusRequest$Builder;"
                    ),
                    &[JValue::Object(&attrs)],
                )?;
                if let Some(l) = listener {
                    env.call_method(
                        &fb,
                        jni_str!("setOnAudioFocusChangeListener"),
                        jni_sig!(
                            "(Landroid/media/AudioManager$OnAudioFocusChangeListener;)\
                             Landroid/media/AudioFocusRequest$Builder;"
                        ),
                        &[JValue::Object(l)],
                    )?;
                    // We duck (or pause, for speech) ourselves.
                    env.call_method(
                        &fb,
                        jni_str!("setWillPauseWhenDucked"),
                        jni_sig!("(Z)Landroid/media/AudioFocusRequest$Builder;"),
                        &[JValue::Bool(true)],
                    )?;
                }
                let req = env
                    .call_method(
                        &fb,
                        jni_str!("build"),
                        jni_sig!("()Landroid/media/AudioFocusRequest;"),
                        &[],
                    )?
                    .l()?;
                let answer = env
                    .call_method(
                        &am,
                        jni_str!("requestAudioFocus"),
                        jni_sig!("(Landroid/media/AudioFocusRequest;)I"),
                        &[JValue::Object(&req)],
                    )?
                    .i()?;
                if let Ok(mut slot) = self.request.lock() {
                    *slot = env.new_global_ref(&req).ok();
                }
                answer
            } else {
                let null = JObject::null();
                env.call_method(
                    &am,
                    jni_str!("requestAudioFocus"),
                    jni_sig!("(Landroid/media/AudioManager$OnAudioFocusChangeListener;II)I"),
                    &[
                        JValue::Object(listener.unwrap_or(&null)),
                        JValue::Int(STREAM_MUSIC),
                        JValue::Int(request.android_gain()),
                    ],
                )?
                .i()?
            };
            if answer == GRANTED {
                self.set_session_active(env, true)?;
            }
            Ok(answer == GRANTED)
        });
        result.unwrap_or_else(|e| {
            // No answer at all: play rather than fall silent for a JNI
            // failure, the behaviour before focus existed.
            log::warn!("vieww-audio: requestAudioFocus failed: {e}");
            true
        })
    }

    fn abandon_focus(&self) {
        let r = with_env(&self.app, |env, activity| {
            let am = audio_manager(env, activity)?;
            if self.sdk >= 26 {
                let req = self.request.lock().ok().and_then(|mut s| s.take());
                if let Some(req) = req {
                    env.call_method(
                        &am,
                        jni_str!("abandonAudioFocusRequest"),
                        jni_sig!("(Landroid/media/AudioFocusRequest;)I"),
                        &[JValue::Object(req.as_obj())],
                    )?;
                }
            } else {
                let null = JObject::null();
                let listener = self.java.listener.as_ref().map(Global::as_obj);
                env.call_method(
                    &am,
                    jni_str!("abandonAudioFocus"),
                    jni_sig!("(Landroid/media/AudioManager$OnAudioFocusChangeListener;)I"),
                    &[JValue::Object(listener.unwrap_or(&null))],
                )?;
            }
            self.stop_service(env, activity);
            Ok(())
        });
        if let Err(e) = r {
            log::warn!("vieww-audio: abandonAudioFocus failed: {e}");
        }
    }

    fn publish(&self, now: &NowPlaying) {
        let Some(session) = &self.java.session else {
            return;
        };
        let r = with_env(&self.app, |env, activity| {
            let mb = env.new_object(
                jni_str!("android/media/MediaMetadata$Builder"),
                jni_sig!("()V"),
                &[],
            )?;
            for (key, value) in [
                ("android.media.metadata.TITLE", &now.title),
                ("android.media.metadata.ARTIST", &now.artist),
                ("android.media.metadata.ALBUM", &now.album),
            ] {
                let k = env.new_string(key)?;
                let v = env.new_string(value)?;
                env.call_method(
                    &mb,
                    jni_str!("putString"),
                    jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/media/MediaMetadata$Builder;"),
                    &[JValue::Object(&k), JValue::Object(&v)],
                )?;
            }
            if let Some(d) = now.duration {
                let k = env.new_string("android.media.metadata.DURATION")?;
                env.call_method(
                    &mb,
                    jni_str!("putLong"),
                    jni_sig!("(Ljava/lang/String;J)Landroid/media/MediaMetadata$Builder;"),
                    &[
                        JValue::Object(&k),
                        JValue::Long(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
                    ],
                )?;
            }
            let meta = env
                .call_method(
                    &mb,
                    jni_str!("build"),
                    jni_sig!("()Landroid/media/MediaMetadata;"),
                    &[],
                )?
                .l()?;
            env.call_method(
                session.as_obj(),
                jni_str!("setMetadata"),
                jni_sig!("(Landroid/media/MediaMetadata;)V"),
                &[JValue::Object(&meta)],
            )?;

            let sb = env.new_object(
                jni_str!("android/media/session/PlaybackState$Builder"),
                jni_sig!("()V"),
                &[],
            )?;
            env.call_method(
                &sb,
                jni_str!("setActions"),
                jni_sig!("(J)Landroid/media/session/PlaybackState$Builder;"),
                &[JValue::Long(ACTIONS)],
            )?;
            env.call_method(
                &sb,
                jni_str!("setState"),
                jni_sig!("(IJF)Landroid/media/session/PlaybackState$Builder;"),
                &[
                    JValue::Int(if now.playing {
                        STATE_PLAYING
                    } else {
                        STATE_PAUSED
                    }),
                    JValue::Long(i64::try_from(now.position.as_millis()).unwrap_or(i64::MAX)),
                    JValue::Float(if now.playing { 1.0 } else { 0.0 }),
                ],
            )?;
            let state = env
                .call_method(
                    &sb,
                    jni_str!("build"),
                    jni_sig!("()Landroid/media/session/PlaybackState;"),
                    &[],
                )?
                .l()?;
            env.call_method(
                session.as_obj(),
                jni_str!("setPlaybackState"),
                jni_sig!("(Landroid/media/session/PlaybackState;)V"),
                &[JValue::Object(&state)],
            )?;
            if now.playing && self.backgroundable() {
                self.start_service(env, activity, now);
            }
            Ok(())
        });
        if let Err(e) = r {
            log::warn!("vieww-audio: media session not updated: {e}");
        }
    }

    fn clear_now_playing(&self) {
        let _ = with_env(&self.app, |env, activity| {
            self.stop_service(env, activity);
            self.set_session_active(env, false)
        });
    }
}

/// Without the listener class, a call is still noticed: `getMode()` leaves
/// `MODE_NORMAL` while the phone rings or a call (or VoIP call) is up.
fn poll_calls(app: crate::AndroidApp) {
    let _ = std::thread::Builder::new()
        .name("vieww-audio-calls".into())
        .spawn(move || {
            let mut in_call = false;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let mode = with_env(&app, |env, activity| {
                    let am = audio_manager(env, activity)?;
                    env.call_method(&am, jni_str!("getMode"), jni_sig!("()I"), &[])?
                        .i()
                });
                let Ok(mode) = mode else { continue };
                let now = mode != MODE_NORMAL;
                if now != in_call {
                    in_call = now;
                    let session = AudioSession::shared();
                    session.on_interruption(if now {
                        Interruption::Began
                    } else {
                        Interruption::Ended {
                            should_resume: true,
                        }
                    });
                }
            }
        });
}

/// Register the bridge. Idempotent; the first activity wins.
pub(crate) fn install(app: &crate::AndroidApp) {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let set_up = with_env(app, |env, activity| {
            let sdk = env
                .get_static_field(
                    jni_str!("android/os/Build$VERSION"),
                    jni_str!("SDK_INT"),
                    jni_sig!("I"),
                )?
                .i()?;
            Ok((sdk, java_side(env, activity)))
        });
        let (sdk, java) = match set_up {
            Ok(v) => v,
            Err(e) => {
                log::warn!("vieww-audio: no audio session bridge: {e}");
                return;
            }
        };
        let listening = java.listener.is_some();
        log::info!(
            "vieww-audio: android session bridge (sdk {sdk}, listener {listening}, \
             media session {}, playback service {})",
            java.session.is_some(),
            java.service.is_some()
        );
        AudioSession::shared().set_bridge(Arc::new(AndroidBridge {
            app: app.clone(),
            java,
            sdk,
            request: Mutex::new(None),
            content: Mutex::new(Content::Media),
            servicing: AtomicBool::new(false),
        }));
        if !listening {
            poll_calls(app.clone());
        }
    });
}

/// A competing focus request, for the on-device audio check
/// (`audio_check.rs`): what another app's request would do, made from this
/// process with a different focus client (no listener), so Android sends
/// the loss to vieww's own listener through the real focus stack.
pub(crate) struct CompetingFocus {
    app: crate::AndroidApp,
    request: Global<JObject<'static>>,
}

impl CompetingFocus {
    /// Ask for focus with `gain` (`AUDIOFOCUS_GAIN_TRANSIENT` = 2,
    /// `..._MAY_DUCK` = 3). `None` if the request could not be made or was
    /// refused.
    pub(crate) fn claim(app: &crate::AndroidApp, gain: i32) -> Option<Self> {
        let made = with_env(app, |env, activity| {
            let am = audio_manager(env, activity)?;
            let builder = env.new_object(
                jni_str!("android/media/AudioAttributes$Builder"),
                jni_sig!("()V"),
                &[],
            )?;
            env.call_method(
                &builder,
                jni_str!("setUsage"),
                jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                // USAGE_ASSISTANCE_NAVIGATION_GUIDANCE: a prompt, as a
                // navigation app's would be.
                &[JValue::Int(12)],
            )?;
            let attrs = env
                .call_method(
                    &builder,
                    jni_str!("build"),
                    jni_sig!("()Landroid/media/AudioAttributes;"),
                    &[],
                )?
                .l()?;
            let fb = env.new_object(
                jni_str!("android/media/AudioFocusRequest$Builder"),
                jni_sig!("(I)V"),
                &[JValue::Int(gain)],
            )?;
            env.call_method(
                &fb,
                jni_str!("setAudioAttributes"),
                jni_sig!(
                    "(Landroid/media/AudioAttributes;)Landroid/media/AudioFocusRequest$Builder;"
                ),
                &[JValue::Object(&attrs)],
            )?;
            let req = env
                .call_method(
                    &fb,
                    jni_str!("build"),
                    jni_sig!("()Landroid/media/AudioFocusRequest;"),
                    &[],
                )?
                .l()?;
            let answer = env
                .call_method(
                    &am,
                    jni_str!("requestAudioFocus"),
                    jni_sig!("(Landroid/media/AudioFocusRequest;)I"),
                    &[JValue::Object(&req)],
                )?
                .i()?;
            (answer == GRANTED)
                .then(|| env.new_global_ref(&req))
                .transpose()
        });
        match made {
            Ok(Some(request)) => Some(Self {
                app: app.clone(),
                request,
            }),
            Ok(None) => None,
            Err(e) => {
                log::warn!("vieww-audio check: competing focus request failed: {e}");
                None
            }
        }
    }

    /// Give it back: vieww's listener hears `AUDIOFOCUS_GAIN`.
    pub(crate) fn release(self) {
        let _ = with_env(&self.app, |env, activity| {
            let am = audio_manager(env, activity)?;
            env.call_method(
                &am,
                jni_str!("abandonAudioFocusRequest"),
                jni_sig!("(Landroid/media/AudioFocusRequest;)I"),
                &[JValue::Object(self.request.as_obj())],
            )?;
            Ok(())
        });
    }
}

/// Media keys pressed while the app is in front (AU-04).
///
/// Android gives a media key to the focused window first and routes it to
/// the media session only if the window leaves it unhandled; winit marks
/// every key but volume as handled. So a headset's play/pause button, a
/// Bluetooth remote or `input keyevent KEYCODE_MEDIA_PAUSE` never reached
/// the `MediaSession` while a vieww app was open (Galaxy A73, Device Farm
/// run 40818820451). The platform crate sees the key, so it forwards it as
/// the same [`RemoteCommand`] the session callback would have produced.
pub(crate) fn forward_media_key(key: &winit::keyboard::Key) {
    use winit::keyboard::{Key, NamedKey};
    let skip = Duration::from_secs(15);
    let command = match key {
        Key::Named(NamedKey::MediaPlay) => RemoteCommand::Play,
        Key::Named(NamedKey::MediaPause) => RemoteCommand::Pause,
        Key::Named(NamedKey::MediaPlayPause | NamedKey::HeadsetHook) => {
            RemoteCommand::TogglePlayPause
        }
        Key::Named(NamedKey::MediaStop) => RemoteCommand::Stop,
        Key::Named(NamedKey::MediaTrackNext) => RemoteCommand::Next,
        Key::Named(NamedKey::MediaTrackPrevious) => RemoteCommand::Previous,
        Key::Named(NamedKey::MediaFastForward) => RemoteCommand::SkipForward(skip),
        Key::Named(NamedKey::MediaRewind) => RemoteCommand::SkipBackward(skip),
        _ => return,
    };
    AudioSession::shared().push_remote(command);
}
