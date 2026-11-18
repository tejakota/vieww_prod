//! Which texels of an atlas changed, so a backend uploads those and not the
//! whole texture.
//!
//! # The problem this module exists for
//!
//! Every planner atlas used to re-send its whole texture whenever its version
//! moved. A version moves when *anything* is packed, and the atlases pack at
//! the granularity of one glyph, one image, one gradient ramp — so a frame
//! that rasterised a single new letter paid for the entire glyph atlas: up to
//! 4096x4096 of `R8`, 16 MB, for a few hundred bytes of coverage. The image
//! atlas is four bytes a texel (64 MB at the same side) and the ramp atlas
//! sixteen (a full row of stops is 4 KB, the whole texture 4 KB times its
//! row count). A steady frame uploads nothing at all — no version moves — so
//! the cost lands on exactly the frames that are *almost* steady, which is
//! what real interfaces are: one new line of text scrolling in, one
//! regenerated chart thumbnail, one animated gradient.
//!
//! # The scheme, and why it is the mask atlas'
//!
//! [`MaskAtlas`](crate::MaskAtlas) pioneered this and its numbers are the
//! argument: an animated shape is a new mask every frame, and whole-texture
//! uploads made that 4.2 MB a frame for one rotating panel against masks of a
//! few hundred kilobytes. Its scheme is recorded here once, unchanged, so the
//! other three atlases can share it instead of copying it:
//!
//! - The atlas keeps a [`DirtyRects`]: the current [`version`](DirtyRects::version),
//!   the rectangles written since the frame began, and a flag for "this frame
//!   rewrote the texture wholesale".
//! - [`begin_frame`](DirtyRects::begin_frame) starts a frame's recording:
//!   rectangles are cleared, and the base — the version a backend must hold
//!   for the rectangles to describe its difference — is set to the current
//!   version.
//! - A rect-bounded write ([`blit`](DirtyRects::blit)) appends its rectangle and
//!   bumps the version.
//! - A wholesale rewrite ([`rewrote`](DirtyRects::rewrote)) — a compaction, a grow —
//!   bumps the version and sets the flag: the rectangles no longer describe
//!   the difference.
//! - [`changes_since`](DirtyRects::changes_since) turns what a backend holds into
//!   what it must be sent: nothing, the rectangles, or everything.
//!
//! The mask atlas keeps its own name for the answer —
//! [`MaskChanges`](crate::MaskChanges) — because its backends were written
//! against it; it is [`AtlasChanges`].

use crate::generation;

/// What a backend holding some earlier version of an atlas must upload to
/// match it — the answer to "which texels changed", so that a frame that
/// packs one new glyph, image or ramp costs its own bytes instead of the
/// whole texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasChanges<'a> {
    /// Nothing: the texture is current.
    None,
    /// Only these rectangles (`x, y, width, height`, in texels) differ from
    /// the version the backend holds.
    Regions(&'a [[u32; 4]]),
    /// Upload everything — the texture was rebuilt or resized, or the
    /// backend's copy is from before this frame began.
    Full,
}

/// One atlas' version and the rectangles written to it since the frame began
/// — the recorder half of the scheme in the [module doc](self), shared by
/// every planner atlas. See [`MaskAtlas`](crate::MaskAtlas), which this was
/// factored out of.
#[derive(Debug, Clone)]
pub struct DirtyRects {
    /// The atlas version; changes whenever the texels do.
    version: u64,
    /// Rectangles (`x, y, width, height`), in texels, blitted since the
    /// [`begin_frame`](Self::begin_frame) of the frame in progress — what a
    /// texture already holding `base` needs to catch up. See
    /// [`changes_since`](Self::changes_since).
    rects: Vec<[u32; 4]>,
    /// The version the texels had when `rects` started recording.
    base: u64,
    /// Set when this frame rewrote the texture wholesale (a compaction or a
    /// grow): `rects` no longer describes the difference.
    full: bool,
}

impl Default for DirtyRects {
    fn default() -> Self {
        Self::new()
    }
}

impl DirtyRects {
    #[must_use]
    pub fn new() -> Self {
        let version = generation::next();
        Self {
            version,
            rects: Vec::new(),
            base: version,
            // Nothing has been recorded yet, so a backend holding any version
            // at all — there are none, but the first frame is about to make
            // the question real — must be sent everything. This is also the
            // state `rewrote` leaves, for the same reason.
            full: true,
        }
    }

