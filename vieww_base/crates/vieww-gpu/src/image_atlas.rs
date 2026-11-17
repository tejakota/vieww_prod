//! Persistent RGBA8 atlas for decoded [`vieww_foundation::Image`] values.
//!
//! Images are keyed by the identity of their shared pixel allocation, matching
//! `Image`'s own equality semantics. The atlas is deliberately simple and
//! frame-stable: it grows and repacks between frames, never evicts while a
//! `ScenePlan` is being built, and exposes a generation so a backend uploads
//! only when pixels or packing changed — and *which rectangles* changed, via
//! [`ImageAtlas::changes_since`], so a frame that decodes one image sends that
//! image's texels instead of the whole (up to 64 MB) texture.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use vieww_foundation::Image;

use crate::atlas::AtlasSlot;
use crate::dirty::{AtlasChanges, DirtyRects};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageKey(u64);

impl ImageKey {
    #[must_use]
    pub fn of(image: &Image) -> Self {
        // `Image` equality is pointer identity for the shared pixel buffer.
        // The allocation is kept alive by `image` for the duration of planning,
        // so using its address as the cache identity is safe and collision-free
        // for live images in this process.
        Self(Arc::as_ptr(image.shared()) as usize as u64)
    }
}

#[derive(Debug, Clone, Copy)]
struct Placement {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageSlot {
    /// Texel top-left inside the atlas. Stable for the atlas's lifetime.
    pub x: u32,
    pub y: u32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub width: u32,
    pub height: u32,
}

impl From<ImageSlot> for AtlasSlot {
    fn from(slot: ImageSlot) -> Self {
        Self {
            x: slot.x,
            y: slot.y,
            u0: slot.u0,
            v0: slot.v0,
            u1: slot.u1,
            v1: slot.v1,
            width: slot.width,
            height: slot.height,
        }
    }
}

#[derive(Debug)]
pub struct ImageAtlas {
    side: u32,
    texels: Vec<u8>,
    entries: HashMap<ImageKey, Entry>,
    pen_x: u32,
    shelf_y: u32,
    shelf_height: u32,
    /// The version and this frame's dirty rectangles — see [`DirtyRects`].
    dirty: DirtyRects,
    frame: u64,
    used_texels: u64,
}

/// One resident image.
///
/// # Why a `Weak`, and why this used to be wrong
///
/// [`ImageKey`] is the image's `Arc` address. An address is only an identity
/// while the allocation is alive: once the image is dropped, the allocator is
/// free to hand the same address to the *next* image, and the atlas then
/// answered that image with the previous one's texels. A scene that builds a
/// fresh `Image` every frame (a video frame, a regenerated chart) hit it at
/// allocator whim — `test-gpu-work` rendered its minified photo from a stale
/// mip level on roughly one run in three.
///
/// Holding a `Weak` fixes both halves: while the entry exists the `ArcInner`
/// allocation cannot be freed, so its address cannot be reused; and a dead
/// `Weak` marks the entry stale, so it is replaced rather than trusted.
#[derive(Debug, Clone)]
struct Entry {
    placement: Placement,
    image: Weak<Vec<u8>>,
    last_used: u64,
}

impl Entry {
    fn is_alive(&self) -> bool {
        self.image.strong_count() > 0
    }
}

impl Default for ImageAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageAtlas {
    pub const INITIAL_SIDE: u32 = 512;
    pub const MAX_SIDE: u32 = 4096;
    const PAD: u32 = 1;

    #[must_use]
    pub fn new() -> Self {
        let side = Self::INITIAL_SIDE;
        Self {
            side,
            texels: vec![0; (side * side * 4) as usize],
            entries: HashMap::new(),
            pen_x: 0,
            shelf_y: 0,
            shelf_height: 0,
            dirty: DirtyRects::new(),
            frame: 1,
            used_texels: 0,
        }
    }

