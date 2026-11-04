//! Shadow mode: the planner runs beside the real renderer, every frame, and
//! changes nothing.
//!
//! # Why this exists
//!
//! [`crate::plan_frame`] has been able to place every pass on the CPU, the GPU
//! or both since this crate was written, and no frame has ever been drawn
//! the way it said. The windowed path (`vieww-platform-winit`'s
//! `present_planned`) makes a different, simpler decision — the whole frame on
//! the GPU when `vieww_gpu::ScenePlan::is_complete`, otherwise the whole frame
//! on the CPU — and the planner's opinion of that frame is never even
//! computed. Which means nobody knows whether the planner's opinion is any
//! good: its throughputs ([`DeviceProfile`]) are round numbers, its 250,000-
//! pixel hybrid threshold is a guess, and making it authoritative today would
//! be putting a guess in charge of every frame.
//!
//! The audit's answer, and this module: **compute the plan, run the frame the
//! old way, and write down both.** [`ShadowPlanner::plan`] builds the render
//! graph for the frame and plans it; the caller renders exactly as before and
//! reports what actually ran and how long it took through
//! [`ShadowPlanner::observe`]; the result is a [`ShadowFrame`] — one JSON line
//! — saying what the planner would have done, what it predicted, and what the
//! real path measured.
//!
//! Two things fall out of a run of those lines:
//!
//! * **Calibration.** [`Calibration`] compares the heuristic's prediction for
//!   the executor that *actually ran* against the measured time, and gives the
//!   throughput the device profile would need for the heuristic to have been
//!   right ([`Calibration::calibrated`]). That is the "verify the planner's
//!   pixel-count, dispatch-overhead, CPU/GPU timing assumptions on actual
//!   devices" step, as a number per device rather than as a task.
//! * **Disagreement.** Every frame where the planner would not have done what
//!   the live path did is counted, with the planner's predicted saving. That
//!   is the evidence an authoritative rollout needs before it is switched on.
//!
//! # What shadow mode deliberately does not do
//!
//! * It does **not** feed measurements back into the cost model it predicts
//!   with. A self-correcting model would converge on whatever the live path
//!   does and report perfect agreement — the opposite of what a shadow run is
//!   for. Predictions always come from the uncorrected [`HeuristicCostModel`];
//!   correction is reported separately, by [`Calibration`].
//! * It does **not** claim per-pass actuals. The live CPU path rasterises the
//!   whole scene in one call and the GPU path submits one command buffer, so
//!   the only honest measurement is per frame. Per-pass predictions are
//!   recorded so that, when a backend can time passes, the comparison is
//!   ready; until then the frame is the unit.
//! * It is **not free.** Planning builds a scene graph and a render graph per
//!   frame and allocates to do it, so it must stay opt-in: the zero
//!   steady-state allocation gate applies to the frame path with shadow mode
//!   off.

use std::collections::VecDeque;
use std::fmt::Write as _;

use vieww_paint::Scene;
use vieww_render_graph::{PassDesc, PassKind};
use vieww_scene::SceneGraph;

use crate::capabilities::DeviceProfile;
use crate::cost::{CostModel, HeuristicCostModel, MeasuredCostModel};
use crate::frame_budget::FrameBudget;
use crate::planner::{plan_frame, work_pixels, FramePlan};
use crate::policy::Placement;

/// Which executor actually rendered a frame on the live path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Executor {
    Cpu,
    Gpu,
    /// Both, in one frame: the split-frame path — some of the frame's rows
    /// from the CPU rasterizer, the rest from the GPU, merged — which is
    /// what a `Mixed` recommendation asked for and, since the split landed,
    /// what the live path can actually do.
    Mixed,
}

impl Executor {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Mixed => "mixed",
        }
    }
}

/// What the planner, taken as a whole, recommends for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recommendation {
    /// Every pass on the CPU.
    Cpu,
    /// Every pass on the GPU.
    Gpu,
    /// Some passes on each side, or at least one pass split — something the
    /// live path cannot do today, which is the point of the hybrid engine.
    Mixed,
}

impl Recommendation {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Mixed => "mixed",
        }
    }

    /// Whether running the frame on `executor` is what this recommendation
    /// asked for. A `Mixed` recommendation agrees only with a split frame —
    /// which, since the split-frame path landed, is an executor the live
    /// path can actually be.
    #[must_use]
    pub const fn agrees_with(self, executor: Executor) -> bool {
        matches!(
            (self, executor),
            (Self::Cpu, Executor::Cpu)
                | (Self::Gpu, Executor::Gpu)
                | (Self::Mixed, Executor::Mixed)
        )
    }
}

/// One pass of a planned frame, as the planner saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct PassSummary {
    pub name: &'static str,
    pub kind: PassKind,
    pub pixels: f32,
    pub placement: Placement,
    pub cpu_ms: f32,
    pub gpu_ms: Option<f32>,
}

/// The planner's opinion of one frame, before it has been rendered.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowPlan {
    pub frame_plan: FramePlan,
    /// In execution order.
    pub passes: Vec<PassSummary>,
    /// Predicted time if every pass ran on the CPU — what the heuristic says
    /// the live CPU fallback costs.
    pub all_cpu_ms: f32,
    /// Predicted time if every pass ran on the GPU — what the heuristic says
    /// the live GPU path costs. `None` on a device profile with no GPU.
    pub all_gpu_ms: Option<f32>,
    /// Pixels of work, summed over passes ([`work_pixels`]): layers are
    /// bounds-sized and composites are charged their layer, so this counts
    /// real work, not passes × screens.
    pub pixels: f32,
}

impl ShadowPlan {
    #[must_use]
    pub fn recommendation(&self) -> Recommendation {
        let mut cpu = false;
        let mut gpu = false;
        for pass in &self.passes {
            match pass.placement {
                Placement::Cpu => cpu = true,
                Placement::Gpu => gpu = true,
                Placement::Hybrid(_) => return Recommendation::Mixed,
            }
        }
        match (cpu, gpu) {
            (_, false) => Recommendation::Cpu,
            (false, true) => Recommendation::Gpu,
            (true, true) => Recommendation::Mixed,
        }
    }

    /// What the heuristic predicted for the executor that actually ran.
    #[must_use]
    pub fn predicted_for(&self, executor: Executor) -> Option<f32> {
        match executor {
            Executor::Cpu => Some(self.all_cpu_ms),
            Executor::Gpu => self.all_gpu_ms,
            // The plan's own mixed prediction: the per-pass costs under the
            // placements it chose, summed — the number a split frame's
            // wall time is held against.
            Executor::Mixed => Some(self.frame_plan.predicted_ms),
        }
    }

    /// The CPU's share of a mixed frame's predicted work, as a fraction in
    /// `[0.05, 0.95]` — every pass weighted by its own predicted duration
    /// under the placement it received (a `Hybrid` pass counts its split,
    /// not as a whole unit either way). This is the number the live path's
    /// split-frame execution partitions the frame's rows by: both sides
    /// render the same scene, and this decides whose pixels each row uses.
    #[must_use]
    pub fn mixed_cpu_fraction(&self) -> f32 {
        let mut cpu = 0.0;
        let mut total = 0.0;
        for pass in &self.passes {
            let gpu_ms = pass.gpu_ms.unwrap_or(pass.cpu_ms);
            let ms = match pass.placement {
                Placement::Cpu => pass.cpu_ms,
                Placement::Gpu => gpu_ms,
                Placement::Hybrid(split) => {
                    (split.cpu_fraction * pass.cpu_ms).max(split.gpu_fraction() * gpu_ms)
                }
            };
            // `Placement::gpu_fraction`'s complement is the CPU share —
            // a `Hybrid` pass contributes its own split, not a whole unit.
            cpu += ms * (1.0 - pass.placement.gpu_fraction());
            total += ms;
        }
        if total > 0.0 {
            (cpu / total).clamp(0.05, 0.95)
        } else {
            0.5
        }
    }

    fn count(&self, want: fn(&Placement) -> bool) -> usize {
        self.passes.iter().filter(|p| want(&p.placement)).count()
    }
}

/// One shadow-mode frame: the plan, and what actually happened.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowFrame {
    pub frame: u64,
    /// Free text from the caller — a fixture name, a window title.
    pub label: String,
    pub width: u32,
    pub height: u32,
    /// Which profile the predictions were made with — the mode machine's
    /// state when the frame was planned. A log's frames can span a promotion
    /// (`Shadow` → `ShadowCalibrated` → `Authoritative`), and every number
    /// after the mode switch means something different from every number
    /// before it; without this field on the line, an analysis cannot tell
    /// the raw model's error from the calibrated model's, or an
    /// authoritative decision's provenance from a shadow-only opinion.
    pub mode: PlannerMode,
    /// The planning profile's CPU fill throughput when this frame was
    /// planned — the number every CPU prediction on this line divided by.
    /// `mode` names the profile's provenance; this pair of fields *is* the
    /// profile, so a receipt states the correction it is looking at rather
    /// than inferring it from the mode tag: after a promotion the number
    /// here is the calibrated one (FW-22 — receipts record the calibrated
    /// profile, not just the fact that some profile was calibrated).
    pub cpu_fill_mpix_per_sec: f32,
    /// The planning profile's GPU fill throughput, same provenance. Never
    /// null: a profile always carries the number, and a device without a
    /// GPU simply never has a prediction that uses it.
    pub gpu_fill_mpix_per_sec: f32,
    pub plan: ShadowPlan,
    pub actual: Executor,
    pub actual_ms: f32,
    /// Why the live path took the executor it did, when that was not a free
    /// choice — `"plan incomplete: Text"`, `"no gpu"`. `None` when the live
    /// path simply took its preferred route.
    pub fallback: Option<String>,
}

impl ShadowFrame {
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.plan.recommendation().agrees_with(self.actual)
    }

    /// Measured over predicted, for the executor that ran. Above 1.0 the
    /// heuristic is optimistic about this device; below, pessimistic.
    #[must_use]
    pub fn error_ratio(&self) -> Option<f32> {
        self.plan
            .predicted_for(self.actual)
            .filter(|p| *p > 0.0)
            .map(|p| self.actual_ms / p)
    }

    /// One line of JSON, no trailing newline. Hand-written: this crate takes
    /// no serialisation dependency for one record type.
    #[must_use]
    pub fn to_json_line(&self) -> String {
        let mut s = String::with_capacity(256 + 96 * self.plan.passes.len());
        let opt = |v: Option<f32>| v.map_or_else(|| "null".to_owned(), fmt_f32);
        let _ = write!(
            s,
            "{{\"frame\":{},\"mode\":\"{}\",\
             \"cpu_fill_mpix_per_sec\":{},\"gpu_fill_mpix_per_sec\":{},\
             \"label\":{},\"width\":{},\"height\":{},\
             \"actual\":\"{}\",\"actual_ms\":{},\"fallback\":{},\
             \"recommendation\":\"{}\",\"agrees\":{},\
             \"predicted_plan_ms\":{},\"predicted_all_cpu_ms\":{},\"predicted_all_gpu_ms\":{},\
             \"predicted_actual_ms\":{},\"error_ratio\":{},\"pixels\":{},\
             \"passes_cpu\":{},\"passes_gpu\":{},\"passes_hybrid\":{},\"passes\":[",
            self.frame,
            self.mode.name(),
            fmt_f32(self.cpu_fill_mpix_per_sec),
            fmt_f32(self.gpu_fill_mpix_per_sec),
            json_str(&self.label),
            self.width,
            self.height,
            self.actual.name(),
            fmt_f32(self.actual_ms),
            self.fallback
                .as_deref()
                .map_or_else(|| "null".to_owned(), json_str),
            self.plan.recommendation().name(),
            self.agrees(),
            fmt_f32(self.plan.frame_plan.predicted_ms),
            fmt_f32(self.plan.all_cpu_ms),
            opt(self.plan.all_gpu_ms),
            opt(self.plan.predicted_for(self.actual)),
            opt(self.error_ratio()),
            fmt_f32(self.plan.pixels),
            self.plan.count(|p| matches!(p, Placement::Cpu)),
            self.plan.count(|p| matches!(p, Placement::Gpu)),
            self.plan.count(|p| matches!(p, Placement::Hybrid(_))),
        );
        for (i, pass) in self.plan.passes.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let placement = match pass.placement {
                Placement::Cpu => "\"cpu\"".to_owned(),
                Placement::Gpu => "\"gpu\"".to_owned(),
                Placement::Hybrid(split) => {
                    format!(
                        "{{\"hybrid_cpu_fraction\":{}}}",
                        fmt_f32(split.cpu_fraction)
                    )
                }
            };
            let _ = write!(
                s,
                "{{\"name\":\"{}\",\"kind\":\"{:?}\",\"pixels\":{},\"placement\":{},\
                 \"cpu_ms\":{},\"gpu_ms\":{}}}",
                pass.name,
                pass.kind,
                fmt_f32(pass.pixels),
                placement,
                fmt_f32(pass.cpu_ms),
                opt(pass.gpu_ms),
            );
        }
        s.push_str("]}");
        s
    }
}

