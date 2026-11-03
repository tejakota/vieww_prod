//! Estimating what a pass costs on a given executor and device.
//!
//! Two layers, matching `docs/RENDERER-V2-NOTES.md`'s "feed measured cost
//! back into planning": [`HeuristicCostModel`] is a closed-form estimate
//! from [`vieww_scene::CostHint`] and [`DeviceProfile`] — always available,
//! even before a single frame has run — and [`MeasuredCostModel`] wraps it,
//! overriding the heuristic with an exponential moving average of real
//! measured durations once a pass (identified by name) has actually run at
//! least once.

use std::collections::HashMap;

use vieww_render_graph::{PassDesc, PassKind};
use vieww_scene::Affinity;

use crate::capabilities::DeviceProfile;

/// Which executor a pass could run on. The planner's actual placement
/// decision is [`crate::policy::Placement`]; this is just "how long would
/// each option take."
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecutorEstimate {
    pub cpu_ms: f32,
    /// `None` when the device has no GPU at all — not "very expensive," a
    /// genuinely unavailable option so [`crate::policy`] never accidentally
    /// picks it.
    pub gpu_ms: Option<f32>,
}

/// Something that can estimate a pass's cost. A trait so
/// [`HeuristicCostModel`] and [`MeasuredCostModel`] are interchangeable
/// wherever `planner::Planner` needs an estimate.
pub trait CostModel {
    fn estimate(&self, pass: &PassDesc, pixels: f32, device: &DeviceProfile) -> ExecutorEstimate;
}

/// A pure, closed-form estimate: pixels touched divided by the device's
/// measured-elsewhere throughput number, plus GPU dispatch overhead.
///
/// This is deliberately simple — a real backend's actual cost has cache
/// effects, driver overhead variance, and thermal throttling this can never
/// capture — which is exactly why [`MeasuredCostModel`] exists to correct
/// it once real numbers are available. A heuristic that tried to be more
/// "realistic" without real measurements behind it would just be
/// confidently wrong in a different way.
#[derive(Debug, Default)]
pub struct HeuristicCostModel;

impl CostModel for HeuristicCostModel {
    fn estimate(&self, pass: &PassDesc, pixels: f32, device: &DeviceProfile) -> ExecutorEstimate {
        let mpix = (pixels / 1_000_000.0).max(0.000_1);
        // The pass's own hint carries `cpu_complexity` (×100, see
        // `vieww_scene::CostHint`): strokes, curves and shadows cost the
        // scanline rasterizer 2–2.7× a plain fill of the same area —
        // measured on the gallery's shadow log, 2026-10-15 — and an
        // area-only model priced them identically, which is why every
        // close CPU/GPU call on a strokes-heavy scene was wrong. The GPU
        // side gets no per-command term: its dominant error (8.4× on the
        // reference laptop, per `Calibration`) is a per-device constant
        // the calibration corrects, not a shape effect, and inventing a
        // shape term here would double-count a guess.
        let complexity = f32::from(pass.cost.cpu_complexity.max(100)) / 100.0;
        let cpu_ms = 1000.0 * mpix / device.cpu_fill_mpix_per_sec.max(0.001) * complexity;

        let gpu_ms = if device.has_gpu() {
            let raw = 1000.0 * mpix / device.gpu_fill_mpix_per_sec.max(0.001);
            // A filter/compute-shaped pass amortizes dispatch overhead
            // across more work than a trivial raster pass does, but every
            // GPU pass pays it at least once.
            let overhead = device.gpu_dispatch_overhead_ms
                * match pass.kind {
                    PassKind::Filter => 1.0,
                    _ => 1.0,
                };
            Some(raw + overhead)
        } else {
            None
        };

        ExecutorEstimate { cpu_ms, gpu_ms }
    }
}

/// A key identifying "the same pass" across frames, for measurement
/// history. Passes don't have stable ids across `Graph`s (a `PassId` is
/// only meaningful within the graph it came from), so history is keyed by
/// name — a real integration would want a more precise key (name plus a
/// content hash of what it reads/writes), left as an extension point via
/// `MeasuredCostModel::record`'s generic key type.
pub type MeasurementKey = &'static str;

