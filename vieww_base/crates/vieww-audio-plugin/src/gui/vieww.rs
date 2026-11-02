//! The first [`PluginGui`](super::PluginGui): a parameter panel rendered by
//! vieww itself, offscreen.
//!
//! Every row is a parameter: a rounded track, a fill at the parameter's
//! normalized position, and a 7-segment readout of the plain value — the
//! digits a screenshot can read, drawn from rectangles because a plugin
//! binary embeds no font. The scene is built with the same
//! [`Command`](vieww_paint::Command)s any vieww widget records and
//! rasterized by [`NativeRenderer`], this workspace's one CPU rasterizer:
//! no OS windowing, no GPU context, nothing a CI container cannot run.
//!
//! # What a host does with it
//!
//! ```ignore
//! use vieww_audio_plugin::gui::{vieww::Panel, GuiHost, Parent, Pointer, PluginGui};
//!
//! let mut gui = Panel::new(&plugin.param_info());
//! assert!(gui.attach(Box::new(host), Parent::Offscreen));
//! gui.param_notify(7, 0.5);                   // automation moved it
//! let frame = gui.frame();                    // RGBA8: present or hash it
//! gui.pointer(Pointer::Down(120.0, 46.0));    // the user grabbed a track
//! ```
//!
//! # Honesty
//!
//! * No window: [`Parent::Native`] is refused by name. Presenting the
//!   frame into a real parent view is the platform crates' job; this
//!   crate links no window system on purpose (see the module docs).
//! * No text: parameter names are the host's to draw (it has the font);
//!   the readout is geometry.
//! * Input is pointer-only: the intersection of what CLAP hosts forward
//!   and what a test can synthesize. Keyboard, if a GUI ever needs it,
//!   grows the trait then.
//! * `request_resize` is never called by this panel — its rows fit its
//!   height, so it has nothing to ask for. A host that wants a scrollbar
//!   answers its own questions; the trait carries the call for the GUIs
//!   that need it.

use vieww_paint::foundation::{Color, Offset, Path, Rect, Transform};
use vieww_paint::native::NativeRenderer;
use vieww_paint::{Clip, Command, Paint, Scene, Stroke};

use super::{Frame, GuiHost, Parent, PluginGui, Pointer};
use vieww_audio::preset::ParamInfo;
#[cfg(test)]
use vieww_audio::preset::Taper;

/// Panel chrome, in logical pixels.
mod layout {
    /// Space around everything.
    pub(super) const PAD: f32 = 12.0;
    /// The narrowest panel that still reads (the resize floor).
    pub(super) const MIN_W: u32 = 240;
    /// The title strip's height.
    pub(super) const HEADER: f32 = 30.0;
    /// One parameter row's height.
    pub(super) const ROW: f32 = 26.0;
    /// The value track's width and height.
    pub(super) const TRACK: (f32, f32) = (200.0, 8.0);
    /// Track corner radius.
    pub(super) const RADIUS: f32 = 4.0;
    /// One readout digit cell: `(w, h)`.
    pub(super) const CELL: (f32, f32) = (7.0, 13.0);
    /// Segment thickness within a cell.
    pub(super) const SEG: f32 = 2.0;
    /// Readout cells: five digit slots, then the point, then the tenth.
    pub(super) const CELLS: usize = 6;
    /// The last slot's narrow width (the decimal point's cell).
    pub(super) const DOT: f32 = 3.0;
}

/// Chrome colours, fixed: the palette *is* the determinism contract.
mod colors {
    use vieww_paint::foundation::Color;
    /// Panel background.
    pub(super) const BG: Color = Color::rgb(24, 27, 32);
    /// The title strip.
    pub(super) const HEADER: Color = Color::rgb(38, 43, 51);
    /// The strip's accent underline.
    pub(super) const ACCENT: Color = Color::rgb(77, 163, 255);
    /// An unfilled track.
    pub(super) const TRACK: Color = Color::rgb(52, 58, 67);
    /// A filled track.
    pub(super) const FILL: Color = Color::rgb(77, 163, 255);
    /// A lit readout segment.
    pub(super) const SEG_ON: Color = Color::rgb(232, 234, 237);
    /// An unlit one — visible, so digit shapes read at a glance.
    pub(super) const SEG_OFF: Color = Color::rgb(41, 46, 54);
    /// A row being edited.
    pub(super) const EDIT: Color = Color::rgb(96, 178, 255);
}

