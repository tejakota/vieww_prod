//! A view the OS draws, placed by layout (FW-20's widget-tree integration).
//!
//! [`PlatformView`] reserves a rectangle in the widget tree the way any leaf
//! does — it takes the constraints' biggest size, so a `SizedBox` or a
//! `Flexible` around it decides how large it is — and its
//! [`PlatformViewController`] keeps a native view from
//! [`PlatformViews`](vieww_foundation::capability::PlatformViews) at that
//! rectangle: created on the first frame the widget is laid out, moved with
//! [`set_bounds`](vieww_foundation::capability::PlatformViews::set_bounds)
//! whenever layout moves it (an unchanged rectangle costs nothing), and
//! destroyed when the widget leaves the tree.
//!
//! The controller, not the widget, owns the native view, for the reason a
//! `ScrollController` owns a scroll offset: widgets are rebuilt every frame
//! something changes, and a native view recreated on every rebuild would
//! flicker and lose its state. Hold the controller in application state and
//! hand the same one to each rebuild.
//!
//! # What the OS view does not take part in
//!
//! The trait's own limits, restated where they bite: a native view is
//! composited by the OS *above* vieww's surface, so it is not clipped by an
//! ancestor's clip, not faded by an ancestor's opacity, and not covered by
//! vieww content drawn later. A scroll view moving one moves it (layout
//! places it), but does not hide the part scrolled out of the viewport.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use vieww_foundation::capability::{PlatformViewHandle, PlatformViewSpec, PlatformViews};
use vieww_foundation::{Key, Rect, ServiceError};

use crate::{widget_node_from, Widget, WidgetKind};

struct Inner {
    service: Rc<dyn PlatformViews>,
    spec: PlatformViewSpec,
    handle: RefCell<Option<PlatformViewHandle>>,
    bounds: Cell<Option<Rect>>,
    error: RefCell<Option<ServiceError>>,
    /// Render objects currently holding this controller. A rebuild makes the
    /// new one before dropping the old, so the count only reaches zero when
    /// the widget has really left the tree — the moment the view goes.
    mounted: Cell<usize>,
}

/// The native view behind a [`PlatformView`]: which service makes it, what
/// kind it is, and — once laid out — the handle.
#[derive(Clone)]
pub struct PlatformViewController {
    inner: Rc<Inner>,
}

impl fmt::Debug for PlatformViewController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformViewController")
            .field("kind", &self.inner.spec.kind)
            .field("created", &self.inner.handle.borrow().is_some())
            .field("bounds", &self.inner.bounds.get())
            .finish_non_exhaustive()
    }
}

impl PartialEq for PlatformViewController {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl PlatformViewController {
    /// A view of `spec.kind` from `service` (the application's registered
    /// `dyn PlatformViews`). Nothing is created until the widget is laid out.
    #[must_use]
    pub fn new(service: Rc<dyn PlatformViews>, spec: PlatformViewSpec) -> Self {
        Self {
            inner: Rc::new(Inner {
                service,
                spec,
                handle: RefCell::new(None),
                bounds: Cell::new(None),
                error: RefCell::new(None),
                mounted: Cell::new(0),
            }),
        }
    }

    /// Whether the native view exists right now.
    #[must_use]
    pub fn is_created(&self) -> bool {
        self.inner.handle.borrow().is_some()
    }

    /// Where the view was last placed, in logical pixels from the window's
    /// top-left.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        self.inner.bounds.get()
    }

    /// Why the service refused to make or move the view, if it did — an
    /// `Unsupported` answer on a platform with no embedding is the common one.
    #[must_use]
    pub fn error(&self) -> Option<ServiceError> {
        self.inner.error.borrow().clone()
    }

    /// Send the view a message — the trait's escape hatch.
    ///
    /// # Errors
    ///
    /// [`ServiceError::Failed`] before the view exists; otherwise the
    /// service's answer.
    pub fn send(&self, message: &str) -> Result<String, ServiceError> {
        let handle = self.inner.handle.borrow();
        let id = handle
            .as_ref()
            .map(PlatformViewHandle::id)
            .ok_or_else(|| ServiceError::failed("the platform view has not been laid out yet"))?;
        self.inner.service.send(id, message)
    }

    /// Layout placed the widget at `bounds`: create the view the first time,
    /// move it when the rectangle changed, nothing otherwise. Called by the
    /// render object; not something an application calls.
    pub fn place(&self, bounds: Rect) {
        if self.inner.bounds.get() == Some(bounds) && self.is_created() {
            return;
        }
        let created = self.is_created();
        let result = if created {
            let id = self
                .inner
                .handle
                .borrow()
                .as_ref()
                .map(PlatformViewHandle::id)
                .expect("created");
            self.inner.service.set_bounds(id, bounds)
        } else {
            self.inner
                .service
                .create(&self.inner.spec, bounds)
                .map(|handle| {
                    *self.inner.handle.borrow_mut() = Some(handle);
                })
        };
        match result {
            Ok(()) => {
                self.inner.bounds.set(Some(bounds));
                *self.inner.error.borrow_mut() = None;
            }
            Err(error) => *self.inner.error.borrow_mut() = Some(error),
        }
    }

    /// A render object took this controller. Paired with [`Self::unmounted`].
    pub fn mounted(&self) {
        self.inner.mounted.set(self.inner.mounted.get() + 1);
    }

    /// A render object let go of it. The last one destroys the native view
    /// (dropping the handle disposes it, by the trait's contract).
    pub fn unmounted(&self) {
        let left = self.inner.mounted.get().saturating_sub(1);
        self.inner.mounted.set(left);
        if left == 0 {
            self.inner.handle.borrow_mut().take();
            self.inner.bounds.set(None);
        }
    }
}

/// A rectangle of the layout handed to a native view. See the module docs.
#[derive(Debug)]
pub struct PlatformView {
    controller: PlatformViewController,
    key: Option<Key>,
}

impl PlatformView {
    #[must_use]
    pub const fn new(controller: PlatformViewController) -> Self {
        Self {
            controller,
            key: None,
        }
    }

    /// Set the reconciliation key.
    #[must_use]
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// The controller the render object places.
    #[must_use]
    pub const fn controller(&self) -> &PlatformViewController {
        &self.controller
    }
}

impl Widget for PlatformView {
    fn debug_name(&self) -> &'static str {
        "PlatformView"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::RenderLeaf
    }

    fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    fn debug_properties(&self) -> Vec<(&'static str, String)> {
        vec![("kind", self.controller.inner.spec.kind.clone())]
    }
}

widget_node_from!(PlatformView);
