//! The AUv3 view story, stubbed to the host-neutral
//! [`gui`](crate::gui) abstraction.
//!
//! AUv3 hosts ask for a `UIViewController` —
//! `requestViewControllerWithCompletionHandler:` — which on Apple
//! platforms means UIKit/AppKit, window systems this crate deliberately
//! links none of (the same refusal the CLAP `gui` wiring makes, for the
//! same reason). What the AUv3 wrapper completes the handler with is
//! `NULL`, by name, and what it offers instead is what every format
//! here offers: the offscreen [`Panel`](crate::gui::vieww::Panel), whose
//! frames a presenter embeds.
//!
//! [`Auv3View`] is that wiring: it owns the panel, attaches it to a
//! [`Parent::Offscreen`](crate::gui::Parent::Offscreen) with a
//! [`QueueHost`] — the [`GuiHost`](crate::gui::GuiHost) whose edits go
//! into a queue the AU's main thread drains into the parameter tree's
//! path — and hands out frames. A presenter crate (AppKit/UIKit glue in
//! the platform layer, the same follow-up the CLAP wiring names) is what
//! turns `frame()` into a view controller's contents; until then this
//! is the whole honest story: the GUI exists, renders deterministically
//! offscreen, is driven by parameter notifications and pointer input,
//! and is never a window.

use std::sync::Mutex;

use crate::gui::vieww::Panel;
use crate::gui::{Frame, GuiHost, Parent, PluginGui};

/// The queue depth before panel edits are coalesced away.
const MAX_QUEUED_EDITS: usize = 256;

/// One edit the panel raised: a parameter and its plain value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edit {
    /// The parameter the user moved.
    pub id: u32,
    /// Its new plain value.
    pub value: f64,
}

/// The [`GuiHost`] the panel edits flow through: edits queue here (the
/// panel's thread and the AU's main thread are the same by the GUI
/// contract, but the queue keeps the panel usable from a headless test
/// driving it directly).
///
/// The queue is bounded: `MAX_QUEUED_EDITS` edits, then the newest
/// value *for the same parameter* replaces the queued one — a host
/// draining the queue cannot fall behind a spinning knob.
#[derive(Debug, Default)]
pub struct QueueHost {
    queue: Mutex<Vec<Edit>>,
}

impl QueueHost {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take every queued edit, in order.
    pub fn take(&self) -> Vec<Edit> {
        self.queue
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }

    fn push(&self, e: Edit) {
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_QUEUED_EDITS {
                // Coalesce: the latest value for a parameter wins.
                if let Some(slot) = q.iter_mut().find(|s| s.id == e.id) {
                    *slot = e;
                    return;
                }
                q.remove(0);
            }
            q.push(e);
        }
    }
}

impl GuiHost for QueueHost {
    fn set_param(&self, id: u32, value: f64) {
        self.push(Edit { id, value });
    }

    fn request_resize(&self, w: u32, h: u32) -> bool {
        // The panel never resizes itself (its height follows its
        // parameter count), so this is only ever the host's own idea
        // coming back at it. Accept by not lying.
        let _ = (w, h);
        true
    }

    fn closed(&self) {}
}

/// The offscreen panel plus its edit queue: the AUv3 view stub.
///
/// Created when the audio unit initialises (behind the `gui` feature),
/// driven by [`Auv3View::param_notify`] when parameters change and
/// [`Auv3View::pointer`] from whatever presenter is built; the frames
/// come out through [`Auv3View::frame`].
#[derive(Debug, Default)]
pub struct Auv3View {
    host: std::sync::Arc<QueueHost>,
    panel: Option<Panel>,
}

impl Auv3View {
    /// A view with no panel yet (the AU exists before its parameters
    /// are known).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the panel for a parameter table (idempotent: a second
    /// call re-attaches, as a host re-opening an editor would).
    ///
    /// The panel gets an [`Arc`] clone of the host — the box the trait
    /// takes is `'static` by signature, and the Arc is the shared
    /// queue, not a borrow of this view.
    pub fn attach(&mut self, params: &[crate::ParamInfo]) -> bool {
        let mut panel = Panel::new(params);
        if panel.attach(Box::new(self.host.clone()), Parent::Offscreen) {
            self.panel = Some(panel);
            true
        } else {
            false
        }
    }

