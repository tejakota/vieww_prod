//! Native haptics, share sheet and photo picker (UX-02).
//!
//! # Status: compiled for both phones, not yet run on either
//!
//! Every native call below type-checks for `aarch64-linux-android` and
//! `aarch64-apple-ios` and has **not executed on a device**: this repository's
//! farm runs have not reached it yet. Treat the selectors, JNI signatures and
//! thread choices as the design, and expect the first device run to correct
//! one of them — which is why each service still answers
//! [`ServiceError`](vieww_foundation::ServiceError) rather than panicking when a call fails.
//!
//! | | Android | iOS |
//! |---|---|---|
//! | [`Haptics`](vieww_foundation::mobile::Haptics) | `View.performHapticFeedback` on the decor view (no permission, honours the person's haptics setting) | `UIImpactFeedbackGenerator` / `UINotificationFeedbackGenerator` / `UISelectionFeedbackGenerator` |
//! | [`Share`](vieww_foundation::mobile::Share) | `Intent.ACTION_SEND` through `Intent.createChooser` (text and links) | `UIActivityViewController` (text, links, files) |
//! | [`PhotoPicker`](vieww_foundation::mobile::PhotoPicker) | not here: the system picker answers through `onActivityResult`, which a `NativeActivity` never receives without a Java shim (vavlt ships its own) | `PHPickerViewController`, out of process, no library permission |
//!
//! Android shares only text and links: a file needs a `content://` URI, which
//! means a `FileProvider` declared in the app's manifest; a `file://` URI is
//! refused by Android 7 and later. A file share there answers `Unsupported`
//! with that reason, so an app can fall back (vavlt and upxcale write to the
//! app's own folder).

#[cfg(any(target_os = "android", target_os = "ios"))]
use vieww_foundation::mobile::{HapticPattern, Haptics, Share, ShareItem};
#[cfg(any(target_os = "android", target_os = "ios"))]
use vieww_foundation::ServiceError;

/// The text a share sheet carries: the message, then the link on its own line.
#[must_use]
pub fn share_text(item: &vieww_foundation::mobile::ShareItem) -> String {
    match (&item.url, item.text.is_empty()) {
        (Some(url), true) => url.clone(),
        (Some(url), false) => format!("{}\n{url}", item.text),
        (None, _) => item.text.clone(),
    }
}

// ─── Android ─────────────────────────────────────────────────────────────

/// `android.view.HapticFeedbackConstants` for a pattern. `CONFIRM` and
/// `REJECT` arrived in API 30; on older releases the view ignores an
/// unknown constant, so `fallback` is what they degrade to.
#[must_use]
pub const fn android_haptic_constant(
    pattern: vieww_foundation::mobile::HapticPattern,
) -> (i32, i32) {
    use vieww_foundation::mobile::HapticPattern as P;
    // (constant, fallback before API 30)
    match pattern {
        P::Selection => (4, 4), // CLOCK_TICK
        P::Light => (1, 1),     // VIRTUAL_KEY
        P::Medium => (3, 3),    // KEYBOARD_TAP
        P::Heavy => (0, 0),     // LONG_PRESS
        P::Success => (16, 1),  // CONFIRM, else VIRTUAL_KEY
        P::Warning => (0, 0),   // LONG_PRESS
        P::Error => (17, 0),    // REJECT, else LONG_PRESS
    }
}

#[cfg(target_os = "android")]
mod android {
    use jni::objects::{JObject, JValue};
    use jni::{jni_sig, jni_str};

    use super::{
        android_haptic_constant, share_text, HapticPattern, Haptics, ServiceError, Share, ShareItem,
    };

    /// Haptics and the share sheet, through the running activity.
    #[derive(Debug, Clone)]
    pub struct AndroidServices {
        pub(crate) android: crate::AndroidApp,
    }

    impl AndroidServices {
        /// The services for this activity — what `for_android` registers, for
        /// a host that wants one directly.
        #[must_use]
        pub fn new(android: &crate::AndroidApp) -> Self {
            Self {
                android: android.clone(),
            }
        }

        /// Run `f` with the JNI env and the activity.
        fn with_activity<T>(
            &self,
            f: impl FnOnce(&mut jni::Env<'_>, &JObject<'_>) -> Result<T, jni::errors::Error>,
        ) -> Result<T, ServiceError> {
            if self.android.vm_as_ptr().is_null() || self.android.activity_as_ptr().is_null() {
                return Err(ServiceError::unsupported("no activity"));
            }
            // SAFETY: both pointers belong to the running activity and are not
            // kept past this call — the contract `deep_links.rs` relies on.
            let vm = unsafe { jni::JavaVM::from_raw(self.android.vm_as_ptr().cast()) };
            let activity_ptr = self.android.activity_as_ptr();
            vm.attach_current_thread(|env| {
                let activity = unsafe { JObject::from_raw(env, activity_ptr.cast()) };
                f(env, &activity)
            })
            .map_err(|e| ServiceError::failed(format!("JNI: {e}")))
        }

