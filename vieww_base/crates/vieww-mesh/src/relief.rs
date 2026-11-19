//! Image → 3D: turning a picture into geometry.
//!
//! # What this module is
//!
//! Two builders that take a raster image and return a [`Mesh`] — the same
//! type the OBJ and glTF loaders produce, ready for a `Node` in a scene:
//!
//! * [`heightmap_relief`] — a plane displaced by an image's shading, the
//!   terrain/emboss/engraving path. One height per grid vertex, computed
//!   from the source's luminance; the picture's own pixels become the
//!   texture that completes the illusion.
//! * [`silhouette_solid`] — a *closed* solid grown from an image's alpha
//!   silhouette, the coin/medallion/bas-relief path. A figure photographed
//!   or drawn on a plain background becomes a rounded, free-standing slab
//!   whose front and back carry the picture and whose rim follows the
//!   outline.
//!
//! # Why builders, and not a displace modifier with a texture
//!
//! Blender has both (Displace with an image texture, and Multires
//! sculpting); vieww's [`Modifier::Displace`] is noise-only because a
//! modifier operates on an existing mesh, and an image is not something a
//! stack step can ask the user to ship alongside it. An image is *source
//! data*, not a deformation — so the entry points here take the image as
//! the primary argument and build the geometry from scratch, the way
//! [`lathe`] builds from a profile and [`Metaballs::mesh`] from a field.
//!
//! The one place a picture and a mesh genuinely meet mid-way is the
//! *sculpt* parameter of [`SolidSpec`]: a silhouette extrusion whose depth
//! is modulated by the picture's own shading (light = raised, dark =
//! recessed) stops being a pillow and becomes a bas-relief — which is how
//! an anatomical illustration becomes something with volume.
//!
//! # The sampling contract
//!
//! Both builders read pixels through [`Image`]'s RGBA8 buffer — no GPU, no
//! mips, no wrap modes. Coverage and depth are computed on a box-blurred
//! copy of the alpha (and, for sculpt, the luminance), because a relief's
//! outline quality is decided by the smoothing, not the resolution: a
//! 128-cell grid over a smoothed field looks organic, the same grid over
//! raw pixels looks like voxel art.
//!
//! [`Modifier::Displace`]: crate::modifiers::Modifier::Displace
//! [`lathe`]: crate::modifiers::lathe
//! [`Metaballs::mesh`]: crate::implicit::Metaballs::mesh

use vieww_foundation::Image;

use crate::Mesh;

/// The coverage level that counts as "inside" the silhouette.
///
/// Half coverage is where the picture's edge formally lies, so it is where
/// the solid's surface is defined to meet its full depth — and, in the
/// same stroke, where the rim's taper reaches zero.
const LEVEL: f32 = 0.5;

// ---------------------------------------------------------------------------
// Field sampling
// ---------------------------------------------------------------------------

/// A blurred scalar field sampled from an image, one value per grid cell.
///
/// This is the module's private workhorse: silhouette coverage, luminance,
/// anything per-pixel becomes one of these, at grid resolution, smoothed.
struct Field {
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Field {
    /// Sample an image into a `grid_w` × `grid_h` field of `channel`
    /// values, box-blurred `blur` times.
    ///
    /// Bilinear sampling of the source keeps a coarse grid from aliasing
    /// the picture's detail into moiré before the blur ever runs; the blur
    /// then does the job a marching-cubes grid would do — making the level
    /// set at [`LEVEL`] smooth enough to read as a curve.
    fn sample<F>(image: &Image, grid_w: usize, grid_h: usize, blur: u32, channel: F) -> Self
    where
        F: Fn(&[u8]) -> f32,
    {
        let (iw, ih) = (image.width() as usize, image.height() as usize);
        let px = image.pixels();
        let mut values = vec![0.0_f32; grid_w * grid_h];
        for (gy, row) in values.chunks_mut(grid_w).enumerate() {
            // Cell centers, not corners: grid row gy maps to the source
            // pixel at (gy + 0.5) / grid_h — no edge row of the picture is
            // systematically oversampled.
            let fy = (gy as f32 + 0.5) * ih as f32 / grid_h as f32 - 0.5;
            let y0 = fy.floor().clamp(0.0, ih.saturating_sub(1) as f32) as usize;
            let y1 = (y0 + 1).min(ih - 1);
            let ty = (fy - y0 as f32).clamp(0.0, 1.0);
            for (gx, cell) in row.iter_mut().enumerate() {
                let fx = (gx as f32 + 0.5) * iw as f32 / grid_w as f32 - 0.5;
                let x0 = fx.floor().clamp(0.0, iw.saturating_sub(1) as f32) as usize;
                let x1 = (x0 + 1).min(iw - 1);
                let tx = (fx - x0 as f32).clamp(0.0, 1.0);
                let p00 = &px[(y0 * iw + x0) * 4..];
                let p10 = &px[(y0 * iw + x1) * 4..];
                let p01 = &px[(y1 * iw + x0) * 4..];
                let p11 = &px[(y1 * iw + x1) * 4..];
                let top = channel(p00) * (1.0 - tx) + channel(p10) * tx;
                let bottom = channel(p01) * (1.0 - tx) + channel(p11) * tx;
                *cell = top * (1.0 - ty) + bottom * ty;
            }
        }
        let mut field = Self {
            width: grid_w,
            height: grid_h,
            values,
        };
        for _ in 0..blur {
            field.blur();
        }
        field
    }

    /// A value by cell index, zero outside the grid — so boundary lookups
    /// never panic and never invent mass.
    fn get(&self, x: usize, y: usize) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.values[y * self.width + x]
    }