/// One parameter row.
#[derive(Debug, Clone)]
struct Row {
    info: ParamInfo,
    value: f64,
}

/// A parameter panel rendered by vieww's rasterizer.
///
/// Built from a [`ParamInfo`] table (a
/// [`Parametric`](vieww_audio::preset::Parametric)'s `param_info()`);
/// values start at each parameter's default.
pub struct Panel {
    rows: Vec<Row>,
    width: u32,
    height: u32,
    scale: f64,
    host: Option<Box<dyn GuiHost + Send>>,
    edit: Option<usize>,
    shown: bool,
    dirty: bool,
    renderer: NativeRenderer,
    frame: Vec<u8>,
}

impl std::fmt::Debug for Panel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panel")
            .field("rows", &self.rows)
            .field("size", &(self.width, self.height))
            .field("scale", &self.scale)
            .field("attached", &self.host.is_some())
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl Panel {
    /// A panel for `params`, sized to fit.
    ///
    /// # Panics
    /// If `params` is empty — a GUI with nothing to show is a caller bug,
    /// not a rendering case.
    #[must_use]
    pub fn new(params: &[ParamInfo]) -> Self {
        assert!(!params.is_empty(), "a panel needs parameters");
        let rows = params
            .iter()
            .cloned()
            .map(|info| Row {
                value: info.default,
                info,
            })
            .collect::<Vec<_>>();
        let height = Self::height_for(rows.len());
        let mut gui = Self {
            rows,
            width: 380,
            height,
            scale: 1.0,
            host: None,
            edit: None,
            shown: false,
            dirty: true,
            renderer: NativeRenderer::new(),
            frame: Vec::new(),
        };
        gui.reallocate();
        gui
    }

    /// The size [`resize`](PluginGui::resize) keeps for `w × h`: its
    /// minimums applied, nothing else — the panel never clips rows, so
    /// a smaller height than the rows need is refused upward.
    #[must_use]
    pub fn clamped_size(w: u32, h: u32) -> (u32, u32) {
        (w.max(layout::MIN_W), h.max(Self::height_for(2)))
    }

    /// The panel's natural height for `n` rows — its minimum: rows are
    /// never clipped, they are the reason for the size.
    #[must_use]
    pub fn height_for(n: usize) -> u32 {
        let h = layout::HEADER + layout::PAD + (n as f32) * layout::ROW + layout::PAD;
        h.max(layout::HEADER + 2.0 * layout::PAD + layout::ROW) as u32
    }

    /// The parameter ids and current values the panel shows.
    #[must_use]
    pub fn values(&self) -> Vec<(u32, f64)> {
        self.rows.iter().map(|r| (r.info.id, r.value)).collect()
    }

    /// The frame's pixel dimensions: the logical size times the scale.
    /// This is the size a host presents — `get_size`'s answer under the
    /// offscreen convention (logical geometry, physical backing store).
    #[must_use]
    pub fn pixels(&self) -> (u32, u32) {
        self.pixel_size()
    }

    fn pixel_size(&self) -> (u32, u32) {
        let s = self.scale;
        #[expect(clippy::cast_possible_truncation, reason = "a ceil'd scale")]
        let px = |v: u32| (f64::from(v) * s).ceil().max(1.0) as u32;
        (px(self.width), px(self.height))
    }

    fn reallocate(&mut self) {
        let (w, h) = self.pixel_size();
        let bytes = (w as usize) * (h as usize) * 4;
        self.frame.resize(bytes, 0);
    }

    /// Row `i`'s rectangle (the hit target) and its value track, in
    /// logical coordinates.
    fn track(&self, i: usize) -> (Rect, Rect) {
        let (_, th) = layout::TRACK;
        let top = layout::HEADER + layout::PAD + row_h(i);
        let x = layout::PAD;
        (
            Rect::new(x, top, self.width as f32 - layout::PAD, top + layout::ROW),
            Rect::new(
                x,
                top + (layout::ROW - th) * 0.5,
                x + layout::TRACK.0,
                top + (layout::ROW + th) * 0.5,
            ),
        )
    }

    /// The readout block's origin for row `i` (right-aligned).
    fn readout(&self, i: usize) -> Offset {
        let (_, ch) = layout::CELL;
        let block_w = (layout::CELLS as f32 - 1.0) * layout::CELL.0 + layout::DOT;
        Offset::new(
            self.width as f32 - layout::PAD - block_w,
            layout::HEADER + layout::PAD + row_h(i) + (layout::ROW - ch) * 0.5,
        )
    }

    /// Where in `info`'s range a plain value sits (0..1; 0.0 for a
    /// degenerate range — the parameter table's own convention).
    fn normalized(info: &ParamInfo, v: f64) -> f64 {
        info.normalize(v)
    }

    /// The plain value at normalized position `n` in `info`'s track
    /// (clamped, snapped when stepped — the table's own convention).
    fn plain(info: &ParamInfo, n: f64) -> f64 {
        info.denormalize(n)
    }

    /// Build this frame's scene, in logical coordinates.
    fn scene(&self) -> Scene {
        let mut scene = Scene::new();
        let w = self.width as f32;
        let h = self.height as f32;
        solid(&mut scene, Rect::new(0.0, 0.0, w, h), colors::BG);
        solid(
            &mut scene,
            Rect::new(0.0, 0.0, w, layout::HEADER),
            colors::HEADER,
        );
        solid(
            &mut scene,
            Rect::new(0.0, layout::HEADER - 2.0, w, layout::HEADER),
            colors::ACCENT,
        );

        for (i, row) in self.rows.iter().enumerate() {
            let (_, track) = self.track(i);
            rrect(&mut scene, track, layout::RADIUS, colors::TRACK);
            let n = Self::normalized(&row.info, row.value);
            let fill_w = track.width() * (n as f32);
            if fill_w > 2.0 * layout::RADIUS {
                rrect(
                    &mut scene,
                    Rect::new(track.left, track.top, track.left + fill_w, track.bottom),
                    layout::RADIUS,
                    if self.edit == Some(i) {
                        colors::EDIT
                    } else {
                        colors::FILL
                    },
                );
            }
            scene.push_command(Command::StrokePath {
                path: Path::rounded_rect(track, layout::RADIUS),
                stroke: Stroke::new(1.0),
                paint: Paint::solid(colors::TRACK),
                transform: Transform::IDENTITY,
                clip: Clip::NONE,
            });
            let origin = self.readout(i);
            for (cell, what) in value_digits(row.value).iter().enumerate() {
                draw_cell(&mut scene, cell_rect(origin, cell), *what);
            }
        }
        scene
    }

    fn render(&mut self) {
        // The scale is applied to the whole scene in one place, so the
        // logical model — hit testing, sizes, layout — never sees it.
        #[expect(clippy::cast_precision_loss, reason = "a DPR")]
        let factor = self.pixel_size().0 as f32 / self.width.max(1) as f32;
        let scene = self.scene().scaled(factor);
        let (w, h) = self.pixel_size();
        // The base colour is part of the contract, not a parameter: the
        // background command covers every pixel anyway, and a fixed base
        // keeps the frame byte-stable if a command ever leaves a hole.
        if self
            .renderer
            .render_in_place(&scene, w, h, colors::BG)
            .is_ok()
        {
            let out = self.renderer.last_frame();
            if out.len() == self.frame.len() {
                self.frame.copy_from_slice(out);
                self.dirty = false;
            }
        }
    }

    /// Deliver a pointer event to the row geometry, in logical
    /// coordinates.
    fn hit(&mut self, phase: Pointer) {
        if self.host.is_none() {
            return;
        }
        let (x, y) = phase.position();
        if matches!(phase, Pointer::Up(..)) {
            self.edit = None;
            self.dirty = true;
            return;
        }
        if matches!(phase, Pointer::Down(..)) {
            self.edit = self.rows.iter().enumerate().position(|(i, _)| {
                let (row, _) = self.track(i);
                y >= row.top && y < row.bottom && x >= row.left && x < row.right
            });
        }
        if let Some(i) = self.edit {
            // The whole row is the target (a finger misses an 8-px
            // track); the track's x span maps to values. Off the rows,
            // a drag keeps its row — a slider you can over-shoot.
            let (_, track) = self.track(i);
            let n = if track.width() > 0.0 {
                f64::from(((x - track.left) / track.width()).clamp(0.0, 1.0))
            } else {
                0.5
            };
            let info = self.rows[i].info.clone();
            let v = Self::plain(&info, n);
            self.rows[i].value = v;
            self.dirty = true;
            if let Some(host) = self.host.as_ref() {
                host.set_param(info.id, v);
            }
        }
    }
}

