//! The first real [`PlatformViews`]: X11 child windows, on Linux.
//!
//! # Why this module exists, and what it is honest about
//!
//! `vieww_foundation::capability` has declared the platform-view shape
//! (spec, handle, `create`/`set_bounds`/`send`) since the capability module
//! was written, and until now **nothing has ever implemented it** — the gap
//! PENDING §6.2 names: *"no application has ever used one. Whether the
//! existing shape is the right one is unknown, and it is the kind of thing
//! whose design is decided by the second consumer rather than the first."*
//! This module is the first consumer, and it exists to teach the rest of the
//! tree what the shape feels like from the inside. The now-known shape —
//! including the one gap it found, below — is declared in
//! `docs/architecture/platform-views.md`.
//!
//! # The one gap the first consumer found: the anchor
//!
//! [`PlatformViews::create`] positions a view "at `bounds` in logical pixels
//! from the top-left of **the window**" — but the service is built before
//! any window exists (`services::platform()` runs before `App::run` creates
//! one), and the registry it goes into is the application's own, closed by
//! the time the platform layer could add to it. So the host window has to be
//! handed to the service explicitly, by whoever owns it, and this module
//! puts that on an inherent method — [`X11PlatformViews::anchor`] — rather
//! than on the trait, because a trait method would force every
//! implementation (Android's, iOS's, the web's) to answer a question only
//! this one needs asked. Whether the anchor belongs on the trait, on the
//! platform layer's automatic wiring, or stays inherent is explicitly the
//! second consumer's decision to make; the architecture doc says so.
//!
//! # The implementation, and the X11 properties it leans on
//!
//! - **`dlopen`, not a link-time dependency.** `libX11.so.6` is opened at
//!   runtime through `libloading` (already this workspace's loader —
//!   `vieww-reload`, `vieww-plugin`), the same decision `softbuffer`'s
//!   `x11-dlopen` feature makes for this crate's CPU present path: a
//!   Wayland-only session has no X11, and a framework whose Linux builds
//!   hard-require it would be refusing to run where the OS lets it. A
//!   session without X11 gets `connect()` returning `Err`, and the caller
//!   falls back to [`NoPlatformViews`](vieww_foundation::capability::NoPlatformViews)
//!   — embedding is refused honestly
//!   rather than the app failing to start.
//! - **Its own `Display` connection.** X11 window IDs are *server-global*,
//!   not per-connection, so a child of a window created on winit's connection
//!   can be created, moved and destroyed from a second connection without
//!   either side holding a pointer into the other. That is what makes the
//!   explicit-anchor shape workable at all: the service never borrows
//!   winit's `Display*` (whose lifetime is the window's, i.e. shorter than
//!   the service's), only the XID — a `u64` that stays valid as long as the
//!   host window is mapped.
//! - **One thread.** An X11 `Display` is not thread-safe, and nothing here
//!   implements `Send`/`Sync` on purpose: the service is created, anchored,
//!   called and dropped on the application's thread, which is also the frame
//!   loop's. The trait's own contract (sequential, never re-entered) is the
//!   same rule stated for plugins.
//!
//! # What a view *is* here
//!
//! The one registered `kind` is `"x11-window"`: a plain child window the X
//! server clears to a background colour taken from the spec's `background`
//! parameter (`"#rrggbb"`). That is deliberately the most minimal thing that
//! is still *real* — the OS composites it, vieww only positions it — because
//! the point of the first consumer is the seam, not the content. A video
//! surface, a map or a browser would register further kinds against the same
//! `create`/`set_bounds`/dispose lifecycle; the factory-registry shape (a
//! `kind` string plus string parameters) is what the declared architecture
//! hangs that on.
//!
//! # Untested notes, stated rather than implied
//!
//! - **Scale.** Bounds arrive in logical pixels and are placed in device
//!   pixels at the host's scale factor ([`X11PlatformViews::set_scale`];
//!   `App` keeps it in step with `Window::scale_factor()`), edges rounded so
//!   abutting views stay abutting at a fractional scale.
//! - **`background` assumes a TrueColor visual** whose channel masks are the
//!   standard `0xff0000/0x00ff00/0x0000ff` (every default visual of the last
//!   twenty years, and Xvfb's). A 8-bit PseudoColor screen would need a
//!   colormap allocation this module does not do.
//! - **Reparenting on re-anchor is not implemented.** Anchoring a second host
//!   moves where *new* views parent; existing ones keep their old parent until
//!   destroyed, because `XReparentWindow` brings its own event-ordering traps
//!   that nothing here has a reason to pay for yet.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint, c_ulong, c_void};
use std::rc::Rc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle, WindowHandle};
use vieww_foundation::capability::{
    PlatformViewHandle, PlatformViewId, PlatformViewSpec, PlatformViews,
};
use vieww_foundation::{Rect, ServiceError};

