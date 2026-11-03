//! [`PlatformViews`] as Win32 child windows (FW-20's non-Linux backend).
//!
//! The Windows counterpart of [`platform_views`](crate::platform_views)'s
//! X11 child windows, with the same one registered kind — a plain child
//! window cleared to the spec's `background` (`"#rrggbb"`) — under its own
//! name, [`KIND`]. The OS composites it; vieww positions it. Bounds arrive in
//! logical pixels and are placed at the host's scale ([`Win32PlatformViews::set_scale`]),
//! which the platform layer keeps in step with `Window::scale_factor()`.
//!
//! **Type-checked, not run, from the Linux sandbox this was written in**:
//! the Windows legs of `hardware.yml` / framework-checks are its first runs,
//! and a failure there is a finding, not a flake.

#![cfg(target_os = "windows")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use vieww_foundation::capability::{
    PlatformViewHandle, PlatformViewId, PlatformViewSpec, PlatformViews,
};
use vieww_foundation::{Rect, ServiceError};

/// The `kind` this backend registers.
pub const KIND: &str = "win32-window";

type Hwnd = *mut c_void;
type Hbrush = *mut c_void;
type Hinstance = *mut c_void;
type Lresult = isize;
type Wparam = usize;
type Lparam = isize;

#[repr(C)]
struct WndClassW {
    style: u32,
    wnd_proc: unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult,
    cls_extra: i32,
    wnd_extra: i32,
    instance: Hinstance,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: Hbrush,
    menu_name: *const u16,
    class_name: *const u16,
}

#[repr(C)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, w: Wparam, l: Lparam) -> Lresult;
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn MoveWindow(hwnd: Hwnd, x: i32, y: i32, width: i32, height: i32, repaint: i32) -> i32;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
    fn GetClientRect(hwnd: Hwnd, rect: *mut WinRect) -> i32;
    fn FillRect(dc: *mut c_void, rect: *const WinRect, brush: Hbrush) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> Hbrush;
    fn DeleteObject(object: *mut c_void) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
}

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
const WM_ERASEBKGND: u32 = 0x0014;
const GWLP_USERDATA: i32 = -21;

/// `"VieWWPlatformView\0"` as UTF-16.
fn class_name() -> Vec<u16> {
    "VieWWPlatformView\0".encode_utf16().collect()
}

/// Fill the client area with the brush the window carries in its user data.
unsafe extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, w: Wparam, l: Lparam) -> Lresult {
    if msg == WM_ERASEBKGND {
        // SAFETY: `hwnd` is the window this procedure serves; its user data
        // is the brush `create` stored (or 0 before that).
        let brush = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as Hbrush;
        if !brush.is_null() {
            let mut rect = WinRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: `w` is the HDC Windows passes with WM_ERASEBKGND.
            unsafe {
                GetClientRect(hwnd, &raw mut rect);
                FillRect(w as *mut c_void, &raw const rect, brush);
            }
            return 1;
        }
    }
    // SAFETY: the default procedure for everything else.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

struct Inner {
    host: Option<Hwnd>,
    scale: f64,
    next: u64,
    /// id → (child HWND, its brush, its logical bounds).
    views: HashMap<PlatformViewId, (Hwnd, Hbrush, Rect)>,
    class_registered: bool,
}

/// [`PlatformViews`] as Win32 child windows of the anchored host.
pub struct Win32PlatformViews {
    inner: Rc<RefCell<Inner>>,
}

impl std::fmt::Debug for Win32PlatformViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32PlatformViews").finish_non_exhaustive()
    }
}

impl Default for Win32PlatformViews {
    fn default() -> Self {
        Self::new()
    }
}

fn device(bounds: Rect, scale: f64) -> (i32, i32, i32, i32) {
    let edge = |v: f32| (f64::from(v) * scale).round();
    let (l, t, r, b) = (
        edge(bounds.left),
        edge(bounds.top),
        edge(bounds.right),
        edge(bounds.bottom),
    );
    (
        l as i32,
        t as i32,
        (r - l).max(1.0) as i32,
        (b - t).max(1.0) as i32,
    )
}

