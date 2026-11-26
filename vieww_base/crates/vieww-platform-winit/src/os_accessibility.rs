//! The OS's accessibility preferences, read and pushed to the tree (FW-02).
//!
//! # What was missing
//!
//! `FrameDriver::set_accessibility` implemented reduce-motion settling, high
//! contrast and text scale, and nothing outside tests ever called it: the
//! platform layer did not read the settings, so no app honoured reduced motion
//! on any platform. This module is the missing half.
//!
//! # Shape
//!
//! Three layers, split so that everything except the raw OS call is testable
//! on any machine — a phone is needed to *get* the numbers, not to check what
//! is done with them (the same split `insets.rs` makes):
//!
//! 1. **Raw reads** — one small `cfg`-gated function per platform that asks
//!    the OS and returns plain values. The only untestable lines.
//! 2. **Adapters** — pure functions from those raw values to
//!    [`Accessibility`]: [`from_android`], [`from_ios`], [`from_macos`],
//!    [`from_windows`], [`from_web`], [`from_gnome`]. One test each.
//! 3. **[`AccessibilityWatch`]** — remembers the last answer, re-asks on a
//!    schedule and whenever the window regains focus (which is how a change
//!    made in the system Settings app arrives: the person leaves, flips the
//!    switch and comes back), and reports only a *change*, so the frame
//!    driver republishes only when something moved.
//!
//! # Overrides
//!
//! `VIEWW_REDUCE_MOTION`, `VIEWW_HIGH_CONTRAST` and `VIEWW_TEXT_SCALE` win
//! over the OS on every platform. They exist so a farm run or a screenshot
//! test can force a preference without touching a device's settings.

use std::time::{Duration, Instant};

use vieww_foundation::Accessibility;

/// The activity handle on Android, and nothing anywhere else, so [`read`] has
/// one signature on every platform.
#[cfg(target_os = "android")]
pub(crate) type AndroidHandle = winit::platform::android::activity::AndroidApp;
/// See the Android alias above.
#[cfg(not(target_os = "android"))]
pub(crate) type AndroidHandle = ();

// ---------------------------------------------------------------------------
// 2. Adapters — pure.
// ---------------------------------------------------------------------------

/// Android: `Settings.Global.ANIMATOR_DURATION_SCALE`, `Configuration.fontScale`
/// and `Settings.Secure.high_text_contrast_enabled`.
///
/// "Remove animations" in Settings ▸ Accessibility sets the duration scale to
/// exactly `0`, which is what reduce-motion means on Android. A fractional
/// scale (the developer option's `0.5x`) is a speed, not a request for less
/// movement, and is left alone.
#[must_use]
pub fn from_android(
    animator_duration_scale: f32,
    font_scale: f32,
    high_text_contrast: bool,
) -> Accessibility {
    Accessibility::new(
        animator_duration_scale <= 0.0,
        high_text_contrast,
        font_scale,
    )
}

/// iOS: `UIAccessibilityIsReduceMotionEnabled`,
/// `UIAccessibilityDarkerSystemColorsEnabled` and the application's
/// `preferredContentSizeCategory`.
#[must_use]
pub fn from_ios(reduce_motion: bool, darker_colors: bool, size_category: &str) -> Accessibility {
    Accessibility::new(reduce_motion, darker_colors, ios_text_scale(size_category))
}

/// Dynamic Type's category name as a multiplier against the default `Large`
/// (17 pt body). The ratios are body-style point sizes over 17.
///
/// An unknown category — a name added by a later iOS — is `1.0`, not a guess.
#[must_use]
pub fn ios_text_scale(category: &str) -> f32 {
    // The constants are `UICTContentSizeCategoryXS` and so on; accept the bare
    // suffix too, which is how they are logged.
    let name = category
        .strip_prefix("UICTContentSizeCategory")
        .unwrap_or(category);
    match name {
        "XS" => 0.82,
        "S" => 0.88,
        "M" => 0.94,
        "L" => 1.0,
        "XL" => 1.12,
        "XXL" => 1.24,
        "XXXL" => 1.35,
        "AccessibilityM" => 1.65,
        "AccessibilityL" => 1.94,
        "AccessibilityXL" => 2.35,
        "AccessibilityXXL" => 2.76,
        "AccessibilityXXXL" => 3.12,
        _ => 1.0,
    }
}

