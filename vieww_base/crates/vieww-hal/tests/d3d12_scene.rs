//! `D3d12SceneRenderer` against the CPU rasterizer — the D3D12 twin of
//! `tests/vulkan_scene.rs`, and the first thing that ever *executes* it:
//! the `framework-checks` Windows runner, on WARP or a hardware adapter.
//!
//! The oracle, the tolerance shape and the edge rule are the Vulkan suite's,
//! verbatim — see that file for why the comparison is `NativeRenderer` and
//! not a stored image, and why "interiors must match, edges may not" is the
//! assertion: the same reasoning, one backend later. What is *different* is
//! the reach: this file pins the port's first-increment claims (the shared
//! traversal through root signatures and descriptor tables, the target
//! format, the readback conversion, the retained-damage path), not Vulkan's
//! full matrix — the compositor suite's scenes come across as the port is
//! validated on hardware, which is what the Vulkan backend's own history
//! did too.
//!
//! `#[ignore]`d as every hardware-gated suite here is:
//! `cargo test -p vieww-hal --features d3d12 -- --ignored`. The Windows
//! runner sets `VIEWW_D3D12_ALLOW_WARP=1`; WARP is correctness evidence,
//! never performance evidence.

#![cfg(feature = "d3d12")]

#[path = "support/parity_row.rs"]
mod parity_row;

use vieww_foundation::{BlendMode, Color, ImageFilter, Offset, Rect, Shadow, Transform};
use vieww_gpu::Planner;
use vieww_hal::d3d12::{D3d12Device, D3d12SceneRenderer};
use vieww_paint::native::NativeRenderer;
use vieww_paint::{Canvas, Clip, Command, Paint, Scene};

const WIDTH: u32 = 120;
const HEIGHT: u32 = 90;

/// One D3D12 device at a time for the whole file, as the Vulkan suites
/// serialise theirs: these tests each bring up a device, and the runner may
/// be WARP.
static ONE_DEVICE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The guard is returned, not dropped here: it has to be held for as long as
/// the device lives.
type Exclusive = std::sync::MutexGuard<'static, ()>;

fn try_renderer() -> Option<(Exclusive, D3d12Device, D3d12SceneRenderer)> {
    let guard = ONE_DEVICE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let device = match D3d12Device::new() {
        Ok(device) => device,
        Err(error) => {
            eprintln!("skipping: no D3D12 device available ({error})");
            return None;
        }
    };
    let renderer = D3d12SceneRenderer::new(&device).expect("the pipelines should build");
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

/// Count pixels where the two renderers disagree, excluding pixels that sit
/// on a shape's antialiased edge — `tests/vulkan_scene.rs`'s
/// `interior_mismatches`, verbatim: an edge is a property of the reference
/// picture, not of the diff.
fn interior_mismatches(gpu: &[u8], cpu: &[u8], tolerance: u8) -> usize {
    interior_mismatches_at(gpu, cpu, tolerance, WIDTH, HEIGHT)
}

/// [`interior_mismatches`] at another frame size — the compositor matrix's
/// scenes run at the Vulkan suite's 128x96.
fn interior_mismatches_at(gpu: &[u8], cpu: &[u8], tolerance: u8, width: u32, height: u32) -> usize {
    assert_eq!(gpu.len(), cpu.len(), "same dimensions");
    let w = width as usize;
    let h = height as usize;
    let channel = |buffer: &[u8], index: usize, c: usize| buffer[index * 4 + c];
    let differs = |index: usize| {
        (0..4).any(|c| channel(gpu, index, c).abs_diff(channel(cpu, index, c)) > tolerance)
    };
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
                // The first few, with values: a count alone says *that* the
                // backends disagree, not how (a missing pass, a wrong blend,
                // an alpha convention, a channel swap all look different).
                if count < 8 {
                    let i = (y * w + x) * 4;
                    eprintln!(
                        "mismatch at ({x}, {y}): gpu {:?} cpu {:?}",
                        &gpu[i..i + 4],
                        &cpu[i..i + 4]
                    );
                }
                count += 1;
            }
        }
    }
    parity_row::record("d3d12", tolerance, gpu, cpu, count);
    count
}