    /// One 3×3 box pass, clamped at the borders.
    ///
    /// Separable would be faster; at grid resolutions (≤ 512 cells on a
    /// side) the difference is milliseconds once, at startup, and the
    /// clamped border — which keeps mass inside the frame — is the
    /// property that matters more than pass count.
    fn blur(&mut self) {
        let (w, h) = (self.width, self.height);
        let mut out = self.values.clone();
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                let mut n = 0.0;
                for dy in -1_i32..=1 {
                    for dx in -1_i32..=1 {
                        let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                        if xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h {
                            sum += self.values[yy as usize * w + xx as usize];
                            n += 1.0;
                        }
                    }
                }
                out[y * w + x] = sum / n;
            }
        }
        self.values = out;
    }
}

/// Rec. 709 luminance of an RGBA8 pixel, 0..=1.
fn luminance(px: &[u8]) -> f32 {
    (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0
}

/// The alpha of an RGBA8 pixel, 0..=1.
fn alpha(px: &[u8]) -> f32 {
    px[3] as f32 / 255.0
}

// ---------------------------------------------------------------------------
// heightmap_relief
// ---------------------------------------------------------------------------

/// How a relief reads the picture it is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliefChannel {
    /// Luminance: light rises, dark sinks. Terrain, embossing.
    Luminance,
    /// Luminance inverted: dark rises, light sinks. Engraving, intaglio.
    InvertedLuminance,
    /// Alpha: opaque rises, transparent sinks. Cut-out depth.
    Alpha,
}

/// Parameters for [`heightmap_relief`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReliefSpec {
    /// The plane's width in world units. Height follows the image's aspect.
    pub width: f32,
    /// How far the brightest pixel rises above the plane.
    pub strength: f32,
    /// Grid cells across the image's long axis. Detail, at the cost of
    /// triangles; 64 is a coin, 256 is a landscape print.
    pub resolution: u32,
    /// Which image channel drives the displacement, and in which direction.
    pub channel: ReliefChannel,
    /// Box-blur passes over the sampled field. 0 is bit-crisp; 2 is the
    /// embossed-print look.
    pub smoothing: u32,
}

impl Default for ReliefSpec {
    fn default() -> Self {
        Self {
            width: 1.0,
            strength: 0.25,
            resolution: 128,
            channel: ReliefChannel::Luminance,
            smoothing: 1,
        }
    }
}

