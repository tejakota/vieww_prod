//! The quality profile: where a device tier turns into render decisions.
//!
//! # Why this exists
//!
//! [`App::auto_rasterizer`](crate::App::auto_rasterizer) chooses *who* draws;
//! nothing yet decides *how much* the draw is allowed to cost. On the farm's
//! Low phone both answers miss the budget (LG Stylo 5: CPU 36.2 ms, GPU
//! 40.1 ms against 16.7 — Device Farm, 2026-10-17), and the cheapest lever a
//! fill-rate-bound GPU frame has is how many pixels it fills. A
//! [`QualityProfile`] is the one struct those decisions live in: the tier
//! picks one ([`QualityProfile::for_tier`]), and the render path reads it.
//!
//! # What it gates today, and what it is for tomorrow
//!
//! One field: [`resolution_scale`][Self::resolution_scale], read by the GPU
//! path's direct route (`NativeRenderer::present_planned` →
//! `SceneRenderer::render_planned_to`). Effect gates (blurs, shadows — the
//! advisory list's next candidates) join the struct when the scene planner
//! can skip them; a field nothing reads would be a guess about a renderer
//! that has not been measured yet.
//!
//! # The scale is the tier's default on phones (FW-01)
//!
//! The CPU path never scales: its output is the fidelity reference
//! (`docs/FIDELITY.md` §1) and it must stay byte-identical. The GPU path at a
//! scale below 1.0 renders the plan into smaller targets and the final blit
//! enlarges them — pixels move. Device Farm's `gpuscale` pass settled the
//! question this module used to leave open: the LG Stylo 5's GPU path runs at
//! 19.1 ms at full resolution and 8.2 ms at 0.5×, so the tier profile has
//! earned its default and **a phone no longer needs an environment variable
//! to pass its budget.**
//!
//! `VIEWW_GPU_SCALE` is now an override: `off` (or `1`) forces full
//! resolution, `auto` or unset takes the tier's profile, a number between 0.25
//! and 1.0 forces that scale. Desktops resolve to full resolution through
//! [`QualityProfile::for_device`] whatever their tier says.

use std::sync::OnceLock;

use crate::device_tier::{default_quality_profile, device_facts, DeviceTier};

pub use vieww_foundation::QualityProfile;

/// How `VIEWW_GPU_SCALE` asks the scale to be chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ScaleRequest {
    /// Unset, empty or `auto`: the device's own profile.
    Auto,
    /// `off`, `full` or `1`: full resolution whatever the tier says.
    Full,
    /// A number, clamped into `0.25..=1.0`.
    Fixed(f32),
}

/// Parse the variable's value. Anything unparseable is `Auto` rather than an
/// error: a typo in a diagnostic variable must not silently *disable* the
/// tier default on the phones that need it.
pub(crate) fn parse_scale_request(value: Option<&str>) -> ScaleRequest {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return ScaleRequest::Auto;
    };
    if value.eq_ignore_ascii_case("auto") {
        return ScaleRequest::Auto;
    }
    if value.eq_ignore_ascii_case("off") || value.eq_ignore_ascii_case("full") {
        return ScaleRequest::Full;
    }
    match value.parse::<f32>() {
        Ok(n) if n.is_finite() => ScaleRequest::Fixed(n.clamp(0.25, 1.0)),
        _ => ScaleRequest::Auto,
    }
}

/// The scale a request resolves to under `profile`; `None` is "no scaling".
/// A scale of 1.0 is "off" by another name: the render path should not run its
/// smaller-targets-and-scaled-blit machinery for no change.
pub(crate) fn resolve_scale(request: ScaleRequest, profile: QualityProfile) -> Option<f32> {
    let scale = match request {
        ScaleRequest::Auto => profile.resolution_scale,
        ScaleRequest::Full => 1.0,
        ScaleRequest::Fixed(n) => n,
    };
    (scale < 1.0).then_some(scale)
}