    /// Start a frame: drop dead images, and when more than half the texture is
    /// in use, rebuild it with only the images the previous frame drew.
    ///
    /// Entries never move *within* a frame — vertices hold their texel
    /// positions — so this is the only place eviction happens.
    pub fn begin_frame(&mut self) {
        self.dirty.begin_frame();
        let previous = self.frame;
        self.frame += 1;
        // Dead images are forgotten immediately — which releases their `Weak`
        // and so their address — but their texels stay where they are until a
        // compaction reclaims the space. Repacking here would re-upload the
        // whole texture on every frame of a scene that makes a new image per
        // frame, which is exactly the scene that produces dead entries.
        self.entries.retain(|_, e| e.is_alive());
        let capacity = u64::from(self.side) * u64::from(self.side);
        if self.used_texels * 2 <= capacity {
            return;
        }
        let keep: Vec<(ImageKey, Entry, Vec<u8>)> = self
            .entries
            .iter()
            .filter(|(_, e)| e.last_used >= previous)
            .map(|(k, e)| (*k, e.clone(), self.copy_out(e.placement)))
            .collect();
        // Everything from here on rewrites the texture wholesale, so this
        // frame's difference is "all of it" — a backend cannot be told that
        // only the re-placed images moved, because the reset cleared texels
        // the rectangles would not cover.
        self.dirty.rewrote();
        self.texels.iter_mut().for_each(|t| *t = 0);
        self.entries.clear();
        self.pen_x = 0;
        self.shelf_y = 0;
        self.shelf_height = 0;
        self.used_texels = 0;
        for (key, entry, bytes) in keep {
            if let Some(placement) =
                self.try_allocate(entry.placement.width, entry.placement.height)
            {
                self.blit(placement, &bytes);
                self.replicate_padding(placement);
                self.used_texels += u64::from(placement.width) * u64::from(placement.height);
                self.entries.insert(key, Entry { placement, ..entry });
            }
        }
    }

    fn copy_out(&self, p: Placement) -> Vec<u8> {
        let mut out = Vec::with_capacity((p.width * p.height * 4) as usize);
        for row in 0..p.height {
            let from = ((p.y + row) * self.side * 4 + p.x * 4) as usize;
            out.extend_from_slice(&self.texels[from..from + (p.width * 4) as usize]);
        }
        out
    }

