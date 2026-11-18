//! `SceneRenderer` against a real Vulkan device, and against the CPU
//! rasterizer that is this workspace's reference for what a scene looks like.
//!
//! # Why the comparison is with `NativeRenderer` and not with a stored image
//!
//! A golden PNG says "this is what it looked like the day somebody approved
//! it". The question worth answering about a *second* renderer is different:
//! does it agree with the first one? `vieww_paint::NativeRenderer` is the
//! renderer this framework ships, has the larger test suite behind it, and is
//! what every fixture in `fixtures-out/` was drawn with — so it is the oracle,
//! and the GPU path's job is to match it.
//!
//! # Why the tolerance is not zero
//!
//! Two rasterizers agreeing pixel-for-pixel is not a reasonable requirement
//! and asking for it would mean pinning both to one implementation forever.
//! The CPU path computes analytic coverage per scanline; the GPU path
//! triangulates with `lyon` and lets the hardware sample. They disagree by
//! design at exactly one place — the fractional pixels along a shape's edge —
//! and nowhere else.
//!
//! So the assertion is shaped to say that: **interiors must match closely**,
//! and the disagreement must stay on the boundary. A regression that
//! misplaces a shape, drops it, or paints it the wrong colour moves interior
//! pixels and fails, while a change to either rasterizer's antialiasing does
//! not — which is the distinction a parity test is for.
//!
//! `#[ignore]`d for the same reason the other Vulkan suites here are: this
//! needs a loader and an ICD (`lavapipe`, or real hardware). Run with
//! `cargo test -p vieww-hal --features vulkan -- --ignored`.

#![cfg(feature = "vulkan")]

#[path = "support/parity_row.rs"]
mod parity_row;

use vieww_foundation::{
    BlendMode, Color, Gradient, Image, ImageFilter, Offset, Path, Rect, TextStyle, Transform,
};
use vieww_gpu::{plan_scene, Unsupported};
use vieww_hal::vulkan::{SceneRenderer, StagingSlot, VulkanDevice};
use vieww_paint::native::NativeRenderer;
use vieww_paint::{Clip, Command, Paint, Scene, Stroke};
use vieww_text::{FontStore, Paragraph, TextSpan};

const WIDTH: u32 = 120;
const HEIGHT: u32 = 90;

/// One Vulkan device at a time, for the whole file.
///
/// # This is a real observation, not defensive habit
///
/// These tests each build their own `VulkanDevice`, and `libtest` runs them on
/// as many threads as the machine has. Once this suite began creating images
/// and samplers — the glyph atlas — running it multi-threaded started to
/// **SIGSEGV inside `libvulkan_lvp.so`**, reproducibly, on about half of runs.
/// Every frame in the backtrace at the fault is lavapipe's own; the deepest
/// frame belonging to this workspace is an ordinary `wait_for_fences`, and the
/// same eight tests pass every time with `--test-threads=1`.
///
/// So: concurrent devices on Mesa's software rasterizer, not a data race in
/// anything here — nothing in this crate shares a Vulkan object between
/// threads, and there is no `unsafe` in the tests at all. The serialisation is
/// therefore in the *tests*, where the constraint actually is, rather than
/// worked around with synchronisation inside `SceneRenderer` that would slow
/// down a real driver to accommodate a software one.
///
/// It is written here rather than in `TRACKER.md` alone because the next
/// person to add a test to this file will add it without the guard, watch CI
/// crash in a C library, and have no way to know why.
static ONE_DEVICE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The guard is returned, not dropped here: it has to be held for as long as
/// the device lives, and a `let _ = ...` at the top of a function would release
/// it immediately.
type Exclusive = std::sync::MutexGuard<'static, ()>;

fn try_renderer() -> Option<(Exclusive, VulkanDevice, SceneRenderer)> {
    // A test that fails while holding the lock poisons it. Recovering rather
    // than propagating is deliberate: the useful failure is the assertion that
    // actually failed, and letting every subsequent test fail with
    // `PoisonError` instead buries it.
    let guard = ONE_DEVICE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let device = match VulkanDevice::new() {
        Ok(device) => device,
        Err(error) => {
            eprintln!("skipping: no Vulkan device available ({error})");
            return None;
        }
    };
    let renderer = SceneRenderer::new(&device).expect("the pipeline should build");
    // The renderer holds a cloned device handle and the device must outlive
    // it, so both are returned and dropped together by the caller.
    Some((guard, device, renderer))
}