/// A plane displaced by an image's shading — the emboss/terrain path.
///
/// The result is an open surface (no back, no rim): a sheet pushed up by
/// the picture, in the XY plane facing +Z, centered on the origin, with
/// UVs mapping every vertex to the source pixel. Apply the source image as
/// the material's map and the two — geometry and picture — agree by
/// construction.
///
/// Vertex rows run along +Y to the picture's top, because vieww-3d's
/// `Texture::sample` treats v = 0 as the top row (glTF's convention): an
/// upright picture stays upright on an upright plane. A plain name, not a
/// link — `vieww-mesh` deliberately does not depend on `vieww-3d`, so
/// rustdoc cannot resolve one across that boundary.
#[must_use]
pub fn heightmap_relief(spec: &ReliefSpec, image: &Image) -> Mesh {
    let aspect = image.height() as f32 / image.width().max(1) as f32;
    let gw = spec.resolution.max(2) as usize;
    let gh = ((gw as f32 * aspect).round() as usize).max(2);
    let read = |px: &[u8]| match spec.channel {
        ReliefChannel::Luminance => luminance(px),
        ReliefChannel::InvertedLuminance => 1.0 - luminance(px),
        ReliefChannel::Alpha => alpha(px),
    };
    let field = Field::sample(image, gw, gh, spec.smoothing, read);

    let height = spec.width * aspect;
    let mut mesh = Mesh::default();
    for y in 0..=gh {
        for x in 0..=gw {
            let (u, v) = (x as f32 / gw as f32, y as f32 / gh as f32);
            // Sample the cell to the vertex's lower-left, clamped: the
            // grid has one more vertex row/column than cells.
            let h = field.get(x.min(gw - 1), y.min(gh - 1)) * spec.strength;
            mesh.positions.push([
                -spec.width / 2.0 + u * spec.width,
                height / 2.0 - v * height,
                h,
            ]);
            mesh.uvs.push([u, v]);
        }
    }
    for y in 0..gh {
        for x in 0..gw {
            let a = (y * (gw + 1) + x) as u32;
            let b = a + 1;
            let c = a + (gw + 1) as u32;
            let d = c + 1;
            mesh.indices.extend_from_slice(&[a, c, d, a, d, b]);
        }
    }
    mesh.compute_normals();
    mesh
}

// ---------------------------------------------------------------------------
// silhouette_solid
// ---------------------------------------------------------------------------

/// Parameters for [`silhouette_solid`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidSpec {
    /// The long axis of the mesh, world units. The image's aspect decides
    /// everything else — a standing figure is as tall as this is wide.
    pub size: f32,
    /// The half-thickness at the silhouette's heart: where the (blurred)
    /// alpha is fully opaque, the surface sits `depth` from the mid-plane.
    pub depth: f32,
    /// Grid cells across the long axis. The outline is as good as this is
    /// fine; 96 reads as a medallion, 192 as a figurine.
    pub resolution: u32,
    /// Box-blur passes over the alpha field before the level set is taken.
    /// This is the roundness control: more passes, softer edges.
    pub smoothing: u32,
    /// How much the picture's shading sculpts the surface, 0..=1. At 0 a
    /// uniform pillow; at 1 light areas keep full depth while dark areas
    /// sink toward the mid-plane — the bas-relief register.
    pub sculpt: f32,
}

impl Default for SolidSpec {
    fn default() -> Self {
        Self {
            size: 1.0,
            depth: 0.08,
            resolution: 128,
            smoothing: 2,
            sculpt: 0.0,
        }
    }
}

/// The two fields and the vertex tables a silhouette build shares.
struct SolidBuilder<'a> {
    spec: &'a SolidSpec,
    coverage: Field,
    shading: Option<Field>,
    /// World extents, centered on the origin.
    half_w: f32,
    half_h: f32,
    /// Cells across/down.
    gw: usize,
    gh: usize,
    /// Front (+z) and back (−z) vertex index per grid corner, `u32::MAX`
    /// until written — the tables are the memo that keeps shared corners
    /// one vertex instead of four.
    front: Vec<u32>,
    back: Vec<u32>,
    mesh: Mesh,
}

impl<'a> SolidBuilder<'a> {
    /// Depth at a grid *corner*, as a fraction of `spec.depth`-scaled
    /// world units.
    ///
    /// The square root of the coverage *rescaled to the level set* is a
    /// disc's profile seen edge-on: full depth where coverage is at least
    /// [`LEVEL`], tapering to zero as coverage falls to nothing, which is
    /// what rounds the silhouette's boundary into a rim instead of a knife
    /// edge. Sculpt then pulls dark areas toward the mid-plane.
    fn depth_at(&self, x: usize, y: usize) -> f32 {
        let fill = (self.coverage.get(x, y) / LEVEL).min(1.0);
        let mut d = fill.sqrt();
        if let Some(shade) = &self.shading {
            let s = shade.get(x, y).clamp(0.0, 1.0);
            d *= 1.0 - self.spec.sculpt_factor(s);
        }
        d * self.spec.depth
    }

