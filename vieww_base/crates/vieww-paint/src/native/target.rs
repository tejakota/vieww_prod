//! The premultiplied working buffer every CPU draw operation writes into.
//!
//! One type serves the root frame buffer and every offscreen layer (spec
//! §7.1's layer pool, minus the pooling — see `docs/RENDERER-MIGRATION.md`
//! for that as tracked follow-up work): a `Target` is just premultiplied
//! RGBA32F pixels plus a size, and [`Target::composite_coverage`] is the one
//! place a shape's coverage mask, a paint colour and a [`BlendMode`] become a
//! pixel write — so a fill, a stroke, a glyph and a shadow patch all go
//! through the same compositing arithmetic [`super::color`] defines.

use vieww_foundation::BlendMode;

use super::color::{over, write_straight_u8, Premul};
use super::geometry::fill::{CoverageMask, LcdMaskView, MaskView, LCD_SUBPIXELS};
use super::gradient::DeviceRamp;
use super::linear::{blend_with_pipeline, ColorPipeline};

#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<Premul>,
    /// The coverage gamma, applied once per row into this scratch by the
    /// flat-fill fast path — one buffer, reused, so the SIMD and scalar
    /// loops both read the same transferred values and stay byte-identical.
    /// See [`super::linear::coverage_alpha`].
    scratch: Vec<f32>,
    /// Writes are confined to this rectangle, when one is set.
    ///
    /// # Why a write mask and not a smaller surface
    ///
    /// A damaged repaint has to restrict what reaches the screen to the
    /// damaged region — but it must **not** restrict what the drawing
    /// operations *think* they are drawing into. The two are different, and
    /// the difference is not subtle: a shadow is a silhouette blurred over
    /// its own patch, and a patch cut off at the edge of a damage region
    /// blurs to different values along that edge than the same patch cut off
    /// at the edge of the window. Same for a blurred layer, whose kernel
    /// reaches across the boundary by design.
    ///
    /// Shrinking the surface therefore produces a repaint that is *nearly*
    /// right — wrong only in a band a few pixels wide along each region edge,
    /// which is precisely the fault nobody sees in review and everybody sees
    /// as "the shadows look slightly wrong after you click something".
    ///
    /// So effects are computed at their true extent, against the real window,
    /// and this mask decides which of the resulting pixels are allowed to
    /// land. Found by `examples/fixtures`' interaction fixture, which
    /// compares a retained repaint against a full one on a real screen every
    /// time it runs.
    write_mask: Option<(i32, i32, i32, i32)>,
    /// The bounding box of every pixel written since this buffer was last
    /// cleared, as `(x0, y0, x1, y1)` half-open in this buffer's own
    /// coordinates. `None` means nothing has been written at all.
    ///
    /// # Why a layer needs to know what it touched
    ///
    /// A layer's buffer is sized to the *bounds it declared*, which is a
    /// bounding box of what might be drawn — a `Stack` with one small badge in
    /// a corner declares the whole stack. `PopLayer` then composited every
    /// pixel of that buffer down into the parent, reading and testing millions
    /// of fully transparent pixels to find the badge. Nested layers pay it
    /// again per level: `fixtures`' `00-layers-nested` (24 deep) spent 39 ms of
    /// a 16.7 ms budget almost entirely here.
    ///
    /// Tracking the union of what actually landed costs four comparisons per
    /// *draw call* — not per pixel — and turns that composite into a walk of
    /// the ink instead of a walk of the buffer.
    ///
    /// It is a bounding box, not a region: it can over-report (an L-shape
    /// reports its enclosing rectangle) and that is fine, because
    /// over-reporting only costs time. It must never *under*-report, which is
    /// why every method that writes a pixel updates it, and why the two
    /// filters — which read across the whole buffer — say so explicitly rather
    /// than relying on what was there before them.
    ink: Option<(i32, i32, i32, i32)>,
}

/// A covered shadow's skip: the two whole-pixel interior bands, each
/// `(x0, y0, x1, y1)` in device coordinates, and the origin that translates
/// device coordinates into the drawing buffer's. See `covered_bands` in
/// `native/reference.rs`.
type CoveredSkip = ([(i32, i32, i32, i32); 2], (i32, i32));

/// One `composite_flat` draw's per-row work, shared by the serial walk and
/// the row-parallel one so both are the same code.
#[derive(Clone, Copy)]
struct FlatRow<'t> {
    table: &'t [f32; super::linear::COVERAGE_LEVELS],
    color: Premul,
    mode: BlendMode,
    pipeline: ColorPipeline,
    normal: bool,
    replaces: bool,
}

/// Where a coverage row's run of full coverage is, if it has one worth
/// filling: `Some((lo, hi))` when `raw[lo..hi]` is all `>= 1.0`, at least 16
/// wide, with nothing full outside it.
fn full_run(raw: &[f32]) -> Option<(usize, usize)> {
    let span = raw.len();
    let lo = raw.iter().position(|&c| c >= 1.0).unwrap_or(span);
    let hi = raw.iter().rposition(|&c| c >= 1.0).map_or(lo, |i| i + 1);
    (hi - lo >= 16 && raw[lo..hi].iter().all(|&c| c >= 1.0)).then_some((lo, hi))
}

/// [`full_run`] for consecutive rows, remembered across rows that are the
/// *same slice*. A rectangle's middle band is served as one row with stride
/// zero (`BandedMask`), so every row of a card or a list row's background is
/// the same memory; scanning it three times a row was the largest part of
/// `composite_flat` on Market Watch's storm (callgrind, 2026-11-17: 12% of
/// the frame in the scans' iterator code). Same data, same answer: the bytes
/// cannot change.
///
/// It remembers the row's **inked span** too: the part between its first and
/// last pixel of coverage above zero. A stroke's row (a sparkline, a chart
/// line) is as wide as the stroke's bounding box and zero nearly everywhere,
/// and every one of those zeros went through the coverage transfer and the
/// blend to leave its pixel as it was. The reference loop
/// (`simd::over_flat_row_scalar`, which the SIMD rows match bit for bit)
/// skips a pixel whose coverage is `<= 0.0` without touching it, so cutting
/// those ends off first writes the same bytes.
#[derive(Default)]
struct RunMemo {
    row: Option<(*const f32, usize)>,
    /// `(inked span, full run within that span)`.
    found: Option<(Span, Span)>,
}

/// A half-open range of a row, if there is one.
type Span = Option<(usize, usize)>;

/// Whether a coverage value reaches its pixel: anything but `<= 0.0`
/// (`NaN` included, as the reference loop's `c <= 0.0` test lets it through).
fn reaches(c: f32) -> bool {
    c > 0.0 || c.is_nan()
}

/// Where `raw` has coverage above zero: `[first, last + 1)`, or `None`.
fn inked(raw: &[f32]) -> Span {
    let first = raw.iter().position(|&c| reaches(c))?;
    let last = raw.iter().rposition(|&c| reaches(c))?;
    Some((first, last + 1))
}

impl RunMemo {
    /// `raw`'s inked span, and its full run relative to that span.
    fn spans(&mut self, raw: &[f32]) -> (Span, Span) {
        let key = (raw.as_ptr(), raw.len());
        if self.row != Some(key) || self.found.is_none() {
            self.row = Some(key);
            let ink = inked(raw);
            let run = ink.and_then(|(a, b)| full_run(&raw[a..b]));
            self.found = Some((ink, run));
        }
        self.found.expect("just set")
    }
}

impl FlatRow<'_> {
    /// `color` through the coverage `raw` over `dst` (one row's visible
    /// span), with the row's full run looked up in `memo`.
    fn paint_with(
        &self,
        dst: &mut [Premul],
        raw: &[f32],
        scratch: &mut Vec<f32>,
        memo: &mut RunMemo,
    ) {
        let (table, color, replaces) = (self.table, self.color, self.replaces);
        // Only the inked span: outside it every coverage is `<= 0.0`, which
        // leaves its pixel untouched on every path below.
        let (ink, run) = memo.spans(raw);
        let Some((a, b)) = ink else {
            return;
        };
        let (dst, raw) = (&mut dst[a..b], &raw[a..b]);
        let span = raw.len();
        if self.normal {
            // **The covered middle skips the gamma.** A shape's row is
            // antialiased edges around a run of full coverage, and
            // `coverage_alpha` is the identity at and above 1.0 — so that run
            // is its own mask, and for an opaque colour (where
            // `over_flat_row` stores `color` at c >= 1) it is a fill. Mapping
            // it through the table into the scratch first was most of
            // `composite_flat`'s cost on a panel-heavy frame (callgrind,
            // `00-layers-nested`, FW-07). Same bytes: the edges still go
            // through the shared transfer.
            if let Some((lo, hi)) = run {
                for (from, to) in [(0, lo), (hi, span)] {
                    if from < to {
                        scratch.clear();
                        scratch.extend(
                            raw[from..to]
                                .iter()
                                .map(|c| super::linear::coverage_alpha_in(table, *c)),
                        );
                        super::simd::over_flat_row(&mut dst[from..to], scratch, color, replaces);
                    }
                }
                if replaces {
                    dst[lo..hi].fill(color);
                } else {
                    super::simd::over_flat_row(&mut dst[lo..hi], &raw[lo..hi], color, false);
                }
                return;
            }
        }
        // The coverage gamma, applied once per row into the scratch, so the
        // SIMD and scalar paths below read the same transferred values —
        // byte-identity survives because the transfer is shared, not because
        // each loop re-implements it.
        scratch.clear();
        scratch.extend(
            raw.iter()
                .map(|c| super::linear::coverage_alpha_in(table, *c)),
        );
        let mask = scratch.as_slice();
        // The normal blend in gamma space — nearly every draw — is the
        // explicit-SIMD row in `native/simd.rs`, byte-identical to the loop
        // below specialised to `normal`.
        if self.normal {
            super::simd::over_flat_row(dst, mask, color, replaces);
            return;
        }
        for (px, &c) in dst.iter_mut().zip(mask.iter()) {
            if c <= 0.0 {
                continue;
            }
            if replaces && c >= 1.0 {
                *px = color;
                continue;
            }
            // No `if c >= 1.0 { color }` fast path to skip `scaled`'s four
            // multiplies-by-one on a translucent full-coverage pixel: it was
            // tried, and it cost more than it saved. See `composite_layer`
            // for the measurement of the pair.
            let src = color.scaled(c);
            *px = blend_with_pipeline(self.pipeline, self.mode, src, *px);
        }
    }
}

/// `len` transparent pixels from a *zeroed* allocation.
///
/// `vec![Premul::TRANSPARENT; n]` writes every pixel (the zero-value
/// specialisation does not reach a user struct), and a layer buffer is
/// megabytes: `00-layers-nested` allocates 24 of them a frame, since the
/// pool's byte budget keeps only a few idle, and that fill was 6% of the
/// frame (callgrind, FW-07). A zeroed allocation of that size comes from
/// fresh pages the kernel already zeroed, and pages nothing draws on are
/// never touched at all. Same pixels, byte for byte: `TRANSPARENT` is four
/// `+0.0`, whose bit pattern is all zeros.
fn transparent_pixels(len: usize) -> Vec<Premul> {
    const _: () = assert!(std::mem::size_of::<Premul>() == 16);
    if len == 0 {
        return Vec::new();
    }
    let layout = std::alloc::Layout::array::<Premul>(len).expect("pixel buffer size overflows");
    // SAFETY: `layout` is non-zero-sized; a null return is handled.
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) }.cast::<Premul>();
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    // SAFETY: `ptr` is a fresh global allocation of exactly `layout`
    // (`len` `Premul`s, Premul's alignment), so it is valid for a `Vec` of
    // that capacity; every element is initialised, because `Premul` is
    // `repr(C)` of four `f32`s and all-zero bits are `+0.0` — which is
    // `Premul::TRANSPARENT`.
    unsafe { Vec::from_raw_parts(ptr, len, len) }
}

/// One gradient row's work: the shared body of `composite_gradient`'s
/// serial and row-parallel walks — the `FlatRow` split's discipline, so the
/// two cannot drift. The loop stays scalar by measurement; see
/// `composite_gradient`'s own comment for the number.
struct GradientRows<'a> {
    ramp: &'a DeviceRamp,
    coverage: MaskView<'a>,
    table: &'static [f32; super::linear::COVERAGE_LEVELS],
    dy_start: usize,
    dx_start: usize,
    x_left: usize,
    span: usize,
    normal: bool,
    pipeline: ColorPipeline,
    mode: BlendMode,
}

impl GradientRows<'_> {
    /// The band's rows, `base` being the first row's offset into the walk.
    fn paint(&self, band: &mut [Premul], width: usize, base: usize) {
        for (k, line) in band.chunks_exact_mut(width).enumerate() {
            let dy = self.dy_start + base + k;
            let y = self.coverage.y0 + dy as i32;
            let mask_row = self.coverage.row(dy as u32);
            // Once per row, not once per pixel.
            let row = self.ramp.row(y);
            let dst_span = &mut line[self.x_left..self.x_left + self.span];
            // **The opaque-interior skip** — a row of the shape's own
            // coverage that is all ones (the interior of a gradient panel,
            // most of its rows) needs neither the gamma table lookup nor
            // the scale, and every branch below has only the blend's own
            // answers to give. The scan costs one pass over the row's
            // coverage; an edge row pays it back as noise beside its own
            // table lookups.
            let interior = mask_row[self.dx_start..self.dx_start + self.span]
                .iter()
                .all(|&c| c >= 1.0);
            for (j, dst) in dst_span.iter_mut().enumerate() {
                let src = if interior {
                    self.ramp
                        .at(&row, self.coverage.x0 + self.dx_start as i32 + j as i32, y)
                } else {
                    // The coverage gamma — see `linear::coverage_alpha` — for
                    // the same reason the flat-fill loop applies it: coverage
                    // is area, alpha is light.
                    let c =
                        super::linear::coverage_alpha_in(self.table, mask_row[self.dx_start + j]);
                    if c <= 0.0 {
                        continue;
                    }
                    self.ramp
                        .at(&row, self.coverage.x0 + self.dx_start as i32 + j as i32, y)
                        .scaled(c)
                };
                if src.a <= 0.0 && self.normal {
                    continue;
                }
                if self.normal && src.a == 1.0 {
                    *dst = src;
                    continue;
                }
                // `normal` is `mode.is_normal() && GammaSpace`, decided once
                // at the call — so where it holds, the twenty-eight-way match
                // in `blend` and the pipeline match in `blend_with_pipeline`
                // have only one possible answer and are pure dispatch. See
                // `color::over`.
                *dst = if self.normal {
                    over(src, *dst)
                } else {
                    blend_with_pipeline(self.pipeline, self.mode, src, *dst)
                };
            }
        }
    }
}