fn fill_rect(scene: &mut Scene, rect: Rect, color: Color) {
    scene.push_command(Command::FillRect {
        rect,
        paint: Paint::solid(color),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
}

/// The CPU rasterizer's answer for the same scene.
fn on_cpu(scene: &Scene, base: Color) -> Vec<u8> {
    let mut renderer = NativeRenderer::new();
    let (pixels, _) = renderer
        .render_to_pixels(scene, WIDTH, HEIGHT, base)
        .expect("the CPU rasterizer should render this scene");
    pixels.data().to_vec()
}

/// Count pixels where `gpu` and `cpu` disagree, excluding pixels that sit on
/// a shape's antialiased edge.
///
/// # The mistake this function was written wrong once to make
///
/// The first version decided "is this pixel on an edge?" by asking whether
/// any *neighbouring pixel also disagreed between the two images*. That is
/// circular, and it fails in exactly the case a parity test exists to catch:
/// when the GPU path is broadly wrong — a swapped colour channel, say — every
/// pixel of every shape disagrees, so every pixel has a disagreeing
/// neighbour, so every pixel is classified as an edge, and the function
/// returns zero. It was confirmed by making that mutation deliberately: three
/// parity tests passed with red and blue swapped in the planner.
///
/// An edge is a property of the *picture*, not of the diff. So the question
/// asked here is whether the **reference** image has a coverage transition at
/// this pixel — a neighbour in `cpu` that differs from `cpu`'s own value —
/// which is true along shape boundaries and false in flat interiors no matter
/// what the GPU produced.
fn interior_mismatches(gpu: &[u8], cpu: &[u8], tolerance: u8) -> usize {
    assert_eq!(gpu.len(), cpu.len(), "same dimensions");
    let w = WIDTH as usize;
    let h = HEIGHT as usize;
    let channel = |buffer: &[u8], index: usize, c: usize| buffer[index * 4 + c];
    let differs = |index: usize| {
        (0..4).any(|c| channel(gpu, index, c).abs_diff(channel(cpu, index, c)) > tolerance)
    };
    // A transition in the reference itself: this is where antialiasing lives,
    // and the one place two rasterizers are allowed to disagree.
    let reference_edge = |x: usize, y: usize| {
        let index = y * w + x;
        (-1i64..=1).any(|dy| {
            (-1i64..=1).any(|dx| {
                let nx = x as i64 + dx;
                let ny = y as i64 + dy;
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    return false;
                }
                let neighbour = ny as usize * w + nx as usize;
                (0..4).any(|c| channel(cpu, index, c).abs_diff(channel(cpu, neighbour, c)) > 8)
            })
        })
    };

    let mut count = 0;
    for y in 0..h {
        for x in 0..w {
            if differs(y * w + x) && !reference_edge(x, y) {
                count += 1;
            }
        }
    }
    parity_row::record("vulkan", tolerance, gpu, cpu, count);
    count
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn an_empty_scene_is_the_clear_colour_exactly() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let clear = Color::rgba(20, 30, 40, 255);
    let plan = plan_scene(&Scene::default(), WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "an empty scene has nothing unsupported");

    let pixels = renderer
        .render(&plan, WIDTH, HEIGHT, clear)
        .expect("render should succeed");

    // Exactly, not approximately: nothing was drawn, so no rasterization
    // happened and there is no coverage to disagree about.
    for pixel in pixels.as_chunks::<4>().0 {
        assert_eq!(pixel, &[20, 30, 40, 255], "every pixel is the clear colour");
    }
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn one_rectangle_lands_where_the_geometry_says() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let mut scene = Scene::default();
    fill_rect(
        &mut scene,
        Rect::new(20.0, 20.0, 80.0, 60.0),
        Color::rgba(255, 0, 0, 255),
    );
    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);
    let pixels = renderer
        .render(&plan, WIDTH, HEIGHT, Color::rgba(0, 0, 0, 255))
        .expect("render should succeed");

    let at = |x: usize, y: usize| {
        let i = (y * WIDTH as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    };

    assert_eq!(at(50, 40), [255, 0, 0, 255], "inside the rectangle");
    assert_eq!(at(5, 5), [0, 0, 0, 255], "outside it, top-left");
    assert_eq!(at(100, 80), [0, 0, 0, 255], "outside it, bottom-right");
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn overlapping_translucent_fills_composite_like_the_cpu_rasterizer() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::rgba(255, 255, 255, 255);
    let mut scene = Scene::default();
    fill_rect(
        &mut scene,
        Rect::new(10.0, 10.0, 70.0, 60.0),
        Color::rgba(200, 40, 40, 255),
    );
    // Half-transparent, and overlapping the first: this is the case where a
    // wrong blend factor produces a picture that still looks plausible.
    fill_rect(
        &mut scene,
        Rect::new(40.0, 30.0, 110.0, 80.0),
        Color::rgba(40, 80, 220, 128),
    );

    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete());
    assert_eq!(
        plan.draw_call_count(),
        1,
        "two unclipped fills share a scissor, so they batch into one draw"
    );

    let gpu = renderer
        .render(&plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);

    let mismatches = interior_mismatches(&gpu, &cpu, 2);
    assert_eq!(
        mismatches, 0,
        "GPU and CPU must agree away from shape edges; {mismatches} interior pixels differed"
    );
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn a_transformed_path_and_a_stroke_match_the_cpu_rasterizer() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::rgba(12, 12, 16, 255);
    let mut scene = Scene::default();

    let mut triangle = Path::new();
    triangle.move_to(Offset::new(10.0, 10.0));
    triangle.line_to(Offset::new(60.0, 15.0));
    triangle.line_to(Offset::new(30.0, 55.0));
    triangle.close();
    scene.push_command(Command::FillPath {
        path: triangle.clone(),
        paint: Paint::solid(Color::rgba(90, 200, 120, 255)),
        // A translation, so the planner's transform handling is exercised
        // rather than skipped by the identity fast path.
        transform: Transform::translate(Offset::new(24.0, 12.0)),
        clip: Clip::NONE,
    });

    let mut line = Path::new();
    line.move_to(Offset::new(8.0, 78.0));
    line.line_to(Offset::new(112.0, 66.0));
    scene.push_command(Command::StrokePath {
        path: line,
        stroke: Stroke::new(6.0),
        paint: Paint::solid(Color::rgba(240, 190, 60, 255)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });

    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    assert!(plan.triangle_count() >= 3);

    let gpu = renderer
        .render(&plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);

    let mismatches = interior_mismatches(&gpu, &cpu, 2);
    assert_eq!(
        mismatches, 0,
        "a transformed fill and a stroke must land where the CPU puts them; \
         {mismatches} interior pixels differed"
    );
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn a_rectangular_clip_becomes_a_scissor_and_cuts_the_same_way() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::rgba(255, 255, 255, 255);
    let mut clip = Clip::NONE;
    clip.add_rect(Rect::new(30.0, 30.0, 70.0, 70.0));

    let mut scene = Scene::default();
    scene.push_command(Command::FillRect {
        rect: Rect::new(0.0, 0.0, 120.0, 90.0),
        paint: Paint::solid(Color::rgba(30, 120, 200, 255)),
        transform: Transform::IDENTITY,
        clip,
    });

    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "a rectangular clip is expressible");
    assert_eq!(plan.runs.len(), 1);
    assert!(
        plan.runs[0].scissor.is_some(),
        "the clip became a scissor rather than being dropped"
    );

    let gpu = renderer
        .render(&plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);
    assert_eq!(
        interior_mismatches(&gpu, &cpu, 2),
        0,
        "the scissor must cut where the CPU clip cuts"
    );
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn the_renderer_refuses_a_plan_it_cannot_draw() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    // A shaped clip: expressible by the CPU rasterizer, not by a scissor.
    let mut clip = Clip::NONE;
    clip.add_rect(Rect::new(10.0, 10.0, 90.0, 70.0));
    let mut rounded = Path::new();
    rounded.move_to(Offset::new(20.0, 20.0));
    rounded.cubic_to(
        Offset::new(60.0, 10.0),
        Offset::new(80.0, 40.0),
        Offset::new(40.0, 60.0),
    );
    rounded.close();
    clip.add_path(rounded);

    let mut scene = Scene::default();
    scene.push_command(Command::FillRect {
        rect: Rect::new(0.0, 0.0, 120.0, 90.0),
        paint: Paint::solid(Color::rgba(200, 30, 30, 255)),
        transform: Transform::IDENTITY,
        clip,
    });

    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(!plan.is_complete());
    assert_eq!(plan.unsupported.get(&Unsupported::ShapedClip), Some(&1));

    let refused = renderer.render(&plan, WIDTH, HEIGHT, Color::WHITE);
    assert!(
        refused.is_err(),
        "an incomplete plan must be refused, not partly drawn — see the module doc"
    );
    let message = refused.unwrap_err().to_string();
    assert!(
        message.contains("shaped-clip"),
        "the error should name what stopped it, got: {message}"
    );

    // And the escape hatch still works, for tools that want the partial view.
    assert!(renderer
        .render_incomplete(&plan, WIDTH, HEIGHT, Color::WHITE)
        .is_ok());
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn the_renderer_is_reusable_across_frames_and_sizes() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let mut scene = Scene::default();
    fill_rect(
        &mut scene,
        Rect::new(5.0, 5.0, 40.0, 40.0),
        Color::rgba(255, 255, 0, 255),
    );
    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);

    // Several frames at one size: the whole point of the persistent pipeline.
    for _ in 0..8 {
        let pixels = renderer
            .render(&plan, WIDTH, HEIGHT, Color::rgba(0, 0, 0, 255))
            .expect("every frame should render");
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
    }

    // Then a resize, which must rebuild only the size-dependent objects.
    let bigger = renderer
        .render(&plan, WIDTH * 2, HEIGHT * 2, Color::rgba(0, 0, 0, 255))
        .expect("a resize should not break the renderer");
    assert_eq!(bigger.len(), (WIDTH * 2 * HEIGHT * 2 * 4) as usize);

    // And back down again, which is the case a naive cache gets wrong.
    let smaller = renderer
        .render(&plan, WIDTH / 2, HEIGHT / 2, Color::rgba(0, 0, 0, 255))
        .expect("shrinking should work too");
    assert_eq!(smaller.len(), (WIDTH / 2 * (HEIGHT / 2) * 4) as usize);
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn many_shapes_stay_one_draw_call_and_still_match_the_cpu() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::rgba(250, 250, 250, 255);
    let mut scene = Scene::default();
    // A grid of differently-coloured tiles: the case per-vertex colour exists
    // for, and the one a per-draw uniform would turn into 48 draw calls.
    for row in 0..6 {
        for column in 0..8 {
            let x = column as f32 * 15.0;
            let y = row as f32 * 15.0;
            fill_rect(
                &mut scene,
                Rect::new(x + 1.0, y + 1.0, x + 14.0, y + 14.0),
                Color::rgba(
                    (column * 30) as u8,
                    (row * 40) as u8,
                    ((column + row) * 20) as u8,
                    255,
                ),
            );
        }
    }

    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);
    assert_eq!(
        plan.draw_call_count(),
        1,
        "48 tiles, one draw call — this is the batching claim, checked"
    );
    assert_eq!(plan.triangle_count(), 96, "two triangles per tile");

    let gpu = renderer
        .render(&plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);
    assert_eq!(
        interior_mismatches(&gpu, &cpu, 2),
        0,
        "batching must not change the picture"
    );
}

