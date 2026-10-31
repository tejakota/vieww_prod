//! How a generation is sampled and when it stops — the §5.6 caps, as
//! code instead of intentions.
//!
//! Two kinds of discipline live here:
//!
//! * **Stopping** ([`Stopper`]): end-of-sequence tokens, a hard ceiling on
//!   how many tokens may be written, and a cancellation flag checked
//!   between every pair of tokens. None of the three is advisory; the
//!   generation loop asks after every token and obeys the first answer.
//! * **Sampling** ([`Sampling`]): how the next token is chosen from the
//!   logits. The default is *greedy* — the same situation yields the same
//!   assessment, which is what makes a benchmark comparable between
//!   quantisations and a decision reproducible for the person who asked
//!   for it. Qwen's own recommended sampling for non-thinking mode
//!   (temperature 0.7, top-k 20, top-p 0.8) ships as a named preset for
//!   the bench to compare against, never as the default.
//!
//! The module is pure `f32` arithmetic on purpose: it is unit-tested
//! without a model, without weights, and without a GPU in sight.

/// How the next token is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    /// Greedy: take the argmax. When true, the other fields are ignored.
    pub greedy: bool,
    /// Softmax temperature, applied before top-k/top-p. Sensible range
    /// `(0, 2]`.
    pub temperature: f32,
    /// Keep only the `k` most likely tokens before top-p.
    pub top_k: usize,
    /// Keep the smallest prefix whose cumulative probability reaches this.
    pub top_p: f32,
    /// The seed greedy-sampling never needs and stochastic sampling always
    /// does — held here so a whole benchmark run can be replayed exactly.
    pub seed: u64,
}

impl Default for Sampling {
    fn default() -> Self {
        Self::GREEDY
    }
}

impl Sampling {
    /// The reproducible default: argmax, every time.
    pub const GREEDY: Self = Self {
        greedy: true,
        temperature: 1.0,
        top_k: usize::MAX,
        top_p: 1.0,
        seed: 0,
    };

    /// Qwen's recommended non-thinking settings (temperature 0.7, top-k 20,
    /// top-p 0.8), seeded for replay.
    pub const QWEN_NO_THINK: Self = Self {
        greedy: false,
        temperature: 0.7,
        top_k: 20,
        top_p: 0.8,
        seed: 0,
    };

    /// The settings, sanitised: a non-positive temperature becomes greedy,
    /// because a temperature of zero *is* greedy and pretending otherwise
    /// invites a division by zero dressed as a config.
    #[must_use]
    pub fn sanitised(self) -> Self {
        if self.greedy || self.temperature <= 0.0 {
            Self::GREEDY
        } else {
            Self {
                temperature: self.temperature.clamp(0.01, 2.0),
                top_k: self.top_k.max(1),
                top_p: self.top_p.clamp(0.01, 1.0),
                ..self
            }
        }
    }

