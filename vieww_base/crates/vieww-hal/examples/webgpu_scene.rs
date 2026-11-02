//! FW-09's browser certification driver: the same scenes, rasterized by
//! the CPU oracle and drawn through the WebGPU backend, in one wasm.
//!
//! `web-test/webgpu_parity.py` loads this module with
//! `web/vieww-gpu-web.js` as its `vieww_gpu_web` import module, then
//! polls `webgpu_step` until `webgpu_state` settles and reads the
//! per-scene verdicts:
//!
//! * `webgpu_scene_status(i)` — 0 pending, 1 passed, 2 failed;
//! * `webgpu_scene_worst(i)` — the worst per-channel disagreement, in
//!   255ths;
//! * `webgpu_scene_interior(i)` — pixels over the tolerance on flat
//!   reference ground (the performance contract's failure count: any
//!   nonzero is a failure);
//! * `webgpu_scene_edge(i)` — pixels over the tolerance that hug an edge
//!   of the reference itself (allowed: two rasterizers disagree by design
//!   exactly there);
//! * `webgpu_scene_exact(i)` — 1 when the two frames are byte-identical
//!   (the empty scene must be: nothing was drawn, so there is no
//!   antialiasing to disagree about).
//!
//! The contract is `vieww-test-harness`'s `ParityContract::Performance`
//! as the hardware suites state it: interiors within 2 per channel,
//! disagreements only where the reference itself has an edge. The final
//! `webgpu_present` hands the last frame to a `<canvas id="vieww-canvas">`
//! through the shim's copy-mode post pass — the presentation path, exercised.
//!
//! On any non-wasm target this file is empty: the backend it drives only
//! exists on wasm.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;

use vieww_foundation::{BlendMode, Color, ImageFilter, Offset, Rect, Transform};
use vieww_gpu::Planner;
use vieww_hal::webgpu::WebGpuDevice;
use vieww_paint::native::NativeRenderer;
use vieww_paint::{Clip, Command, Paint, Scene};

const WIDTH: u32 = 120;
const HEIGHT: u32 = 90;
const TOLERANCE: u8 = 2;
const EDGE_GRADIENT: u8 = 8;

/// Where the whole run stands — what `webgpu_state` reports.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Bring-up or rendering still in flight; poll again.
    Running,
    /// Every scene passed the performance contract.
    Passed,
    /// Something refused; `webgpu_failure` has the message.
    Failed,
    /// The browser has no WebGPU (or no adapter): the canvas-CPU backend
    /// is the answer there, and this run certifies nothing.
    Skipped,
}

/// One scene's verdict, once its GPU frame landed.
#[derive(Clone, Copy, Default)]
struct Verdict {
    status: u32,
    worst: u32,
    interior: u32,
    edge: u32,
    exact: u32,
}

struct Run {
    phase: Phase,
    failure: String,
    device: Option<WebGpuDevice>,
    renderer: Option<vieww_hal::webgpu::WebGpuSceneRenderer<vieww_hal::webgpu::ExternBridge>>,
    planner: Planner,
    /// The scene being rendered, and whether its GPU frame is in flight.
    index: usize,
    submitted: bool,
    cpu: Vec<Vec<u8>>,
    verdicts: Vec<Verdict>,
    present: u32,
}

thread_local! {
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
}