/// The analytic edge coverage — the antialiasing the GPU path was missing
/// until 2026-10-09 (see `TRACKER.md`) — verified three ways at once:
///
/// 1. **The ramp exists.** A fractional rect and a rounded card must produce
///    pixels of *intermediate* alpha. The tessellated material this replaces
///    could not produce one, and "the edge is no longer hard" is the whole
///    feature.
/// 2. **Straight edges are exact.** For an axis-aligned edge the SDF's
///    coverage at a pixel centre is the pixel's covered area — the same
///    arithmetic the CPU rasterizer's scanline performs — so both renderers
///    must land on it tightly, not approximately.
/// 3. **The shader agrees with the Rust mirror.** The expected value comes
///    from `vieww_gpu::scene::shape_distance` itself, so a shader that
///    drifts from the arithmetic it was written from fails here even when
///    both are self-consistent.
///
/// Corner arcs get the looser check: the CPU approximates a quarter circle
/// with four vertical subsamples per pixel row, the SDF is exact, and the
/// two honest approximations agree to within a fraction of the band.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn fractional_and_rounded_rects_have_antialiased_edges_like_the_cpu() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::rgba(255, 255, 255, 255);
    let mut scene = Scene::default();

    // Every edge of this rect falls mid-pixel, so every edge pixel is a
    // partial-coverage pixel.
    let rect = Rect::new(20.4, 10.35, 70.6, 50.65);
    fill_rect(&mut scene, rect, Color::rgba(0, 0, 0, 255));

    // The rounded card — the shape a premium interface is made of.
    let card = Rect::new(30.25, 55.3, 105.75, 84.7);
    scene.push_command(Command::FillPath {
        path: Path::rounded_rect(card, 9.0),
        paint: Paint::solid(Color::rgba(0, 0, 0, 255)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });

    let plan = plan_scene(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    let gpu = renderer
        .render(&plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);

    // Interiors agree exactly, as they always did.
    assert_eq!(
        interior_mismatches(&gpu, &cpu, 2),
        0,
        "interior parity must hold with analytic edges too"
    );

    // Coverage is read from the red channel, not the alpha: the shapes are
    // opaque black on an opaque white base, so every pixel is opaque and the
    // ramp lives in the colour — red is `255 * (1 - coverage)`.
    let red_of = |buffer: &[u8], x: usize, y: usize| buffer[(y * WIDTH as usize + x) * 4];

    // 1. The ramp exists — mid-band pixels, on both renderers, for both
    //    shapes. A hard edge has none.
    let mut partial = 0;
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let r = red_of(&gpu, x, y);
            if (12..243).contains(&r) {
                partial += 1;
            }
        }
    }
    assert!(
        partial > 40,
        "two fractional-edged shapes must produce dozens of \
         partial-coverage pixels; found {partial}"
    );

    // 2 & 3. Straight edges: exact agreement with the SDF's area coverage,
    //    and therefore with each other. Sampled along all four edges of the
    //    fractional rect.
    let edge_samples = [
        (20, 30),
        (21, 30),
        (69, 30),
        (70, 30),
        (45, 10),
        (45, 11),
        (45, 50),
        (45, 51),
    ];
    for &(x, y) in &edge_samples {
        let centre = Offset::new(x as f32 + 0.5, y as f32 + 0.5);
        let d = vieww_gpu::scene::shape_distance(centre, rect, 0.0);
        let coverage = (0.5 - d).clamp(0.0, 1.0).sqrt(); // through the fill coverage gamma, as both renderers apply it
        let expected = (255.0 * (1.0 - coverage)).round();
        let gpu_r = f32::from(red_of(&gpu, x, y));
        let cpu_r = f32::from(red_of(&cpu, x, y));
        // The GPU must land on the SDF's own answer: it is the arithmetic
        // the shader was written from, and anything looser here would let
        // the two drift apart silently.
        assert!(
            (gpu_r - expected).abs() <= 3.0,
            "gpu edge pixel ({x}, {y}): {gpu_r} vs the SDF's {expected}"
        );
        // The CPU's scanline coverage is exact along vertical edges and
        // quantised to four vertical subsamples along horizontal ones — 0.125
        // of a pixel at worst, 32 of 255 — so its agreement with the SDF is
        // bounded, not exact. That bound is itself the claim: two different
        // definitions of the same edge, this close.
        assert!(
            (cpu_r - expected).abs() <= 34.0,
            "cpu edge pixel ({x}, {y}): {cpu_r} vs the SDF's {expected}"
        );
    }

    // The corner arc: sampled in the neighbourhood of the arc point at 225
    // degrees — up and left, the direction the top-left corner faces — where
    // the SDF itself says coverage is partial. On those pixels the GPU must
    // match the SDF exactly and the CPU approximately: the CPU approximates
    // the arc with four subsamples per row, the SDF is exact, and both are
    // honest definitions of the same quarter circle.
    let arc_centre = Offset::new(card.left + 9.0, card.top + 9.0);
    let arc_point = Offset::new(
        arc_centre.dx - 9.0 * std::f32::consts::FRAC_1_SQRT_2,
        arc_centre.dy - 9.0 * std::f32::consts::FRAC_1_SQRT_2,
    );
    let mut partial_corners = 0;
    for dy in -2i64..=2 {
        for dx in -2i64..=2 {
            let x = arc_point.dx as i64 + dx;
            let y = arc_point.dy as i64 + dy;
            if x < 0 || y < 0 || x >= WIDTH as i64 || y >= HEIGHT as i64 {
                continue;
            }
            let (x, y) = (x as usize, y as usize);
            let centre = Offset::new(x as f32 + 0.5, y as f32 + 0.5);
            let d = vieww_gpu::scene::shape_distance(centre, card, 9.0);
            let coverage = (0.5 - d).clamp(0.0, 1.0).sqrt(); // through the fill coverage gamma, as both renderers apply it
            if coverage <= 0.05 || coverage >= 0.95 {
                continue;
            }
            partial_corners += 1;
            let expected = (255.0 * (1.0 - coverage)).round();
            let gpu_r = f32::from(red_of(&gpu, x, y));
            let cpu_r = f32::from(red_of(&cpu, x, y));
            assert!(
                (gpu_r - expected).abs() <= 3.0,
                "gpu corner pixel ({x}, {y}): {gpu_r} vs the SDF's {expected}"
            );
            assert!(
                (cpu_r - expected).abs() <= 40.0,
                "cpu corner pixel ({x}, {y}): {cpu_r} vs the SDF's {expected}"
            );
        }
    }
    assert!(
        partial_corners >= 2,
        "the arc's partial-coverage band exists and was sampled: \
         {partial_corners} pixels"
    );
}

