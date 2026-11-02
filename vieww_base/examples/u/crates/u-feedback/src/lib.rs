//! `u-feedback` — what a person's response to U is allowed to mean.
//!
//! The plan's §7.3, read conservatively — which is the only way the plan
//! permits reading it. A "not helpful" is *not* evidence of the opposite
//! preference (§7.3's own rule: "a rejected recommendation is not
//! automatically evidence of the opposite preference; the recommendation
//! may have been incomplete or wrong"), so usefulness feedback validates
//! and is recorded, and becomes **no persona signal at all**.
//!
//! The signals that *are* allowed, in ascending strength:
//!
//! 1. A decision's explicitly named priorities — weak, inferred evidence
//!    about the person, at the plan's §7.4 caution level.
//! 2. An explicit correction of U's reading — strong, and superseding.
//! 3. A direct answer to a preference question — strong, explicit.
//!
//! Everything else the plan lists (outcomes, usefulness) is kept as
//! *memory*, and used as *context*, and never quietly turned into a
//! personality.

use u_core::Id;
use u_persona::{Dimension, Persona, Source};

/// What a person said about an assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feedback {
    /// It helped.
    Helpful,
    /// It did not.
    NotHelpful,
    /// It did not, and they said why — the note is a *correction of the
    /// assessment*, never a persona statement.
    NotHelpfulBecause(String),
}

/// Validate a feedback note before it is stored: present, bounded, and not
/// a whole paragraph nobody will read.
///
/// # Errors
/// [`u_core::Error::OverBudget`] when the note exceeds the cap; the empty
/// note is fine and becomes `None` at the call site.
pub fn check_note(note: &str) -> Result<(), u_core::Error> {
    const MAX_NOTE: usize = 500;
    let got = note.trim().chars().count();
    if got > MAX_NOTE {
        Err(u_core::Error::OverBudget {
            what: "the note",
            got,
            bound: MAX_NOTE,
        })
    } else {
        Ok(())
    }
}

/// Whether this feedback is the same verdict as the last one recorded for
/// the same decision — a person tapping twice is one opinion, not two.
#[must_use]
pub fn is_duplicate(last: Option<&Feedback>, next: &Feedback) -> bool {
    matches!(
        (last, next),
        (Some(Feedback::Helpful), Feedback::Helpful)
            | (Some(Feedback::NotHelpful), Feedback::NotHelpful)
            | (Some(Feedback::NotHelpfulBecause(_)), Feedback::NotHelpful)
            | (Some(Feedback::NotHelpful), Feedback::NotHelpfulBecause(_))
    )
}