/// X11's `Window`/`Drawable`/`Pixmap`: a server-global ID, never a pointer,
/// valid across connections for as long as the resource exists.
type Xid = c_ulong;

/// An open `Display*`. Opaque: Xlib's `Display` is a struct whose layout is
/// private to the library, so the only honest type for it is a pointer to
/// nothing in particular.
type DisplayPtr = *mut c_void;

/// The `kind` string this implementation registers exactly one factory for.
pub const KIND: &str = "x11-window";

/// The library every symbol below comes from. A missing one is a session
/// without X11 — reported, not fatal to the process.
const LIB_X11: &str = "libX11.so.6";

// ── The FFI table ──────────────────────────────────────────────────────────
//
// Signatures transcribed from Xlib.h (X11R7.x). Return types matter even for
// calls whose result is ignored: Xlib declares most "void-looking" calls as
// returning `int`, and declaring one `-> ()` while the library returns `int`
// is an ABI mismatch that happens to work on x86-64 and nowhere promised.

type OpenDisplayFn = unsafe extern "C" fn(display_name: *const c_char) -> DisplayPtr;
type CloseDisplayFn = unsafe extern "C" fn(display: DisplayPtr) -> c_int;
type DefaultScreenFn = unsafe extern "C" fn(display: DisplayPtr) -> c_int;
type RootWindowFn = unsafe extern "C" fn(display: DisplayPtr, screen: c_int) -> Xid;
type CreateSimpleWindowFn = unsafe extern "C" fn(
    display: DisplayPtr,
    parent: Xid,
    x: c_int,
    y: c_int,
    width: c_uint,
    height: c_uint,
    border_width: c_uint,
    border: c_ulong,
    background: c_ulong,
) -> Xid;
type DestroyWindowFn = unsafe extern "C" fn(display: DisplayPtr, window: Xid) -> c_int;
type MapWindowFn = unsafe extern "C" fn(display: DisplayPtr, window: Xid) -> c_int;
type MoveResizeWindowFn = unsafe extern "C" fn(
    display: DisplayPtr,
    window: Xid,
    x: c_int,
    y: c_int,
    width: c_uint,
    height: c_uint,
) -> c_int;
type FlushFn = unsafe extern "C" fn(display: DisplayPtr) -> c_int;
type SyncFn = unsafe extern "C" fn(display: DisplayPtr, discard: c_int) -> c_int;
type QueryTreeFn = unsafe extern "C" fn(
    display: DisplayPtr,
    window: Xid,
    root: *mut Xid,
    parent: *mut Xid,
    children: *mut *mut Xid,
    nchildren: *mut c_uint,
) -> c_int;
type GeometryFn = unsafe extern "C" fn(
    display: DisplayPtr,
    drawable: Xid,
    root: *mut Xid,
    x: *mut c_int,
    y: *mut c_int,
    width: *mut c_uint,
    height: *mut c_uint,
    border_width: *mut c_uint,
    depth: *mut c_uint,
) -> c_int;
type FreeFn = unsafe extern "C" fn(data: *mut c_void) -> c_int;

/// The dlopen'd symbols plus the one `Display` they all talk to.
///
/// Constructed once per [`X11PlatformViews`] (its own connection — see the
/// module doc for why that is legal), and kept alive by every handle the
/// service ever mints, so a view outliving the service still gets destroyed
/// through a live connection.
struct X11 {
    /// Kept, never read: dropping the `Library` would invalidate every fn
    /// pointer below. The field's whole job is lifetime.
    _library: libloading::Library,
    close_display: CloseDisplayFn,
    default_screen: DefaultScreenFn,
    root_window: RootWindowFn,
    create_simple_window: CreateSimpleWindowFn,
    destroy_window: DestroyWindowFn,
    map_window: MapWindowFn,
    move_resize_window: MoveResizeWindowFn,
    flush: FlushFn,
    sync: SyncFn,
    query_tree: QueryTreeFn,
    geometry: GeometryFn,
    free: FreeFn,
    display: DisplayPtr,
}