/// macOS: `NSWorkspace.accessibilityDisplayShouldReduceMotion` and
/// `…ShouldIncreaseContrast`. macOS has no system-wide text scale.
#[must_use]
pub fn from_macos(reduce_motion: bool, increase_contrast: bool) -> Accessibility {
    Accessibility::new(reduce_motion, increase_contrast, 1.0)
}

/// Windows: `SPI_GETCLIENTAREAANIMATION` (true means animations are *on*),
/// the `HCF_HIGHCONTRASTON` flag of `SPI_GETHIGHCONTRAST`, and the Settings ▸
/// Accessibility ▸ Text size percentage (`TextScaleFactor`, 100–225).
#[must_use]
pub fn from_windows(
    client_area_animation: bool,
    high_contrast_on: bool,
    text_scale_percent: u32,
) -> Accessibility {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a percentage between 100 and 225"
    )]
    let scale = text_scale_percent as f32 / 100.0;
    Accessibility::new(!client_area_animation, high_contrast_on, scale)
}

/// Web: `prefers-reduced-motion: reduce` and `prefers-contrast: more`. A page
/// cannot read the browser's text size as a multiplier — it zooms the layout
/// instead — so the scale stays `1.0`.
#[must_use]
pub fn from_web(prefers_reduced_motion: bool, prefers_more_contrast: bool) -> Accessibility {
    Accessibility::from_media_queries(prefers_reduced_motion, prefers_more_contrast)
}

/// GNOME / GTK desktops (`gsettings`): `enable-animations`, the a11y
/// `high-contrast` switch and `text-scaling-factor`.
#[must_use]
pub fn from_gnome(
    enable_animations: bool,
    high_contrast: bool,
    text_scaling_factor: f32,
) -> Accessibility {
    Accessibility::new(!enable_animations, high_contrast, text_scaling_factor)
}

/// Apply the `VIEWW_*` overrides to what the OS said.
#[must_use]
pub fn with_overrides(
    mut base: Accessibility,
    reduce_motion: Option<&str>,
    high_contrast: Option<&str>,
    text_scale: Option<&str>,
) -> Accessibility {
    let flag = |value: &str| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        )
    };
    if let Some(v) = reduce_motion.filter(|v| !v.trim().is_empty()) {
        base.reduce_motion = flag(v);
    }
    if let Some(v) = high_contrast.filter(|v| !v.trim().is_empty()) {
        base.high_contrast = flag(v);
    }
    if let Some(v) = text_scale.and_then(|v| v.trim().parse::<f32>().ok()) {
        base = Accessibility::new(base.reduce_motion, base.high_contrast, v);
    }
    base
}

// ---------------------------------------------------------------------------
// 1. Raw reads — one per platform, `cfg`-gated.
// ---------------------------------------------------------------------------

/// What this platform's OS currently says, before overrides.
///
/// `android` is only used on Android; every other build ignores it, which
/// keeps one signature for the caller.
#[must_use]
pub(crate) fn read_os(android: Option<&AndroidHandle>) -> Accessibility {
    #[cfg(target_os = "android")]
    {
        return android.and_then(read_android).unwrap_or_default();
    }
    #[cfg(target_os = "ios")]
    {
        let _ = android;
        return read_ios();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = android;
        return read_macos();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = android;
        return read_windows();
    }
    #[cfg(all(
        unix,
        not(any(target_os = "android", target_os = "ios", target_os = "macos"))
    ))]
    {
        let _ = android;
        return read_gnome();
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = android;
        Accessibility::default()
    }
}

/// The OS answer with the environment overrides applied.
#[must_use]
pub(crate) fn read(android: Option<&AndroidHandle>) -> Accessibility {
    with_overrides(
        read_os(android),
        std::env::var("VIEWW_REDUCE_MOTION").ok().as_deref(),
        std::env::var("VIEWW_HIGH_CONTRAST").ok().as_deref(),
        std::env::var("VIEWW_TEXT_SCALE").ok().as_deref(),
    )
}

