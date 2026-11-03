//! The environment prefilter's GPU half against the CPU oracle (FW-21).
//!
//! `vieww_3d::environment::prefilter_texel` is the CPU half of a pin whose
//! WGSL half is `vieww-shaders`' `shaders/ibl_prefilter.wgsl` — the same
//! Hammersley sequence, the same GGX lobe, the same ndl-weighted gather,
//! written out line for line (each side holds the other to its formulas by
//! string test). This suite is the numeric join between them: for every
//! convolved level of a real chain, the GPU pass
//! (`vieww_hal::vulkan::VulkanDevice::render_env_prefilter_level`, fed the
//! very mip `Environment::prefilter_job` chose, at the oracle's own sample
//! count) must land within the f16 rounding of the upload and the download
//! on the CPU's own level.
//!
//! # The tolerance, and why it is what it is
//!
//! Everything but the storage is the same arithmetic in the same order, so
//! the disagreement has three sources, each bounded:
//!
//! * the source mip is uploaded RGBA16F — per texel relative error ≤ 2⁻¹¹
//!   ≈ 4.9e-4, and the errors of the texels a lobe gathers do not cancel
//!   (the weighting is the same one that produced them);
//! * the destination target is RGBA16F too — another ≤ 4.9e-4 on the way
//!   out (values above 1, the sun, survive because nothing clamps);
//! * the GPU's transcendentals (`atan2`, `acos`, `sin`, `cos`) differ from
//!   the CPU's by a few ulps, which perturbs the bilinear fetch position
//!   by ~1e-6 of a texel.
//!
//! Hence the bound this suite asserts per channel:
//! `|gpu − cpu| ≤ 2e-3·|cpu| + 5e-3` — roughly twice the worst case the
//! storage alone explains, and the teeth test below proves it is still
//! tight enough to notice a 0.2 error in roughness, so a passing run means
//! something.
//!
//! Run with `cargo test -p vieww-3d --features vulkan -- --ignored
//! --test-threads=1` — the lavapipe CI leg (`framework-checks.yml`'s
//! `gpu-lavapipe` job) and the self-hosted Linux box both do exactly that,
//! so this file joins them by existing.

#![cfg(feature = "vulkan")]

use std::sync::Mutex;
use std::sync::MutexGuard;

use vieww_3d::environment::{prefilter_texel, EnvMap, Environment, PrefilterJob};
use vieww_3d::math::Vec3;
use vieww_3d::scene::Rgb;
use vieww_hal::vulkan::VulkanDevice;

/// One `VulkanDevice` at a time — the other GPU suites' convention
/// (`gpu_unlit.rs`): software Vulkan serialises badly.
static ONE_DEVICE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn try_device() -> Option<(MutexGuard<'static, ()>, VulkanDevice)> {
    let guard = ONE_DEVICE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match VulkanDevice::new() {
        Ok(device) => Some((guard, device)),
        Err(error) => {
            eprintln!("skipping: no Vulkan device available ({error})");
            None
        }
    }
}

/// Run one `PrefilterJob` through the GPU pass, as the plumbing will.
fn run_level(device: &VulkanDevice, job: &PrefilterJob) -> Vec<f32> {
    #[allow(clippy::cast_possible_truncation)]
    let gpu = device.render_env_prefilter_level(
        job.dst_width as u32,
        job.dst_height as u32,
        &job.src_texels,
        job.src_width as u32,
        job.src_height as u32,
        job.roughness,
        job.samples,
    );
    gpu.expect("the prefilter pass renders")
}

/// The worst channel-wise disagreement between a GPU level (linear RGBA
/// f32) and a CPU level, as a multiple of the tolerance — `1.0` means "at
/// the bound", and a passing parity run is `≤ 1.0`.
fn worst_error(gpu: &[f32], cpu: &EnvMap) -> f32 {
    let mut worst = 0.0f32;
    for (texel, want) in gpu.as_chunks::<4>().0.iter().zip(cpu.pixels()) {
        for (got, reference) in [(texel[0], want.r), (texel[1], want.g), (texel[2], want.b)] {
            let bound = 2e-3 * reference.abs() + 5e-3;
            worst = worst.max((got - reference).abs() / bound);
        }
    }
    worst
}

/// `worst_error` for two CPU maps — used by the teeth test to prove its
/// wrong-roughness target is wrong on the CPU first.
fn worst_map_error(a: &EnvMap, b: &EnvMap) -> f32 {
    let mut worst = 0.0f32;
    for (got, want) in a.pixels().iter().zip(b.pixels()) {
        for (x, reference) in [(got.r, want.r), (got.g, want.g), (got.b, want.b)] {
            let bound = 2e-3 * reference.abs() + 5e-3;
            worst = worst.max((x - reference).abs() / bound);
        }
    }
    worst
}

