//! The pixel layer: the twin's sprite, the morse strip, the room and the
//! world. Everything is a filled rectangle on the [`PX`] grid.

use std::any::Any;

use vieww::foundation::{Color, Rect, Size};
use vieww::prelude::*;
use vieww::widget::{CustomPaint, CustomPainter, DrawInstruction};

use crate::{AMBER, EDGE, GROUND, MINT, PX};

fn rect(x: f32, y: f32, w: f32, h: f32, color: Color) -> DrawInstruction {
    DrawInstruction::FillRect {
        rect: Rect::new(x, y, x + w, y + h),
        color,
    }
}

// ---- the twin ---------------------------------------------------------------

/// The 10×16 sprite. `.` is empty; letters index [`Avatar::colors`].
const SPRITE: [&str; 16] = [
    "...hhhh...",
    "..hhhhhh..",
    ".hhhhhhhh.",
    ".hssssssh.",
    ".hsessess.",
    "..ssssss..",
    "...ssss...",
    "..cccccc..",
    ".tttttttt.",
    ".tttbbttt.",
    ".stttttts.",
    ".sttttttss",
    "..pp..pp..",
    "..pp..pp..",
    "..pp..pp..",
    "..ff..ff..",
];

/// The pixel twin. Its colours are settings on the part (DESIGN: hair, skin,
/// shirt, trousers, collar); the collar and badge carry the morse identity.
#[derive(Debug, Clone, PartialEq)]
pub struct Avatar {
    pub scale: f32,
    pub hair: Color,
    pub skin: Color,
    pub shirt: Color,
    pub trousers: Color,
    pub collar: Color,
    /// Rows the sprite is lifted by (the idle sway), 0 when motion is gated.
    pub lift: i32,
}

impl Avatar {
    /// The default twin, drawn at `scale` logical px per sprite pixel.
    #[must_use]
    pub fn new(scale: f32) -> Self {
        Self {
            scale,
            hair: Color::rgb(0x5A, 0x40, 0x32),
            skin: Color::rgb(0xE6, 0xB8, 0x94),
            shirt: Color::rgb(0x2C, 0x3A, 0x57),
            trousers: Color::rgb(0x1C, 0x24, 0x38),
            collar: MINT,
            lift: 0,
        }
    }

    /// The size it paints at.
    #[must_use]
    pub fn size(&self) -> Size {
        Size::new(10.0 * self.scale, 16.0 * self.scale)
    }

    fn color(&self, c: char) -> Option<Color> {
        Some(match c {
            'h' => self.hair,
            's' => self.skin,
            'e' => Color::rgb(0x10, 0x12, 0x10),
            'c' => self.collar,
            't' => self.shirt,
            'b' => AMBER,
            'p' => self.trousers,
            'f' => Color::rgb(0x05, 0x06, 0x05),
            _ => return None,
        })
    }

    /// As a widget.
    #[must_use]
    pub fn widget(self) -> WidgetNode {
        let size = self.size();
        CustomPaint::sized(size, self).into()
    }
}

impl CustomPainter for Avatar {
    fn paint(&self, _size: Size) -> Vec<DrawInstruction> {
        let mut out = Vec::new();
        let dy = -(self.lift as f32) * self.scale;
        for (row, line) in SPRITE.iter().enumerate() {
            // Merge runs of one colour so a row is a few rects, not ten.
            let cells: Vec<char> = line.chars().collect();
            let mut x = 0;
            while x < cells.len() {
                let c = cells[x];
                let mut end = x + 1;
                while end < cells.len() && cells[end] == c {
                    end += 1;
                }
                if let Some(color) = self.color(c) {
                    out.push(rect(
                        x as f32 * self.scale,
                        row as f32 * self.scale + dy,
                        (end - x) as f32 * self.scale,
                        self.scale,
                        color,
                    ));
                }
                x = end;
            }
        }
        out
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn should_repaint(&self, previous: &dyn CustomPainter) -> bool {
        previous.as_any().downcast_ref::<Self>() != Some(self)
    }
}

// ---- morse ------------------------------------------------------------------

fn code(c: char) -> &'static str {
    match c.to_ascii_lowercase() {
        'a' => ".-", 'b' => "-...", 'c' => "-.-.", 'd' => "-..", 'e' => ".",
        'f' => "..-.", 'g' => "--.", 'h' => "....", 'i' => "..", 'j' => ".---",
        'k' => "-.-", 'l' => ".-..", 'm' => "--", 'n' => "-.", 'o' => "---",
        'p' => ".--.", 'q' => "--.-", 'r' => ".-.", 's' => "...", 't' => "-",
        'u' => "..-", 'v' => "...-", 'w' => ".--", 'x' => "-..-", 'y' => "-.--",
        'z' => "--..", '0' => "-----", '1' => ".----", '2' => "..---",
        '3' => "...--", '4' => "....-", '5' => ".....", '6' => "-....",
        '7' => "--...", '8' => "---..", '9' => "----.",
        _ => "",
    }
}