#[cfg(target_os = "android")]
fn read_android(app: &AndroidHandle) -> Option<Accessibility> {
    use jni::objects::{JObject, JValue};
    use jni::{jni_sig, jni_str};

    // Both pointers belong to the running activity and neither is kept past
    // this call — the contract `insets.rs` and `clipboard.rs` rely on.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let activity_ptr = app.activity_as_ptr();
    let result = vm.attach_current_thread(|env| -> Result<(f32, f32, bool), jni::errors::Error> {
        let activity = unsafe { JObject::from_raw(env, activity_ptr.cast()) };
        let resolver = env
            .call_method(
                &activity,
                jni_str!("getContentResolver"),
                jni_sig!("()Landroid/content/ContentResolver;"),
                &[],
            )?
            .l()?;

        // The three-argument getters take a default and do not throw
        // `SettingNotFoundException`, which the two-argument ones do.
        let duration_key = env.new_string("animator_duration_scale")?;
        let duration = env
            .call_static_method(
                jni_str!("android/provider/Settings$Global"),
                jni_str!("getFloat"),
                jni_sig!("(Landroid/content/ContentResolver;Ljava/lang/String;F)F"),
                &[
                    JValue::Object(&resolver),
                    JValue::Object(&duration_key),
                    JValue::Float(1.0),
                ],
            )?
            .f()?;

        let contrast_key = env.new_string("high_text_contrast_enabled")?;
        let contrast = env
            .call_static_method(
                jni_str!("android/provider/Settings$Secure"),
                jni_str!("getInt"),
                jni_sig!("(Landroid/content/ContentResolver;Ljava/lang/String;I)I"),
                &[
                    JValue::Object(&resolver),
                    JValue::Object(&contrast_key),
                    JValue::Int(0),
                ],
            )?
            .i()?;

        let resources = env
            .call_method(
                &activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let configuration = env
            .call_method(
                &resources,
                jni_str!("getConfiguration"),
                jni_sig!("()Landroid/content/res/Configuration;"),
                &[],
            )?
            .l()?;
        let font_scale = env
            .get_field(&configuration, jni_str!("fontScale"), jni_sig!("F"))?
            .f()?;
        Ok((duration, font_scale, contrast != 0))
    });
    // A failed read is "unknown", not "nothing requested": keep the last
    // answer rather than switching motion back on under someone who turned it
    // off.
    result.ok().map(|(d, f, c)| from_android(d, f, c))
}

#[cfg(target_os = "ios")]
fn read_ios() -> Accessibility {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    extern "C" {
        fn UIAccessibilityIsReduceMotionEnabled() -> Bool;
        fn UIAccessibilityDarkerSystemColorsEnabled() -> Bool;
    }
    // SAFETY: both are plain UIKit C functions with no arguments, callable
    // from the main thread, which is the thread the frame loop runs on.
    let (reduce, darker) = unsafe {
        (
            UIAccessibilityIsReduceMotionEnabled().as_bool(),
            UIAccessibilityDarkerSystemColorsEnabled().as_bool(),
        )
    };
    let category = (|| -> Option<String> {
        let class = AnyClass::get("UIApplication")?;
        // SAFETY: `sharedApplication` and `preferredContentSizeCategory` are
        // zero-argument class/instance getters returning objects (or nil).
        unsafe {
            let app: *mut AnyObject = msg_send![class, sharedApplication];
            if app.is_null() {
                return None;
            }
            let value: *mut AnyObject = msg_send![app, preferredContentSizeCategory];
            if value.is_null() {
                return None;
            }
            let utf8: *const std::ffi::c_char = msg_send![value, UTF8String];
            if utf8.is_null() {
                return None;
            }
            Some(
                std::ffi::CStr::from_ptr(utf8)
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    })();
    from_ios(reduce, darker, category.as_deref().unwrap_or("L"))
}

#[cfg(target_os = "macos")]
fn read_macos() -> Accessibility {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    let Some(class) = AnyClass::get("NSWorkspace") else {
        return Accessibility::default();
    };
    // SAFETY: `sharedWorkspace` and the two `accessibilityDisplay…` getters
    // are zero-argument and return an object / BOOL; AppKit is loaded because
    // winit created a window.
    unsafe {
        let workspace: *mut AnyObject = msg_send![class, sharedWorkspace];
        if workspace.is_null() {
            return Accessibility::default();
        }
        let reduce: Bool = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
        let contrast: Bool = msg_send![workspace, accessibilityDisplayShouldIncreaseContrast];
        from_macos(reduce.as_bool(), contrast.as_bool())
    }
}

#[cfg(target_os = "windows")]
fn read_windows() -> Accessibility {
    use std::ffi::c_void;

    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    const SPI_GETHIGHCONTRAST: u32 = 0x0042;
    const HCF_HIGHCONTRASTON: u32 = 0x0000_0001;

    #[repr(C)]
    struct HighContrastW {
        cb_size: u32,
        flags: u32,
        default_scheme: *mut u16,
    }

    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(
            action: u32,
            param: u32,
            pv_param: *mut c_void,
            win_ini: u32,
        ) -> i32;
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: isize,
            sub_key: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };

    let mut animations: i32 = 1;
    // SAFETY: the out-pointers are valid for the sizes the actions document
    // (a BOOL, and a HIGHCONTRASTW whose cbSize is set).
    let animations_known = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            std::ptr::addr_of_mut!(animations).cast(),
            0,
        ) != 0
    };

    let mut contrast = HighContrastW {
        cb_size: std::mem::size_of::<HighContrastW>() as u32,
        flags: 0,
        default_scheme: std::ptr::null_mut(),
    };
    // SAFETY: as above.
    let contrast_known = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cb_size,
            std::ptr::addr_of_mut!(contrast).cast(),
            0,
        ) != 0
    };

    let mut percent: u32 = 100;
    let mut size: u32 = 4;
    let key = wide("Software\\Microsoft\\Accessibility");
    let name = wide("TextScaleFactor");
    // SAFETY: `percent` is a 4-byte DWORD buffer and `size` says so.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(percent).cast(),
            &mut size,
        )
    };
    if status != 0 {
        percent = 100;
    }
    from_windows(
        !animations_known || animations != 0,
        contrast_known && contrast.flags & HCF_HIGHCONTRASTON != 0,
        percent,
    )
}