/// The resolution scale the GPU path renders at this process: `None` is full
/// resolution. The tier's profile is the default; `VIEWW_GPU_SCALE` overrides
/// it. Read once, before any thread that could set the environment races it.
pub(crate) fn gpu_scale() -> Option<f32> {
    static SCALE: OnceLock<Option<f32>> = OnceLock::new();
    *SCALE.get_or_init(|| {
        let request = parse_scale_request(std::env::var("VIEWW_GPU_SCALE").ok().as_deref());
        resolve_scale(request, default_quality_profile())
    })
}

/// The GPU scale line the on-device suite observes as `gpu_scale` — `None`
/// when the process is at full resolution, so the suite log stays quiet
/// unless the mechanism is actually engaged.
#[must_use]
pub fn gpu_scale_status() -> Option<String> {
    let scale = gpu_scale()?;
    // The `tier <name>` wording is parsed by the farm's verdict tooling
    // (`gpuscale_verdict.py`), so it stays in every message.
    let var = std::env::var("VIEWW_GPU_SCALE").ok();
    let source = match parse_scale_request(var.as_deref()) {
        ScaleRequest::Auto if var.as_deref().is_some_and(|v| !v.trim().is_empty()) => {
            format!("VIEWW_GPU_SCALE=auto: tier {}", device_facts().tier.name())
        }
        ScaleRequest::Auto => format!("default for tier {}", device_facts().tier.name()),
        ScaleRequest::Fixed(_) => "VIEWW_GPU_SCALE override".to_owned(),
        ScaleRequest::Full => "VIEWW_GPU_SCALE=off".to_owned(),
    };
    Some(format!("GPU path renders at {scale:.2}x resolution ({source})"))
}

#[cfg(test)]
mod tests {
    use super::{parse_scale_request, resolve_scale, QualityProfile, ScaleRequest};
    use crate::device_tier::DeviceTier;
    use vieww_foundation::DeviceClass;

    #[test]
    fn the_profiles_match_the_tiers() {
        assert_eq!(
            QualityProfile::for_tier(DeviceTier::Low),
            QualityProfile::LOW
        );
        assert_eq!(
            QualityProfile::for_tier(DeviceTier::High),
            QualityProfile::HIGH
        );
    }

    #[test]
    fn a_low_phone_scales_with_no_environment_variable() {
        // FW-01's done criterion, at the policy level: unset means the tier.
        let low = QualityProfile::for_device(DeviceTier::Low, DeviceClass::Handheld);
        assert_eq!(resolve_scale(parse_scale_request(None), low), Some(0.5));
        let mid = QualityProfile::for_device(DeviceTier::Medium, DeviceClass::Handheld);
        assert_eq!(resolve_scale(parse_scale_request(None), mid), Some(0.75));
        let high = QualityProfile::for_device(DeviceTier::High, DeviceClass::Handheld);
        assert_eq!(resolve_scale(parse_scale_request(None), high), None);
    }

    #[test]
    fn a_desktop_is_not_scaled_even_when_its_tier_is_low() {
        let low_desktop = QualityProfile::for_device(DeviceTier::Low, DeviceClass::Desktop);
        assert_eq!(resolve_scale(ScaleRequest::Auto, low_desktop), None);
    }

    #[test]
    fn the_environment_variable_overrides_the_profile_both_ways() {
        let low = QualityProfile::LOW;
        assert_eq!(resolve_scale(parse_scale_request(Some("off")), low), None);
        assert_eq!(resolve_scale(parse_scale_request(Some("1")), low), None);
        assert_eq!(resolve_scale(parse_scale_request(Some("0.6")), low), Some(0.6));
        assert_eq!(resolve_scale(parse_scale_request(Some("0.01")), low), Some(0.25), "clamped");
        let high = QualityProfile::HIGH;
        assert_eq!(resolve_scale(parse_scale_request(Some("0.5")), high), Some(0.5));
        assert_eq!(resolve_scale(parse_scale_request(Some("auto")), low), Some(0.5));
    }