fn fmt_f32(v: f32) -> String {
    if v.is_finite() {
        // Four decimals: a microsecond-and-a-bit at millisecond scale, and
        // enough for a fraction.
        let s = format!("{v:.4}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s.is_empty() || s == "-" {
            "0".to_owned()
        } else {
            s.to_owned()
        }
    } else {
        "null".to_owned()
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The measured-cost keys: the vocabulary the only backend that can time its
/// own work reports — `vieww-hal`'s Vulkan timestamp queries, its
/// `GpuPassTimes`' five fields. These are
/// *kind* keys, not pass names: the planner's graph calls a pass
/// `root_raster` while the backend reports "here is how long drawing took",
/// and pretending they share a vocabulary would be a mapping pretending to
/// be a measurement. [`measured_kind`] translates the graph's
/// [`PassKind`]s into this vocabulary; a kind the backend has never reported
/// simply has no measured history and the heuristic estimate stands.
pub mod measured_kinds {
    pub const DRAW: &str = "draw";
    pub const LAYERS: &str = "layers";
    pub const FILTERS: &str = "filters";
    pub const SHADOWS: &str = "shadows";
    pub const BLIT: &str = "blit";
}

/// The measured-cost key for a pass of this kind — [`measured_kinds`]'
/// vocabulary. [`PassKind::Raster`] work is the backend's `draw`, a
/// composite is its `layers`, a filter pass is `filters`, and the present
/// blit is `blit`. Nothing maps to `shadows`: the graph prices a box shadow
/// inside the raster or filter pass that draws it, so the backend's
/// dedicated shadow bucket is recorded (it is real data) but never
/// consulted by a pass estimate.
#[must_use]
pub fn measured_kind(kind: PassKind) -> &'static str {
    match kind {
        PassKind::Raster => measured_kinds::DRAW,
        PassKind::Filter => measured_kinds::FILTERS,
        PassKind::Composite => measured_kinds::LAYERS,
        PassKind::Blit => measured_kinds::BLIT,
    }
}

/// The most-recent window of `(predicted, actual)` frame times per
/// executor — the audit's ring buffer, and the fresh twin of
/// [`Calibration`]'s lifetime accumulators.
///
/// `Calibration` answers "has this model ever been good enough to act on"
/// and its answer only ever improves: a gate that cleared at frame 2,000
/// stays cleared at frame 200,000 even if the device's behavior has changed
/// underneath it (thermal throttling, a power cable pulled, a window dragged
/// to a busier GPU). The ring answers "has it been good *lately*": a bounded
/// most-recent window whose error can get worse, which is exactly what a
/// machine that has changed since warmup needs before its opinions are
/// still obeyed.
#[derive(Debug, Clone)]
pub struct TimingRing {
    capacity: usize,
    cpu: VecDeque<(f32, f32)>,
    gpu: VecDeque<(f32, f32)>,
    mixed: VecDeque<(f32, f32)>,
}

impl TimingRing {
    /// A ring holding the last `capacity` frames per executor. Two hundred
    /// and forty is about four seconds at 60 Hz: long enough that one bad
    /// frame cannot flip the reading, short enough that "lately" still means
    /// something a user could have watched.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            cpu: VecDeque::new(),
            gpu: VecDeque::new(),
            mixed: VecDeque::new(),
        }
    }

    fn queue(&mut self, executor: Executor) -> &mut VecDeque<(f32, f32)> {
        match executor {
            Executor::Cpu => &mut self.cpu,
            Executor::Gpu => &mut self.gpu,
            Executor::Mixed => &mut self.mixed,
        }
    }

    fn read(&self, executor: Executor) -> &VecDeque<(f32, f32)> {
        match executor {
            Executor::Cpu => &self.cpu,
            Executor::Gpu => &self.gpu,
            Executor::Mixed => &self.mixed,
        }
    }

    /// Record one frame's prediction and measurement for `executor`.
    pub fn record(&mut self, executor: Executor, predicted_ms: f32, actual_ms: f32) {
        if !(predicted_ms.is_finite() && actual_ms.is_finite()) {
            return;
        }
        let capacity = self.capacity;
        let queue = self.queue(executor);
        if queue.len() == capacity {
            queue.pop_front();
        }
        queue.push_back((predicted_ms, actual_ms));
    }

    /// The recent-window error reading for `executor`: how many frames the
    /// window holds, the ratio of measured-to-predicted totals over the
    /// window, and the root-mean-square of the per-frame ratios — the same
    /// two numbers [`Calibration`] tracks over a whole run, recomputed over
    /// the window that can get worse. `None` until the window has frames
    /// with a positive prediction.
    #[must_use]
    pub fn recent_error(&self, executor: Executor) -> Option<RecentError> {
        let window = self.read(executor);
        let n = window.len();
        if n == 0 {
            return None;
        }
        let predicted: f64 = window.iter().map(|(p, _)| f64::from(*p)).sum();
        let actual: f64 = window.iter().map(|(_, a)| f64::from(*a)).sum();
        if predicted <= 0.0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a frame count is far below f64's exact range"
        )]
        let nf = n as f64;
        let (sum, sq_sum) =
            window
                .iter()
                .filter(|(p, _)| *p > 0.0)
                .fold((0.0_f64, 0.0_f64), |(s, q), (p, a)| {
                    let r = f64::from(*a) / f64::from(*p);
                    (s + r, q + r * r)
                });
        let mean = sum / nf;
        let mean_sq = sq_sum / nf;
        let rmse = (mean_sq - 2.0 * mean + 1.0).max(0.0).sqrt();
        Some(RecentError {
            samples: n,
            ratio_of_sums: actual / predicted,
            rmse_of_ratios: rmse,
        })
    }
}

/// [`TimingRing`]'s windowed reading — see [`TimingRing::recent_error`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecentError {
    pub samples: usize,
    /// Measured over predicted, summed over the window: how *far* off the
    /// model is on this device right now.
    pub ratio_of_sums: f64,
    /// Root-mean-square of the per-frame ratios: how *consistently* far off
    /// — the number the authoritative gate is stated on, over the window
    /// that can drift.
    pub rmse_of_ratios: f64,
}

/// Running comparison of predicted and measured frame time, per executor.
///
/// Two readings live here and both are kept because they answer different
/// questions. The **ratio of sums** (`cpu_error_ratio`) is "how wrong is
/// the model in total over this run" — the number `calibrated` needs, and
/// the one a device profile correction wants. The **per-frame ratio
/// distribution** (`rmse_of_ratio`) is "how consistent is that wrongness"
/// — a model whose error is a steady 8.4× is fully correctable (multiply
/// back and the residual is noise), while one whose error swings 1×–20×
/// frame to frame cannot be trusted at any scale. The authoritative gate
/// ("ready") is stated on the second, because acting on a frame is
/// exactly where consistency, not the total, is what protects you.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Calibration {
    cpu_predicted: f64,
    cpu_actual: f64,
    cpu_frames: u64,
    cpu_ratio_sum: f64,
    cpu_ratio_sq_sum: f64,
    gpu_predicted: f64,
    gpu_actual: f64,
    gpu_overhead: f64,
    gpu_frames: u64,
    gpu_ratio_sum: f64,
    gpu_ratio_sq_sum: f64,
    agreeing_frames: u64,
}