    #[must_use]
    pub const fn side(&self) -> u32 {
        self.side
    }
    #[must_use]
    pub fn texels(&self) -> &[u8] {
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
    /// masks. An insert's rectangle is the image plus its one-texel padding
    /// ring (linear sampling reads it, so it changes with the image); a
    /// compaction or a grow is a full upload, as is any backend that missed a
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

    /// The slot for a resident, still-alive image.
    #[must_use]
    pub fn get(&self, key: &ImageKey) -> Option<ImageSlot> {
        self.entries
            .get(key)
            .filter(|e| e.is_alive())
            .map(|e| self.slot(e.placement))
    }

    pub fn insert(&mut self, image: &Image) -> Option<ImageSlot> {
        let key = ImageKey::of(image);
        let frame = self.frame;
        if let Some(existing) = self.entries.get_mut(&key) {
            if existing
                .image
                .upgrade()
                .is_some_and(|live| Arc::ptr_eq(&live, image.shared()))
            {
                existing.last_used = frame;
                let placement = existing.placement;
                return Some(self.slot(placement));
            }
            // A dead image whose address the allocator reused. Its texels
            // are not this image's; drop the entry (its space is reclaimed at
            // the next compaction) and place the new image fresh.
            self.entries.remove(&key);
        }
        let w = image.width();
        let h = image.height();
        if w == 0
            || h == 0
            || w.saturating_add(Self::PAD * 2) > Self::MAX_SIDE
            || h.saturating_add(Self::PAD * 2) > Self::MAX_SIDE
        {
            return None;
        }
        let placement = self.allocate(w, h)?;
        self.blit(placement, image.pixels());
        self.used_texels += u64::from(w) * u64::from(h);
        self.entries.insert(
            key,
            Entry {
                placement,
                image: Arc::downgrade(image.shared()),
                last_used: frame,
            },
        );
        Some(self.slot(placement))
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<Placement> {
        loop {
            if let Some(p) = self.try_allocate(w, h) {
                return Some(p);
            }
            if self.side >= Self::MAX_SIDE {
                return None;
            }
            self.grow();
        }
    }
    fn try_allocate(&mut self, w: u32, h: u32) -> Option<Placement> {
        let total_w = w.saturating_add(Self::PAD * 2);
        let total_h = h.saturating_add(Self::PAD * 2);
        if self.pen_x.saturating_add(total_w) > self.side {
            let next = self
                .shelf_y
                .saturating_add(self.shelf_height)
                .saturating_add(Self::PAD);
            if next.saturating_add(total_h) > self.side {
                return None;
            }
            self.shelf_y = next;
            self.pen_x = 0;
            self.shelf_height = 0;
        }
        if self.shelf_y.saturating_add(total_h) > self.side {
            return None;
        }
        let p = Placement {
            x: self.pen_x + Self::PAD,
            y: self.shelf_y + Self::PAD,
            width: w,
            height: h,
        };
        self.pen_x = self.pen_x.saturating_add(total_w);
        self.shelf_height = self.shelf_height.max(total_h);
        Some(p)
    }
    /// Double the side, keeping every image at the texel it already had —
    /// see `Atlas::grow` for why a frame's earlier vertices need that.
    fn grow(&mut self) {
        let old_side = self.side;
        let old = std::mem::take(&mut self.texels);
        self.side = (old_side * 2).min(Self::MAX_SIDE);
        self.texels = vec![0; (self.side * self.side * 4) as usize];
        for row in 0..old_side {
            let from = (row * old_side * 4) as usize;
            let to = (row * self.side * 4) as usize;
            let n = (old_side * 4) as usize;
            self.texels[to..to + n].copy_from_slice(&old[from..from + n]);
        }
        // A resize is a full upload for every backend — the texture must be
        // recreated — so the frame's recording is abandoned rather than
        // appended to.
        self.dirty.rewrote();
    }
    fn blit(&mut self, p: Placement, src: &[u8]) {
        debug_assert_eq!(src.len(), (p.width * p.height * 4) as usize);
        // The recorded rectangle covers the padding ring too: blitting an
        // image writes its texels *and* replicates its edges one texel out
        // (below), and a backend that only took the image's own rectangle
        // would leave the ring holding the previous image's edge.
        self.dirty.blit(
            p.x - Self::PAD,
            p.y - Self::PAD,
            p.width + Self::PAD * 2,
            p.height + Self::PAD * 2,
        );
        for row in 0..p.height {
            let from = (row * p.width * 4) as usize;
            let to = ((p.y + row) * self.side * 4 + p.x * 4) as usize;
            let n = (p.width * 4) as usize;
            self.texels[to..to + n].copy_from_slice(&src[from..from + n]);
        }
        self.replicate_padding(p);
    }

    fn replicate_padding(&mut self, p: Placement) {
        if p.width == 0 || p.height == 0 {
            return;
        }
        let left = p.x - Self::PAD;
        let top = p.y - Self::PAD;
        let right = p.x + p.width;
        let bottom = p.y + p.height;
        for row in p.y..bottom {
            let src = ((row * self.side + p.x) * 4) as usize;
            let dst = ((row * self.side + left) * 4) as usize;
            let pixel = [
                self.texels[src],
                self.texels[src + 1],
                self.texels[src + 2],
                self.texels[src + 3],
            ];
            self.texels[dst..dst + 4].copy_from_slice(&pixel);
            let src_r = ((row * self.side + right - 1) * 4) as usize;
            let dst_r = ((row * self.side + right) * 4) as usize;
            let pixel = [
                self.texels[src_r],
                self.texels[src_r + 1],
                self.texels[src_r + 2],
                self.texels[src_r + 3],
            ];
            self.texels[dst_r..dst_r + 4].copy_from_slice(&pixel);
        }
        for x in left..=right {
            let src = ((p.y * self.side + x) * 4) as usize;
            let dst = ((top * self.side + x) * 4) as usize;
            let pixel = [
                self.texels[src],
                self.texels[src + 1],
                self.texels[src + 2],
                self.texels[src + 3],
            ];
            self.texels[dst..dst + 4].copy_from_slice(&pixel);
            let src_b = (((bottom - 1) * self.side + x) * 4) as usize;
            let dst_b = ((bottom * self.side + x) * 4) as usize;
            let pixel = [
                self.texels[src_b],
                self.texels[src_b + 1],
                self.texels[src_b + 2],
                self.texels[src_b + 3],
            ];
            self.texels[dst_b..dst_b + 4].copy_from_slice(&pixel);
        }
    }
    fn slot(&self, p: Placement) -> ImageSlot {
        let s = self.side as f32;
        // Linear filtering must never see the neighbouring image. Sample at
        // half-texel inset points for multi-pixel images; a 1x1 image uses its
        // centre point, producing a non-zero span while staying inside itself.
        let inset = |size: u32| if size <= 1 { 0.25 } else { 0.5 };
        let left = p.x as f32 + inset(p.width);
        let top = p.y as f32 + inset(p.height);
        let right = p.x as f32 + p.width.max(1) as f32 - inset(p.width);
        let bottom = p.y as f32 + p.height.max(1) as f32 - inset(p.height);
        ImageSlot {
            x: p.x,
            y: p.y,
            u0: left / s,
            v0: top / s,
            u1: right / s,
            v1: bottom / s,
            width: p.width,
            height: p.height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The allocator reuses a freed image's address for the next image of the
    /// same size far more often than chance. The atlas must answer the new
    /// image with the new image's texels.
    #[test]
    fn a_new_image_at_a_dead_images_address_is_not_served_its_texels() {
        let mut atlas = ImageAtlas::new();
        let mut reused = 0;
        for i in 0..200u32 {
            let value = (i % 250) as u8;
            let img = image(8, 8, [value, 0, 0, 255]);
            let slot = atlas.insert(&img).expect("fits");
            let at = ((slot.y * atlas.side() + slot.x) * 4) as usize;
            assert_eq!(
                atlas.texels()[at],
                value,
                "image {i} was served another image's texels"
            );
            if i > 0 && slot.x == 0 {
                reused += 1;
            }
            drop(img);
            atlas.begin_frame();
        }
        let _ = reused;
    }

    #[test]
    fn compaction_keeps_last_frames_images_and_reclaims_the_rest() {
        let mut atlas = ImageAtlas::new();
        // Four 400x400 images in a 1024x1024 texture: over half full, so the
        // next frame compacts.
        let big: Vec<Image> = (0..4).map(|i| image(400, 400, [i, 1, 2, 255])).collect();
        for img in &big {
            atlas.insert(img).expect("fits");
        }
        atlas.begin_frame();
        // Only the last image is drawn this frame; the rest stay alive.
        let kept = atlas.insert(&big[3]).expect("resident");
        let _ = kept;
        atlas.begin_frame();
        assert_eq!(
            atlas.len(),
            1,
            "only the image drawn last frame survives compaction"
        );
        let slot = atlas.get(&ImageKey::of(&big[3])).expect("still resident");
        let at = ((slot.y * atlas.side() + slot.x) * 4) as usize;
        assert_eq!(atlas.texels()[at], 3);
    }

    fn image(width: u32, height: u32, rgba: [u8; 4]) -> Image {
        Image::from_rgba8(rgba.repeat((width * height) as usize), width, height)
    }

    #[test]
    fn inserts_an_image_once_and_reuses_its_identity() {
        let img = image(2, 2, [10, 20, 30, 255]);
        let mut atlas = ImageAtlas::new();
        let a = atlas.insert(&img).expect("fits");
        let version = atlas.version();
        let b = atlas.insert(&img).expect("same image remains resident");
        assert_eq!(a, b);
        assert_eq!(atlas.len(), 1);
        assert_eq!(atlas.version(), version);
    }

    #[test]
    fn growth_preserves_existing_pixels() {
        let first = image(400, 400, [1, 2, 3, 255]);
        let second = image(400, 400, [7, 8, 9, 255]);
        let mut atlas = ImageAtlas::new();
        atlas.insert(&first).expect("first fits");
        atlas
            .insert(&second)
            .expect("second triggers shelf/growth as needed");
        let slot = atlas
            .get(&ImageKey::of(&first))
            .expect("first survived growth");
        assert_eq!(slot.width, 400);
        assert_eq!(slot.height, 400);
        let x = ((slot.u0 * atlas.side() as f32) - 0.5).round() as usize;
        let y = ((slot.v0 * atlas.side() as f32) - 0.5).round() as usize;
        let offset = (y * atlas.side() as usize + x) * 4;
        assert_eq!(&atlas.texels()[offset..offset + 4], &[1, 2, 3, 255]);
    }

    #[test]
    fn one_pixel_images_have_a_stable_nonzero_sampling_region() {
        let img = image(1, 1, [255, 128, 64, 255]);
        let mut atlas = ImageAtlas::new();
        let slot = atlas.insert(&img).expect("fits");
        assert!(slot.u1 > slot.u0);
        assert!(slot.v1 > slot.v0);
    }

    #[test]
    fn adjacent_images_get_edge_replicated_padding_for_linear_sampling() {
        let red = image(4, 4, [255, 0, 0, 255]);
        let blue = image(4, 4, [0, 0, 255, 255]);
        let mut atlas = ImageAtlas::new();
        let first = atlas.insert(&red).expect("red fits");
        let second = atlas.insert(&blue).expect("blue fits beside red");
        assert!(second.u0 > first.u1, "images are separated by padding");

        let sample_x = |slot: ImageSlot, edge: bool| {
            let px = if edge {
                slot.u1 * atlas.side() as f32
            } else {
                slot.u0 * atlas.side() as f32
            };
            px.floor() as usize
        };
        let y = (first.v0 * atlas.side() as f32).floor() as usize;
        let row = y * atlas.side() as usize * 4;
        let right = sample_x(first, true).saturating_sub(1) * 4;
        let left = sample_x(second, false).saturating_sub(1) * 4;
        assert_eq!(
            &atlas.texels()[row + right..row + right + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &atlas.texels()[row + left..row + left + 4],
            &[0, 0, 255, 255]
        );
    }

    // ------------------------------------------------- partial uploads

    /// A "texture" the way a backend holds one: the texels it was last sent,
    /// replayed exactly as `changes_since` prescribes.
    #[derive(Default)]
    struct Backend {
        texels: Vec<u8>,
        held: Option<u64>,
        fulls: u32,
        region_bytes: u64,
    }

    impl Backend {
        /// Apply what `changes_since` says to send, and check the result
        /// equals the atlas — the replay every partial-upload backend
        /// performs, held to byte identity after every frame.
        fn sync(&mut self, atlas: &ImageAtlas) {
            let side = atlas.side() as usize;
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
                            let i = (row as usize * side + x as usize) * 4;
                            let n = w as usize * 4;
                            self.texels[i..i + n]
                                .copy_from_slice(&atlas.texels()[i..i + n]);
                            self.region_bytes += n as u64;
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
    /// atlas after every sync — the property `vieww-hal`'s partial image
    /// uploads are held to. The padding ring is the point: an insert's
    /// rectangle has to cover the one-texel ring the blit writes by edge
    /// replication, or a backend would hold the ring as it was before the
    /// image. Runs the atlas through a first upload, a frame the backend
    /// skips, a growth, and a compaction.
    #[test]
    fn changes_since_replayed_leaves_a_texture_equal_to_the_atlas() {
        let mut atlas = ImageAtlas::new();
        let mut backend = Backend::default();

        // Frame 1: two adjacent images. The first upload is a full one.
        atlas.begin_frame();
        let red = image(4, 4, [255, 0, 0, 255]);
        let blue = image(4, 4, [0, 0, 255, 255]);
        atlas.insert(&red);
        atlas.insert(&blue);
        backend.sync(&atlas);
        assert_eq!(backend.fulls, 1);

        // Frame 2: one new image. Only its rectangle goes: the image plus its
        // ring, 6x6 of RGBA8, against a whole 512x512 texture of 1 MiB.
        let before = backend.region_bytes;
        atlas.begin_frame();
        let green = image(4, 4, [0, 255, 0, 255]);
        atlas.insert(&green);
        backend.sync(&atlas);
        assert_eq!(
            backend.region_bytes - before,
            6 * 6 * 4,
            "the rectangle is the image plus its padding ring"
        );

        // Frames 3-4: the backend skips a frame; the one after is a full one.
        drop((red, blue, green)); // their texels are reclaimed at the compaction
        atlas.begin_frame();
        let _ = atlas.insert(&image(6, 6, [9, 9, 9, 255]));
        atlas.begin_frame();
        atlas.insert(&image(6, 6, [1, 2, 3, 255]));
        backend.sync(&atlas);
        assert_eq!(backend.fulls, 2);

        // Frames 5-7: four big images past the half line, then a compaction
        // that keeps only the one drawn last — a full upload, never the
        // re-placed images as rectangles.
        let big: Vec<Image> = (0..4).map(|i| image(400, 400, [i, 1, 2, 255])).collect();
        for img in &big {
            atlas.insert(img);
        }
        backend.sync(&atlas);
        atlas.begin_frame(); // the all-live rewrite; same content, new version
        atlas.insert(&big[3]);
        atlas.begin_frame(); // the compaction: only big[3] survives
        assert_eq!(
            atlas.len(),
            1,
            "only the image drawn last frame survives compaction"
        );
        backend.sync(&atlas);
        assert_eq!(backend.fulls, 4, "the growth and the compaction are fulls");

        // The frame after it is a rectangle again, on the compacted atlas.
        let before = backend.region_bytes;
        atlas.begin_frame();
        atlas.insert(&image(3, 3, [7, 7, 7, 255]));
        backend.sync(&atlas);
        assert_eq!(
            backend.region_bytes - before,
            5 * 5 * 4,
            "a 3x3 image plus its ring is 5x5 of RGBA8"
        );
        assert_eq!(backend.texels, atlas.texels());
    }
}