#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn an_empty_scene_is_the_clear_colour_exactly() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let clear = Color::rgba(20, 30, 40, 255);
    let mut planner = Planner::new();
    let plan = planner.plan(&Scene::default(), WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "an empty scene has nothing unsupported");

    let pixels = renderer
        .render_planned(&planner, &plan, WIDTH, HEIGHT, clear)
        .expect("render should succeed");

    for pixel in pixels.as_chunks::<4>().0 {
        assert_eq!(pixel, &[20, 30, 40, 255], "every pixel is the clear colour");
    }
}

#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn one_rectangle_lands_where_the_geometry_says() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::WHITE;
    let mut scene = Scene::default();
    fill_rect(
        &mut scene,
        Rect::new(20.0, 20.0, 80.0, 60.0),
        Color::rgba(255, 0, 0, 255),
    );
    let mut planner = Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);

    let gpu = renderer
        .render_planned(&planner, &plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);

    let mismatches = interior_mismatches(&gpu, &cpu, 2);
    assert_eq!(
        mismatches, 0,
        "GPU and CPU must agree away from shape edges; {mismatches} interior pixels differed"
    );
}

#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn overlapping_translucent_fills_composite_like_the_cpu_rasterizer() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::WHITE;
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
    let mut planner = Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete());

    let gpu = renderer
        .render_planned(&planner, &plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);

    let mismatches = interior_mismatches(&gpu, &cpu, 2);
    assert_eq!(
        mismatches, 0,
        "GPU and CPU must agree away from shape edges; {mismatches} interior pixels differed"
    );
}

#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn a_group_opacity_layer_composites_like_the_cpu_rasterizer() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::WHITE;
    let mut scene = Scene::default();
    let layer = Rect::new(10.0, 10.0, 100.0, 70.0);
    scene.push_layer(layer, 0.5, vieww_foundation::BlendMode::Normal);
    fill_rect(
        &mut scene,
        Rect::new(20.0, 20.0, 80.0, 60.0),
        Color::rgba(255, 0, 0, 255),
    );
    fill_rect(
        &mut scene,
        Rect::new(40.0, 30.0, 90.0, 55.0),
        Color::rgba(0, 0, 255, 255),
    );
    scene.pop_layer();

    let mut planner = Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(
        plan.is_complete(),
        "a plain group-opacity layer is in reach"
    );
    assert!(
        plan.target_depth >= 1,
        "the layer needs an offscreen target"
    );

    let gpu = renderer
        .render_planned(&planner, &plan, WIDTH, HEIGHT, base)
        .expect("render should succeed");
    let cpu = on_cpu(&scene, base);

    let mismatches = interior_mismatches(&gpu, &cpu, 2);
    assert_eq!(
        mismatches, 0,
        "GPU and CPU must agree away from shape edges; {mismatches} interior pixels differed"
    );
}

#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn the_renderer_refuses_a_plan_it_cannot_draw() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    // Layers nested past MAX_TARGET_DEPTH: beyond what any backend's target
    // stack holds, with or without a Planner's atlases. (This used a shaped
    // clip, which the Planner's mask atlas now draws — so the plan came out
    // complete and the test failed for a capability gained, not lost.)
    let mut scene = Scene::default();
    let full = Rect::new(0.0, 0.0, 120.0, 90.0);
    for _ in 0..=vieww_gpu::MAX_TARGET_DEPTH {
        scene.push_layer(full, 0.9, vieww_foundation::BlendMode::Normal);
    }
    fill_rect(&mut scene, full, Color::rgba(200, 30, 30, 255));
    for _ in 0..=vieww_gpu::MAX_TARGET_DEPTH {
        scene.pop_layer();
    }

    let mut planner = Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(!plan.is_complete());

    let refused = renderer.render_planned(&planner, &plan, WIDTH, HEIGHT, Color::WHITE);
    assert!(
        refused.is_err(),
        "an incomplete plan must be refused, not partly drawn"
    );
    let message = refused.unwrap_err().to_string();
    assert!(
        message.contains("layer"),
        "the error should name what stopped it, got: {message}"
    );
}

