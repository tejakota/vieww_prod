//! `u-decision` — the universal decision engine.
//!
//! A person brings any situation, in their own words, with no category
//! chosen and no template filled. [`interpret`] reads it into an
//! [`Interpretation`] — the flexible representation the plan's §4.2 draws —
//! and [`compose`] writes the [`Assessment`] the plan's §4.4 contracts.
//!
//! # What this crate is, honestly
//!
//! This is the **deterministic baseline**: cue words, sentence shapes, and
//! templates that are true by construction. It does not understand language;
//! it recognises the shapes decisions are written in. It is deliberately
//! the engine the plan's §5.6 demands as the fallback when a model is
//! missing, loaded late, or failed — which makes it the *first* engine to
//! ship, the one every screen works against, and the floor any model has to
//! beat before it replaces this behind the same trait.
//!
//! Its promises, which a model runtime inherits:
//!
//! * **No categories gate anything.** Tags are inferred, optional, and never
//!   asked for (§1, §2.1).
//! * **No invented numbers.** Not a percentage, not a score, not a rating —
//!   "avoid false precision" is §2.4's own rule, and the types have nowhere
//!   to put one.
//! * **Suggested options are marked as suggested**, never passed off as the
//!   person's own (§2.4).
//! * **One follow-up question at most**, and only when a missing fact
//!   materially changes the assessment (§2.5).
//! * **Facts, claims, assumptions and inferences stay separate** — the
//!   unknowns and assumptions lists exist so the assessment never has to
//!   smuggle a guess into a declarative sentence (§4.3).

use u_core::{Budget, Error};

mod interpret;
mod compose;

pub use compose::compose;
pub use interpret::interpret;

/// The shape of the plan's §4.2 representation, versioned.
pub const SCHEMA_VERSION: u32 = 1;

/// One way the situation could go.
///
/// `origin` keeps U's suggestions honest: a person's own option and an
/// option U thought of are different facts and are never shown as the same
/// kind of thing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Option_ {
    /// The option, in the person's own words where they gave it.
    pub label: String,
    /// Where it came from.
    pub origin: Origin,
}

/// Where an option came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Origin {
    /// The person wrote it, or it stands in the situation they wrote.
    Theirs,
    /// U suggested it; it is an inference, not a fact about their life.
    Suggested,
}

/// How sure the engine is, and why — as words, never as a number.
///
/// A percentage here would be false precision about U's own certainty, which
/// is the one thing U must never perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Confidence {
    /// Read as: act on this only with that in mind.
    Low,
    /// Read as: the shape is right; specifics may not be.
    Medium,
    /// Read as: the situation and its parts were clear.
    High,
}

/// What the engine understood (§4.2's illustration, made real).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Interpretation {
    /// The schema this record was written for.
    pub schema: u32,
    /// The situation, normalised — trimmed, whitespace collapsed, nothing
    /// else touched. The person's words are evidence, not material.
    pub situation: String,
    /// What kind of thing is being decided, as a short plain phrase.
    pub intent: String,
    /// Every option seen: theirs first, U's suggestions after.
    pub options: Vec<Option_>,
    /// What bounds the choice — money, time, commitments, place, people.
    pub constraints: Vec<String>,
    /// What matters in this context, most-important first.
    pub priorities: Vec<String>,
    /// Situational context worth carrying, as plain `key: value` lines.
    pub context: Vec<String>,
    /// The horizon this plays out over, when the words said so.
    pub time_horizon: Option<String>,
    /// Missing facts that could change the assessment.
    pub unknowns: Vec<String>,
    /// What the engine proceeded assuming, stated.
    pub assumptions: Vec<String>,
    /// Optional inferred tags — labels for retrieval, never gates.
    pub optional_tags: Vec<String>,
    /// How sure, and the one-line reason.
    pub confidence: Confidence,
    /// The one-line reason for [`Self::confidence`].
    pub confidence_reason: String,
}

impl Interpretation {
    /// A new interpretation, carrying the schema version with it.
    #[must_use]
    pub fn new(situation: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            situation: situation.into(),
            intent: String::new(),
            options: Vec::new(),
            constraints: Vec::new(),
            priorities: Vec::new(),
            context: Vec::new(),
            time_horizon: None,
            unknowns: Vec::new(),
            assumptions: Vec::new(),
            optional_tags: Vec::new(),
            confidence: Confidence::Low,
            confidence_reason: String::new(),
        }
    }
}

/// The plan's §4.4 output contract, made real and versioned.
///
/// Every field is plain prose or a list of plain prose. There is no field
/// for a score, a percentage, or a ranking — the contract itself refuses
/// false precision.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Assessment {
    /// The contract version this assessment satisfies.
    pub schema: u32,
    /// What U takes the situation and the decision to be.
    pub understanding: String,
    /// User-provided options and U's suggestions, distinguished.
    pub options: Vec<Option_>,
    /// What matters here, as this assessment sees it.
    pub relevant_priorities: Vec<String>,
    /// How the options compare against those priorities, qualitatively.
    pub tradeoffs: Vec<String>,
    /// Missing facts that could change this assessment.
    pub unknowns: Vec<String>,
    /// What was assumed to say anything at all.
    pub assumptions: Vec<String>,
    /// The reasoned assessment itself, assumptions included.
    pub assessment: String,
    /// Confidence in the above, as a word.
    pub confidence: Confidence,
    /// The practical next step: an action, a question, or a reason to wait.
    pub next_step: String,
    /// The one clarification worth asking, if any.
    pub follow_up_question: Option<String>,
}

