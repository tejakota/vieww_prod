//! Outbound drag-and-drop: this platform's half of a seam that starts in
//! [`vieww_interaction::drag_out`].
//!
//! # What exists and what honestly does not
//!
//! `vieww_interaction::drag_out` owns the portable half — the
//! [`OutgoingDrag`] payload (the data-provider API: one drag offering text,
//! URIs, files and custom MIME forms, plus which effects the source allows),
//! the [`DragSession`] state machine, and the `CF_HDROP` byte layout every
//! Windows drop target reads. What it could not ship was a *starter*:
//! `winit` 0.30 has no `start_drag` on any backend (the windowing issue this
//! seam tracks, rust-windowing/winit#1550), so its only implementation was
//! [`NullDragStarter`], which refuses.
//!
//! The refusal is still the whole answer on four of five targets — but the
//! Windows bridge exists, type-checked against the real `windows` 0.58 crate:
//! OLE `DoDragDrop` through the shell's own data object and a hand-written
//! `IDropSource`. This module is where that bridge meets this crate's
//! services, which is the piece FW-19 named: **wired into the adapter**, so
//! a widget three levels deep in a tree can start an OS drag without knowing
//! which platform it is on.
//!
//! # The Windows half is type-checked, never run
//!
//! Carried over verbatim from `drag_out_windows.rs`'s own status header: the
//! parts most likely to be wrong on a first real run are marked
//! **[session-verifies]** there — `SHCreateDataObject` accepting `SetData`
//! with `CF_HDROP`, OLE being initialised on the calling thread, and the
//! blocking modal loop being acceptable on the event-loop thread (which is
//! how every native Windows application runs one). Until a Windows session
//! runs the drag-file-to-Explorer list, treat the starter as compiled, not
//! proven. The pure half — `CF_HDROP` bytes, effect-bitmask mapping — is
//! unit-tested on every machine in `vieww-interaction`.
//!
//! # Registration rather than absence
//!
//! [`provide`] registers the starter in [`services::platform`](crate::services)
//! on *every* target, including the ones whose starter refuses. That is a
//! different choice from the one [`crate::services::unavailable_capabilities`]
//! makes for the camera and its kin, and the difference is deliberate: those
//! capabilities have no portable state machine, so "not registered" is a
//! complete answer. A drag-out does have one — [`DragSession`] needs *some*
//! starter to run its already-dragging and finish logic at all, and a
//! `Services::get` that answers `None` cannot distinguish "this platform
//! cannot drag out" from "this build wired no services". Registered-and-
//! refusing keeps the distinction in [`DragError::NotSupportedByPlatform`],
//! where an application can match on it.

use std::rc::Rc;

use vieww_foundation::Services;
pub use vieww_interaction::drag_out::{
    DragContent, DragError, DragSession, DragSessionState, DropEffect, NullDragStarter,
    OutgoingDrag, PlatformDragStarter,
};

/// The drag starter this platform provides: the OLE `DoDragDrop` bridge on
/// Windows, an honest refusal everywhere else.
///
/// Constructed by [`services::platform`](crate::services::platform) and
/// registered as `dyn PlatformDragStarter`, so this is rarely named
/// directly — an application pulls it out of [`Services`] instead, and a
/// widget gets it the way it gets any service, through
/// `Inherited<SharedServices>`.
///
/// `Default` rather than `new` only because the Windows half's `new` is
/// `const fn` and `Default` is what the lint asks for; both do the same
/// thing.
#[derive(Debug, Default)]
pub struct OsDragStarter {
    #[cfg(target_os = "windows")]
    inner: vieww_interaction::drag_out_windows::WindowsDragStarter,
}

impl OsDragStarter {
    /// What the last drag this starter ran ended with — `None` before any
    /// drag, after a cancelled one, and on every non-Windows target (where no
    /// drag can run at all).
    ///
    /// `DoDragDrop` reports its outcome as an out-parameter rather than a
    /// return value, which is why the starter *remembers* it: the caller that
    /// needs "copy or move?" after a finished drag asks here rather than
    /// re-deriving it from anything.
    #[must_use]
    pub fn last_effect(&self) -> Option<DropEffect> {
        #[cfg(target_os = "windows")]
        {
            self.inner.last_effect()
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }
}

impl PlatformDragStarter for OsDragStarter {
    fn start_drag(&self, drag: &OutgoingDrag) -> Result<(), DragError> {
        #[cfg(target_os = "windows")]
        {
            self.inner.start_drag(drag)
        }
        // The honest answer on macOS, Linux, Android and iOS: winit 0.30
        // exposes no way to start an OS drag on any of them (see the module
        // doc). A plausible-looking success here would be a lie a widget
        // could not detect.
        #[cfg(not(target_os = "windows"))]
        {
            vieww_interaction::drag_out::NullDragStarter.start_drag(drag)
        }
    }
}

/// Register [`OsDragStarter`] as the platform's `dyn PlatformDragStarter`.
///
/// Called by [`services::platform`](crate::services::platform) (and its
/// Android counterpart) on every target — see the module doc for why this
/// capability is registered-and-refusing rather than left absent.
pub fn provide(services: &mut Services) {
    services.provide::<dyn PlatformDragStarter>(Rc::new(OsDragStarter::default()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registered starter is *present* on every target — an application
    /// that checks `Services::has` learns "drag-out exists as a seam", and
    /// the refusal lives in the call, not the lookup.
    #[test]
    fn the_platform_registers_a_drag_starter_on_every_target() {
        let mut services = Services::new();
        provide(&mut services);
        assert!(services.has::<dyn PlatformDragStarter>());
        assert!(services.get::<dyn PlatformDragStarter>().is_some());
    }

    /// On the four targets with no OS bridge, the starter refuses honestly
    /// and the session machinery survives it: a failed start leaves the
    /// session idle rather than wedged mid-drag, which is the contract
    /// `DragSession::start` documents and the one a widget depends on.
    ///
    /// The session is driven through a concrete starter because `start` takes
    /// `&impl PlatformDragStarter` (a `Sized` starter — its own tests in
    /// `vieww-interaction` pass concrete ones), while the service-registered
    /// `Rc<dyn>` a widget actually holds is exercised directly underneath.
    ///
    /// On Windows this test would attempt a real `DoDragDrop` modal loop, so
    /// the assertions are gated to the refusing targets — the Windows half's
    /// proof is a type-check plus the session-verifies list, not this suite.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn without_an_os_bridge_the_starter_refuses_and_the_session_survives() {
        let mut services = Services::new();
        provide(&mut services);
        let starter = services
            .get::<dyn PlatformDragStarter>()
            .expect("registered above");

        // The widget's path: the starter it pulled out of `Services` refuses
        // the drag itself, honestly.
        let drag = OutgoingDrag::single(DragContent::Text("hello".into()), DropEffect::Copy);
        assert_eq!(
            starter.start_drag(&drag),
            Err(DragError::NotSupportedByPlatform)
        );

        // And the session machinery survives the refusal: a failed start
        // leaves the session idle rather than wedged mid-drag.
        let mut session = DragSession::new();
        let result = session.start(drag, &OsDragStarter::default());
        assert_eq!(result, Err(DragError::NotSupportedByPlatform));
        assert!(
            session.is_idle(),
            "a refused drag must not leave the session mid-drag"
        );
        // And the refused starter has no effect to report — the honest
        // `None`, not a fabricated one.
        let os_starter = OsDragStarter::default();
        assert_eq!(os_starter.last_effect(), None);
    }
}