#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
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
    let mut planner = Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);

    // Several frames at one size: the point of the persistent pipelines.
    for _ in 0..8 {
        let pixels = renderer
            .render_planned(&planner, &plan, WIDTH, HEIGHT, Color::rgba(0, 0, 0, 255))
            .expect("every frame should render");
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
    }

    // Then a resize, which must rebuild only the size-dependent objects.
    let bigger = renderer
        .render_planned(
            &planner,
            &plan,
            WIDTH * 2,
            HEIGHT * 2,
            Color::rgba(0, 0, 0, 255),
        )
        .expect("a resize should not break the renderer");
    assert_eq!(bigger.len(), (WIDTH * 2 * HEIGHT * 2 * 4) as usize);

    // And back down again, which is the case a naive cache gets wrong.
    let smaller = renderer
        .render_planned(
            &planner,
            &plan,
            WIDTH / 2,
            HEIGHT / 2,
            Color::rgba(0, 0, 0, 255),
        )
        .expect("shrinking should work too");
    assert_eq!(smaller.len(), (WIDTH / 2 * (HEIGHT / 2) * 4) as usize);
}

#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn a_damaged_frame_keeps_the_previous_frame_outside_the_damage() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let base = Color::WHITE;
    let mut first = Scene::default();
    fill_rect(
        &mut first,
        Rect::new(0.0, 0.0, 120.0, 90.0),
        Color::rgba(30, 30, 200, 255),
    );
    fill_rect(
        &mut first,
        Rect::new(10.0, 10.0, 40.0, 40.0),
        Color::rgba(255, 0, 0, 255),
    );
    let mut planner = Planner::new();
    let plan_first = planner.plan(&first, WIDTH as f32, HEIGHT as f32);
    let whole_first = renderer
        .render_planned(&planner, &plan_first, WIDTH, HEIGHT, base)
        .expect("the first frame should render");

    // The same scene with only the small square moved, and a damage rect
    // that covers exactly the square's old position — not the new one.
    let mut second = Scene::default();
    fill_rect(
        &mut second,
        Rect::new(0.0, 0.0, 120.0, 90.0),
        Color::rgba(30, 30, 200, 255),
    );
    fill_rect(
        &mut second,
        Rect::new(60.0, 50.0, 90.0, 80.0),
        Color::rgba(255, 0, 0, 255),
    );
    let plan_second = planner.plan(&second, WIDTH as f32, HEIGHT as f32);
    let damage = [vieww_gpu::PixelRect {
        x0: 5,
        y0: 5,
        x1: 45,
        y1: 45,
    }];
    let (damaged_pixels, redrawn) = renderer
        .render_planned_damaged(&planner, &plan_second, WIDTH, HEIGHT, base, Some(&damage))
        .expect("the damaged frame should render");
    assert_eq!(
        redrawn.pixels(WIDTH, HEIGHT),
        40 * 40,
        "the damage is the only thing the frame redrew"
    );

    // The retained contract, in bytes. Outside the damage the target was
    // kept untouched, so it holds *exactly* the first frame's pixels — the
    // same target, the same bytes.
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let inside = (5..45).contains(&x) && (5..45).contains(&y);
            if inside {
                continue;
            }
            let index = (y * WIDTH as usize + x) * 4;
            assert_eq!(
                &damaged_pixels[index..index + 4],
                &whole_first[index..index + 4],
                "pixel ({x}, {y}) outside the damage is the previous frame, byte for byte"
            );
        }
    }
    // Inside the damage the second scene was drawn — and the second scene
    // there is plain background, so the old square is genuinely gone.
    for y in 5..45usize {
        for x in 5..45usize {
            let index = (y * WIDTH as usize + x) * 4;
            assert_eq!(
                &damaged_pixels[index..index + 4],
                &[30, 30, 200, 255],
                "pixel ({x}, {y}) inside the damage is the new scene's background"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The compositor matrix, graduated — the scenes `tests/vulkan_compositor.rs`
// held from the port's first increment (see this file's own header). The
// doc's why-not.md put it as "bring the Vulkan compositor matrix across as
// each port is validated"; the Windows runner (WARP first, hardware after)
// is the validation, so the matrix follows: every blend mode, group opacity
// against it, a shaped clip, a shadow, a dithered gradient. The oracle and
// the tolerance stay the Vulkan suite's — 2/255 interior, edges free.

/// The compositor scenes' shared backdrop: two opaque fills.
fn backdrop(scene: &mut Scene) {
    fill_rect(
        scene,
        Rect::new(0.0, 0.0, 128.0, 96.0),
        Color::rgba(30, 30, 46, 255),
    );
    fill_rect(
        scene,
        Rect::new(8.0, 8.0, 88.0, 72.0),
        Color::rgba(120, 200, 90, 255),
    );
}

fn push_layer(
    scene: &mut Scene,
    bounds: Rect,
    alpha: f32,
    blend: BlendMode,
    filter: ImageFilter,
    clip: Clip,
) {
    scene.push_command(Command::PushLayer {
        bounds,
        alpha,
        blend,
        clip,
        filter,
    });
}

/// All twenty-eight blend modes through a translucent group over an opaque
/// and a translucent backdrop — the modes that read dst alpha have something
/// other than 1.0 to read. The Vulkan suite's scene, verbatim.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn all_twenty_eight_blend_modes_match() {
    use BlendMode as B;
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let modes = [
        B::Normal,
        B::Clear,
        B::Src,
        B::Dst,
        B::DstOver,
        B::SrcIn,
        B::DstIn,
        B::SrcOut,
        B::DstOut,
        B::SrcAtop,
        B::DstAtop,
        B::Xor,
        B::Plus,
        B::Multiply,
        B::Screen,
        B::Overlay,
        B::Darken,
        B::Lighten,
        B::ColorDodge,
        B::ColorBurn,
        B::HardLight,
        B::SoftLight,
        B::Difference,
        B::Exclusion,
        B::Hue,
        B::Saturation,
        B::Color,
        B::Luminosity,
    ];
    let mut planner = Planner::new();
    for mode in modes {
        let mut scene = Scene::default();
        backdrop(&mut scene);
        fill_rect(
            &mut scene,
            Rect::new(0.0, 64.0, 128.0, 96.0),
            Color::rgba(90, 20, 160, 0),
        );
        push_layer(
            &mut scene,
            Rect::new(12.0, 12.0, 116.0, 84.0),
            0.85,
            mode,
            ImageFilter::NONE,
            Clip::NONE,
        );
        fill_rect(
            &mut scene,
            Rect::new(20.0, 20.0, 76.0, 60.0),
            Color::rgba(250, 140, 30, 255),
        );
        fill_rect(
            &mut scene,
            Rect::new(52.0, 36.0, 108.0, 76.0),
            Color::rgba(30, 200, 230, 150),
        );
        scene.push_command(Command::PopLayer);
        let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);
        assert!(plan.is_complete(), "{mode:?}: {:?}", plan.unsupported);
        let gpu = renderer
            .render_planned(&planner, &plan, WIDTH, HEIGHT, Color::WHITE)
            .expect("render should succeed");
        let bad = interior_mismatches(&gpu, &on_cpu(&scene, Color::WHITE), 2);
        assert_eq!(bad, 0, "blend {mode:?}: {bad} interior pixels disagree");
    }
}

/// Group opacity composites the group once, not each child — the property
/// that made `PushLayer` exist.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn group_opacity_composites_the_group_not_each_child() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let mut scene = Scene::default();
    backdrop(&mut scene);
    push_layer(
        &mut scene,
        Rect::new(8.0, 8.0, 120.0, 88.0),
        0.5,
        BlendMode::Normal,
        ImageFilter::NONE,
        Clip::NONE,
    );
    fill_rect(
        &mut scene,
        Rect::new(16.0, 16.0, 72.0, 64.0),
        Color::rgba(255, 255, 255, 255),
    );
    fill_rect(
        &mut scene,
        Rect::new(48.0, 32.0, 112.0, 80.0),
        Color::rgba(0, 0, 0, 255),
    );
    scene.push_command(Command::PopLayer);
    let mut planner = Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    let gpu = renderer
        .render_planned(&planner, &plan, WIDTH, HEIGHT, Color::WHITE)
        .expect("render should succeed");
    assert_eq!(
        interior_mismatches(&gpu, &on_cpu(&scene, Color::WHITE), 2),
        0
    );
}