    /// Whether the corner is inside the level set.
    fn inside(&self, x: usize, y: usize) -> bool {
        self.coverage.get(x, y) > LEVEL
    }

    /// Whether a cell carries geometry: conservative, any inside corner,
    /// so the rim lands *outside* the last inside corner and the
    /// silhouette's skin is never clipped.
    fn solid_cell(&self, cx: usize, cy: usize) -> bool {
        self.inside(cx, cy)
            || self.inside(cx + 1, cy)
            || self.inside(cx, cy + 1)
            || self.inside(cx + 1, cy + 1)
    }

    /// The front or back vertex at a grid corner, creating it on first
    /// use. Corners have picture UVs, so a textured material shows the
    /// source on both faces.
    fn vertex(&mut self, x: usize, y: usize, front: bool) -> u32 {
        let slot = y * (self.gw + 1) + x;
        let taken = if front {
            self.front[slot]
        } else {
            self.back[slot]
        };
        if taken != u32::MAX {
            return taken;
        }
        let (u, v) = (x as f32 / self.gw as f32, y as f32 / self.gh as f32);
        let d = self.depth_at(x.min(self.gw - 1), y.min(self.gh - 1));
        let index = self.mesh.positions.len() as u32;
        self.mesh.positions.push([
            -self.half_w + u * 2.0 * self.half_w,
            self.half_h - v * 2.0 * self.half_h,
            if front { d } else { -d },
        ]);
        self.mesh.uvs.push([u, v]);
        if front {
            self.front[slot] = index;
        } else {
            self.back[slot] = index;
        }
        index
    }

    /// The front face of a cell, CCW seen from +Z.
    fn front_face(&mut self, cx: usize, cy: usize) {
        let a = self.vertex(cx, cy, true);
        let b = self.vertex(cx + 1, cy, true);
        let c = self.vertex(cx, cy + 1, true);
        let d = self.vertex(cx + 1, cy + 1, true);
        self.mesh.indices.extend_from_slice(&[a, c, d, a, d, b]);
    }

    /// The back face of a cell, CCW seen from −Z.
    fn back_face(&mut self, cx: usize, cy: usize) {
        let a = self.vertex(cx, cy, false);
        let b = self.vertex(cx + 1, cy, false);
        let c = self.vertex(cx, cy + 1, false);
        let d = self.vertex(cx + 1, cy + 1, false);
        self.mesh.indices.extend_from_slice(&[a, d, c, a, b, d]);
    }

    /// The rim wall along a boundary edge, walking `a → b`.
    ///
    /// Every rim of the build walks its edge counter-clockwise around the
    /// solid (left edges downward, bottom edges rightward, and so on), so
    /// the vertical seam this quad closes at its start (back → front) is
    /// the exact opposite of the seam the previous quad in the walk closed
    /// at its end (front → back) — which is what makes the whole boundary
    /// watertight, edge for edge, with no special cases.
    fn rim(&mut self, ax: usize, ay: usize, bx: usize, by: usize) {
        let fa = self.vertex(ax, ay, true);
        let ba = self.vertex(ax, ay, false);
        let fb = self.vertex(bx, by, true);
        let bb = self.vertex(bx, by, false);
        self.mesh
            .indices
            .extend_from_slice(&[fa, fb, bb, fa, bb, ba]);
    }
}

