//! **A retained, damaged repaint must produce the same bytes as a full one.**
//!
//! This is the single test that makes incremental rendering safe to turn on.
//! Every other property of a damage pipeline — that it is faster, that it
//! touches fewer commands, that the regions are small — is worth nothing if
//! the picture it leaves on the screen differs from the picture a full
//! repaint would have left, because the difference does not announce itself:
//! it is one stale rectangle in a corner, in one theme, after one particular
//! interaction, and it survives every test that only looks at what changed.
//!
//! So each case here renders a scene, changes it, and then compares two
//! answers for the *new* scene: a full render from scratch, and a retained
//! render given only the damage between the two scenes. They must be equal
//! byte for byte, not nearly equal — a tolerance here would hide exactly the
//! class of fault the test exists for.
//!
//! The cases are chosen for the ways a partial repaint classically goes
//! wrong: something that *moved* (its old pixels must be gone, which is why
//! the region is cleared before it is redrawn), something with a *shadow*
//! (whose ink reaches well outside the shape's own bounds), something inside
//! a *clip* (whose mask must resolve the same way over a region as over the
//! window), and something *blurred* (whose layer must be confined to the
//! region rather than reaching across it).

use vieww_foundation::{Color, Image, ImageFilter, Offset, Path, Rect, Shadow};
use vieww_paint::native::NativeRenderer;
use vieww_paint::{Canvas, Damage, Scene};

const W: u32 = 320;
const H: u32 = 240;
const BASE: Color = Color::rgb(250, 250, 252);

fn surface() -> Rect {
    Rect::new(0.0, 0.0, W as f32, H as f32)
}

/// Render `first`, then `second`, the retained way — and compare against a
/// full render of `second`.
fn assert_retained_matches_full(name: &str, first: &Scene, second: &Scene) {
    // The retained renderer sees the first frame, then the second with only
    // the damage between them.
    let mut retained = NativeRenderer::new();
    retained
        .render_to_pixels(first, W, H, BASE)
        .expect("the first frame");
    let damage = Damage::between(first, second, surface());
    let (incremental, _) = retained
        .render_retained(second, &damage, W, H, BASE)
        .expect("the retained frame");

    // A renderer that has never seen anything else renders the second scene
    // whole.
    let mut fresh = NativeRenderer::new();
    let (complete, _) = fresh
        .render_to_pixels(second, W, H, BASE)
        .expect("the full frame");

    if incremental.data() == complete.data() {
        return;
    }

    // Report where, not just that — a count and a first offender is the
    // difference between a five-minute fix and an afternoon.
    let mut differing = 0usize;
    let mut first_at = None;
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            if incremental.data()[i..i + 4] != complete.data()[i..i + 4] {
                differing += 1;
                first_at.get_or_insert((x, y));
            }
        }
    }
    let (x, y) = first_at.expect("they differ somewhere");
    let i = ((y * W + x) * 4) as usize;
    panic!(
        "{name}: a retained repaint differs from a full one at {differing} of {} pixels; \
         first at ({x}, {y}) — retained {:?} against full {:?}. \
         Either the damage was too small, or the repaint reached outside its region.",
        W * H,
        &incremental.data()[i..i + 4],
        &complete.data()[i..i + 4],
    );
}

fn card(scene: &mut Scene, at: Rect, color: Color) {
    scene.fill_rrect(at, 10.0, color.into());
}

#[test]
fn a_shape_that_moved_leaves_no_ghost() {
    let mut first = Scene::new();
    card(
        &mut first,
        Rect::new(40.0, 40.0, 140.0, 110.0),
        Color::rgb(60, 120, 240),
    );
    let mut second = Scene::new();
    card(
        &mut second,
        Rect::new(90.0, 70.0, 190.0, 140.0),
        Color::rgb(60, 120, 240),
    );

    assert_retained_matches_full("a moved card", &first, &second);
}

#[test]
fn a_shape_that_shrank_leaves_no_ghost() {
    let mut first = Scene::new();
    card(
        &mut first,
        Rect::new(40.0, 40.0, 260.0, 200.0),
        Color::rgb(200, 80, 60),
    );
    let mut second = Scene::new();
    card(
        &mut second,
        Rect::new(40.0, 40.0, 120.0, 100.0),
        Color::rgb(200, 80, 60),
    );

    assert_retained_matches_full("a shrunken card", &first, &second);
}

#[test]
fn a_shadow_repaints_its_whole_reach() {
    let shadow = Shadow::new(Color::rgba(0, 0, 0, 90), Offset::new(0.0, 6.0), 24.0);
    let mut first = Scene::new();
    first.draw_shadow(Rect::new(60.0, 60.0, 180.0, 140.0), 12.0, shadow);
    card(
        &mut first,
        Rect::new(60.0, 60.0, 180.0, 140.0),
        Color::WHITE,
    );

    let mut second = Scene::new();
    second.draw_shadow(Rect::new(100.0, 80.0, 220.0, 160.0), 12.0, shadow);
    card(
        &mut second,
        Rect::new(100.0, 80.0, 220.0, 160.0),
        Color::WHITE,
    );

    assert_retained_matches_full("a moved shadow", &first, &second);
}

#[test]
fn content_inside_a_rounded_clip_repaints_identically() {
    fn frame(highlight: usize) -> Scene {
        let mut scene = Scene::new();
        scene.save();
        scene.clip_rrect(Rect::new(20.0, 20.0, 300.0, 220.0), 16.0);
        for i in 0..8 {
            let y = 30.0 + i as f32 * 24.0;
            let color = if i == highlight {
                Color::rgb(240, 160, 60)
            } else {
                Color::rgb(200, 208, 220)
            };
            scene.fill_rect(Rect::new(30.0, y, 290.0, y + 18.0), color.into());
        }
        scene.restore();
        scene
    }

    assert_retained_matches_full("a row lit inside a clip", &frame(2), &frame(5));
}