/// A shadow over a plain ground — the cached-coverage tint pass, held to the
/// CPU's own blurred answer.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn a_shadow_matches_the_cpu() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let mut scene = Scene::default();
    scene.push_command(Command::DrawShadow {
        rect: Rect::new(24.0, 20.0, 96.0, 70.0),
        radius: 8.0,
        shadow: Shadow::new(Color::rgba(0, 0, 0, 110), Offset::new(0.0, 6.0), 14.0),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
        covered: false,
    });
    fill_rect(
        &mut scene,
        Rect::new(24.0, 20.0, 96.0, 70.0),
        Color::rgba(240, 240, 244, 255),
    );
    let mut planner = Planner::new();
    let plan = planner.plan(&scene, WIDTH as f32, HEIGHT as f32);
    assert!(plan.is_complete(), "{:?}", plan.unsupported);
    let gpu = renderer
        .render_planned(&planner, &plan, WIDTH, HEIGHT, Color::WHITE)
        .expect("render should succeed");
    assert_eq!(
        interior_mismatches(&gpu, &on_cpu(&scene, Color::WHITE), 2),
        0
    );
}

// ---------------------------------------------------------------------------
// The rest of the compositor matrix: the scenes `tests/vulkan_compositor.rs`
// holds that the first graduation (above) deferred — nested layers, backdrop
// blur, colour matrices and image transforms. The plan's own order ("bring the
// Vulkan compositor matrix across as each port is validated"): WARP has held
// the graduated half green, so the rest follows. Scenes and backdrops are the
// Vulkan suite's verbatim, at its 128x96 — one frame size for the whole
// matrix, so a WARP failure and a lavapipe failure describe the same scene.