/// The on/off timeline of `text` in morse units: dot 1u, dash 3u, 1u between
/// marks, 3u between letters, 5u between words (DESIGN: `MorseStrip`).
#[must_use]
pub fn morse_runs(text: &str) -> Vec<(bool, u32)> {
    let mut runs: Vec<(bool, u32)> = Vec::new();
    let mut gap = 0u32;
    for (i, word) in text.split_whitespace().enumerate() {
        if i > 0 {
            gap = 5;
        }
        for (j, letter) in word.chars().enumerate() {
            if j > 0 {
                gap = 3;
            }
            for (k, mark) in code(letter).chars().enumerate() {
                if k > 0 {
                    gap = 1;
                }
                if gap > 0 && !runs.is_empty() {
                    runs.push((false, gap));
                }
                gap = 0;
                runs.push((true, if mark == '-' { 3 } else { 1 }));
            }
        }
    }
    runs
}

/// A strip of morse code. `unit` is in logical px (2–4 per DESIGN).
#[derive(Debug, Clone, PartialEq)]
pub struct Morse {
    pub text: String,
    pub unit: f32,
    pub color: Color,
}

impl Morse {
    #[must_use]
    pub fn new(text: &str, unit: f32, color: Color) -> Self {
        Self { text: text.to_owned(), unit, color }
    }

    /// Its width in logical px.
    #[must_use]
    pub fn width(&self) -> f32 {
        morse_runs(&self.text).iter().map(|(_, u)| *u).sum::<u32>() as f32 * self.unit
    }

    #[must_use]
    pub fn widget(self) -> WidgetNode {
        let size = Size::new(self.width().max(1.0), self.unit * 2.0);
        CustomPaint::sized(size, self).into()
    }
}

impl CustomPainter for Morse {
    fn paint(&self, size: Size) -> Vec<DrawInstruction> {
        let mut x = 0.0;
        let mut out = Vec::new();
        for (on, units) in morse_runs(&self.text) {
            let w = units as f32 * self.unit;
            if on {
                out.push(rect(x, 0.0, w, size.height, self.color));
            }
            x += w;
        }
        out
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn should_repaint(&self, previous: &dyn CustomPainter) -> bool {
        previous.as_any().downcast_ref::<Self>() != Some(self)
    }
}

// ---- scenes -----------------------------------------------------------------

/// Which scene a [`Scene`] paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The twin's room: window, picture, shelf, floor.
    Room,
    /// The shared world: dithered night sky, moon, trees, lamp, cobbles.
    World,
}

/// A pixel scene on the [`PX`] grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub place: Place,
}

impl Scene {
    #[must_use]
    pub fn widget(self, size: Size) -> WidgetNode {
        CustomPaint::sized(size, self).into()
    }
}

/// Snap to the grid.
fn g(v: f32) -> f32 {
    (v / PX).floor() * PX
}