/// One tinted row's work — the shared body of `composite_tinted_within`'s
/// serial and row-parallel walks, extracted so the two cannot drift (the
/// `FlatRow` discipline). The clip closure stays a *parameter* rather than a
/// field: `&dyn Fn` is not `Sync`, and the parallel arm never carries one.
struct TintRows<'a> {
    alpha: &'a [f32],
    patch_width: u32,
    x_start: i32,
    x_end: i32,
    y_start: i32,
    ox: i32,
    oy: i32,
    skip: Option<CoveredSkip>,
    tint: Premul,
    normal: bool,
    pipeline: ColorPipeline,
}

impl TintRows<'_> {
    /// The band's rows, `first` being the first row's offset into the walk.
    fn rows(
        &self,
        band: &mut [Premul],
        first: i32,
        row: &mut Vec<Premul>,
        coverage: Option<&dyn Fn(i32, i32) -> f32>,
        buf_width: usize,
    ) {
        for (k, line) in band.chunks_exact_mut(buf_width).enumerate() {
            let y = self.y_start + first + k as i32;
            // The run this row skips: the union of the skip bands containing
            // the row, which the bands' nested x-ranges make one contiguous
            // run. Patch-local, half-open, already clamped into the row.
            let mut skip_run: Option<(i32, i32)> = None;
            if let Some((bands, (ox_skip, oy_skip))) = self.skip {
                let ty = y + self.oy - oy_skip;
                let (mut lo, mut hi) = (i32::MAX, i32::MIN);
                let mut any = false;
                for &(x0, y0, x1, y1) in &bands {
                    if ty >= y0 && ty < y1 {
                        any = true;
                        lo = lo.min(x0);
                        hi = hi.max(x1);
                    }
                }
                if any && hi > lo {
                    // Device x into this buffer's coordinates, then into the
                    // patch's, and clamped into the row's span.
                    let lo = (lo - ox_skip - self.ox).max(self.x_start);
                    let hi = (hi - ox_skip - self.ox).min(self.x_end);
                    if hi > lo {
                        skip_run = Some((lo, hi));
                    }
                }
            }
            let segments: [(i32, i32); 3] = match skip_run {
                Some((lo, hi)) => [(self.x_start, lo), (hi, self.x_end), (0, 0)],
                None => [(self.x_start, self.x_end), (0, 0), (0, 0)],
            };
            for &(seg_start, seg_end) in segments.iter().filter(|&(s, e)| e > s) {
                row.clear();
                let src_row = y as usize * self.patch_width as usize;
                for x in seg_start..seg_end {
                    let mut pixel = self.tint.scaled(self.alpha[src_row + x as usize]);
                    if let Some(coverage) = coverage {
                        let c = coverage(x, y);
                        if c < 1.0 {
                            pixel = pixel.scaled(c);
                        }
                    }
                    row.push(pixel);
                }
                let dst = &mut line[(seg_start + self.ox) as usize..(seg_end + self.ox) as usize];
                if self.normal {
                    super::simd::over_layer_row(dst, row.as_slice(), 1.0, true);
                } else {
                    for (d, s) in dst.iter_mut().zip(row.iter()) {
                        if s.a <= 0.0 {
                            continue;
                        }
                        *d = blend_with_pipeline(self.pipeline, BlendMode::Normal, *s, *d);
                    }
                }
            }
        }
    }
}

/// `write_rgba8`'s per-chunk body: the chunk's byte rows converted from the
/// shared pixels, straight through `write_straight_u8` — the same whole-row
/// conversion the flat-element split ran.
fn rgba8_band_chunk(walk: &BandWalk<'_, u8, &[Premul]>, chunk: usize) {
    let (pointer, len) = walk.chunks[chunk].a;
    // SAFETY: this chunk's own disjoint block of the byte band, alive for
    // the pool call (see `BandWalk`'s impl).
    let bytes = unsafe { std::slice::from_raw_parts_mut(pointer, len) };
    let row_pixels = walk.a_row / 4;
    let first = walk.chunks[chunk].base * row_pixels;
    write_straight_u8(&walk.context[first..first + len / 4], bytes);
}

/// `write_rgba8_region`'s shared row body.
struct RegionRows<'a> {
    pixels: &'a [Premul],
    width: usize,
    x0: usize,
    span: usize,
}

impl RegionRows<'_> {
    /// `base` is the first row's offset into the region's bands.
    fn write_rows(&self, bytes: &mut [u8], base: usize, byte_row: usize) {
        for (k, out_line) in bytes.chunks_exact_mut(byte_row).enumerate() {
            let line = &self.pixels[(base + k) * self.width..][..self.width];
            write_straight_u8(
                &line[self.x0..self.x0 + self.span],
                &mut out_line[self.x0 * 4..self.x0 * 4 + self.span * 4],
            );
        }
    }
}

/// `write_rgba8_region`'s per-chunk body.
fn rgba8_region_band_chunk(walk: &BandWalk<'_, u8, &RegionRows<'_>>, chunk: usize) {
    let (pointer, len) = walk.chunks[chunk].a;
    // SAFETY: this chunk's own disjoint block of the byte band, alive for
    // the pool call (see `BandWalk`'s impl).
    let bytes = unsafe { std::slice::from_raw_parts_mut(pointer, len) };
    walk.context
        .write_rows(bytes, walk.chunks[chunk].base, walk.a_row);
}

/// `reset_region`'s per-chunk body: the chunk's rows filled.
fn clear_band_chunk(walk: &BandWalk<'_, Premul, (usize, usize, Premul)>, chunk: usize) {
    let (x0, span, color) = walk.context;
    let (pointer, len) = walk.chunks[chunk].a;
    // SAFETY: this chunk's own disjoint block of the band, alive for the
    // pool call (see `BandWalk`'s impl).
    let band = unsafe { std::slice::from_raw_parts_mut(pointer, len) };
    for line in band.chunks_exact_mut(walk.a_row) {
        line[x0..x0 + span].fill(color);
    }
}

/// `composite_layer_and_clear`'s per-chunk body: the take kernel over the
/// chunk's rows of both bands, the same per-row slicing the serial walk's
/// `take_rows` closure does.
fn take_band_chunk(
    walk: &BandWalk<'_, Premul, (f32, bool, usize, usize, usize, usize)>,
    chunk: usize,
) {
    let (alpha, opaque_copy, sx0, sx1, dx0, dx1) = walk.context;
    let (dst_pointer, dst_len) = walk.chunks[chunk].a;
    // SAFETY: this chunk's own disjoint blocks of the two bands, alive for
    // the pool call (see `BandWalk`'s impl).
    let dst_band = unsafe { std::slice::from_raw_parts_mut(dst_pointer, dst_len) };
    let src_band =
        unsafe { std::slice::from_raw_parts_mut(walk.chunks[chunk].b.0, walk.chunks[chunk].b.1) };
    for (d, s) in dst_band
        .chunks_exact_mut(walk.a_row)
        .zip(src_band.chunks_exact_mut(walk.b_row))
    {
        super::simd::over_layer_row_take(&mut d[dx0..dx1], &mut s[sx0..sx1], alpha, opaque_copy);
    }
}

/// `composite_clipped_layer_and_clear`'s per-chunk body: as `take_band_chunk`
/// with the clip's coverage row gathered per row — the chunk's base row is
/// `y_start` plus the band offset, the absolute-row rule the receipt test
/// pins.
/// The clipped take's context, aliased for readability at the signature.
type ClippedTakeContext<'a> = (
    f32,
    bool,
    usize,
    usize,
    usize,
    usize,
    i32,
    &'a CoverageMask,
    (i32, i32),
);

fn clipped_take_band_chunk(walk: &BandWalk<'_, Premul, ClippedTakeContext<'_>>, chunk: usize) {
    let (alpha, opaque_copy, sx0, sx1, dx0, dx1, y_start, clip, clip_origin) = walk.context;
    let (dst_pointer, dst_len) = walk.chunks[chunk].a;
    // SAFETY: this chunk's own disjoint blocks of the two bands, alive for
    // the pool call (see `BandWalk`'s impl).
    let dst_band = unsafe { std::slice::from_raw_parts_mut(dst_pointer, dst_len) };
    let src_band =
        unsafe { std::slice::from_raw_parts_mut(walk.chunks[chunk].b.0, walk.chunks[chunk].b.1) };
    let x_start = sx0 as i32;
    let x_end = sx1 as i32;
    let mut cov = vec![0.0f32; sx1 - sx0];
    for (i, (d, s)) in dst_band
        .chunks_exact_mut(walk.a_row)
        .zip(src_band.chunks_exact_mut(walk.b_row))
        .enumerate()
    {
        let y = y_start + walk.chunks[chunk].base as i32 + i as i32;
        Target::clip_coverage_row(clip, clip_origin, y, x_start, x_end, &mut cov);
        let row = &mut s[sx0..sx1];
        for (px, &c) in row.iter_mut().zip(cov.iter()) {
            if c < 1.0 {
                *px = px.scaled(c);
            }
        }
        super::simd::over_layer_row_take(&mut d[dx0..dx1], row, alpha, opaque_copy);
    }
}