    /// Changes whenever the texels did; a backend re-uploads on a change.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Start a frame's recording: nothing has been written yet, so a backend
    /// holding the current version is current, and every write from here
    /// until the next `begin_frame` is a rectangle (or a
    /// [`rewrote`](Self::rewrote)).
    pub fn begin_frame(&mut self) {
        self.rects.clear();
        self.base = self.version;
        self.full = false;
    }

    /// A rect-bounded write — one glyph, image or ramp row placed at
    /// `x, y` — recorded for [`changes_since`](Self::changes_since) and
    /// marked by a version bump.
    ///
    /// Not recorded after a [`rewrote`](Self::rewrote) in the same frame: the
    /// rewrite already forced a full upload, which carries this rectangle's
    /// texels with it. The version bump still happens — the texels did
    /// change.
    pub fn blit(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if !self.full {
            self.rects.push([x, y, width, height]);
        }
        self.version = generation::next();
    }

    /// The texture was rewritten wholesale — a compaction re-placed
    /// everything — so the rectangles recorded this frame no longer describe
    /// any backend's difference. Marked with a version bump, like every
    /// change. A *grow* is not this: see [`grew`](Self::grew).
    pub fn rewrote(&mut self) {
        self.full = true;
        self.version = generation::next();
    }

    /// The texture **grew**: its side (or row count) doubled, every existing
    /// texel kept its position and value, and only the new space is
    /// unwritten. Unlike [`rewrote`](Self::rewrote) this leaves the frame's
    /// rectangles intact — they still describe the difference from `base`
    /// exactly, because nothing they cover moved — so a backend that holds
    /// `base` can recreate its texture at the new size by *carrying its own
    /// copy across* (one GPU image-to-image copy) and then sending
    /// the rectangles, instead of re-uploading the whole texture from the
    /// host. A grow after a [`rewrote`](Self::rewrote) in the same frame
    /// changes nothing: the frame is already a full rewrite, and `grew`
    /// deliberately does not clear that.
    pub fn grew(&mut self) {
        self.version = generation::next();
    }