impl CustomPainter for Scene {
    fn paint(&self, size: Size) -> Vec<DrawInstruction> {
        let mut out = Vec::new();
        let (w, h) = (g(size.width), g(size.height));
        match self.place {
            Place::Room => {
                out.push(rect(0.0, 0.0, w, h, Color::rgb(0x0E, 0x12, 0x14)));
                // The window: four panes of night, with stars.
                for (i, (px, py)) in [(8.0, 14.0), (8.0, 32.0), (14.0, 14.0), (14.0, 32.0)].iter().enumerate() {
                    out.push(rect(g(px * PX), g(py * PX), 17.0 * PX, 8.0 * PX, Color::rgb(0x12, 0x1A, 0x2A)));
                    if i % 2 == 0 {
                        out.push(rect(g(px * PX + 18.0), g(py * PX + 12.0), PX, PX, Color::rgb(0xC8, 0xD6, 0xF0)));
                    }
                }
                // A picture on the wall.
                out.push(rect(g(w * 0.42), g(h * 0.14), 18.0 * PX, 14.0 * PX, EDGE));
                out.push(rect(g(w * 0.42) + 2.0 * PX, g(h * 0.14) + 3.0 * PX, 5.0 * PX, 6.0 * PX, BLUEISH));
                out.push(rect(g(w * 0.42) + 9.0 * PX, g(h * 0.14) + 6.0 * PX, 5.0 * PX, 5.0 * PX, MINT));
                // The shelf and its books.
                let shelf_y = g(h * 0.36);
                for (i, c) in [Color::rgb(0xFF, 0x7A, 0x8C), BLUEISH, AMBER, MINT].iter().enumerate() {
                    out.push(rect(g(w * 0.64) + i as f32 * 4.0 * PX, shelf_y - 9.0 * PX, 3.0 * PX, 9.0 * PX, *c));
                }
                out.push(rect(g(w * 0.63), shelf_y, g(w * 0.33), PX, EDGE));
                // A chair and the floor line.
                out.push(rect(g(w * 0.77), g(h * 0.60), 16.0 * PX, 14.0 * PX, Color::rgb(0x1A, 0x22, 0x2E)));
                out.push(rect(0.0, g(h * 0.88), w, PX, EDGE));
            }
            Place::World => {
                out.push(rect(0.0, 0.0, w, h, GROUND));
                // Dithered sky: a checker that thins towards the horizon.
                let horizon = g(h * 0.55);
                let mut y = 0.0;
                let mut row = 0;
                while y < horizon {
                    let density = 1.0 - y / horizon; // fewer cells lower down
                    let mut x = if row % 2 == 0 { 0.0 } else { PX };
                    while x < w {
                        if ((x / PX) as i32 % 7) as f32 / 7.0 < density * 0.5 {
                            out.push(rect(x, y, PX, PX, Color::rgb(0x14, 0x1E, 0x2C)));
                        }
                        x += 2.0 * PX;
                    }
                    y += PX;
                    row += 1;
                }
                // The moon, with a dithered glow.
                let (mx, my) = (g(w * 0.78), g(h * 0.10));
                out.push(rect(mx - 3.0 * PX, my - 3.0 * PX, 11.0 * PX, 11.0 * PX, Color::rgb(0x16, 0x22, 0x30)));
                out.push(rect(mx, my, 5.0 * PX, 5.0 * PX, Color::rgb(0xF2, 0xF0, 0xD8)));
                // Ground, then cobblestones.
                out.push(rect(0.0, horizon, w, h - horizon, Color::rgb(0x0F, 0x15, 0x10)));
                let path_y = g(h * 0.78);
                let mut x = 0.0;
                let mut i = 0;
                while x < w {
                    let y = path_y + if i % 2 == 0 { 0.0 } else { 2.0 * PX };
                    out.push(rect(x, y, 5.0 * PX, 2.0 * PX, Color::rgb(0x2A, 0x33, 0x2C)));
                    x += 6.0 * PX;
                    i += 1;
                }
                // Trees: a trunk, a shaded crown.
                for tx in [0.12_f32, 0.34, 0.9] {
                    let (x, base) = (g(w * tx), horizon + 6.0 * PX);
                    out.push(rect(x + 3.0 * PX, base - 6.0 * PX, 2.0 * PX, 6.0 * PX, Color::rgb(0x4A, 0x36, 0x28)));
                    out.push(rect(x, base - 14.0 * PX, 8.0 * PX, 8.0 * PX, Color::rgb(0x1E, 0x5A, 0x3A)));
                    out.push(rect(x, base - 14.0 * PX, 4.0 * PX, 8.0 * PX, Color::rgb(0x28, 0x74, 0x4A)));
                }
                // The north-gate lamp: a post and a warm glow.
                let (lx, ly) = (g(w * 0.62), horizon - 4.0 * PX);
                out.push(rect(lx, ly, PX, 14.0 * PX, EDGE));
                out.push(rect(lx - PX, ly - 3.0 * PX, 3.0 * PX, 3.0 * PX, AMBER));
                out.push(rect(lx - 3.0 * PX, ly - 5.0 * PX, 7.0 * PX, 7.0 * PX, Color::rgba(0xF6, 0xB9, 0x5A, 40)));
            }
        }
        out
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn should_repaint(&self, previous: &dyn CustomPainter) -> bool {
        previous.as_any().downcast_ref::<Self>() != Some(self)
    }
}

const BLUEISH: Color = Color::rgb(0x6C, 0x92, 0xE0);
