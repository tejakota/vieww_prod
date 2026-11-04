//! The platform's own [`PlatformViews`], registered by
//! [`services::platform`](crate::services::platform) and anchored by the
//! platform layer — FW-20's anchor decision.
//!
//! # The decision the first consumer left open
//!
//! `docs/architecture/platform-views.md` recorded three places the host
//! window could reach the service: on the trait, through the platform
//! layer's automatic wiring, or by an inherent `anchor` the application
//! calls. With a widget consumer and three backends now in the tree, the
//! answer is the **platform layer's wiring**: `App` owns the window, so `App`
//! anchors this service when it creates the window and keeps the scale in
//! step on `ScaleFactorChanged`. The trait stays unchanged (Android and iOS
//! answer "which window" with an activity or a view controller, not a
//! handle), and the backends keep their inherent `anchor` for embedders that
//! run without an `App`.
//!
//! # Lazy
//!
//! Nothing is opened until a window is anchored: an application that never
//! embeds a view pays for one `Rc` and nothing else — on Linux, no X11
//! connection is made until then, and a Wayland window (or no X11 at all)
//! leaves the service refusing with the reason, never failing the start-up.

// Android hosts no backend here (its views come through the JNI layer), so
// the registration and anchoring calls are compiled out there.
#![cfg_attr(target_os = "android", allow(dead_code))]
use std::cell::RefCell;
use std::rc::Rc;

use raw_window_handle::HasWindowHandle;
use vieww_foundation::capability::{
    PlatformViewHandle, PlatformViewId, PlatformViewSpec, PlatformViews,
};
use vieww_foundation::{Rect, ServiceError};

#[cfg(target_os = "linux")]
type Backend = crate::platform_views::X11PlatformViews;
#[cfg(target_os = "windows")]
type Backend = crate::platform_views_win32::Win32PlatformViews;
#[cfg(target_os = "macos")]
type Backend = crate::platform_views_appkit::AppKitPlatformViews;

#[derive(Default)]
struct State {
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    backend: Option<Backend>,
    /// Why there is no backend, once an anchor has been attempted.
    refused: Option<String>,
    scale: f64,
    /// A window has been anchored successfully. Views belong to the first
    /// window the application opens; a dialog opened later does not take
    /// them over.
    anchored: bool,
}

/// The platform's `dyn PlatformViews`: X11 child windows on Linux, Win32
/// child windows on Windows, AppKit subviews on macOS, an honest refusal
/// elsewhere.
#[derive(Clone, Default)]
pub struct OsPlatformViews {
    state: Rc<RefCell<State>>,
}

impl std::fmt::Debug for OsPlatformViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsPlatformViews")
            .field("refused", &self.state.borrow().refused)
            .finish_non_exhaustive()
    }
}

thread_local! {
    /// The instance `services::platform` registered, for `App` to anchor.
    static REGISTERED: RefCell<Option<OsPlatformViews>> = const { RefCell::new(None) };
}

impl OsPlatformViews {
    /// Register into `services` and remember the instance for the window.
    pub(crate) fn provide(services: &mut vieww_foundation::Services) {
        let views = Self::default();
        REGISTERED.with(|slot| *slot.borrow_mut() = Some(views.clone()));
        services.provide::<dyn PlatformViews>(Rc::new(views));
    }

