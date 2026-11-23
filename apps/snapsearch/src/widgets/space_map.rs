//! The Space view — the library as a map, placed by embedding similarity.
//!
//! The app's whole premise is a joint embedding space, and the grid never
//! showed it: masonry is recency-shaped, not similarity-shaped. This view is
//! that space. Every photo's embedding is projected into 2D by a
//! force-directed layout — photos link to their nearest neighbours by cosine
//! similarity, repel everything else, and settle where alike ends up beside
//! alike. The layout runs through `vieww-dataviz::force` (the D3 `d3-force`
//! port: Barnes–Hut many-body, link springs, a cooling alpha), and it is
//! deterministic: same library, same map, every rebuild — no RNG is involved
//! and the simulation starts every node on the same phyllotaxis spiral.
//!
//! When a search has run, its matches wear a viridis ring on the map —
//! best match bright yellow, weaker ones deep blue, everything else dimmed
//! to context (`vieww-dataviz::color::viridis`). Seeing *where* the matches
//! land is something the ranked list cannot say: a query that pulls matches
//! from two corners of the map is a query describing two subjects, and the
//! map says so at a glance.
//!
//! Dots are coloured by their photo's mean thumbnail colour, so clusters are
//! visually thematic before any label is read. Tapping near a dot opens the
//! photo, exactly as tapping a tile does — the map is a navigation surface,
//! not a decoration.

use std::rc::Rc;

use vieww::foundation::{
    Color, Constraints, Gradient, Image as Image_, Offset, Rect, Size, Sketchbook,
};
use vieww::widget::prelude::*;
use vieww::widget::{widget_node_from, Handler, Painter, Painting};
use vieww_dataviz::force::Simulation;

use crate::embed::cosine;
use crate::photo::Photo;

/// How many nearest neighbours each photo links to.
const NEIGHBOURS: usize = 3;

/// How many force ticks to allow the layout at most.
const MAX_TICKS: usize = 400;

/// The base dot radius, in logical pixels.
const DOT: f32 = 4.5;

/// The match ring radius, and its stroke width.
const MATCH_RING: f32 = 8.5;
const MATCH_STROKE: f32 = 2.0;

/// How close a tap must land to open a photo, in map pixels.
const TAP_RADIUS: f32 = 22.0;

/// Where the map's photos sit, in the unit square `[0, 1]²`.
///
/// Built deterministically from the library's embeddings; identical input
/// produces an identical map, so a rebuild mid-session does not reshuffle
/// the dots under the finger.
pub struct SpaceLayout {
    /// One position per included photo, `0..=1` on both axes.
    pub positions: Vec<Offset>,
    /// Each included photo's mean thumbnail colour.
    pub colors: Vec<Color>,
    /// The k-nearest-neighbour graph the layout was wired with, as index
    /// pairs — drawn as the map's faint connective tissue.
    pub links: Vec<(usize, usize)>,
}

/// Up to this many photos the neighbour search is exact (O(n²), a few ms);
/// above it the search is approximate so a three-thousand-photo library does
/// not cost nine million dot products.
pub const EXACT_KNN_MAX: usize = 500;

/// Force ticks above [`EXACT_KNN_MAX`]: the layout converges long before the
/// full budget on a big graph, and every tick costs O(n log n).
const MAX_TICKS_LARGE: usize = 150;

/// Random projections used by the approximate search, and how far along each
/// sorted projection a photo looks for candidates.
const PROJECTIONS: usize = 6;
const WINDOW: usize = 14;