impl X11 {
    /// `dlopen` the library, resolve the symbols, open the connection.
    ///
    /// `name` is the display string (`":0"`, `":999"`); `None` is `$DISPLAY`,
    /// which is what every real caller wants and what a test with a
    /// deliberately-bogus name uses to prove the failure path.
    fn connect(name: Option<&str>) -> Result<Self, String> {
        // SAFETY: `libX11.so.6` is a system library with a stable C ABI; the
        // only failure modes are "absent" (a Wayland-only session) and
        // "corrupt", both of which `Library::new` reports instead of
        // mishandling.
        let library = unsafe { libloading::Library::new(LIB_X11) }
            .map_err(|error| format!("{LIB_X11} could not be opened: {error}"))?;

        // SAFETY: each `get` resolves a symbol that has existed under this
        // exact name and signature since X11R5; a missing one means the
        // library is not Xlib at all, which the error reports. The fn
        // pointers are copied out (fn pointers are `Copy`) so the `Symbol`'s
        // borrow of the library can end here while the library itself is
        // kept alive by `_library` above.
        macro_rules! symbol {
            ($name:literal) => {{
                unsafe {
                    *library
                        .get($name)
                        .map_err(|error| format!("{}: {}", stringify!($name), error))?
                }
            }};
        }

        let open_display: OpenDisplayFn = symbol!(b"XOpenDisplay\0");
        let close_display: CloseDisplayFn = symbol!(b"XCloseDisplay\0");
        let default_screen: DefaultScreenFn = symbol!(b"XDefaultScreen\0");
        let root_window: RootWindowFn = symbol!(b"XRootWindow\0");
        let create_simple_window: CreateSimpleWindowFn = symbol!(b"XCreateSimpleWindow\0");
        let destroy_window: DestroyWindowFn = symbol!(b"XDestroyWindow\0");
        let map_window: MapWindowFn = symbol!(b"XMapWindow\0");
        let move_resize_window: MoveResizeWindowFn = symbol!(b"XMoveResizeWindow\0");
        let flush: FlushFn = symbol!(b"XFlush\0");
        let sync: SyncFn = symbol!(b"XSync\0");
        let query_tree: QueryTreeFn = symbol!(b"XQueryTree\0");
        let geometry: GeometryFn = symbol!(b"XGetGeometry\0");
        let free: FreeFn = symbol!(b"XFree\0");

        let name = name.map(CString::new).transpose().map_err(|_| {
            "the display name contains a NUL byte, which X11 cannot be asked for".to_owned()
        })?;
        // SAFETY: `open_display` with a valid CString or null is the
        // documented contract; a null return means no such display, which is
        // the `Err` this function exists to report.
        let display = unsafe {
            match name.as_deref() {
                Some(name) => open_display(name.as_ptr()),
                None => open_display(std::ptr::null()),
            }
        };
        if display.is_null() {
            return Err(format!(
                "no X11 display {:?} (DISPLAY is {})",
                name.map_or_else(
                    || std::env::var("DISPLAY").unwrap_or_default(),
                    |s| s.to_string_lossy().into_owned()
                ),
                if std::env::var("DISPLAY").map_or(true, |v| v.is_empty()) {
                    "unset"
                } else {
                    "set"
                },
            ));
        }

        Ok(Self {
            _library: library,
            close_display,
            default_screen,
            root_window,
            create_simple_window,
            destroy_window,
            map_window,
            move_resize_window,
            flush,
            sync,
            query_tree,
            geometry,
            free,
            display,
        })
    }

    /// The root window of this connection's default screen — the parent a
    /// host window is created against.
    fn root(&self) -> Xid {
        // SAFETY: `display` is a live connection opened in `connect` and
        // alive for as long as `self` is.
        unsafe {
            let screen = (self.default_screen)(self.display);
            (self.root_window)(self.display, screen)
        }
    }

    /// Round-trip the connection, so every request before this point has
    /// reached the server before the next line runs. Tests query geometry
    /// right after creating and destroying; without this, `XQueryTree` may
    /// answer from before the create arrived.
    fn sync(&self) {
        // SAFETY: live connection; `0` = do not flush the event queue.
        unsafe { (self.sync)(self.display, 0) };
    }
}

impl Drop for X11 {
    fn drop(&mut self) {
        // SAFETY: `display` was opened by `connect` and is closed exactly
        // here, once, with no other reference to it in existence (the XID
        // handed to callers is a server-side number, not this pointer).
        unsafe {
            (self.close_display)(self.display);
        }
    }
}

/// Everything the service and the dispose closures share.
struct Inner {
    x11: Rc<X11>,
    state: RefCell<State>,
}

struct State {
    /// The host window's XID, once [`X11PlatformViews::anchor`] has run.
    host: Option<Xid>,
    /// Device pixels per logical pixel on the host — what turns the trait's
    /// logical bounds into X11's device geometry. 1.0 until told otherwise
    /// ([`X11PlatformViews::set_scale`]).
    scale: f64,
    /// Monotonic source of `PlatformViewId`s.
    next_id: u64,
    /// Live views: capability id → (X11 window, the bounds it is *supposed*
    /// to be at — the cache that makes an unchanged `set_bounds` free).
    views: HashMap<PlatformViewId, (Xid, Rect)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            host: None,
            scale: 1.0,
            next_id: 0,
            views: HashMap::new(),
        }
    }
}