/// The compositor suite's own frame size (its scenes' rects are laid out for
/// it; the first-increment tests above keep this file's 120x90).
const CW: u32 = 128;
const CH: u32 = 96;

/// The compositor scenes' shared backdrop: the Vulkan suite's checkerboard —
/// pixel-aligned saturated colours, so blend modes, blurs and backdrop
/// filters have something to visibly act on and no edge is left to two
/// rasterization rules.
fn checker(scene: &mut Scene) {
    for row in 0..6 {
        for col in 0..8 {
            let color = match (row + col) % 4 {
                0 => Color::rgba(230, 60, 40, 255),
                1 => Color::rgba(40, 170, 90, 255),
                2 => Color::rgba(50, 90, 220, 255),
                _ => Color::rgba(240, 220, 60, 255),
            };
            let x = col as f32 * 16.0;
            let y = row as f32 * 16.0;
            fill(
                scene,
                Rect::new(x, y, x + 16.0, y + 16.0),
                color,
                Clip::NONE,
            );
        }
    }
}

/// `fill_rect` with a clip, as the compositor scenes need both.
fn fill(scene: &mut Scene, rect: Rect, color: Color, clip: Clip) {
    scene.push_command(Command::FillRect {
        rect,
        paint: Paint::solid(color),
        transform: Transform::IDENTITY,
        clip,
    });
}

/// A rounded-rect clip, as the Vulkan suite builds them.
fn rounded_clip(rect: Rect, radius: f32) -> Clip {
    let mut clip = Clip::NONE;
    clip.add_path(vieww_foundation::Path::rounded_rect(rect, radius));
    clip
}

/// Draw `scene` on both renderers at the compositor size and count interior
/// mismatches — the same edge rule as every test above, at `CW x CH`.
fn compositor_mismatches(renderer: &mut D3d12SceneRenderer, scene: &Scene, clear: Color) -> usize {
    let mut planner = Planner::new();
    let plan = planner.plan(scene, CW as f32, CH as f32);
    assert!(plan.is_complete(), "plan has gaps: {:?}", plan.unsupported);
    let gpu = renderer
        .render_planned(&planner, &plan, CW, CH, clear)
        .expect("render should succeed");
    let mut cpu_renderer = NativeRenderer::new();
    let (pixels, _) = cpu_renderer
        .render_to_pixels(scene, CW, CH, clear)
        .expect("the CPU rasterizer should render this scene");
    interior_mismatches(&gpu, pixels.data(), 2)
}