/// How many layouts have been computed in this process. Tests use it to prove
/// the cache is doing its job; it is otherwise inert.
pub static LAYOUTS_COMPUTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Each slot's best `k` neighbours as `(other, cosine)`, best first.
///
/// Exact for small inputs, approximate (random-projection candidates, exact
/// cosine on those) for large ones. Deterministic either way.
pub fn neighbours(vectors: &[Vec<f32>], k: usize) -> Vec<Vec<(usize, f32)>> {
    let n = vectors.len();
    let top = |slot: usize, candidates: &mut dyn Iterator<Item = usize>| {
        let a = &vectors[slot];
        let mut scored: Vec<(usize, f32)> = candidates
            .filter(|&other| other != slot)
            .map(|other| (other, cosine(a, &vectors[other])))
            .collect();
        scored.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
        scored.truncate(k);
        scored
    };

    if n <= EXACT_KNN_MAX {
        return (0..n).map(|slot| top(slot, &mut (0..n))).collect();
    }

    // Candidate sets from sorted random projections: points that are close
    // in the embedding are close along most directions, so a window around
    // each point's rank in each ordering holds its neighbours with high
    // probability.
    let dim = vectors[0].len();
    let mut candidates: Vec<Vec<usize>> = vec![Vec::with_capacity(PROJECTIONS * WINDOW * 2); n];
    for direction in 0..PROJECTIONS {
        let weights: Vec<f32> = (0..dim)
            .map(|d| {
                // A deterministic +/-1 per (direction, dimension).
                let h = crate::photo::hash_str(&format!("p{direction}:{d}"));
                if h & 1 == 0 { 1.0 } else { -1.0 }
            })
            .collect();
        let mut order: Vec<(usize, f32)> = vectors
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.iter().zip(&weights).map(|(x, w)| x * w).sum::<f32>()))
            .collect();
        order.sort_by(|x, y| x.1.total_cmp(&y.1).then(x.0.cmp(&y.0)));
        for (rank, &(slot, _)) in order.iter().enumerate() {
            let lo = rank.saturating_sub(WINDOW);
            let hi = (rank + WINDOW + 1).min(n);
            candidates[slot].extend(order[lo..hi].iter().map(|&(other, _)| other));
        }
    }
    (0..n)
        .map(|slot| {
            let mut list = std::mem::take(&mut candidates[slot]);
            list.sort_unstable();
            list.dedup();
            top(slot, &mut list.into_iter())
        })
        .collect()
}

impl SpaceLayout {
    /// Lay out `photos` by the embeddings in `vectors`, which are parallel
    /// to them — the caller filters out photos whose vector has not landed
    /// yet, so the map's photos and its positions stay index-aligned.
    pub fn build(photos: &[Photo], vectors: &[Vec<f32>]) -> Rc<Self> {
        let images: Vec<Image_> = photos.iter().map(|p| p.image.clone()).collect();
        Rc::new(Self::compute(&images, vectors))
    }

    /// The layout as plain data, so it can be computed on a worker thread
    /// (`Image_` is `Arc`-backed and `Send`; `Rc` is only for the UI side).
    pub fn compute(images: &[Image_], vectors: &[Vec<f32>]) -> Self {
        LAYOUTS_COMPUTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let n = images.len().min(vectors.len());
        let mut links = Vec::new();
        let mut positions = vec![Offset::new(0.5, 0.5); n];
        let colors = images.iter().take(n).map(mean_color).collect();

        if n > 1 {
            let mut sim = Simulation::new(n);
            // A wider spread than D3's default: this is a map, not a hairball.
            sim.charge = -70.0;
            sim.collide = 1.0;
            for (slot, near) in neighbours(&vectors[..n], NEIGHBOURS).into_iter().enumerate() {
                for (other, similarity) in near {
                    // Alike photos want to be *close*: similarity 1.0 maps to
                    // a short spring, 0.0 to a long one.
                    let distance = 160.0 - 130.0 * similarity.clamp(0.0, 1.0);
                    sim.link(slot, other, distance);
                    links.push((slot, other));
                }
            }
            sim.run(if n > EXACT_KNN_MAX { MAX_TICKS_LARGE } else { MAX_TICKS });

            // Normalise into the unit square with a margin, so no dot sits
            // on the map's edge where a tap target is half clipped.
            let mut min_x = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            for node in &sim.nodes {
                min_x = min_x.min(node.position.dx);
                max_x = max_x.max(node.position.dx);
                min_y = min_y.min(node.position.dy);
                max_y = max_y.max(node.position.dy);
            }
            let span_x = (max_x - min_x).max(1e-6);
            let span_y = (max_y - min_y).max(1e-6);
            let margin = 0.06;
            positions = sim
                .nodes
                .iter()
                .map(|node| {
                    let ux = (node.position.dx - min_x) / span_x;
                    let uy = (node.position.dy - min_y) / span_y;
                    Offset::new(
                        margin + ux * (1.0 - 2.0 * margin),
                        margin + uy * (1.0 - 2.0 * margin),
                    )
                })
                .collect();
        }

        Self { positions, colors, links }
    }

