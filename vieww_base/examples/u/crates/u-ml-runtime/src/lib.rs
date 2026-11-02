//! `u-ml-runtime` — the seam a model arrives through, the engine already
//! behind it, and — behind the `model` feature — the model itself.
//!
//! The plan's Phase 2 is *Candle + Qwen3-0.6B behind a measured baseline*,
//! and this crate is that sentence in the order the plan's §11 guiding
//! rule insists on (the universal decision experience **first**, the
//! measured on-device inference second):
//!
//! * [`Runtime`] — the one trait every engine implements: read an input
//!   and the persona hints relevant to it, produce the §4.4 assessment.
//! * [`Deterministic`] — the engine that shipped first: `u-decision`'s
//!   cue reader and composer, enforcing the same [`Budget`] a model
//!   will. It is the §5.6 fallback ("deterministic fallback behaviour
//!   when inference fails") *and* the quality floor any model must beat
//!   before it replaces this behind the same trait.
//! * [`Manifest`] / [`TokenizerManifest`] / [`Quant`] — §5.1's
//!   verification language: family, parameters, quantisation, the exact
//!   revision, a SHA-256 digest, and the licence the bytes carry. The
//!   three manifests the plan's §5.2 table asks to compare are committed
//!   at `models/manifests/` and embedded here; the weights stay outside
//!   the repository, fetched by a developer (see `docs/models.md`) and
//!   digest-checked at load.
//! * [`cohort`] — §5.5's device cohorts, read from `/proc/meminfo`,
//!   each carrying the caps §5.6 demands.
//!
//! # The `model` feature
//!
//! With `--features model` the crate grows the rest of §5.3's checklist:
//! [`qwen`] is Qwen3-0.6B in GGUF on Candle's CPU device — loaded only
//! after its digest matches its manifest, running on a worker thread
//! with a cancellation token and a timeout, emitting contract-validated
//! structured output or an honest error. A default build (and every APK
//! build today) resolves *none* of that: the plan's §10 release gates
//! forbid shipping a model/runtime combination that has not been
//! measured on real devices, and Phase 3's physical-device reports do
//! not exist yet. The seam is the same either way — that is the point of
//! a seam.
//!
//! # The safeguards that are real in every build
//!
//! The budget is enforced *at this layer*, not inside whichever engine —
//! input caps, output caps, single-shot calls (no unbounded retries,
//! §5.6), and errors a caller can act on. The day a model runs hot, the
//! caps are already the app's, not the model's.

use u_core::{Budget, Error};
use u_decision::{compose, interpret, Assessment, Hint, Input, Interpretation};

pub mod cohort;
pub mod generation;
#[cfg(feature = "model")]
pub mod model;
pub mod prompt;
pub mod quantization;
#[cfg(feature = "model")]
pub mod qwen;
#[cfg(feature = "model")]
pub mod tokenizer;

pub use quantization::{Manifest, Quant, TokenizerManifest};
#[cfg(feature = "model")]
pub use qwen::Qwen;

/// One engine behind the whole app: read a situation, write an assessment.
///
/// Single-shot by design — the caller asks once, gets one answer or one
/// error, and decides what to do. No retries here, no polling, no hidden
/// second attempt: §5.6's "avoid unbounded retries" starts at the type's
/// own shape. When the model runtime lands, *it* will own a worker thread
/// and a cancellation token internally; this trait's contract stays the
/// same, because the screens are already built against it.
pub trait Runtime {
    /// The engine's own name — what a report prints and a settings screen
    /// can show honestly.
    fn name(&self) -> &'static str;

    /// Read one input into the §4.2 representation.
    ///
    /// The hints are the persona's *relevant* preferences (§7.5), passed
    /// to both stages so each engine can decide where personalisation
    /// belongs — the deterministic engine folds them in here, because
    /// cues are all it has; a model engine reads the situation without
    /// them and saves them for [`Self::assess`], where §7.5 puts them.
    ///
    /// # Errors
    /// [`Error::Empty`] / [`Error::OverBudget`] from the budget, and
    /// whatever an engine's own failure mode is — as an [`Error`], never a
    /// panic, because a phone discards stderr.
    fn interpret(&self, input: &Input, hints: &[Hint]) -> Result<Interpretation, Error>;

    /// Compose the §4.4 assessment for an interpretation, with the
    /// persona's relevant preferences in view.
    ///
    /// # Errors
    /// Contract violations, as the engine's own error — validation is
    /// §5.6's "valid structured output or tested recovery".
    fn assess(&self, seen: &Interpretation, hints: &[Hint]) -> Result<Assessment, Error>;

    /// The whole pipeline in one call: input in, assessment out.
    ///
    /// # Errors
    /// As [`Self::interpret`] and [`Self::assess`].
    fn decide(&self, input: &Input, hints: &[Hint]) -> Result<Assessment, Error> {
        let seen = self.interpret(input, hints)?;
        self.assess(&seen, hints)
    }

    /// Abandon the work in flight, if there is any and if the engine can.
    ///
    /// §5.6's "cancel work when requested or when the user leaves". An
    /// engine whose work is a cue read over a few hundred bytes has
    /// nothing to cancel — the default is the honest no-op. An engine
    /// that owns a worker notices between tokens and stops; the call the
    /// caller already holds comes back as [`Error::Cancelled`], which is
    /// the caller's signal to abandon the *result*, not to retry it.
    fn cancel(&self) {}
}

/// The deterministic baseline: `u-decision` behind [`Runtime`], enforcing
/// the budget exactly as a model runtime will.
///
/// Its quality is bounded — it reads cues, not meaning — and that is the
/// point: it is the floor, the fallback, and the thing that makes every
/// screen work today, so the model that replaces it has something to beat
/// and the app has something to fall back to. Its name is a claim about
/// exactly one thing: the same input always produces the same output.
#[derive(Debug, Clone)]
pub struct Deterministic {
    budget: Budget,
}