/// Three layers deep with a blend, a blur and a shaped clip in the stack —
/// the scene that exercises target nesting (`L[1]`, `L[2]`, their scratch
/// twins), the filter chain over a damage-grown band, and the mask multiply
/// at pop, all in one frame. The Vulkan suite's scene, verbatim.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn nested_layers_with_mixed_effects_match() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let mut scene = Scene::default();
    checker(&mut scene);
    push_layer(
        &mut scene,
        Rect::new(0.0, 0.0, 128.0, 96.0),
        0.9,
        BlendMode::Multiply,
        ImageFilter::NONE,
        Clip::NONE,
    );
    fill(
        &mut scene,
        Rect::new(8.0, 8.0, 120.0, 88.0),
        Color::rgba(250, 230, 200, 255),
        Clip::NONE,
    );
    push_layer(
        &mut scene,
        Rect::new(16.0, 16.0, 112.0, 80.0),
        0.7,
        BlendMode::Screen,
        ImageFilter::blur(2.0),
        rounded_clip(Rect::new(16.0, 16.0, 112.0, 80.0), 10.0),
    );
    fill(
        &mut scene,
        Rect::new(24.0, 24.0, 64.0, 72.0),
        Color::rgba(0, 90, 200, 255),
        Clip::NONE,
    );
    push_layer(
        &mut scene,
        Rect::new(48.0, 32.0, 104.0, 72.0),
        1.0,
        BlendMode::Difference,
        ImageFilter::NONE,
        Clip::NONE,
    );
    fill(
        &mut scene,
        Rect::new(48.0, 32.0, 104.0, 72.0),
        Color::rgba(255, 255, 255, 255),
        Clip::NONE,
    );
    scene.push_command(Command::PopLayer);
    scene.push_command(Command::PopLayer);
    scene.push_command(Command::PopLayer);
    assert_eq!(
        compositor_mismatches(&mut renderer, &scene, Color::WHITE),
        0,
        "nested layers with mixed effects must match away from edges"
    );
}

/// A backdrop blur: the checker behind the layer blurs, the glass tint and
/// the foreground chip on top of it stay sharp — the pass that reads the
/// destination *before* the layer's own content exists.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn a_backdrop_blur_blurs_what_is_behind_and_keeps_the_foreground_sharp() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let mut scene = Scene::default();
    checker(&mut scene);
    push_layer(
        &mut scene,
        Rect::new(16.0, 16.0, 112.0, 80.0),
        1.0,
        BlendMode::Normal,
        ImageFilter::backdrop_blur(3.0),
        rounded_clip(Rect::new(16.0, 16.0, 112.0, 80.0), 14.0),
    );
    fill(
        &mut scene,
        Rect::new(16.0, 16.0, 112.0, 80.0),
        Color::rgba(255, 255, 255, 90),
        Clip::NONE,
    );
    fill(
        &mut scene,
        Rect::new(40.0, 40.0, 88.0, 56.0),
        Color::rgba(10, 10, 10, 255),
        Clip::NONE,
    );
    scene.push_command(Command::PopLayer);
    assert_eq!(
        compositor_mismatches(&mut renderer, &scene, Color::WHITE),
        0,
        "the backdrop blur must match the CPU away from edges"
    );
}

/// A colour-matrix layer: the grayscale matrix over a translucent fill, the
/// pass mode that carries 20 floats in the immediates block.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn a_colour_matrix_layer_matches() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let mut scene = Scene::default();
    checker(&mut scene);
    push_layer(
        &mut scene,
        Rect::new(8.0, 8.0, 120.0, 88.0),
        0.9,
        BlendMode::Normal,
        ImageFilter::color(vieww_foundation::grayscale_matrix()),
        Clip::NONE,
    );
    fill(
        &mut scene,
        Rect::new(16.0, 16.0, 112.0, 80.0),
        Color::rgba(220, 40, 120, 200),
        Clip::NONE,
    );
    scene.push_command(Command::PopLayer);
    assert_eq!(
        compositor_mismatches(&mut renderer, &scene, Color::WHITE),
        0,
        "the colour matrix must match the CPU away from edges"
    );
}