    /// The photo nearest to `point` (in map pixels), within the tap radius —
    /// the map's hit test.
    pub fn nearest(&self, point: Offset, size: Size) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (index, position) in self.positions.iter().enumerate() {
            let dx = position.dx * size.width - point.dx;
            let dy = position.dy * size.height - point.dy;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance <= TAP_RADIUS && best.is_none_or(|(_, d)| distance < d) {
                best = Some((index, distance));
            }
        }
        best.map(|(index, _)| index)
    }
}

/// A photo's mean colour, from its thumbnail, lifted a little toward white
/// so dark photos stay visible on the dark map.
fn mean_color(image: &Image_) -> Color {
    let px = image.pixels();
    let mut r = 0u64;
    let mut g = 0u64;
    let mut b = 0u64;
    let mut count = 0u64;
    let mut i = 0;
    while i + 2 < px.len() {
        r += u64::from(px[i]);
        g += u64::from(px[i + 1]);
        b += u64::from(px[i + 2]);
        count += 1;
        i += 4;
    }
    if count == 0 {
        return Color::rgb(128, 128, 128);
    }
    Color::rgb(
        (r / count).min(u64::from(u8::MAX)) as u8,
        (g / count).min(u64::from(u8::MAX)) as u8,
        (b / count).min(u64::from(u8::MAX)) as u8,
    )
    .lerp(Color::WHITE, 0.25)
}

/// The map widget: the painter's input is plain data, the gestures are a
/// single tap detector over the whole surface.
pub struct SpaceMap {
    /// The photos the layout placed, in the layout's order.
    pub photos: Vec<Photo>,
    pub layout: Rc<SpaceLayout>,
    /// One score per placed photo, `0..=1` relative to the best match, when
    /// a search has run.
    pub scores: Vec<Option<f32>>,
    /// A search is on screen — non-matching dots dim to context.
    pub has_matches: bool,
    pub on_tap: Handler<Photo>,
}

impl std::fmt::Debug for SpaceMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpaceMap")
            .field("dots", &self.layout.positions.len())
            .field("has_matches", &self.has_matches)
            .finish()
    }
}

impl Widget for SpaceMap {
    fn debug_name(&self) -> &'static str {
        "SpaceMap"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        // The map's positions live in the unit square, so both the painter
        // and the hit test need the laid-out size — a `LayoutBuilder` reports
        // exactly that, once.
        let photos = self.photos.clone();
        let layout = Rc::clone(&self.layout);
        let scores = self.scores.clone();
        let has_matches = self.has_matches;
        let on_tap = Rc::clone(&self.on_tap);

        LayoutBuilder::new(move |constraints: Constraints| {
            let width = if constraints.has_bounded_width() {
                constraints.max_width
            } else {
                0.0
            };
            let height = if constraints.has_bounded_height() {
                constraints.max_height
            } else {
                0.0
            };
            let size = Size::new(width.max(0.0), height.max(0.0));

            let tap_photos = photos.clone();
            let tap_layout = Rc::clone(&layout);
            let open = Rc::clone(&on_tap);
            let detector =
                GestureDetector::new().on_tap(move |tap: vieww::foundation::TapDetails| {
                    if let Some(index) = tap_layout.nearest(tap.local, size) {
                        if let Some(photo) = tap_photos.get(index) {
                            open(photo.clone());
                        }
                    }
                });

            detector
                .child(Painting::new(SpacePainter {
                    layout: Rc::clone(&layout),
                    scores: scores.clone(),
                    has_matches,
                    background: Color::rgb(14, 12, 22),
                    link: Color::rgba(120, 110, 180, 26),
                    dimmed: Color::rgba(120, 116, 128, 70),
                }))
                .into()
        })
        .into()
    }
}