    #[test]
    fn a_typo_in_the_variable_falls_back_to_the_tier_not_to_off() {
        assert_eq!(parse_scale_request(Some("hlaf")), ScaleRequest::Auto);
        assert_eq!(parse_scale_request(Some("NaN")), ScaleRequest::Auto);
        assert_eq!(parse_scale_request(Some("  ")), ScaleRequest::Auto);
    }
}

/// The derived-value cache budgets a tier gets (`vieww_paint::native`'s
/// blur-band and path-fill caches). Pixels cannot move — both caches memoise
/// a deterministic function — so unlike [`QualityProfile::resolution_scale`]
/// this needs no opt-in: a smaller budget changes only how often the CPU
/// path re-derives a blur or a fill, never what it draws. Since Device Farm
/// run 39382602099 (`vieww_paint::native::CacheBudgets`' docs): the path-fill
/// cache is off on every tier, and the blur cache runs on High only — each
/// measured slower on, on the phones it is now off for. `VIEWW_BLUR_CACHE_MB`
/// / `VIEWW_PATH_CACHE_MB` still override any tier.
#[must_use]
pub const fn cache_budgets_for(tier: DeviceTier) -> vieww_paint::native::CacheBudgets {
    match tier {
        DeviceTier::Low => vieww_paint::native::CacheBudgets::LOW_MEMORY,
        DeviceTier::Medium => vieww_paint::native::CacheBudgets::MID_MEMORY,
        DeviceTier::High => vieww_paint::native::CacheBudgets::DEFAULT,
    }
}

/// The row-parallel scanline walk's worker request a tier gets, or `None`
/// for the machine's core count. `mobile` is whether this is a phone or
/// tablet (Android, iOS); `ios` splits the phones by platform, because the
/// evidence is Android-only.
///
/// **Serial on Medium and High phones, parallel on Low ones and desktops.**
///
/// *2026-11-16, Redmi Note 7 Pro (SM6150, Medium): parallel lost by half.*
/// `b9a8870` turned the walk parallel on Android Medium phones with the rule
/// that it goes back to serial if it does not win live and storm. Against
/// the four serial runs of the same day on the same phone (raster p50 scroll
/// 6.6-6.7 ms, live 15.3-16.0, storm 30.5-33.1, thermal 16.3-17.5), the
/// parallel run measured 14.4, 26.7, 65.6 and 28.0 — about twice as slow in
/// every continuous phase, a gap many times the 1 ms the serial runs moved
/// between themselves. The SM6150 has two big cores and six little ones; a
/// chunk on a little core finishes last and the frame waits for it, which
/// is the likeliest reason, though no run has measured where the workers
/// ran. The reasoning below is what the change rested on, kept for the
/// record.
///
/// *Previously:* Android Medium phones walk parallel again; iOS and High stay serial;
/// Low and desktops always walked parallel. The serial default for
/// everything above Low came from Device Farm run 39402544760's same-unit
/// A/B on the demo screen (`cpunothreads` against `cpu`), raster stage
/// medians: Galaxy A73 (Medium) 13.61 serial against 14.36 with threads,
/// Galaxy S26 Ultra (High) 5.00 against 5.26, worst frames worse on both.
/// Two things have changed since that run took its numbers:
///
/// - The CPU present path's small-mask gate did not exist yet (it landed
///   with runs 43714067671 / 43722791555): a Medium phone walking parallel
///   then handed the pool every 48-px mask, which is the flood those runs
///   measured at 23-48 ms p95 on the Stylo's CPU pass. Today the same
///   phone hands the pool nothing under 128 device pixels, so the light
///   frames that lost 0.75 ms to hand-offs barely engage it at all.
/// - The Market Watch comparison (2026-11-16, Redmi Note 7 Pro, Medium by
///   RAM) puts the *product* load's raster medians at 13.8 ms (scroll),
///   24.4 ms (live) and 36.5 ms (storm) — 65-80% of every frame, the long
///   raster the walk was built for and the demo screen never showed. Every
///   parallel half of a damaged CPU frame — the region clears, the RGBA8
///   converts, the big composites, the large walks — sits behind that same
///   128-px gate, so a heavy frame splits across the cores while a light
///   one stays serial by payload size, not by tier.
///
/// The comparison carries the same-unit A/B that decides this
/// (`vieww-cpuserial`, `debug.mw.threads=0`): if the parallel leg does not
/// beat it on the Redmi's storm and live phases, Medium goes back to serial
/// with that run's numbers. High stays serial either way — both demo A/Bs
/// cost it raster time it does not need, its frames pass with room — and
/// iOS stays serial where it always was: no run has ever measured the pool
/// there, and the iPhone 13's CPU frames already pass at 5.8-7.5 ms.
/// Desktops: Linux keeps the core count; Windows and macOS walk serially.
/// The wide comparison (run 43870118033, 2026-11-16) benched the same
/// frames headless with both walks on GitHub's machines, and the pool only
/// paid on Linux (storm 7.0-7.4 ms against 8.3-8.4 serial, x64 and arm64).
/// Windows x64 lost 10.48 against 9.10, Windows arm64 9.30 against 9.01,
/// macOS arm64 12.13 against 10.60, and scroll and live lost on all three.
///
/// Low on the CPU present path walks serially too. In the same run the
/// msm8953 phone (Low, eight A53s) took 81.7 ms of storm raster with the
/// pool against 58.7 ms serial, with shadows alone at 26.4 against 6.4, and
/// lost on every phase. Low on the Vulkan path keeps the pool, because its
/// small-mask hand-offs are off the critical path and won that pass 6-8 ms
/// (runs 43714067671 / 43722791555).
///
/// Pixels cannot move either way. `VIEWW_RASTER_THREADS` still overrides.
#[must_use]
pub(crate) const fn raster_threads_for(
    tier: DeviceTier,
    mobile: bool,
    ios: bool,
    linux: bool,
    gpu: bool,
) -> Option<usize> {
    if !mobile {
        return if linux { None } else { Some(1) };
    }
    // `ios` is kept in the signature for the day a platform split is
    // measured; today every phone walks serially on the CPU path.
    let _ = ios;
    match tier {
        DeviceTier::Medium | DeviceTier::High => Some(1),
        DeviceTier::Low if gpu => None,
        DeviceTier::Low => Some(1),
    }
}