#[cfg(all(
    unix,
    not(any(target_os = "android", target_os = "ios", target_os = "macos"))
))]
fn read_gnome() -> Accessibility {
    fn get(schema: &str, key: &str) -> Option<String> {
        let out = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }
    // No `gsettings` (a minimal container, a non-GNOME session) is "nothing
    // requested", which is also what `Accessibility::default` says.
    let Some(animations) = get("org.gnome.desktop.interface", "enable-animations") else {
        return Accessibility::default();
    };
    let contrast =
        get("org.gnome.desktop.a11y.interface", "high-contrast").is_some_and(|v| v == "true");
    let scale = get("org.gnome.desktop.interface", "text-scaling-factor")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0);
    from_gnome(animations != "false", contrast, scale)
}

// ---------------------------------------------------------------------------
// 3. The watch.
// ---------------------------------------------------------------------------

/// Decides when to ask the OS again, and reports only changes.
///
/// A window that is asleep draws no frames, so the watch cannot rely on a timer
/// alone: the platform layer calls [`mark_dirty`](Self::mark_dirty) when the
/// window regains focus or the app resumes, which is when a change made in
/// system Settings becomes visible, and requests a frame so [`poll`](Self::poll)
/// runs.
pub struct AccessibilityWatch {
    read: Box<dyn FnMut() -> Accessibility>,
    last: Option<Accessibility>,
    interval: Option<Duration>,
    next: Option<Instant>,
    dirty: bool,
}