/// A closed solid grown from an image's alpha silhouette — the coin path.
///
/// # How it is built
///
/// The alpha is sampled onto a grid and blurred, and the mesh is the solid
/// between two displaced copies of that field: a front face at `+d(x, y)`
/// and a back face at `−d(x, y)`, where `d` follows the rounded profile the
/// builder's private `depth_at` describes. Interior cells contribute a
/// front quad and a back quad; every edge a solid cell shares with a
/// non-solid neighbour contributes a rim quad. Each rim walks
/// counter-clockwise around the solid, so consecutive rims close each
/// other's vertical seams and the result is watertight — every directed
/// edge used exactly once, renderable from any side, trustable by a
/// boolean op or a physics hull.
///
/// One degeneracy is accepted rather than patched: where two solid regions
/// touch only diagonally (a checkerboard corner), the two boundary loops
/// share a vertex without sharing edges, the same non-manifold corner a
/// real modeller produces when two cubes meet at a point. Smoothing makes
/// it vanishingly rare on real pictures; nothing renders wrongly when it
/// happens.
///
/// With `sculpt > 0`, `d` is additionally scaled by the picture's local
/// luminance — highlights keep the full depth and shadows sink, turning
/// the pillow into a bas-relief of whatever the picture shows.
#[must_use]
pub fn silhouette_solid(spec: &SolidSpec, image: &Image) -> Mesh {
    let (gw, gh) = grid_for(image, spec.resolution);
    let coverage = Field::sample(image, gw, gh, spec.smoothing, alpha);
    let shading = (spec.sculpt > f32::EPSILON)
        .then(|| Field::sample(image, gw, gh, spec.smoothing, luminance));

    // World extents: the long axis is `size`, the short axis follows the
    // image's aspect, and the mesh is centered on the origin.
    let portrait = image.height() >= image.width();
    let (half_w, half_h) = if portrait {
        let h = spec.size * 0.5;
        (h * image.width() as f32 / image.height() as f32, h)
    } else {
        let w = spec.size * 0.5;
        (w, w * image.height() as f32 / image.width() as f32)
    };

    let mut b = SolidBuilder {
        spec,
        coverage,
        shading,
        half_w,
        half_h,
        gw,
        gh,
        front: vec![u32::MAX; (gw + 1) * (gh + 1)],
        back: vec![u32::MAX; (gw + 1) * (gh + 1)],
        mesh: Mesh::default(),
    };

    for cy in 0..gh {
        for cx in 0..gw {
            if !b.solid_cell(cx, cy) {
                continue;
            }
            b.front_face(cx, cy);
            b.back_face(cx, cy);
            // A rim per cell edge whose neighbour across it carries no
            // geometry — the grid's border counts as "no neighbour".
            let (left, right, below, above) = (
                cx == 0 || !b.solid_cell(cx - 1, cy),
                cx + 1 == gw || !b.solid_cell(cx + 1, cy),
                cy == 0 || !b.solid_cell(cx, cy - 1),
                cy + 1 == gh || !b.solid_cell(cx, cy + 1),
            );
            // Each rim walks counter-clockwise: left edges downward,
            // bottom edges rightward, right edges upward, top edges
            // leftward — see `SolidBuilder::rim`.
            if left {
                b.rim(cx, cy + 1, cx, cy);
            }
            if below {
                b.rim(cx, cy, cx + 1, cy);
            }
            if right {
                b.rim(cx + 1, cy, cx + 1, cy + 1);
            }
            if above {
                b.rim(cx + 1, cy + 1, cx, cy + 1);
            }
        }
    }

    b.mesh.compute_normals();
    b.mesh
}

/// The grid dimensions for an image at a resolution: the long axis gets
/// `resolution` cells, the short axis follows the aspect, both ≥ 2.
fn grid_for(image: &Image, resolution: u32) -> (usize, usize) {
    let (iw, ih) = (image.width().max(1) as f32, image.height().max(1) as f32);
    let long = resolution.max(2) as f32;
    if ih >= iw {
        let gw = ((long * iw / ih).round() as usize).max(2);
        (gw, long as usize)
    } else {
        let gh = ((long * ih / iw).round() as usize).max(2);
        (long as usize, gh)
    }
}

impl SolidSpec {
    /// The sculpt factor applied to a shading sample: none at 0, most of
    /// the recess at 1, lifted short of all of it so a shadow never
    /// punctures the surface.
    fn sculpt_factor(&self, shade: f32) -> f32 {
        self.sculpt.clamp(0.0, 1.0) * (1.0 - shade) * 0.9
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16×16 image with a centered opaque 8×8 square.
    fn square_image() -> Image {
        let mut px = vec![0_u8; 16 * 16 * 4];
        for y in 4..12 {
            for x in 4..12 {
                let p = (y * 16 + x) * 4;
                px[p] = 220;
                px[p + 1] = 200;
                px[p + 2] = 190;
                px[p + 3] = 255;
            }
        }
        Image::from_rgba8(px, 16, 16)
    }

    /// A fully opaque image, brighter at the top.
    fn gradient_image() -> Image {
        let mut px = vec![0_u8; 8 * 8 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let p = (y * 8 + x) * 4;
                let v = 255 - (y * 30).min(255) as u8;
                px[p] = v;
                px[p + 1] = v;
                px[p + 2] = v;
                px[p + 3] = 255;
            }
        }
        Image::from_rgba8(px, 8, 8)
    }