/// The Vulkan suite's test image: gradients in x and y, a hard pattern in
/// blue, and alpha holes — so magnification, rotation and minification each
/// have a different interpolation answer to get right.
fn test_image(w: u32, h: u32) -> vieww_foundation::Image {
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            pixels.extend_from_slice(&[
                (x * 255 / w.max(1)) as u8,
                (y * 255 / h.max(1)) as u8,
                (((x / 3) + (y / 3)) % 2 * 200) as u8,
                if (x + y) % 7 == 0 { 90 } else { 255 },
            ]);
        }
    }
    vieww_foundation::Image::from_rgba8(pixels, w, h)
}

/// Images magnified, rotated and minified — the image atlas's bilinear
/// sampling and mips against the CPU's own. The rotated case's edges are
/// excluded by the interior rule, as every shaped edge here is.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn images_magnified_rotated_and_minified_match() {
    let Some((_lock, _device, mut renderer)) = try_renderer() else {
        return;
    };
    let cases = [
        (
            "image-magnified",
            test_image(12, 9),
            Rect::new(10.0, 10.0, 118.0, 86.0),
            Transform::IDENTITY,
        ),
        (
            "image-rotated",
            test_image(20, 20),
            Rect::new(-30.0, -30.0, 30.0, 30.0),
            Transform::rotate(0.5).then(Transform::translate(Offset::new(64.0, 48.0))),
        ),
        (
            "image-minified",
            test_image(256, 192),
            Rect::new(16.0, 12.0, 112.0, 84.0),
            Transform::IDENTITY,
        ),
        (
            "image-minified-far",
            test_image(300, 300),
            Rect::new(40.0, 20.0, 70.0, 50.0),
            Transform::IDENTITY,
        ),
    ];
    for (name, image, rect, transform) in cases {
        let mut scene = Scene::default();
        fill(
            &mut scene,
            Rect::new(0.0, 0.0, 128.0, 96.0),
            Color::rgba(30, 30, 30, 255),
            Clip::NONE,
        );
        scene.push_command(Command::DrawImage {
            rect,
            image,
            transform,
            clip: Clip::NONE,
        });
        assert_eq!(
            compositor_mismatches(&mut renderer, &scene, Color::WHITE),
            0,
            "{name}: the sampled image must match the CPU away from edges"
        );
    }
}

// ---------------------------------------------------------------------------
// The atlas-diet and trim twins: partial uploads for every atlas, the
// carried grow, and the idle target trim — `tests/vulkan_scene.rs`'s tests on
// this backend, so every backend sends the same bytes for the same churn.

/// A text reveal that mixes scripts: every few characters bring in glyphs
/// the atlas has not seen, which is what a line scrolling in does.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn partial_glyph_uploads_match_full_uploads_frame_for_frame() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 260;
    const H: u32 = 100;
    let base = Color::rgba(250, 250, 248, 255);
    let mut store = vieww_text::FontStore::embedded_only();
    let reveal = "Hamburgefonstiv 123 — The quick brown fox jumps over the lazy dog! \
                  ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ; αβγδεζ. \
                  Съешь ещё этих мягких французских булок, да выпей чаю.";
    let body = vieww_foundation::TextStyle {
        color: Color::rgba(20, 20, 30, 255),
        size: 24.0,
        ..vieww_foundation::TextStyle::default()
    };
    let label_style = vieww_foundation::TextStyle {
        color: Color::rgba(120, 30, 30, 255),
        size: 17.0,
        ..vieww_foundation::TextStyle::default()
    };
    let mut planner = Planner::new();
    let (mut partial, mut full) = (0, 0);
    let (mut sent, mut whole) = (0u64, 0u64);
    for frame in 0..40 {
        let shown: String = reveal.chars().take((frame as usize + 1) * 3).collect();
        let mut scene = Scene::default();
        for (text, style, y) in [
            (shown.as_str(), body, 40.0),
            ("0123456789", label_style, 82.0),
        ] {
            let paragraph = vieww_text::Paragraph::layout(
                &mut store,
                &[vieww_text::TextSpan::new(text, style)],
                1_000.0,
            );
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
            assert!(
                inked > 300,
                "the frame must have drawn text: {inked} inked pixels"
            );
        }
        let side = u64::from(planner.atlas().side());
        let bytes = renderer.last_atlas_upload_bytes().0;
        sent += bytes;
        whole += side * side;
        match bytes {
            0 => {}
            b if b == side * side => full += 1,
            _ => partial += 1,
        }
        let mut fresh = D3d12SceneRenderer::new(&device).expect("the pipelines should build");
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
/// only the changed rectangles.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
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
        vieww_foundation::Image::from_rgba8(rgba, w, h)
    };
    let logo = churn(200);
    let mut planner = Planner::new();
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
            assert!(
                inked > 3_000,
                "the frame must have drawn images: {inked} inked pixels"
            );
        }
        let side = u64::from(planner.image_atlas().side());
        let bytes = renderer.last_atlas_upload_bytes().1;
        sent += bytes;
        whole += side * side * 4;
        match bytes {
            0 => {}
            b if b == side * side * 4 => full += 1,
            _ => partial += 1,
        }
        let mut fresh = D3d12SceneRenderer::new(&device).expect("the pipelines should build");
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

