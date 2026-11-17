//! Gradient colour ramps, one row of [`RampAtlas::WIDTH`] premultiplied
//! `RGBA32F` texels per distinct stop list.
//!
//! A gradient's *geometry* (where `t` comes from) is evaluated per fragment in
//! the scene shader; its *colours* (what `t` means) come from here. The row is
//! `vieww_paint::native::gradient_ramp` — the CPU rasterizer's own ramp — so
//! the two renderers disagree only inside a texel interval that contains a
//! stop, and there by less than the interpolation across one 1/255 step.
//!
//! Rows are never moved within a frame (vertices hold their row index); a full
//! texture doubles its height. [`RampAtlas::begin_frame`] drops rows the
//! previous frame did not use once more than half the rows are taken.
//!
//! # Why rows upload as rectangles, like glyphs and images
//!
//! A new ramp is exactly one row — 4 KB, the smallest write of the three
//! atlases — and the honest question is whether recording it is worth any
//! machinery at all. It is, for two reasons. The machinery is not per-atlas:
//! [`DirtyRects`](crate::DirtyRects) is shared, so a row costs the recorder
//! one `blit(0, row, WIDTH, 1)` call. And a row is also the *only* unit of
//! churn: a gradient whose stops animate — the reason
//! [`RampAtlas::begin_frame`] compacts at all — is a new key and therefore a
//! new row every frame, which whole-texture uploads paid at 4 KB times the
//! row count, every frame, growing until the compaction: 64 KB at the
//! initial 16 rows, 16 MB at the 4096-row ceiling, in the widest format of
//! the three atlases.

use std::collections::HashMap;

use vieww_foundation::Gradient;

use crate::dirty::{AtlasChanges, DirtyRects};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RampKey(Vec<u32>);

impl RampKey {
    fn of(gradient: &Gradient) -> Self {
        let mut bits = Vec::with_capacity(gradient.stops().len() * 5);
        for stop in gradient.stops() {
            bits.push(stop.offset.to_bits());
            bits.push(u32::from_le_bytes([
                stop.color.r,
                stop.color.g,
                stop.color.b,
                stop.color.a,
            ]));
        }
        Self(bits)
    }
}

/// See the module doc.
#[derive(Debug)]
pub struct RampAtlas {
    rows: u32,
    texels: Vec<f32>,
    entries: HashMap<RampKey, (u32, u64)>,
    next_row: u32,
    frame: u64,
    /// The version and this frame's dirty rectangles — see [`DirtyRects`].
    dirty: DirtyRects,
}

impl Default for RampAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl RampAtlas {
    /// Samples per ramp.
    pub const WIDTH: u32 = 256;
    pub const INITIAL_ROWS: u32 = 16;
    pub const MAX_ROWS: u32 = 4096;

    #[must_use]
    pub fn new() -> Self {
        Self {
            rows: Self::INITIAL_ROWS,
            texels: vec![0.0; (Self::WIDTH * Self::INITIAL_ROWS * 4) as usize],
            entries: HashMap::new(),
            next_row: 0,
            frame: 1,
            dirty: DirtyRects::new(),
        }
    }