impl Calibration {
    fn record(&mut self, frame: &ShadowFrame, device: &DeviceProfile) {
        let Some(predicted) = frame.plan.predicted_for(frame.actual) else {
            return;
        };
        // A split frame feeds no bucket here: these ratios are what
        // `Calibration::calibrated` corrects the profile's whole-frame CPU
        // and GPU numbers with, and a mixed frame is neither. The ring —
        // `TimingRing`, the *recent* guard on actuation — takes mixed
        // frames; this lifetime model deliberately does not.
        if frame.actual == Executor::Mixed {
            if frame.agrees() {
                self.agreeing_frames += 1;
            }
            return;
        }
        // **Launch is not evidence** (FW-22): a frame inside the launch
        // window never touches the timing buckets this correction and the
        // gate are computed from — the phones' launch-time CPU cost sat
        // 34–38% under their steady state (PENDING §4.6: Stylo 22.1 vs
        // 35.7 ms, A73 11.0 vs 16.6), so launch frames pull the correction
        // toward numbers the machine cannot sustain once warm. Agreement
        // still counts — a decision is a decision whenever it is made — and
        // the recent ring still takes the frame (a window that washes it
        // out in `TIMING_RING_CAPACITY` frames anyway).
        let launch = frame.frame < LAUNCH_EXCLUDE_FRAMES;
        if !launch && predicted > 0.0 && frame.actual_ms.is_finite() {
            let ratio = f64::from(frame.actual_ms) / f64::from(predicted);
            if ratio.is_finite() {
                match frame.actual {
                    Executor::Cpu => {
                        self.cpu_ratio_sum += ratio;
                        self.cpu_ratio_sq_sum += ratio * ratio;
                    }
                    Executor::Gpu => {
                        self.gpu_ratio_sum += ratio;
                        self.gpu_ratio_sq_sum += ratio * ratio;
                    }
                    Executor::Mixed => {}
                }
            }
        }
        if !launch {
            match frame.actual {
                Executor::Cpu => {
                    self.cpu_predicted += f64::from(predicted);
                    self.cpu_actual += f64::from(frame.actual_ms);
                    self.cpu_frames += 1;
                }
                Executor::Gpu => {
                    self.gpu_predicted += f64::from(predicted);
                    self.gpu_actual += f64::from(frame.actual_ms);
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a frame has a handful of passes"
                    )]
                    let passes = frame.plan.passes.len() as f64;
                    self.gpu_overhead += f64::from(device.gpu_dispatch_overhead_ms) * passes;
                    self.gpu_frames += 1;
                }
                Executor::Mixed => {}
            }
        }
        if frame.agrees() {
            self.agreeing_frames += 1;
        }
    }

    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.cpu_frames + self.gpu_frames
    }

    #[must_use]
    pub const fn cpu_frames(&self) -> u64 {
        self.cpu_frames
    }

    #[must_use]
    pub const fn gpu_frames(&self) -> u64 {
        self.gpu_frames
    }

    #[must_use]
    pub const fn agreeing_frames(&self) -> u64 {
        self.agreeing_frames
    }

    /// Total measured CPU time over total predicted, across every CPU frame.
    /// A ratio of sums, not a mean of ratios, so a tiny frame with a large
    /// relative error cannot dominate.
    #[must_use]
    pub fn cpu_error_ratio(&self) -> Option<f64> {
        (self.cpu_frames > 0 && self.cpu_predicted > 0.0)
            .then(|| self.cpu_actual / self.cpu_predicted)
    }

    #[must_use]
    pub fn gpu_error_ratio(&self) -> Option<f64> {
        (self.gpu_frames > 0 && self.gpu_predicted > 0.0)
            .then(|| self.gpu_actual / self.gpu_predicted)
    }

    /// Root-mean-square of the per-frame error ratios (measured over
    /// predicted), for one executor — the *consistency* reading this
    /// struct's doc describes. `None` until that executor has frames with
    /// a positive prediction; identical to zero when every frame measured
    /// exactly what the model said.
    #[must_use]
    pub fn rmse_of_ratio(&self, executor: Executor) -> Option<f64> {
        let (frames, sum, sq_sum) = match executor {
            Executor::Cpu => (self.cpu_frames, self.cpu_ratio_sum, self.cpu_ratio_sq_sum),
            Executor::Gpu => (self.gpu_frames, self.gpu_ratio_sum, self.gpu_ratio_sq_sum),
            // No lifetime buckets for split frames — see `record`. `None`
            // says "cannot vouch", which is the conservative answer.
            Executor::Mixed => return None,
        };
        if frames == 0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "frame counts are far below f64's exact range"
        )]
        let n = frames as f64;
        // mean((r - 1)^2) = mean(r^2) - 2*mean(r) + 1, clamped against the
        // negative side the algebra allows and floating-point rounding can
        // produce when every ratio is exactly 1.
        let mean_sq = sq_sum / n;
        let mean = sum / n;
        Some((mean_sq - 2.0 * mean + 1.0).max(0.0).sqrt())
    }

    /// Whether this executor's calibrated reading is good enough to act
    /// on: at least [`AUTHORITATIVE_MIN_FRAMES`] frames whose error, as a
    /// root-mean-square of per-frame ratios, stays within
    /// [`AUTHORITATIVE_MAX_RMSE`]. This is the advisory's gate ("RMSE < 5%
    /// and samples > 1000"), and it is deliberately about the frames the
    /// planner *made its predictions with* — see [`ShadowPlanner`] for the
    /// machine that feeds it.
    #[must_use]
    pub fn ready(&self, executor: Executor) -> bool {
        if executor == Executor::Mixed {
            // A split frame leans on both halves' models: it is ready when
            // each whole-frame executor is.
            return self.ready(Executor::Cpu) && self.ready(Executor::Gpu);
        }
        let frames = match executor {
            Executor::Cpu => self.cpu_frames,
            Executor::Gpu => self.gpu_frames,
            Executor::Mixed => return false,
        };
        match self.rmse_of_ratio(executor) {
            Some(rmse) => frames >= AUTHORITATIVE_MIN_FRAMES && rmse <= AUTHORITATIVE_MAX_RMSE,
            None => false,
        }
    }

    /// `device` with its fill throughputs corrected so the heuristic would
    /// have predicted the measured totals.
    ///
    /// The CPU side is exact for the heuristic's linear model. The GPU side
    /// holds the dispatch overhead fixed and attributes the whole remaining
    /// error to fill rate, which is an approximation: separating the two
    /// needs frames of different sizes on the same device, and the overhead
    /// is reported unchanged so nobody mistakes it for a measured value.
    /// A side with no frames, or whose measured time does not exceed its
    /// assumed overhead, keeps the profile's original number.
    #[must_use]
    pub fn calibrated(&self, device: &DeviceProfile) -> DeviceProfile {
        let mut out = *device;
        if let Some(ratio) = self.cpu_error_ratio().filter(|r| *r > 0.0) {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a throughput in Mpix/s fits an f32"
            )]
            let fill = (f64::from(device.cpu_fill_mpix_per_sec) / ratio) as f32;
            out.cpu_fill_mpix_per_sec = fill;
        }
        let gpu_fill_pred = self.gpu_predicted - self.gpu_overhead;
        let gpu_fill_actual = self.gpu_actual - self.gpu_overhead;
        if self.gpu_frames > 0 && gpu_fill_pred > 0.0 && gpu_fill_actual > 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a throughput in Mpix/s fits an f32"
            )]
            let fill =
                (f64::from(device.gpu_fill_mpix_per_sec) * gpu_fill_pred / gpu_fill_actual) as f32;
            out.gpu_fill_mpix_per_sec = fill;
        }
        out
    }
}

/// The gate an authoritative rollout has to clear, as numbers rather than
/// as an argument: the model the planner would act on must predict at
/// least this many frames, with a per-frame relative-error RMSE no worse
/// than this. The advisory's figures ("RMSE < 5% and samples > 1000"),
/// adopted verbatim — the same warmup also feeds the raw-model
/// `calibrated()` correction, so the total observation budget before a
/// frame can be acted on is twice this.
pub const AUTHORITATIVE_MIN_FRAMES: u64 = 1_000;
pub const AUTHORITATIVE_MAX_RMSE: f64 = 0.05;

/// How many of a planner's first frames are **launch, not evidence** for
/// calibration (FW-22). PENDING §4.6 measured the phones' launch-time CPU
/// cost 34–38% under their steady state (Stylo 22.1 vs 35.7 ms, A73 11.0
/// vs 16.6) — the profile's round numbers and any startup benchmark both
/// inherit that bias — so a calibration fed launch frames corrects the
/// profile toward numbers the machine cannot sustain once warm. Sixty is
/// the first second at 60 Hz, the same convention the adaptive cadence
/// uses ("the first second (start-up) is not evidence").
///
/// Launch frames still count as *agreement* (a decision is a decision
/// whenever it is made) and still enter the recent ring (whose window
/// washes them out in `TIMING_RING_CAPACITY` frames); they never enter
/// the ratio buckets the profile correction and the gate are computed
/// from, and [`ShadowPlanner::note_cpu_pass_kind`] /
/// [`ShadowPlanner::note_gpu_pass_kind`] refuse their samples.
pub const LAUNCH_EXCLUDE_FRAMES: u64 = 60;

/// How many `(predicted, actual)` frames [`TimingRing`] holds per executor —
/// the audit's ring buffer's capacity. Two hundred and forty is about four
/// seconds at 60 Hz: long enough that one bad frame cannot flip the window's
/// reading, short enough that "lately" still means something a user could
/// have watched.
pub const TIMING_RING_CAPACITY: usize = 240;

/// How many recent samples the ring needs before its reading may *veto* an
/// authoritative decision ([`ShadowPlanner::plan_for_frame`]'s freshness
/// guard). A window with a handful of frames is a window that has not seen
/// enough of the present to say the model has drifted; the gate's own
/// thousand-frame requirement still stands beneath it. Thirty is a quarter
/// of the ring — half a second of frames.
pub const TIMING_RING_MIN_SAMPLES: usize = 30;

/// The [`CostModel`] [`ShadowPlanner::plan_measured`] plans through: the
/// measured history looked up by *kind of work* ([`measured_kind`]), with
/// [`MeasuredCostModel`]'s own fallback to the heuristic for every kind the
/// backend has never reported. A private adapter rather than a public one
/// because the kind-key vocabulary is this module's own contract — the
/// planner's graph speaks `PassKind`s, the measured model speaks the
/// backend's timestamp buckets, and the translation is not a detail another
/// crate should have to (or be able to) get right.
#[derive(Debug)]
struct MeasuredByKind<'a> {
    measured: &'a MeasuredCostModel<HeuristicCostModel>,
}

impl CostModel for MeasuredByKind<'_> {
    fn estimate(
        &self,
        pass: &PassDesc,
        pixels: f32,
        device: &DeviceProfile,
    ) -> crate::cost::ExecutorEstimate {
        self.measured
            .estimate_keyed(measured_kind(pass.kind), pass, pixels, device)
    }
}

/// How far the shadow planner has been allowed to go.
///
/// The machine advances in one direction only, and every step is earned
/// by data rather than by confidence:
///
/// 1. **`Shadow`** — plan, log, change nothing. The state the planner has
///    lived in since 2026-10-15, and the state it stays in unless the
///    embedding application explicitly opts in
///    (`ShadowPlanner::with_authoritative`, `VIEWW_PLANNER_AUTHORITATIVE=1`
///    on the windowed path).
/// 2. **`ShadowCalibrated`** — after [`AUTHORITATIVE_MIN_FRAMES`] observed
///    frames, the profile is replaced by `Calibration::calibrated`'s
///    correction (the Kaby Lake receipts measured the raw model at 8.4×
///    optimistic on the GPU; the correction is exactly that kind of
///    number) and a **fresh** gate `Calibration` starts filling. Still
///    changing nothing: these frames measure how good the *corrected*
///    model is, which is the question actuation actually depends on.
/// 3. **`Authoritative`** — the gate has cleared on at least one
///    executor. `authoritative_choice` may now hand the live path a
///    whole-frame executor decision, and still only for frames whose
///    recommending executor's gate reading is `ready` at that moment —
///    `Mixed` frames and unready executors fall back to the live path's
///    own `ScenePlan::is_complete` gate, which stays the per-frame
///    fallback forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerMode {
    /// Planning and logging, never acting.
    #[default]
    Shadow,
    /// Planning with the calibrated profile, logging, still never acting.
    ShadowCalibrated,
    /// The gate cleared; `authoritative_choice` may act within its own
    /// per-frame readiness checks.
    Authoritative,
}

impl PlannerMode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::ShadowCalibrated => "shadow-calibrated",
            Self::Authoritative => "authoritative",
        }
    }
}

/// The planner, run in shadow.
#[derive(Debug)]
pub struct ShadowPlanner {
    device: DeviceProfile,
    model: HeuristicCostModel,
    next_frame: u64,
    calibration: Calibration,
    /// How far the mode machine has been allowed to go.
    mode: PlannerMode,
    /// Whether the embedding application opted in to the machine running
    /// at all. `false` pins `mode` at [`PlannerMode::Shadow`] forever,
    /// which is the behavior every existing caller gets.
    authoritative_enabled: bool,
    /// The profile predictions are currently made with — `device` in
    /// shadow, `calibration.calibrated(&device)` once promoted.
    planning_device: DeviceProfile,
    /// The fresh gate reading started at promotion, fed by frames whose
    /// predictions came from the calibrated profile.
    gate: Calibration,
    /// The audit's fix, connected: an exponential moving average of *measured*
    /// GPU times by kind of work, fed from the live loop's own timestamp
    /// queries through [`Self::note_gpu_pass_kind`]. It is consulted only
    /// where its numbers are acted on — [`Self::plan_for_frame`] in
    /// [`PlannerMode::Authoritative`] — so the shadow log's predictions keep
    /// the raw model's meaning for the entire run (see the module doc's
    /// "deliberately does not do": a self-correcting *logged* model would
    /// converge on the live path and report perfect agreement, which is the
    /// opposite of what a shadow run measures).
    measured: MeasuredCostModel<HeuristicCostModel>,
    /// The most-recent window of per-frame prediction errors — the fresh
    /// twin of `gate`'s lifetime reading; see [`TimingRing`].
    ring: TimingRing,
}

