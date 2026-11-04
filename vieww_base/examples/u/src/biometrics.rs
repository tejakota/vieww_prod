//! The biometric lock's platform half: the Android shim behind
//! vieww-foundation's [`Biometrics`] trait, and the permissions provider the
//! service's gate needs. This is what makes the settings toggle real: on a
//! phone, flipping it gates the app behind the system's fingerprint or face
//! sheet; on a desktop there is no sensor, the honest `NoBiometrics` answers
//! `available` empty, the row says so, and the gate simply never appears.
//!
//! # The shim, and why it is polled
//!
//! The Java class in `java/UBiometricShim.java` is compiled and dexed by this
//! crate's `build.rs` (the vavlt / three-capture pattern: no Gradle, no AGP)
//! and loaded through `InMemoryDexClassLoader`. android-activity does not
//! surface Java callbacks, and a native method on a class from a foreign
//! classloader would need `RegisterNatives` — so the shim holds the prompt's
//! outcome in a static field and Rust polls it, which is the same rule the
//! camera shim follows: Java never calls back.
//!
//! The prompt must start on the Java main thread. `authenticate` is called
//! there already — a tap handler — so it posts the start through
//! `AndroidApp::run_on_java_main_thread` and hands the *waiting* to a
//! worker, which polls the shim until the sheet settles. The worker carries
//! only raw JNI globals (`Raw`, `Copy`), which is how it is `Send` without
//! any of the app's `Rc` state crossing threads.
//!
//! # jni, pinned
//!
//! This module is written against `jni` 0.22 — the version
//! vieww-platform-winit resolves for this target, whose callback-style
//! `attach_current_thread` and `Desc`-based calls are what the platform's
//! own `insets.rs` uses. A second `jni` in the binary would be a second
//! `JavaVM` type that looks identical and is not.

use std::rc::Rc;

use vieww_foundation::capability::{Biometrics, NoBiometrics};
use vieww_foundation::permission::Permissions;

/// The registry entries the app publishes above the root: the capability
/// (real on Android, the honest refusal elsewhere) and — on Android, where
/// the gate will actually be exercised — the permission resolver. Without
/// the resolver, `Services::gate` answers `None` and the lock screen cannot
/// exist; that is the framework's rule, and it is correct: asking a person
/// for a finger on a device that cannot check one would be a worse lie than
/// hiding the button.
///
/// `android` is the activity handle the platform handed the app.
#[cfg(target_os = "android")]
#[must_use]
pub fn entries(
    android: Option<&vieww_platform_winit::AndroidApp>,
) -> (Rc<dyn Biometrics>, Option<Rc<dyn Permissions>>) {
    match android.and_then(android::load) {
        Some(loaded) => (
            Rc::new(android::AndroidBiometrics {
                bridge: loaded.clone(),
            }),
            Some(Rc::new(loaded.permissions())),
        ),
        None => (Rc::new(NoBiometrics), None),
    }
}

/// Every other platform: the honest refusal. There is no sensor to check,
/// so the settings row says so and the lock never appears.
#[cfg(not(target_os = "android"))]
#[must_use]
pub fn entries() -> (Rc<dyn Biometrics>, Option<Rc<dyn Permissions>>) {
    (Rc::new(NoBiometrics), None)
}

#[cfg(target_os = "android")]
mod android {
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use jni::objects::{JClass, JObject, JValue};
    use jni::{jni_sig, jni_str, Env, JavaVM};
    use vieww_foundation::capability::{
        BiometricKind, BiometricPrompt, BiometricStrength, Biometrics,
    };
    use vieww_foundation::permission::{Grant, Permission, PermissionState, Permissions};
    use vieww_foundation::service::ServiceError;
    use vieww_foundation::task::{NoWaker, Task, Threads};
    use vieww_platform_winit::AndroidApp;

    /// The dex the build script produced, embedded — the shim travels inside
    /// the binary, so no APK packaging step knows it exists.
    const DEX: &[u8] = include_bytes!(env!("U_BIOMETRIC_DEX"));
    /// The shim's class, by the name it goes by.
    const CLASS: &str = "dev.vieww.u.UBiometricShim";