impl AccessibilityWatch {
    /// A watch over `read`. `interval` is how often to re-ask on frames that
    /// happen anyway; `None` re-asks only when marked dirty (for a read that
    /// spawns a process).
    pub fn new(read: impl FnMut() -> Accessibility + 'static, interval: Option<Duration>) -> Self {
        Self {
            read: Box::new(read),
            last: None,
            interval,
            next: None,
            dirty: true,
        }
    }

    /// The default cadence for this platform: two seconds where the read is a
    /// cheap in-process call, never where it spawns `gsettings`.
    #[must_use]
    pub const fn default_interval() -> Option<Duration> {
        if cfg!(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "windows"
        )) {
            Some(Duration::from_secs(2))
        } else {
            None
        }
    }

    /// Ask again on the next [`poll`](Self::poll).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// The last value this watch saw.
    #[must_use]
    pub fn last(&self) -> Option<Accessibility> {
        self.last
    }

    /// Ask if due. `Some(new)` only when the answer differs from the last one
    /// (the first answer always counts, so the tree is told at start-up).
    pub fn poll(&mut self, now: Instant) -> Option<Accessibility> {
        let due = self.dirty || self.next.is_some_and(|at| now >= at);
        if !due {
            return None;
        }
        self.dirty = false;
        self.next = self.interval.map(|every| now + every);
        let value = (self.read)();
        if self.last == Some(value) {
            return None;
        }
        self.last = Some(value);
        Some(value)
    }
}