impl ShadowPlanner {
    /// Plan as if running on `device`. The profile's throughputs are what the
    /// predictions are made with; [`Self::calibration`] reports how wrong
    /// they turned out to be.
    #[must_use]
    pub fn new(device: DeviceProfile) -> Self {
        Self {
            device,
            model: HeuristicCostModel,
            next_frame: 0,
            calibration: Calibration::default(),
            mode: PlannerMode::Shadow,
            authoritative_enabled: false,
            planning_device: device,
            gate: Calibration::default(),
            measured: MeasuredCostModel::new(),
            ring: TimingRing::new(TIMING_RING_CAPACITY),
        }
    }

    /// Like [`Self::new`], with the mode machine allowed to run: after
    /// enough observed frames it promotes to the calibrated profile and,
    /// once the gate clears, may hand the live path whole-frame executor
    /// decisions. Without this opt-in the planner stays in
    /// [`PlannerMode::Shadow`] no matter how much data arrives.
    #[must_use]
    pub fn with_authoritative(device: DeviceProfile) -> Self {
        let mut out = Self::new(device);
        out.authoritative_enabled = true;
        out
    }

    /// [`Self::with_authoritative`], said later: grant the mode machine its
    /// permission on a planner that already runs. Everything it has
    /// accumulated survives — the calibration, the measured history, the
    /// launch window — where replacing the planner with a fresh
    /// [`Self::with_authoritative`] would restart the launch window (another
    /// [`LAUNCH_EXCLUDE_FRAMES`] frames of refused samples), drop every
    /// measurement, and re-run the startup benchmark to get back what it
    /// just threw away. The gate still starts empty either way: it is
    /// filled by the frames *after* the grant, which is the honest reading
    /// of "may I act" — the answer is earned from now, not backdated.
    pub fn allow_authoritative(&mut self) {
        self.authoritative_enabled = true;
    }

    #[must_use]
    pub const fn device(&self) -> &DeviceProfile {
        &self.device
    }

    #[must_use]
    pub const fn calibration(&self) -> &Calibration {
        &self.calibration
    }

    /// The gate reading — predictions versus measurements *under the
    /// currently-planning profile*, filled since the last promotion.
    /// Empty and meaningless in [`PlannerMode::Shadow`].
    #[must_use]
    pub const fn gate(&self) -> &Calibration {
        &self.gate
    }

    #[must_use]
    pub const fn mode(&self) -> PlannerMode {
        self.mode
    }

    /// The most-recent prediction-error window — [`TimingRing`], the fresh
    /// twin of [`Self::gate`]'s lifetime reading.
    #[must_use]
    pub const fn ring(&self) -> &TimingRing {
        &self.ring
    }

    /// Feed one *measured* GPU time for a kind of work into the measured-cost
    /// model — the live loop's half of the connection this planner's audit
    /// asked for ("`MeasuredCostModel` exists to correct the heuristic, but
    /// the live path does not feed measurements back into planning"). The
    /// caller is the windowed path after a GPU frame, passing the backend's
    /// own timestamp-query buckets: `draw` for geometry, `layers` for
    /// composites, `filters` for blurs and colour matrices, `shadows` for
    /// box shadows, `blit` for the present blit — the
    /// [`measured_kinds`] vocabulary. Zero-duration samples are ignored: a
    /// bucket the timestamps did not touch is an absence, not a measurement.
    ///
    /// Samples inside the launch window are refused too (FW-22): the
    /// first [`LAUNCH_EXCLUDE_FRAMES`] frames run on boost clocks and cold
    /// caches, and PENDING §4.6's phones showed exactly that bias. The
    /// history only ever *refines an acted-on decision* — see
    /// [`Self::plan_for_frame`] — never the shadow log's predictions.
    pub fn note_gpu_pass_kind(&mut self, kind: crate::cost::MeasurementKey, measured_ms: f32) {
        if self.next_frame < LAUNCH_EXCLUDE_FRAMES {
            return;
        }
        if measured_ms > 0.0 && measured_ms.is_finite() {
            self.measured.record_gpu(kind, measured_ms);
        }
    }

    /// Feed one *measured* CPU time for a kind of work into the measured-cost
    /// model — the CPU half of the same connection
    /// [`Self::note_gpu_pass_kind`] wires for the GPU. `measured_ms` is how
    /// long the work took, `pixels` how many pixels it covered, and the two
    /// are recorded as a **rate** ([`MeasuredCostModel::record_cpu_rate`])
    /// so the estimate scales with the pass's own size — the CPU
    /// rasterizer's cost genuinely does, which an absolute whole-frame
    /// duration cannot express across varying frame shapes.
    ///
    /// The same launch-window refusal applies, and for the same §4.6
    /// reason. The caller is whatever renders a CPU frame and can time one
    /// bucket of it: the startup micro-benchmark seeds the `draw` rate
    /// through [`Self::seed_cpu_fill_benchmark`], a live CPU path that
    /// times its raster stage per frame would keep it warm.
    pub fn note_cpu_pass_kind(
        &mut self,
        kind: crate::cost::MeasurementKey,
        measured_ms: f32,
        pixels: f32,
    ) {
        if self.next_frame < LAUNCH_EXCLUDE_FRAMES {
            return;
        }
        self.measured.record_cpu_rate(kind, measured_ms, pixels);
    }

    /// The measured CPU rate recorded for `kind`, in Mpix/s — the read the
    /// calibration loop's tests (and a receipt that wants to say what the
    /// micro-benchmark measured) use; `None` until a sample lands.
    #[must_use]
    pub fn measured_cpu_rate(&self, kind: crate::cost::MeasurementKey) -> Option<f32> {
        self.measured.cpu_rate(kind)
    }

    /// **The calibration loop's seed step** (FW-22): run the startup CPU
    /// fill micro-benchmark (`probe`'s, behind this crate's `calibrate`
    /// feature) and feed *both halves* with its number —
    ///
    /// * the planning profile's `cpu_fill_mpix_per_sec` (the heuristic's
    ///   divisor for every CPU estimate), and
    /// * the measured model's `draw` rate (the number the acted-on plan
    ///   prices raster work at, [`Self::note_cpu_pass_kind`]'s own unit).
    ///
    /// The scheduler's core count is filled too, unconditionally — the same
    /// contract `probe::calibrate_cpu_fill` keeps — so a debug build (where
    /// the benchmark itself refuses to run) still upgrades the profile's
    /// core guess.
    ///
    /// Returns the measured rate when the benchmark ran and was
    /// plausible, `None` when it kept the profile's guess (a debug build,
    /// a rasterizer error, an implausible sample — see `probe`'s docs for
    /// each). The benchmark is a **launch-time** measurement, which is
    /// precisely why the loop does not stop here: §4.6's phones sat 34–38%
    /// under their steady state at launch, so the steady frames past
    /// [`LAUNCH_EXCLUDE_FRAMES`] — through [`Self::observe`] and the
    /// per-kind notes — are what the profile correction and the measured
    /// history actually trust.
    #[cfg(feature = "calibrate")]
    pub fn seed_cpu_fill_benchmark(&mut self) -> Option<f32> {
        let cores = crate::probe::detect_cpu_cores();
        self.device.cpu_cores = cores;
        self.planning_device.cpu_cores = cores;
        let rate = crate::probe::measure_fill_rate();
        if let Some(rate) = rate {
            self.device.cpu_fill_mpix_per_sec = rate;
            self.planning_device.cpu_fill_mpix_per_sec = rate;
            self.measured
                .record_cpu_rate(measured_kinds::DRAW, 1000.0 / rate, 1_000_000.0);
        }
        rate
    }

    /// Plan one frame of `scene` at `width`×`height` device pixels.
    ///
    /// Predictions come from the profile the mode machine is currently
    /// running — the raw one in shadow, the calibrated one after promotion.
    ///
    /// `None` if the render graph does not compile. `build_graph` is not
    /// known to produce such a graph, but this runs inside a live frame loop
    /// as a diagnostic, and a diagnostic must never be the thing that takes a
    /// window down — a missing shadow line is the right failure.
    #[must_use]
    pub fn plan(&self, scene: &Scene, width: u32, height: u32) -> Option<ShadowPlan> {
        self.plan_with(&self.model, scene, width, height)
    }

    /// [`Self::plan`], with the per-pass estimates made through
    /// [`Self::measured`] where that history has samples — the plan the
    /// planner would *act on*: real GPU timings for the kinds of work this
    /// device has already run, measured CPU rates where a caller has fed
    /// them (the micro-benchmark's seed, a live CPU path's per-frame
    /// notes), the heuristic wherever both are missing (which is
    /// [`MeasuredCostModel`]'s own contract). Used by
    /// [`Self::plan_for_frame`] in [`PlannerMode::Authoritative`] only, for
    /// the module-doc reason: the logged predictions keep the raw model's
    /// meaning, and the model being acted on is the one the gate scores
    /// next.
    #[must_use]
    fn plan_measured(&self, scene: &Scene, width: u32, height: u32) -> Option<ShadowPlan> {
        self.plan_with(
            &MeasuredByKind {
                measured: &self.measured,
            },
            scene,
            width,
            height,
        )
    }

    /// The body of [`Self::plan`] against an arbitrary cost model — the one
    /// seam through which the heuristic and the measured-augmented plans are
    /// built, so they cannot drift apart.
    fn plan_with(
        &self,
        model: &dyn CostModel,
        scene: &Scene,
        width: u32,
        height: u32,
    ) -> Option<ShadowPlan> {
        let scene_graph = SceneGraph::from_scene(scene);
        let (graph, screen) = vieww_render_graph::build_graph(&scene_graph, width, height);
        let exec = graph.compile(screen).ok()?;
        let budget = FrameBudget::new(self.planning_device.frame_deadline_ms());
        let (frame_plan, _per_pass) =
            plan_frame(&graph, &exec, &self.planning_device, model, budget);

        let mut passes = Vec::with_capacity(exec.order.len());
        let mut all_cpu_ms = 0.0f32;
        let mut all_gpu_ms = self.planning_device.has_gpu().then_some(0.0f32);
        let mut pixels_total = 0.0f32;
        for &id in &exec.order {
            let pass = graph.pass(id);
            let pixels = work_pixels(&graph, pass);
            let estimate = model.estimate(pass, pixels, &self.planning_device);
            all_cpu_ms += estimate.cpu_ms;
            if let (Some(total), Some(gpu)) = (all_gpu_ms.as_mut(), estimate.gpu_ms) {
                *total += gpu;
            }
            pixels_total += pixels;
            passes.push(PassSummary {
                name: pass.name,
                kind: pass.kind,
                pixels,
                placement: frame_plan
                    .placements
                    .get(&id)
                    .copied()
                    .unwrap_or(Placement::Cpu),
                cpu_ms: estimate.cpu_ms,
                gpu_ms: estimate.gpu_ms,
            });
        }

        Some(ShadowPlan {
            frame_plan,
            passes,
            all_cpu_ms,
            all_gpu_ms,
            pixels: pixels_total,
        })
    }