    /// The shim's state codes, mirrored verbatim. IDLE and RUNNING are kept
    /// for the record: the poll loop treats both as "keep waiting".
    #[allow(
        dead_code,
        reason = "mirrored from the Java shim; waited on through `Ok(_)`"
    )]
    const IDLE: i32 = 0;
    #[allow(
        dead_code,
        reason = "mirrored from the Java shim; waited on through `Ok(_)`"
    )]
    const RUNNING: i32 = 1;
    const OK: i32 = 2;
    const CANCELLED: i32 = 3;
    const DENIED: i32 = 4;
    const UNAVAILABLE: i32 = 5;

    /// How long the worker waits for the sheet to settle. Ninety seconds is
    /// deliberately generous — a person deciding whether to try the finger
    /// again is not a hang.
    const PROMPT_TIMEOUT: Duration = Duration::from_secs(90);
    /// The poll interval: a tenth of a second of a person's fingerprint
    /// wait, on a worker that costs nothing else.
    const POLL_EVERY: Duration = Duration::from_millis(100);

    /// Availability cache values: 0 unknown (ask the shim once), 1 strong,
    /// 2 weak-only, 5 unavailable. Cached because `available` is called from
    /// `build`, where a JNI round trip per frame is the one cost a settings
    /// screen must not pay.
    const UNKNOWN: u8 = 0;

    /// The JNI handles the shim lives behind, as numbers so any thread can
    /// carry them. The activity was promoted to a global reference at load
    /// time and is deliberately never deleted — the process owns it, and one
    /// leaked global is the price of the shim surviving past `load`. The
    /// class itself travels as an `Arc<Global>` — `Global` is `Send`, so the
    /// worker carries it whole rather than as another pointer.
    #[derive(Clone, Copy)]
    pub(super) struct Raw {
        vm: usize,
        activity: usize,
    }

    /// The shim's class, by the handle the loader made — never by name,
    /// because `FindClass` from a bare native thread resolves against the
    /// system classloader, which has never heard of the embedded dex.
    pub(super) type ShimClass = jni::objects::Global<jni::objects::JClass<'static>>;

    impl Raw {
        /// Attach this thread and run one closure against the JVM. Every
        /// JNI call the bridge makes goes through here. On error, a pending
        /// Java exception is cleared on a second pass — the one place that
        /// can reach the environment after the failed callback let go of it.
        fn with_env<T>(
            &self,
            run: impl FnOnce(&mut Env) -> Result<T, jni::errors::Error>,
        ) -> Result<T, ServiceError> {
            let vm = unsafe { JavaVM::from_raw(self.vm as *mut _) };
            let result = vm.attach_current_thread(run);
            if result.is_err() {
                let _ = vm.attach_current_thread(|env| -> Result<(), jni::errors::Error> {
                    if env.exception_check() {
                        env.exception_clear();
                    }
                    Ok(())
                });
            }
            result.map_err(|error| ServiceError::Failed(format!("jni: {error}")))
        }

        /// The activity, a `'static` global reference wrapped per call.
        fn activity<'e>(&self, env: &Env<'e>) -> JObject<'e> {
            // SAFETY: a global reference, held for the life of the process.
            unsafe { JObject::from_raw(env, self.activity as jni::sys::jobject) }
        }

        /// What the hardware can check: 1 strong, 2 weak-only, 5 nothing.
        fn availability(&self, class: &ShimClass) -> Result<i32, ServiceError> {
            self.with_env(|env| {
                let activity = self.activity(env);
                env.call_static_method(
                    class,
                    jni_str!("availability"),
                    jni_sig!("(Landroid/content/Context;)I"),
                    &[JValue::Object(&activity)],
                )
                .and_then(|value| value.i())
            })
        }

        /// The shim's current state code.
        fn state(&self, class: &ShimClass) -> Result<i32, ServiceError> {
            self.with_env(|env| {
                env.call_static_method(class, jni_str!("state"), jni_sig!("()I"), &[])
                    .and_then(|value| value.i())
            })
        }

        /// What the settled check was worth: 1 strong, 0 weak.
        fn strength(&self, class: &ShimClass) -> i32 {
            self.with_env(|env| {
                env.call_static_method(class, jni_str!("strength"), jni_sig!("()I"), &[])
                    .and_then(|value| value.i())
            })
            .unwrap_or(1)
        }

        /// Reset the shim to idle, so a later prompt can begin.
        fn reset(&self, class: &ShimClass) {
            let _ = self.with_env(|env| {
                env.call_static_method(class, jni_str!("reset"), jni_sig!("()V"), &[])?;
                Ok(())
            });
        }

        /// Start the prompt. Must run on the Java main thread — the one
        /// thread the system sheet may be shown from.
        fn begin(&self, class: &ShimClass, prompt: &BiometricPrompt) {
            let _ = self.with_env(|env| {
                let activity = self.activity(env);
                let title = env.new_string(&prompt.title)?;
                let subtitle = env.new_string(prompt.subtitle.as_deref().unwrap_or_default())?;
                let cancel = env.new_string(&prompt.cancel_label)?;
                env.call_static_method(
                    class,
                    jni_str!("authenticate"),
                    jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)V"),
                    &[
                        JValue::Object(&activity),
                        JValue::Object(&title),
                        JValue::Object(&subtitle),
                        JValue::Object(&cancel),
                        JValue::Bool(prompt.allow_device_credential),
                    ],
                )?;
                Ok(())
            });
        }
    }

    /// One shim load, shared between the service and its permissions
    /// resolver — two handles on the same globals.
    #[derive(Clone)]
    pub(super) struct Loaded {
        android: AndroidApp,
        raw: Raw,
        class: Arc<ShimClass>,
        availability: Arc<AtomicU8>,
    }

    impl Loaded {
        /// What the hardware can check, asked once and cached: the answer
        /// changes only when a person enrolls or deletes a fingerprint,
        /// which is a settings visit away, and `build` must not pay a JNI
        /// round trip every frame.
        fn availability(&self) -> u8 {
            let cached = self.availability.load(Ordering::Relaxed);
            if cached != UNKNOWN {
                return cached;
            }
            let asked = self
                .raw
                .availability(&self.class)
                .unwrap_or(UNAVAILABLE)
                .clamp(0, u8::MAX as i32) as u8;
            self.availability.store(asked, Ordering::Relaxed);
            asked
        }

        /// The permissions resolver for the shim's gate. `USE_BIOMETRIC` is
        /// a *normal* Android permission — granted at install, never asked
        /// for — so the state is a fact read from the OS, and `request` has
        /// nothing to show: it reports the state back, which is the honest
        /// shape for a permission that cannot prompt.
        pub(super) fn permissions(&self) -> AndroidPermissions {
            AndroidPermissions { raw: self.raw }
        }
    }

    /// Whether `android.permission.USE_BIOMETRIC` is granted on this install.
    fn use_biometric_granted(raw: &Raw) -> bool {
        raw.with_env(|env| {
            let activity = raw.activity(env);
            let name = env.new_string("android.permission.USE_BIOMETRIC")?;
            env.call_method(
                &activity,
                jni_str!("checkSelfPermission"),
                jni_sig!("(Ljava/lang/String;)I"),
                &[JValue::Object(&name)],
            )
            .and_then(|value| value.i())
            .map(|granted| granted == 0)
        })
        .unwrap_or(false)
    }

    /// The permissions resolver behind the biometric gate.
    pub(super) struct AndroidPermissions {
        raw: Raw,
    }

    impl Permissions for AndroidPermissions {
        fn state(&self, permission: Permission) -> PermissionState {
            if permission.name() == "biometrics" && use_biometric_granted(&self.raw) {
                PermissionState::Granted
            } else {
                PermissionState::Denied
            }
        }

        fn request(&self, permission: Permission, then: Rc<dyn Fn(PermissionState)>) {
            then(self.state(permission));
        }
    }

    /// The real thing: the system sheet behind the trait.
    pub(super) struct AndroidBiometrics {
        pub(super) bridge: Loaded,
    }

    impl Biometrics for AndroidBiometrics {
        fn available(&self) -> Vec<(BiometricKind, BiometricStrength)> {
            match self.bridge.availability() {
                1 => vec![(BiometricKind::Fingerprint, BiometricStrength::Strong)],
                2 => vec![(BiometricKind::Face, BiometricStrength::Weak)],
                _ => Vec::new(),
            }
        }

        fn authenticate(
            &self,
            prompt: &BiometricPrompt,
            _grant: &Grant<'_, dyn Biometrics>,
        ) -> Task<BiometricStrength, ServiceError> {
            // The start goes to the Java main thread — `authenticate` is
            // called from a tap handler, which *is* the main thread, but it
            // is posted rather than assumed, because the trait does not say
            // where it is called from and the sheet refuses to be shown
            // from anywhere else.
            let raw = self.bridge.raw;
            let class = Arc::clone(&self.bridge.class);
            let android = self.bridge.android.clone();
            let prompt = prompt.clone();
            android.run_on_java_main_thread(Box::new(move || {
                raw.begin(&class, &prompt);
            }));

            // The worker only polls: `raw` is `Copy` and the class is an
            // `Rc` over a `'static` global, so this closure is `Send` by
            // construction. `NoWaker` is deliberate — the lock screen reads
            // the clock, so it draws frames for the whole wait and notices
            // the result the moment it lands.
            let raw = self.bridge.raw;
            let class = Arc::clone(&self.bridge.class);
            Task::spawn(&Threads, Arc::new(NoWaker), move || {
                let deadline = Instant::now() + PROMPT_TIMEOUT;
                loop {
                    match raw.state(&class) {
                        Ok(OK) => {
                            let strength = if raw.strength(&class) >= 1 {
                                BiometricStrength::Strong
                            } else {
                                BiometricStrength::Weak
                            };
                            raw.reset(&class);
                            return Ok(strength);
                        }
                        Ok(CANCELLED) => {
                            raw.reset(&class);
                            return Err(ServiceError::Denied("cancelled".to_owned()));
                        }
                        Ok(DENIED) => {
                            raw.reset(&class);
                            return Err(ServiceError::Denied("not recognised".to_owned()));
                        }
                        Ok(UNAVAILABLE) => {
                            raw.reset(&class);
                            return Err(ServiceError::Failed(
                                "no biometric enrolled on this device".to_owned(),
                            ));
                        }
                        Ok(_) => {
                            // IDLE: the posted start has not run yet. The
                            // loop simply waits — the timeout below covers
                            // the post that never ran.
                        }
                        Err(error) => {
                            raw.reset(&class);
                            return Err(error);
                        }
                    }
                    if Instant::now() >= deadline {
                        raw.reset(&class);
                        return Err(ServiceError::Failed("the prompt timed out".to_owned()));
                    }
                    std::thread::sleep(POLL_EVERY);
                }
            })
        }
    }

    /// Load the shim: the dex, the class loader, the class, the globals.
    /// `None` on any refusal — the app then opens on the honest no-hardware
    /// answer rather than on a bridge that half-exists.
    pub(super) fn load(android: &AndroidApp) -> Option<Loaded> {
        let vm_ptr = android.vm_as_ptr() as usize;
        let vm = unsafe { JavaVM::from_raw(android.vm_as_ptr().cast()) };
        let activity_ptr = android.activity_as_ptr() as usize;

        let class = vm
            .attach_current_thread(|env| -> Result<ShimClass, jni::errors::Error> {
                // SAFETY: android-activity owns this reference for the life
                // of the process; it is promoted to a global reference
                // immediately.
                let activity = unsafe { JObject::from_raw(env, android.activity_as_ptr().cast()) };
                let activity = env.new_global_ref(activity)?;

                // The dex is 'static, so the direct buffer it backs stays
                // valid for as long as the class loader might touch it.
                // SAFETY: the buffer outlives the loader.
                let buf =
                    unsafe { env.new_direct_byte_buffer(DEX.as_ptr() as *mut u8, DEX.len())? };
                let parent = env
                    .call_method(
                        &activity,
                        jni_str!("getClassLoader"),
                        jni_sig!("()Ljava/lang/ClassLoader;"),
                        &[],
                    )?
                    .l()?;
                let loader = env.new_object(
                    jni_str!("dalvik/system/InMemoryDexClassLoader"),
                    jni_sig!("(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V"),
                    &[JValue::Object(&buf), JValue::Object(&parent)],
                )?;
                let name = env.new_string(CLASS)?;
                let class = env
                    .call_method(
                        &loader,
                        jni_str!("loadClass"),
                        jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
                        &[JValue::Object(&name)],
                    )?
                    .l()?;
                // The reference that came back is typed as a plain object;
                // the class wrapper is what the calls ahead want, and the
                // macro-generated `from_raw` re-types the same reference.
                let class = unsafe { JClass::from_raw(env, class.as_raw()) };
                env.new_global_ref(class)
            })
            .ok()?;

        Some(Loaded {
            android: android.clone(),
            raw: Raw {
                vm: vm_ptr,
                activity: activity_ptr,
            },
            class: Arc::new(class),
            availability: Arc::new(AtomicU8::new(UNKNOWN)),
        })
    }
}
