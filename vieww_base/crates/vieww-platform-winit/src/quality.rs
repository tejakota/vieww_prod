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
//! # Why the scale is opt-in, and off by default
//!
//! The CPU path never scales: its output is the fidelity reference
//! (`docs/FIDELITY.md` §1) and it must stay byte-identical. The GPU path at a
//! scale below 1.0 renders the plan into smaller targets and the final blit
//! enlarges them — pixels move, so the default is full resolution and
//! `VIEWW_GPU_SCALE` opts in (`auto`, or a number between 0.25 and 1.0).
//! Device Farm's `gpuscale` pass measures what it buys on the phones where
//! the GPU path currently loses: if 0.5× turns the LG Stylo 5's 40.1 ms into
//! a pass, the tier profile has earned its default; if not, nothing changed.

use std::sync::OnceLock;

use crate::device_tier::{device_facts, DeviceTier};

/// The render decisions a device tier gets by default. Every field is a
/// default, not a mandate: explicit application settings and diagnostics
/// (`VIEWW_GPU_SCALE`) override the profile, and the profile's own numbers
/// are what Device Farm measures against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityProfile {
    /// The fraction of the swapchain's resolution the GPU path renders at.
    /// `1.0` is full resolution; the CPU path is unaffected by this field
    /// whatever it says (see the module docs).
    pub resolution_scale: f32,
}

impl QualityProfile {
    /// The Low tier: the LG Stylo 5 class. A quarter of the pixels — the
    /// single biggest lever on a fill-rate-bound frame, and the first thing
    /// the `gpuscale` pass tests.
    pub const LOW: Self = Self {
        resolution_scale: 0.5,
    };

    /// The Medium tier: the A36 / iPhone 13 class, where measurements already
    /// pick a rasterizer that passes. Three quarters keeps the density loss
    /// at the edge of noticeability while still cutting a third of the fill.
    pub const MEDIUM: Self = Self {
        resolution_scale: 0.75,
    };

    /// The High tier: everything passes with room, so nothing is scaled.
    pub const HIGH: Self = Self {
        resolution_scale: 1.0,
    };

    /// The profile a tier gets by default.
    #[must_use]
    pub const fn for_tier(tier: DeviceTier) -> Self {
        match tier {
            DeviceTier::Low => Self::LOW,
            DeviceTier::Medium => Self::MEDIUM,
            DeviceTier::High => Self::HIGH,
        }
    }
}

/// The resolution scale the GPU path renders at this process, from
/// `VIEWW_GPU_SCALE`: `None` is full resolution (the default), `auto` takes
/// the probed tier's profile, and a number is clamped into `0.25..=1.0`.
/// Read once, before any thread that could set the environment races it.
pub(crate) fn gpu_scale() -> Option<f32> {
    static SCALE: OnceLock<Option<f32>> = OnceLock::new();
    *SCALE.get_or_init(|| {
        let requested = std::env::var("VIEWW_GPU_SCALE").ok()?;
        let requested = requested.trim();
        if requested.is_empty() || requested.eq_ignore_ascii_case("off") {
            return None;
        }
        let scale = if requested.eq_ignore_ascii_case("auto") {
            QualityProfile::for_tier(device_facts().tier).resolution_scale
        } else {
            requested.parse::<f32>().ok()?.clamp(0.25, 1.0)
        };
        // A scale of 1.0 is "off" by another name: the render path should not
        // run its smaller-targets-and-scaled-blit machinery for no change.
        (scale < 1.0).then_some(scale)
    })
}

/// The GPU scale line the on-device suite observes as `gpu_scale` — `None`
/// when the process is at full resolution, so the suite log stays quiet
/// unless the mechanism is actually engaged.
#[must_use]
pub fn gpu_scale_status() -> Option<String> {
    let scale = gpu_scale()?;
    let source = std::env::var("VIEWW_GPU_SCALE")
        .map(|value| {
            if value.trim().eq_ignore_ascii_case("auto") {
                format!("auto: tier {}", device_facts().tier.name())
            } else {
                "set".to_owned()
            }
        })
        .unwrap_or_else(|_| "set".to_owned());
    Some(format!(
        "GPU path renders at {scale:.2}x resolution (VIEWW_GPU_SCALE={source})"
    ))
}

#[cfg(test)]
mod tests {
    use super::{gpu_scale, QualityProfile};
    use crate::device_tier::DeviceTier;

    #[test]
    fn the_profiles_match_the_tiers() {
        assert_eq!(
            QualityProfile::for_tier(DeviceTier::Low),
            QualityProfile::LOW
        );
        assert_eq!(
            QualityProfile::for_tier(DeviceTier::Medium),
            QualityProfile::MEDIUM
        );
        assert_eq!(
            QualityProfile::for_tier(DeviceTier::High),
            QualityProfile::HIGH
        );
    }

    #[test]
    fn the_scale_is_off_unless_asked() {
        // Only meaningful when nothing asked; when something did, whichever
        // test calls `gpu_scale` first caches the answer for all of them.
        if std::env::var_os("VIEWW_GPU_SCALE").is_none() {
            assert_eq!(gpu_scale(), None);
        }
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
/// Desktops keep the core count: a large window's full repaint is exactly
/// the long raster the walk was built for, and no desktop measurement says
/// otherwise. Pixels cannot move either way. `VIEWW_RASTER_THREADS` still
/// overrides.
#[must_use]
pub(crate) const fn raster_threads_for(tier: DeviceTier, mobile: bool, ios: bool) -> Option<usize> {
    if !mobile {
        return None;
    }
    // `ios` is kept in the signature for the day a platform split is
    // measured; today every phone above Low walks serially on both.
    let _ = ios;
    match tier {
        DeviceTier::Medium | DeviceTier::High => Some(1),
        DeviceTier::Low => None,
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
    vieww_paint::native::set_process_raster_small_masks(path == crate::native::PresentPath::Vulkan);
    APPLIED.get_or_init(|| {
        let tier = device_facts().tier;
        vieww_paint::native::set_process_cache_budgets(cache_budgets_for(tier));
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let ios = cfg!(target_os = "ios");
        if let Some(request) = raster_threads_for(tier, mobile, ios) {
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
    fn phones_above_low_walk_serially_and_everything_else_in_parallel() {
        // Android: Low parallel; Medium serial (the Redmi's parallel run,
        // 2026-11-16, was twice as slow); High serial.
        assert_eq!(raster_threads_for(DeviceTier::Low, true, false), None);
        assert_eq!(raster_threads_for(DeviceTier::Medium, true, false), Some(1));
        assert_eq!(raster_threads_for(DeviceTier::High, true, false), Some(1));
        // iOS: no measurement has ever run the pool there, and iPhones pass
        // already — everything above Low stays serial.
        assert_eq!(raster_threads_for(DeviceTier::Low, true, true), None);
        assert_eq!(raster_threads_for(DeviceTier::Medium, true, true), Some(1));
        assert_eq!(raster_threads_for(DeviceTier::High, true, true), Some(1));
        // Desktops keep the core count at every tier.
        for tier in [DeviceTier::Low, DeviceTier::Medium, DeviceTier::High] {
            assert_eq!(
                raster_threads_for(tier, false, false),
                None,
                "{tier:?} desktop"
            );
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