/// Applies the probed tier's cache budgets and raster-thread default to the
/// process, once, before the first renderer is built (the caches read the
/// setting at construction, the scanline walk at its first fill), and the
/// present path's small-mask gate — every call, because a window whose
/// Vulkan open failed falls back to the CPU path after the first call.
pub(crate) fn apply_cache_budgets(path: crate::native::PresentPath) {
    static APPLIED: OnceLock<()> = OnceLock::new();
    // The GPU present path rasters only the atlases its planner builds, so
    // a 48-px mask handed to the pool is work off its critical path; the
    // CPU present path composites the frame itself, and the same hand-off
    // measured 23-48 ms p95 against it on the Stylo's eight A53s (runs
    // 43714067671 / 43722791555) while winning the GPU pass 6-8 ms. The
    // gate therefore follows the path — `set_process_raster_small_masks`'
    // docs in `vieww_paint::native` have the numbers.
    vieww_paint::native::set_process_raster_small_masks(path.is_gpu());
    APPLIED.get_or_init(|| {
        let tier = device_facts().tier;
        vieww_paint::native::set_process_cache_budgets(cache_budgets_for(tier));
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let ios = cfg!(target_os = "ios");
        let linux = cfg!(target_os = "linux");
        let gpu = path.is_gpu();
        if let Some(request) = raster_threads_for(tier, mobile, ios, linux, gpu) {
            vieww_paint::native::set_process_raster_threads(request);
        }
    });
}