impl Inner {
    fn destroy(&self, id: PlatformViewId) {
        let Some((window, _)) = self.state.borrow_mut().views.remove(&id) else {
            return; // double-dispose is a no-op, like every Drop guard here
        };
        // SAFETY: `window` was created by this service from a live
        // connection and is destroyed exactly once (the map remove above is
        // the single owner); destroying an already-destroyed XID would be a
        // harmless X error, which the remove makes unreachable anyway.
        unsafe {
            (self.x11.destroy_window)(self.x11.display, window);
            (self.x11.flush)(self.x11.display);
        }
    }
}

/// [`PlatformViews`] as real X11 child windows. See the module doc for the
/// shape, the anchor, and the honest limits.
pub struct X11PlatformViews {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for X11PlatformViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The connection and the window map are live Xlib state, not data —
        // what an embedder can act on is *that* the service is connected,
        // whether it is anchored, and how many views are live.
        let state = self.inner.state.borrow();
        f.debug_struct("X11PlatformViews")
            .field("kind", &KIND)
            .field("anchored", &state.host.is_some())
            .field("views", &state.views.len())
            .finish()
    }
}

impl X11PlatformViews {
    /// Open a connection to the session's X server (`$DISPLAY`).
    ///
    /// # Errors
    /// A [`String`] naming what is missing — no `libX11` (a Wayland-only
    /// session) or no display (a headless one). Callers fall back to
    /// `NoPlatformViews`, which makes embedding *unsupported* rather than
    /// making the application fail to start.
    pub fn connect() -> Result<Self, String> {
        Ok(Self {
            inner: Rc::new(Inner {
                x11: Rc::new(X11::connect(None)?),
                state: RefCell::default(),
            }),
        })
    }

    /// Give the service the window that views will be children of.
    ///
    /// The one wiring step the declared shape needs and the trait does not
    /// carry — see the module doc ("the one gap the first consumer found").
    /// A winit `Window` works here (it implements [`HasWindowHandle`]), and
    /// so does [`X11HostWindow`], the minimal host this module offers
    /// embedders that are not running a full vieww `App`.
    ///
    /// A second call replaces the anchor for *newly created* views; existing
    /// ones keep their current parent (no reparenting — declared open).
    ///
    /// # Errors
    /// [`ServiceError::failed`] when the handle is not an X11 window: a
    /// Wayland winit window cannot parent an Xlib child, and saying so beats
    /// creating a child of a meaningless XID.
    pub fn anchor(&self, host: &dyn HasWindowHandle) -> Result<(), ServiceError> {
        let handle = host
            .window_handle()
            .map_err(|error| ServiceError::failed(format!("the host window's handle: {error}")))?;
        let xid = match handle.as_raw() {
            // SAFETY-free: these are plain data read from the handle.
            RawWindowHandle::Xlib(xlib) => xlib.window as Xid,
            RawWindowHandle::Xcb(xcb) => u64::from(xcb.window.get()),
            other => {
                return Err(ServiceError::failed(format!(
                    "X11 child windows need an X11 host window; this handle is {other:?}"
                )));
            }
        };
        self.inner.state.borrow_mut().host = Some(xid);
        Ok(())
    }

    /// The host's scale factor — device pixels per logical pixel, winit's
    /// `Window::scale_factor()`. Every live view is moved to its bounds at
    /// the new scale, so a window dragged to a monitor of another density
    /// keeps its views where layout put them.
    pub fn set_scale(&self, scale: f64) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let mut state = self.inner.state.borrow_mut();
        if (state.scale - scale).abs() < f64::EPSILON {
            return;
        }
        state.scale = scale;
        let x11 = &self.inner.x11;
        for &(window, bounds) in state.views.values() {
            let (x, y, w, h) = device_geometry(bounds, scale);
            // SAFETY: a live child this service created, moved on the same
            // connection — as `set_bounds` does.
            unsafe { (x11.move_resize_window)(x11.display, window, x, y, w, h) };
        }
        // SAFETY: flushing a live connection.
        unsafe { (x11.flush)(x11.display) };
    }

    /// The scale factor views are placed at.
    #[must_use]
    pub fn scale(&self) -> f64 {
        self.inner.state.borrow().scale
    }

    /// A minimal top-level host window, for embedders that do not run a full
    /// vieww `App`: mapped, background-cleared, owning nothing but its XID.
    ///
    /// This is also what the module's own tests anchor against — a real X11
    /// window, not a mock, because the point of the first consumer is the
    /// seam rather than the content.
    #[must_use]
    pub fn host_window(&self, width: u32, height: u32) -> X11HostWindow {
        let x11 = Rc::clone(&self.inner.x11);
        // SAFETY: a fresh top-level window on the root of a live connection;
        // the black border and background are pixels, not colours, and 0 is
        // the classic default both.
        let xid = unsafe {
            (x11.create_simple_window)(
                x11.display,
                x11.root(),
                0,
                0,
                width,
                height,
                1,
                0,
                0x00_ff_ff_ff,
            )
        };
        // SAFETY: mapping a window just created on a live connection.
        unsafe { (x11.map_window)(x11.display, xid) };
        x11.sync();
        X11HostWindow { x11, xid }
    }
}