#[test]
fn a_blurred_layer_repaints_identically() {
    fn frame(offset: f32) -> Scene {
        let mut scene = Scene::new();
        scene.fill_rect(
            Rect::new(0.0, 0.0, 320.0, 240.0),
            Color::rgb(230, 234, 242).into(),
        );
        let content = Rect::new(60.0 + offset, 70.0, 180.0 + offset, 150.0);
        scene.push_filtered_layer(
            content,
            1.0,
            vieww_foundation::BlendMode::Normal,
            ImageFilter::blur(6.0),
        );
        scene.fill_path(
            &Path::rounded_rect(content, 14.0),
            Color::rgb(90, 60, 220).into(),
        );
        scene.pop_layer();
        scene
    }

    assert_retained_matches_full("a moved blurred layer", &frame(0.0), &frame(50.0));
}

#[test]
fn a_clean_frame_keeps_the_previous_picture_exactly() {
    let mut scene = Scene::new();
    card(
        &mut scene,
        Rect::new(40.0, 40.0, 200.0, 160.0),
        Color::rgb(60, 160, 120),
    );
    scene.fill_rect(
        Rect::new(0.0, 200.0, 320.0, 240.0),
        Color::rgb(30, 34, 44).into(),
    );

    let mut renderer = NativeRenderer::new();
    let (before, _) = renderer
        .render_to_pixels(&scene, W, H, BASE)
        .expect("first");
    let before = before.data().to_vec();

    // Nothing changed at all: the damage is clean, and the retained surface
    // must hand back the identical picture rather than rebuild it.
    let clean = Damage::new(surface());
    let (after, report) = renderer
        .render_retained(&scene, &clean, W, H, BASE)
        .expect("clean frame");

    assert_eq!(
        after.data(),
        before.as_slice(),
        "a clean frame changed the picture"
    );
    assert_eq!(
        report.translated_commands, 0,
        "a clean frame should translate no commands at all"
    );
}

/// Several disjoint regions in one frame — the case a single union rectangle
/// would quietly paper over.
#[test]
fn two_separate_changes_both_land() {
    fn frame(top: Color, bottom: Color) -> Scene {
        let mut scene = Scene::new();
        card(&mut scene, Rect::new(20.0, 20.0, 120.0, 90.0), top);
        card(&mut scene, Rect::new(190.0, 150.0, 300.0, 220.0), bottom);
        scene
    }

    assert_retained_matches_full(
        "two corners changed",
        &frame(Color::rgb(60, 120, 240), Color::rgb(200, 80, 60)),
        &frame(Color::rgb(240, 180, 60), Color::rgb(60, 180, 120)),
    );
}

/// An image that moves — the case `paint_image`'s row walk must survive
/// under a damage region just as the shape walks do. Two things make images
/// the interesting variant: the draw reaches to the edges of its transformed
/// rect (so the damage must cover both the old and the new footprint), and
/// the sampler interpolates between texels (so a repaint whose clip cut a
/// row mid-texel would land different bytes than a full render of the same
/// scene).
#[test]
fn an_image_that_moved_repaints_identically() {
    fn frame(offset: f32) -> Scene {
        let mut scene = Scene::new();
        // A base coat, so the image composites over something and the
        // region it vacated has to be restored rather than left
        // transparent.
        scene.fill_rect(Rect::new(0.0, 0.0, 320.0, 240.0), BASE.into());
        let mut pixels = Vec::with_capacity(24 * 18 * 4);
        for y in 0..18u32 {
            for x in 0..24u32 {
                let opaque = ((x / 3) + (y / 2)) % 2 == 0;
                if opaque {
                    pixels.extend_from_slice(&[240, 120, 40, 255]);
                } else {
                    pixels.extend_from_slice(&[40, 120, 240, 150]);
                }
            }
        }
        let image = Image::from_rgba8(pixels, 24, 18);
        // Deliberately off the texel grid and at a non-integer scale, so
        // the bilinear sampler does real interpolation on both frames and
        // the opaque fast path and the blend path both run.
        scene.draw_image(
            Rect::new(40.5 + offset, 60.25, 160.5 + offset, 150.75),
            &image,
        );
        scene
    }

    assert_retained_matches_full("a moved image", &frame(0.0), &frame(57.5));
}

/// The same image clipped by a rounded rect — the clip mask must resolve
/// identically over the damage region as over the whole window, and the
/// image's row walk must multiply the clip in at the same pixels.
#[test]
fn an_image_inside_a_rounded_clip_repaints_identically() {
    fn frame(shift: f32) -> Scene {
        let mut scene = Scene::new();
        scene.fill_rect(Rect::new(0.0, 0.0, 320.0, 240.0), BASE.into());
        scene.save();
        scene.clip_rrect(Rect::new(30.0, 30.0, 250.0, 210.0), 24.0);
        let mut pixels = Vec::with_capacity(20 * 20 * 4);
        for _ in 0..20 * 20 {
            pixels.extend_from_slice(&[200, 60, 160, 255]);
        }
        let image = Image::from_rgba8(pixels, 20, 20);
        scene.draw_image(Rect::new(50.0 + shift, 50.0, 200.0 + shift, 170.0), &image);
        scene.restore();
        scene
    }

    assert_retained_matches_full("a clipped image moved", &frame(0.0), &frame(33.0));
}