/// The scenes, in order. Each is one of the shapes the hardware parity
/// suites pin: an empty frame, blended fills, a gradient ramp (the RGBA32F
/// atlas), group opacity (the OVER post pass) and a blurred layer (the
/// box-blur ping-pong). Five small frames, not one big one, so a failure
/// names the feature that broke.
fn scenes() -> Vec<Scene> {
    let mut out = Vec::new();

    out.push(Scene::default());

    let mut fills = Scene::default();
    fills.push_command(Command::FillRect {
        rect: Rect::new(10.0, 10.0, 70.0, 60.0),
        paint: Paint::solid(Color::rgba(200, 40, 40, 255)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
    fills.push_command(Command::FillRect {
        rect: Rect::new(40.0, 30.0, 110.0, 80.0),
        paint: Paint::solid(Color::rgba(40, 80, 220, 128)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
    out.push(fills);

    let mut gradient = Scene::default();
    let ramp = vieww_foundation::Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0))
        .with_stops(&[
            (0.0, Color::rgba(255, 240, 120, 255)),
            (0.5, Color::rgba(40, 170, 90, 255)),
            (1.0, Color::rgba(20, 30, 120, 255)),
        ])
        .with_dither();
    gradient.push_command(Command::FillRect {
        rect: Rect::new(8.0, 8.0, 112.0, 82.0),
        paint: Paint::gradient(ramp),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
    out.push(gradient);

    let mut opacity = Scene::default();
    for row in 0..6u16 {
        for col in 0..8u16 {
            let color = match (row + col) % 4 {
                0 => Color::rgba(230, 60, 40, 255),
                1 => Color::rgba(40, 170, 90, 255),
                2 => Color::rgba(50, 90, 220, 255),
                _ => Color::rgba(240, 220, 60, 255),
            };
            let (x, y) = (f32::from(col) * 16.0, f32::from(row) * 16.0);
            opacity.push_command(Command::FillRect {
                rect: Rect::new(x, y, x + 16.0, y + 16.0),
                paint: Paint::solid(color),
                transform: Transform::IDENTITY,
                clip: Clip::NONE,
            });
        }
    }
    opacity.push_command(Command::PushLayer {
        bounds: Rect::new(8.0, 8.0, 120.0, 88.0),
        alpha: 0.5,
        blend: BlendMode::Normal,
        filter: ImageFilter::NONE,
        clip: Clip::NONE,
    });
    opacity.push_command(Command::FillRect {
        rect: Rect::new(16.0, 16.0, 72.0, 64.0),
        paint: Paint::solid(Color::WHITE),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
    opacity.push_command(Command::FillRect {
        rect: Rect::new(48.0, 32.0, 112.0, 80.0),
        paint: Paint::solid(Color::rgba(0, 0, 0, 255)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
    opacity.push_command(Command::PopLayer);
    out.push(opacity);

    let mut blur = Scene::default();
    blur.push_command(Command::PushLayer {
        bounds: Rect::new(12.0, 12.0, 108.0, 78.0),
        alpha: 0.5,
        blend: BlendMode::Normal,
        filter: ImageFilter::blur(3.0),
        clip: Clip::NONE,
    });
    blur.push_command(Command::FillRect {
        rect: Rect::new(24.0, 24.0, 96.0, 66.0),
        paint: Paint::solid(Color::rgba(90, 200, 255, 255)),
        transform: Transform::IDENTITY,
        clip: Clip::NONE,
    });
    blur.push_command(Command::PopLayer);
    out.push(blur);

    out
}

/// The CPU oracle's frame for scene `i` — rendered in the same wasm, so
/// the comparison is one machine's two answers, not a stored golden.
fn cpu_frame(scene: &Scene) -> Vec<u8> {
    NativeRenderer::new()
        .render_to_pixels(scene, WIDTH, HEIGHT, Color::WHITE)
        .expect("the CPU rasterizer renders this scene")
        .0
        .data()
        .to_vec()
}

/// The performance contract's failure count, and the numbers beside it.
///
/// The rule is the hardware suites': a disagreement over the tolerance is
/// an *interior* violation unless the reference itself has a coverage
/// transition at that pixel (a neighbour more than `EDGE_GRADIENT` away in
/// some channel) — an edge is a property of the picture, not of the diff,
/// which is the mistake an earlier version of this comparison made.
fn compare(gpu: &[u8], cpu: &[u8]) -> Verdict {
    if gpu.len() != cpu.len() {
        return Verdict {
            status: 2,
            worst: 255,
            interior: u32::try_from(cpu.len()).unwrap_or(u32::MAX),
            edge: 0,
            exact: 0,
        };
    }
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let channel = |b: &[u8], i: usize, c: usize| b[i * 4 + c];
    let mut worst = 0u8;
    let mut interior = 0usize;
    let mut edge = 0usize;
    let mut exact = 1u32;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let d = (0..4)
                .map(|c| channel(gpu, i, c).abs_diff(channel(cpu, i, c)))
                .max()
                .unwrap_or(0);
            if d != 0 {
                exact = 0;
            }
            worst = worst.max(d);
            if d <= TOLERANCE {
                continue;
            }
            let on_reference_edge = (-1i64..=1).any(|dy| {
                (-1i64..=1).any(|dx| {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        return false;
                    }
                    let n = ny as usize * w + nx as usize;
                    (0..4).any(|c| channel(cpu, i, c).abs_diff(channel(cpu, n, c)) > EDGE_GRADIENT)
                })
            });
            if on_reference_edge {
                edge += 1;
            } else {
                interior += 1;
            }
        }
    }
    let status = if interior == 0 { 1 } else { 2 };
    Verdict {
        status,
        worst: u32::from(worst),
        interior: interior as u32,
        edge: edge as u32,
        exact,
    }
}

/// Advance the run one poll. Called by the page once per frame.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_step() -> u32 {
    RUN.with(|cell| {
        let mut run = cell.borrow_mut();
        let run = run.get_or_insert_with(|| Run {
            phase: Phase::Running,
            failure: String::new(),
            device: None,
            renderer: None,
            planner: Planner::new(),
            index: 0,
            submitted: false,
            cpu: Vec::new(),
            verdicts: Vec::new(),
            present: 0,
        });

        if run.phase != Phase::Running {
            return phase_code(run.phase);
        }

        // Start: probe availability, then begin bring-up.
        if run.device.is_none() {
            if !WebGpuDevice::available() {
                run.phase = Phase::Skipped;
                run.failure = "navigator.gpu is absent".to_owned();
                return phase_code(run.phase);
            }
            run.device = Some(WebGpuDevice::request());
            return phase_code(run.phase);
        }
        let device = run.device.as_ref().expect("just set");

        // Bring-up: the poll protocol — Pending stays Running.
        if run.renderer.is_none() {
            match device.scene_renderer() {
                Ok(renderer) => {
                    run.cpu = scenes().iter().map(cpu_frame).collect();
                    run.renderer = Some(renderer);
                }
                Err(vieww_hal::webgpu::WebGpuError::Pending) => {}
                Err(vieww_hal::webgpu::WebGpuError::NoWebGpu) => {
                    run.phase = Phase::Skipped;
                }
                Err(error) => {
                    run.phase = Phase::Failed;
                    run.failure = error.to_string();
                }
            }
            return phase_code(run.phase);
        }

        let scene_list = scenes();
        while run.index < scene_list.len() {
            let scene = &scene_list[run.index];
            let plan = run.planner.plan(scene, WIDTH as f32, HEIGHT as f32);
            if !plan.is_complete() {
                run.phase = Phase::Failed;
                run.failure = format!(
                    "scene {}/{}: the planner left gaps: {:?}",
                    run.index,
                    scene_list.len(),
                    plan.unsupported
                );
                return phase_code(run.phase);
            }
            if !run.submitted {
                let renderer = run.renderer.as_mut().expect("checked above");
                if let Err(error) =
                    renderer.render_planned(&run.planner, &plan, WIDTH, HEIGHT, Color::WHITE)
                {
                    run.phase = Phase::Failed;
                    run.failure = format!("scene {}: render: {error}", run.index);
                    return phase_code(run.phase);
                }
                run.submitted = true;
                return phase_code(run.phase);
            }
            let renderer = run.renderer.as_mut().expect("checked above");
            let Some(gpu) = renderer.readback() else {
                // A device that fails *after* bring-up (a pipeline the
                // engine rejects, device lost) never crosses
                // `render_planned`'s error path — the readback simply
                // never lands, and without this poll the run would spin
                // in Running until the harness times out. The shim
                // reports the same state code its bring-up does, so one
                // poll per step is enough to name the failure instead.
                if let Some(device) = run.device.as_ref() {
                    if let vieww_hal::webgpu::BringUp::Failed(message) = device.state() {
                        run.phase = Phase::Failed;
                        run.failure = format!(
                            "scene {}: the device failed mid-run: {}",
                            run.index, message
                        );
                    }
                }
                return phase_code(run.phase);
            };
            let verdict = compare(&gpu, &run.cpu[run.index]);
            run.verdicts.push(verdict);
            run.index += 1;
            run.submitted = false;
        }

        // Every frame drawn: present the last one into the page's canvas.
        let renderer = run.renderer.as_mut().expect("checked above");
        run.present = u32::from(renderer.present("vieww-canvas").is_ok());
        run.phase = if run.verdicts.iter().all(|v| v.status == 1) {
            Phase::Passed
        } else {
            Phase::Failed
        };
        phase_code(run.phase)
    })
}

const fn phase_code(phase: Phase) -> u32 {
    match phase {
        Phase::Running => 0,
        Phase::Passed => 1,
        Phase::Failed => 2,
        Phase::Skipped => 3,
    }
}

/// Where the whole run stands: 0 running, 1 passed, 2 failed, 3 skipped.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_state() -> u32 {
    RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref().map_or(0, |r| phase_code(r.phase))
    })
}

/// How many scenes the run certifies.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_scenes() -> u32 {
    scenes().len() as u32
}

/// Scene `i`'s verdict: 0 pending, 1 passed, 2 failed.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_scene_status(index: u32) -> u32 {
    RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref()
            .and_then(|r| r.verdicts.get(index as usize))
            .map_or(0, |v| v.status)
    })
}