    /// The frame's plan, plus the whole-frame executor the live path is
    /// being asked to run it on — `Some` only in
    /// [`PlannerMode::Authoritative`], only for a `Cpu` recommendation
    /// (never `Gpu`: the live path already prefers the GPU whenever its own
    /// `ScenePlan::is_complete` gate passes, so "acting" on it would be a
    /// no-op wearing a decision's clothes; never `Mixed`: no live backend
    /// can execute a split frame), and only when both readings agree that
    /// the CPU model is trustworthy *now*:
    ///
    /// * the **gate** ([`Self::gate`], since promotion) is `ready`, and
    /// * the **ring** ([`Self::ring`], the most recent frames) is not
    ///   *worse* than the gate's own bound — the freshness guard. A gate
    ///   that cleared a thousand frames ago stays cleared while the device
    ///   under it changes (thermal throttling, a power cable pulled); a
    ///   planner that has drifted lately hands the decision back to the
    ///   live path until its recent record recovers.
    ///
    /// In [`PlannerMode::Authoritative`] the plan itself is the
    /// measured-augmented one (`plan_measured`): real GPU timings by
    /// kind of work where this device has them, the heuristic where it does
    /// not. The returned plan is what the caller should draw from *and* log —
    /// the gate scores the model actuation actually uses, not a different
    /// one computed for the occasion.
    #[must_use]
    pub fn plan_for_frame(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
    ) -> (Option<Executor>, Option<ShadowPlan>) {
        let plan = if self.mode == PlannerMode::Authoritative {
            self.plan_measured(scene, width, height)
        } else {
            self.plan(scene, width, height)
        };
        let Some(plan) = plan else {
            return (None, None);
        };
        // What an authoritative planner may act on, under its guards: a
        // whole-frame CPU recommendation under the CPU gate; a `Mixed`
        // recommendation under *both* gates — a split frame leans on both
        // halves' models, so both must be recently sound. GPU needs no
        // actuation: the live path already prefers the GPU whenever the
        // scene plan is complete.
        let recommendation = plan.recommendation();
        let act = if self.mode == PlannerMode::Authoritative
            && self.gate.ready(Executor::Cpu)
            && self.recent_record_is_sound(Executor::Cpu)
            && (recommendation == Recommendation::Cpu
                || (recommendation == Recommendation::Mixed
                    && self.gate.ready(Executor::Gpu)
                    && self.recent_record_is_sound(Executor::Gpu)))
        {
            match recommendation {
                Recommendation::Cpu => Some(Executor::Cpu),
                Recommendation::Mixed => Some(Executor::Mixed),
                Recommendation::Gpu => None,
            }
        } else {
            None
        };
        (act, Some(plan))
    }

    /// The ring's freshness guard for actuation: once the recent window has
    /// samples, its per-frame RMSE must be no worse than the gate's own
    /// [`AUTHORITATIVE_MAX_RMSE`] bound. An empty or tiny window (a planner
    /// that just promoted, or a quiet window) cannot be *bad* yet, so it
    /// does not veto — the gate still has to have cleared on a thousand
    /// frames, and the ring only takes the decision *away* when the recent
    /// record says the model has drifted.
    #[must_use]
    fn recent_record_is_sound(&self, executor: Executor) -> bool {
        match self.ring.recent_error(executor) {
            Some(recent) if recent.samples >= TIMING_RING_MIN_SAMPLES => {
                recent.rmse_of_ratios <= AUTHORITATIVE_MAX_RMSE
            }
            _ => true,
        }
    }

    /// Record what the live path actually did with a frame planned by
    /// [`Self::plan`] or [`Self::plan_for_frame`], fold it into the raw
    /// [`Self::calibration`] and — past promotion — into the
    /// [`Self::gate`], then run the mode machine's forward step.
    pub fn observe(
        &mut self,
        plan: ShadowPlan,
        label: impl Into<String>,
        (width, height): (u32, u32),
        actual: Executor,
        actual_ms: f32,
        fallback: Option<String>,
    ) -> ShadowFrame {
        let predicted_for_actual = plan.predicted_for(actual);
        let frame = ShadowFrame {
            frame: self.next_frame,
            label: label.into(),
            width,
            height,
            mode: self.mode,
            // The profile the plan's predictions divided by, read before
            // `advance()` can promote it for the *next* frame — the same
            // convention `mode` above keeps.
            cpu_fill_mpix_per_sec: self.planning_device.cpu_fill_mpix_per_sec,
            gpu_fill_mpix_per_sec: self.planning_device.gpu_fill_mpix_per_sec,
            plan,
            actual,
            actual_ms,
            fallback,
        };
        self.next_frame += 1;
        if let Some(predicted) = predicted_for_actual {
            self.ring.record(actual, predicted, actual_ms);
        }
        self.calibration.record(&frame, &self.planning_device);
        if self.mode != PlannerMode::Shadow {
            self.gate.record(&frame, &self.planning_device);
        }
        self.advance();
        frame
    }

