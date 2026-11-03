//! [`PlatformViews`] as AppKit subviews (FW-20's macOS backend).
//!
//! The macOS counterpart of the X11 and Win32 child windows: one registered
//! kind, [`KIND`], an `NSBox` filled with the spec's `background`, added as
//! a subview of the window's content view (the `ns_view` winit's handle
//! names). AppKit lays out in points, which are vieww's logical pixels, so
//! no scale conversion happens here — the backing store's density is the
//! OS's business. AppKit's y axis runs upward unless the parent is flipped,
//! so each frame is converted against the parent's own bounds.
//!
//! **Type-checked, not run, from the Linux sandbox this was written in**;
//! Mac real-device verification is out of scope by the owner's decision,
//! and the macOS CI leg is its first compile on Apple's toolchain.

#![cfg(target_os = "macos")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use vieww_foundation::capability::{
    PlatformViewHandle, PlatformViewId, PlatformViewSpec, PlatformViews,
};
use vieww_foundation::{Rect, ServiceError};

/// The `kind` this backend registers.
pub const KIND: &str = "appkit-view";

/// `NSBoxCustom`: a box that draws exactly its fill colour and border.
const NS_BOX_CUSTOM: usize = 4;

struct Inner {
    /// The content view, borrowed (the window owns it).
    host: Option<*mut AnyObject>,
    next: u64,
    /// id → (the `NSBox`, owned +1 by this map, and its logical bounds).
    views: HashMap<PlatformViewId, (*mut AnyObject, Rect)>,
}

/// [`PlatformViews`] as subviews of the anchored window's content view.
pub struct AppKitPlatformViews {
    inner: Rc<RefCell<Inner>>,
}

impl std::fmt::Debug for AppKitPlatformViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppKitPlatformViews")
            .finish_non_exhaustive()
    }
}

impl Default for AppKitPlatformViews {
    fn default() -> Self {
        Self::new()
    }
}

/// `"#rrggbb"` as sRGB components; mid grey without one.
fn rgb(spec: &PlatformViewSpec) -> Result<(f64, f64, f64), String> {
    let Some(text) = spec.parameters.get("background") else {
        return Ok((0.5, 0.5, 0.5));
    };
    let hex = text
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| text.clone())?;
    let v = u32::from_str_radix(hex, 16).map_err(|_| text.clone())?;
    let c = |shift: u32| f64::from((v >> shift) & 0xff) / 255.0;
    Ok((c(16), c(8), c(0)))
}

/// A logical, top-left-origin rectangle as the parent's own frame.
///
/// # Safety
///
/// `parent` is a live `NSView`.
unsafe fn frame_in(parent: *mut AnyObject, bounds: Rect) -> NSRect {
    // SAFETY: plain property reads on a live view.
    let (flipped, parent_bounds): (Bool, NSRect) =
        unsafe { (msg_send![parent, isFlipped], msg_send![parent, bounds]) };
    let width = f64::from(bounds.right - bounds.left).max(1.0);
    let height = f64::from(bounds.bottom - bounds.top).max(1.0);
    let y = if flipped.as_bool() {
        f64::from(bounds.top)
    } else {
        parent_bounds.size.height - f64::from(bounds.bottom)
    };
    NSRect::new(
        NSPoint::new(f64::from(bounds.left), y),
        NSSize::new(width, height),
    )
}

impl AppKitPlatformViews {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                host: None,
                next: 0,
                views: HashMap::new(),
            })),
        }
    }

    /// The content view new views are added to.
    ///
    /// # Errors
    ///
    /// When the handle is not an AppKit view.
    pub fn anchor(&self, host: &dyn HasWindowHandle) -> Result<(), ServiceError> {
        let handle = host
            .window_handle()
            .map_err(|e| ServiceError::failed(format!("the host window's handle: {e}")))?;
        match handle.as_raw() {
            RawWindowHandle::AppKit(view) => {
                self.inner.borrow_mut().host = Some(view.ns_view.as_ptr().cast());
                Ok(())
            }
            other => Err(ServiceError::failed(format!(
                "AppKit subviews need an AppKit host; this handle is {other:?}"
            ))),
        }
    }

    /// Points are logical pixels on macOS: nothing to convert. Kept so the
    /// platform layer can treat every backend alike.
    pub const fn set_scale(&self, _scale: f64) {}
}

impl PlatformViews for AppKitPlatformViews {
    fn registered(&self) -> Vec<String> {
        vec![KIND.to_owned()]
    }

    fn create(
        &self,
        spec: &PlatformViewSpec,
        bounds: Rect,
    ) -> Result<PlatformViewHandle, ServiceError> {
        if spec.kind != KIND {
            return Err(ServiceError::failed(format!(
                "no factory registered for kind {:?}; this platform offers {:?}",
                spec.kind,
                self.registered()
            )));
        }
        let (r, g, b) = rgb(spec).map_err(|bad| {
            ServiceError::failed(format!(
                "the \"background\" parameter must be \"#rrggbb\"; got {bad:?}"
            ))
        })?;
        let mut inner = self.inner.borrow_mut();
        let host = inner
            .host
            .ok_or_else(|| ServiceError::failed("no host window: anchor() first"))?;
        // SAFETY: AppKit on the main thread (the application's, which is
        // where the trait is called); `host` is the window's live content
        // view; the box is +1 from alloc/init and owned by `views`.
        let view: *mut AnyObject = unsafe {
            let frame = frame_in(host, bounds);
            let boxed: *mut AnyObject = msg_send![class!(NSBox), alloc];
            let boxed: *mut AnyObject = msg_send![boxed, initWithFrame: frame];
            let _: () = msg_send![boxed, setBoxType: NS_BOX_CUSTOM];
            let _: () = msg_send![boxed, setBorderWidth: 0.0f64];
            let _: () = msg_send![boxed, setTitlePosition: 0usize];
            let color: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: r,
                green: g,
                blue: b,
                alpha: 1.0f64
            ];
            let _: () = msg_send![boxed, setFillColor: color];
            let _: () = msg_send![host, addSubview: boxed];
            boxed
        };
        if view.is_null() {
            return Err(ServiceError::failed("NSBox could not be created"));
        }
        let id = PlatformViewId(inner.next);
        inner.next += 1;
        inner.views.insert(id, (view, bounds));
        let shared = Rc::clone(&self.inner);
        Ok(PlatformViewHandle::new(id, move |id| {
            if let Some((view, _)) = shared.borrow_mut().views.remove(&id) {
                // SAFETY: removed and released once, the map being the owner.
                unsafe {
                    let _: () = msg_send![view, removeFromSuperview];
                    let _: () = msg_send![view, release];
                }
            }
        }))
    }

    fn set_bounds(&self, id: PlatformViewId, bounds: Rect) -> Result<(), ServiceError> {
        let mut inner = self.inner.borrow_mut();
        let host = inner.host;
        let Some(view) = inner.views.get_mut(&id) else {
            return Err(ServiceError::failed(format!(
                "platform view {id:?} is already gone"
            )));
        };
        if view.1 == bounds {
            return Ok(());
        }
        view.1 = bounds;
        if let Some(host) = host {
            // SAFETY: a live subview this service created, on the main thread.
            unsafe {
                let frame = frame_in(host, bounds);
                let _: () = msg_send![view.0, setFrame: frame];
            }
        }
        Ok(())
    }

    fn send(&self, _id: PlatformViewId, _message: &str) -> Result<String, ServiceError> {
        Err(ServiceError::unsupported("PlatformViews"))
    }
}