/// Every non-analytic shape — a rotated rect, a star, a dashed curved stroke,
/// a circle under a shaped clip — antialiased like the CPU, edges included.
///
/// These used to be tessellated and rasterised hard, which is what the
/// "two rasterizers differ on every edge" caveat in this file was written
/// for. Their coverage now comes from the CPU rasterizer through
/// `GpuSeam::shape_coverage`, so the bound here is the whole picture, not the
/// interior: one byte of quantisation, plus one for a clip multiplied in at
/// 8 bits rather than `f32`.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn arbitrary_paths_and_strokes_are_antialiased_like_the_cpu_everywhere() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::rgba(255, 255, 255, 255);
    let mut scene = Scene::default();

    scene.push_command(Command::FillRect {
        rect: Rect::new(-15.0, -8.0, 15.0, 8.0),
        paint: Paint::solid(Color::rgba(20, 40, 200, 255)),
        transform: Transform {
            a: 0.866,
            b: 0.5,
            c: -0.5,
            d: 0.866,
            tx: 25.3,
            ty: 22.7,
        },
        clip: Clip::NONE,
    });

    let mut star = Path::new();
    for i in 0..10 {
        let r = if i % 2 == 0 { 18.0 } else { 7.5 };
        let a = std::f32::consts::PI * f32::from(i as u8) / 5.0 - 0.3;
        let p = Offset::new(78.4 + r * a.cos(), 24.6 + r * a.sin());
        if i == 0 {
            star.move_to(p);
        } else {
            star.line_to(p);
        }
    }
    star.close();
    scene.push_command(Command::FillPath {
        path: star,
        paint: Paint::solid(Color::rgba(200, 30, 60, 230)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });

    let mut wave = Path::new();
    wave.move_to(Offset::new(8.0, 70.0));
    wave.cubic_to(
        Offset::new(25.0, 45.0),
        Offset::new(45.0, 95.0),
        Offset::new(62.0, 66.0),
    );
    let style = vieww_foundation::StrokeStyle {
        dash: Some(vieww_foundation::Dash::new([7.0, 3.5])),
        ..vieww_foundation::StrokeStyle::default()
    };
    scene.push_command(Command::StrokePath {
        path: wave,
        stroke: Stroke::new(3.3).styled(style),
        paint: Paint::solid(Color::rgba(10, 120, 40, 255)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });

    let mut clip = Clip::NONE;
    clip.add_rect(Rect::new(70.0, 50.0, 115.0, 88.0));
    clip.add_path(Path::rounded_rect(Rect::new(70.0, 50.0, 115.0, 88.0), 14.0));
    scene.push_command(Command::FillPath {
        path: Path::arc(Offset::new(100.2, 74.6), 19.3, 0.0, std::f32::consts::TAU),
        paint: Paint::solid(Color::rgba(90, 20, 140, 255)),
        transform: Transform::IDENTITY,
        clip,
    });

    let mut planner = vieww_gpu::Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    let gpu = renderer
        .render_planned(&planner, &plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);

    let mut worst = 0u8;
    let mut partial = 0;
    for (g, c) in gpu.as_chunks::<4>().0.iter().zip(cpu.as_chunks::<4>().0) {
        for ch in 0..4 {
            worst = worst.max(g[ch].abs_diff(c[ch]));
        }
        if (12..243).contains(&c[0]) && *c != [255, 255, 255, 255] {
            partial += 1;
        }
    }
    assert!(
        partial > 100,
        "the scene must have antialiased edges to compare: {partial}"
    );
    assert!(
        worst <= 2,
        "worst channel difference {worst}/255 — every edge pixel should be the CPU's"
    );
}

/// A shape that changes every frame adds a new mask every frame; the mask
/// texture then takes only the changed rectangles (`upload_masks`) instead of
/// the whole atlas. Held here to a renderer built fresh for every frame —
/// whose first upload is always a full one — byte for byte, over enough
/// frames for the atlas to compact more than once, with a frame skipped in
/// the middle (the planner advances, the renderer does not upload).
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn partial_mask_uploads_match_full_uploads_frame_for_frame() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 160;
    const H: u32 = 120;
    let base = Color::rgba(250, 250, 248, 255);
    let star = |t: f32| {
        let mut p = Path::new();
        for i in 0..14 {
            let r = if i % 2 == 0 { 34.0 } else { 15.0 };
            let a = i as f32 * std::f32::consts::PI / 7.0 + t;
            let pt = Offset::new(50.0 + r * a.cos(), 60.0 + r * a.sin());
            if i == 0 {
                p.move_to(pt);
            } else {
                p.line_to(pt);
            }
        }
        p.close();
        p
    };
    let mut planner = vieww_gpu::Planner::new();
    let (mut partial, mut full) = (0, 0);
    let (mut sent, mut whole) = (0u64, 0u64);
    for frame in 0..48 {
        let t = frame as f32 * 0.13;
        let mut scene = Scene::new();
        scene.push_command(Command::FillPath {
            path: star(t),
            paint: Paint::solid(Color::rgba(200, 70, 30, 255)),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        scene.push_command(Command::FillPath {
            path: Path::rounded_rect(Rect::new(-28.0, -18.0, 28.0, 18.0), 7.0),
            paint: Paint::solid(Color::rgba(30, 120, 80, 220)),
            transform: Transform::rotate(t * 0.7)
                .then(Transform::translate(Offset::new(115.0, 60.0))),
            clip: Clip::NONE,
        });
        // A steady shape: its mask must survive every partial upload.
        scene.push_command(Command::StrokePath {
            path: Path::arc(Offset::new(80.0, 100.0), 12.0, 0.3, 4.0),
            stroke: Stroke::new(2.5),
            paint: Paint::solid(Color::rgba(20, 40, 160, 255)),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let plan = planner.plan(&scene, W as f32, H as f32);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        if frame == 23 {
            continue; // the renderer misses this frame's upload entirely
        }
        let got = renderer
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        let side = u64::from(planner.mask_atlas().side());
        sent += renderer.last_mask_upload_bytes();
        whole += side * side;
        match renderer.last_mask_upload_bytes() {
            0 => {}
            b if b == side * side => full += 1,
            _ => partial += 1,
        }
        let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
        let want = fresh
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        drop(fresh);
        assert!(
            got == want,
            "frame {frame}: partial-upload pixels differ from a full upload"
        );
    }
    assert!(
        partial >= 10,
        "partial uploads should be common: {partial} partial, {full} full"
    );
    assert!(
        full >= 2,
        "the run should cross compactions and the skipped frame: {full} full"
    );
    assert!(
        sent * 2 < whole,
        "partial uploads should send well under half the bytes of whole-texture ones: {sent} vs {whole}"
    );
    eprintln!("{partial} partial, {full} full; {sent} bytes sent against {whole} for whole-texture uploads");
}

/// Text whose visible run grows a few characters a frame — a line scrolling
/// in, a font fallback appearing — adds a few new glyphs to the atlas per
/// frame; the glyph texture then takes only the changed rectangles instead of
/// the whole atlas. Held here to a renderer built fresh for every frame —
/// whose first upload is always a full one — byte for byte, with a frame
/// skipped in the middle (the planner advances, the renderer does not
/// upload), and a steady label whose glyphs must survive every partial
/// upload.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn partial_glyph_uploads_match_full_uploads_frame_for_frame() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 260;
    const H: u32 = 100;
    let base = Color::rgba(250, 250, 248, 255);
    // One font store for the whole run, the way an application holds one: a
    // fresh store per frame would be a new font identity and re-rasterise
    // everything, which is a different test — see `vulkan_text.rs`'s
    // `text_scene_in` for why that is a trap.
    let mut store = FontStore::embedded_only();
    // A reveal that mixes scripts: every few characters bring in glyphs the
    // atlas has not seen, which is what a line scrolling in does. Whether the
    // embedded font covers a script only changes what the glyphs look like;
    // the atlas packs whatever is rasterised.
    let reveal = "Hamburgefonstiv 123 — The quick brown fox jumps over the lazy dog! \
                  ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ; αβγδεζ. \
                  Съешь ещё этих мягких французских булок, да выпей чаю.";
    let body = TextStyle {
        color: Color::rgba(20, 20, 30, 255),
        size: 24.0,
        ..TextStyle::default()
    };
    let label_style = TextStyle {
        color: Color::rgba(120, 30, 30, 255),
        size: 17.0,
        ..TextStyle::default()
    };
    let mut planner = vieww_gpu::Planner::new();
    let (mut partial, mut full) = (0, 0);
    let (mut sent, mut whole) = (0u64, 0u64);
    for frame in 0..40 {
        let shown: String = reveal.chars().take((frame as usize + 1) * 3).collect();
        let mut scene = Scene::default();
        for (text, style, y) in [
            (shown.as_str(), body, 40.0),
            ("0123456789", label_style, 82.0),
        ] {
            let paragraph = Paragraph::layout(&mut store, &[TextSpan::new(text, style)], 1_000.0);
            for run in paragraph.runs() {
                let mut run = run.clone();
                run.origin = Offset::new(run.origin.dx + 8.0, run.origin.dy + y);
                run.color = style.color;
                scene.push_command(Command::DrawGlyphs {
                    run,
                    transform: Transform::IDENTITY,
                    clip: Clip::NONE,
                });
            }
        }
        let plan = planner.plan(&scene, W as f32, H as f32);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        if frame == 21 {
            continue; // the renderer misses this frame's upload entirely
        }
        let got = renderer
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        if frame == 0 {
            let inked = got
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| **p != [250, 250, 248, 255])
                .count();
            assert!(inked > 300, "the frame must have drawn text: {inked} inked pixels");
        }
        let side = u64::from(planner.atlas().side());
        sent += renderer.last_glyph_upload_bytes();
        whole += side * side;
        match renderer.last_glyph_upload_bytes() {
            0 => {}
            b if b == side * side => full += 1,
            _ => partial += 1,
        }
        let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
        let want = fresh
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        drop(fresh);
        assert_eq!(
            got, want,
            "frame {frame}: partial-upload pixels differ from a full upload"
        );
    }
    assert!(
        partial >= 10,
        "partial uploads should be common: {partial} partial, {full} full"
    );
    assert!(
        full >= 2,
        "the run should cross the first upload and the skipped frame: {full} full"
    );
    assert!(
        sent * 2 < whole,
        "partial uploads should send well under half the bytes of whole-texture ones: {sent} vs {whole}"
    );
    eprintln!(
        "{partial} partial, {full} full; {sent} bytes sent against {whole} for whole-texture uploads"
    );
}

/// A scene that builds a fresh `Image` every frame — a video frame, a
/// regenerated chart — inserts one image per frame, and its texture takes
/// only the changed rectangles (the image plus its padding ring) instead of
/// the whole atlas. Held here to a renderer built fresh for every frame,
/// byte for byte, with a steady image that must survive every partial upload
/// and a run long enough to cross the atlas's compaction, plus a frame
/// skipped in the middle.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn partial_image_uploads_match_full_uploads_frame_for_frame() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 200;
    const H: u32 = 150;
    let base = Color::rgba(250, 250, 248, 255);
    let churn = |frame: u32| {
        let (w, h) = (96u32, 96u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let r = (x * 3 + y * 5 + frame * 17) as u8;
                let g = (x * 7 + y * 11 + frame * 3) as u8;
                let b = (x * 13 + y * 3 + frame * 29) as u8;
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        Image::from_rgba8(rgba, w, h)
    };
    // The steady element: drawn every frame from frame zero, so its texels
    // must survive every partial upload and the compaction.
    let logo = churn(200);
    let mut planner = vieww_gpu::Planner::new();
    let (mut partial, mut full) = (0, 0);
    let (mut sent, mut whole) = (0u64, 0u64);
    for frame in 0..40 {
        let mut scene = Scene::default();
        scene.push_command(Command::DrawImage {
            rect: Rect::new(8.0, 8.0, 72.0, 72.0),
            image: logo.clone(),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let frame_image = churn(frame);
        scene.push_command(Command::DrawImage {
            rect: Rect::new(100.0, 20.0, 196.0, 116.0),
            image: frame_image.clone(),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let plan = planner.plan(&scene, W as f32, H as f32);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        if frame == 31 {
            continue; // the renderer misses this frame's upload entirely
        }
        let got = renderer
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        if frame == 0 {
            let inked = got
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| **p != [250, 250, 248, 255])
                .count();
            assert!(inked > 3_000, "the frame must have drawn images: {inked} inked pixels");
        }
        let side = u64::from(planner.image_atlas().side());
        sent += renderer.last_image_upload_bytes();
        whole += side * side * 4;
        match renderer.last_image_upload_bytes() {
            0 => {}
            b if b == side * side * 4 => full += 1,
            _ => partial += 1,
        }
        let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
        let want = fresh
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        drop(fresh);
        assert_eq!(
            got, want,
            "frame {frame}: partial-upload pixels differ from a full upload"
        );
    }
    assert!(
        partial >= 10,
        "partial uploads should be common: {partial} partial, {full} full"
    );
    assert!(
        full >= 2,
        "the run should cross the compaction and the skipped frame: {full} full"
    );
    assert!(
        sent * 2 < whole,
        "partial uploads should send well under half the bytes of whole-texture ones: {sent} vs {whole}"
    );
    eprintln!(
        "{partial} partial, {full} full; {sent} bytes sent against {whole} for whole-texture uploads"
    );
}

/// A gradient whose stops animate — a spinner's hue, a hover transition — is
/// a new ramp row every frame, and the ramp texture takes only that row
/// instead of the whole texture. Held here to a renderer built fresh for
/// every frame, byte for byte, with a steady gradient that must survive
/// every partial upload and a run long enough to cross the atlas's
/// compaction (rows move to new indices), plus a frame skipped in the middle.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn partial_ramp_uploads_match_full_uploads_frame_for_frame() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 140;
    const H: u32 = 90;
    let base = Color::rgba(250, 250, 248, 255);
    let steady = Gradient::horizontal().with_stops(&[
        (0.0, Color::rgba(30, 30, 200, 255)),
        (1.0, Color::rgba(200, 30, 30, 255)),
    ]);
    let mut planner = vieww_gpu::Planner::new();
    let (mut partial, mut full) = (0, 0);
    let (mut sent, mut whole) = (0u64, 0u64);
    for frame in 0..24 {
        let hue = (frame * 11 % 256) as u8;
        let animated = Gradient::horizontal().with_stops(&[
            (0.0, Color::rgba(255, 0, 0, 255)),
            (0.5, Color::rgba(hue, 200, 60, 255)),
            (1.0, Color::rgba(0, 0, 255, 255)),
        ]);
        let mut scene = Scene::default();
        scene.push_command(Command::FillRect {
            rect: Rect::new(10.0, 10.0, 130.0, 38.0),
            paint: Paint::gradient(steady),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        scene.push_command(Command::FillRect {
            rect: Rect::new(10.0, 50.0, 130.0, 80.0),
            paint: Paint::gradient(animated),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let plan = planner.plan(&scene, W as f32, H as f32);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        if frame == 17 {
            continue; // the renderer misses this frame's upload entirely
        }
        let got = renderer
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        if frame == 0 {
            let inked = got
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| **p != [250, 250, 248, 255])
                .count();
            assert!(inked > 4_000, "the frame must have drawn gradients: {inked} inked pixels");
        }
        let texture = u64::from(vieww_gpu::RampAtlas::WIDTH)
            * u64::from(planner.ramp_atlas().rows())
            * 16;
        sent += renderer.last_ramp_upload_bytes();
        whole += texture;
        match renderer.last_ramp_upload_bytes() {
            0 => {}
            b if b == texture => full += 1,
            _ => partial += 1,
        }
        let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
        let want = fresh
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        drop(fresh);
        assert_eq!(
            got, want,
            "frame {frame}: partial-upload pixels differ from a full upload"
        );
    }
    assert!(
        partial >= 10,
        "partial uploads should be common: {partial} partial, {full} full"
    );
    assert!(
        full >= 2,
        "the run should cross a compaction and the skipped frame: {full} full"
    );
    assert!(
        sent * 2 < whole,
        "partial uploads should send well under half the bytes of whole-texture ones: {sent} vs {whole}"
    );
    eprintln!(
        "{partial} partial, {full} full; {sent} bytes sent against {whole} for whole-texture uploads"
    );
}

/// A renderer that once drew a deep-layered frame frees the layer targets it
/// no longer uses, after the hysteresis — and a deep frame after the shrink
/// still matches the CPU oracle, so the freed-and-regrown path is the same
/// pixels as the never-freed one.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with --ignored"]
fn idle_layer_targets_are_freed_after_the_hysteresis() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 120;
    const H: u32 = 90;
    let base = Color::rgba(250, 250, 248, 255);
    let mut planner = vieww_gpu::Planner::new();

    let deep = |depth: usize| {
        let mut scene = Scene::default();
        for _ in 0..depth {
            scene.push_command(Command::PushLayer {
                bounds: Rect::new(4.0, 4.0, 116.0, 86.0),
                alpha: 0.9,
                blend: BlendMode::Normal,
                clip: Clip::NONE,
                filter: ImageFilter::NONE,
            });
        }
        scene.push_command(Command::FillRect {
            rect: Rect::new(10.0, 10.0, 110.0, 80.0),
            paint: Paint::from(Color::rgb(60, 120, 240)),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        for _ in 0..depth {
            scene.push_command(Command::PopLayer);
        }
        scene
    };

    // One six-deep frame grows the target list to its depth...
    let scene = deep(6);
    let plan = planner.plan(&scene, W as f32, H as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    let deep_count = {
        let got = renderer
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
        let want = fresh
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        drop(fresh);
        assert_eq!(
            got, want,
            "the deep frame must match a renderer that never shrank"
        );
        renderer.target_count()
    };
    assert!(
        deep_count >= 14,
        "a six-deep frame must allocate its levels and scratches: {deep_count}"
    );

    // ...and a run of flat frames, past the narrowed hysteresis, frees the
    // surplus down to what they use.
    renderer.set_memory_trim_frames(4);
    let mut flat = Scene::default();
    flat.push_command(Command::FillRect {
        rect: Rect::new(10.0, 10.0, 110.0, 80.0),
        paint: Paint::from(Color::rgb(60, 120, 240)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
    for _ in 0..8 {
        let plan = planner.plan(&flat, W as f32, H as f32);
        renderer
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
    }
    let shrunk = renderer.target_count();
    assert!(
        shrunk <= 2,
        "flat frames past the hysteresis must free the deep frame's surplus: {shrunk}"
    );

    // The regrow path is the same pixels as the never-freed one.
    let plan = planner.plan(&deep(6), W as f32, H as f32);
    let got = renderer
        .render_planned(&planner, &plan, W, H, base)
        .expect("render should succeed");
    let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
    let want = fresh
        .render_planned(&planner, &plan, W, H, base)
        .expect("render should succeed");
    drop(fresh);
    assert_eq!(
        got, want,
        "the deep frame after the shrink must match a renderer that never shrank"
    );
}

/// A staging buffer that once served a full atlas upload — the megabytes a
/// single grow or compaction pins — decays back to the bytes the partial
/// uploads actually send, and the frames around the decay still match a full
/// upload's pixels.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with --ignored"]
fn oversized_staging_decays_after_the_hysteresis() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 200;
    const H: u32 = 150;
    let base = Color::rgba(250, 250, 248, 255);
    let logo = Image::from_rgba8(vec![220u8; 96 * 96 * 4], 96, 96);
    let churn = |frame: u32| {
        let (w, h) = (24u32, 24u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 9) as u8, (y * 7) as u8, frame as u8, 255]);
            }
        }
        Image::from_rgba8(rgba, w, h)
    };
    let mut planner = vieww_gpu::Planner::new();
    renderer.set_memory_trim_frames(4);

    let mut saw_full = 0;
    let mut capacity_after_decay = None;
    for frame in 0..24 {
        let mut scene = Scene::default();
        scene.push_command(Command::DrawImage {
            rect: Rect::new(8.0, 8.0, 104.0, 104.0),
            image: logo.clone(),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        scene.push_command(Command::DrawImage {
            rect: Rect::new(120.0, 20.0, 168.0, 68.0),
            image: churn(frame),
            transform: Transform::IDENTITY,
            clip: Clip::NONE,
        });
        let plan = planner.plan(&scene, W as f32, H as f32);
        assert!(plan.is_complete(), "{:?}", plan.unsupported);
        let got = renderer
            .render_planned(&planner, &plan, W, H, base)
            .expect("render should succeed");
        if frame == 0 {
            let side = u64::from(planner.image_atlas().side());
            assert_eq!(
                renderer.last_image_upload_bytes(),
                side * side * 4,
                "the first frame is a full upload, which sizes the staging"
            );
            saw_full = renderer.staging_capacity(StagingSlot::Images);
            assert!(
                saw_full >= side * side * 4,
                "the staging must hold the full upload: {saw_full}"
            );
        }
        if frame == 23 {
            let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
            let want = fresh
                .render_planned(&planner, &plan, W, H, base)
                .expect("render should succeed");
            drop(fresh);
            assert_eq!(
                got, want,
                "the frame after the staging decay must match a full upload"
            );
            capacity_after_decay = Some(renderer.staging_capacity(StagingSlot::Images));
        }
    }
    let decayed = capacity_after_decay.expect("the last frame ran");
    assert!(
        decayed * 8 <= saw_full,
        "the staging must decay from the full upload's {saw_full} bytes to the partial uploads' size, not stay at {decayed}"
    );
}

/// An atlas grow is carried, not re-sent: the frame whose gradients overflow
/// the ramp atlas' initial rows doubles the texture with one GPU copy of the
/// old rows and uploads only the new ramps' rectangles — the bytes the
/// renderer reports for that frame are one ramp row against the whole
/// (doubled) texture, and the pixels still match a renderer that never grew.
///
/// The ramp atlas is the vehicle because its doubling is a *pure* grow: its
/// compaction has an all-live early return, so a live set that only grows
/// reaches the doubling without any same-frame rewrite. (The image atlas'
/// half-full repack rewrites first, and the glyph atlas needs hundreds of
/// distinct glyphs to overflow — both are carries too, but this is the one a
/// test can reach deterministically.)
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with --ignored"]
fn an_atlas_grow_is_carried_not_reuploaded() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 200;
    const H: u32 = 150;
    let base = Color::rgba(250, 250, 248, 255);
    let gradient = |i: u32| {
        let hue = u8::try_from((i * 37) % 256).unwrap_or(0);
        Gradient::horizontal().with_stops(&[
            (0.0, Color::rgba(255 - hue, hue, 40, 255)),
            (1.0, Color::rgba(hue, 40, 255 - hue, 255)),
        ])
    };
    let scene_with = |count: u32| {
        let mut scene = Scene::default();
        for i in 0..count {
            let x = 4.0 + (i % 8) as f32 * 24.0;
            let y = 4.0 + (i / 8) as f32 * 22.0;
            scene.push_command(Command::FillRect {
                rect: Rect::new(x, y, x + 22.0, y + 20.0),
                paint: Paint::gradient(gradient(i)),
                transform: Transform::IDENTITY,
                clip: Clip::NONE,
            });
        }
        scene
    };
    let mut planner = vieww_gpu::Planner::new();

    // Sixteen distinct ramps fill the initial rows; the first upload is the
    // whole texture (the renderer has nothing yet).
    let scene = scene_with(vieww_gpu::RampAtlas::INITIAL_ROWS);
    let plan = planner.plan(&scene, W as f32, H as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    renderer
        .render_planned(&planner, &plan, W, H, base)
        .expect("render should succeed");
    let whole_initial = u64::from(vieww_gpu::RampAtlas::WIDTH)
        * u64::from(vieww_gpu::RampAtlas::INITIAL_ROWS)
        * 16;
    assert_eq!(
        renderer.last_ramp_upload_bytes(),
        whole_initial,
        "the first frame uploads the whole initial texture"
    );

    // One more ramp doubles the rows. The carry: the old rows ride the resize
    // as one GPU copy, and only the new ramp's row comes from the host.
    let scene = scene_with(vieww_gpu::RampAtlas::INITIAL_ROWS + 1);
    let plan = planner.plan(&scene, W as f32, H as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    assert!(
        planner.ramp_atlas().rows() > vieww_gpu::RampAtlas::INITIAL_ROWS,
        "the seventeenth ramp must double the rows"
    );
    let got = renderer
        .render_planned(&planner, &plan, W, H, base)
        .expect("render should succeed");
    let row = u64::from(vieww_gpu::RampAtlas::WIDTH) * 16;
    assert_eq!(
        renderer.last_ramp_upload_bytes(),
        row,
        "the doubling frame must send one ramp row, not the grown texture — the carry is not running"
    );
    // And the pixels match a renderer that uploaded the whole thing: the
    // carry must have preserved every existing row.
    let mut fresh = SceneRenderer::new(&device).expect("the pipeline should build");
    let want = fresh
        .render_planned(&planner, &plan, W, H, base)
        .expect("render should succeed");
    drop(fresh);
    assert_eq!(got, want, "the doubling frame's pixels differ from a full upload");

    // The frame after is steady again: nothing new, nothing sent.
    let plan = planner.plan(&scene, W as f32, H as f32);
    renderer
        .render_planned(&planner, &plan, W, H, base)
        .expect("render should succeed");
    assert_eq!(
        renderer.last_ramp_upload_bytes(),
        0,
        "a steady frame after the doubling uploads nothing"
    );
}