/// Wraps a [`HeuristicCostModel`] with a measured-cost override, updated
/// via an exponential moving average so one outlier frame (a GC pause, a
/// thermal spike) does not swing the estimate wildly.
///
/// CPU measurements arrive in one of two shapes, because the two callers
/// that exist measure different things:
///
/// * an **absolute duration** ([`Self::record_cpu`]) — the whole frame took
///   this long — meaningful only while the frame shape is stable (the live
///   loop's own convention, the same one the GPU buckets live by);
/// * a **rate** ([`Self::record_cpu_rate`]) — this work took this long over
///   this many pixels — which scales with the pass's own size, the property
///   a rasterizer's cost actually has and an absolute history cannot
///   express. The startup micro-benchmark feeds this shape (FW-22: its
///   fill rate is the measured model's first CPU number), and so does any
///   caller that knows the pixels it timed.
///
/// A recorded rate takes precedence over an absolute history for the same
/// key: a rate is strictly more information.
#[derive(Debug)]
pub struct MeasuredCostModel<M: CostModel = HeuristicCostModel> {
    fallback: M,
    /// EMA smoothing factor: how much weight the newest sample gets.
    alpha: f32,
    cpu_history: HashMap<MeasurementKey, f32>,
    /// Measured CPU **rates**, in Mpix/s, by key — see the type's doc.
    cpu_rates: HashMap<MeasurementKey, f32>,
    gpu_history: HashMap<MeasurementKey, f32>,
}

impl MeasuredCostModel<HeuristicCostModel> {
    #[must_use]
    pub fn new() -> Self {
        Self::with_fallback(HeuristicCostModel, 0.2)
    }
}

impl Default for MeasuredCostModel<HeuristicCostModel> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: CostModel> MeasuredCostModel<M> {
    #[must_use]
    pub fn with_fallback(fallback: M, alpha: f32) -> Self {
        Self {
            fallback,
            alpha: alpha.clamp(0.0, 1.0),
            cpu_history: HashMap::new(),
            cpu_rates: HashMap::new(),
            gpu_history: HashMap::new(),
        }
    }

    /// Record a real, measured duration for a pass that actually ran on
    /// `executor`. Subsequent [`CostModel::estimate`] calls for the same
    /// key use an EMA of every recorded sample instead of the heuristic.
    pub fn record_cpu(&mut self, key: MeasurementKey, measured_ms: f32) {
        record(&mut self.cpu_history, key, measured_ms, self.alpha);
    }

    pub fn record_gpu(&mut self, key: MeasurementKey, measured_ms: f32) {
        record(&mut self.gpu_history, key, measured_ms, self.alpha);
    }

    /// Record a real, measured CPU **rate**: `measured_ms` for `pixels`
    /// pixels of work under `key`, smoothed as an EMA over the rate itself
    /// (Mpix/s). Unlike [`Self::record_cpu`]'s absolute duration, a rate
    /// scales with the pass's own pixel count — the shape a rasterizer's
    /// cost has — so it is the honest form for a caller that knows the area
    /// it timed: the startup micro-benchmark (FW-22) and any per-frame CPU
    /// measurement that carries its frame size.
    ///
    /// A rate deliberately does **not** stack the heuristic's
    /// `cpu_complexity` term on top: the measurement already contains
    /// whatever work was timed under that key, and the caller owns which
    /// kind of work that was — the same contract the GPU buckets live by.
    /// Nothing is recorded for a non-finite, non-positive sample.
    pub fn record_cpu_rate(&mut self, key: MeasurementKey, measured_ms: f32, pixels: f32) {
        if !(measured_ms.is_finite() && measured_ms > 0.0 && pixels.is_finite() && pixels > 0.0) {
            return;
        }
        let rate = (pixels / 1_000_000.0) / (measured_ms / 1000.0);
        if rate.is_finite() && rate > 0.0 {
            record(&mut self.cpu_rates, key, rate, self.alpha);
        }
    }

    /// The measured CPU rate recorded for `key`, in Mpix/s — `None` until
    /// [`Self::record_cpu_rate`] has a sample for it. The read the
    /// calibration loop's receipts and tests use; estimates go through
    /// [`Self::estimate_keyed`].
    #[must_use]
    pub fn cpu_rate(&self, key: MeasurementKey) -> Option<f32> {
        self.cpu_rates.get(key).copied()
    }