    /// The host whose edits the AU's main thread drains.
    #[must_use]
    pub fn edits(&self) -> &std::sync::Arc<QueueHost> {
        &self.host
    }

    /// A parameter changed (automation, a preset load): repaint.
    pub fn param_notify(&mut self, id: u32, value: f64) {
        if let Some(p) = self.panel.as_mut() {
            p.param_notify(id, value);
        }
    }

    /// Input, in GUI coordinates — what a presenter forwards from its
    /// view controller's touch handling.
    pub fn pointer(&mut self, phase: crate::gui::Pointer) {
        if let Some(p) = self.panel.as_mut() {
            p.pointer(phase);
        }
    }

    /// The current frame (RGBA8, top-left first).
    pub fn frame(&mut self) -> Option<Frame<'_>> {
        self.panel.as_mut().map(PluginGui::frame)
    }

    /// Whether the panel is attached.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.panel.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::Pointer;
    use crate::ParamInfo;

    fn params() -> Vec<ParamInfo> {
        vec![
            ParamInfo::new(0, "Gain", -48.0, 24.0, 0.0).unit("dB"),
            ParamInfo::new(1, "Drive", 0.0, 1.0, 0.0),
        ]
    }

    #[test]
    fn the_panel_attaches_offscreen_and_renders_deterministically() {
        let mut v = Auv3View::new();
        assert!(!v.is_attached());
        assert!(v.attach(&params()));
        assert!(v.is_attached());
        let a = v.frame().expect("a frame").to_vec();
        let b = v.frame().expect("a frame").to_vec();
        assert_eq!(a, b, "byte-identical frames for identical state");
    }

    #[test]
    fn a_drag_round_trips_into_the_edit_queue() {
        let mut v = Auv3View::new();
        assert!(v.attach(&params()));
        // Where is the Gain control? Take the frame and drag through its
        // track: the panel lays one control per parameter, stacked.
        let (w, _) = {
            let f = v.frame().expect("a frame");
            (f.width, f.height)
        };
        // Row 0 spans y = HEADER + PAD .. + ROW = 42..68 (the panel's
        // own geometry: 30 + 12, +26).
        let row = (w as f32 / 2.0, 50.0);
        v.pointer(Pointer::Down(row.0, row.1));
        v.pointer(Pointer::Move(row.0 + 60.0, row.1));
        v.pointer(Pointer::Up(row.0 + 60.0, row.1));
        let edits = v.host.take();
        assert!(
            !edits.is_empty(),
            "a drag through a control reaches the GuiHost"
        );
        for e in &edits {
            assert_eq!(e.id, 0, "the top control is Gain");
            assert!((-48.0..=24.0).contains(&e.value), "clamped: {}", e.value);
        }
        // Param changes repaint. The drag above over-shot the track, so
        // Gain already sits at its maximum (24.0) — a notify of the SAME
        // value must not repaint, and a notify of a DIFFERENT one must.
        let before = v.frame().expect("a frame").to_vec();
        v.param_notify(0, 24.0);
        let same = v.frame().expect("a frame").to_vec();
        assert_eq!(before, same, "a redundant notify does not repaint");
        v.param_notify(0, -12.0);
        let after = v.frame().expect("a frame").to_vec();
        assert_ne!(before, after, "the readout moves with the value");
    }

    #[test]
    fn the_queue_coalesces_at_capacity() {
        let h = QueueHost::new();
        for i in 0..MAX_QUEUED_EDITS {
            h.set_param(0, f64::from(u32::try_from(i).unwrap_or(0)));
        }
        h.set_param(0, 99.0);
        let q = h.take();
        assert_eq!(q.len(), MAX_QUEUED_EDITS, "coalesced, not grown");
        // The newest value for the parameter replaces the OLDEST queued
        // one — in place, at its position — so a draining host cannot
        // fall behind a spinning knob.
        assert_eq!(q.first().copied(), Some(Edit { id: 0, value: 99.0 }));
        assert_eq!(q.last().copied(), Some(Edit { id: 0, value: 255.0 }));
        assert_eq!(h.take().len(), 0, "take empties");
    }
}