/// The logical bounds rectangle as X11 device geometry, `(x, y, width,
/// height)`, at `scale` device pixels per logical one.
///
/// The edges are scaled and rounded, then the size taken between them, so
/// two views that abut in logical pixels abut in device pixels too at a
/// fractional scale (scaling the size separately can open a one-pixel
/// seam). The size is clamped to at least one pixel, because a zero-sized X
/// window is a server error.
fn device_geometry(bounds: Rect, scale: f64) -> (c_int, c_int, c_uint, c_uint) {
    let edge = |v: f32| (f64::from(v) * scale).round();
    let (left, top) = (edge(bounds.left), edge(bounds.top));
    let (right, bottom) = (edge(bounds.right), edge(bounds.bottom));
    (
        left as c_int,
        top as c_int,
        (right - left).max(1.0) as c_uint,
        (bottom - top).max(1.0) as c_uint,
    )
}

impl PlatformViews for X11PlatformViews {
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
        let host = self.inner.state.borrow().host.ok_or_else(|| {
            ServiceError::failed(
                "no host window: anchor() must be called with the application's window \
                     before views can be created",
            )
        })?;
        let background = background_pixel(spec).map_err(|offender| {
            ServiceError::failed(format!(
                "the \"background\" parameter must be \"#rrggbb\"; got {offender:?}"
            ))
        })?;
        let x11 = Rc::clone(&self.inner.x11);
        let scale = self.inner.state.borrow().scale;
        let (x, y, w, h) = device_geometry(bounds, scale);
        // SAFETY: `host` came from `anchor` (an X11 window the caller says
        // is live), and `create_simple_window` on a live connection with
        // in-bounds geometry values either returns a new window XID or sets
        // an X error it cannot here (geometry is clamped, not negative).
        let window = unsafe {
            (x11.create_simple_window)(x11.display, host, x, y, w, h, 0, 0, background as c_ulong)
        };
        // SAFETY: mapping the window just created, same live connection —
        // and flushing it: this connection is the service's own and nothing
        // else ever flushes it, so without this the server would not see the
        // window until some later request happened to (found by the first
        // cross-connection test: the app's window asked the server and saw
        // no child).
        unsafe {
            (x11.map_window)(x11.display, window);
            (x11.flush)(x11.display);
        }

        let id = {
            let mut state = self.inner.state.borrow_mut();
            let id = PlatformViewId(state.next_id);
            state.next_id += 1;
            state.views.insert(id, (window, bounds));
            id
        };
        let inner = Rc::clone(&self.inner);
        Ok(PlatformViewHandle::new(id, move |id| inner.destroy(id)))
    }

    fn set_bounds(&self, id: PlatformViewId, bounds: Rect) -> Result<(), ServiceError> {
        let mut state = self.inner.state.borrow_mut();
        let Some(&(window, current)) = state.views.get(&id) else {
            return Err(ServiceError::failed(format!(
                "platform view {:?} is already gone",
                id
            )));
        };
        // The trait's own contract: an unchanged rectangle must be cheap —
        // this is on the path of any scroll that contains a view, and a
        // no-op XMoveResizeWindow is still a request, a flush and a
        // round-trip this branch does not pay.
        if current == bounds {
            return Ok(());
        }
        let x11 = Rc::clone(&self.inner.x11);
        let (x, y, w, h) = device_geometry(bounds, state.scale);
        // SAFETY: `window` is a live child of the anchored host, created by
        // this same service (the map holds it), and move/resize is not
        // destructive: a bad value clamps at the server.
        unsafe {
            (x11.move_resize_window)(x11.display, window, x, y, w, h);
            (x11.flush)(x11.display);
        }
        state.views.insert(id, (window, bounds));
        Ok(())
    }

    fn send(&self, _id: PlatformViewId, _message: &str) -> Result<String, ServiceError> {
        // The trait's documented refusal for views that take no messages: an
        // X11 window has no message channel, and inventing an XClientMessage
        // protocol here would be a second ABI nobody asked for.
        Err(ServiceError::unsupported("PlatformViews"))
    }
}

/// A top-level X11 window created by [`X11PlatformViews::host_window`], for
/// embedders without a vieww `App`. Dropping it destroys the window.
pub struct X11HostWindow {
    x11: Rc<X11>,
    xid: Xid,
}