    /// Anchor to `window` — called by `App` when it creates the window.
    /// The backend is opened here, the first time, so a failure to open it
    /// is recorded as the reason views are refused.
    pub fn anchor(&self, window: &dyn HasWindowHandle, scale: f64) {
        let mut state = self.state.borrow_mut();
        state.scale = scale;
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        {
            if state.backend.is_none() {
                #[cfg(target_os = "linux")]
                let made = crate::platform_views::X11PlatformViews::connect();
                #[cfg(not(target_os = "linux"))]
                let made: Result<Backend, String> = Ok(Backend::new());
                match made {
                    Ok(backend) => state.backend = Some(backend),
                    Err(why) => {
                        state.refused = Some(why);
                        return;
                    }
                }
            }
            let backend = state.backend.as_ref().expect("opened above");
            match backend.anchor(window) {
                Ok(()) => {
                    backend.set_scale(scale);
                    state.refused = None;
                    state.anchored = true;
                }
                Err(error) => state.refused = Some(error.to_string()),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            let _ = window;
            state.refused = Some("no platform-view backend on this OS".to_owned());
        }
    }

    /// The window's scale factor changed.
    pub fn set_scale(&self, scale: f64) {
        let mut state = self.state.borrow_mut();
        state.scale = scale;
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        if let Some(backend) = &state.backend {
            backend.set_scale(scale);
        }
    }

    /// Why views are refused right now, if they are.
    #[must_use]
    pub fn refusal(&self) -> Option<String> {
        self.state.borrow().refused.clone()
    }

    fn unanchored(&self) -> ServiceError {
        // Permanent where the session cannot host views (the reason is
        // `refusal()`'s), transient before the window exists.
        match &self.state.borrow().refused {
            Some(_) => ServiceError::unsupported("PlatformViews"),
            None => ServiceError::failed("no window yet: views are created once the window exists"),
        }
    }
}

/// Anchor the registered instance, if `services::platform` made one.
pub(crate) fn anchor_registered(window: &dyn HasWindowHandle, scale: f64) {
    let registered = REGISTERED.with(|slot| slot.borrow().clone());
    if let Some(views) = registered {
        if !views.state.borrow().anchored {
            views.anchor(window, scale);
        }
    }
}

/// The window's new scale, for the registered instance.
pub(crate) fn scale_registered(scale: f64) {
    let registered = REGISTERED.with(|slot| slot.borrow().clone());
    if let Some(views) = registered {
        views.set_scale(scale);
    }
}

impl PlatformViews for OsPlatformViews {
    fn registered(&self) -> Vec<String> {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        if let Some(backend) = &self.state.borrow().backend {
            return backend.registered();
        }
        Vec::new()
    }

    fn create(
        &self,
        spec: &PlatformViewSpec,
        bounds: Rect,
    ) -> Result<PlatformViewHandle, ServiceError> {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        if let Some(backend) = &self.state.borrow().backend {
            if self.state.borrow().refused.is_none() {
                return backend.create(spec, bounds);
            }
        }
        let _ = (spec, bounds);
        Err(self.unanchored())
    }

    fn set_bounds(&self, id: PlatformViewId, bounds: Rect) -> Result<(), ServiceError> {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        if let Some(backend) = &self.state.borrow().backend {
            return backend.set_bounds(id, bounds);
        }
        let _ = (id, bounds);
        Err(self.unanchored())
    }

    fn send(&self, id: PlatformViewId, message: &str) -> Result<String, ServiceError> {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        if let Some(backend) = &self.state.borrow().backend {
            return backend.send(id, message);
        }
        let _ = (id, message);
        Err(self.unanchored())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use vieww_foundation::Size;
    use vieww_render::FrameDriver;
    use vieww_widget::{Align, PlatformView, PlatformViewController, SizedBox};

    #[test]
    fn before_a_window_views_are_refused_transiently() {
        let views = OsPlatformViews::default();
        let error = views
            .create(
                &PlatformViewSpec::new("x11-window"),
                Rect::new(0.0, 0.0, 1.0, 1.0),
            )
            .expect_err("no window yet");
        assert!(!error.is_permanent(), "a window may still come: {error:?}");
    }

    /// The whole FW-20 chain on a real X server: a `PlatformView` widget laid
    /// out by a real `FrameDriver`, through the platform's own service,
    /// anchored to a real window at scale 1.5 — the child window the server
    /// reports is where layout put the widget, in device pixels.
    #[test]
    fn a_platform_view_widget_becomes_a_real_child_window_at_the_hosts_scale() {
        vieww_hardware::skip_without!(display);
        let x11 = match crate::platform_views::X11PlatformViews::connect() {
            Ok(x11) => x11,
            Err(reason) => {
                eprintln!("skipping: {reason}");
                return;
            }
        };
        let host = x11.host_window(800, 600);

        let views = OsPlatformViews::default();
        views.anchor(&host, 1.5);
        assert_eq!(views.refusal(), None);
        let service: Rc<dyn PlatformViews> = Rc::new(views.clone());
        let controller = PlatformViewController::new(
            service,
            PlatformViewSpec::new(crate::platform_views::KIND).with("background", "#2a7af6"),
        );

        let mut driver = FrameDriver::new(Size::new(400.0, 300.0));
        driver.set_root(
            Align::new(vieww_foundation::Alignment::CENTER).child(
                SizedBox::from_size(Size::new(100.0, 60.0))
                    .child(PlatformView::new(controller.clone())),
            ),
        );
        driver.draw_frame();
        assert!(controller.is_created(), "{:?}", controller.error());
        let children = host.child_xids();
        assert_eq!(children.len(), 1);
        // Centred in 400×300: (150, 120)–(250, 180) logical, ×1.5.
        assert_eq!(host.geometry_of(children[0]), Some((225, 180, 150, 90)));

        drop(driver);
        assert!(
            host.child_xids().is_empty(),
            "leaving the tree destroyed it"
        );
    }
}