    #[test]
    fn relief_heights_follow_the_channel() {
        let spec = ReliefSpec {
            width: 2.0,
            strength: 0.5,
            resolution: 16,
            channel: ReliefChannel::Luminance,
            smoothing: 0,
        };
        let mesh = heightmap_relief(&spec, &gradient_image());
        assert!(mesh.has_normals() && mesh.has_uvs());
        let (min, max) = mesh.bounds().expect("non-empty");
        // The top row (white) is at full height; in luminance mode the
        // plane never sinks below zero.
        assert!(max[2] > 0.4, "highlight rises, got {}", max[2]);
        assert!(min[2] >= -1e-4, "no sink: {}", min[2]);
    }

    #[test]
    fn intaglio_inverts() {
        let up = heightmap_relief(
            &ReliefSpec {
                resolution: 8,
                smoothing: 0,
                ..ReliefSpec::default()
            },
            &gradient_image(),
        );
        let down = heightmap_relief(
            &ReliefSpec {
                resolution: 8,
                smoothing: 0,
                channel: ReliefChannel::InvertedLuminance,
                ..ReliefSpec::default()
            },
            &gradient_image(),
        );
        assert!(up.bounds().unwrap().1[2] > down.bounds().unwrap().1[2]);
    }

    #[test]
    fn solid_is_watertight_and_sized() {
        let spec = SolidSpec {
            size: 2.0,
            depth: 0.1,
            resolution: 24,
            smoothing: 2,
            sculpt: 0.0,
        };
        let mesh = silhouette_solid(&spec, &square_image());
        assert!(mesh.triangles() > 0);
        assert!(mesh.has_normals() && mesh.has_uvs());

        // Watertight: every directed edge used exactly once, each with its
        // opposite also used exactly once — two faces per edge, all
        // consistently oriented.
        let mut directed = std::collections::HashMap::new();
        for tri in mesh.indices.as_chunks::<3>().0 {
            for k in 0..3 {
                let a = tri[k];
                let b = tri[(k + 1) % 3];
                *directed.entry((a, b)).or_insert(0_u32) += 1;
            }
        }
        for ((a, b), n) in &directed {
            assert_eq!(*n, 1, "edge {a}->{b} used {n} times");
            assert_eq!(
                directed.get(&(*b, *a)),
                Some(&1),
                "edge {a}->{b} has no opposite face"
            );
        }

        // The heart of the square stands at ±depth; the plan fits in the
        // image's aspect square of `size`.
        let (min, max) = mesh.bounds().expect("bounds");
        assert!(max[2] <= spec.depth + 1e-3 && min[2] >= -spec.depth - 1e-3);
        assert!(max[0] - min[0] <= spec.size + 1e-3);
        assert!(max[1] - min[1] <= spec.size + 1e-3);
        assert!(
            max[2] > 0.5 * spec.depth,
            "heart reaches depth, got {}",
            max[2]
        );
    }

    #[test]
    fn sculpt_recesses_without_puncturing() {
        let spec = SolidSpec {
            size: 1.0,
            depth: 0.3,
            resolution: 24,
            smoothing: 1,
            sculpt: 1.0,
        };
        let mesh = silhouette_solid(&spec, &gradient_image());
        let (min, max) = mesh.bounds().expect("bounds");
        // Dark areas sink toward the plane but never through it.
        assert!(min[2] >= -spec.depth, "no puncture: {}", min[2]);
        assert!(max[2] <= spec.depth + 1e-3);
        assert!(max[2] > min[2] + 1e-3, "sculpt produced relief");
    }

    #[test]
    fn empty_alpha_is_empty_mesh() {
        let blank = Image::from_rgba8(vec![0_u8; 8 * 8 * 4], 8, 8);
        let mesh = silhouette_solid(&SolidSpec::default(), &blank);
        assert_eq!(mesh.triangles(), 0);
    }
}