impl Target {
    #[must_use]
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: transparent_pixels((width as usize) * (height as usize)),
            write_mask: None,
            ink: None,
            scratch: Vec::new(),
        }
    }

    /// Confine every subsequent write to `region`, or lift the restriction.
    ///
    /// See the field's own documentation for why this exists rather than a
    /// smaller surface.
    pub(crate) fn set_write_mask(&mut self, region: Option<(i32, i32, i32, i32)>) {
        self.write_mask = region;
    }

    /// The region writes are confined to, when a damaged frame is replaying
    /// one: a caller that knows it need not compute what cannot land.
    pub(crate) const fn write_mask(&self) -> Option<(i32, i32, i32, i32)> {
        self.write_mask
    }

    /// The horizontal and vertical ranges a write may land in.
    fn writable(&self) -> (i32, i32, i32, i32) {
        match self.write_mask {
            Some((x0, y0, x1, y1)) => (
                x0.max(0),
                y0.max(0),
                x1.min(self.width as i32),
                y1.min(self.height as i32),
            ),
            None => (0, 0, self.width as i32, self.height as i32),
        }
    }

    /// Reset every pixel to fully transparent, keeping the buffer's own
    /// allocation — what [`TargetPool`](super::pool::TargetPool) calls on a
    /// buffer it is about to hand back out, so reuse costs a fill, not an
    /// allocation.
    pub(crate) fn clear(&mut self) {
        // **Only the part that was written.** A pooled buffer handed back out
        // is usually the same buffer that was just used, and what needs
        // resetting is what the last user actually drew — not the megabytes
        // around it that were already transparent. `ink` is exactly that
        // answer, and it is already being maintained for the composite path.
        if let Some((x0, y0, x1, y1)) = self.ink.take() {
            let width = self.width as usize;
            for y in y0.max(0)..y1.min(self.height as i32) {
                let row = y as usize * width;
                let lo = row + x0.max(0) as usize;
                let hi = row + (x1.min(self.width as i32)).max(0) as usize;
                if hi > lo {
                    self.pixels[lo..hi].fill(Premul::TRANSPARENT);
                }
            }
        }
    }

    /// Widen [`Self::ink`] to include the half-open box `(x0, y0, x1, y1)`.
    fn mark_ink(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        self.ink = Some(match self.ink {
            Some((ax0, ay0, ax1, ay1)) => (ax0.min(x0), ay0.min(y0), ax1.max(x1), ay1.max(y1)),
            None => (x0, y0, x1, y1),
        });
    }

    /// The same union as the private [`Self::mark_ink`], for a caller that
    /// walks rows itself and has therefore already trimmed the box to the
    /// pixels it actually wrote — `paint_image`, which slices by row and
    /// records its ink once at the end instead of per written pixel.
    pub(crate) fn mark_ink_box(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.mark_ink(x0, y0, x1, y1);
    }

    /// Carry a rectangle of this buffer's pixels to where it now belongs.
    ///
    /// The row-disciplined memmove: when the destination sits below the source
    /// the rows move bottom-up, and top-down otherwise, so a move whose source
    /// and destination overlap never reads a row an earlier row already wrote.
    /// Within a row, `copy_within` makes the same guarantee for the columns.
    ///
    /// Both rectangles are the same size — a
    /// [`CopyRegion`](super::super::CopyRegion) translates — and either side
    /// may run off this buffer. The rows and columns that move are the ones
    /// where **both** sides are inside: clipping source and destination
    /// independently would slide the carried content by the amount one side
    /// was trimmed, and a copy that lands its rows eight pixels from where
    /// they belong is worse than no copy at all.
    pub(crate) fn move_region(&mut self, src: (i32, i32, i32, i32), dst: (i32, i32, i32, i32)) {
        let (sx0, sy0, sx1, sy1) = src;
        let (dx0, dy0, dx1, dy1) = dst;
        let rows_total = (sy1 - sy0).min(dy1 - dy0);
        let cols_total = (sx1 - sx0).min(dx1 - dx0);
        let (w, h) = (self.width as i32, self.height as i32);
        // Row and column indices, relative to the rectangles' own origins,
        // where both the source and the destination are on this buffer.
        let r0 = (0 - sy0).max(0 - dy0).max(0);
        let r1 = (h - sy0).min(h - dy0).min(rows_total);
        let c0 = (0 - sx0).max(0 - dx0).max(0);
        let c1 = (w - sx0).min(w - dx0).min(cols_total);
        if r1 <= r0 || c1 <= c0 {
            return;
        }

        let width = self.width as usize;
        // Bottom-up when the rows move down: a row written early would be the
        // source of a later one. Within a row, `copy_within` is a memmove and
        // needs no help with columns.
        let descending = dy0 > sy0;
        for row in r0..r1 {
            let r = if descending { r1 - 1 - (row - r0) } else { row };
            let src_start = (sy0 + r) as usize * width + (sx0 + c0) as usize;
            let src_end = src_start + (c1 - c0) as usize;
            let dst_start = (dy0 + r) as usize * width + (dx0 + c0) as usize;
            self.pixels.copy_within(src_start..src_end, dst_start);
        }
        self.mark_ink(dx0 + c0, dy0 + r0, dx0 + c1, dy0 + r1);
    }

    /// Declare that every pixel of this buffer now counts as written — what
    /// the two layer filters do, since both read across the whole buffer and
    /// can turn a transparent pixel into a visible one.
    pub(crate) fn mark_all_ink(&mut self) {
        self.ink = Some((0, 0, self.width as i32, self.height as i32));
    }

    /// Grow the recorded ink outward by `margin` pixels, clamped to the
    /// buffer — what a blur does to the extent of what is visible in it.
    pub(crate) fn grow_ink(&mut self, margin: i32) {
        if let Some((x0, y0, x1, y1)) = self.ink {
            self.ink = Some((
                (x0 - margin).max(0),
                (y0 - margin).max(0),
                (x1 + margin).min(self.width as i32),
                (y1 + margin).min(self.height as i32),
            ));
        }
    }

    /// The box every written pixel of this buffer falls inside, if any.
    #[must_use]
    pub(crate) fn ink(&self) -> Option<(i32, i32, i32, i32)> {
        self.ink
    }

    /// A buffer every pixel of which is `color`.
    ///
    /// Test-only: `native/instancing.rs`'s tests composite an instanced draw
    /// onto a known ground and read the result back. Nothing in the renderer
    /// needs it — the root buffer is filled through `reset`, and a layer's
    /// buffer starts transparent — which is why it is gated rather than
    /// carrying an `#[allow(dead_code)]` that would also hide a real one.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn filled(width: u32, height: u32, color: Premul) -> Self {
        Self {
            width,
            height,
            pixels: vec![color; (width as usize) * (height as usize)],
            write_mask: None,
            ink: Some((0, 0, width as i32, height as i32)),
            scratch: Vec::new(),
        }
    }

    /// This buffer's own pixels, straight-alpha RGBA8, top row first.
    ///
    /// Test-only, for the reason [`Self::filled`] gives: the renderer's own
    /// output path is `write_rgba8` into a buffer it keeps between frames, so
    /// nothing outside a test wants a freshly allocated `Vec`.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn to_rgba8(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.pixels.len() * 4];
        self.write_rgba8(&mut out);
        out
    }

    /// Test-only with [`Self::get`]/[`Self::set`], its only callers.
    #[cfg(test)]
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            None
        } else {
            Some(y as usize * self.width as usize + x as usize)
        }
    }

    /// Test-only since the image walk went row-sliced: `paint_image` was the
    /// last production caller, and every remaining reader is a test that
    /// wants one pixel as an `Option`-free `Premul`. Kept (rather than
    /// rewritten) for the same reason [`Self::filled`] is — the tests read
    /// better for it.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn get(&self, x: i32, y: i32) -> Premul {
        self.index(x, y)
            .map_or(Premul::TRANSPARENT, |i| self.pixels[i])
    }

    /// Test-only with [`Self::get`]: the row walks mark their ink in boxes
    /// now, so nothing outside a test writes one pixel at a time.
    #[cfg(test)]
    pub(crate) fn set(&mut self, x: i32, y: i32, color: Premul) {
        if let Some(i) = self.index(x, y) {
            self.pixels[i] = color;
            self.mark_ink(x, y, x + 1, y + 1);
        }
    }

    /// Composite a shape into this buffer: `color_at(x, y)` gives the
    /// shape's own (unmasked) colour at **absolute device** coordinates
    /// (matching `coverage`, which every rasterizer entry point in this
    /// crate produces in device space regardless of which layer it is
    /// destined for), `coverage` scales it, and `mode` decides how it meets
    /// what's already there.
    ///
    /// `origin` is this buffer's own device-space top-left — `(0, 0)` for
    /// the root frame, a `PushLayer`'s device bounds for anything nested —
    /// and is what maps `coverage`'s absolute coordinates onto this
    /// buffer's own 0-based pixel array. Forgetting it is exactly the bug
    /// class this parameter exists to make impossible to forget: every
    /// caller has to say which space `coverage` was measured in relative to.
    pub(crate) fn composite_coverage(
        &mut self,
        coverage: MaskView<'_>,
        origin: (i32, i32),
        mode: BlendMode,
        pipeline: ColorPipeline,
        mut color_at: impl FnMut(i32, i32) -> Premul,
    ) {
        if coverage.is_empty() {
            return;
        }

        // # Why the bounds arithmetic is hoisted out of the loop
        //
        // This used to call `self.get(tx, ty)` and `self.set(tx, ty, …)` per
        // pixel, each of which recomputed `y * width + x` and re-checked the
        // pixel against all four edges of the buffer through an `Option`. Both
        // answers are the same for an entire row and knowable before the loop
        // starts: the mask's rectangle intersected with this buffer's own is a
        // range of rows and, within each, a range of columns. Clipping once
        // and then walking two slices removes four comparisons, a
        // multiplication and two `Option`s from every pixel of every fill,
        // stroke, glyph and shadow in the frame — which, at roughly a million
        // pixels a frame before overdraw, is not a micro-optimisation.
        //
        // It also removes the silent-clamp behaviour that `set` had: a write
        // outside the buffer was discarded pixel by pixel. Now it is excluded
        // by the ranges, which is the same result computed once.
        // The writable window is the buffer, narrowed to the write mask when
        // a damaged repaint has set one.
        let (wx0, wy0, wx1, wy1) = self.writable();
        let dy_start = (wy0 - (coverage.y0 - origin.1)).max(0);
        let dy_end = (wy1 - (coverage.y0 - origin.1)).min(coverage.height as i32);
        let dx_start = (wx0 - (coverage.x0 - origin.0)).max(0);
        let dx_end = (wx1 - (coverage.x0 - origin.0)).min(coverage.width as i32);
        if dy_start >= dy_end || dx_start >= dx_end {
            return;
        }

        let normal = mode.is_normal() && matches!(pipeline, ColorPipeline::GammaSpace);
        // The window this call may write into, recorded before the loop: it is
        // a bound on the ink, which is all `ink` promises to be.
        self.mark_ink(
            coverage.x0 - origin.0 + dx_start,
            coverage.y0 - origin.1 + dy_start,
            coverage.x0 - origin.0 + dx_end,
            coverage.y0 - origin.1 + dy_end,
        );

        let table = super::linear::coverage_table();
        for dy in dy_start..dy_end {
            let y = coverage.y0 + dy;
            let ty = y - origin.1;
            let mask_row = coverage.row(dy as u32);
            let pixel_row = ty as usize * self.width as usize;

            for dx in dx_start..dx_end {
                // The coverage gamma — see `linear::coverage_alpha`. The
                // full/empty short-circuits there keep the interior and the
                // skip paths untouched; only the antialiased fringe pays
                // the table lookup.
                let c = super::linear::coverage_alpha_in(table, mask_row[dx as usize]);
                if c <= 0.0 {
                    continue;
                }
                let x = coverage.x0 + dx;
                let src = color_at(x, y).scaled(c);
                if src.a <= 0.0 && mode.is_normal() {
                    continue;
                }
                let index = pixel_row + (x - origin.0) as usize;
                // **The opaque fast path.** A fully opaque source under a
                // normal blend in gamma space *replaces* the destination —
                // there is nothing to read, mix and write back. That is the
                // single most common pixel in any interface: the interior of
                // every solid fill, every panel, every icon. Taking it before
                // the general blend skips a load and a 28-way match per pixel.
                // Exactly 1.0, not `>=`: `over` with an alpha above one
                // produces `src + dst * (1 - a)` — a *negative* destination
                // term — so a pixel whose accumulated alpha overshot is not
                // the same as a straight replacement. At exactly one it is.
                if normal && src.a == 1.0 {
                    self.pixels[index] = src;
                    continue;
                }
                let dst = self.pixels[index];
                // `normal` is `mode.is_normal() && GammaSpace`, decided once
                // above — so where it holds, the twenty-eight-way match in
                // `blend` and the pipeline match in `blend_with_pipeline` have
                // only one possible answer and are pure dispatch. See
                // `color::over`.
                self.pixels[index] = if normal {
                    over(src, dst)
                } else {
                    blend_with_pipeline(pipeline, mode, src, dst)
                };
            }
        }
    }

    /// Per-channel — LCD subpixel — glyph compositing, for an
    /// [`LcdMaskView`]: the counterpart of [`composite_coverage`] for text
    /// rasterised at three times the horizontal resolution.
    ///
    /// # The contract: an opaque destination
    ///
    /// Each channel blends with its **own** coverage: a stem edge that covers
    /// the R stripe but not the B stripe pushes red in and keeps blue out.
    /// That is only meaningful over a destination whose alpha is already 1.0 —
    /// over anything translucent, the per-channel result carries fringes that
    /// a later blend would expose rather than resolve, which is why every
    /// engine that offers LCD text restricts it to opaque surfaces and why
    /// `reference.rs` only routes root-surface glyphs here (layer-surface
    /// glyphs fall back to gray coverage, keyed separately in the raster
    /// cache). The method takes the destination as it finds it and writes
    /// `a: dst.a` — the honest statement of "this blend does not change
    /// coverage" — which on the opaque destinations it is called on is 1.0
    /// before and after.
    ///
    /// # The degenerate case is the gray case, bit for bit
    ///
    /// A mask whose three channels are equal — a purely vertical edge, or the
    /// interior of any glyph — is a gray-coverage mask in disguise, and the
    /// equal-channel branch below runs the gray path's own
    /// `over(color.scaled(k), dst)` rather than the per-channel formula, so
    /// the two paths cannot drift apart on the pixels where they agree.
    pub(crate) fn composite_lcd(
        &mut self,
        coverage: LcdMaskView<'_>,
        origin: (i32, i32),
        color: vieww_foundation::Color,
        mut clip_at: impl FnMut(i32, i32) -> f32,
    ) {
        if coverage.is_empty() {
            return;
        }
        // The same hoisted window arithmetic as `composite_coverage`: the
        // mask's rectangle against this buffer's writable one, once, instead
        // of four comparisons and two `Option`s per pixel.
        let (wx0, wy0, wx1, wy1) = self.writable();
        let dy_start = (wy0 - (coverage.y0 - origin.1)).max(0);
        let dy_end = (wy1 - (coverage.y0 - origin.1)).min(coverage.height as i32);
        let dx_start = (wx0 - (coverage.x0 - origin.0)).max(0);
        let dx_end = (wx1 - (coverage.x0 - origin.0)).min(coverage.width as i32);
        if dy_start >= dy_end || dx_start >= dx_end {
            return;
        }

        self.mark_ink(
            coverage.x0 - origin.0 + dx_start,
            coverage.y0 - origin.1 + dy_start,
            coverage.x0 - origin.0 + dx_end,
            coverage.y0 - origin.1 + dy_end,
        );

        // The run's colour, premultiplied at full coverage. `scaled(k)` below
        // is the same helper the gray path scales its source with, so the
        // equal-channel branch and the gray path are one implementation.
        let c = Premul::from_straight(color);

        let table = super::linear::coverage_table();
        // Per row, each pixel lands in one of four classes, and the classes
        // come in runs: the glyph's interior is whole runs of full coverage,
        // its horizontal edges whole runs of equal fractional coverage, and
        // only the vertical stem edges — where LCD coverage is genuinely
        // split per channel — scatter as singles. So the row is classified
        // first (the three coverages, clip-multiplied, into scratch), then
        // emitted: interior runs as a fill, equal-channel runs through the
        // shared masked-row kernel — the gray path's own `over(c.scaled(k),
        // dst)` per pixel, lanes at a time — and split pixels as the
        // per-channel formula, unchanged. The class order is the original
        // loop's branch order, so a pixel's class is the branch it always
        // took.
        let span = (dx_end - dx_start) as usize;
        let mut kr_row = vec![0.0f32; span];
        let mut kg_row = vec![0.0f32; span];
        let mut kb_row = vec![0.0f32; span];
        let mut classes = vec![0u8; span];
        let gray_src = vec![c; span];
        for dy in dy_start..dy_end {
            let y = coverage.y0 + dy;
            let ty = y - origin.1;
            let mask_row = coverage.row(dy as u32);
            let pixel_row = ty as usize * self.width as usize;
            let dst_span = &mut self.pixels
                [pixel_row + (coverage.x0 - origin.0 + dx_start) as usize..][..span];

            // Classify.
            for (j, dx) in (dx_start..dx_end).enumerate() {
                let base = dx as usize * LCD_SUBPIXELS;
                let x = coverage.x0 + dx;
                // A clip is a mask over the whole pixel, not over individual
                // sub-columns, so it multiplies all three channels equally.
                let clip = clip_at(x, y);
                if clip <= 0.0 {
                    classes[j] = 0;
                    continue;
                }
                // The same coverage gamma as the gray path, per channel —
                // Skia's LCD pipeline gamma-corrects too. It is applied
                // *before* the clip multiplication, so a doubly-fractional
                // pixel (fringe coverage under a soft clip) is
                // `gamma(mask) * clip` — exactly the gray path's product —
                // and the equal-channel branch below stays bit-identical to
                // the gray compositor, clip and all.
                let kr = super::linear::coverage_alpha_in(table, mask_row[base]);
                let kg = super::linear::coverage_alpha_in(table, mask_row[base + 1]);
                let kb = super::linear::coverage_alpha_in(table, mask_row[base + 2]);
                let (kr, kg, kb) = if clip >= 1.0 {
                    (kr, kg, kb)
                } else {
                    (kr * clip, kg * clip, kb * clip)
                };
                kr_row[j] = kr;
                kg_row[j] = kg;
                kb_row[j] = kb;
                classes[j] = if kr <= 0.0 && kg <= 0.0 && kb <= 0.0 {
                    0
                } else if kr == 1.0 && kg == 1.0 && kb == 1.0 && c.a == 1.0 {
                    1
                } else if kr == kg && kg == kb {
                    2
                } else {
                    3
                };
            }

            // Emit, run by run.
            let mut j = 0;
            while j < span {
                let class = classes[j];
                let start = j;
                j += 1;
                while j < span && classes[j] == class {
                    j += 1;
                }
                let (dst_run, src_run, kr_run) = (
                    &mut dst_span[start..j],
                    &gray_src[start..j],
                    &kr_row[start..j],
                );
                match class {
                    // The original loop's `continue`: nothing lands.
                    0 => {}
                    // The opaque interior fast path, same licence as
                    // `composite_coverage`'s: full coverage on every channel,
                    // an opaque colour — there is nothing to read and blend.
                    1 => dst_run.fill(c),
                    // Equal channels: gray coverage in disguise. The gray
                    // path's own arithmetic — `over(c.scaled(k), dst)` —
                    // through the shared masked-row kernel, so the bytes
                    // agree with the gray compositor, clip and all.
                    2 => super::simd::over_masked_row(dst_run, src_run, kr_run),
                    // Genuinely split channels: each colour channel blends
                    // with its own effective alpha `e_ch = a * k_ch` — the
                    // source term `c_ch * k_ch` is exactly what
                    // `scaled(k_ch)` would give that channel, so this is the
                    // gray formula evaluated per channel rather than a new
                    // definition of blending.
                    _ => {
                        for (k, px) in dst_run.iter_mut().enumerate() {
                            let (kr, kg, kb) =
                                (kr_row[start + k], kg_row[start + k], kb_row[start + k]);
                            let dst = *px;
                            let e_r = c.a * kr;
                            let e_g = c.a * kg;
                            let e_b = c.a * kb;
                            *px = Premul {
                                r: c.r * kr + dst.r * (1.0 - e_r),
                                g: c.g * kg + dst.g * (1.0 - e_g),
                                b: c.b * kb + dst.b * (1.0 - e_b),
                                a: dst.a,
                            };
                        }
                    }
                }
            }
        }
    }

    /// [`composite_coverage`](Self::composite_coverage) for one flat colour
    /// with no clip mask over it.
    ///
    /// # Why this is a separate method rather than a branch inside the loop
    ///
    /// The general path takes a `FnMut(i32, i32) -> Premul` and calls it once
    /// per pixel. For a flat fill that closure's whole body is a clip lookup
    /// that always answers `1.0` and a match on a `None` gradient — work that
    /// is identical for every pixel of the shape and that the compiler cannot
    /// hoist, because it cannot see that the closure's answer does not vary.
    ///
    /// Splitting it out lets each case be what it is. Here the source colour is
    /// a constant, so the inner loop is a coverage load, one `scaled` and a
    /// blend — and, in the case that matters most, not even that: an opaque
    /// colour at full coverage is a store of a value already in a register.
    /// That is the interior of every solid panel, button face and icon path in
    /// the window, and it was previously paying an indirect call per pixel for
    /// the privilege.
    ///
    /// The result is bit-identical to the general path: the same arithmetic on
    /// the same numbers, with `mode` and `pipeline` reaching
    /// [`blend_with_pipeline`] unchanged for every pixel that is not the
    /// fully-covered opaque case — which the general path already
    /// special-cases, the same way.
    pub(crate) fn composite_flat(
        &mut self,
        coverage: MaskView<'_>,
        origin: (i32, i32),
        mode: BlendMode,
        pipeline: ColorPipeline,
        color: Premul,
    ) {
        if coverage.is_empty() || (color.a <= 0.0 && mode.is_normal()) {
            return;
        }
        let (wx0, wy0, wx1, wy1) = self.writable();
        let dy_start = (wy0 - (coverage.y0 - origin.1)).max(0);
        let dy_end = (wy1 - (coverage.y0 - origin.1)).min(coverage.height as i32);
        let dx_start = (wx0 - (coverage.x0 - origin.0)).max(0);
        let dx_end = (wx1 - (coverage.x0 - origin.0)).min(coverage.width as i32);
        if dy_start >= dy_end || dx_start >= dx_end {
            return;
        }

        let normal = mode.is_normal() && matches!(pipeline, ColorPipeline::GammaSpace);
        // An opaque colour under a normal blend replaces whatever it lands on,
        // wherever coverage is full — which is every pixel of a shape but its
        // antialiased outline. Hoisted, so the test per pixel is one bool.
        let replaces = normal && color.a == 1.0;
        self.mark_ink(
            coverage.x0 - origin.0 + dx_start,
            coverage.y0 - origin.1 + dy_start,
            coverage.x0 - origin.0 + dx_end,
            coverage.y0 - origin.1 + dy_end,
        );

        let table = super::linear::coverage_table();
        let row = FlatRow {
            table,
            color,
            mode,
            pipeline,
            normal,
            replaces,
        };
        // Both clamps above guarantee the span's left edge lands in the
        // buffer: `dx_start` is 0 (the offset itself was >= `wx0 >= 0`) or
        // exactly `wx0 - (coverage.x0 - origin.0)`, which cancels back to
        // `wx0`. Computed in `i32` before the cast — `coverage.x0 - origin.0`
        // is legitimately negative for a glyph overhanging its layer.
        let x_left = coverage.x0 - origin.0 + dx_start;
        let ty0 = coverage.y0 + dy_start - origin.1;
        debug_assert!(
            ty0 >= 0 && x_left >= 0,
            "clamped span must land inside the buffer"
        );
        let width = self.width as usize;
        let rows = (dy_end - dy_start) as usize;
        let span = (dx_end - dx_start) as usize;
        let (x_left, ty0) = (x_left as usize, ty0 as usize);
        let band = &mut self.pixels[ty0 * width..(ty0 + rows) * width];
        let paint_rows = |band: &mut [Premul], first: usize, scratch: &mut Vec<f32>| {
            let mut memo = RunMemo::default();
            for (k, line) in band.chunks_exact_mut(width).enumerate() {
                let dy = (dy_start as usize + first + k) as u32;
                let raw = &coverage.row(dy)[dx_start as usize..dx_end as usize];
                row.paint_with(&mut line[x_left..x_left + span], raw, scratch, &mut memo);
            }
        };

        // **Rows in parallel** for a big enough draw, on the rasterizer's
        // own worker count and size gate. A pixel's result depends only on
        // its own coverage and destination, so splitting the band changes
        // which thread computes it, never what — bytes are identical at any
        // worker count. On the 2-core/4-thread reference laptop this halves
        // the compositing half of a panel-heavy frame (FW-07). The bands run
        // on the raster pool since C-2 — no thread spawn per call.
        let workers = super::geometry::fill::composite_workers(span as u32, rows as u32);
        if workers <= 1 {
            let mut scratch = std::mem::take(&mut self.scratch);
            paint_rows(band, 0, &mut scratch);
            self.scratch = scratch;
            return;
        }
        // The pool (C-2) was tried here and measured worse on the
        // clip-heavy fixtures - 00-fills-rrect-clip 4.99 -> 5.66 ms best of
        // three, 00-fills-nested-clips 4.49 -> 5.22 - where a frame is
        // *many small* flat composites and each pool submission pays its
        // install/claim/retire handshake; the few-large-band walks (the
        // layer pops, the present's conversions, a damage region's clear)
        // are the ones the pool won (00-layers-nested 7.88 -> 6.74). This
        // walk stays on `std::thread::scope`: its calls are the many-small
        // kind.
        let paint_rows = |band: &mut [Premul], first: usize, scratch: &mut Vec<f32>| {
            let mut memo = RunMemo::default();
            for (k, line) in band.chunks_exact_mut(width).enumerate() {
                let dy = (dy_start as usize + first + k) as u32;
                let raw = &coverage.row(dy)[dx_start as usize..dx_end as usize];
                row.paint_with(&mut line[x_left..x_left + span], raw, scratch, &mut memo);
            }
        };
        let chunk = rows.div_ceil(workers);
        std::thread::scope(|scope| {
            let mut pieces = band.chunks_mut(chunk * width).enumerate();
            let first = pieces.next();
            for (n, piece) in pieces {
                let paint_rows = &paint_rows;
                scope.spawn(move || paint_rows(piece, n * chunk, &mut Vec::new()));
            }
            if let Some((_, piece)) = first {
                paint_rows(piece, 0, &mut self.scratch);
            }
        });
    }

    /// [`composite_coverage`](Self::composite_coverage) for a gradient with no
    /// clip mask over it — the other half of the split [`composite_flat`] made.
    ///
    /// The gradient is evaluated through a [`DeviceRamp`], which has the
    /// shape's inverse transform and bounds already folded into it, so the
    /// row's constants are computed once at the top of each row and the inner
    /// loop is a multiply, an add and a ramp lookup. See `native/gradient.rs`
    /// for what that replaced.
    pub(crate) fn composite_gradient(
        &mut self,
        coverage: MaskView<'_>,
        origin: (i32, i32),
        mode: BlendMode,
        pipeline: ColorPipeline,
        ramp: &DeviceRamp,
    ) {
        if coverage.is_empty() {
            return;
        }
        let (wx0, wy0, wx1, wy1) = self.writable();
        let dy_start = (wy0 - (coverage.y0 - origin.1)).max(0);
        let dy_end = (wy1 - (coverage.y0 - origin.1)).min(coverage.height as i32);
        let dx_start = (wx0 - (coverage.x0 - origin.0)).max(0);
        let dx_end = (wx1 - (coverage.x0 - origin.0)).min(coverage.width as i32);
        if dy_start >= dy_end || dx_start >= dx_end {
            return;
        }
        let normal = mode.is_normal() && matches!(pipeline, ColorPipeline::GammaSpace);
        self.mark_ink(
            coverage.x0 - origin.0 + dx_start,
            coverage.y0 - origin.1 + dy_start,
            coverage.x0 - origin.0 + dx_end,
            coverage.y0 - origin.1 + dy_end,
        );

        let table = super::linear::coverage_table();
        // **Rows in parallel**, on the same gate and discipline as
        // [`composite_flat`](Self::composite_flat): a gradient pixel depends
        // only on its own coverage and destination, so splitting the band
        // changes which thread computes it, never what. The header card of a
        // scrolling list is one gradient over most of a phone's width — on a
        // single core that is a serial millisecond the frame's budget cannot
        // hide.
        let width = self.width as usize;
        let rows = (dy_end - dy_start) as usize;
        let x_left_i = coverage.x0 - origin.0 + dx_start;
        let ty0_i = coverage.y0 + dy_start - origin.1;
        debug_assert!(
            ty0_i >= 0 && x_left_i >= 0,
            "clamped span must land inside the buffer"
        );
        let x_left = x_left_i as usize;
        let ty0 = ty0_i as usize;
        let span = (dx_end - dx_start) as usize;
        let band = &mut self.pixels[ty0 * width..(ty0 + rows) * width];
        // One gradient row's work, shared by the serial walk and the
        // row-parallel one so both are the same code — the `FlatRow` split's
        // discipline.
        let rows_ctx = GradientRows {
            ramp,
            coverage,
            table,
            dy_start: dy_start as usize,
            dx_start: dx_start as usize,
            x_left,
            span,
            normal,
            pipeline,
            mode,
        };
        let workers = super::geometry::fill::composite_workers(span as u32, rows as u32);
        if workers <= 1 {
            rows_ctx.paint(band, width, 0);
            return;
        }
        // As `composite_flat`: the pool measured worse for the many-small
        // per-draw walks, so this one keeps `std::thread::scope` too.
        let chunk = rows.div_ceil(workers);
        std::thread::scope(|scope| {
            let mut pieces = band.chunks_mut(chunk * width).enumerate();
            let first = pieces.next();
            for (n, piece) in pieces {
                let rows_ctx = &rows_ctx;
                scope.spawn(move || rows_ctx.paint(piece, width, n * chunk));
            }
            if let Some((_, piece)) = first {
                rows_ctx.paint(piece, width, 0);
            }
        });
    }

    /// A shape under a shaped clip, fused: the clip's cached rows are read in
    /// place — no staging row, no per-pixel closure — and a row whose own
    /// coverage is all ones (the shape's interior, most of its rows) skips
    /// the coverage work entirely, so a flat colour runs the SIMD row kernel
    /// with the clip as the mask and a gradient pays neither the table
    /// lookup nor the scale. This is the restatement of the closure walk at
    /// the bottom of `paint_shape`'s clipped branch; the multiply order is
    /// that walk's own (`colour × clip`, then `× coverage`) so the bytes are
    /// identical. Normal blend in gamma space only — anything else keeps the
    /// closure, which handles the exotic pipelines the honest way.
    pub(crate) fn composite_clipped_shape(
        &mut self,
        coverage: MaskView<'_>,
        origin: (i32, i32),
        clip: &super::geometry::fill::CoverageMask,
        flat: Premul,
        ramp: Option<&DeviceRamp>,
    ) {
        if coverage.is_empty() {
            return;
        }
        let (wx0, wy0, wx1, wy1) = self.writable();
        let dy_start = (wy0 - (coverage.y0 - origin.1)).max(0);
        let dy_end = (wy1 - (coverage.y0 - origin.1)).min(coverage.height as i32);
        let dx_start = (wx0 - (coverage.x0 - origin.0)).max(0);
        let dx_end = (wx1 - (coverage.x0 - origin.0)).min(coverage.width as i32);
        if dy_start >= dy_end || dx_start >= dx_end {
            return;
        }
        self.mark_ink(
            coverage.x0 - origin.0 + dx_start,
            coverage.y0 - origin.1 + dy_start,
            coverage.x0 - origin.0 + dx_end,
            coverage.y0 - origin.1 + dy_end,
        );

        let table = super::linear::coverage_table();
        let width = self.width as usize;
        let replaces = flat.a == 1.0;
        // The clip's own column range for this walk's span, in the clip
        // mask's coordinates — the mask is rasterised over the shape's
        // snapped-out region, so the span sits inside it; a shape that
        // somehow asks past the edge reads the zeros `coverage_at` would.
        let clip_x0 = coverage.x0 + dx_start - clip.x0;
        let clip_x1 = coverage.x0 + dx_end - clip.x0;
        #[expect(clippy::cast_possible_wrap, reason = "a clip's width fits a frame")]
        let clip_width = clip.width as i32;
        let clip_span = clip_x0.max(0) as usize..clip_x1.min(clip_width).max(0) as usize;
        let clamped = clip_x0 >= 0 && clip_x1 <= clip_width;
        for dy in dy_start..dy_end {
            let y = coverage.y0 + dy;
            let ty = (y - origin.1) as usize;
            let mask_row = coverage.row(dy as u32);
            let clip_dy = y - clip.y0;
            let dst_span = &mut self.pixels[ty * width
                + (coverage.x0 - origin.0 + dx_start) as usize
                ..ty * width + (coverage.x0 - origin.0 + dx_end) as usize];
            // A row outside the rasterised clip mask reads as zero coverage
            // everywhere: nothing to composite under a normal blend.
            if !(0..clip.height as i32).contains(&clip_dy) {
                continue;
            }
            let clip_row =
                &clip.data[clip_dy as usize * clip.width as usize..][..clip.width as usize];
            // **The opaque-interior skip** — the same one the gradient walk
            // takes: a row of the shape's own coverage that is all ones
            // needs neither the gamma table nor the scale.
            let interior = clamped
                && mask_row[dx_start as usize..dx_end as usize]
                    .iter()
                    .all(|&c| c >= 1.0);
            if interior {
                match ramp {
                    None => super::simd::over_flat_row(
                        dst_span,
                        &clip_row[clip_span.clone()],
                        flat,
                        replaces,
                    ),
                    Some(ramp) => {
                        for (j, dst) in dst_span.iter_mut().enumerate() {
                            // `at_device`, not `at(&row, ..)`: the closure
                            // walk this replaces computed `t` as
                            // `(ax·x + ay·y) + c` and the row-cached form
                            // associates the other way — one ulp apart, which
                            // is a moved byte at the ramp's quantisation
                            // edges. The hoist is not worth the drift.
                            //
                            // The clip still scales: an interior row is the
                            // *shape's* coverage at ones, and the clip's edge
                            // falls wherever it falls — the flat branch says
                            // the same thing with `over_flat_row`'s mask.
                            let clip_c = if clamped {
                                clip_row[clip_span.start + j]
                            } else {
                                clip_row.get(clip_span.start + j).copied().unwrap_or(0.0)
                            };
                            let src = ramp
                                .at_device(coverage.x0 + dx_start + j as i32, y)
                                .scaled(clip_c);
                            if src.a <= 0.0 {
                                continue;
                            }
                            if src.a == 1.0 {
                                *dst = src;
                                continue;
                            }
                            *dst = over(src, *dst);
                        }
                    }
                }
                continue;
            }
            for (j, dst) in dst_span.iter_mut().enumerate() {
                let c = super::linear::coverage_alpha_in(table, mask_row[dx_start as usize + j]);
                if c <= 0.0 {
                    continue;
                }
                let clip_c = if clamped {
                    clip_row[clip_span.start + j]
                } else {
                    clip_row.get(clip_span.start + j).copied().unwrap_or(0.0)
                };
                let colour = match ramp {
                    Some(ramp) => ramp.at_device(coverage.x0 + dx_start + j as i32, y),
                    None => flat,
                };
                let src = colour.scaled(clip_c).scaled(c);
                if src.a <= 0.0 {
                    continue;
                }
                if src.a == 1.0 {
                    *dst = src;
                    continue;
                }
                *dst = over(src, *dst);
            }
        }
    }

    /// Seed this buffer with a copy of `source`'s pixels underneath this
    /// buffer's own device-space region — real backdrop sampling for
    /// [`vieww_foundation::ImageFilter::backdrop`] (see `native/reference.rs`'s
    /// `Command::PushLayer` handling for where this is called and why it
    /// happens before any of this layer's own content paints).
    ///
    /// `source_origin` and `dest_origin` are both device-space top-lefts,
    /// the same convention every other method on this type uses: a pixel at
    /// absolute position `(x, y)` is read from `source` at
    /// `(x - source_origin.0, y - source_origin.1)` (out of bounds reads as
    /// transparent, via [`Self::get`]) and written to `self` at
    /// `(x - dest_origin.0, y - dest_origin.1)`.
    pub(crate) fn snapshot_from(
        &mut self,
        source: &Target,
        source_origin: (i32, i32),
        dest_origin: (i32, i32),
    ) {
        // Every pixel of this buffer is written, including the transparent
        // ones — a snapshot is a copy, not a draw.
        self.mark_all_ink();
        // Row at a time. This used to call `get`/`set` per pixel — two bounds
        // checks, two index multiplications and an `Option` each — to copy a
        // rectangle whose overlap with the source is one range of rows and one
        // range of columns, both knowable before the loop. A backdrop-filtered
        // layer snapshots its whole area on every frame it is drawn, so this is
        // a full-window copy per glass panel per frame.
        //
        // Rows outside the overlap stay as they are, which is what the
        // per-pixel version did too: `get` answered transparent off the end of
        // the source, and the buffer arrived from the pool already transparent.
        let (sw, sh) = (source.width as i32, source.height as i32);
        let dy0 = (source_origin.1 - dest_origin.1).max(0);
        let dy1 = (source_origin.1 + sh - dest_origin.1).min(self.height as i32);
        let dx0 = (source_origin.0 - dest_origin.0).max(0);
        let dx1 = (source_origin.0 + sw - dest_origin.0).min(self.width as i32);
        if dy1 <= dy0 || dx1 <= dx0 {
            return;
        }
        for dy in dy0..dy1 {
            let sy = dest_origin.1 + dy - source_origin.1;
            let src_row = sy as usize * source.width as usize;
            let dst_row = dy as usize * self.width as usize;
            let sx = dest_origin.0 + dx0 - source_origin.0;
            let count = (dx1 - dx0) as usize;
            self.pixels[dst_row + dx0 as usize..dst_row + dx0 as usize + count].copy_from_slice(
                &source.pixels[src_row + sx as usize..src_row + sx as usize + count],
            );
        }
    }

    /// Composite another whole buffer (a resolved layer) over this one at
    /// `(ox, oy)`, through `mode` and a flat `alpha` (spec §7.1's group
    /// opacity, applied at resolve — "opacity animatable without re-recording
    /// children").
    pub(crate) fn composite_layer(
        &mut self,
        other: &Target,
        ox: i32,
        oy: i32,
        mode: BlendMode,
        alpha: f32,
        pipeline: ColorPipeline,
    ) {
        // Row ranges computed once, for the reason `composite_coverage` gives:
        // a layer resolve reads and writes every pixel of the layer, and the
        // per-pixel `get`/`set` this used to call re-derived the same bounds
        // answer for each of them. Nesting makes it worse than a single fill,
        // because every level of depth composites the whole thing again — a
        // 24-deep stack of layers ran this loop 24 times over.
        let Some((x_start, y_start, x_end, y_end)) = self.layer_window(other, ox, oy) else {
            return;
        };
        self.mark_ink(x_start + ox, y_start + oy, x_end + ox, y_end + oy);

        let normal = mode.is_normal() && matches!(pipeline, ColorPipeline::GammaSpace);
        let opaque_copy = normal && alpha == 1.0;
        self.composite_layer_rows(
            other,
            (x_start, y_start, x_end, y_end),
            ox,
            oy,
            mode,
            alpha,
            pipeline,
            normal,
            opaque_copy,
        );
    }

    /// A cached shadow's coverage, tinted and composited source-over at
    /// `(ox, oy)`, writing only inside `within` (this buffer's coordinates,
    /// half-open) and scaled by `coverage` (patch-local) where a shape clip
    /// asks.
    ///
    /// The shadow cache keeps coverage, one `f32` a pixel, rather than tinted
    /// pixels at sixteen bytes: four times the shadows in the same memory,
    /// which a scrolling list of cards needs (two card heights at four
    /// sub-pixel phases, and an animated shadow beside them, overran a cache
    /// of tinted patches and missed on every frame). Each row is tinted into
    /// a scratch row (`tint.scaled(a)`, exactly what a tinted patch held) and
    /// blended by the same row function a layer is, so the pixels are the
    /// ones the tinted patch produced.
    ///
    /// `skip` is the interior a **covered** shadow does not blend — two whole
    /// pixel bands in device coordinates (see `covered_bands` in
    /// `reference.rs`), with the origin that translates device coordinates
    /// into this buffer's. The bands' x-ranges are nested, so the bands that
    /// contain any one row union to a single contiguous run; that run is
    /// carved out of the row and the segments either side of it are blended
    /// separately. Rows and pixels outside the skip are blended exactly as
    /// they always were — same tint, same coverage scaling, same row function
    /// — so the skip cannot change anything it does not delete, and what it
    /// deletes is pixels the caller has promised an opaque fill will replace.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_tinted_within(
        &mut self,
        alpha: &[f32],
        (width, height): (u32, u32),
        (ox, oy): (i32, i32),
        tint: Premul,
        coverage: Option<&dyn Fn(i32, i32) -> f32>,
        pipeline: ColorPipeline,
        within: (i32, i32, i32, i32),
        skip: Option<CoveredSkip>,
    ) {
        let (wx0, wy0, wx1, wy1) = self.writable();
        let x_start = (wx0 - ox).max(within.0 - ox).max(0);
        let y_start = (wy0 - oy).max(within.1 - oy).max(0);
        let x_end = (wx1 - ox).min(within.2 - ox).min(width as i32);
        let y_end = (wy1 - oy).min(within.3 - oy).min(height as i32);
        if x_start >= x_end || y_start >= y_end {
            return;
        }
        self.mark_ink(x_start + ox, y_start + oy, x_end + ox, y_end + oy);
        let normal = matches!(pipeline, ColorPipeline::GammaSpace);
        // One tint job per row, independent of every other row. The band and
        // the clip closure are **parameters**, not captures: a closure that
        // captured `&mut self.pixels` or `&dyn Fn` could not be shared across
        // the threads below, and the parallel branch never passes a clip
        // anyway (`&dyn Fn` is not `Sync`; a shaped clip keeps the serial
        // walk, which is the rare case). `buf_width` is this buffer's row;
        // `width`, the parameter, is the patch's — the source and destination
        // of one row are one `oy` apart.
        let buf_width = self.width as usize;
        let rows = (y_end - y_start) as usize;
        let band = &mut self.pixels[(y_start + oy) as usize * buf_width..][..rows * buf_width];
        let total_rows = (y_end - y_start) as u32;
        let span = (x_end - x_start) as u32;
        let workers = if coverage.is_none() {
            super::geometry::fill::composite_workers(span, total_rows)
        } else {
            1
        };
        let tint_rows = TintRows {
            alpha,
            patch_width: width,
            x_start,
            x_end,
            y_start,
            ox,
            oy,
            skip,
            tint,
            normal,
            pipeline,
        };
        if workers <= 1 {
            let mut row: Vec<Premul> = Vec::with_capacity((x_end - x_start) as usize);
            tint_rows.rows(band, 0, &mut row, coverage, buf_width);
            return;
        }
        // The parallel arm carries no coverage closure (`&dyn Fn` is not
        // `Sync`; a shaped clip keeps the serial walk), so each spawned
        // piece runs the shared row body with `coverage: None`. As
        // `composite_flat`, this per-draw walk keeps `std::thread::scope`:
        // the pool measured worse for the many-small kind.
        let chunk = total_rows.div_ceil(workers as u32) as usize;
        std::thread::scope(|scope| {
            let mut pieces = band.chunks_mut(chunk * buf_width).enumerate();
            let first = pieces.next();
            for (n, piece) in pieces {
                let tint_rows = &tint_rows;
                scope.spawn(move || {
                    let mut row: Vec<Premul> = Vec::with_capacity((x_end - x_start) as usize);
                    tint_rows.rows(piece, (n * chunk) as i32, &mut row, None, buf_width);
                });
            }
            if let Some((_, piece)) = first {
                let mut row: Vec<Premul> = Vec::with_capacity((x_end - x_start) as usize);
                tint_rows.rows(piece, 0, &mut row, coverage, buf_width);
            }
        });
    }

    /// The part of `other` (in its own coordinates, half-open) that
    /// [`Self::composite_layer`] reads at `(ox, oy)`: its ink, cut to this
    /// buffer's writable window. `None` when that is empty.
    fn layer_window(&self, other: &Target, ox: i32, oy: i32) -> Option<(i32, i32, i32, i32)> {
        let (wx0, wy0, wx1, wy1) = self.writable();
        let mut y_start = (wy0 - oy).max(0);
        let mut y_end = (wy1 - oy).min(other.height as i32);
        let mut x_start = (wx0 - ox).max(0);
        let mut x_end = (wx1 - ox).min(other.width as i32);
        // **Only where the layer actually has ink.**
        //
        // A layer's buffer is as big as the bounds it declared, and the pixels
        // outside what it drew are fully transparent — compositing them is a
        // read, an alpha test and a branch each, for a guaranteed no-op. A
        // 24-deep nest pays it once per level. `Target::ink`'s own doc has the
        // measurement; `None` means the layer drew nothing at all and there is
        // nothing to composite.
        match other.ink() {
            Some((ix0, iy0, ix1, iy1)) => {
                x_start = x_start.max(ix0);
                y_start = y_start.max(iy0);
                x_end = x_end.min(ix1);
                y_end = y_end.min(iy1);
            }
            None => return None,
        }
        (y_start < y_end && x_start < x_end).then_some((x_start, y_start, x_end, y_end))
    }

    /// [`Self::composite_layer`], for a layer about to go back to the pool:
    /// `other` is left fully transparent, with no ink, so the pool's
    /// [`Self::clear`] has nothing left to do.
    ///
    /// When the composite reads the layer's whole ink box under the normal
    /// blend, the clear happens in the same pass — each pixel is zeroed right
    /// after it is read, while its cache line is still loaded — instead of as
    /// a second walk over the same megabytes when the buffer is next handed
    /// out. Otherwise (a damaged repaint's write mask cutting the read, or
    /// another blend mode) it composites as usual and the pool clears later,
    /// exactly as before.
    pub(crate) fn composite_layer_and_clear(
        &mut self,
        other: &mut Target,
        ox: i32,
        oy: i32,
        mode: BlendMode,
        alpha: f32,
        pipeline: ColorPipeline,
    ) {
        let normal = mode.is_normal() && matches!(pipeline, ColorPipeline::GammaSpace);
        let window = self.layer_window(other, ox, oy);
        // Measured, ten interleaved runs each: `00-layers-nested` (24 nested
        // translucent layers) 25.7 ms -> 22.0 ms median, byte-identical
        // output. Instruction counts cannot show this one — it is memory
        // traffic, and callgrind counts the old `memset` per byte — which is
        // why it was timed rather than profiled.
        if !(normal && window.is_some() && window == other.ink) {
            self.composite_layer(other, ox, oy, mode, alpha, pipeline);
            return;
        }
        let (x_start, y_start, x_end, y_end) = window.expect("checked above");
        self.mark_ink(x_start + ox, y_start + oy, x_end + ox, y_end + oy);
        let opaque_copy = alpha == 1.0;
        let (ow, sw) = (other.width as usize, self.width as usize);
        let rows = (y_end - y_start) as usize;
        let (sx0, sx1) = (x_start as usize, x_end as usize);
        let (dx0, dx1) = ((x_start + ox) as usize, (x_end + ox) as usize);
        let src_band = &mut other.pixels[y_start as usize * ow..y_end as usize * ow];
        let dst_band = &mut self.pixels[(y_start + oy) as usize * sw..(y_end + oy) as usize * sw];
        let take_rows = |dst: &mut [Premul], src: &mut [Premul]| {
            for (d, s) in dst.chunks_exact_mut(sw).zip(src.chunks_exact_mut(ow)) {
                super::simd::over_layer_row_take(
                    &mut d[dx0..dx1],
                    &mut s[sx0..sx1],
                    alpha,
                    opaque_copy,
                );
            }
        };
        // Rows in parallel past the rasterizer's size gate, as in
        // `composite_flat`: each pixel's result is its own source and
        // destination, so the split cannot change a byte (FW-07). On the
        // raster pool since C-2, with both bands pre-split into disjoint
        // per-chunk blocks.
        let workers =
            super::geometry::fill::composite_workers((x_end - x_start) as u32, rows as u32);
        if workers <= 1 {
            take_rows(dst_band, src_band);
        } else {
            run_bands(
                dst_band,
                Some(src_band),
                rows,
                sw,
                ow,
                workers,
                (alpha, opaque_copy, sx0, sx1, dx0, dx1),
                take_band_chunk,
            );
        }
        other.ink = None;
    }

    /// Multiply a resolved clip mask into every pixel of this buffer — the
    /// pre-pop clip pass, as its own walk, for the composite routes that
    /// cannot fuse it ([`Self::composite_clipped_layer_and_clear`] names
    /// them). Identical to the loop that used to live at the `PopLayer` call
    /// site: every pixel below full coverage is scaled by it, every pixel at
    /// or above full coverage is left untouched — `scaled` by exactly one is
    /// the identity, and skipping it keeps the buffer's bits where they were.
    pub(crate) fn multiply_clip(&mut self, mask: &CoverageMask, origin: (i32, i32)) {
        let width = self.width as usize;
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                let coverage = mask.coverage_at(origin.0 + x, origin.1 + y);
                if coverage < 1.0 {
                    let index = y as usize * width + x as usize;
                    self.pixels[index] = self.pixels[index].scaled(coverage);
                }
            }
        }
    }

    /// One row of a clip mask's coverage for a layer, as `out` — zeros
    /// outside the mask's extent, its own values inside, the same arithmetic
    /// `coverage_at` answers per pixel, gathered once per row. `y`, `x0` and
    /// `x1` are in the **layer's** coordinates; `origin` is the layer's own
    /// device-space top-left, the same translation `coverage_at`'s callers
    /// apply.
    fn clip_coverage_row(
        mask: &CoverageMask,
        origin: (i32, i32),
        y: i32,
        x0: i32,
        x1: i32,
        out: &mut [f32],
    ) {
        let ay = origin.1 + y;
        let row = if ay >= mask.y0 {
            let dy = (ay - mask.y0) as u32;
            (dy < mask.height).then(|| {
                let w = mask.width as usize;
                let start = dy as usize * w;
                &mask.data[start..start + w]
            })
        } else {
            None
        };
        match row {
            None => out.fill(0.0),
            Some(row) => {
                for (px, ax) in out.iter_mut().zip((origin.0 + x0)..(origin.0 + x1)) {
                    *px = if ax >= mask.x0 && ((ax - mask.x0) as u32) < mask.width {
                        row[(ax - mask.x0) as usize]
                    } else {
                        0.0
                    };
                }
            }
        }
    }

    /// [`Self::composite_layer_and_clear`] for a layer that pops under a
    /// shaped clip: the clip's coverage multiplies into the layer's pixels
    /// and the result composites down, **as one walk of the layer**.
    ///
    /// # What was two passes, and why one
    ///
    /// Until this, `PopLayer` multiplied the clip into the resolved layer
    /// buffer as its own whole-buffer walk, and *then*
    /// `composite_layer_and_clear` walked the same megabytes again to read
    /// them down into the parent and clear them — every pixel of a rounded
    /// panel visited twice, once to scale it and once to composite it, with
    /// the scaled buffer written back to memory in between purely so the
    /// second pass could read it back. The fused walk scales each row and
    /// hands it straight to the take kernel while it is still in cache: the
    /// scale rule is byte for byte the old pre-pass's (`scaled` below full
    /// coverage, untouched at or above it), applied to the same pixels in the
    /// same order, so the bytes the kernel sees — and therefore the bytes it
    /// writes, and the bytes the pool's clear sees — are identical.
    ///
    /// When the take kernel's own conditions do not hold — a blend other
    /// than normal-in-gamma, or a damaged repaint's write mask cutting the
    /// read window — the clip multiplies in first, as its own walk exactly as
    /// it always did, and the ordinary composite runs after: the fusion is
    /// for the common shape (a rounded panel over a plain background), not a
    /// semantic change.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_clipped_layer_and_clear(
        &mut self,
        other: &mut Target,
        ox: i32,
        oy: i32,
        mode: BlendMode,
        alpha: f32,
        pipeline: ColorPipeline,
        clip: &CoverageMask,
        clip_origin: (i32, i32),
    ) {
        let normal = mode.is_normal() && matches!(pipeline, ColorPipeline::GammaSpace);
        let window = self.layer_window(other, ox, oy);
        if !(normal && window.is_some() && window == other.ink) {
            other.multiply_clip(clip, clip_origin);
            self.composite_layer_and_clear(other, ox, oy, mode, alpha, pipeline);
            return;
        }
        let (x_start, y_start, x_end, y_end) = window.expect("checked above");
        self.mark_ink(x_start + ox, y_start + oy, x_end + ox, y_end + oy);
        let opaque_copy = alpha == 1.0;
        let (ow, sw) = (other.width as usize, self.width as usize);
        let rows = (y_end - y_start) as usize;
        let (sx0, sx1) = (x_start as usize, x_end as usize);
        let (dx0, dx1) = ((x_start + ox) as usize, (x_end + ox) as usize);
        let take_rows = |base_y: i32, dst: &mut [Premul], src: &mut [Premul]| {
            // One coverage row per worker, filled per layer row and read by
            // the scale loop — the same gathering `composite_flat`'s scratch
            // does for the coverage gamma. `base_y` is the **layer row this
            // band starts at**: the parallel split hands each worker its own
            // sub-band, whose rows restart at 0, and a coverage row is a
            // function of the absolute row — the un-clipped `take_rows` never
            // needed to know.
            let mut cov = vec![0.0f32; sx1 - sx0];
            for (i, (d, s)) in dst
                .chunks_exact_mut(sw)
                .zip(src.chunks_exact_mut(ow))
                .enumerate()
            {
                let y = base_y + i as i32;
                Self::clip_coverage_row(clip, clip_origin, y, x_start, x_end, &mut cov);
                let row = &mut s[sx0..sx1];
                for (px, &c) in row.iter_mut().zip(cov.iter()) {
                    if c < 1.0 {
                        *px = px.scaled(c);
                    }
                }
                super::simd::over_layer_row_take(&mut d[dx0..dx1], row, alpha, opaque_copy);
            }
        };
        // Rows in parallel past the rasterizer's size gate, as in
        // `composite_layer_and_clear`: each pixel's result is its own source
        // and destination, so the split cannot change a byte. On the raster
        // pool since C-2; the chunk's base row keeps the coverage rows
        // absolute — the bug the receipt test pins.
        let workers =
            super::geometry::fill::composite_workers((x_end - x_start) as u32, rows as u32);
        let src_band = &mut other.pixels[y_start as usize * ow..y_end as usize * ow];
        let dst_band = &mut self.pixels[(y_start + oy) as usize * sw..(y_end + oy) as usize * sw];
        if workers <= 1 {
            take_rows(y_start, dst_band, src_band);
        } else {
            run_bands(
                dst_band,
                Some(src_band),
                rows,
                sw,
                ow,
                workers,
                (
                    alpha,
                    opaque_copy,
                    sx0,
                    sx1,
                    dx0,
                    dx1,
                    y_start,
                    clip,
                    clip_origin,
                ),
                clipped_take_band_chunk,
            );
        }
        other.ink = None;
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_layer_rows(
        &mut self,
        other: &Target,
        (x_start, y_start, x_end, y_end): (i32, i32, i32, i32),
        ox: i32,
        oy: i32,
        mode: BlendMode,
        alpha: f32,
        pipeline: ColorPipeline,
        normal: bool,
        opaque_copy: bool,
    ) {
        for y in y_start..y_end {
            let src_row = y as usize * other.width as usize;
            let dst_row = (y + oy) as usize * self.width as usize;
            // **No opaque-row `copy_from_slice` fast path here, and it was
            // measured rather than assumed.**
            //
            // `over` with `src.a == 1` is `src`, so a fully opaque run of a
            // layer is a copy, and scanning the row's alphas first to turn it
            // into one `memcpy` looks like an obvious win — panels and popovers
            // are mostly opaque. Together with the matching `c >= 1.0` skip in
            // `composite_flat`, it took `examples/fixtures`' `23-editor-glass`
            // from **1,203,538,312 instructions to 1,223,637,694** — 1.7%
            // *worse*. The scan is a second pass over the row, the per-pixel
            // blend it replaces is already only a few instructions, and the
            // branch it adds is one the compiler had already arranged for.
            //
            // Recorded because it is the third plausible-sounding change in
            // this file to measure worse than the code it replaced, and the
            // fourth to measure the opposite of a single-sample estimate.
            //
            // The normal blend in gamma space is the explicit-SIMD row in
            // `native/simd.rs`, byte-identical to the loop below specialised
            // to `normal`.
            if normal {
                let src = &other.pixels[src_row + x_start as usize..src_row + x_end as usize];
                let dst = &mut self.pixels
                    [dst_row + (x_start + ox) as usize..dst_row + (x_end + ox) as usize];
                super::simd::over_layer_row(dst, src, alpha, opaque_copy);
                continue;
            }
            for x in x_start..x_end {
                let src = other.pixels[src_row + x as usize];
                let src = if alpha == 1.0 { src } else { src.scaled(alpha) };
                if src.a <= 0.0 && mode.is_normal() {
                    continue;
                }
                let index = dst_row + (x + ox) as usize;
                if opaque_copy && src.a == 1.0 {
                    self.pixels[index] = src;
                    continue;
                }
                let dst = self.pixels[index];
                // `normal` is `mode.is_normal() && GammaSpace`, decided once
                // above — so where it holds, the twenty-eight-way match in
                // `blend` and the pipeline match in `blend_with_pipeline` have
                // only one possible answer and are pure dispatch. See
                // `color::over`.
                self.pixels[index] = if normal {
                    over(src, dst)
                } else {
                    blend_with_pipeline(pipeline, mode, src, dst)
                };
            }
        }
    }

    /// [`to_rgba8`](Self::to_rgba8) into a buffer the caller already owns.
    ///
    /// A frame's output is the same size as the last frame's, so a renderer
    /// presenting continuously can keep one buffer and refill it instead of
    /// allocating and freeing a megabyte or two every frame — which at
    /// 1366x679 is 3.7 MB of allocation per frame for nothing.
    ///
    /// # Panics
    ///
    /// If `out` is not exactly four bytes per pixel.
    pub(crate) fn write_rgba8(&self, out: &mut [u8]) {
        assert_eq!(out.len(), self.pixels.len() * 4);
        // Four pixels per SSE2 step on x86-64, the scalar path elsewhere and
        // for the tail — the same bytes either way; see `write_straight_u8`.
        //
        // Split into row bands across the rasterizer's workers past its size
        // gate: the conversion is per pixel, so where a band starts changes
        // nothing — and bands are whole rows of a multiple of four pixels
        // where possible, so the SIMD/tail split even lands the same (FW-07).
        let workers = super::geometry::fill::composite_workers(self.width, self.height);
        if workers <= 1 {
            write_straight_u8(&self.pixels, out);
            return;
        }
        // Row bands on the raster pool since C-2. The conversion is per
        // pixel, so a row split lands the same bytes the flat-element split
        // did — `write_straight_u8`'s SIMD and scalar halves are bit-equal,
        // which is what the row band boundaries now select between.
        let rows = self.height as usize;
        let width = self.width as usize;
        run_bands(
            out,
            None,
            rows,
            width * 4,
            0,
            workers,
            self.pixels.as_slice(),
            rgba8_band_chunk,
        );
    }

    /// Resize if needed and refill with `color`, reusing this buffer's
    /// allocation whenever the size has not changed.
    ///
    /// The root frame buffer is the same size on frame two as on frame one
    /// essentially always — a window resize is the exception, not the rule —
    /// so allocating a fresh one per frame is pure waste. At 1366x679 that
    /// allocation is 14.8 MB of `Premul`, and measurably: an *empty* scene
    /// cost 11.9 ms a frame before this, which is 71% of a 60 Hz budget spent
    /// on a buffer nothing had drawn into yet.
    pub(crate) fn reset(&mut self, width: u32, height: u32, color: Premul) {
        let wanted = (width as usize) * (height as usize);
        if self.pixels.len() == wanted {
            self.pixels.fill(color);
        } else {
            self.pixels.clear();
            self.pixels.resize(wanted, color);
        }
        self.width = width;
        self.height = height;
        // A full reset fills every pixel with `color`, which for the root
        // frame buffer is the window's background — a real, visible colour, not
        // "nothing". Every pixel of it is therefore ink.
        self.ink = Some((0, 0, width as i32, height as i32));
    }

    /// Resize the buffer **without** filling it — for a frame whose first
    /// command paints an opaque colour over every pixel (`render_in_place`
    /// checks), where a fill now would only be filled over again. The bytes
    /// the buffer already holds are stale by definition; the covering fill
    /// replaces all of them.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        let wanted = (width as usize) * (height as usize);
        if self.pixels.len() != wanted {
            self.pixels.resize(wanted, Premul::TRANSPARENT);
        }
        self.width = width;
        self.height = height;
        self.ink = None;
    }

    /// Refill one rectangle with `color`, leaving the rest of the buffer as
    /// it is.
    ///
    /// A damaged repaint clears the region it is about to redraw and nothing
    /// else — that is what makes the buffer *retained*: the pixels outside
    /// the region are last frame's, and they are still correct precisely
    /// because nothing damaged them.
    ///
    /// Clearing is not optional inside the region, though. A damage region
    /// says what to redraw, not what to skip clearing: a shape that shrank or
    /// moved must have its old pixels gone, and a partial-background repaint
    /// would leave them standing.
    pub(crate) fn reset_region(&mut self, region: (i32, i32, i32, i32), color: Premul) {
        let (x0, y0, x1, y1) = region;
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32);
        let y1 = y1.min(self.height as i32);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let width = self.width as usize;
        let span = (x1 - x0) as usize;
        let rows = (y1 - y0) as usize;
        let band = &mut self.pixels[y0 as usize * width..][..rows * width];
        let clear_rows = |band: &mut [Premul]| {
            for line in band.chunks_exact_mut(width) {
                line[x0 as usize..x0 as usize + span].fill(color);
            }
        };
        // Rows in parallel on the rasterizer's own gate — the same discipline
        // as `composite_flat`, and for the same reason: a damage region's
        // clear is a row-independent fill of up to a whole screen.
        let workers = super::geometry::fill::composite_workers(span as u32, rows as u32);
        if workers <= 1 {
            clear_rows(band);
        } else {
            run_bands(
                band,
                None,
                rows,
                width,
                0,
                workers,
                (x0 as usize, span, color),
                clear_band_chunk,
            );
        }
        self.mark_ink(x0, y0, x1, y1);
    }

    /// [`write_rgba8`](Self::write_rgba8) for one rectangle only.
    ///
    /// The other half of a retained surface. Converting the whole buffer to
    /// RGBA8 costs the same whether one pixel changed or all of them — at
    /// 1440x900 that conversion alone is most of an 11 ms floor — so a
    /// damaged frame converts only the rows it actually repainted.
    ///
    /// # Panics
    ///
    /// If `out` is not exactly four bytes per pixel of this buffer.
    pub(crate) fn write_rgba8_region(&self, out: &mut [u8], region: (i32, i32, i32, i32)) {
        assert_eq!(out.len(), self.pixels.len() * 4);
        let (x0, y0, x1, y1) = region;
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width as i32);
        let y1 = y1.min(self.height as i32);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let width = self.width as usize;
        let span = (x1 - x0) as usize;
        let rows = (y1 - y0) as usize;
        // Rows in parallel on the rasterizer's own gate, on the raster pool
        // since C-2: each row converts independently, so the bytes are
        // identical at any worker count — and a damaged frame's conversion is
        // otherwise a serial millisecond per region on a phone. The pixels
        // ride the walk's context (shared read-only); only the byte rows are
        // split.
        let workers = super::geometry::fill::composite_workers(span as u32, rows as u32);
        let ctx = RegionRows {
            pixels: &self.pixels[y0 as usize * width..][..rows * width],
            width,
            x0: x0 as usize,
            span,
        };
        let bytes = &mut out[y0 as usize * width * 4..][..rows * width * 4];
        if workers <= 1 {
            ctx.write_rows(bytes, 0, width * 4);
        } else {
            run_bands(
                bytes,
                None,
                rows,
                width * 4,
                0,
                workers,
                &ctx,
                rgba8_region_band_chunk,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Row-parallel walks on the raster pool.
//
// The compositing methods below split their rows past the rasterizer's size
// gate and hand the bands to workers. They used to do it with
// `std::thread::scope` — a thread create and join per worker, per call — and
// a frame has hundreds of calls above that gate: every fill, every layer
// pop, every present's conversion. The fill walk's pool
// (`native/raster_pool.rs`) removed exactly that cost for the scanline walk;
// these are the same shape of work on the other half of the frame.
//
// The pool shares one job across its workers, so a job carries its bands as
// raw pointers into blocks that `split_at_mut` proved disjoint — the pattern
// `geometry/fill.rs`'s `RowJob` established — plus the walk's read-only
// context and a plain-fn body. The pool's per-call contract (every chunk of
// *this* batch runs exactly once before the call returns) is what makes
// pointing workers at the caller's bands sound.

/// One chunk of a row-parallel walk: its disjoint block of the primary band,
/// the same rows of the optional second band, and the chunk's band-relative
/// row base — the `n * chunk_rows` the `std::thread::scope` splits used to
/// pass their closures.
struct BandChunk<A> {
    a: (*mut A, usize),
    b: (*mut A, usize),
    base: usize,
}

/// A row-parallel walk, pre-split for the pool.
struct BandWalk<'a, A, C> {
    chunks: Vec<BandChunk<A>>,
    /// One row of the primary band, in elements.
    a_row: usize,
    /// One row of the second band; `0` when there is none.
    b_row: usize,
    /// The walk's read-only per-chunk context.
    context: C,
    /// Runs one chunk: the walk and the chunk's index.
    body: fn(&BandWalk<'a, A, C>, usize),
    lifetime: std::marker::PhantomData<&'a ()>,
}

// SAFETY: the raw band pointers are the disjoint halves of `split_at_mut`
// splits made by `run_bands`, and each chunk's body touches only its own
// block — the disjointness the borrow checker would have checked, carried by
// the splits instead. The context is `Sync` and only read. The pool's
// contract returns before the caller's bands end, so no pointer outlives its
// borrow; writes go through `A: Send` values.
unsafe impl<A: Send, C: Sync> Sync for BandWalk<'_, A, C> {}

/// The pool's per-chunk trampoline: a plain fn, as `run_chunks` requires.
fn band_walk_chunk<A: Send, C: Sync>(walk: &BandWalk<'_, A, C>, chunk: usize) {
    (walk.body)(walk, chunk);
}

/// Split `a` (and the same-rows `b`, when the walk takes one) into `workers`
/// row bands and run `body` over them on the raster pool — the
/// `std::thread::scope` these walks grew up with, minus the spawn per call.
///
/// `rows` is the band's row count; `a_row`/`b_row` are its row strides. The
/// chunk bodies see exactly the bands and bases the scope versions passed
/// their closures — the last band may be short, as `chunks_mut`'s last piece
/// always was — so the bytes are the scope version's bytes at any worker
/// count.
#[allow(clippy::too_many_arguments)]
fn run_bands<A: Send, C: Sync>(
    a: &mut [A],
    b: Option<&mut [A]>,
    rows: usize,
    a_row: usize,
    b_row: usize,
    workers: usize,
    context: C,
    body: fn(&BandWalk<'_, A, C>, usize),
) {
    debug_assert!(workers >= 1, "the caller decides serial versus parallel");
    let chunk_rows = rows.div_ceil(workers);
    let mut chunks = Vec::with_capacity(workers);
    let mut a_rest = a;
    let mut b_rest = b;
    for n in 0..workers {
        let (block, rest) = a_rest.split_at_mut((chunk_rows * a_row).min(a_rest.len()));
        a_rest = rest;
        let b_block = match b_rest.take() {
            Some(rest) => {
                let (block, tail) = rest.split_at_mut((chunk_rows * b_row).min(rest.len()));
                b_rest = Some(tail);
                (block.as_mut_ptr(), block.len())
            }
            None => (std::ptr::NonNull::dangling().as_ptr(), 0),
        };
        chunks.push(BandChunk {
            a: (block.as_mut_ptr(), block.len()),
            b: b_block,
            base: n * chunk_rows,
        });
    }
    let walk = BandWalk {
        chunks,
        a_row,
        b_row,
        context,
        body,
        lifetime: std::marker::PhantomData,
    };
    super::raster_pool::run_chunks(&walk, workers, workers, band_walk_chunk);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `composite_layer_and_clear` is `composite_layer` followed by a clear,
    /// on both of its paths: the fused one (the whole ink box is read) and
    /// the fallback (a write mask cuts the read, so the pool must clear).
    #[test]
    fn a_new_target_is_transparent() {
        let t = Target::new(7, 5);
        assert_eq!(t.pixels.len(), 35);
        assert!(t.pixels.iter().all(|p| *p == Premul::TRANSPARENT));
        assert!(Target::new(0, 9).pixels.is_empty());
    }

    #[test]
    fn composite_and_clear_matches_composite_then_clear() {
        let tint = |x: u32, y: u32| Premul {
            r: (x as f32 / 40.0) * 0.6,
            g: (y as f32 / 30.0) * 0.6,
            b: 0.1,
            a: 0.6 + ((x + y) % 5) as f32 * 0.1,
        };
        for mask in [None, Some((5, 4, 20, 18))] {
            let mut layer = Target::new(24, 20);
            for y in 2..17 {
                for x in 3..21 {
                    layer.set(x, y, tint(x as u32, y as u32));
                }
            }
            let ground = Premul {
                r: 0.2,
                g: 0.3,
                b: 0.4,
                a: 1.0,
            };
            let mut expected = Target::filled(40, 30, ground);
            let mut actual = expected.clone();
            expected.set_write_mask(mask);
            actual.set_write_mask(mask);
            for alpha in [1.0, 0.97] {
                let mut fused = layer.clone();
                expected.composite_layer(
                    &layer,
                    7,
                    5,
                    BlendMode::Normal,
                    alpha,
                    ColorPipeline::GammaSpace,
                );
                actual.composite_layer_and_clear(
                    &mut fused,
                    7,
                    5,
                    BlendMode::Normal,
                    alpha,
                    ColorPipeline::GammaSpace,
                );
                assert_eq!(
                    actual.pixels, expected.pixels,
                    "mask {mask:?}, alpha {alpha}"
                );
                assert_eq!(actual.ink(), expected.ink());
                // Whichever path ran, the buffer is transparent once the
                // pool's `clear` has had its turn.
                fused.clear();
                assert!(fused.pixels.iter().all(|p| *p == Premul::TRANSPARENT));
                assert_eq!(fused.ink(), None);
            }
        }
    }

    /// `composite_clipped_layer_and_clear` is `multiply_clip` followed by
    /// `composite_layer_and_clear`, on both of its routes: the fused walk
    /// (whole ink box, normal blend) and the fallback (a write mask cuts the
    /// read window, so the clip multiplies in as its own pass first). The
    /// clip is a fractional-coverage mask offset off-grid from the layer, so
    /// the scale rule — `scaled` below full coverage, untouched at or above
    /// it — runs on real antialiased values, not just 0 and 1.
    #[test]
    fn composite_clipped_matches_multiply_then_composite() {
        // A 14x10 mask at device (12, 9): 1.0 in the middle, a fractional
        // ramp down the left and top edges, zero padding — the shape of a
        // rounded clip without building one.
        let mask = {
            let mut data = vec![0.0f32; 14 * 10];
            for y in 0..10usize {
                for x in 0..14usize {
                    let edge = (x.min(y).min(3)) as f32 / 3.0;
                    data[y * 14 + x] = if x >= 3 && y >= 3 { 1.0 } else { edge };
                }
            }
            CoverageMask {
                x0: 12,
                y0: 9,
                width: 14,
                height: 10,
                data,
            }
        };
        let tint = |x: u32, y: u32| Premul {
            r: (x as f32 / 20.0) * 0.8,
            g: (y as f32 / 16.0) * 0.8,
            b: 0.2,
            a: 0.7 + ((x + y) % 4) as f32 * 0.1,
        };
        let ground = Premul {
            r: 0.25,
            g: 0.35,
            b: 0.45,
            a: 1.0,
        };
        for cut in [None, Some((8, 6, 26, 22))] {
            for alpha in [1.0, 0.9] {
                // The layer sits at device (10, 8), so the clip's mask (at
                // (12, 9)) cuts in from its top-left corner and ends before
                // its right edge — off-grid overlap in both axes.
                let mut layer = Target::new(20, 16);
                for y in 0..16usize {
                    for x in 0..20usize {
                        layer.set(x as i32, y as i32, tint(x as u32, y as u32));
                    }
                }
                let origin = (10, 8);

                let mut expected = Target::filled(40, 30, ground);
                let mut expected_layer = layer.clone();
                expected_layer.multiply_clip(&mask, origin);
                expected.composite_layer_and_clear(
                    &mut expected_layer,
                    10,
                    8,
                    BlendMode::Normal,
                    alpha,
                    ColorPipeline::GammaSpace,
                );

                let mut actual = Target::filled(40, 30, ground);
                let mut fused = layer.clone();
                actual.composite_clipped_layer_and_clear(
                    &mut fused,
                    10,
                    8,
                    BlendMode::Normal,
                    alpha,
                    ColorPipeline::GammaSpace,
                    &mask,
                    origin,
                );

                assert_eq!(
                    actual.pixels, expected.pixels,
                    "cut {cut:?}, alpha {alpha}: the fused walk differs from multiply-then-composite"
                );
                assert_eq!(actual.ink(), expected.ink());
                // The released buffer must be transparent whichever route
                // ran, once the pool's clear has had its turn.
                fused.clear();
                expected_layer.clear();
                assert!(fused.pixels.iter().all(|p| *p == Premul::TRANSPARENT));
                assert_eq!(fused.pixels, expected_layer.pixels);
            }
        }

        // **The parallel split.** A layer big enough to cross the workers
        // gate hands each worker a sub-band whose rows restart at 0 — and a
        // coverage row is a function of the absolute row, so a take_rows
        // that derives `y` from its band-relative index composites the
        // clip's coverage from the wrong rows. `23-editor-glass` found it as
        // 224 wrong pixels; this is the size that prevents it from coming
        // back. The clip's edge ramp (three fractional columns) runs down
        // the whole height so a mis-derived row shows as a whole column of
        // wrong coverage.
        {
            let (w, h) = (260u32, 300u32);
            let mut big = CoverageMask {
                x0: 0,
                y0: 0,
                width: w,
                height: h,
                data: vec![1.0f32; (w * h) as usize],
            };
            for y in 0..h as usize {
                for x in 0..3usize {
                    big.data[y * w as usize + x] = (x + 1) as f32 / 4.0;
                }
            }
            let mut layer = Target::new(w, h);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    layer.set(x, y, tint(x as u32, y as u32));
                }
            }
            let mut expected = Target::filled(w + 20, h + 10, ground);
            let mut expected_layer = layer.clone();
            expected_layer.multiply_clip(&big, (0, 0));
            expected.composite_layer_and_clear(
                &mut expected_layer,
                0,
                0,
                BlendMode::Normal,
                1.0,
                ColorPipeline::GammaSpace,
            );
            let mut actual = Target::filled(w + 20, h + 10, ground);
            let mut fused = layer;
            actual.composite_clipped_layer_and_clear(
                &mut fused,
                0,
                0,
                BlendMode::Normal,
                1.0,
                ColorPipeline::GammaSpace,
                &big,
                (0, 0),
            );
            assert_eq!(
                actual.pixels, expected.pixels,
                "the parallel split mis-derived its coverage rows"
            );
        }
    }

    fn red() -> Premul {
        Premul::from_straight(vieww_foundation::Color::rgba(255, 0, 0, 255))
    }

    fn blue() -> Premul {
        Premul::from_straight(vieww_foundation::Color::rgba(0, 0, 255, 255))
    }

    /// A coverage rectangle that starts left of the buffer's origin — a glyph
    /// whose ink begins a pixel or two left of its layer's declared bounds,
    /// which is exactly what the launch animation produced at its device
    /// switch: `cov=(1096,172 20x67) origin=(1097,160)`.
    ///
    /// `composite_flat` used to hoist `(coverage.x0 - origin.0) as usize` out
    /// of the loop, so the negative offset wrapped to near 2^64 and the row
    /// base add overflowed. The regression this pins is that the draw clips
    /// the overhang instead of panicking, and lands the surviving columns at
    /// the right pixels — `coverage.x0 + dx` for the clamped `dx` range, the
    /// same arithmetic the general path performs per pixel.
    #[test]
    fn composite_flat_clips_a_coverage_that_starts_left_of_the_origin() {
        use super::super::geometry::fill::CoverageMask;

        let mut target = Target::new(10, 10);
        // An 8x4 mask whose device-space box starts at x = -3: only the 5
        // columns that overlap the buffer should land.
        let mask = CoverageMask {
            x0: -3,
            y0: 2,
            width: 8,
            height: 4,
            data: vec![1.0; 8 * 4],
        };
        target.composite_flat(
            mask.view(),
            (0, 0),
            vieww_foundation::BlendMode::Normal,
            ColorPipeline::GammaSpace,
            red(),
        );

        for y in 2..6 {
            for x in 0..5 {
                assert_eq!(target.get(x, y), red(), "visible column {x},{y} is painted");
            }
            assert_eq!(
                target.get(5, y),
                Premul::TRANSPARENT,
                "the column past the clipped span stays clear"
            );
        }
        assert_eq!(target.get(0, 1), Premul::TRANSPARENT, "row above the mask");
    }

    /// The nested-layer flavour of the same geometry: a coverage measured in
    /// device space against a target whose own origin is nonzero, with the
    /// overhang on the left — the exact `PushLayer` + glyph situation from
    /// the film. With origin (5, 5) and a mask box starting at x = 2, the
    /// first three columns fall before the buffer and must be dropped.
    #[test]
    fn composite_flat_clips_a_left_overhang_against_a_layer_origin() {
        use super::super::geometry::fill::CoverageMask;

        let mut target = Target::new(6, 6);
        let mask = CoverageMask {
            x0: 2,
            y0: 6,
            width: 8,
            height: 3,
            data: vec![1.0; 8 * 3],
        };
        target.composite_flat(
            mask.view(),
            (5, 5),
            vieww_foundation::BlendMode::Normal,
            ColorPipeline::GammaSpace,
            blue(),
        );

        // Device x 2..10 against origin 5: local 0..5 painted, the two columns
        // left of the origin dropped rather than wrapped around the address
        // space.
        for y in 1..4 {
            for x in 0..5 {
                assert_eq!(target.get(x, y), blue(), "local column {x},{y} is painted");
            }
        }
        assert_eq!(target.get(5, 1), Premul::TRANSPARENT);
    }

    /// The plain case: both buffers share the same device-space origin, so a
    /// snapshot is a straight copy.
    #[test]
    fn snapshot_from_copies_pixels_when_origins_match() {
        let mut source = Target::new(4, 4);
        source.set(1, 1, red());
        source.set(3, 3, blue());

        let mut dest = Target::new(4, 4);
        dest.snapshot_from(&source, (0, 0), (0, 0));

        assert_eq!(dest.get(1, 1), red());
        assert_eq!(dest.get(3, 3), blue());
        assert_eq!(dest.get(0, 0), Premul::TRANSPARENT);
    }

    /// The case `Command::PushLayer` actually hits: the destination
    /// (`self`) is a smaller offscreen buffer sitting somewhere inside a
    /// larger source at a nonzero device-space offset. Each is expressed in
    /// its own local coordinates, and it is exactly this arithmetic —
    /// mapping one buffer's origin through the other's — that a snapshot
    /// implementation gets wrong by an off-by-origin bug if either offset is
    /// dropped.
    #[test]
    fn snapshot_from_maps_a_nested_layers_origin_correctly() {
        // A 10x10 source (e.g. the root frame) with a single red pixel at
        // absolute device position (7, 7).
        let mut source = Target::new(10, 10);
        source.set(7, 7, red());

        // A 4x4 destination layer whose device-space top-left is (5, 5) —
        // so absolute (7, 7) is local (2, 2) inside it.
        let mut dest = Target::new(4, 4);
        dest.snapshot_from(&source, (0, 0), (5, 5));

        assert_eq!(
            dest.get(2, 2),
            red(),
            "the marked pixel lands at the mapped local position"
        );
        // Every other pixel in range is still whatever the source had there:
        // transparent.
        assert_eq!(dest.get(0, 0), Premul::TRANSPARENT);
        assert_eq!(dest.get(3, 3), Premul::TRANSPARENT);
    }

    /// A destination region that reaches beyond the source's own bounds
    /// reads back transparent for the out-of-range part rather than
    /// panicking or wrapping — `Target::get`'s existing out-of-bounds
    /// contract, which a snapshot must inherit rather than special-case.
    #[test]
    fn snapshot_from_reads_transparent_beyond_the_sources_edge() {
        let mut source = Target::new(4, 4);
        source.set(3, 3, red());

        // A destination positioned so it overhangs the source's bottom-right
        // edge.
        let mut dest = Target::new(4, 4);
        dest.snapshot_from(&source, (0, 0), (2, 2));

        assert_eq!(
            dest.get(1, 1),
            red(),
            "the one in-range pixel is still copied correctly"
        );
        assert_eq!(
            dest.get(3, 3),
            Premul::TRANSPARENT,
            "out of the source's range reads as transparent"
        );
    }

    /// Both `source_origin` and `dest_origin` participate — a fully general
    /// case where neither buffer sits at the device's `(0, 0)`, which is
    /// what a backdrop-filtered layer nested inside another layer produces
    /// (the parent frame is itself an offscreen buffer with its own
    /// nonzero origin).
    #[test]
    fn snapshot_from_composes_both_origins() {
        // Source is itself an offscreen buffer whose device-space top-left
        // is (100, 100), holding one marked pixel at absolute (108, 104).
        let mut source = Target::new(20, 20);
        source.set(8, 4, blue());

        // Destination's device-space top-left is (105, 102): absolute
        // (108, 104) should land at local (3, 2).
        let mut dest = Target::new(6, 6);
        dest.snapshot_from(&source, (100, 100), (105, 102));

        assert_eq!(dest.get(3, 2), blue());
    }

    // ── LCD subpixel compositing ─────────────────────────────────────────

    use super::super::geometry::fill::{CoverageMask, LcdMask, LCD_SUBPIXELS};
    use vieww_foundation::Rect as DeviceRect;

    /// An LCD mask whose three channels are equal is a gray mask in disguise,
    /// and `composite_lcd` must produce the *same bytes* the gray path
    /// produces for it — not close, not within a tolerance: the same, because
    /// the equal-channel branch deliberately calls the gray path's own
    /// `over(color.scaled(k), dst)` rather than re-deriving the arithmetic.
    /// This is what makes the LCD and gray paths unable to drift apart on the
    /// pixels where they agree.
    #[test]
    fn an_equal_channel_lcd_composite_is_the_gray_composite_bit_for_bit() {
        let ink = vieww_foundation::Color::rgb(30, 30, 30);
        let coverage = 0.6_f32;

        let gray = {
            let mut target = Target::new(8, 8);
            target.reset(8, 8, Premul::from_straight(vieww_foundation::Color::WHITE));
            let mask = CoverageMask {
                x0: 1,
                y0: 1,
                width: 4,
                height: 4,
                data: vec![coverage; 16],
            };
            target.composite_flat(
                mask.view(),
                (0, 0),
                vieww_foundation::BlendMode::Normal,
                ColorPipeline::GammaSpace,
                Premul::from_straight(ink),
            );
            target
        };
        let lcd = {
            let mut target = Target::new(8, 8);
            target.reset(8, 8, Premul::from_straight(vieww_foundation::Color::WHITE));
            let mask = LcdMask {
                x0: 1,
                y0: 1,
                width: 4,
                height: 4,
                data: vec![coverage; 16 * LCD_SUBPIXELS],
            };
            target.composite_lcd(
                mask.view_translated(0, 0, DeviceRect::new(-1.0e3, -1.0e3, 1.0e3, 1.0e3))
                    .expect("in range"),
                (0, 0),
                ink,
                |_, _| 1.0,
            );
            target
        };

        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(
                    gray.get(x, y),
                    lcd.get(x, y),
                    "pixel ({x},{y}) differs between the gray and equal-channel LCD paths"
                );
            }
        }
    }

    /// The split-channel case, asserted in the direction that makes physical
    /// sense: a stripe fully covering the R sub-column and not the B one
    /// blends *more red in and keeps more blue out*, over a white background
    /// with dark ink — so the resulting pixel's blue channel must sit above
    /// its red channel. White is 255 in every channel; dark ink is 30; the R
    /// stripe at full coverage pulls R all the way to 30 while B stays at
    /// 255, G lands between.
    #[test]
    fn a_split_channel_lcd_composite_pushes_red_in_and_keeps_blue_out() {
        let ink = vieww_foundation::Color::rgb(30, 30, 30);
        let mut target = Target::new(4, 4);
        target.reset(4, 4, Premul::from_straight(vieww_foundation::Color::WHITE));

        let mask = LcdMask {
            x0: 0,
            y0: 0,
            width: 2,
            height: 2,
            // Pixel 0: [1, 0.5, 0] — the centred-vertical-stem edge. Pixel 1:
            // [0, 0, 0], uncovered, same row.
            data: vec![1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.0, 0.0, 0.0],
        };
        target.composite_lcd(
            mask.view_translated(0, 0, DeviceRect::new(-1.0e3, -1.0e3, 1.0e3, 1.0e3))
                .expect("in range"),
            (0, 0),
            ink,
            |_, _| 1.0,
        );

        let px = target.get(0, 0).to_straight_u8();
        assert_eq!(px[0], 30, "full R coverage takes R to the ink colour");
        assert!(
            px[2] > px[1] && px[1] > px[0],
            "G at half coverage lands between R (full) and B (none): {:?}",
            px
        );
        assert_eq!(px[2], 255, "no B coverage leaves the background's B alone");
        // And the uncovered neighbour is untouched white.
        assert_eq!(
            target.get(1, 0).to_straight_u8(),
            [255, 255, 255, 255],
            "zero coverage on every channel writes nothing"
        );
    }
}