/// Scene `i`'s worst per-channel disagreement, in 255ths.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_scene_worst(index: u32) -> u32 {
    RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref()
            .and_then(|r| r.verdicts.get(index as usize))
            .map_or(0, |v| v.worst)
    })
}

/// Scene `i`'s over-tolerance pixels on flat reference ground — the
/// performance contract's failure count.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_scene_interior(index: u32) -> u32 {
    RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref()
            .and_then(|r| r.verdicts.get(index as usize))
            .map_or(0, |v| v.interior)
    })
}

/// Scene `i`'s over-tolerance pixels hugging an edge of the reference —
/// allowed: that is where two rasterizers disagree by design.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_scene_edge(index: u32) -> u32 {
    RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref()
            .and_then(|r| r.verdicts.get(index as usize))
            .map_or(0, |v| v.edge)
    })
}

/// 1 when scene `i`'s two frames are byte-identical. The empty scene must
/// be: nothing was drawn, so there is no coverage to disagree about.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_scene_exact(index: u32) -> u32 {
    RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref()
            .and_then(|r| r.verdicts.get(index as usize))
            .map_or(0, |v| v.exact)
    })
}

/// 1 when the last frame presented into the canvas.
#[unsafe(no_mangle)]
pub extern "C" fn webgpu_presented() -> u32 {
    RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref().map_or(0, |r| r.present)
    })
}

/// The run's failure (or skip) message, into `buf` (at most `cap` bytes,
/// NUL-terminated); returns the message's full length.
///
/// # Safety
///
/// `buf` must point at `cap` writable bytes for the duration of the call
/// (the page's JS owns it), and `cap` must not exceed the buffer's real
/// size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn webgpu_failure(buf: *mut u8, cap: u32) -> u32 {
    let message = RUN.with(|cell| {
        let run = cell.borrow();
        run.as_ref().map_or_else(String::new, |r| r.failure.clone())
    });
    let bytes = message.as_bytes();
    if !buf.is_null() && cap > 0 {
        let n = bytes.len().min(cap as usize - 1);
        // SAFETY: the caller (the page's JS, through the wasm export)
        // guarantees `cap` writable bytes at `buf`; `n < cap`.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
            *buf.add(n) = 0;
        }
    }
    bytes.len() as u32
}