impl PluginGui for Panel {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn set_scale(&mut self, scale: f64) -> bool {
        if !(0.25..=8.0).contains(&scale) || !scale.is_finite() {
            return false;
        }
        self.scale = scale;
        self.reallocate();
        self.dirty = true;
        true
    }

    fn attach(&mut self, host: Box<dyn GuiHost + Send>, parent: Parent) -> bool {
        if parent.native().is_some() {
            // Refused by name, not by crash: the panel renders offscreen,
            // and presenting into a window is the platform crates' job.
            return false;
        }
        self.host = Some(host);
        self.shown = true;
        self.dirty = true;
        true
    }

    fn resize(&mut self, w: u32, h: u32) -> bool {
        let (w, h) = Self::clamped_size(w, h);
        if (w, h) == (self.width, self.height) {
            return true;
        }
        (self.width, self.height) = (w, h);
        self.reallocate();
        self.dirty = true;
        true
    }

    fn param_notify(&mut self, id: u32, value: f64) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.info.id == id) {
            let v = row.info.clamp(value);
            if v != row.value {
                row.value = v;
                self.dirty = true;
            }
        }
    }

    fn pointer(&mut self, phase: Pointer) {
        self.hit(phase);
    }

    fn frame(&mut self) -> Frame<'_> {
        if self.dirty || self.frame.iter().all(|b| *b == 0) {
            self.render();
        }
        let (w, h) = self.pixel_size();
        Frame {
            width: w,
            height: h,
            rgba: &self.frame,
        }
    }

    fn show(&mut self) -> bool {
        self.shown = true;
        self.host.is_some()
    }

    fn hide(&mut self) -> bool {
        self.shown = false;
        self.host.is_some()
    }

    fn close(&mut self) {
        if let Some(host) = self.host.take() {
            host.closed();
        }
        self.edit = None;
        self.shown = false;
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        // The host-initiated path called `closed`; a dropped panel has
        // none left to tell — same resource semantics as any Rust guard.
        self.host = None;
    }
}