    /// Texture height in rows; the width is always [`Self::WIDTH`].
    #[must_use]
    pub const fn rows(&self) -> u32 {
        self.rows
    }
    /// `WIDTH * rows * 4` floats, premultiplied RGBA.
    #[must_use]
    pub fn texels(&self) -> &[f32] {
        &self.texels
    }
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.dirty.version()
    }
    /// What a backend whose texture holds `held` (the
    /// [`version`](Self::version) it last uploaded, `None` if it has never
    /// uploaded) must be sent — the same answer
    /// [`MaskAtlas::changes_since`](crate::MaskAtlas::changes_since) gives for
    /// masks. A new ramp's rectangle is its whole row; a compaction or a row
    /// count doubling is a full upload, as is any backend that missed a
    /// frame.
    #[must_use]
    pub fn changes_since(&self, held: Option<u64>) -> AtlasChanges<'_> {
        self.dirty.changes_since(held)
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn begin_frame(&mut self) {
        self.dirty.begin_frame();
        let previous = self.frame;
        self.frame += 1;
        if self.next_row * 2 <= self.rows {
            return;
        }
        let live: Vec<(RampKey, u32, u64)> = self
            .entries
            .iter()
            .filter(|(_, (_, used))| *used >= previous)
            .map(|(k, (row, used))| (k.clone(), *row, *used))
            .collect();
        if live.len() == self.entries.len() {
            return;
        }
        // Everything from here on rewrites the texture wholesale — rows move
        // to new indices — so this frame's difference is "all of it", not the
        // rows that happen to have been copied.
        self.dirty.rewrote();
        let old = std::mem::take(&mut self.texels);
        self.texels = vec![0.0; old.len()];
        self.entries.clear();
        self.next_row = 0;
        let w = (Self::WIDTH * 4) as usize;
        for (key, row, used) in live {
            let to = self.next_row as usize * w;
            let from = row as usize * w;
            self.texels[to..to + w].copy_from_slice(&old[from..from + w]);
            self.entries.insert(key, (self.next_row, used));
            self.next_row += 1;
        }
    }

    /// The row holding `gradient`'s ramp, or `None` if the texture is full.
    pub fn row(&mut self, gradient: &Gradient) -> Option<u32> {
        let key = RampKey::of(gradient);
        let frame = self.frame;
        if let Some((row, used)) = self.entries.get_mut(&key) {
            *used = frame;
            return Some(*row);
        }
        if self.next_row >= self.rows {
            if self.rows >= Self::MAX_ROWS {
                return None;
            }
            self.rows *= 2;
            self.texels
                .resize((Self::WIDTH * self.rows * 4) as usize, 0.0);
            // A resize is a full upload for every backend — the texture must
            // be recreated — so this frame's recording is abandoned rather
            // than appended to.
            self.dirty.rewrote();
        }
        let row = self.next_row;
        self.next_row += 1;
        let samples = vieww_paint::native::gradient_ramp(gradient, Self::WIDTH as usize);
        let start = (row * Self::WIDTH * 4) as usize;
        for (i, c) in samples.iter().enumerate() {
            self.texels[start + i * 4..start + i * 4 + 4].copy_from_slice(c);
        }
        self.entries.insert(key, (row, frame));
        self.dirty.blit(0, row, Self::WIDTH, 1);
        Some(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_foundation::Color;

    #[test]
    fn a_ramp_row_holds_the_stop_colours_premultiplied_at_its_ends() {
        let mut atlas = RampAtlas::new();
        let gradient = Gradient::horizontal().with_stops(&[
            (0.0, Color::rgba(255, 0, 0, 255)),
            (1.0, Color::rgba(0, 0, 255, 128)),
        ]);
        let row = atlas.row(&gradient).unwrap() as usize;
        let w = RampAtlas::WIDTH as usize;
        let first = &atlas.texels()[row * w * 4..row * w * 4 + 4];
        assert_eq!(first, &[1.0, 0.0, 0.0, 1.0]);
        let last = &atlas.texels()[(row * w + w - 1) * 4..(row * w + w) * 4];
        let a = 128.0 / 255.0;
        assert!((last[2] - a).abs() < 1e-6 && (last[3] - a).abs() < 1e-6);
    }

    #[test]
    fn the_same_stops_share_a_row_and_growing_keeps_rows() {
        let mut atlas = RampAtlas::new();
        let mut rows = Vec::new();
        for i in 0..40u8 {
            let g = Gradient::horizontal()
                .with_stops(&[(0.0, Color::rgba(i, 0, 0, 255)), (1.0, Color::WHITE)]);
            rows.push(atlas.row(&g).unwrap());
        }
        for (i, row) in rows.iter().enumerate() {
            let g = Gradient::horizontal()
                .with_stops(&[(0.0, Color::rgba(i as u8, 0, 0, 255)), (1.0, Color::WHITE)]);
            assert_eq!(atlas.row(&g), Some(*row));
        }
        assert_eq!(atlas.len(), 40);
    }

    // ------------------------------------------------- partial uploads

    /// A "texture" the way a backend holds one: the texels it was last sent,
    /// replayed exactly as `changes_since` prescribes.
    #[derive(Default)]
    struct Backend {
        texels: Vec<f32>,
        held: Option<u64>,
        fulls: u32,
        rows_sent: u64,
    }

    impl Backend {
        /// Apply what `changes_since` says to send, and check the result
        /// equals the atlas — the replay every partial-upload backend
        /// performs, held to texel identity after every frame.
        fn sync(&mut self, atlas: &RampAtlas) {
            match atlas.changes_since(self.held) {
                crate::AtlasChanges::None => {}
                crate::AtlasChanges::Full => {
                    self.texels = atlas.texels().to_vec();
                    self.fulls += 1;
                }
                crate::AtlasChanges::Regions(rects) => {
                    assert_eq!(
                        self.texels.len(),
                        atlas.texels().len(),
                        "a region upload never resizes"
                    );
                    for &[x, y, w, h] in rects {
                        for row in y..y + h {
                            // A ramp's rectangle is always (0, row, WIDTH, 1),
                            // whatever the row count.
                            let i = (row as usize * RampAtlas::WIDTH as usize + x as usize) * 4;
                            let n = w as usize * 4;
                            self.texels[i..i + n]
                                .copy_from_slice(&atlas.texels()[i..i + n]);
                            self.rows_sent += 1;
                        }
                    }
                }
            }
            self.held = Some(atlas.version());
            assert_eq!(
                self.texels, atlas.texels(),
                "the backend's texture must equal the atlas after syncing"
            );
        }
    }

    /// `changes_since` replayed onto a texture must leave it equal to the
    /// atlas after every sync — the property `vieww-hal`'s partial ramp
    /// uploads are held to. Runs the atlas through a first upload, a frame
    /// the backend skips, a row doubling, and a compaction that moves rows:
    /// the compaction has to reach the backend as a full upload, never the
    /// moved rows as rectangles.
    #[test]
    fn changes_since_replayed_leaves_a_texture_equal_to_the_atlas() {
        let grad = |i: u8| {
            Gradient::horizontal().with_stops(&[
                (0.0, Color::rgba(i, 0, 0, 255)),
                (1.0, Color::WHITE),
            ])
        };
        let mut atlas = RampAtlas::new();
        let mut backend = Backend::default();

        // Frame 1: three ramps. The first upload is a full one.
        atlas.begin_frame();
        for i in 1..=3u8 {
            atlas.row(&grad(i));
        }
        backend.sync(&atlas);
        assert_eq!(backend.fulls, 1);

        // Frame 2: one new ramp. Only its row goes: 4 KB, against 64 KB for
        // the whole 16-row texture.
        let before = backend.rows_sent;
        atlas.begin_frame();
        atlas.row(&grad(4));
        backend.sync(&atlas);
        assert_eq!(backend.rows_sent - before, 1, "one new ramp is one row");

        // Frames 3-4: the backend skips a frame; the one after is a full one.
        atlas.begin_frame();
        atlas.row(&grad(5));
        atlas.begin_frame();
        atlas.row(&grad(6));
        backend.sync(&atlas);
        assert_eq!(backend.fulls, 2);

        // Frame 5: more than 16 distinct ramps. The doubling is a resize, so
        // a full upload.
        atlas.begin_frame();
        for i in 7..=24u8 {
            atlas.row(&grad(i));
        }
        assert!(atlas.rows() > RampAtlas::INITIAL_ROWS, "the frame doubled the rows");
        backend.sync(&atlas);
        assert_eq!(backend.fulls, 3, "the doubling is a full upload");

        // Frames 6-7: only three ramps drawn, then a compaction. Rows move to
        // new indices — the backend must be sent everything, never the rows
        // that happen to have been copied.
        atlas.begin_frame();
        for i in 1..=3u8 {
            atlas.row(&grad(i));
        }
        atlas.begin_frame();
        assert!(atlas.len() < 24, "the compaction must have run");
        backend.sync(&atlas);
        assert_eq!(backend.fulls, 4, "the compaction is a full upload");

        // The frame after it is a row again, on the compacted atlas.
        let before = backend.rows_sent;
        atlas.begin_frame();
        atlas.row(&grad(25));
        backend.sync(&atlas);
        assert_eq!(backend.rows_sent - before, 1, "a new ramp after the compaction is its own row");
        assert_eq!(backend.texels, atlas.texels());
    }
}