impl std::fmt::Debug for X11HostWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The XID is the identity; the connection behind it is a pointer into
        // Xlib, printed as nothing rather than as an address.
        f.debug_struct("X11HostWindow")
            .field("xid", &self.xid)
            .finish()
    }
}

impl X11HostWindow {
    /// The XID of the window, for an embedder that needs to talk to X11
    /// itself (or for a test that needs to ask the server what happened).
    #[must_use]
    pub const fn xid(&self) -> Xid {
        self.xid
    }

    /// The X11 geometry of any window, as `(x, y, width, height)` relative
    /// to its parent — the assertion oracle for "the child is where
    /// `set_bounds` said", read from the server rather than from our own
    /// bookkeeping.
    #[must_use]
    pub fn geometry_of(&self, window: Xid) -> Option<(i32, i32, u32, u32)> {
        let mut root: Xid = 0;
        let (mut x, mut y) = (0i32, 0i32);
        let (mut w, mut h) = (0u32, 0u32);
        let (mut border, mut depth) = (0u32, 0u32);
        // SAFETY: all out-pointers are valid locals; a zero status means the
        // window does not exist (destroyed, or never did), which is the
        // `None` this returns — the answer the tests want when they ask
        // about a disposed view.
        let status = unsafe {
            (self.x11.geometry)(
                self.x11.display,
                window,
                &mut root,
                &mut x,
                &mut y,
                &mut w,
                &mut h,
                &mut border,
                &mut depth,
            )
        };
        if status == 0 {
            return None;
        }
        Some((x, y, w, h))
    }

    /// The XIDs of this window's children, as the server sees them right
    /// now — the oracle for "the child window really exists / really does
    /// not".
    #[must_use]
    pub fn child_xids(&self) -> Vec<Xid> {
        let mut root: Xid = 0;
        let mut parent: Xid = 0;
        let mut children: *mut Xid = std::ptr::null_mut();
        let mut count: c_uint = 0;
        // SAFETY: valid out-pointers; on success `children` is an Xlib-owned
        // array of `count` XIDs that `XFree` releases below — the one
        // allocation rule for every Xlib out-array.
        let status = unsafe {
            (self.x11.query_tree)(
                self.x11.display,
                self.xid,
                &mut root,
                &mut parent,
                &mut children,
                &mut count,
            )
        };
        if status == 0 || children.is_null() || count == 0 {
            // A window with no children answers success with a null array —
            // `from_raw_parts` on null is UB even at length zero, so the
            // empty answer is returned before the slice is attempted. There
            // is also nothing to `XFree` in this branch.
            return Vec::new();
        }
        // SAFETY: `children` is non-null here, pointing at `count` contiguous
        // XIDs owned by Xlib; copying them out before the free is the
        // documented pattern.
        let ids = unsafe {
            let slice = std::slice::from_raw_parts(children, count as usize);
            slice.to_vec()
        };
        // SAFETY: releasing the Xlib-owned array exactly once.
        unsafe {
            (self.x11.free)(children.cast::<c_void>());
        }
        ids
    }
}

impl HasWindowHandle for X11HostWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: the handle is plain data around an XID this struct owns —
        // `XlibWindowHandle` carries no borrow or pointer, so the borrow the
        // returned `WindowHandle` implies cannot outlive anything. The XID
        // stays valid until `Drop`, and every user of the handle is gone
        // before then (the host is destroyed last).
        let handle = raw_window_handle::XlibWindowHandle::new(self.xid);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xlib(handle)) })
    }
}

impl Drop for X11HostWindow {
    fn drop(&mut self) {
        // SAFETY: the XID was created by `host_window` on this connection
        // and is destroyed exactly here — the struct is the single owner.
        unsafe {
            (self.x11.destroy_window)(self.x11.display, self.xid);
            (self.x11.flush)(self.x11.display);
        }
    }
}