/// A gradient whose stops animate is a new ramp row every frame, and the ramp
/// texture takes only that row instead of the whole texture.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn partial_ramp_uploads_match_full_uploads_frame_for_frame() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 140;
    const H: u32 = 90;
    let base = Color::rgba(250, 250, 248, 255);
    let steady = vieww_foundation::Gradient::horizontal().with_stops(&[
        (0.0, Color::rgba(30, 30, 200, 255)),
        (1.0, Color::rgba(200, 30, 30, 255)),
    ]);
    let mut planner = Planner::new();
    let (mut partial, mut full) = (0, 0);
    let (mut sent, mut whole) = (0u64, 0u64);
    for frame in 0..24 {
        let hue = (frame * 11 % 256) as u8;
        let animated = vieww_foundation::Gradient::horizontal().with_stops(&[
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
            assert!(
                inked > 4_000,
                "the frame must have drawn gradients: {inked} inked pixels"
            );
        }
        let texture =
            u64::from(vieww_gpu::RampAtlas::WIDTH) * u64::from(planner.ramp_atlas().rows()) * 16;
        let bytes = renderer.last_atlas_upload_bytes().2;
        sent += bytes;
        whole += texture;
        match bytes {
            0 => {}
            b if b == texture => full += 1,
            _ => partial += 1,
        }
        let mut fresh = D3d12SceneRenderer::new(&device).expect("the pipelines should build");
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
/// still matches a renderer that never shrank.
#[test]
#[ignore = "needs a D3D12 device — the framework-checks Windows runner (WARP allowed); run with `cargo test --features d3d12 -- --ignored`"]
fn idle_layer_targets_are_freed_after_the_hysteresis() {
    let Some((_lock, device, mut renderer)) = try_renderer() else {
        return;
    };
    const W: u32 = 120;
    const H: u32 = 90;
    let base = Color::rgba(250, 250, 248, 255);
    let mut planner = Planner::new();

    let deep = |depth: usize| {
        let mut scene = Scene::default();
        for _ in 0..depth {
            push_layer(
                &mut scene,
                Rect::new(4.0, 4.0, 116.0, 86.0),
                0.9,
                BlendMode::Normal,
                ImageFilter::NONE,
                Clip::NONE,
            );
        }
        fill(
            &mut scene,
            Rect::new(10.0, 10.0, 110.0, 80.0),
            Color::rgba(60, 120, 240, 255),
            Clip::NONE,
        );
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
        let mut fresh = D3d12SceneRenderer::new(&device).expect("the pipelines should build");
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
    fill(
        &mut flat,
        Rect::new(10.0, 10.0, 110.0, 80.0),
        Color::rgba(60, 120, 240, 255),
        Clip::NONE,
    );
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
    let mut fresh = D3d12SceneRenderer::new(&device).expect("the pipelines should build");
    let want = fresh
        .render_planned(&planner, &plan, W, H, base)
        .expect("render should succeed");
    drop(fresh);
    assert_eq!(
        got, want,
        "a deep frame after a shrink must match a renderer that never shrank"
    );
}
