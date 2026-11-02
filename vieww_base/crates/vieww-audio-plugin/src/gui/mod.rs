//! Plugin GUIs: one host-neutral interface, and a first implementation that
//! renders through vieww's own rasterizer, offscreen.
//!
//! **Why an abstraction at all.** Every plugin format has a GUI story and
//! none of them agree: CLAP has the `clap.gui` extension (a `clap_window`
//! with a win32/cocoa/x11 handle), VST3 has `IPlugView` (a `ViewRect` and
//! `attached`), LV2 has `ui` TTL data (a binary per toolkit), AUv3 has a
//! view controller the host instantiates. A plugin that wants an editor on
//! more than one host currently reimplements it per format — which is why
//! so many Linux LV2s are GTK while their VST3 twins are Qt.
//!
//! The interface here is deliberately the intersection of those four:
//!
//! * [`PluginGui::attach`] — a host hands over itself (the
//!   [`GuiHost`], the plugin-side channel for edits and resize requests)
//!   and a [`Parent`]: a native window handle, or [`Parent::Offscreen`],
//!   where the host reads frames itself.
//! * [`PluginGui::resize`] / [`PluginGui::size`] — hosts resize editors
//!   (VST3 `onSize`, CLAP `set_size`) and ask what size is wanted first
//!   (`get_size`, `can_resize`).
//! * [`PluginGui::param_notify`] — the host tells the GUI a parameter
//!   changed (automation, a preset load, another control), the GUI repaints.
//! * [`PluginGui::frame`] — the current offscreen frame; for a windowed
//!   GUI this is what would be blitted.
//! * [`PluginGui::pointer`] — input, in GUI coordinates. A GUI that cannot
//!   be driven by tests is a picture, not a component: this is what makes
//!   the round trip (drag a control → [`GuiHost::set_param`] → plugin →
//!   [`PluginGui::param_notify`] → repaint) testable headlessly.
//! * [`PluginGui::close`] — the host is done; the GUI tells the host so
//!   (CLAP `closed`, VST3 `notified`) and forgets it. Dropping the GUI
//!   does the same without the notification, like every other Rust
//!   resource.
//!
//! **The first implementation** ([`vieww::Panel`]) renders with
//! `vieww-paint`'s `NativeRenderer` — this workspace's one CPU rasterizer,
//! the same `Scene` every vieww widget draws — into an RGBA buffer the
//! host owns. No OS windowing: it answers [`Parent::Offscreen`] only and
//! refuses a native handle by name (the plugin crate deliberately links
//! no window system). That is an honest half of the story: a CLAP host
//! wanting an embedded editor needs the surface *presented* — copied into
//! the parent window — which is the platform crates' job, not this
//! crate's. The `clap.gui` wiring (see [`crate::clap`]) exposes the
//! size/scale/show/hide lifecycle and the offscreen frames; a presenter
//! glue crate is the follow-up.
//!
//! Text: the renderer draws geometry only — tracks, fills, and 7-segment
//! value readouts drawn from rectangles. Glyph runs need font bytes, and a
//! plugin binary should not embed a font (licensing, size, and taste) any
//! more than it should link a toolkit; parameter *names* stay the host's
//! job. The readout is what automation needs to see: the digits move when
//! the value does.
//!
//! **Determinism.** Same parameter table, same values, same size, same
//! scale → byte-identical frame. The scene is built from the same f32
//! geometry every time and the rasterizer is this workspace's own CPU
//! reference implementation, so a test can hash a frame and a host can
//! diff frames to decide what to present. The tests pin that.

pub mod vieww;

use std::ffi::c_void;

/// A host that shares its [`GuiHost`] across handles (an `Arc` it hands
/// to each attach) forwards through this impl: every method is `&self`,
/// so the share is the whole story.
impl<T: GuiHost + ?Sized> GuiHost for std::sync::Arc<T> {
    fn set_param(&self, id: u32, value: f64) {
        (**self).set_param(id, value);
    }
    fn request_resize(&self, w: u32, h: u32) -> bool {
        (**self).request_resize(w, h)
    }
    fn closed(&self) {
        (**self).closed();
    }
}

/// Where a host wants the GUI drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parent {
    /// The host reads [`PluginGui::frame`] itself — a headless test, a
    /// screenshot generator, or a host that presents the bytes (a canvas,
    /// a texture, a remote desktop).
    Offscreen,
    /// A native window/view handle: an X11 `Window`, a Win32 `HWND`, a
    /// Cocoa `NSView *` — the same union CLAP's `clap_window` carries.
    /// Given as the raw pointer the format handed over; which one it is,
    /// is the format wrapper's knowledge.
    Native(*mut c_void),
}

impl Parent {
    /// Whether this is a native handle (and which, if the caller knows).
    #[must_use]
    pub const fn native(&self) -> Option<*mut c_void> {
        match *self {
            Self::Native(p) => Some(p),
            Self::Offscreen => None,
        }
    }
}

/// What the host side of a GUI must answer: the plugin-side of the GUI's
/// own actions.
///
/// Every format's host has these: CLAP `clap_host_gui`, VST3
/// `IComponentHandler` (`performEdit`/`setSize`), LV2's ports, AUv3's
/// observer closures.
pub trait GuiHost {
    /// The user moved a control: set parameter `id` to `value` (plain,
    /// as the parameter table stores it, already taper-adjusted by the
    /// GUI).
    fn set_param(&self, id: u32, value: f64);