    /// Estimate using measurement history keyed by `key`, falling back to
    /// the wrapped heuristic model per-executor where no history exists
    /// yet.
    #[must_use]
    pub fn estimate_keyed(
        &self,
        key: MeasurementKey,
        pass: &PassDesc,
        pixels: f32,
        device: &DeviceProfile,
    ) -> ExecutorEstimate {
        let heuristic = self.fallback.estimate(pass, pixels, device);
        let mpix = (pixels / 1_000_000.0).max(0.000_1);
        // A measured rate wins over an absolute history (more
        // information), and both win over the heuristic.
        let cpu_ms = self
            .cpu_rates
            .get(key)
            .map(|rate| 1000.0 * mpix / rate.max(0.001))
            .or_else(|| self.cpu_history.get(key).copied())
            .unwrap_or(heuristic.cpu_ms);
        ExecutorEstimate {
            cpu_ms,
            gpu_ms: heuristic
                .gpu_ms
                .map(|h| self.gpu_history.get(key).copied().unwrap_or(h)),
        }
    }
}

fn record(
    history: &mut HashMap<MeasurementKey, f32>,
    key: MeasurementKey,
    sample: f32,
    alpha: f32,
) {
    history
        .entry(key)
        .and_modify(|ema| *ema = alpha * sample + (1.0 - alpha) * *ema)
        .or_insert(sample);
}

impl<M: CostModel> CostModel for MeasuredCostModel<M> {
    fn estimate(&self, pass: &PassDesc, pixels: f32, device: &DeviceProfile) -> ExecutorEstimate {
        self.estimate_keyed(pass.name, pass, pixels, device)
    }
}