/// The cache-budget line the on-device suite observes as `cache_budgets`:
/// what each cache and the scanline walk are **actually** running with,
/// and where that came from.
///
/// It used to print the tier's budget with an "overrides" note beside it —
/// so Device Farm run 39273227484's `cpubare` pass, which ran both caches
/// at 0, reported "blur 2 MiB [VIEWW_BLUR_CACHE_MB overrides]", the number
/// the override had replaced. Each value now follows the cache's own
/// reading of its variable (a value that parses wins; anything else falls
/// back to the tier's), and the raster threads are reported beside them,
/// because the A/B passes switch those too.
#[must_use]
pub fn cache_budgets_status() -> String {
    let budgets = vieww_paint::native::process_cache_budgets();
    let tier = device_facts().tier.name();
    let effective = |var: &str, tier_bytes: usize| -> String {
        match std::env::var(var)
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
        {
            Some(0) => format!("off ({var}=0)"),
            Some(mb) => format!("{mb} MiB ({var})"),
            None if tier_bytes == 0 => format!("off (tier {tier})"),
            None => format!("{} MiB (tier {tier})", tier_bytes / (1024 * 1024)),
        }
    };
    let workers = vieww_paint::native::raster_worker_limit();
    let threads = if workers <= 1 {
        match std::env::var("VIEWW_RASTER_THREADS") {
            Ok(v) => format!("serial (VIEWW_RASTER_THREADS={})", v.trim()),
            Err(_) if vieww_paint::native::raster_threads_set_by_platform() => {
                format!("serial (tier {tier})")
            }
            Err(_) => "serial (one core)".to_string(),
        }
    } else {
        format!("up to {workers} workers per large mask")
    };
    format!(
        "blur {}, fill {}, raster threads {}",
        effective("VIEWW_BLUR_CACHE_MB", budgets.blur_bytes),
        effective("VIEWW_PATH_CACHE_MB", budgets.fill_bytes),
        threads,
    )
}

#[cfg(test)]
mod raster_thread_tests {
    use super::*;

    #[test]
    fn only_linux_desktops_and_low_phones_on_vulkan_use_the_pool() {
        use DeviceTier::{High, Low, Medium};
        // Phones (Android, then iOS): Low keeps the pool only on Vulkan.
        for ios in [false, true] {
            assert_eq!(raster_threads_for(Low, true, ios, false, true), None);
            assert_eq!(raster_threads_for(Low, true, ios, false, false), Some(1));
            for gpu in [false, true] {
                assert_eq!(raster_threads_for(Medium, true, ios, false, gpu), Some(1));
                assert_eq!(raster_threads_for(High, true, ios, false, gpu), Some(1));
            }
        }
        // Desktops: Linux keeps the core count, Windows and macOS walk serially.
        for tier in [Low, Medium, High] {
            for gpu in [false, true] {
                assert_eq!(
                    raster_threads_for(tier, false, false, true, gpu),
                    None,
                    "{tier:?} linux"
                );
                assert_eq!(
                    raster_threads_for(tier, false, false, false, gpu),
                    Some(1),
                    "{tier:?} win/mac"
                );
            }
        }
    }
}

#[cfg(test)]
mod cache_budget_tests {
    use super::*;

    #[test]
    fn every_tier_maps_to_a_budget_no_larger_than_the_default() {
        let default = vieww_paint::native::CacheBudgets::DEFAULT;
        for tier in [DeviceTier::Low, DeviceTier::Medium, DeviceTier::High] {
            let b = cache_budgets_for(tier);
            assert!(b.blur_bytes <= default.blur_bytes && b.fill_bytes <= default.fill_bytes);
        }
        assert_eq!(cache_budgets_for(DeviceTier::High), default);
    }

    #[test]
    fn the_caches_run_where_they_measured_a_win() {
        // Run 39382602099: the fill cache cost time on every phone, the blur
        // cache on the Stylo 5 (Low) and A73 (Medium) but paid on the S26
        // Ultra (High). The row-parallel walk is not a budget and stays on.
        for tier in [DeviceTier::Low, DeviceTier::Medium, DeviceTier::High] {
            assert_eq!(cache_budgets_for(tier).fill_bytes, 0, "{tier:?}");
        }
        assert_eq!(cache_budgets_for(DeviceTier::Low).blur_bytes, 0);
        assert_eq!(cache_budgets_for(DeviceTier::Medium).blur_bytes, 0);
        assert!(cache_budgets_for(DeviceTier::High).blur_bytes > 0);
    }
}