    /// Choose the next token from one row of logits.
    ///
    /// `NaN` logits — a quantisation artefact the plan's §5.2 comparison
    /// exists to catch — are treated as the least likely token rather than
    /// allowed to poison a softmax.
    #[must_use]
    pub fn pick(&self, logits: &[f32]) -> u32 {
        let sane = self.sanitised();
        if logits.is_empty() {
            return 0;
        }
        let argmax = |l: &[f32]| {
            l.iter()
                .enumerate()
                .max_by(|(i, a), (j, b)| {
                    let a = if a.is_nan() { f32::NEG_INFINITY } else { **a };
                    let b = if b.is_nan() { f32::NEG_INFINITY } else { **b };
                    // Ties break to the earlier token: a conventional
                    // argmax, and a deterministic one.
                    a.partial_cmp(&b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(i.cmp(j).reverse())
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        if sane.greedy {
            return u32::try_from(argmax(logits)).unwrap_or(0);
        }
        // Temperature + softmax over the top-k.
        let mut idx: Vec<usize> = (0..logits.len()).collect();
        idx.sort_by(|&i, &j| {
            let a = if logits[i].is_nan() {
                f32::NEG_INFINITY
            } else {
                logits[i]
            };
            let b = if logits[j].is_nan() {
                f32::NEG_INFINITY
            } else {
                logits[j]
            };
            b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
        });
        idx.truncate(sane.top_k);
        let scaled: Vec<f32> = idx
            .iter()
            .map(|&i| {
                let l = if logits[i].is_nan() {
                    f32::NEG_INFINITY
                } else {
                    logits[i]
                };
                (l / sane.temperature).max(-60.0)
            })
            .collect();
        let max = scaled.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = scaled.iter().map(|s| (s - max).exp()).collect();
        let sum: f32 = exps.iter().sum();
        let mut cdf: Vec<f32> = Vec::with_capacity(exps.len());
        let mut acc = 0.0;
        for e in &exps {
            acc += e / sum;
            cdf.push(acc);
        }
        // Top-p: keep the smallest prefix reaching p, then sample within it.
        let cutoff = cdf
            .iter()
            .position(|c| *c >= sane.top_p)
            .unwrap_or(cdf.len() - 1);
        let roll = next_f64(sane.seed, logits.len() as u64) as f32;
        let target = roll * cdf[cutoff];
        let chosen = cdf.iter().position(|c| *c >= target).unwrap_or(cutoff);
        u32::try_from(idx[chosen]).unwrap_or(0)
    }
}

/// A tiny, self-contained PRNG (splitmix64) — one dependency fewer on a
/// phone, and deterministic for a given seed, which is the whole point of
/// carrying the seed at all.
fn next_f64(seed: u64, stream: u64) -> f64 {
    let mut z = seed.wrapping_add(stream.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// Why a generation stopped — reported honestly, because "the model
/// finished" and "we gave up waiting" are different facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model wrote its end-of-turn token.
    Finished,
    /// The token ceiling of §5.6 was reached.
    TokenCap,
    /// The caller cancelled — the person left, or the timeout fired.
    Cancelled,
}

/// The three ways a generation ends, checked after every token.
#[derive(Debug, Clone)]
pub struct Stopper {
    /// The token ids that mean "turn over": `<|im_end|>`, and the EOS the
    /// config names, and the plain end-of-text — a model that emits any
    /// of them has said its piece.
    eos: Vec<u32>,
    /// §5.6's ceiling on tokens written.
    max_new_tokens: usize,
    /// Set by the caller from outside the generation loop.
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Stopper {
    /// A stopper that will end on `eos`, after `max_new_tokens`, or when
    /// `cancelled` is raised — whichever comes first.
    #[must_use]
    pub fn new(
        eos: Vec<u32>,
        max_new_tokens: usize,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            eos,
            max_new_tokens,
            cancelled,
        }
    }

    /// May the loop write token number `produced + 1`?
    #[must_use]
    pub fn continues(&self, produced: usize) -> bool {
        if self.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        produced < self.max_new_tokens
    }

    /// Whether the token just produced ends the turn. Called *after* the
    /// token is written, so an EOS is seen even as the last allowed token.
    #[must_use]
    pub fn ends_turn(&self, token: u32) -> bool {
        self.eos.contains(&token)
    }

    /// The honest reason the loop exited, from the facts it observed.
    #[must_use]
    pub fn reason(&self, produced: usize, saw_eos: bool) -> StopReason {
        if self.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            StopReason::Cancelled
        } else if saw_eos {
            StopReason::Finished
        } else {
            let _ = produced;
            StopReason::TokenCap
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flag() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
    }

    #[test]
    fn greedy_picks_the_argmax_and_ignores_everything_else() {
        let s = Sampling::GREEDY;
        assert_eq!(s.pick(&[0.1, 3.0, 2.9]), 1);
        // Ties break to the earlier token, deterministically.
        assert_eq!(s.pick(&[1.0, 1.0, 0.5]), 0);
    }

    #[test]
    fn a_nan_logit_loses_rather_than_poisons() {
        let s = Sampling::GREEDY;
        assert_eq!(s.pick(&[f32::NAN, 0.2, 0.1]), 1);
        assert_eq!(s.pick(&[f32::NAN, f32::NAN]), 0);
    }

    #[test]
    fn zero_temperature_is_greedy_by_name_and_by_behaviour() {
        let mut s = Sampling::QWEN_NO_THINK;
        s.temperature = 0.0;
        assert!(s.sanitised().greedy);
        assert_eq!(s.sanitised().pick(&[0.0, 5.0, 1.0]), 1);
    }

    #[test]
    fn stochastic_sampling_stays_inside_the_logits_and_is_replayable() {
        let s = Sampling::QWEN_NO_THINK;
        let logits: Vec<f32> = (0..100).map(|i| (i % 7) as f32 - 3.0).collect();
        // Same seed, same token, twice; and always a valid index.
        let a = s.pick(&logits);
        let b = s.pick(&logits);
        assert_eq!(a, b);
        assert!((a as usize) < logits.len());
        // A one-hot distribution cannot escape its own peak.
        let mut hot = vec![f32::NEG_INFINITY; 10];
        hot[7] = 12.0;
        for seed in 0..8 {
            let s = Sampling {
                greedy: false,
                temperature: 0.7,
                top_k: 20,
                top_p: 0.8,
                seed,
            };
            assert_eq!(s.pick(&hot), 7);
        }
    }

    #[test]
    fn the_stopper_ends_on_eos_cap_or_cancel_in_that_order_of_truth() {
        let cancelled = flag();
        let stopper = Stopper::new(vec![151_645, 151_643], 4, cancelled.clone());
        assert!(stopper.continues(0));
        assert!(stopper.ends_turn(151_645));
        assert!(!stopper.ends_turn(42));
        assert!(stopper.continues(3));
        assert!(!stopper.continues(4), "the token cap is a wall, not a mood");
        assert_eq!(stopper.reason(4, false), StopReason::TokenCap);
        assert_eq!(stopper.reason(4, true), StopReason::Finished);
        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(!stopper.continues(0));
        assert_eq!(stopper.reason(2, false), StopReason::Cancelled);
    }
}