impl Assessment {
    /// The parts of the contract that must not be empty for the record to
    /// mean anything. Empty lists are allowed wherever the plan allows them
    /// — a decision with no options yet is a *real state*, not a broken
    /// one — but these two sentences are the assessment's spine.
    pub const REQUIRED_PROSE: [&'static str; 2] = ["understanding", "next_step"];

    /// Every way this record violates the §4.4 contract, by field name.
    ///
    /// Empty when the record is valid. Validation lives *with* the type
    /// because the contract is the type's whole reason to exist, and a
    /// validator that can drift from the shape it checks is a promise that
    /// silently expires.
    #[must_use]
    pub fn contract_violations(&self) -> Vec<&'static str> {
        let mut broken = Vec::new();
        if self.understanding.trim().is_empty() {
            broken.push(Self::REQUIRED_PROSE[0]);
        }
        if self.next_step.trim().is_empty() {
            broken.push(Self::REQUIRED_PROSE[1]);
        }
        if self.schema != SCHEMA_VERSION {
            broken.push("schema");
        }
        broken
    }
}

/// What a person brings to a decision: the situation, and whatever optional
/// structure they felt like giving (§2.3).
///
/// Only `situation` is required. Everything else may be empty, and the
/// engine proceeds with stated assumptions rather than demanding fields —
/// "only the situation description is needed to begin".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    /// The free-form situation. Required.
    pub situation: String,
    /// Options the person is already considering, one per entry.
    pub options: Vec<String>,
    /// What matters most to them here, most important first.
    pub matters: Vec<String>,
    /// Constraints they named: budget, timing, commitments.
    pub constraints: Vec<String>,
    /// Any other context or notes.
    pub notes: String,
}

impl Input {
    /// Check the input against a [`Budget`].
    ///
    /// # Errors
    /// [`Error::Empty`] for a blank situation, [`Error::OverBudget`] for one
    /// past the character cap.
    pub fn check(&self, budget: &Budget) -> Result<(), Error> {
        budget.check_input(&self.situation)
    }
}

/// A persona preference the engine is allowed to use, passed in as a plain
/// hint rather than a persona crate type.
///
/// [`u-decision`] deliberately does not depend on `u-persona`: the engine
/// consumes *evidence about a person*, not a data structure that happens to
/// live in another crate — which is what keeps both replaceable and both
/// testable alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    /// The dimension, as a plain label ("stability", "risk tolerance", …).
    pub dimension: String,
    /// The direction the person leans, `-1..=1`.
    pub leaning: f32,
    /// The context the preference was learned in, if it is not global.
    pub context: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_contract_valid_assessment_reports_no_violations() {
        let assessment = Assessment {
            schema: SCHEMA_VERSION,
            understanding: "You are weighing a move.".into(),
            options: Vec::new(),
            relevant_priorities: vec!["money".into()],
            tradeoffs: Vec::new(),
            unknowns: Vec::new(),
            assumptions: vec!["the situation is as described".into()],
            assessment: "…".into(),
            confidence: Confidence::Medium,
            next_step: "List what a good month looks like.".into(),
            follow_up_question: None,
        };
        assert!(assessment.contract_violations().is_empty());
    }

    #[test]
    fn an_empty_spine_is_a_named_contract_violation() {
        let assessment = Assessment {
            schema: SCHEMA_VERSION,
            understanding: "  ".into(),
            options: Vec::new(),
            relevant_priorities: Vec::new(),
            tradeoffs: Vec::new(),
            unknowns: Vec::new(),
            assumptions: Vec::new(),
            assessment: String::new(),
            confidence: Confidence::Low,
            next_step: String::new(),
            follow_up_question: None,
        };
        let broken = assessment.contract_violations();
        assert!(broken.contains(&"understanding"));
        assert!(broken.contains(&"next_step"));
    }

    #[test]
    fn a_schema_from_the_future_is_refused_by_name() {
        let mut assessment = Assessment {
            schema: SCHEMA_VERSION,
            understanding: "x".into(),
            options: Vec::new(),
            relevant_priorities: Vec::new(),
            tradeoffs: Vec::new(),
            unknowns: Vec::new(),
            assumptions: Vec::new(),
            assessment: "x".into(),
            confidence: Confidence::Low,
            next_step: "x".into(),
            follow_up_question: None,
        };
        assessment.schema = SCHEMA_VERSION + 1;
        assert!(assessment.contract_violations().contains(&"schema"));
    }

    #[test]
    fn an_empty_situation_is_refused_by_the_budget() {
        let input = Input::default();
        assert_eq!(
            input.check(&Budget::DEFAULT),
            Err(Error::Empty("the situation"))
        );
    }
}