widget_node_from!(SpaceMap);

/// The map's painter: plate, links, dots, match rings.
struct SpacePainter {
    layout: Rc<SpaceLayout>,
    scores: Vec<Option<f32>>,
    has_matches: bool,
    background: Color,
    link: Color,
    dimmed: Color,
}

impl std::fmt::Debug for SpacePainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpacePainter")
            .field("dots", &self.layout.positions.len())
            .field("has_matches", &self.has_matches)
            .finish()
    }
}

impl Painter for SpacePainter {
    fn paint(&self, book: &mut Sketchbook, size: Size) {
        if size.width < 8.0 || size.height < 8.0 {
            return;
        }

        // The plate: a near-black field with the faintest vertical gradient,
        // so the map reads as a *place* rather than a scatter on the page.
        let top = self.background.lerp(Color::rgb(26, 22, 40), 0.6);
        book.rect(
            Rect::new(0.0, 0.0, size.width, size.height),
            Gradient::vertical().between(top, self.background),
        );

        let to_px = |p: Offset| Offset::new(p.dx * size.width, p.dy * size.height);

        // The connective tissue: the kNN graph, barely there.
        for (a, b) in &self.layout.links {
            let pa = to_px(self.layout.positions[*a]);
            let pb = to_px(self.layout.positions[*b]);
            book.line(pa, pb, self.link, 1.0);
        }

        // The dots. With a search on screen, non-matches sit dimmed as
        // context and matches wear the viridis ring their score earned.
        for (index, position) in self.layout.positions.iter().enumerate() {
            let point = to_px(*position);
            let color = self.layout.colors[index];
            let score = self.scores.get(index).copied().flatten();

            match score {
                Some(s) => {
                    let ramp = vieww_dataviz::color::viridis(s.clamp(0.0, 1.0));
                    book.ring(point, MATCH_RING, MATCH_STROKE, ramp);
                    book.circle(point, DOT + 1.5, color);
                }
                None => {
                    let dot = if self.has_matches {
                        self.dimmed
                    } else {
                        color
                    };
                    book.circle(point, DOT, dot);
                }
            }
        }
    }