        fn sdk_int(env: &mut jni::Env<'_>) -> Result<i32, jni::errors::Error> {
            env.get_static_field(
                jni_str!("android/os/Build$VERSION"),
                jni_str!("SDK_INT"),
                jni_sig!("I"),
            )?
            .i()
        }
    }

    impl Haptics for AndroidServices {
        fn play(&self, pattern: HapticPattern) -> Result<(), ServiceError> {
            let (constant, fallback) = android_haptic_constant(pattern);
            let played = self.with_activity(|env, activity| {
                let window = env
                    .call_method(
                        activity,
                        jni_str!("getWindow"),
                        jni_sig!("()Landroid/view/Window;"),
                        &[],
                    )?
                    .l()?;
                let view = env
                    .call_method(
                        &window,
                        jni_str!("getDecorView"),
                        jni_sig!("()Landroid/view/View;"),
                        &[],
                    )?
                    .l()?;
                let c = if Self::sdk_int(env)? >= 30 {
                    constant
                } else {
                    fallback
                };
                env.call_method(
                    &view,
                    jni_str!("performHapticFeedback"),
                    jni_sig!("(I)Z"),
                    &[JValue::Int(c)],
                )?
                .z()
            })?;
            if played {
                Ok(())
            } else {
                // The person turned touch feedback off, or the view refused.
                Err(ServiceError::failed("the view did not play the haptic"))
            }
        }
    }

    impl Share for AndroidServices {
        fn share(&self, item: ShareItem) -> Result<(), ServiceError> {
            if item.file.is_some() {
                return Err(ServiceError::unsupported(
                    "share: a file needs a FileProvider content:// URI on Android",
                ));
            }
            let text = share_text(&item);
            self.with_activity(|env, activity| {
                let action = env.new_string("android.intent.action.SEND")?;
                let intent = env.new_object(
                    jni_str!("android/content/Intent"),
                    jni_sig!("(Ljava/lang/String;)V"),
                    &[JValue::Object(&action)],
                )?;
                let mime = env.new_string("text/plain")?;
                env.call_method(
                    &intent,
                    jni_str!("setType"),
                    jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
                    &[JValue::Object(&mime)],
                )?;
                let key = env.new_string("android.intent.extra.TEXT")?;
                let value = env.new_string(&text)?;
                env.call_method(
                    &intent,
                    jni_str!("putExtra"),
                    jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;"),
                    &[JValue::Object(&key), JValue::Object(&value)],
                )?;
                if !item.text.is_empty() {
                    let key = env.new_string("android.intent.extra.SUBJECT")?;
                    let value = env.new_string(&item.text)?;
                    env.call_method(
                        &intent,
                        jni_str!("putExtra"),
                        jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;"),
                        &[JValue::Object(&key), JValue::Object(&value)],
                    )?;
                }
                let chooser = env
                    .call_static_method(
                        jni_str!("android/content/Intent"),
                        jni_str!("createChooser"),
                        jni_sig!("(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;"),
                        &[JValue::Object(&intent), JValue::Object(&JObject::null())],
                    )?
                    .l()?;
                env.call_method(
                    activity,
                    jni_str!("startActivity"),
                    jni_sig!("(Landroid/content/Intent;)V"),
                    &[JValue::Object(&chooser)],
                )?;
                Ok(())
            })
        }
    }
}

#[cfg(target_os = "android")]
pub use android::AndroidServices;