/// Whether a pass's [`vieww_scene::Affinity`] rules an executor out
/// entirely, independent of cost — a `GpuOnly` pass with no GPU present
/// still has to run *somewhere*, so this is advisory for CPU (never
/// actually forbidden) and load-bearing only in that it biases
/// [`crate::policy`] strongly toward the affinity's preferred side.
#[must_use]
pub fn affinity_bias(affinity: Affinity) -> f32 {
    match affinity {
        Affinity::Cpu => -1.0,
        Affinity::Balanced => 0.0,
        Affinity::Gpu => 0.6,
        Affinity::GpuOnly => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_render_graph::{PassDesc, PassKind};
    use vieww_scene::CostHint;

    fn pass() -> PassDesc {
        PassDesc::new("p", PassKind::Raster, CostHint::CONSERVATIVE)
    }

    #[test]
    fn a_strokes_heavy_pass_costs_more_than_a_fill_of_the_same_area() {
        // The per-command complexity weight: same pixels, same device, the
        // only difference is what the pass is made of. Without the weight
        // the two estimates were identical, and every CPU/GPU close call
        // on a strokes-heavy scene was priced by area alone.
        let stroke_pass = PassDesc::new(
            "strokes",
            PassKind::Raster,
            CostHint {
                gpu_affinity: Affinity::Balanced,
                cpu_complexity: vieww_scene::STROKE_COMPLEXITY,
                ..CostHint::CONSERVATIVE
            },
        );
        let fill_pass = PassDesc::new("fills", PassKind::Raster, CostHint::CONSERVATIVE);
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let stroke = HeuristicCostModel.estimate(&stroke_pass, 1_000_000.0, &device);
        let fill = HeuristicCostModel.estimate(&fill_pass, 1_000_000.0, &device);
        assert!(
            (stroke.cpu_ms / fill.cpu_ms - 2.7).abs() < 0.01,
            "{stroke:?} vs {fill:?}"
        );
        // The GPU side is deliberately untouched by the shape term.
        assert_eq!(stroke.gpu_ms, fill.gpu_ms);
    }

    #[test]
    fn a_device_with_no_gpu_reports_no_gpu_estimate() {
        let est =
            HeuristicCostModel.estimate(&pass(), 1_000_000.0, &DeviceProfile::CPU_ONLY_FALLBACK);
        assert!(est.gpu_ms.is_none());
    }

    #[test]
    fn a_discrete_gpu_is_faster_per_pixel_than_cpu_at_scale() {
        let est =
            HeuristicCostModel.estimate(&pass(), 100_000_000.0, &DeviceProfile::DESKTOP_DISCRETE);
        assert!(est.gpu_ms.unwrap() < est.cpu_ms);
    }

    #[test]
    fn measured_history_overrides_the_heuristic() {
        let mut model = MeasuredCostModel::new();
        let device = DeviceProfile::DESKTOP_DISCRETE;
        let before = model.estimate_keyed("my_pass", &pass(), 1_000_000.0, &device);
        model.record_cpu("my_pass", 0.001);
        let after = model.estimate_keyed("my_pass", &pass(), 1_000_000.0, &device);
        assert_ne!(before.cpu_ms, after.cpu_ms);
    }

    #[test]
    fn ema_smooths_toward_the_new_sample_without_jumping_all_the_way() {
        let mut model = MeasuredCostModel::with_fallback(HeuristicCostModel, 0.5);
        model.record_cpu("p", 10.0);
        model.record_cpu("p", 20.0);
        // 0.5*10 + 0.5*(prior 10) = 10 first time (inserted directly), then
        // 0.5*20 + 0.5*10 = 15 second time.
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let est = model.estimate_keyed("p", &pass(), 1_000_000.0, &device);
        assert!((est.cpu_ms - 15.0).abs() < 1e-6);
    }

    #[test]
    fn a_measured_rate_scales_with_the_pass_size() {
        // FW-22: the micro-benchmark's rate is the measured model's CPU
        // number, and it must price a 4 Mpix pass at four times a 1 Mpix
        // pass — the property an absolute-duration history cannot express
        // and the reason the rate shape exists.
        let mut model = MeasuredCostModel::new();
        // 2 ms per Mpix = 500 Mpix/s.
        model.record_cpu_rate("draw", 2.0, 1_000_000.0);
        assert!((model.cpu_rate("draw").unwrap() - 500.0).abs() < 1e-3);
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let one = model.estimate_keyed("draw", &pass(), 1_000_000.0, &device);
        let four = model.estimate_keyed("draw", &pass(), 4_000_000.0, &device);
        assert!((one.cpu_ms - 2.0).abs() < 1e-3, "{:?}", one);
        assert!((four.cpu_ms - 8.0).abs() < 1e-3, "{:?}", four);
        // Another key keeps the heuristic.
        let other = model.estimate_keyed("layers", &pass(), 1_000_000.0, &device);
        let heuristic = HeuristicCostModel.estimate(&pass(), 1_000_000.0, &device);
        assert!((other.cpu_ms - heuristic.cpu_ms).abs() < 1e-6);
    }

    #[test]
    fn a_rate_wins_over_an_absolute_history_and_bad_samples_never_land() {
        let mut model = MeasuredCostModel::with_fallback(HeuristicCostModel, 0.5);
        model.record_cpu("draw", 99.0);
        model.record_cpu_rate("draw", 1.0, 1_000_000.0);
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let est = model.estimate_keyed("draw", &pass(), 1_000_000.0, &device);
        assert!((est.cpu_ms - 1.0).abs() < 1e-3, "the rate wins: {est:?}");
        // Non-finite, zero and negative inputs record nothing at all.
        model.record_cpu_rate("bad", f32::NAN, 1_000_000.0);
        model.record_cpu_rate("bad", 0.0, 1_000_000.0);
        model.record_cpu_rate("bad", -1.0, 1_000_000.0);
        model.record_cpu_rate("bad", 1.0, 0.0);
        assert!(model.cpu_rate("bad").is_none());
        // The rate's own EMA smooths like every other history: a second
        // sample at twice the rate moves it halfway (1000 -> 2000, alpha
        // 0.5, lands at 1500).
        model.record_cpu_rate("draw", 1.0, 2_000_000.0); // 2000 Mpix/s
        let smoothed = model.cpu_rate("draw").unwrap();
        assert!((smoothed - 1500.0).abs() < 1.0, "{smoothed}");
    }
}