/// `"#rrggbb"` as a `COLORREF` (`0x00bbggrr`); white without one.
fn colorref(spec: &PlatformViewSpec) -> Result<u32, String> {
    let Some(text) = spec.parameters.get("background") else {
        return Ok(0x00ff_ffff);
    };
    let hex = text
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| text.clone())?;
    let rgb = u32::from_str_radix(hex, 16).map_err(|_| text.clone())?;
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    Ok((b << 16) | (g << 8) | r)
}

impl Win32PlatformViews {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                host: None,
                scale: 1.0,
                next: 0,
                views: HashMap::new(),
                class_registered: false,
            })),
        }
    }

    /// The window new views are children of.
    ///
    /// # Errors
    ///
    /// When the handle is not a Win32 window.
    pub fn anchor(&self, host: &dyn HasWindowHandle) -> Result<(), ServiceError> {
        let handle = host
            .window_handle()
            .map_err(|e| ServiceError::failed(format!("the host window's handle: {e}")))?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win) => {
                self.inner.borrow_mut().host = Some(win.hwnd.get() as Hwnd);
                Ok(())
            }
            other => Err(ServiceError::failed(format!(
                "Win32 child windows need a Win32 host; this handle is {other:?}"
            ))),
        }
    }

    /// The host's scale factor; live views are moved to match.
    pub fn set_scale(&self, scale: f64) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let mut inner = self.inner.borrow_mut();
        inner.scale = scale;
        for &(hwnd, _, bounds) in inner.views.values() {
            let (x, y, w, h) = device(bounds, scale);
            // SAFETY: a live child this service created.
            unsafe { MoveWindow(hwnd, x, y, w, h, 1) };
        }
    }
}

impl PlatformViews for Win32PlatformViews {
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
        let color = colorref(spec).map_err(|bad| {
            ServiceError::failed(format!(
                "the \"background\" parameter must be \"#rrggbb\"; got {bad:?}"
            ))
        })?;
        let mut inner = self.inner.borrow_mut();
        let host = inner
            .host
            .ok_or_else(|| ServiceError::failed("no host window: anchor() first"))?;
        let class = class_name();
        // SAFETY: plain Win32 calls with valid, NUL-terminated UTF-16 and a
        // live parent HWND from `anchor`.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            if !inner.class_registered {
                let wc = WndClassW {
                    style: 0,
                    wnd_proc,
                    cls_extra: 0,
                    wnd_extra: 0,
                    instance,
                    icon: std::ptr::null_mut(),
                    cursor: std::ptr::null_mut(),
                    background: std::ptr::null_mut(),
                    menu_name: std::ptr::null(),
                    class_name: class.as_ptr(),
                };
                RegisterClassW(&raw const wc);
                inner.class_registered = true;
            }
            let (x, y, w, h) = device(bounds, inner.scale);
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                x,
                y,
                w,
                h,
                host,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(ServiceError::failed("CreateWindowExW failed"));
        }
        // SAFETY: a brush for the window's lifetime, stored where the window
        // procedure reads it, deleted with the window.
        let brush = unsafe {
            let brush = CreateSolidBrush(color);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, brush as isize);
            brush
        };
        let id = PlatformViewId(inner.next);
        inner.next += 1;
        inner.views.insert(id, (hwnd, brush, bounds));
        let shared = Rc::clone(&self.inner);
        Ok(PlatformViewHandle::new(id, move |id| {
            if let Some((hwnd, brush, _)) = shared.borrow_mut().views.remove(&id) {
                // SAFETY: destroyed once, the map being the single owner.
                unsafe {
                    DestroyWindow(hwnd);
                    DeleteObject(brush);
                }
            }
        }))
    }

    fn set_bounds(&self, id: PlatformViewId, bounds: Rect) -> Result<(), ServiceError> {
        let mut inner = self.inner.borrow_mut();
        let scale = inner.scale;
        let Some(view) = inner.views.get_mut(&id) else {
            return Err(ServiceError::failed(format!(
                "platform view {id:?} is already gone"
            )));
        };
        if view.2 == bounds {
            return Ok(());
        }
        view.2 = bounds;
        let (x, y, w, h) = device(bounds, scale);
        // SAFETY: a live child this service created.
        unsafe { MoveWindow(view.0, x, y, w, h, 1) };
        Ok(())
    }

    fn send(&self, _id: PlatformViewId, _message: &str) -> Result<String, ServiceError> {
        Err(ServiceError::unsupported("PlatformViews"))
    }
}