/// The gallery of environments the parity runs over: a hot sun (HDR values
/// far above 1 — the reason the pass reads back linear, not sRGB bytes) and
/// a softbox room (values around 1, lobe-sized gradients everywhere).
fn environments() -> Vec<(&'static str, EnvMap)> {
    vec![
        (
            "daylight sky with a 60-radiance sun",
            EnvMap::sky(Vec3::new(0.35, 0.75, -0.55).normalize(), 4.0, 60.0),
        ),
        ("studio softboxes", EnvMap::studio(1.0)),
    ]
}

#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn every_prefiltered_level_matches_the_cpu_oracle() {
    let Some((_lock, device)) = try_device() else {
        return;
    };
    for (what, map) in environments() {
        let env = Environment::with_levels(map, 6, 64);
        for k in 1..env.specular.len() {
            let job = env.prefilter_job(k).expect("a job per convolved level");
            let gpu = run_level(&device, &job);
            assert_eq!(
                gpu.len(),
                job.dst_width * job.dst_height * 4,
                "{what}, level {k}: one linear RGBA texel per destination texel"
            );
            for (i, texel) in gpu.as_chunks::<4>().0.iter().enumerate() {
                assert_eq!(texel[3], 1.0, "{what}, level {k}, texel {i}: alpha is 1");
            }
            let worst = worst_error(&gpu, &env.specular[k]);
            assert!(
                worst <= 1.0,
                "{what}, level {k} (roughness {}): the GPU prefilter drifted \
                 {worst:.2}x the tolerance from the CPU oracle",
                job.roughness,
            );
            eprintln!("{what}, level {k}: worst {worst:.3}x the tolerance");
        }
    }
}

/// The GPU pass is deterministic on the device: the same job, twice, gives
/// the same texels to the bit — the property that makes a per-texel parity
/// mean anything (and the CPU half's own determinism is pinned in
/// `environment.rs`'s tests).
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn the_prefilter_pass_is_deterministic_on_the_device() {
    let Some((_lock, device)) = try_device() else {
        return;
    };
    let env = Environment::with_levels(
        EnvMap::sky(Vec3::new(-0.2, 0.6, 0.75).normalize(), 3.0, 25.0),
        4,
        64,
    );
    let job = env.prefilter_job(2).expect("level 2 exists");
    let a = run_level(&device, &job);
    let b = run_level(&device, &job);
    assert_eq!(a, b, "the same prefilter job rendered twice differs");
}

/// The teeth: a 0.2 error in roughness must FAIL the parity bound — first
/// on the CPU (recompute level 5's texels at level 4's roughness), then on
/// the GPU (run the pass with the wrong roughness) — so a green parity run
/// above means the pass got the lobe right, not that the bound forgives
/// everything.
#[test]
#[ignore = "needs a Vulkan loader + ICD (lavapipe or a real GPU); run with `cargo test --features vulkan -- --ignored`"]
fn the_tolerance_notices_a_wrong_roughness() {
    let Some((_lock, device)) = try_device() else {
        return;
    };
    let env = Environment::with_levels(
        EnvMap::sky(Vec3::new(0.3, 0.8, -0.4).normalize(), 2.0, 200.0),
        6,
        64,
    );
    let job = env.prefilter_job(5).expect("level 5 exists");
    let level = env.specular[5].clone();
    let wrong_roughness = 0.8; // level 4's, not level 5's 1.0.

    // CPU first: the oracle itself disagrees with its wrong-roughness twin
    // well past the bound, so the bound has teeth.
    let src = EnvMap::new(
        job.src_width,
        job.src_height,
        job.src_texels
            .iter()
            .map(|t| Rgb::new(t[0], t[1], t[2]))
            .collect(),
    );
    let wrong_cpu = EnvMap::from_fn(job.dst_width, job.dst_height, |r| {
        prefilter_texel(&src, r, wrong_roughness, job.samples)
    });
    assert!(
        worst_map_error(&wrong_cpu, &level) > 1.0,
        "the teeth target is not wrong: roughness {wrong_roughness} vs 1.0 agrees"
    );

    // GPU: the same wrong roughness through the pass must fail the same way.
    let mut wrong_job = job.clone();
    wrong_job.roughness = wrong_roughness;
    let gpu = run_level(&device, &wrong_job);
    assert!(
        worst_error(&gpu, &level) > 1.0,
        "the GPU pass at roughness {wrong_roughness} passed a bound the CPU teeth target fails"
    );
}