    /// The GUI would like to be `w × h`. The host answers whether that
    /// happened; if it did, it calls [`PluginGui::resize`].
    fn request_resize(&self, w: u32, h: u32) -> bool;

    /// The GUI was closed by the user (its own close affordance). The
    /// host hides it; [`PluginGui::close`] is the host-initiated path.
    fn closed(&self);
}

/// A plugin's editor, host-neutral.
///
/// The lifecycle every format expects:
///
/// ```text
/// create → attach(host, parent) → [resize ⇄ request_resize]*
///         → [param_notify → frame]* → close
/// ```
///
/// All methods are main-thread (every format documents its GUI as
/// main-thread-only; CLAP and VST3 say so explicitly), which is why
/// nothing here needs a lock the audio thread would care about.
pub trait PluginGui {
    /// The size the GUI wants next frame, `(w, h)` in logical pixels.
    fn size(&self) -> (u32, u32);

    /// Accept a host's DPI scale (CLAP `set_scale`). `false` if this GUI
    /// does not scale (the rasterizer's pixels are then host-managed).
    fn set_scale(&mut self, scale: f64) -> bool;

    /// Attach to a host's parent. `false` — without side effects — if the
    /// parent kind is not supported.
    fn attach(&mut self, host: Box<dyn GuiHost + Send>, parent: Parent) -> bool;

    /// Resize to exactly `w × h` (a host-driven resize; CLAP `set_size` /
    /// VST3 `onSize`). `false` if the size is refused — the GUI keeps its
    /// old one.
    fn resize(&mut self, w: u32, h: u32) -> bool;

    /// A parameter changed (host → GUI): automation moved it, a preset
    /// was loaded, another control claimed it. The next
    /// [`frame`](Self::frame) shows it. Unknown ids are ignored — a host
    /// may broadcast.
    fn param_notify(&mut self, id: u32, value: f64);

    /// Input, in GUI coordinates (`(x, y)` from the top-left, logical
    /// pixels). A `Down`/`Move` inside a control edits it (through
    /// [`GuiHost::set_param`]); `Up` ends the edit.
    fn pointer(&mut self, phase: Pointer);

    /// The current frame: `(w, h)` and `w * h * 4` bytes of RGBA, in that
    /// order, starting at the top-left. Valid until the next call on this
    /// GUI.
    ///
    /// This is the whole offscreen contract: it is what a presenter
    /// copies into a window, what a test hashes, and what a remote host
    /// streams.
    fn frame(&mut self) -> Frame<'_>;

    /// Make the GUI visible (CLAP `show`; VST3 has no call — attaching is
    /// showing). Offscreen GUIs mark themselves shown; the frame is the
    /// presentation either way.
    fn show(&mut self) -> bool;

    /// Hide without destroying (CLAP `hide`).
    fn hide(&mut self) -> bool;

    /// The host is done with this GUI: tell it
    /// ([`GuiHost::closed`](trait.GuiHost.html#tymethod.closed)) and
    /// forget it. Further [`frame`](Self::frame) calls render the same
    /// last frame; further input goes nowhere.
    fn close(&mut self);
}

/// One pointer event, in GUI coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    /// A press at `(x, y)`.
    Down(f32, f32),
    /// A drag (the button is still down from the last `Down`/`Move`).
    Move(f32, f32),
    /// The button came up at `(x, y)`.
    Up(f32, f32),
}

impl Pointer {
    /// The position this event carries.
    #[must_use]
    pub const fn position(&self) -> (f32, f32) {
        match *self {
            Self::Down(x, y) | Self::Move(x, y) | Self::Up(x, y) => (x, y),
        }
    }
}

/// An offscreen frame: RGBA8, row-major, top-left first.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: &'a [u8],
}

impl Frame<'_> {
    /// The pixel at `(x, y)` as `(r, g, b, a)`, or `None` outside.
    #[must_use]
    pub const fn pixel(&self, x: u32, y: u32) -> Option<(u8, u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y * self.width + x) as usize * 4;
        Some((
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ))
    }

    /// The frame's bytes, owned: what a test hashes and a presenter
    /// copies out.
    #[must_use]
    pub fn to_vec(&self) -> Vec<u8> {
        self.rgba.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents_report_what_they_carry() {
        assert!(Parent::Offscreen.native().is_none());
        let p = Parent::Native(0x1234 as *mut c_void);
        assert_eq!(p.native(), Some(0x1234 as *mut c_void));
    }

    #[test]
    fn pointer_positions_read_back() {
        assert_eq!(Pointer::Down(3.0, 4.0).position(), (3.0, 4.0));
        assert_eq!(Pointer::Move(1.0, 2.0).position(), (1.0, 2.0));
        assert_eq!(Pointer::Up(5.0, 6.0).position(), (5.0, 6.0));
    }

    #[test]
    fn frames_read_pixels_and_refuse_outside() {
        // A 2×1 image: red, then transparent blue.
        let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
        let f = Frame {
            width: 2,
            height: 1,
            rgba: &rgba,
        };
        assert_eq!(f.pixel(0, 0), Some((255, 0, 0, 255)));
        assert_eq!(f.pixel(1, 0), Some((0, 0, 255, 128)));
        assert_eq!(f.pixel(2, 0), None);
        assert_eq!(f.pixel(0, 1), None);
    }
}