/// Row `i`'s top edge, in logical pixels.
#[expect(clippy::cast_precision_loss, reason = "a row index")]
fn row_h(i: usize) -> f32 {
    i as f32 * layout::ROW
}

/// A solid rectangle.
fn solid(scene: &mut Scene, rect: Rect, color: Color) {
    scene.push_command(Command::FillRect {
        rect,
        paint: Paint::solid(color),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
}

/// A solid rounded rectangle.
fn rrect(scene: &mut Scene, rect: Rect, radius: f32, color: Color) {
    scene.push_command(Command::FillPath {
        path: Path::rounded_rect(rect, radius),
        paint: Paint::solid(color),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
}

// ── the 7-segment readout ─────────────────────────────────────────────────
//
// Segment names (a cell is w × h, thickness t):
//
//     aaaa
//    f    b
//    f    b          0: a b c d e f      1: b c
//     gggg           2: a b g e d        3: a b g c d
//    e    c          4: f g b c          5: a f g c d
//    e    c          6: a f g e c d      7: a b c
//     dddd           8: all              9: a b c d f g

/// What one readout slot draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    /// A blank slot (leading blanks): all seven segments unlit.
    Blank,
    /// A digit, as seven bits `abcdefg`.
    Digit(u8),
    /// The decimal point.
    Dot,
    /// The minus sign.
    Minus,
}

/// `v` as readout cells: sign, up to three integer digits, a point, a
/// tenth. Wider values clamp their display (`999.9`) rather than squeeze;
/// non-finite values read as 0.0.
fn value_digits(v: f64) -> [Cell; layout::CELLS] {
    const A: u8 = 1 << 0;
    const B: u8 = 1 << 1;
    const C: u8 = 1 << 2;
    const D: u8 = 1 << 3;
    const E: u8 = 1 << 4;
    const F: u8 = 1 << 5;
    const G: u8 = 1 << 6;
    #[rustfmt::skip]
    const PATTERNS: [u8; 10] = [
        A | B | C | D | E | F,      // 0
        B | C,                      // 1
        A | B | G | E | D,          // 2
        A | B | G | C | D,          // 3
        F | G | B | C,              // 4
        A | F | G | C | D,          // 5
        A | F | G | E | C | D,      // 6
        A | B | C,                  // 7
        A | B | C | D | E | F | G,  // 8
        A | B | C | D | F | G,      // 9
    ];

    let negative = v.is_sign_negative() && v != 0.0;
    let magnitude = if v.is_finite() { v.abs() } else { 0.0 };
    let clamped = magnitude.min(999.95);
    #[expect(clippy::cast_possible_truncation, reason = "clamped to 999.95")]
    let whole = clamped.trunc() as u32;
    #[expect(clippy::cast_possible_truncation, reason = "a tenth digit")]
    let tenth = ((clamped - f64::from(whole)) * 10.0).trunc() as u32;

    let hundreds = whole / 100 % 10;
    let tens = whole / 10 % 10;
    let ones = whole % 10;
    let mut text: Vec<Cell> = Vec::with_capacity(layout::CELLS);
    if negative {
        text.push(Cell::Minus);
    }
    // Leading zeros are blank (12.5 reads " 12.5", not "012.5") — but at
    // least one digit always draws.
    let mut drew = false;
    for (digit, leading) in [(hundreds, true), (tens, hundreds == 0), (ones, false)] {
        if leading && digit == 0 && !drew {
            continue;
        }
        text.push(Cell::Digit(PATTERNS[digit as usize]));
        drew = true;
    }
    text.push(Cell::Dot);
    text.push(Cell::Digit(PATTERNS[(tenth % 10) as usize]));

    // Right-align into the fixed slots.
    let mut cells = [Cell::Blank; layout::CELLS];
    let take = text.len().min(layout::CELLS);
    let skip = text.len() - take;
    for (slot, cell) in text[skip..].iter().enumerate() {
        cells[layout::CELLS - take + slot] = *cell;
    }
    cells
}

/// The `i`'th readout slot's rectangle. The last slot is the narrow
/// decimal-point cell.
#[expect(clippy::cast_precision_loss, reason = "a slot index")]
fn cell_rect(origin: Offset, i: usize) -> Rect {
    let (cw, ch) = layout::CELL;
    let w = if i + 1 == layout::CELLS {
        layout::DOT
    } else {
        cw
    };
    Rect::new(
        origin.dx + i as f32 * cw,
        origin.dy,
        origin.dx + i as f32 * cw + w,
        origin.dy + ch,
    )
}

/// Draw one readout cell: unlit segment ghosts plus the lit pattern —
/// all seven boxes always draw, so a right-aligned number's digits keep
/// a stable rhythm instead of jittering position.
fn draw_cell(scene: &mut Scene, cell: Rect, what: Cell) {
    let (cw, ch, t) = (cell.width(), cell.height(), layout::SEG);
    let (x0, y0) = (cell.left, cell.top);
    let third = (ch - 2.0 * t) / 3.0;
    // a..g in bit order.
    let segments = [
        Rect::new(x0 + t, y0, x0 + cw - t, y0 + t),
        Rect::new(x0 + cw - t, y0 + t, x0 + cw, y0 + t + third),
        Rect::new(x0 + cw - t, y0 + t + third + t, x0 + cw, y0 + ch - t),
        Rect::new(x0 + t, y0 + ch - t, x0 + cw - t, y0 + ch),
        Rect::new(x0, y0 + t + third + t, x0 + t, y0 + ch - t),
        Rect::new(x0, y0 + t, x0 + t, y0 + t + third),
        Rect::new(
            x0 + t,
            y0 + ch - t - third - t * 0.5,
            x0 + cw - t,
            y0 + ch - t - third + t * 0.5,
        ),
    ];
    let bits = match what {
        Cell::Digit(bits) => bits,
        Cell::Minus => 1 << 6, // the middle bar alone
        _ => 0,
    };
    for (i, rect) in segments.iter().enumerate() {
        let lit = bits & (1 << i) != 0;
        solid(
            scene,
            *rect,
            if lit { colors::SEG_ON } else { colors::SEG_OFF },
        );
    }
    if what == Cell::Dot {
        solid(
            scene,
            Rect::new(x0 + cw - t, y0 + ch - t, x0 + cw, y0 + ch),
            colors::SEG_ON,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Frame, GuiHost, Parent, PluginGui, Pointer};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// A host that records what the GUI did.
    #[derive(Debug, Default)]
    struct Recorder {
        params: Mutex<Vec<(u32, f64)>>,
        closes: AtomicUsize,
    }

    impl GuiHost for Recorder {
        fn set_param(&self, id: u32, value: f64) {
            self.params.lock().unwrap().push((id, value));
        }
        fn request_resize(&self, _: u32, _: u32) -> bool {
            // This panel never asks; the answer is irrelevant but honest.
            true
        }
        fn closed(&self) {
            self.closes.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn params() -> Vec<ParamInfo> {
        vec![
            ParamInfo::new(1, "Gain", 0.0, 2.0, 1.0),
            ParamInfo::new(7, "Level", -60.0, 6.0, -18.0),
            {
                let mut p = ParamInfo::new(9, "Power", 0.0, 1.0, 0.0);
                p.taper = Taper::Stepped;
                p
            },
        ]
    }

    fn panel() -> Panel {
        Panel::new(&params())
    }

    #[test]
    fn frames_are_deterministic() {
        let a = {
            let mut g = panel();
            g.frame().to_vec()
        };
        let b = {
            let mut g = panel();
            let _ = g.frame().to_vec();
            let _ = g.frame().to_vec(); // the re-render path too
            g.frame().to_vec()
        };
        assert_eq!(a, b, "same inputs, byte-identical frames");
        // And a different value must differ.
        let mut other = params();
        other[0].default = 0.5;
        let c = {
            let mut g = Panel::new(&other);
            g.frame().to_vec()
        };
        assert_ne!(a, c, "a changed value changes the frame");
    }

    #[test]
    fn the_frame_is_sized_and_opaque() {
        let mut g = panel();
        let f: Frame<'_> = g.frame();
        let (width, height, rgba) = (f.width, f.height, f.rgba);
        assert_eq!(rgba.len(), (width * height * 4) as usize);
        assert_eq!((width, height), (380, Panel::height_for(3)));
        // The background covers every pixel: alpha 255 everywhere.
        assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        // And it is not one flat colour (tracks, digits, strip).
        let distinct = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct.len() > 4, "the panel draws structure");
    }

    #[test]
    fn a_drag_round_trips_through_the_host_and_back() {
        let rec = Arc::new(Recorder::default());
        let mut g = panel();
        assert!(g.attach(Box::new(Arc::clone(&rec)), Parent::Offscreen));
        // Row 0 ("Gain", 0..2, default 1.0): drag its track's middle.
        let row_y = layout::HEADER + layout::PAD + 0.5 * layout::ROW;
        g.pointer(Pointer::Down(layout::PAD + 100.0, row_y));
        g.pointer(Pointer::Up(layout::PAD + 100.0, row_y));
        let edits = rec.params.lock().unwrap().clone();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].0, 1);
        assert!((edits[0].1 - 1.0).abs() < 1e-9, "halfway across the track");
        // A drag continues after moving off the rows: same row, clamped.
        // The Down re-edits at the old spot (one event), the Move clamps
        // to max (a second) — the Up only ends the gesture.
        g.pointer(Pointer::Down(layout::PAD + 100.0, row_y));
        g.pointer(Pointer::Move(5_000.0, row_y));
        g.pointer(Pointer::Up(5_000.0, row_y));
        let edits = rec.params.lock().unwrap().clone();
        assert_eq!(edits.len(), 3);
        assert_eq!(edits[2].1, 2.0, "clamped to max, row kept");

        // Host → GUI: the edit comes back as a notification, and the
        // frame changes because of it.
        let before = g.frame().to_vec();
        g.param_notify(1, 0.25);
        let after = g.frame().to_vec();
        assert_ne!(before, after, "the notification repainted");
        assert_eq!(g.values(), vec![(1, 0.25), (7, -18.0), (9, 0.0)]);
        // Unknown ids are ignored, not fatal.
        g.param_notify(999, 1.0);
        assert_eq!(g.values().len(), 3);
    }

    #[test]
    fn drags_clamp_and_snap_to_the_taper() {
        let rec = Arc::new(Recorder::default());
        let mut g = panel();
        g.attach(Box::new(Arc::clone(&rec)), Parent::Offscreen);
        let row_y = layout::HEADER + layout::PAD + 1.5 * layout::ROW;
        // Left edge and far right of row 1 (−60..6, linear) — the row
        // rectangle starts at `PAD` and the track ends at `PAD + 200`, so
        // those x's are the extreme in-row hits.
        g.pointer(Pointer::Down(layout::PAD, row_y));
        g.pointer(Pointer::Up(layout::PAD, row_y));
        g.pointer(Pointer::Down(layout::PAD + layout::TRACK.0, row_y));
        g.pointer(Pointer::Up(layout::PAD + layout::TRACK.0, row_y));
        let edits = rec.params.lock().unwrap().clone();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].1, -60.0, "clamped to min");
        assert_eq!(edits[1].1, 6.0, "clamped to max");
        // Row 2 is stepped: only integers exist.
        let step_y = layout::HEADER + layout::PAD + 2.5 * layout::ROW;
        g.pointer(Pointer::Down(layout::PAD + 10.0, step_y));
        g.pointer(Pointer::Up(layout::PAD + 10.0, step_y));
        let edits = rec.params.lock().unwrap().clone();
        assert_eq!(edits.len(), 3);
        assert_eq!(edits[2].1, 0.0, "stepped values snap to integers");
    }

    #[test]
    fn a_press_outside_the_rows_edits_nothing() {
        let rec = Arc::new(Recorder::default());
        let mut g = panel();
        g.attach(Box::new(Arc::clone(&rec)), Parent::Offscreen);
        g.pointer(Pointer::Down(100.0, 2.0));
        g.pointer(Pointer::Up(100.0, 2.0));
        g.pointer(Pointer::Move(100.0, 2.0));
        assert!(rec.params.lock().unwrap().is_empty());
    }

    #[test]
    fn resize_and_scale_change_the_frame_and_geometry_keeps_working() {
        let rec = Arc::new(Recorder::default());
        let mut g = panel();
        g.attach(Box::new(Arc::clone(&rec)), Parent::Offscreen);
        let f: Frame<'_> = g.frame();
        assert_eq!((f.width, f.height), (380, Panel::height_for(3)));
        assert!(g.resize(500, 300));
        let f: Frame<'_> = g.frame();
        assert_eq!((f.width, f.height), (500, 300));
        // Sub-minimum sizes clamp, and say so by what they keep.
        assert!(g.resize(1, 1));
        let f: Frame<'_> = g.frame();
        assert_eq!((f.width, f.height), (240, Panel::height_for(2)));
        // 2× DPI: double the pixels.
        assert!(g.set_scale(2.0));
        let f: Frame<'_> = g.frame();
        assert_eq!((f.width, f.height), (480, 2 * Panel::height_for(2)));
        // Hit testing still works in logical coordinates at that scale.
        assert!(g.resize(380, Panel::height_for(3)));
        let row_y = layout::HEADER + layout::PAD + 0.5 * layout::ROW;
        g.pointer(Pointer::Down(layout::PAD + 100.0, row_y));
        g.pointer(Pointer::Up(layout::PAD + 100.0, row_y));
        assert!(
            !rec.params.lock().unwrap().is_empty(),
            "logical hits at any scale"
        );
        // A nonsense scale is refused without effect.
        let (w, h) = {
            let f: Frame<'_> = g.frame();
            (f.width, f.height)
        };
        assert!(!g.set_scale(f64::NAN));
        assert!(!g.set_scale(0.0));
        let f: Frame<'_> = g.frame();
        assert_eq!((f.width, f.height), (w, h));
    }

    #[test]
    fn native_parents_are_refused_and_close_notifies() {
        let rec = Arc::new(Recorder::default());
        let mut g = panel();
        assert!(!g.attach(
            Box::new(Arc::clone(&rec)),
            Parent::Native(std::ptr::dangling_mut::<std::ffi::c_void>()),
        ));
        assert_eq!(rec.closes.load(Ordering::Relaxed), 0);
        assert!(g.attach(Box::new(Arc::clone(&rec)), Parent::Offscreen));
        assert!(g.show());
        assert!(g.hide());
        g.close();
        assert_eq!(rec.closes.load(Ordering::Relaxed), 1);
        // After close, input goes nowhere and the frame is stable.
        let stable = g.frame().to_vec();
        g.pointer(Pointer::Down(100.0, 46.0));
        assert_eq!(g.frame().to_vec(), stable);
        // Close is idempotent.
        g.close();
        assert_eq!(rec.closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn readout_digits_are_the_classic_patterns() {
        const A: u8 = 1 << 0;
        const B: u8 = 1 << 1;
        const C: u8 = 1 << 2;
        const D: u8 = 1 << 3;
        const E: u8 = 1 << 4;
        const F: u8 = 1 << 5;
        use Cell::*;
        // 1.0 → "  1.0": blanks, then 1, dot, 0.
        let cells = value_digits(1.0);
        assert_eq!(
            cells[layout::CELLS - 1],
            Digit(A | B | C | D | E | F),
            "the tenth is 0"
        );
        assert_eq!(cells[layout::CELLS - 2], Dot);
        assert_eq!(cells[layout::CELLS - 3], Digit(B | C), "the ones is 1");
        assert!(cells[..layout::CELLS - 3].iter().all(|c| *c == Blank));
        // −12.5 keeps its sign and its digits, right-aligned with a
        // leading blank: " −12.5".
        let cells = value_digits(-12.5);
        assert_eq!(cells[0], Blank);
        assert_eq!(cells[1], Minus);
        assert_eq!(
            cells[layout::CELLS - 1],
            Digit(A | F | 1 << 6 | C | D),
            "5 lights a f g c d"
        );
        // Clamp shape for huge values: still six cells.
        assert_eq!(value_digits(1e9).len(), layout::CELLS);
        assert_eq!(value_digits(f64::NAN).len(), layout::CELLS);
        // Zero prints one digit, not none.
        assert_eq!(
            value_digits(0.0)[layout::CELLS - 3],
            Digit(1 << 0 | B | C | 1 << 3 | 1 << 4 | 1 << 5)
        );
    }

    #[test]
    fn degenerate_ranges_render_and_hit_at_the_middle() {
        let p = vec![ParamInfo::new(3, "Flat", 1.0, 1.0, 1.0)];
        let rec = Arc::new(Recorder::default());
        let mut g = Panel::new(&p);
        g.attach(Box::new(Arc::clone(&rec)), Parent::Offscreen);
        let f: Frame<'_> = g.frame();
        assert_eq!(f.rgba.len(), (f.width * f.height * 4) as usize);
        let row_y = layout::HEADER + layout::PAD + 0.5 * layout::ROW;
        g.pointer(Pointer::Down(layout::PAD + 5.0, row_y));
        g.pointer(Pointer::Up(layout::PAD + 5.0, row_y));
        let edits = rec.params.lock().unwrap().clone();
        assert_eq!(edits, vec![(3, 1.0)], "a flat range sends its only value");
    }
}