impl std::fmt::Debug for AccessibilityWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessibilityWatch")
            .field("last", &self.last)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use vieww_render::FrameDriver;

    #[test]
    fn android_remove_animations_is_reduce_motion_and_a_slow_scale_is_not() {
        assert!(
            from_android(0.0, 1.0, false).reduce_motion,
            "Remove animations"
        );
        assert!(!from_android(1.0, 1.0, false).reduce_motion);
        assert!(
            !from_android(0.5, 1.0, false).reduce_motion,
            "0.5x is a speed"
        );
        let big = from_android(1.0, 1.3, true);
        assert_eq!(big.text_scale, 1.3);
        assert!(big.high_contrast);
    }

    #[test]
    fn ios_dynamic_type_maps_through_the_category_names() {
        assert_eq!(ios_text_scale("UICTContentSizeCategoryL"), 1.0);
        assert_eq!(ios_text_scale("UICTContentSizeCategoryXXXL"), 1.35);
        assert_eq!(
            ios_text_scale("UICTContentSizeCategoryAccessibilityXXXL"),
            3.12
        );
        assert_eq!(
            ios_text_scale("SomethingFromIOS30"),
            1.0,
            "unknown is not a guess"
        );
        let a = from_ios(true, true, "UICTContentSizeCategoryAccessibilityXXXL");
        assert!(a.reduce_motion && a.high_contrast);
        assert_eq!(a.text_scale, 3.0, "clamped to the framework's maximum");
    }

    #[test]
    fn macos_reports_motion_and_contrast_and_never_a_text_scale() {
        let a = from_macos(true, false);
        assert!(a.reduce_motion && !a.high_contrast);
        assert_eq!(a.text_scale, 1.0);
    }

    #[test]
    fn windows_animation_off_is_reduce_motion_and_the_percentage_is_a_scale() {
        assert!(
            from_windows(false, false, 100).reduce_motion,
            "animations off"
        );
        assert!(!from_windows(true, false, 100).reduce_motion);
        let a = from_windows(true, true, 150);
        assert!(a.high_contrast);
        assert_eq!(a.text_scale, 1.5);
        assert_eq!(from_windows(true, false, 400).text_scale, 3.0, "clamped");
    }

    #[test]
    fn web_media_queries_map_one_to_one() {
        assert_eq!(from_web(false, false), Accessibility::default());
        assert!(from_web(true, false).reduce_motion);
        assert!(from_web(false, true).high_contrast);
    }

    #[test]
    fn gnome_disabled_animations_is_reduce_motion() {
        assert!(from_gnome(false, false, 1.0).reduce_motion);
        assert_eq!(from_gnome(true, false, 1.25).text_scale, 1.25);
    }

    #[test]
    fn a_nan_text_scale_from_a_misreporting_platform_is_sanitised() {
        assert_eq!(from_android(1.0, f32::NAN, false).text_scale, 1.0);
    }

    #[test]
    fn overrides_win_over_the_os_in_both_directions() {
        let os = from_gnome(true, false, 1.0);
        let forced = with_overrides(os, Some("1"), Some("true"), Some("1.5"));
        assert!(forced.reduce_motion && forced.high_contrast);
        assert_eq!(forced.text_scale, 1.5);
        let off = with_overrides(from_gnome(false, true, 2.0), Some("0"), Some("off"), None);
        assert!(!off.reduce_motion && !off.high_contrast);
        assert_eq!(off.text_scale, 2.0, "an unset override leaves the OS value");
        let junk = with_overrides(os, Some(""), None, Some("wide"));
        assert_eq!(junk, os, "empty and unparseable overrides are ignored");
    }

    #[test]
    fn the_first_answer_is_reported_and_an_unchanged_one_is_not() {
        let mut watch = AccessibilityWatch::new(Accessibility::default, None);
        let t0 = Instant::now();
        assert_eq!(
            watch.poll(t0),
            Some(Accessibility::default()),
            "start-up is told"
        );
        watch.mark_dirty();
        assert_eq!(
            watch.poll(t0),
            None,
            "asked again, same answer: no republish"
        );
    }

    #[test]
    fn a_watch_with_no_interval_only_asks_when_marked_dirty() {
        let asks = Rc::new(Cell::new(0));
        let counted = Rc::clone(&asks);
        let mut watch = AccessibilityWatch::new(
            move || {
                counted.set(counted.get() + 1);
                Accessibility::default()
            },
            None,
        );
        let t0 = Instant::now();
        watch.poll(t0);
        watch.poll(t0 + Duration::from_secs(60));
        assert_eq!(asks.get(), 1, "no timer, so no spawn per frame");
        watch.mark_dirty();
        watch.poll(t0 + Duration::from_secs(61));
        assert_eq!(asks.get(), 2);
    }

    #[test]
    fn a_timed_watch_asks_again_after_its_interval() {
        let asks = Rc::new(Cell::new(0));
        let counted = Rc::clone(&asks);
        let mut watch = AccessibilityWatch::new(
            move || {
                counted.set(counted.get() + 1);
                Accessibility::default()
            },
            Some(Duration::from_secs(2)),
        );
        let t0 = Instant::now();
        watch.poll(t0);
        watch.poll(t0 + Duration::from_secs(1));
        assert_eq!(asks.get(), 1, "not yet due");
        watch.poll(t0 + Duration::from_secs(3));
        assert_eq!(asks.get(), 2);
    }

    /// FW-02's done criterion: toggling the OS setting changes
    /// `driver.accessibility()` with no restart.
    #[test]
    fn toggling_the_os_setting_reaches_the_driver_without_a_restart() {
        let setting = Rc::new(Cell::new(false));
        let reading = Rc::clone(&setting);
        let mut watch = AccessibilityWatch::new(
            move || from_android(if reading.get() { 0.0 } else { 1.0 }, 1.0, false),
            None,
        );
        let mut driver = FrameDriver::new(vieww_foundation::Size::new(200.0, 200.0));
        let t0 = Instant::now();

        if let Some(a) = watch.poll(t0) {
            driver.set_accessibility(a);
        }
        assert!(!driver.accessibility().reduce_motion);

        // The person turns "Remove animations" on and comes back to the app.
        setting.set(true);
        watch.mark_dirty();
        if let Some(a) = watch.poll(t0) {
            driver.set_accessibility(a);
        }
        assert!(driver.accessibility().reduce_motion, "applied live");

        setting.set(false);
        watch.mark_dirty();
        if let Some(a) = watch.poll(t0) {
            driver.set_accessibility(a);
        }
        assert!(!driver.accessibility().reduce_motion, "and back off again");
    }
}