    /// What a backend whose texture holds `held` (the
    /// [`version`](Self::version) it last uploaded, `None` if it has never
    /// uploaded) must be sent to match.
    ///
    /// # Why this is exact
    ///
    /// Texels change in exactly three places: a rect-bounded blit, a
    /// compaction, and a grow. `begin_frame` starts recording with the
    /// version the texels had at that moment; every blit after it appends its
    /// rectangle; a compaction marks the frame as a full rewrite; a grow
    /// bumps the version and *keeps* the rectangles, because it preserves
    /// every texel's position and value — so the rectangles still describe
    /// the whole difference from `base`, and a backend can carry its old
    /// texture across the resize itself. Anything else — a backend that
    /// skipped a frame, a first upload, a rebuilt texture — is
    /// [`AtlasChanges::Full`], which is always correct.
    #[must_use]
    pub fn changes_since(&self, held: Option<u64>) -> AtlasChanges<'_> {
        match held {
            Some(v) if v == self.version => AtlasChanges::None,
            Some(v) if v == self.base && !self.full => AtlasChanges::Regions(&self.rects),
            _ => AtlasChanges::Full,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIDE: u32 = 64;

    /// A "texture" the way a backend holds one: the texels it was last sent,
    /// replayed exactly as `changes_since` prescribes.
    #[derive(Default)]
    struct Backend {
        texels: Vec<u8>,
        held: Option<u64>,
        full_uploads: u32,
        region_bytes: u64,
    }

    impl Backend {
        /// Apply `changes` for an atlas whose texels are `atlas`, and check
        /// the result equals the atlas — the replay every backend performs,
        /// held to byte identity.
        fn sync(&mut self, atlas: &[u8], dirty: &DirtyRects, side: u32) {
            match dirty.changes_since(self.held) {
                AtlasChanges::None => {}
                AtlasChanges::Full => {
                    self.texels = atlas.to_vec();
                    self.full_uploads += 1;
                }
                AtlasChanges::Regions(rects) => {
                    assert_eq!(
                        self.texels.len(),
                        atlas.len(),
                        "a region upload never resizes"
                    );
                    for &[x, y, w, h] in rects {
                        for row in y..y + h {
                            let i = (row * side + x) as usize;
                            self.texels[i..i + w as usize]
                                .copy_from_slice(&atlas[i..i + w as usize]);
                            self.region_bytes += u64::from(w);
                        }
                    }
                }
            }
            self.held = Some(dirty.version());
            assert_eq!(
                self.texels, atlas,
                "the backend's texture must equal the atlas after syncing"
            );
        }
    }

    /// Paint `value` into `atlas` at `rect`, the way an atlas' own blit does.
    fn paint(atlas: &mut [u8], [x, y, w, h]: [u32; 4], value: u8) {
        for row in y..y + h {
            let i = (row * SIDE + x) as usize;
            atlas[i..i + w as usize].fill(value);
        }
    }

    /// The state machine across the situations a frame produces: a first
    /// upload, steady frames, rect-bounded writes, a wholesale rewrite, and a
    /// backend that skipped a frame — replayed onto a texture that must end
    /// up equal to the atlas every time it syncs.
    #[test]
    fn changes_since_brings_a_texture_exactly_up_to_date() {
        let mut dirty = DirtyRects::new();
        let mut backend = Backend::default();
        let mut atlas = vec![0u8; (SIDE * SIDE) as usize];

        // Frame 1: two rect-bounded writes, then the backend's first upload.
        dirty.begin_frame();
        dirty.blit(4, 6, 30, 20);
        paint(&mut atlas, [4, 6, 30, 20], 11);
        dirty.blit(40, 6, 12, 20);
        paint(&mut atlas, [40, 6, 12, 20], 12);
        backend.sync(&atlas, &dirty, SIDE);
        assert_eq!(backend.full_uploads, 1, "a first upload is a full one");

        // Frame 2: steady — the same version, nothing to send.
        dirty.begin_frame();
        backend.sync(&atlas, &dirty, SIDE);
        assert_eq!(backend.full_uploads, 1, "a steady frame sends nothing");

        // Frame 3: one new rectangle. Only it is sent.
        dirty.begin_frame();
        dirty.blit(4, 30, 8, 8);
        paint(&mut atlas, [4, 30, 8, 8], 13);
        assert_eq!(
            dirty.changes_since(backend.held),
            AtlasChanges::Regions(&[[4, 30, 8, 8]]),
            "the new write's rectangle, and only it"
        );
        backend.sync(&atlas, &dirty, SIDE);
        assert_eq!(backend.full_uploads, 1, "only the first upload was full");

        // Frame 4: the atlas is rebuilt wholesale (a compaction or a grow).
        // Everything is sent, whatever the backend holds.
        dirty.begin_frame();
        dirty.rewrote();
        atlas.fill(9);
        assert_eq!(dirty.changes_since(backend.held), AtlasChanges::Full);
        backend.sync(&atlas, &dirty, SIDE);
        assert_eq!(backend.full_uploads, 2);

        // Frame 5: a write the backend is not shown (it skipped the frame).
        // The frame after it cannot be described by rectangles — the
        // backend's version predates that frame's base — so it is sent
        // everything, which is always correct.
        dirty.begin_frame();
        dirty.blit(50, 50, 4, 4);
        paint(&mut atlas, [50, 50, 4, 4], 14);
        dirty.begin_frame();
        dirty.blit(2, 2, 4, 4);
        paint(&mut atlas, [2, 2, 4, 4], 15);
        assert_eq!(dirty.changes_since(backend.held), AtlasChanges::Full);
        backend.sync(&atlas, &dirty, SIDE);
        assert_eq!(backend.full_uploads, 3);
        assert!(backend.region_bytes > 0, "region uploads happened at all");
    }

    /// A frame with no writes changes nothing — the scheme's whole premise is
    /// that `changes_since` can say *nothing*.
    #[test]
    fn a_frame_without_writes_changes_nothing() {
        let mut dirty = DirtyRects::new();
        dirty.begin_frame();
        dirty.blit(0, 0, 4, 4);
        let version = dirty.version();
        dirty.begin_frame();
        assert_eq!(dirty.version(), version);
        assert_eq!(dirty.changes_since(Some(version)), AtlasChanges::None);
    }

    /// The recording starts over each frame: a rectangle written last frame
    /// is not re-sent this frame.
    #[test]
    fn rectangles_are_cleared_at_the_frame_boundary() {
        let mut dirty = DirtyRects::new();
        dirty.begin_frame();
        dirty.blit(1, 1, 4, 4);
        let after_first = dirty.version();
        dirty.begin_frame();
        dirty.blit(8, 8, 2, 2);
        assert_eq!(
            dirty.changes_since(Some(after_first)),
            AtlasChanges::Regions(&[[8, 8, 2, 2]])
        );
    }
}