    /// The mode machine's single forward step, run after every observation.
    /// One direction only, and each transition requires its own data: the
    /// promotion needs the raw calibration to have something to correct
    /// with, and actuation needs the gate to have cleared.
    fn advance(&mut self) {
        if !self.authoritative_enabled {
            return;
        }
        match self.mode {
            PlannerMode::Shadow => {
                if self.calibration.frames() >= AUTHORITATIVE_MIN_FRAMES
                    && (self.calibration.cpu_error_ratio().is_some()
                        || self.calibration.gpu_error_ratio().is_some())
                {
                    self.planning_device = self.calibration.calibrated(&self.device);
                    self.gate = Calibration::default();
                    self.mode = PlannerMode::ShadowCalibrated;
                }
            }
            PlannerMode::ShadowCalibrated => {
                if self.gate.ready(Executor::Cpu) || self.gate.ready(Executor::Gpu) {
                    self.mode = PlannerMode::Authoritative;
                }
            }
            PlannerMode::Authoritative => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_foundation::{BlendMode, Color, ImageFilter, Rect};
    use vieww_paint::{Canvas, Paint};

    fn flat() -> Scene {
        let mut scene = Scene::new();
        scene.fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Paint::solid(Color::RED));
        scene
    }

    fn glass() -> Scene {
        let mut scene = Scene::new();
        scene.fill_rect(
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            Paint::solid(Color::BLUE),
        );
        scene.push_filtered_layer(
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            1.0,
            BlendMode::Normal,
            ImageFilter::blur(20.0),
        );
        scene.fill_rect(
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            Paint::solid(Color::RED),
        );
        scene.pop_layer();
        scene
    }

    /// Advance a planner through its launch window with frames that measure
    /// exactly what they predict — the cheapest honest way to reach the
    /// steady state the calibration trusts (FW-22: the first
    /// `LAUNCH_EXCLUDE_FRAMES` frames never touch the ratio buckets, so what
    /// they measure is irrelevant here; only their count is not).
    fn past_launch(shadow: &mut ShadowPlanner, plan: &ShadowPlan, size: (u32, u32)) {
        for _ in 0..LAUNCH_EXCLUDE_FRAMES {
            shadow.observe(
                plan.clone(),
                "launch",
                size,
                Executor::Cpu,
                plan.all_cpu_ms,
                None,
            );
        }
    }

    #[test]
    fn a_cpu_only_profile_recommends_the_cpu_and_agrees_with_a_cpu_frame() {
        let mut shadow = ShadowPlanner::new(DeviceProfile::CPU_ONLY_FALLBACK);
        let plan = shadow.plan(&flat(), 200, 200).unwrap();
        assert_eq!(plan.recommendation(), Recommendation::Cpu);
        assert!(plan.all_gpu_ms.is_none());
        let frame = shadow.observe(plan, "flat", (200, 200), Executor::Cpu, 1.0, None);
        assert!(frame.agrees());
        assert_eq!(shadow.calibration().agreeing_frames(), 1);
    }

    #[test]
    fn the_plan_lists_every_pass_in_order_with_its_pixels() {
        let shadow = ShadowPlanner::new(DeviceProfile::DESKTOP_DISCRETE);
        let plan = shadow.plan(&glass(), 1000, 1000).unwrap();
        let names: Vec<_> = plan.passes.iter().map(|p| p.name).collect();
        assert_eq!(names.first(), Some(&"root_raster"));
        assert_eq!(names.last(), Some(&"present_blit"));
        assert!(names.contains(&"layer_filter"));
        let summed: f32 = plan.passes.iter().map(|p| p.pixels).sum();
        assert!((summed - plan.pixels).abs() < 1.0);
        assert!(plan.all_gpu_ms.unwrap() < plan.all_cpu_ms);
    }

    #[test]
    fn a_gpu_leaning_plan_disagrees_with_a_cpu_fallback_and_says_why() {
        let mut shadow = ShadowPlanner::new(DeviceProfile::DESKTOP_DISCRETE);
        let plan = shadow.plan(&glass(), 1000, 1000).unwrap();
        assert_ne!(plan.recommendation(), Recommendation::Cpu);
        let frame = shadow.observe(
            plan,
            "glass",
            (1000, 1000),
            Executor::Cpu,
            12.0,
            Some("plan incomplete: Blur".to_owned()),
        );
        assert!(!frame.agrees());
        let line = frame.to_json_line();
        // The mode rides right after the frame number, and the profile's
        // own numbers ride right after the mode, so a log's frames can be
        // split by which profile made their predictions — and state the
        // correction they were made under, not just tag it.
        assert!(line.starts_with(
            "{\"frame\":0,\"mode\":\"shadow\",\
             \"cpu_fill_mpix_per_sec\":250,\"gpu_fill_mpix_per_sec\":8000,\
             \"label\":\"glass\""
        ));
        assert!(line.contains("\"fallback\":\"plan incomplete: Blur\""));
        assert!(line.contains("\"agrees\":false"));
        assert!(line.ends_with("]}"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn calibration_recovers_the_throughput_that_would_have_been_right() {
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let mut shadow = ShadowPlanner::new(device);
        let plan = shadow.plan(&flat(), 500, 500).unwrap();
        past_launch(&mut shadow, &plan, (500, 500));
        for _ in 0..4 {
            // This "device" is exactly twice as slow as the profile claims.
            let actual = plan.all_cpu_ms * 2.0;
            shadow.observe(
                plan.clone(),
                "flat",
                (500, 500),
                Executor::Cpu,
                actual,
                None,
            );
        }
        // Only the steady frames are in the buckets: the launch window's
        // sixty are not (FW-22).
        assert_eq!(shadow.calibration().cpu_frames(), 4);
        let ratio = shadow.calibration().cpu_error_ratio().unwrap();
        assert!((ratio - 2.0).abs() < 1e-4, "{ratio}");
        let fixed = shadow.calibration().calibrated(&device);
        assert!((fixed.cpu_fill_mpix_per_sec - device.cpu_fill_mpix_per_sec / 2.0).abs() < 1e-3);

        // With the corrected profile, the same frame is predicted correctly.
        let replanned = ShadowPlanner::new(fixed).plan(&flat(), 500, 500).unwrap();
        let original = ShadowPlanner::new(device).plan(&flat(), 500, 500).unwrap();
        assert!((replanned.all_cpu_ms - original.all_cpu_ms * 2.0).abs() < 1e-3);
    }

    #[test]
    fn gpu_calibration_holds_overhead_fixed_and_ignores_impossible_frames() {
        let device = DeviceProfile::DESKTOP_DISCRETE;
        let mut shadow = ShadowPlanner::new(device);
        let plan = shadow.plan(&glass(), 1000, 1000).unwrap();
        past_launch(&mut shadow, &plan, (1000, 1000));
        // Faster than the assumed dispatch overhead alone: says nothing
        // about fill rate, so the fill rate is left alone.
        shadow.observe(plan, "g", (1000, 1000), Executor::Gpu, 0.0001, None);
        let fixed = shadow.calibration().calibrated(&device);
        assert_eq!(fixed.gpu_fill_mpix_per_sec, device.gpu_fill_mpix_per_sec);
        assert!(shadow.calibration().gpu_error_ratio().is_some());
    }

    #[test]
    fn rmse_is_the_spread_of_the_ratios_not_their_total() {
        // Two devices whose total error is identical — every frame 3× the
        // prediction — but one is a steady 3× and the other alternates 1×
        // and 5×. The ratio of sums cannot tell them apart; the RMSE can,
        // and the gate is stated on the RMSE for exactly that reason.
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let mut steady = ShadowPlanner::new(device);
        let steady_plan = steady.plan(&flat(), 400, 400).unwrap();
        past_launch(&mut steady, &steady_plan, (400, 400));
        for _ in 0..10 {
            let observed = steady_plan.clone();
            steady.observe(
                observed,
                "flat",
                (400, 400),
                Executor::Cpu,
                steady_plan.all_cpu_ms * 3.0,
                None,
            );
        }
        let steady_rmse = steady.calibration().rmse_of_ratio(Executor::Cpu).unwrap();
        assert!((steady_rmse - 2.0).abs() < 1e-6, "{steady_rmse}");

        let mut wild = ShadowPlanner::new(device);
        let wild_plan = wild.plan(&flat(), 400, 400).unwrap();
        past_launch(&mut wild, &wild_plan, (400, 400));
        for i in 0..10 {
            let observed = wild_plan.clone();
            let factor = if i % 2 == 0 { 1.0 } else { 5.0 };
            wild.observe(
                observed,
                "flat",
                (400, 400),
                Executor::Cpu,
                wild_plan.all_cpu_ms * factor,
                None,
            );
        }
        let wild_ratio = wild.calibration().cpu_error_ratio().unwrap();
        let wild_rmse = wild.calibration().rmse_of_ratio(Executor::Cpu).unwrap();
        assert!(
            (wild_ratio - 3.0).abs() < 1e-4,
            "same total error: {wild_ratio}"
        );
        assert!(
            wild_rmse > steady_rmse + 0.5,
            "the swinging error must read worse: {wild_rmse} against {steady_rmse}"
        );
    }

    #[test]
    fn the_gate_needs_the_full_sample_window_even_at_perfect_accuracy() {
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let mut shadow = ShadowPlanner::new(device);
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        // The window the gate counts is steady frames only: the launch
        // window's sixty never reach it (FW-22), so the full budget is the
        // gate's own thousand plus the launch window.
        let need = AUTHORITATIVE_MIN_FRAMES + LAUNCH_EXCLUDE_FRAMES;
        for _ in 0..(need - 1) {
            let observed = plan.clone();
            shadow.observe(
                observed,
                "flat",
                (400, 400),
                Executor::Cpu,
                plan.all_cpu_ms,
                None,
            );
        }
        assert_eq!(
            shadow.calibration().cpu_frames(),
            AUTHORITATIVE_MIN_FRAMES - 1
        );
        assert!(
            !shadow.calibration().ready(Executor::Cpu),
            "one frame short of the window is not ready, whatever the RMSE"
        );
        let observed = plan.clone();
        shadow.observe(
            observed,
            "flat",
            (400, 400),
            Executor::Cpu,
            plan.all_cpu_ms,
            None,
        );
        assert!(
            shadow.calibration().ready(Executor::Cpu),
            "a model whose every frame measured exactly what it predicted, \
             over the full window, is ready by construction"
        );
    }

    #[test]
    fn without_the_opt_in_the_machine_never_leaves_shadow() {
        let mut shadow = ShadowPlanner::new(DeviceProfile::DESKTOP_DISCRETE);
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        for _ in 0..(2 * AUTHORITATIVE_MIN_FRAMES + 10) {
            let observed = plan.clone();
            shadow.observe(
                observed,
                "flat",
                (400, 400),
                Executor::Cpu,
                plan.all_cpu_ms,
                None,
            );
        }
        assert_eq!(shadow.mode(), PlannerMode::Shadow);
        assert_eq!(shadow.plan_for_frame(&flat(), 400, 400).0, None);
    }

    #[test]
    fn an_opted_in_planner_promotes_then_actuates_when_the_gate_clears() {
        // A tiny flat frame on a GPU-capable profile: the planner
        // recommends the CPU, because at 40x40 the GPU's dispatch
        // overhead alone costs more than the whole fill — the classic
        // CPU-affinity case, arrived at by arithmetic rather than by the
        // hint. The "device" then measures exactly what the model
        // predicts, so the raw calibration is a perfect correction and
        // the gate clears the moment its window is full. The frames after
        // that are the ones `plan_for_frame` may act on.
        let mut shadow = ShadowPlanner::with_authoritative(DeviceProfile::DESKTOP_DISCRETE);
        let plan = shadow.plan(&flat(), 40, 40).unwrap();
        assert_eq!(plan.recommendation(), Recommendation::Cpu, "test premise");
        let mut acted = 0;
        // Raw calibration: a thousand steady frames past the launch window;
        // gate: a thousand more under the corrected profile.
        let frames = 2 * AUTHORITATIVE_MIN_FRAMES + 2 * LAUNCH_EXCLUDE_FRAMES + 10;
        for _ in 0..frames {
            let (act, planned) = shadow.plan_for_frame(&flat(), 40, 40);
            let planned = planned.expect("a flat scene always compiles");
            // The live path measures whatever it measures; here it is
            // exactly the prediction, which is the best case the gate can
            // see. Read before the plan moves into `observe`.
            let predicted = planned.all_cpu_ms;
            shadow.observe(planned, "flat", (40, 40), Executor::Cpu, predicted, None);
            if act == Some(Executor::Cpu) {
                acted += 1;
            }
        }
        assert_eq!(shadow.mode(), PlannerMode::Authoritative);
        assert!(
            acted > 0,
            "an Authoritative planner with a ready CPU gate acts"
        );
    }

    #[test]
    fn json_numbers_are_compact_and_finite() {
        assert_eq!(fmt_f32(1.5), "1.5");
        assert_eq!(fmt_f32(2.0), "2");
        assert_eq!(fmt_f32(0.0), "0");
        assert_eq!(fmt_f32(f32::NAN), "null");
        assert_eq!(json_str("a\"b\n"), "\"a\\\"b\\u000a\"");
    }

    #[test]
    fn the_ring_holds_only_the_most_recent_frames() {
        let mut ring = TimingRing::new(4);
        for i in 0..6 {
            ring.record(Executor::Cpu, 1.0, i as f32);
        }
        let recent = ring.recent_error(Executor::Cpu).unwrap();
        assert_eq!(recent.samples, 4, "evicted to capacity");
        // The kept frames are 2, 3, 4, 5: predicted 1 ms each, measured
        // 2+3+4+5 = 14 ms over 4 ms predicted.
        assert!((recent.ratio_of_sums - 14.0 / 4.0).abs() < 1e-9);
        // Nothing recorded for the other executor.
        assert!(ring.recent_error(Executor::Gpu).is_none());
        // Non-finite samples never enter the window.
        ring.record(Executor::Gpu, f32::NAN, 1.0);
        assert!(ring.recent_error(Executor::Gpu).is_none());
    }

    #[test]
    fn the_ring_sees_a_drift_the_lifetime_gate_cannot() {
        // The scenario this ring exists for: a device that runs 20% slower
        // than the model says — a thermal throttle, a background job — for
        // long enough to matter. The lifetime gate dilutes those frames
        // across every frame since promotion and stays `ready`; the ring
        // sees that the *recent* record is a steady 1.2x off (RMSE > 0.05
        // within sixteen frames) and hands the decision back — sixty drifted
        // frames keep the gate's own diluted RMSE at ~0.047, still under its
        // bound. Then the device recovers, the window refills with clean
        // frames, and the decision returns — which a poisoned lifetime gate
        // could never do on its own.
        let mut shadow = ShadowPlanner::with_authoritative(DeviceProfile::CPU_ONLY_FALLBACK);
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        let mut acted = 0;
        let frames = 2 * AUTHORITATIVE_MIN_FRAMES + 2 * LAUNCH_EXCLUDE_FRAMES + 10;
        for _ in 0..frames {
            let (act, planned) = shadow.plan_for_frame(&flat(), 400, 400);
            let planned = planned.unwrap();
            shadow.observe(
                planned,
                "flat",
                (400, 400),
                Executor::Cpu,
                plan.all_cpu_ms,
                None,
            );
            if act.is_some() {
                acted += 1;
            }
        }
        assert_eq!(shadow.mode(), PlannerMode::Authoritative);
        assert!(acted > 0, "acts while the record is clean");
        // The drift: a steady 20% slowdown the gate dilutes away. Sixty
        // frames keeps the lifetime gate's own RMSE at ~0.047 (still `ready`
        // — the premise), while the recent window's RMSE hits 0.1.
        let mut vetoed = 0;
        for _ in 0..60 {
            let (act, planned) = shadow.plan_for_frame(&flat(), 400, 400);
            let planned = planned.unwrap();
            shadow.observe(
                planned,
                "flat",
                (400, 400),
                Executor::Cpu,
                plan.all_cpu_ms * 1.2,
                None,
            );
            if act.is_none() {
                vetoed += 1;
            }
        }
        assert!(
            vetoed >= 25,
            "the recent window vetoed the drift the lifetime gate still calls ready"
        );
        assert!(
            shadow.gate().ready(Executor::Cpu),
            "the premise: the lifetime gate itself never noticed"
        );
        assert!(
            shadow
                .ring()
                .recent_error(Executor::Cpu)
                .unwrap()
                .rmse_of_ratios
                > AUTHORITATIVE_MAX_RMSE
        );
        // ...and it takes the decision back once the record recovers: the
        // window must wash the drifted frames out, which costs at most the
        // ring's capacity in clean frames.
        let mut recovered = false;
        for _ in 0..(TIMING_RING_CAPACITY + 2) {
            let (act, planned) = shadow.plan_for_frame(&flat(), 400, 400);
            let planned = planned.unwrap();
            shadow.observe(
                planned,
                "flat",
                (400, 400),
                Executor::Cpu,
                plan.all_cpu_ms,
                None,
            );
            if act.is_some() {
                recovered = true;
                break;
            }
        }
        assert!(
            recovered,
            "the ring released its veto after the record recovered"
        );
    }

    #[test]
    fn measured_gpu_kinds_feed_the_plan_actuation_uses() {
        // In shadow the logged plan is the heuristic's. After the machine
        // promotes, the plan `plan_for_frame` returns carries the measured
        // history where the device has one: a "draw" bucket that measured
        // 50 ms moves the acted-on plan's GPU prediction by exactly that,
        // while the shadow plan (and every JSON line before promotion) still
        // says the heuristic.
        let mut shadow = ShadowPlanner::with_authoritative(DeviceProfile::DESKTOP_DISCRETE);
        let heuristic = shadow.plan(&glass(), 1000, 1000).unwrap();
        assert!(
            heuristic.all_gpu_ms.unwrap() < 40.0,
            "the heuristic's whole-frame GPU prediction for a 1M-pixel glass scene"
        );
        let frames = 2 * AUTHORITATIVE_MIN_FRAMES + 2 * LAUNCH_EXCLUDE_FRAMES + 10;
        for _ in 0..frames {
            let (_, planned) = shadow.plan_for_frame(&glass(), 1000, 1000);
            let planned = planned.unwrap();
            let predicted = planned.all_gpu_ms.unwrap();
            shadow.observe(
                planned,
                "glass",
                (1000, 1000),
                Executor::Gpu,
                predicted,
                None,
            );
        }
        assert_eq!(shadow.mode(), PlannerMode::Authoritative);
        // The backend's timestamp bucket says drawing on this device is
        // expensive. One sample is the whole history (EMA inserts the first
        // sample directly).
        shadow.note_gpu_pass_kind(measured_kinds::DRAW, 50.0);
        let (_, acted) = shadow.plan_for_frame(&glass(), 1000, 1000);
        let acted = acted.unwrap();
        assert!(
            acted.all_gpu_ms.unwrap() >= 50.0,
            "the acted-on plan prices draw work at its measured cost"
        );
        // The logged plan is untouched by the measured history.
        let logged = shadow.plan(&glass(), 1000, 1000).unwrap();
        assert!(
            (logged.all_gpu_ms.unwrap() - heuristic.all_gpu_ms.unwrap()).abs() < 1e-4,
            "the shadow plan keeps the heuristic's meaning for the whole run"
        );
    }

    #[test]
    fn zero_and_non_finite_samples_never_become_history() {
        let mut shadow = ShadowPlanner::new(DeviceProfile::DESKTOP_DISCRETE);
        // Past the launch window first, so the refusal below is the
        // sample's own fault and not the window's (FW-22).
        let plan = shadow.plan(&glass(), 1000, 1000).unwrap();
        past_launch(&mut shadow, &plan, (1000, 1000));
        shadow.note_gpu_pass_kind(measured_kinds::DRAW, 0.0);
        shadow.note_gpu_pass_kind(measured_kinds::DRAW, f32::NAN);
        shadow.note_gpu_pass_kind(measured_kinds::DRAW, -1.0);
        // No history was recorded, so the plan is exactly the heuristic's:
        // same per-pass estimates, same totals. (Not compared as whole
        // `ShadowPlan`s — `predicted_ms` is a HashMap-ordered float sum,
        // equal in meaning and unequal in last bits.)
        let planned = shadow.plan(&glass(), 1000, 1000).unwrap();
        let fresh = ShadowPlanner::new(DeviceProfile::DESKTOP_DISCRETE)
            .plan(&glass(), 1000, 1000)
            .unwrap();
        assert_eq!(planned.passes, fresh.passes);
        assert!((planned.all_cpu_ms - fresh.all_cpu_ms).abs() < 1e-5);
        assert!((planned.all_gpu_ms.unwrap() - fresh.all_gpu_ms.unwrap()).abs() < 1e-5);
    }

    // ── the calibration loop (FW-22) ─────────────────────────────────────

    /// One `"key":value` number out of a JSON line — the receipt fields'
    /// own reader, so the assertions below check numbers, not spellings.
    fn json_f32(line: &str, key: &str) -> f32 {
        let start = line
            .find(&format!("\"{key}\":"))
            .unwrap_or_else(|| panic!("no {key} in {line}"))
            + key.len()
            + 3;
        let end = line[start..].find(',').map_or(line.len(), |i| start + i);
        line[start..end]
            .trim_end_matches('}')
            .parse()
            .unwrap_or_else(|_| panic!("{key} is not a number in {line}"))
    }

    #[test]
    fn every_receipt_line_records_the_profile_it_was_planned_with() {
        // The shadow log is a receipt (FW-22), and a receipt says which
        // profile made the numbers it holds: the fill throughputs ride on
        // every line, not just the mode tag that names their provenance.
        let mut shadow = ShadowPlanner::new(DeviceProfile::CPU_ONLY_FALLBACK);
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        let frame = shadow.observe(plan, "flat", (400, 400), Executor::Cpu, 1.0, None);
        let line = frame.to_json_line();
        assert_eq!(
            json_f32(&line, "cpu_fill_mpix_per_sec"),
            DeviceProfile::CPU_ONLY_FALLBACK.cpu_fill_mpix_per_sec
        );
        assert_eq!(
            json_f32(&line, "gpu_fill_mpix_per_sec"),
            DeviceProfile::CPU_ONLY_FALLBACK.gpu_fill_mpix_per_sec
        );
    }

    #[test]
    fn authorisation_can_be_granted_to_a_planner_that_already_runs() {
        // `allow_authoritative` is `with_authoritative` for a planner that
        // already exists: the measured history and the launch window it
        // has accumulated survive the grant — where a rebuilt planner
        // would have restarted both, and re-run the startup benchmark to
        // get back what it just threw away. The gate still earns its
        // frames from the grant onward, which is the point.
        let mut shadow = ShadowPlanner::new(DeviceProfile::CPU_ONLY_FALLBACK);
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        past_launch(&mut shadow, &plan, (400, 400));
        shadow.note_cpu_pass_kind(measured_kinds::DRAW, 2.0, 1_000_000.0);
        shadow.allow_authoritative();
        // The grant kept the history: the rate noted past the window is
        // still the measured model's, not reset to nothing.
        let rate = shadow.measured_cpu_rate(measured_kinds::DRAW);
        assert!((rate.unwrap() - 500.0).abs() < 0.01, "{rate:?}");
        // And the machine runs from here: promote, fill the gate, act.
        let frames = 2 * AUTHORITATIVE_MIN_FRAMES + LAUNCH_EXCLUDE_FRAMES + 10;
        for _ in 0..frames {
            let (_, planned) = shadow.plan_for_frame(&flat(), 400, 400);
            let planned = planned.expect("a flat scene always compiles");
            let predicted = planned.all_cpu_ms;
            shadow.observe(planned, "flat", (400, 400), Executor::Cpu, predicted, None);
        }
        assert_eq!(shadow.mode(), PlannerMode::Authoritative);
        assert!(shadow.plan_for_frame(&flat(), 400, 400).0.is_some());
    }

    #[test]
    fn the_calibration_loop_closes_the_launch_underestimate() {
        // FW-22's regression, on §4.6's own numbers: the phones' launch-time
        // CPU cost sat 34–38% under their steady state (Stylo 22.1 vs
        // 35.7 ms, A73 11.0 vs 16.6), and every number the planner had was
        // launch-shaped — the profile's round-number guesses, the startup
        // benchmark. Reproduce the finding synthetically — a profile whose
        // guess matches the launch frames exactly, a machine whose steady
        // state is 35.5% slower — and assert the loop closes the
        // steady-state error to under 10%.
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let mut shadow = ShadowPlanner::with_authoritative(device);
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        let launch_ms = plan.all_cpu_ms;
        // 1.55x ⇒ the guess is 35.5% under the steady state: inside the
        // 34–38% band §4.6 measured.
        let steady_ms = launch_ms * 1.55;
        let raw_error = 1.0 - launch_ms / steady_ms;
        assert!(
            (0.34..=0.38).contains(&raw_error),
            "the reproduced finding: {raw_error:.3}"
        );

        // The launch window, at launch speed: the §4.6 trap, in full —
        // every one of these frames measures what the guess predicts, so
        // a calibration that counted them would correct the profile
        // toward a launch speed the machine cannot sustain once warm.
        // (Counted, this window's frames would leave the correction at
        // (60 + 1000×1.55)/1060 = 1.519x instead of the steady 1.55x — a
        // 2% steady-state residue: under the 10% bound, but a residue the
        // exclusion removes entirely, and the exact closure below is this
        // test's teeth.)
        for _ in 0..LAUNCH_EXCLUDE_FRAMES {
            shadow.observe(
                plan.clone(),
                "launch",
                (400, 400),
                Executor::Cpu,
                launch_ms,
                None,
            );
        }
        // A thousand steady frames to promote the raw calibration — all
        // of them post-launch, so the ratio the promotion computes is the
        // steady one, not a launch/steady blend. Each frame is planned
        // fresh, the way the live loop does it.
        for _ in 0..AUTHORITATIVE_MIN_FRAMES {
            let (_, planned) = shadow.plan_for_frame(&flat(), 400, 400);
            let planned = planned.expect("a flat scene always compiles");
            shadow.observe(
                planned,
                "steady",
                (400, 400),
                Executor::Cpu,
                steady_ms,
                None,
            );
        }
        assert_eq!(shadow.mode(), PlannerMode::ShadowCalibrated);
        // The launch frames contributed nothing to the buckets the
        // correction was computed from.
        assert_eq!(shadow.calibration().cpu_frames(), AUTHORITATIVE_MIN_FRAMES);

        // The loop's answer: the calibrated profile, replanned, predicts
        // the steady state the machine actually sustains — the 34–38%
        // underestimate closed to under 10% (here, to under a thousandth).
        let calibrated = shadow.calibration().calibrated(&device);
        let replanned = ShadowPlanner::new(calibrated)
            .plan(&flat(), 400, 400)
            .unwrap();
        let closed = (replanned.all_cpu_ms - steady_ms).abs() / steady_ms;
        assert!(
            closed < 0.10,
            "the loop left a {:.1}% steady-state error",
            closed * 100.0
        );
        assert!(
            closed < 0.001,
            "the exact steady ratio closes exactly: {closed}"
        );

        // The gate, scoring the corrected profile: a thousand more steady
        // frames — which the calibrated profile now predicts exactly — and
        // the machine may act.
        for _ in 0..AUTHORITATIVE_MIN_FRAMES {
            let (act, planned) = shadow.plan_for_frame(&flat(), 400, 400);
            let planned = planned.expect("a flat scene always compiles");
            shadow.observe(
                planned,
                "steady",
                (400, 400),
                Executor::Cpu,
                steady_ms,
                None,
            );
            assert_eq!(act, None, "not authoritative until the gate clears");
        }
        assert_eq!(shadow.mode(), PlannerMode::Authoritative);
        assert_eq!(
            shadow.calibration().cpu_frames(),
            2 * AUTHORITATIVE_MIN_FRAMES,
            "the launch frames never reached the buckets, before or after promotion"
        );

        // And the receipts recorded the whole trip: the last steady
        // frame's line carries the calibrated profile — the number the
        // loop landed on, not the launch-shaped one it started with.
        let last = shadow
            .observe(
                shadow.plan(&flat(), 400, 400).unwrap(),
                "steady",
                (400, 400),
                Executor::Cpu,
                steady_ms,
                None,
            )
            .to_json_line();
        // The line's number is the profile's own f32, quantised to the
        // receipt format's four decimals.
        let lined = json_f32(&last, "cpu_fill_mpix_per_sec");
        assert!(
            (lined - calibrated.cpu_fill_mpix_per_sec).abs() < 1e-3,
            "{lined} vs {}",
            calibrated.cpu_fill_mpix_per_sec
        );
        assert_ne!(lined, device.cpu_fill_mpix_per_sec);

        // And the acted-on plan carries the calibrated prediction, not the
        // launch-shaped one the run started with.
        let (_, acted) = shadow.plan_for_frame(&flat(), 400, 400);
        let acted = acted.unwrap();
        let acted_error = (acted.all_cpu_ms - steady_ms).abs() / steady_ms;
        assert!(
            acted_error < 0.10,
            "the acted-on plan is still {:.1}% off the steady state",
            acted_error * 100.0
        );
    }

    #[test]
    fn launch_frames_alone_produce_no_correction_at_all() {
        // The honest refusal: a run that never leaves the launch window has
        // no steady evidence, so the calibration stays empty, the
        // correction keeps the guess untouched, and — even opted in — the
        // machine never promotes. The old arithmetic would have "corrected"
        // the profile from launch frames alone, which is exactly the §4.6
        // bias.
        let device = DeviceProfile::CPU_ONLY_FALLBACK;
        let mut shadow = ShadowPlanner::with_authoritative(device);
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        for _ in 0..(LAUNCH_EXCLUDE_FRAMES - 1) {
            shadow.observe(
                plan.clone(),
                "launch",
                (400, 400),
                Executor::Cpu,
                plan.all_cpu_ms * 3.0,
                None,
            );
        }
        assert_eq!(shadow.calibration().cpu_frames(), 0);
        assert!(shadow.calibration().cpu_error_ratio().is_none());
        let fixed = shadow.calibration().calibrated(&device);
        assert_eq!(fixed.cpu_fill_mpix_per_sec, device.cpu_fill_mpix_per_sec);
        assert_eq!(shadow.mode(), PlannerMode::Shadow);
    }

    #[test]
    fn launch_samples_never_become_measured_history() {
        // `note_*_pass_kind` refuses samples inside the launch window —
        // the measured model the policy eventually acts on must not be
        // seeded by boost-clock launch timings (§4.6's bias, applied to the
        // EMA where it would otherwise live forever). Past the window, the
        // same note lands.
        let mut shadow = ShadowPlanner::new(DeviceProfile::CPU_ONLY_FALLBACK);
        shadow.note_cpu_pass_kind(measured_kinds::DRAW, 1.0, 1_000_000.0);
        assert!(
            shadow.measured_cpu_rate(measured_kinds::DRAW).is_none(),
            "a launch-window sample never lands"
        );
        let plan = shadow.plan(&flat(), 400, 400).unwrap();
        past_launch(&mut shadow, &plan, (400, 400));
        shadow.note_cpu_pass_kind(measured_kinds::DRAW, 2.0, 1_000_000.0);
        // 2 ms per Mpix is 500 Mpix/s, to f32's own rounding of it.
        let rate = shadow.measured_cpu_rate(measured_kinds::DRAW).unwrap();
        assert!((rate - 500.0).abs() < 0.01, "{rate}");
    }

    #[test]
    fn cpu_measured_rates_feed_the_plan_actuation_uses() {
        // The CPU twin of `measured_gpu_kinds_feed_the_plan_actuation_uses`:
        // once the machine promotes, a measured `draw` rate is what the
        // acted-on plan prices raster work at — the rate the micro-benchmark
        // seeds and a live CPU path keeps warm — while the logged plan keeps
        // the heuristic's meaning for the whole run.
        let mut shadow = ShadowPlanner::with_authoritative(DeviceProfile::CPU_ONLY_FALLBACK);
        let heuristic = shadow.plan(&flat(), 400, 400).unwrap();
        let frames = 2 * AUTHORITATIVE_MIN_FRAMES + 2 * LAUNCH_EXCLUDE_FRAMES + 10;
        for _ in 0..frames {
            let (_, planned) = shadow.plan_for_frame(&flat(), 400, 400);
            let planned = planned.unwrap();
            let predicted = planned.all_cpu_ms;
            shadow.observe(planned, "flat", (400, 400), Executor::Cpu, predicted, None);
        }
        assert_eq!(shadow.mode(), PlannerMode::Authoritative);
        // A measured draw rate 5x slower than the profile's guess: 5000 ms
        // per guessed-Mpix-per-second of throughput, i.e. one Mpix in
        // 5000/guessed ms.
        let guessed = shadow.device().cpu_fill_mpix_per_sec;
        shadow.note_cpu_pass_kind(measured_kinds::DRAW, 5000.0 / guessed, 1_000_000.0);
        let (_, acted) = shadow.plan_for_frame(&flat(), 400, 400);
        let acted = acted.unwrap();
        // The raster pass itself is priced at the measured rate (5x the
        // heuristic's own estimate for the same pass); the whole-frame
        // number moves by whatever share raster work is of it.
        let heuristic_raster = heuristic
            .passes
            .iter()
            .find(|p| p.name == "root_raster")
            .expect("a raster pass");
        let acted_raster = acted
            .passes
            .iter()
            .find(|p| p.name == "root_raster")
            .expect("a raster pass");
        assert!(
            acted_raster.cpu_ms > heuristic_raster.cpu_ms * 4.5,
            "the acted-on plan prices draw work at its measured rate: {} vs {}",
            acted_raster.cpu_ms,
            heuristic_raster.cpu_ms
        );
        assert!(acted.all_cpu_ms > heuristic.all_cpu_ms * 2.0);
        // The logged plan is untouched by the measured history.
        let logged = shadow.plan(&flat(), 400, 400).unwrap();
        assert!(
            (logged.all_cpu_ms - heuristic.all_cpu_ms).abs() < 1e-4,
            "the shadow plan keeps the heuristic's meaning"
        );
    }

    #[test]
    fn the_benchmark_seed_feeds_both_halves_of_the_loop() {
        // `seed_cpu_fill_benchmark` (the `calibrate` feature) is the loop's
        // step one: one measurement, both halves — the profile the
        // heuristic divides by, and the measured model's `draw` rate. In a
        // debug build the benchmark deliberately does not run, so this
        // asserts the plumbing on whatever path the build took.
        let mut shadow = ShadowPlanner::new(DeviceProfile::CPU_ONLY_FALLBACK);
        let before = shadow.device().cpu_fill_mpix_per_sec;
        let seeded = seed_if_available(&mut shadow);
        match seeded {
            Some(rate) => {
                assert!((shadow.device().cpu_fill_mpix_per_sec - rate).abs() < 1e-3);
                assert_eq!(
                    shadow.measured_cpu_rate(measured_kinds::DRAW),
                    Some(rate),
                    "the seed feeds the measured model in the same unit"
                );
            }
            None => {
                assert_eq!(shadow.device().cpu_fill_mpix_per_sec, before);
                assert!(shadow.measured_cpu_rate(measured_kinds::DRAW).is_none());
            }
        }
    }

    /// `seed_cpu_fill_benchmark` when the `calibrate` feature is on, `None`
    /// when it is not — the test above runs on either build.
    #[cfg(feature = "calibrate")]
    fn seed_if_available(shadow: &mut ShadowPlanner) -> Option<f32> {
        shadow.seed_cpu_fill_benchmark()
    }

    #[cfg(not(feature = "calibrate"))]
    fn seed_if_available(_shadow: &mut ShadowPlanner) -> Option<f32> {
        None
    }

    #[test]
    fn the_pass_kinds_map_onto_the_backend_vocabulary() {
        assert_eq!(measured_kind(PassKind::Raster), measured_kinds::DRAW);
        assert_eq!(measured_kind(PassKind::Filter), measured_kinds::FILTERS);
        assert_eq!(measured_kind(PassKind::Composite), measured_kinds::LAYERS);
        assert_eq!(measured_kind(PassKind::Blit), measured_kinds::BLIT);
    }

    // ── the split frame: `Executor::Mixed` ────────────────────────────────

    /// A plan built by hand: the placements and per-pass estimates say
    /// everything these tests need, without a scene whose recommendation
    /// would depend on the cost model's constants.
    fn hand_plan(placements: Vec<Placement>, cpu_ms: Vec<f32>, gpu_ms: Vec<f32>) -> ShadowPlan {
        let passes: Vec<PassSummary> = placements
            .iter()
            .enumerate()
            .map(|(i, placement)| PassSummary {
                name: "p",
                kind: PassKind::Raster,
                pixels: 1000.0,
                placement: *placement,
                cpu_ms: cpu_ms[i],
                gpu_ms: gpu_ms.get(i).copied(),
            })
            .collect();
        let predicted_ms: f32 = passes
            .iter()
            .map(|p| match p.placement {
                Placement::Cpu => p.cpu_ms,
                Placement::Gpu => p.gpu_ms.unwrap_or(p.cpu_ms),
                Placement::Hybrid(s) => {
                    (s.cpu_fraction * p.cpu_ms).max(s.gpu_fraction() * p.gpu_ms.unwrap_or(p.cpu_ms))
                }
            })
            .sum();
        let all_cpu: f32 = cpu_ms.iter().sum();
        let all_gpu: f32 = gpu_ms.iter().sum::<f32>();
        ShadowPlan {
            frame_plan: crate::planner::FramePlan {
                // The tests read the plan's numbers, not its map; the map
                // mirrors nothing but the passes here, and  cannot
                // be built outside the graph anyway.
                placements: std::collections::HashMap::new(),
                predicted_ms,
                deadline_ms: 16.0,
            },
            passes,
            all_cpu_ms: all_cpu,
            all_gpu_ms: Some(all_gpu),
            pixels: 10_000.0,
        }
    }

    #[test]
    fn a_mixed_plan_predicts_its_own_split_time() {
        let plan = hand_plan(
            vec![Placement::Cpu, Placement::Gpu],
            vec![4.0, 6.0],
            vec![2.0, 1.0],
        );
        assert_eq!(plan.recommendation(), Recommendation::Mixed);
        // The mixed prediction is the placements' own sum, not either
        // whole-frame number.
        assert_eq!(plan.predicted_for(Executor::Mixed), Some(4.0 + 1.0));
        assert_eq!(plan.predicted_for(Executor::Cpu), Some(10.0));
        assert_eq!(plan.predicted_for(Executor::Gpu), Some(3.0));
        // And only a split frame executes what a Mixed recommendation
        // asked for.
        assert!(plan.recommendation().agrees_with(Executor::Mixed));
        assert!(!plan.recommendation().agrees_with(Executor::Cpu));
        assert!(!plan.recommendation().agrees_with(Executor::Gpu));
    }

    #[test]
    fn the_split_fraction_weights_each_pass_by_its_own_placement() {
        // Half the predicted work on each side, by duration: a 4 ms CPU
        // pass and a GPU pass whose placement cost is 4 ms.
        let plan = hand_plan(
            vec![Placement::Cpu, Placement::Gpu],
            vec![4.0, 10.0],
            vec![9.0, 4.0],
        );
        let fraction = plan.mixed_cpu_fraction();
        assert!(
            (fraction - 0.5).abs() < 0.01,
            "the CPU half of the predicted work is the CPU band, got {fraction}"
        );
        // An all-GPU plan's CPU fraction is the clamp's floor: a band of
        // zero rows is degenerate, so the split never empties a side.
        let all_gpu = hand_plan(
            vec![Placement::Gpu, Placement::Gpu],
            vec![10.0, 10.0],
            vec![1.0, 1.0],
        );
        assert!((all_gpu.mixed_cpu_fraction() - 0.05).abs() < 1e-6);
        // An all-CPU plan's is the ceiling, for the same reason.
        let all_cpu = hand_plan(
            vec![Placement::Cpu, Placement::Cpu],
            vec![1.0, 1.0],
            vec![9.0, 9.0],
        );
        assert!((all_cpu.mixed_cpu_fraction() - 0.95).abs() < 1e-6);
        // A Hybrid pass contributes its split, not a whole unit: one pass,
        // split 60/40 CPU/GPU by time, is a 60% CPU band.
        let split = hand_plan(
            vec![Placement::Hybrid(crate::hybrid::Split {
                cpu_fraction: 0.6,
            })],
            vec![10.0],
            vec![10.0],
        );
        assert!(
            (split.mixed_cpu_fraction() - 0.6).abs() < 0.02,
            "got {}",
            split.mixed_cpu_fraction()
        );
    }

    #[test]
    fn split_frames_guard_the_ring_but_not_the_lifetime_calibration() {
        let mut planner = ShadowPlanner::with_authoritative(DeviceProfile::DESKTOP_DISCRETE);
        let plan = hand_plan(
            vec![Placement::Cpu, Placement::Gpu],
            vec![4.0, 6.0],
            vec![2.0, 1.0],
        );
        let predicted = plan
            .predicted_for(Executor::Mixed)
            .expect("a mixed plan predicts");
        planner.observe(plan, "split", (800, 600), Executor::Mixed, predicted, None);
        // The lifetime calibration took no whole-frame bucket for the split
        // frame: its CPU and GPU frame counts are what they were (zero —
        // this fresh planner has seen no whole frames at all).
        assert_eq!(planner.calibration().cpu_frames(), 0);
        assert_eq!(planner.calibration().gpu_frames(), 0);
        assert_eq!(planner.calibration().frames(), 0);
        // The ring did take it: the *recent* guard on actuation is exactly
        // where a split frame's prediction quality belongs.
        let recent = planner.ring().recent_error(Executor::Mixed);
        let recent = recent.expect("the split frame is in the ring");
        assert_eq!(recent.samples, 1);
        assert!(
            recent.rmse_of_ratios.abs() < 1e-6,
            "measured exactly the prediction"
        );
        // And its JSON name is what the log line says.
        assert_eq!(Executor::Mixed.name(), "mixed");
    }
}