/// The spec's `background` parameter as an X11 pixel: `"#rrggbb"` on a
/// TrueColor visual with standard masks (see the module doc's untested
/// notes for exactly how honest that is).
///
/// Absent means the mid-grey default — a view nobody gave a colour to is
/// still cleared to something, and mid-grey is visible against both the
/// white and the black hosts it might sit on. Present-but-malformed is an
/// `Err` carrying the offender rather than a silent fallback: an embedder
/// who wrote a colour and got a different one has been handed a bug they
/// cannot see.
fn background_pixel(spec: &PlatformViewSpec) -> Result<u32, String> {
    let Some(text) = spec.parameters.get("background") else {
        return Ok(0x80_80_80);
    };
    let hex = text.strip_prefix('#');
    if hex.is_none_or(|h| h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(text.clone());
    }
    // On the standard TrueColor visual the channel masks are 0xff0000 /
    // 0x00ff00 / 0x0000ff, so the pixel value *is* the hex triplet — red in
    // the high byte, blue in the low, no reordering. A visual whose masks
    // differ would need them read off the server, which is the module doc's
    // declared assumption rather than something this does.
    u32::from_str_radix(hex.unwrap(), 16).map_err(|_| text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `"#rrggbb"` → the X11 pixel the server clears the window with, on a
    /// TrueColor visual: red in the high byte, blue in the low. Absent is
    /// the mid-grey default; malformed is the offender, not a guess.
    #[test]
    fn the_background_parameter_becomes_an_x11_pixel() {
        let spec = PlatformViewSpec::new(KIND).with("background", "#2a7af6");
        assert_eq!(background_pixel(&spec), Ok(0x2a_7a_f6));
        // No parameter at all: the default, mid-grey.
        assert_eq!(
            background_pixel(&PlatformViewSpec::new(KIND)),
            Ok(0x80_80_80)
        );
        // Short, non-hex, and 8-digit are all refusals carrying the offender.
        assert_eq!(
            background_pixel(&PlatformViewSpec::new(KIND).with("background", "2a7af6")),
            Err("2a7af6".to_owned())
        );
        assert_eq!(
            background_pixel(&PlatformViewSpec::new(KIND).with("background", "#2a7af")),
            Err("#2a7af".to_owned())
        );
        assert_eq!(
            background_pixel(&PlatformViewSpec::new(KIND).with("background", "#2a7af6ff")),
            Err("#2a7af6ff".to_owned())
        );
        assert_eq!(
            background_pixel(&PlatformViewSpec::new(KIND).with("background", "#zzzzzz")),
            Err("#zzzzzz".to_owned())
        );
    }

    /// A display name that cannot exist proves the graceful path: no panic,
    /// a message naming what was missing, nothing opened.
    #[test]
    fn connecting_to_a_bogus_display_fails_cleanly() {
        match X11::connect(Some(":65000-no-such-display")) {
            Err(message) => {
                assert!(
                    message.contains("no X11 display"),
                    "said something useful: {message}"
                );
            }
            Ok(x11) => panic!(
                "connected to a display that cannot exist: {:?}",
                x11.display
            ),
        }
    }

    // ── The real seam, under a real X server ─────────────────────────────
    //
    // `skip_without!(display)` is this repo's loud-skip discipline (see
    // `tests/wait_loop.rs`'s header for the full argument): a container
    // without a display reports and returns, and
    // `VIEWW_REQUIRE_HARDWARE=display` turns a missing display into a panic
    // on a machine that was supposed to have one — Xvfb in CI, a laptop in
    // development. The probe checks that `DISPLAY` is set; connecting can
    // still fail on a stale one (the probe's own "necessary, not
    // sufficient" caveat), which is the second loud skip below. The rest of
    // this module's tests are pure logic and run everywhere.
    macro_rules! views_or_return {
        () => {
            match X11PlatformViews::connect() {
                Ok(views) => views,
                Err(reason) => {
                    eprintln!("skipping: {reason}");
                    return;
                }
            }
        };
    }

    /// Logical bounds at a fractional scale: the server's geometry is the
    /// device rectangle, and a scale change moves a live view with it.
    #[test]
    fn views_are_placed_in_device_pixels_at_the_hosts_scale() {
        vieww_hardware::skip_without!(display);
        let views = views_or_return!();
        let host = views.host_window(800, 600);
        views.anchor(&host).expect("anchored");
        views.set_scale(1.5);
        let spec = PlatformViewSpec::new(KIND);
        let handle = views
            .create(&spec, Rect::new(10.0, 20.0, 110.0, 70.0))
            .expect("created");
        let child = host.child_xids()[0];
        assert_eq!(host.geometry_of(child), Some((15, 30, 150, 75)), "×1.5");
        views.set_scale(2.0);
        assert_eq!(
            host.geometry_of(child),
            Some((20, 40, 200, 100)),
            "a scale change re-placed the live view"
        );
        drop(handle);
    }

    #[test]
    fn abutting_views_stay_abutting_at_a_fractional_scale() {
        // 1.25 × 0.5-pixel edges: separately scaled sizes would open a seam.
        let a = device_geometry(Rect::new(0.0, 0.0, 10.2, 5.0), 1.25);
        let b = device_geometry(Rect::new(10.2, 0.0, 20.4, 5.0), 1.25);
        assert_eq!(a.0 + a.2 as c_int, b.0, "{a:?} {b:?}");
    }

    /// The whole lifecycle, asserted against the X server itself rather than
    /// our own bookkeeping: create puts a real child of the host at the
    /// requested rectangle, set_bounds moves it, and dropping the handle
    /// destroys it.
    #[test]
    fn a_view_is_a_real_child_window_created_moved_and_destroyed() {
        vieww_hardware::skip_without!(display);
        let views = views_or_return!();
        let host = views.host_window(400, 300);
        views.anchor(&host).expect("an X11 host anchors");

        let spec = PlatformViewSpec::new(KIND).with("background", "#2a7af6");
        // The repo `Rect` is left/top/right/bottom; this one is the
        // 120×80 rectangle at (20, 40).
        let handle = views
            .create(&spec, Rect::new(20.0, 40.0, 140.0, 120.0))
            .expect("the factory is registered and the host is anchored");

        // The child exists, exactly where the bounds said, in the host's own
        // coordinate space (X11 geometry is parent-relative by definition —
        // which is the trait's "logical pixels from the top-left of the
        // window" at scale 1.0, verified rather than assumed).
        let child = host.child_xids();
        assert_eq!(child.len(), 1, "exactly one child of the host");
        assert_eq!(
            host.geometry_of(child[0]),
            Some((20, 40, 120, 80)),
            "the server's geometry is the requested bounds"
        );

        // Moving it: the server's answer again, not our cache.
        views
            .set_bounds(handle.id(), Rect::new(60.0, 15.0, 150.0, 60.0))
            .expect("a live view moves");
        views
            .set_bounds(handle.id(), Rect::new(60.0, 15.0, 150.0, 60.0))
            .expect("an unchanged rectangle is the cheap no-op path");
        assert_eq!(
            host.geometry_of(child[0]),
            Some((60, 15, 90, 45)),
            "set_bounds reached the server"
        );

        // Dropping the handle destroys the view — the trait's disposal
        // contract, proven by the server's own absence.
        drop(handle);
        assert!(
            !host.child_xids().contains(&child[0]),
            "the child window is gone after the handle"
        );
    }

    /// What the service says about itself: the one registered kind, the
    /// refusals for everything else, and no message channel.
    #[test]
    fn the_service_answers_honestly_about_what_it_offers() {
        vieww_hardware::skip_without!(display);
        let views = views_or_return!();
        let host = views.host_window(200, 100);
        views.anchor(&host).expect("an X11 host anchors");

        assert_eq!(views.registered(), vec![KIND.to_owned()]);

        match views.create(
            &PlatformViewSpec::new("webview"),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        ) {
            Err(ServiceError::Failed(message)) => {
                assert!(
                    message.contains("webview"),
                    "names the refused kind: {message}"
                );
            }
            other => panic!("an unregistered kind must be refused, got {other:?}"),
        }

        let handle = views
            .create(
                &PlatformViewSpec::new(KIND),
                Rect::new(0.0, 0.0, 10.0, 10.0),
            )
            .expect("the default background is the mid-grey fallback");
        assert!(matches!(
            views.send(handle.id(), "hello"),
            Err(ServiceError::Unsupported { .. })
        ));
    }

    /// Anchoring is required, and the refusal says what to do about it —
    /// the anchor gap this first consumer exists to name.
    #[test]
    fn creating_before_an_anchor_refuses_with_the_fix() {
        vieww_hardware::skip_without!(display);
        let views = views_or_return!();
        match views.create(
            &PlatformViewSpec::new(KIND),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        ) {
            Err(ServiceError::Failed(message)) => {
                assert!(message.contains("anchor()"), "says what to do: {message}");
            }
            other => panic!("creating without an anchor must refuse, got {other:?}"),
        }
        // And anchoring is idempotent for new views: a second anchor keeps
        // the service usable rather than poisoning it.
        let host = views.host_window(100, 100);
        views.anchor(&host).expect("first anchor");
        views.anchor(&host).expect("re-anchor keeps working");
        assert!(views
            .create(&PlatformViewSpec::new(KIND), Rect::new(0.0, 0.0, 5.0, 5.0))
            .is_ok());
    }

    /// Two views of one host are independent windows, and destroying one
    /// leaves the other alone — the dispose path must not be a
    /// clear-the-whole-map operation.
    #[test]
    fn two_views_are_two_independent_windows() {
        vieww_hardware::skip_without!(display);
        let views = views_or_return!();
        let host = views.host_window(300, 200);
        views.anchor(&host).expect("an X11 host anchors");

        let first = views
            .create(
                &PlatformViewSpec::new(KIND),
                Rect::new(0.0, 0.0, 50.0, 50.0),
            )
            .expect("first view");
        let second = views
            .create(
                &PlatformViewSpec::new(KIND),
                Rect::new(60.0, 0.0, 110.0, 50.0),
            )
            .expect("second view");
        assert_ne!(first.id(), second.id(), "ids are minted, not reused");

        assert_eq!(host.child_xids().len(), 2);
        drop(first);
        let remaining = host.child_xids();
        assert_eq!(remaining.len(), 1, "exactly the dropped one went");
        assert!(
            host.geometry_of(remaining[0]).is_some(),
            "the surviving view still exists"
        );
    }
}