// ─── iOS ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "ios")]
mod ios {
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};

    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject};
    use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
    use vieww_foundation::mobile::{PhotoPicker, PickedPhoto};

    use super::{share_text, HapticPattern, Haptics, ServiceError, Share, ShareItem};

    // The picker's classes live in PhotosUI, which nothing else links.
    #[link(name = "PhotosUI", kind = "framework")]
    extern "C" {}

    /// A UIKit / PhotosUI class by name; `Unsupported` (naming it) where this
    /// iOS release has none.
    fn class(name: &'static str) -> Result<&'static AnyClass, ServiceError> {
        AnyClass::get(name).ok_or(ServiceError::unsupported(name))
    }

    fn ns_string(s: &str) -> Retained<NSObject> {
        let s = objc2_foundation::NSString::from_str(s);
        // SAFETY: an NSString is an NSObject.
        unsafe { Retained::cast(s) }
    }

    /// The view controller to present over: the key window's root, then
    /// whatever it is already presenting.
    fn presenter() -> Result<*mut AnyObject, ServiceError> {
        let app_class = class("UIApplication")?;
        let app: *mut AnyObject = unsafe { msg_send![app_class, sharedApplication] };
        if app.is_null() {
            return Err(ServiceError::unsupported("no UIApplication"));
        }
        let window: *mut AnyObject = unsafe { msg_send![app, keyWindow] };
        if window.is_null() {
            return Err(ServiceError::failed("no key window to present over"));
        }
        let mut vc: *mut AnyObject = unsafe { msg_send![window, rootViewController] };
        loop {
            if vc.is_null() {
                return Err(ServiceError::failed("no root view controller"));
            }
            let next: *mut AnyObject = unsafe { msg_send![vc, presentedViewController] };
            if next.is_null() {
                return Ok(vc);
            }
            vc = next;
        }
    }

    /// The share sheet, haptics and the photo picker, through UIKit.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct IosServices;

    impl Haptics for IosServices {
        fn play(&self, pattern: HapticPattern) -> Result<(), ServiceError> {
            // UIImpactFeedbackStyle: Light 0, Medium 1, Heavy 2.
            // UINotificationFeedbackType: Success 0, Warning 1, Error 2.
            let (name, arg): (&str, Option<isize>) = match pattern {
                HapticPattern::Selection => ("UISelectionFeedbackGenerator", None),
                HapticPattern::Light => ("UIImpactFeedbackGenerator", Some(0)),
                HapticPattern::Medium => ("UIImpactFeedbackGenerator", Some(1)),
                HapticPattern::Heavy => ("UIImpactFeedbackGenerator", Some(2)),
                HapticPattern::Success => ("UINotificationFeedbackGenerator", Some(0)),
                HapticPattern::Warning => ("UINotificationFeedbackGenerator", Some(1)),
                HapticPattern::Error => ("UINotificationFeedbackGenerator", Some(2)),
            };
            let cls = class(name)?;
            unsafe {
                let alloc: *mut AnyObject = msg_send![cls, alloc];
                let generator: *mut AnyObject = match (name, arg) {
                    ("UIImpactFeedbackGenerator", Some(style)) => {
                        msg_send![alloc, initWithStyle: style]
                    }
                    _ => msg_send![alloc, init],
                };
                if generator.is_null() {
                    return Err(ServiceError::failed(
                        "the feedback generator did not initialise",
                    ));
                }
                match (name, arg) {
                    ("UINotificationFeedbackGenerator", Some(kind)) => {
                        let () = msg_send![generator, notificationOccurred: kind];
                    }
                    ("UISelectionFeedbackGenerator", _) => {
                        let () = msg_send![generator, selectionChanged];
                    }
                    _ => {
                        let () = msg_send![generator, impactOccurred];
                    }
                }
                let () = msg_send![generator, release];
            }
            Ok(())
        }
    }

    impl Share for IosServices {
        fn share(&self, item: ShareItem) -> Result<(), ServiceError> {
            let mut items: Vec<Retained<NSObject>> = Vec::new();
            let text = share_text(&ShareItem {
                url: None,
                ..item.clone()
            });
            if !text.is_empty() {
                items.push(ns_string(&text));
            }
            if let Some(url) = &item.url {
                let url_class = class("NSURL")?;
                let s = ns_string(url);
                let u: *mut AnyObject = unsafe { msg_send![url_class, URLWithString: &*s] };
                if !u.is_null() {
                    // SAFETY: a +0 autoreleased NSURL, retained for the array.
                    if let Some(u) = unsafe { Retained::retain(u.cast::<NSObject>()) } {
                        items.push(u);
                    }
                }
            }
            if let Some((name, bytes)) = &item.file {
                // The sheet shares a file by URL: written to the app's own
                // temporary directory first, under the name it should carry.
                let path = std::env::temp_dir().join(name);
                std::fs::write(&path, bytes)
                    .map_err(|e| ServiceError::failed(format!("share: {e}")))?;
                let url_class = class("NSURL")?;
                let s = ns_string(&path.to_string_lossy());
                let u: *mut AnyObject = unsafe { msg_send![url_class, fileURLWithPath: &*s] };
                if let Some(u) = unsafe { Retained::retain(u.cast::<NSObject>()) } {
                    items.push(u);
                }
            }
            if items.is_empty() {
                return Err(ServiceError::failed("share: nothing to share"));
            }
            let array = objc2_foundation::NSArray::from_vec(items);
            let vc_class = class("UIActivityViewController")?;
            let presenter = presenter()?;
            unsafe {
                let alloc: *mut AnyObject = msg_send![vc_class, alloc];
                let nil: *mut AnyObject = std::ptr::null_mut();
                let sheet: *mut AnyObject =
                    msg_send![alloc, initWithActivityItems: &*array, applicationActivities: nil];
                if sheet.is_null() {
                    return Err(ServiceError::failed("the share sheet did not initialise"));
                }
                // On an iPad the sheet is a popover, and UIKit raises an
                // exception for a popover with nothing to point at.
                let popover: *mut AnyObject = msg_send![sheet, popoverPresentationController];
                if !popover.is_null() {
                    let view: *mut AnyObject = msg_send![presenter, view];
                    let () = msg_send![popover, setSourceView: view];
                }
                let () = msg_send![presenter, presentViewController: sheet, animated: Bool::YES, completion: nil];
                // The presenter retains what it presents.
                let () = msg_send![sheet, release];
            }
            Ok(())
        }
    }

    // ── the picker ──────────────────────────────────────────────────────

    /// What one presentation collects: photos land from PhotosUI's own queue,
    /// and the last one sends `deliver` to the main thread.
    #[derive(Default)]
    struct Collect {
        remaining: usize,
        photos: Vec<PickedPhoto>,
    }

    /// Where a picker's answer goes, once.
    type PickDone = Box<dyn FnOnce(Result<Vec<PickedPhoto>, ServiceError>)>;

    pub(super) struct PickerIvars {
        collect: Arc<Mutex<Collect>>,
        done: RefCell<Option<PickDone>>,
    }

    thread_local! {
        /// Delegates whose results have not been delivered yet. UIKit holds a
        /// picker's delegate weakly, so this is what keeps each one alive
        /// until its `deliver` has run on the main thread.
        static PENDING: RefCell<Vec<Retained<PickerDelegate>>> = const { RefCell::new(Vec::new()) };
    }

    declare_class!(
        /// The `PHPickerViewControllerDelegate` for one presentation.
        pub(super) struct PickerDelegate;

        // SAFETY: NSObject has no subclassing requirements; the ivars are
        // only touched on the main thread except `collect`, which is a Mutex.
        unsafe impl ClassType for PickerDelegate {
            type Super = NSObject;
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "ViewwPhotoPickerDelegate";
        }

        impl DeclaredClass for PickerDelegate {
            type Ivars = PickerIvars;
        }

        unsafe impl PickerDelegate {
            #[method(picker:didFinishPicking:)]
            fn did_finish(&self, picker: *mut AnyObject, results: *mut AnyObject) {
                unsafe {
                    let nil: *mut AnyObject = std::ptr::null_mut();
                    let () = msg_send![picker, dismissViewControllerAnimated: Bool::YES, completion: nil];
                }
                let count: usize = if results.is_null() { 0 } else { unsafe { msg_send![results, count] } };
                if count == 0 {
                    // Cancelled: an empty list, not an error.
                    self.deliver();
                    return;
                }
                self.ivars().collect.lock().map(|mut c| c.remaining = count).ok();
                let me = self as *const Self as usize;
                for i in 0..count {
                    let result: *mut AnyObject = unsafe { msg_send![results, objectAtIndex: i] };
                    let provider: *mut AnyObject = unsafe { msg_send![result, itemProvider] };
                    let name = unsafe {
                        let n: *mut AnyObject = msg_send![provider, suggestedName];
                        if n.is_null() {
                            format!("photo-{i}")
                        } else {
                            let n: &objc2_foundation::NSString = &*n.cast();
                            n.to_string()
                        }
                    };
                    let collect = Arc::clone(&self.ivars().collect);
                    let block = block2::RcBlock::new(move |data: *mut AnyObject, _error: *mut AnyObject| {
                        let bytes = if data.is_null() {
                            None
                        } else {
                            let data: &objc2_foundation::NSData = unsafe { &*data.cast() };
                            Some(data.bytes().to_vec())
                        };
                        let last = collect.lock().map(|mut c| {
                            if let Some(bytes) = bytes {
                                c.photos.push(PickedPhoto { name: name.clone(), bytes });
                            }
                            c.remaining = c.remaining.saturating_sub(1);
                            c.remaining == 0
                        });
                        if last.unwrap_or(false) {
                            // Back to the main thread, where `done` may run.
                            let target = me as *mut AnyObject;
                            unsafe {
                                let nil: *mut AnyObject = std::ptr::null_mut();
                                let () = msg_send![
                                    target,
                                    performSelectorOnMainThread: sel!(deliverPicked),
                                    withObject: nil,
                                    waitUntilDone: Bool::NO
                                ];
                            }
                        }
                    });
                    let uti = ns_string("public.image");
                    unsafe {
                        let _progress: *mut AnyObject = msg_send![
                            provider,
                            loadDataRepresentationForTypeIdentifier: &*uti,
                            completionHandler: &*block
                        ];
                    }
                }
            }

            #[method(deliverPicked)]
            fn deliver_picked(&self) {
                self.deliver();
            }
        }
    );

    impl PickerDelegate {
        fn new(done: PickDone) -> Retained<Self> {
            let mtm = objc2_foundation::MainThreadMarker::new()
                .expect("the photo picker is presented from the main thread");
            let this: Allocated<Self> = mtm.alloc();
            let this = this.set_ivars(PickerIvars {
                collect: Arc::new(Mutex::new(Collect::default())),
                done: RefCell::new(Some(done)),
            });
            unsafe { msg_send_id![super(this), init] }
        }

        /// Hand the photos to the caller, once, and let this delegate go.
        fn deliver(&self) {
            let photos = self
                .ivars()
                .collect
                .lock()
                .map(|mut c| std::mem::take(&mut c.photos))
                .unwrap_or_default();
            if let Some(done) = self.ivars().done.borrow_mut().take() {
                done(Ok(photos));
            }
            let me = self as *const Self;
            PENDING.with(|p| p.borrow_mut().retain(|d| !std::ptr::eq(&**d, me)));
        }
    }

    impl PhotoPicker for IosServices {
        fn pick_photos(
            &self,
            max: usize,
            done: Box<dyn FnOnce(Result<Vec<PickedPhoto>, ServiceError>)>,
        ) {
            let presented = (|| -> Result<Retained<PickerDelegate>, ServiceError> {
                let config_class = class("PHPickerConfiguration")?;
                let filter_class = class("PHPickerFilter")?;
                let picker_class = class("PHPickerViewController")?;
                let presenter = presenter()?;
                unsafe {
                    let config: *mut AnyObject = msg_send![config_class, alloc];
                    let config: *mut AnyObject = msg_send![config, init];
                    let limit = isize::try_from(max).unwrap_or(isize::MAX);
                    let () = msg_send![config, setSelectionLimit: limit];
                    let images: *mut AnyObject = msg_send![filter_class, imagesFilter];
                    let () = msg_send![config, setFilter: images];
                    let picker: *mut AnyObject = msg_send![picker_class, alloc];
                    let picker: *mut AnyObject = msg_send![picker, initWithConfiguration: config];
                    let () = msg_send![config, release];
                    if picker.is_null() {
                        return Err(ServiceError::failed("the photo picker did not initialise"));
                    }
                    let delegate = PickerDelegate::new(Box::new(|_| {}));
                    let () = msg_send![picker, setDelegate: &*delegate];
                    let nil: *mut AnyObject = std::ptr::null_mut();
                    let () = msg_send![presenter, presentViewController: picker, animated: Bool::YES, completion: nil];
                    let () = msg_send![picker, release];
                    Ok(delegate)
                }
            })();
            match presented {
                Ok(delegate) => {
                    *delegate.ivars().done.borrow_mut() = Some(done);
                    PENDING.with(|p| p.borrow_mut().push(delegate));
                }
                Err(error) => done(Err(error)),
            }
        }
    }
}

#[cfg(target_os = "ios")]
pub use ios::IosServices;

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_foundation::mobile::{HapticPattern, ShareItem};

    #[test]
    fn the_share_text_is_the_message_then_the_link() {
        let item = |text: &str, url: Option<&str>| ShareItem {
            text: text.to_owned(),
            url: url.map(str::to_owned),
            file: None,
        };
        assert_eq!(
            share_text(&item("Look", Some("https://x.dev"))),
            "Look\nhttps://x.dev"
        );
        assert_eq!(
            share_text(&item("", Some("https://x.dev"))),
            "https://x.dev"
        );
        assert_eq!(share_text(&item("Look", None)), "Look");
    }

    #[test]
    fn android_constants_degrade_before_api_30() {
        assert_eq!(android_haptic_constant(HapticPattern::Success), (16, 1));
        assert_eq!(android_haptic_constant(HapticPattern::Error), (17, 0));
        assert_eq!(android_haptic_constant(HapticPattern::Selection), (4, 4));
    }
}