impl Default for Deterministic {
    fn default() -> Self {
        Self::new(Budget::DEFAULT)
    }
}

impl Deterministic {
    /// A deterministic engine under an explicit budget.
    #[must_use]
    pub const fn new(budget: Budget) -> Self {
        Self { budget }
    }

    /// The budget this engine enforces.
    #[must_use]
    pub const fn budget(&self) -> Budget {
        self.budget
    }
}

impl Runtime for Deterministic {
    fn name(&self) -> &'static str {
        "deterministic-baseline"
    }

    fn interpret(&self, input: &Input, hints: &[Hint]) -> Result<Interpretation, Error> {
        input.check(&self.budget)?;
        Ok(interpret(input, hints, &self.budget))
    }

    fn assess(&self, seen: &Interpretation, _hints: &[Hint]) -> Result<Assessment, Error> {
        // Hints were already folded into the interpretation by this
        // engine's own `interpret` — a cue reader has no second chance to
        // weigh them, so it takes the first, and the `_` says so.
        let assessment = compose(seen, &self.budget);
        if assessment.contract_violations().is_empty() {
            Ok(assessment)
        } else {
            // The composer is ours and in-crate; a violation here is a bug
            // in the composer, reported as a schema error rather than
            // silently shipped — §5.6's recovery rule, applied at the
            // seam where a model's violations will one day arrive instead.
            Err(Error::Schema {
                found: assessment.schema,
                reads: u_decision::SCHEMA_VERSION,
            })
        }
    }
}

/// The assessment as its schema-carrying JSON — the shape [`u_memory`]
/// persists and the shape a model runtime will be required to emit before
/// it is trusted.
///
/// # Errors
/// Serialisation failure, as [`Error::Corrupt`] — the record, not the
/// engine, is what is broken in that case.
pub fn to_json(assessment: &Assessment) -> Result<serde_json::Value, Error> {
    serde_json::to_value(assessment).map_err(|_| Error::Corrupt { recovered: 0 })
}

/// Parse an assessment from its JSON, refusing any schema this build does
/// not read.
///
/// # Errors
/// [`Error::Corrupt`] when the JSON is not an assessment at all;
/// [`Error::Schema`] when it is a future version's.
pub fn from_json(value: &serde_json::Value) -> Result<Assessment, Error> {
    let assessment: Assessment =
        serde_json::from_value(value.clone()).map_err(|_| Error::Corrupt { recovered: 0 })?;
    if assessment.schema != u_decision::SCHEMA_VERSION {
        return Err(Error::Schema {
            found: assessment.schema,
            reads: u_decision::SCHEMA_VERSION,
        });
    }
    Ok(assessment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(situation: &str) -> Input {
        Input {
            situation: situation.to_owned(),
            ..Input::default()
        }
    }

    #[test]
    fn the_baseline_decides_the_plan_own_example() {
        let runtime = Deterministic::default();
        let assessment = runtime
            .decide(
                &input(
                    "I'm considering moving to another city, but I'm not sure whether the \
                     change is worth it.",
                ),
                &[],
            )
            .expect("the deterministic engine always decides");
        assert!(assessment.contract_violations().is_empty());
        assert_eq!(runtime.name(), "deterministic-baseline");
    }

    #[test]
    fn the_budget_is_enforced_at_the_seam_not_inside_the_engine() {
        let runtime = Deterministic::default();
        assert_eq!(
            runtime.decide(&input("   "), &[]),
            Err(Error::Empty("the situation"))
        );
        let over = "word ".repeat(Budget::DEFAULT.max_input_chars);
        assert!(matches!(
            runtime.decide(&input(&over), &[]),
            Err(Error::OverBudget { .. })
        ));
    }

    #[test]
    fn the_same_input_always_gives_the_same_output() {
        let runtime = Deterministic::default();
        let one = runtime.decide(&input("Should I take the offer or stay?"), &[]);
        let two = runtime.decide(&input("Should I take the offer or stay?"), &[]);
        assert_eq!(one, two, "determinism is the engine's one claim");
    }

    #[test]
    fn assessments_round_trip_through_json_with_their_schema() {
        let runtime = Deterministic::default();
        let assessment = runtime
            .decide(&input("Should I switch teams at work?"), &[])
            .unwrap();
        let json = to_json(&assessment).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(back, assessment);
    }

    #[test]
    fn a_future_schema_is_refused_on_read_as_on_write() {
        let runtime = Deterministic::default();
        let assessment = runtime.decide(&input("anything at all"), &[]).unwrap();
        let mut json = to_json(&assessment).unwrap();
        json["schema"] = serde_json::json!(u_decision::SCHEMA_VERSION + 1);
        assert!(matches!(from_json(&json), Err(Error::Schema { .. })));
    }

    #[test]
    fn the_quant_table_is_the_plan_own() {
        assert!((Quant::Q2K.effective_bits() - 2.625).abs() < f32::EPSILON);
        assert!((Quant::Q3K.effective_bits() - 3.4375).abs() < f32::EPSILON);
        assert!((Quant::Q4K.effective_bits() - 4.5).abs() < f32::EPSILON);
        assert_eq!(Quant::Q4K.label(), "Q4_K");
    }

    #[test]
    fn a_manifest_summarises_itself_honestly() {
        // The embedded, real manifest — not a hand-built one, so the
        // summary is checked against the thing a report will actually
        // print.
        let manifest = &Manifest::embedded()[1];
        let summary = manifest.summary();
        assert!(summary.contains("qwen3-0.6B"));
        assert!(summary.contains("Q3_K"));
        assert!(summary.contains("3.4375"));
    }
}