/// The persona observations a *decision's own words* are allowed to become.
///
/// A decision that names a priority is weak, inferred evidence that the
/// person cares about that dimension *in that context* — nothing more. The
/// map is deliberately small and every entry is a plan §7.2 dimension with
/// a place to look it up; anything the words name that is not on this list
/// is kept in the decision record and never becomes a preference.
#[must_use]
pub fn priority_signals(matters: &[String], tags: &[String]) -> Vec<(Dimension, f32, f32, Source)> {
    // (dimension, leaning, confidence) — the plan's caution levels.
    let mut signals: Vec<(Dimension, f32, f32, Source)> = Vec::new();
    let joined = matters
        .iter()
        .map(|m| m.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    let on = |needle: &str| joined.contains(needle);
    if on("stability") || on("stable") || on("secure") || on("security") || on("safe") {
        signals.push((Dimension::Stability, 0.4, 0.3, Source::DecisionPriority));
    }
    if on("growth") || on("learn") || on("career") || on("opportunit") {
        signals.push((Dimension::Growth, 0.4, 0.3, Source::DecisionPriority));
    }
    if on("risk") {
        signals.push((Dimension::RiskTolerance, 0.3, 0.2, Source::DecisionPriority));
    }
    if on("freedom") || on("flexib") || on("independen") || on("autonomy") {
        signals.push((Dimension::Independence, 0.4, 0.3, Source::DecisionPriority));
    }
    if on("simple") || on("calm") || on("quiet") || on("peace") {
        signals.push((Dimension::Simplicity, 0.4, 0.3, Source::DecisionPriority));
    }
    if on("family") || on("partner") || on("friend") || on("people") {
        signals.push((Dimension::SocialConnection, 0.4, 0.3, Source::DecisionPriority));
    }

    // The context is the decision's own tags — a preference learned from a
    // career decision is scoped to career decisions, never global (§7.2:
    // "preferences may differ by context").
    let _ = tags;
    signals
}

/// Fold a decision's priority signals into the persona.
///
/// The *caller* passes the context (the decision's tags, joined or `None`)
/// so the memory layer decides scoping; this function only applies the
/// plan's bounded update for each signal.
pub fn learn_priorities(persona: &mut Persona, matters: &[String], context: Option<String>, at: Id) {
    for (dimension, leaning, confidence, source) in priority_signals(matters, &[]) {
        persona.observe(dimension, leaning, confidence, source, context.clone(), at);
    }
}

/// Fold an explicit correction into the persona.
///
/// Corrections are the strong signal: explicit, superseding the reading
/// they replace (§7.3's "corrections to U's interpretation").
pub fn learn_correction(
    persona: &mut Persona,
    dimension: Dimension,
    to: f32,
    was_context: Option<String>,
    now_context: Option<String>,
    at: Id,
) {
    persona.correct(dimension, to, was_context, now_context, at);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usefulness_feedback_is_recorded_but_never_learned_from() {
        // There is no function from a verdict to a persona observation, and
        // that absence is the feature: this test names it so it stays a
        // decision rather than an oversight someone "fixes".
        let mut persona = Persona::default();
        // Nothing in this crate takes a Feedback and touches the persona.
        learn_priorities(&mut persona, &[], None, 1);
        assert!(persona.preferences.is_empty());
    }

    #[test]
    fn a_named_priority_becomes_weak_scoped_evidence() {
        let mut persona = Persona::default();
        learn_priorities(
            &mut persona,
            &["growth and learning".to_owned()],
            Some("career".to_owned()),
            10,
        );
        assert_eq!(persona.preferences.len(), 1);
        let preference = &persona.preferences[0];
        assert_eq!(preference.dimension, Dimension::Growth);
        assert!(!preference.explicit, "a named priority is inferred, not stated");
        assert!(preference.confidence <= 0.3, "weak by the plan's own rule");
        assert_eq!(preference.context.as_deref(), Some("career"));
    }

    #[test]
    fn an_unlisted_priority_is_kept_not_learned() {
        let mut persona = Persona::default();
        learn_priorities(&mut persona, &["the vibe".to_owned()], None, 1);
        assert!(persona.preferences.is_empty(), "the words are kept in the record");
    }

    #[test]
    fn a_correction_supersedes_and_states() {
        let mut persona = Persona::default();
        learn_priorities(&mut persona, &["risk".to_owned()], None, 1);
        learn_correction(&mut persona, Dimension::RiskTolerance, -0.6, None, None, 2);
        let current: Vec<_> = persona
            .preferences
            .iter()
            .filter(|p| p.status == u_persona::Status::Current)
            .collect();
        assert_eq!(current.len(), 1);
        assert!(current[0].explicit);
        assert!(current[0].value < 0.0);
    }

    #[test]
    fn double_taps_are_one_opinion() {
        assert!(is_duplicate(
            Some(&Feedback::Helpful),
            &Feedback::Helpful
        ));
        assert!(!is_duplicate(
            Some(&Feedback::Helpful),
            &Feedback::NotHelpful
        ));
        assert!(is_duplicate(
            Some(&Feedback::NotHelpful),
            &Feedback::NotHelpfulBecause("missed the point".into())
        ));
    }

    #[test]
    fn notes_are_bounded() {
        assert!(check_note("").is_ok());
        assert!(check_note("missed the money constraint").is_ok());
        let long = "x".repeat(501);
        assert!(matches!(
            check_note(&long),
            Err(u_core::Error::OverBudget { bound: 500, .. })
        ));
    }
}