    fn should_repaint(&self, previous: &dyn Painter) -> bool {
        match previous.as_any().downcast_ref::<Self>() {
            Some(previous) => {
                !Rc::ptr_eq(&self.layout, &previous.layout)
                    || self.scores != previous.scores
                    || self.has_matches != previous.has_matches
                    || self.background != previous.background
                    || self.link != previous.link
                    || self.dimmed != previous.dimmed
            }
            None => true,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::Embedder;
    use crate::photo;

    fn photos_with_vectors() -> (Vec<Photo>, Vec<Vec<f32>>) {
        let photos: Vec<Photo> = (*photo::sample_library(24)).clone();
        // Distinct vectors: the local embedder derives them from the photos
        // themselves, so clusters already exist in the sample set.
        let embedder = crate::embed::local::LocalEmbedder::new();
        let vectors = photos
            .iter()
            .map(|p| embedder.embed_image(&p.image))
            .collect();
        (photos, vectors)
    }

    #[test]
    fn every_position_lands_in_the_unit_square() {
        let (photos, vectors) = photos_with_vectors();
        let layout = SpaceLayout::build(&photos, &vectors);
        for p in &layout.positions {
            assert!((0.0..=1.0).contains(&p.dx), "x out of range: {}", p.dx);
            assert!((0.0..=1.0).contains(&p.dy), "y out of range: {}", p.dy);
        }
        assert_eq!(layout.positions.len(), photos.len());
    }

    #[test]
    fn the_knn_graph_exists_and_never_exceeds_the_degree_bound() {
        let (photos, vectors) = photos_with_vectors();
        let layout = SpaceLayout::build(&photos, &vectors);
        assert!(!layout.links.is_empty(), "a library with 24 photos has links");
        assert!(
            layout.links.len() <= photos.len() * NEIGHBOURS,
            "each photo links at most NEIGHBOURS times"
        );
    }

    #[test]
    fn a_shorter_input_places_fewer_dots() {
        let (photos, vectors) = photos_with_vectors();
        let layout = SpaceLayout::build(&photos[..8], &vectors[..8]);
        assert_eq!(layout.positions.len(), 8);
    }

    #[test]
    fn the_layout_is_deterministic_across_rebuilds() {
        let (photos, vectors) = photos_with_vectors();
        let a = SpaceLayout::build(&photos, &vectors);
        let b = SpaceLayout::build(&photos, &vectors);
        assert_eq!(a.positions, b.positions, "same input, same map");
    }

    /// `n` unit vectors in clusters, so there are real neighbours to find.
    fn clustered(n: usize, dim: usize) -> Vec<Vec<f32>> {
        (0..n)
            .map(|i| {
                let cluster = i % 20;
                let mut v: Vec<f32> = (0..dim)
                    .map(|d| {
                        let base = ((cluster * 31 + d * 17) % 13) as f32 - 6.0;
                        let jitter = ((i * 7 + d * 3) % 5) as f32 * 0.05;
                        base + jitter
                    })
                    .collect();
                crate::embed::normalize(&mut v);
                v
            })
            .collect()
    }

    #[test]
    fn above_the_exact_limit_the_approximate_search_still_finds_real_neighbours() {
        let vectors = clustered(900, 32);
        let approx = neighbours(&vectors, NEIGHBOURS);
        assert_eq!(approx.len(), 900);
        // Compare the neighbours' similarity with the exact best: the
        // approximation may pick a different tie, but must not be far off.
        let mut gap = 0.0f32;
        for slot in (0..900).step_by(30) {
            let mut exact: Vec<f32> = (0..900)
                .filter(|&o| o != slot)
                .map(|o| cosine(&vectors[slot], &vectors[o]))
                .collect();
            exact.sort_by(|a, b| b.total_cmp(a));
            gap += exact[0] - approx[slot][0].1;
            assert_eq!(approx[slot].len(), NEIGHBOURS);
        }
        assert!(gap / 30.0 < 0.02, "approximate best is within 0.02 of exact on average: {gap}");
    }

    #[test]
    fn a_large_library_lays_out_in_bounded_time_and_stays_in_the_unit_square() {
        let vectors = clustered(1500, 32);
        let images: Vec<Image_> = (0..1500).map(|i| photo::photo_for_seed(i % 40).image).collect();
        let started = std::time::Instant::now();
        let layout = SpaceLayout::compute(&images, &vectors);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "1500 photos took {:?}",
            started.elapsed()
        );
        assert_eq!(layout.positions.len(), 1500);
        assert!(layout.positions.iter().all(|p| (0.0..=1.0).contains(&p.dx) && (0.0..=1.0).contains(&p.dy)));
    }

    #[test]
    fn a_tap_near_a_dot_finds_that_dot() {
        let (photos, vectors) = photos_with_vectors();
        let layout = SpaceLayout::build(&photos, &vectors);
        let size = Size::new(390.0, 600.0);
        let first = layout.positions[0];
        let point = Offset::new(first.dx * size.width + 3.0, first.dy * size.height - 3.0);
        assert!(layout.nearest(point, size).is_some(), "3px off a dot is on it");
        // A point in the far corner of a spread-out map may be near nothing.
        let far = Offset::new(size.width, size.height);
        let _ = layout.nearest(far, size);
    }
}
