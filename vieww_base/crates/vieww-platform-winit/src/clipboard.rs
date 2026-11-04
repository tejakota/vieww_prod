//! The system pasteboard, one implementation per target.
//!
//! Registered as [`Clipboard`] in [`services::platform`](crate::services), so a
//! `TextField` reaches it through the same
//! `SharedServices` that carries storage and deep links. Nothing here is named by
//! a widget.
//!
//! # Text only
//!
//! Every platform pasteboard carries images, files and application-defined types
//! as well. None of that is here, because [`Clipboard`] has no representation for
//! it — see the trait. A pasteboard holding an image reads as empty.
//!
//! # What each target uses
//!
//! | target | mechanism |
//! |---|---|
//! | desktop on X11/Windows/macOS | `arboard`, which owns the X11/Win32/AppKit differences |
//! | desktop on a Wayland session | `wl-copy`/`wl-paste` (the `wl-clipboard` tools) when present, because `arboard` without the `wayland-data-control` feature owns no native Wayland clipboard — B14's broken case; `arboard` remains the fallback through XWayland |
//! | Android | `ClipboardManager` through JNI |
//! | iOS | `UIPasteboard.general` through `objc2` |
//!
//! # Why the Wayland half shells out rather than dlopening
//!
//! The obvious "no subprocess" answer would be `dlopen("libwayland-client")`
//! and speaking the `wl_data_device_manager` protocol by hand — the convention
//! `vieww-audio`'s backend loading and `softbuffer`'s `wayland-dlopen` feature
//! both follow. It is not taken, because a Wayland *protocol* is not a C ABI:
//! `wl_data_offer`/`wl_data_source`/`wl_data_device` is a marshalled object
//! protocol with its own dispatch loop and its own lifetime rules, and a
//! hand-rolled client of it is exactly the "own everything" trap DESIGN §9
//! names — with the added spice that getting it wrong silently corrupts
//! *other applications'* pastes. `wl-clipboard` is the maintained, tested
//! client of that protocol; exec is the honest seam to it. Clipboard *ownership*
//! is why the copy side cannot be a one-shot read either: `wl-copy` forks a
//! child that keeps serving the selection after the call returns, which is the
//! same lifetime answer arboard's held connection gives on X11.

use std::fmt;

use vieww_foundation::{Clipboard, ServiceError};

/// The pasteboard this platform provides.
pub struct PlatformClipboard {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    inner: std::cell::RefCell<Option<arboard::Clipboard>>,
    #[cfg(target_os = "android")]
    app: crate::AndroidApp,
}

/// Hand-written because `arboard::Clipboard` is not `Debug`, and the workspace
/// warns on a type that has no `Debug` at all.
///
/// Reports whether a connection is currently open rather than anything about the
/// contents. Printing what is on the pasteboard would put the user's passwords in
/// a log line the first time somebody debug-formatted a service registry.
impl fmt::Debug for PlatformClipboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = f.debug_struct("PlatformClipboard");
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        out.field("connected", &self.inner.borrow().is_some());
        out.finish()
    }
}

// ---------------------------------------------------------------------- desktop

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl PlatformClipboard {
    /// Connect to the desktop pasteboard.
    ///
    /// The connection is made lazily and kept, rather than opened per operation.
    /// On X11 that is not an optimisation: the clipboard is owned by a live
    /// client, and a handle dropped after each copy takes the contents with it —
    /// pasting into another application then yields nothing, which looks exactly
    /// like a failed copy.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: std::cell::RefCell::new(None),
        }
    }

    /// The live handle, opening one on first use.
    fn with<T>(
        &self,
        act: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
    ) -> Result<T, ServiceError> {
        let mut slot = self.inner.borrow_mut();
        if slot.is_none() {
            *slot = Some(
                arboard::Clipboard::new()
                    .map_err(|error| ServiceError::failed(error.to_string()))?,
            );
        }
        let clipboard = slot.as_mut().expect("opened above");
        act(clipboard).map_err(|error| match error {
            // A pasteboard holding an image, or nothing at all. Not a failure.
            arboard::Error::ContentNotAvailable => ServiceError::failed("no text"),
            other => ServiceError::failed(other.to_string()),
        })
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Default for PlatformClipboard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Clipboard for PlatformClipboard {
    fn read_text(&self) -> Result<Option<String>, ServiceError> {
        // A Wayland session's native clipboard first, when the tools that
        // reach it are installed — see `wayland_cli`'s docs for why this is
        // exec rather than dlopen. A failed read there falls through to
        // arboard (which may still connect through XWayland) rather than
        // becoming a hard error: a paste that finds nothing and a paste that
        // could not ask are one answer to a text field, the contract the
        // arboard half below already keeps.
        if wayland_cli::prefer_cli() {
            if let Ok(text) = wayland_cli::read_text() {
                return Ok(text);
            }
        }
        // Absent and unreadable are one answer to a field, so a failed read is
        // `None` rather than an error. The distinction the trait keeps is
        // `Denied`, which no desktop reports.
        Ok(self.with(arboard::Clipboard::get_text).ok())
    }

    fn write_text(&self, text: &str) -> Result<(), ServiceError> {
        // Copy is the direction where the Wayland split actually bites: a copy
        // through arboard-on-XWayland is invisible to native Wayland
        // applications, which reads to a user as "the copy did not work". So
        // the native route is *preferred* here, not merely tried — and when
        // it fails, the arboard failure that follows carries the wl-clipboard
        // hint, because on a Wayland-only session that fallback cannot work
        // either and the message should say what fixes it.
        let on_wayland =
            wayland_cli::session_is_wayland(std::env::var("WAYLAND_DISPLAY").ok().as_deref());
        if on_wayland && wayland_cli::tools_on_path() && wayland_cli::write_text(text).is_ok() {
            return Ok(());
        }
        // Reaching here on a Wayland session means one of two honest things:
        // the tools are not installed (arboard through XWayland is the
        // remaining answer), or they exist and the compositor refused them
        // (same fallback, and the hint below says what the native route was).
        self.with(|clipboard| clipboard.set_text(text.to_string()))
            .map_err(|error| wayland_cli::hinted(error, on_wayland))
    }
}

// ------------------------------------------------ the Wayland CLI clipboard

/// The native Wayland clipboard, reached through the `wl-clipboard` tools.
///
/// Every decision in here is split from every side effect, the split
/// `crate::package`'s own tool detection practices: whether a session is
/// Wayland and whether the tools exist are pure functions of an env value and
/// a directory list, so a test answers them with a fake `WAYLAND_DISPLAY` and
/// a directory of fake tools rather than by mutating the process's real
/// environment (which two racing tests must never do).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod wayland_cli {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use vieww_foundation::ServiceError;

    /// The copy half of `wl-clipboard`.
    const COPY: &str = "wl-copy";
    /// The paste half.
    const PASTE: &str = "wl-paste";

    /// Whether `WAYLAND_DISPLAY` names a live compositor connection — the
    /// one variable a Wayland session reliably sets and an X11 session
    /// reliably does not. `XDG_SESSION_TYPE` is *not* trusted for this: it
    /// says what the user logged into, not what display servers this process
    /// can actually reach, and a test harness or an SSH forward can make the
    /// two disagree in either direction.
    pub(super) fn session_is_wayland(wayland_display: Option<&str>) -> bool {
        wayland_display.is_some_and(|value| !value.trim().is_empty())
    }

    /// Whether both halves of `wl-clipboard` are in these directories — the
    /// `PATH` walk `tool_on_path` does elsewhere in the workspace, kept local
    /// because this crate has no shared util for it and a copy would be the
    /// third. A plain `is_file`, deliberately without an executable-bit
    /// check, for the same reason the workspace's other lookups skip it: a
    /// `PATH` entry that is not executable is a broken installation the
    /// *exec* below will report by name, not something to hide here.
    ///
    /// `pub(crate)` so this file's own tests can hand it a directory of fake
    /// tools — the same visibility dodge `tray_pace` uses for the same reason.
    pub(crate) fn tools_in_dirs(dirs: &[PathBuf]) -> bool {
        dirs.iter().any(|dir| dir.join(COPY).is_file())
            && dirs.iter().any(|dir| dir.join(PASTE).is_file())
    }

    /// The process's real `PATH`, split. Reading the environment is a side
    /// effect, so this is the only function in the module with one — tests
    /// call [`tools_in_dirs`] with their own list instead.
    pub(super) fn tools_on_path() -> bool {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        tools_in_dirs(&dirs)
    }

    /// Whether a pasteboard operation should go through `wl-clipboard`
    /// before arboard gets a say: a Wayland session with both tools present.
    /// The halves of the decision are pure and separately tested; this is
    /// their conjunction, read off the real environment.
    pub(super) fn prefer_cli() -> bool {
        session_is_wayland(std::env::var("WAYLAND_DISPLAY").ok().as_deref()) && tools_on_path()
    }

    /// The argv `wl-paste` is run with. `--no-newline` because `wl-paste`
    /// appends a newline to its stdout that the clipboard never contained —
    /// without the flag every pasted line grows one.
    pub(crate) fn paste_args() -> &'static [&'static str] {
        &["--no-newline"]
    }

    /// The argv `wl-copy` is run with: none. In particular **not** `-o`
    /// (`--paste-once`): the default forks a child that keeps serving the
    /// selection for as long as anything might paste it, which is the
    /// ownership semantics the clipboard has on every other platform — a
    /// copy that evaporates after one paste elsewhere is a copy that silently
    /// did not happen.
    pub(crate) fn copy_args() -> &'static [&'static str] {
        &[]
    }

    /// Put `text` on the native Wayland clipboard.
    ///
    /// `wl-copy` reads stdin to EOF, then forks its serving child; the parent
    /// exits promptly, so `wait` here is not a hang. The text is written
    /// before the handle is dropped so EOF is what ends the read, and stderr
    /// is captured so a refusal from the compositor arrives as this call's
    /// own message rather than a stray line on the console.
    ///
    /// # Untested on this sandbox
    /// No Wayland compositor runs here, so the round trip needs the Weston
    /// session the monitor tests already name as their own pending hardware
    /// run — the argv, the routing and the failure hint are what this machine
    /// can prove.
    pub(super) fn write_text(text: &str) -> Result<(), ServiceError> {
        let mut child = Command::new(COPY)
            .args(copy_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| ServiceError::failed(format!("{COPY}: {error}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            // Best effort, and deliberately not fatal: `wl-copy` treats a
            // closed stdin as an empty selection rather than an error, and a
            // write failing here means the child is already gone — which the
            // `wait_with_output` below is about to report properly.
            let _ = stdin.write_all(text.as_bytes());
        }
        let output = child
            .wait_with_output()
            .map_err(|error| ServiceError::failed(format!("{COPY}: {error}")))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(ServiceError::failed(format!(
                "{COPY} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    /// Read the native Wayland clipboard. `Ok(None)` is the empty clipboard,
    /// not a failure — the same answer the arboard half gives for one.
    pub(super) fn read_text() -> Result<Option<String>, ServiceError> {
        let output = Command::new(PASTE)
            .args(paste_args())
            .stdin(Stdio::null())
            .output()
            .map_err(|error| ServiceError::failed(format!("{PASTE}: {error}")))?;
        if !output.status.success() {
            return Err(ServiceError::failed(format!(
                "{PASTE} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(text))
        }
    }

    /// The install command a Wayland-session failure should name. One
    /// string, not a formatted one, so the test asserts on the exact words a
    /// user will paste into a shell.
    pub(super) const INSTALL_HINT: &str = "install wl-clipboard (apt install wl-clipboard / \
         dnf install wl-clipboard / pacman -S wl-clipboard) — wl-copy/wl-paste are this \
         session's native clipboard";

    /// An arboard failure, with the Wayland route named when the session is
    /// one — because on a Wayland-*only* session the fallback through
    /// XWayland cannot work either, and an error that stops at "arboard
    /// failed" sends a person debugging a library that was never the answer.
    pub(super) fn hinted(error: ServiceError, on_wayland: bool) -> ServiceError {
        if !on_wayland {
            return error;
        }
        let message = match error {
            ServiceError::Failed(message) => message,
            other => return other,
        };
        ServiceError::failed(format!(
            "{message}; on this Wayland session: {INSTALL_HINT}"
        ))
    }
}

// ---------------------------------------------------------------------- Android

#[cfg(target_os = "android")]
impl PlatformClipboard {
    /// The activity's `ClipboardManager`.
    #[must_use]
    pub fn new(app: &crate::AndroidApp) -> Self {
        Self { app: app.clone() }
    }

    /// Run `act` against the activity's `ClipboardManager`.
    ///
    /// The manager is fetched per call rather than held: it belongs to the
    /// activity, and this object outlives any particular one.
    fn with<T>(
        &self,
        act: impl FnOnce(
            &mut jni::Env<'_>,
            &jni::objects::JObject<'_>,
            &jni::objects::JObject<'_>,
        ) -> Result<T, jni::errors::Error>,
    ) -> Result<T, ServiceError> {
        use jni::objects::{JObject, JValue};
        use jni::{jni_sig, jni_str};

        // Both pointers belong to the running activity and neither is kept past
        // this call — the same contract as `insets::window_insets`.
        let vm = unsafe { jni::JavaVM::from_raw(self.app.vm_as_ptr().cast()) };
        let activity_ptr = self.app.activity_as_ptr();

        vm.attach_current_thread(|env| {
            let activity = unsafe { JObject::from_raw(env, activity_ptr.cast()) };
            let name = env.new_string("clipboard")?;
            let manager = env
                .call_method(
                    &activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&name)],
                )?
                .l()?;
            act(env, &activity, &manager)
        })
        .map_err(|error| ServiceError::failed(error.to_string()))
    }
}

#[cfg(target_os = "android")]
impl Clipboard for PlatformClipboard {
    /// # Android 10 and the focus rule
    ///
    /// From API 29 an application may only read the pasteboard while it holds
    /// input focus; a background read returns empty rather than failing. That is
    /// a platform policy and not something this code can work around — and it is
    /// the right policy, since the alternative is every app reading everything
    /// the user has ever copied. A paste driven by a keystroke is focused by
    /// definition, so it is unaffected.
    fn read_text(&self) -> Result<Option<String>, ServiceError> {
        use jni::jni_sig;
        use jni::jni_str;

        self.with(|env, _activity, manager| {
            let clip = env
                .call_method(
                    manager,
                    jni_str!("getPrimaryClip"),
                    jni_sig!("()Landroid/content/ClipData;"),
                    &[],
                )?
                .l()?;
            if clip.is_null() {
                return Ok(None);
            }
            let count = env
                .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
                .i()?;
            if count <= 0 {
                return Ok(None);
            }
            let item = env
                .call_method(
                    &clip,
                    jni_str!("getItemAt"),
                    jni_sig!("(I)Landroid/content/ClipData$Item;"),
                    &[jni::objects::JValue::Int(0)],
                )?
                .l()?;
            // `getText` rather than `coerceToText`: the latter needs a Context
            // and will happily render a URI or an Intent as a string, which is
            // not text the user copied.
            let text = env
                .call_method(
                    &item,
                    jni_str!("getText"),
                    jni_sig!("()Ljava/lang/CharSequence;"),
                    &[],
                )?
                .l()?;
            if text.is_null() {
                return Ok(None);
            }
            let string = env
                .call_method(
                    &text,
                    jni_str!("toString"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            // Cast then read, as two fallible steps — the same shape
            // `deep_links.rs` uses and for the same reason: the cast is a real
            // `IsInstanceOf` against a value the platform handed over, and
            // reading the characters can fail on its own.
            let string = env.cast_local::<jni::objects::JString>(string)?;
            Ok(Some(string.try_to_string(env)?))
        })
    }

    fn write_text(&self, text: &str) -> Result<(), ServiceError> {
        use jni::objects::JValue;
        use jni::{jni_sig, jni_str};

        self.with(|env, _activity, manager| {
            let label = env.new_string("vieww")?;
            let value = env.new_string(text)?;
            let clip = env
                .call_static_method(
                    jni_str!("android/content/ClipData"),
                    jni_str!("newPlainText"),
                    jni_sig!(
                        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)\
                         Landroid/content/ClipData;"
                    ),
                    &[JValue::Object(&label), JValue::Object(&value)],
                )?
                .l()?;
            env.call_method(
                manager,
                jni_str!("setPrimaryClip"),
                jni_sig!("(Landroid/content/ClipData;)V"),
                &[JValue::Object(&clip)],
            )?;
            Ok(())
        })
    }
}

// -------------------------------------------------------------------------- iOS

#[cfg(target_os = "ios")]
impl PlatformClipboard {
    /// `UIPasteboard.general`.
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

#[cfg(target_os = "ios")]
impl Default for PlatformClipboard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "ios")]
impl Clipboard for PlatformClipboard {
    /// # iOS 16 and the paste prompt
    ///
    /// Reading the general pasteboard shows the user a "Paste" confirmation
    /// unless the read came from a system paste control. `UIPasteboard.string`
    /// is the honest API for a keystroke-driven paste and takes that prompt;
    /// suppressing it means adopting `UIPasteControl`, which is a UIKit view and
    /// cannot be one of vieww's own. Recorded rather than worked around.
    fn read_text(&self) -> Result<Option<String>, ServiceError> {
        use objc2_ui_kit::UIPasteboard;

        // SAFETY: `generalPasteboard` is the process-wide pasteboard and is
        // valid for the life of the application. `string` returns an autoreleased
        // `NSString` that `Retained` takes ownership of.
        let pasteboard = unsafe { UIPasteboard::generalPasteboard() };
        let text = unsafe { pasteboard.string() };
        Ok(text.map(|string| string.to_string()))
    }

    fn write_text(&self, text: &str) -> Result<(), ServiceError> {
        use objc2_foundation::NSString;
        use objc2_ui_kit::UIPasteboard;

        let pasteboard = unsafe { UIPasteboard::generalPasteboard() };
        unsafe { pasteboard.setString(Some(&NSString::from_str(text))) };
        Ok(())
    }
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;

    /// Opening a pasteboard needs a display server, which CI does not have — so
    /// this asserts the *shape* of a failure rather than a round trip: an absent
    /// pasteboard must not panic, and must not be reported as text.
    ///
    /// The round trip is covered against [`MemoryClipboard`] in
    /// `vieww/tests/keys_to_semantics.rs`, where it belongs: what a field does
    /// with cut, copy and paste is this crate's business, and whether X11 is
    /// running is not.
    #[test]
    fn a_pasteboard_that_cannot_be_opened_reads_as_empty_rather_than_panicking() {
        let clipboard = PlatformClipboard::new();
        assert!(matches!(clipboard.read_text(), Ok(None) | Ok(Some(_))));
    }

    // ----- the Wayland route: decisions without a compositor (FW-19) ---------

    #[test]
    fn a_wayland_session_is_named_by_its_display_socket_and_nothing_else() {
        assert!(wayland_cli::session_is_wayland(Some("wayland-0")));
        assert!(wayland_cli::session_is_wayland(Some("wayland-1")));
        // Unset, empty, and blank are all "no compositor named" — an X11
        // session sets none of them.
        assert!(!wayland_cli::session_is_wayland(None));
        assert!(!wayland_cli::session_is_wayland(Some("")));
        assert!(!wayland_cli::session_is_wayland(Some("   ")));
    }

    #[test]
    fn the_tools_are_found_in_a_fake_path() {
        let dir = std::env::temp_dir().join("vieww-clipboard-wl-fake-path");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // One half at a time is still absent — a pasteboard that can copy but
        // not paste is not a pasteboard.
        std::fs::write(dir.join("wl-copy"), b"#!/bin/sh\n").unwrap();
        assert!(!wayland_cli::tools_in_dirs(std::slice::from_ref(&dir)));
        std::fs::write(dir.join("wl-paste"), b"#!/bin/sh\n").unwrap();
        assert!(wayland_cli::tools_in_dirs(std::slice::from_ref(&dir)));

        // A `PATH` that names the directory twice is fine, and one that names
        // only *other* directories is not.
        assert!(wayland_cli::tools_in_dirs(&[dir.clone(), dir.clone()]));
        let elsewhere = std::env::temp_dir().join("vieww-clipboard-wl-nowhere");
        let _ = std::fs::remove_dir_all(&elsewhere);
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert!(!wayland_cli::tools_in_dirs(std::slice::from_ref(
            &elsewhere
        )));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    #[test]
    fn the_paste_argv_suppresses_the_newline_the_tool_adds() {
        // `wl-paste` appends a newline of its own; without `--no-newline`
        // every pasted line silently grows one, which is a data corruption
        // bug that ships looking like a feature.
        assert_eq!(wayland_cli::paste_args(), &["--no-newline"]);
    }

    #[test]
    fn the_copy_argv_keeps_the_selection_alive() {
        // No `-o`/`--paste-once`: the forked child serving the selection is
        // the ownership semantics every other platform's clipboard has, and a
        // paste-once copy is a copy that silently evaporates.
        assert!(wayland_cli::copy_args().is_empty());
    }

    #[test]
    fn an_arboard_failure_on_a_wayland_session_names_the_native_route() {
        let error = wayland_cli::hinted(
            ServiceError::failed("arboard: no X server".to_owned()),
            true,
        );
        let message = match error {
            ServiceError::Failed(message) => message,
            other => panic!("a Failed stays Failed: {other:?}"),
        };
        assert!(message.contains("arboard: no X server"), "{message}");
        assert!(message.contains("wl-clipboard"), "{message}");
        assert!(message.contains("apt install"), "{message}");

        // Off Wayland the original error travels untouched — a hint about
        // Wayland tools on an X11 session is noise that hides the real cause.
        let untouched = wayland_cli::hinted(
            ServiceError::failed("arboard: no X server".to_owned()),
            false,
        );
        assert!(matches!(untouched, ServiceError::Failed(_)));
        assert_eq!(
            match untouched {
                ServiceError::Failed(message) => message,
                _ => unreachable!(),
            },
            "arboard: no X server"
        );
    }
}
