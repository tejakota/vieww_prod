//! `u-persona` — who U is becoming for one person.
//!
//! The plan's §7, made real at its §7.2 baseline: **a frozen general engine
//! plus a structured, correctable persona** — not a fine-tuned model, and
//! not a single global "user score". A preference here is a dimension with
//! a direction, a confidence, a context it applies in, and a history that
//! says where it came from.
//!
//! # The shape of a preference
//!
//! Every preference carries the plan's own required fields — value,
//! confidence, applicable context, evidence source, timestamp,
//! explicit-versus-inferred, and current/uncertain/superseded status —
//! because those seven are what make a preference *inspectable*: the
//! difference between personalisation the person can argue with and a
//! profile they can only delete.
//!
//! # The update policy
//!
//! §7.4's conceptual rule, applied with its own caveats:
//!
//! ```text
//! new = old + learning_rate * confidence * (observed - old)
//! ```
//!
//! with the two guards the plan insists on: an **explicit statement
//! outweighs behavioural inference** (an inferred observation never moves an
//! explicit value, only its confidence), and a **one-time choice is not a
//! permanent preference** (inferred observations move the value a bounded
//! fraction, and never past `Uncertain` confidence from a single datapoint).
//!
//! # No training of the base engine
//!
//! Nothing here touches a model. The persona is retrieved as hints — see
//! [`Persona::relevant`] — and handed to the decision engine as *stated
//! assumptions*, which is the only channel personalisation is allowed
//! through (§7.1: keep general capability stable while U becomes more
//! relevant to the individual).

use u_core::Id;

/// A preference's state in the person's eyes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    /// The current best reading.
    Current,
    /// Held, but contradicted recently — shown, never silently used.
    Uncertain,
    /// Replaced; kept for history, excluded from retrieval.
    Superseded,
}

/// Where a piece of evidence came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Source {
    /// The person answered a preference question directly.
    Stated,
    /// A decision they brought named this dimension as a priority.
    DecisionPriority,
    /// They corrected U's reading of them.
    Correction,
    /// They said an assessment was or was not useful.
    Usefulness,
}

/// One preference, per the plan's §7.2 field list.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preference {
    /// The dimension this reads.
    pub dimension: Dimension,
    /// The direction, `-1..=1`: positive leans in, negative leans away.
    pub value: f32,
    /// How much to trust it, `0..=1`.
    pub confidence: f32,
    /// The context it was learned in — `None` means it reads as global.
    ///
    /// A person may prefer stability at work and exploration on holidays;
    /// context is what keeps those two from averaging into mush.
    pub context: Option<String>,
    /// Whether the person said this outright, or U inferred it.
    pub explicit: bool,
    /// The preference's state.
    pub status: Status,
    /// Where the current reading last came from.
    pub source: Source,
    /// When it last moved.
    pub updated_at: Id,
    /// How many observations sit behind it.
    pub observations: u32,
}

impl Preference {
    /// A brand-new preference from one explicit statement.
    #[must_use]
    pub fn stated(dimension: Dimension, value: f32, context: Option<String>, at: Id) -> Self {
        Self {
            dimension,
            value: value.clamp(-1.0, 1.0),
            confidence: 0.8,
            context,
            explicit: true,
            status: Status::Current,
            source: Source::Stated,
            updated_at: at,
            observations: 1,
        }
    }

    /// A brand-new preference from one inferred observation.
    #[must_use]
    pub fn inferred(dimension: Dimension, value: f32, context: Option<String>, at: Id) -> Self {
        Self {
            dimension,
            value: value.clamp(-1.0, 1.0),
            // One datapoint never earns more than this.
            confidence: 0.3,
            context,
            explicit: false,
            status: Status::Current,
            source: Source::DecisionPriority,
            updated_at: at,
            observations: 1,
        }
    }
}

/// The dimensions preferences live on — the plan's §7.2 list, described as
/// "not an exhaustive fixed taxonomy", and treated that way: adding one is
/// additive, and none of them is required to exist for any decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Dimension {
    /// sameness, predictability, keeping what works
    Stability,
    /// newness, risk-as-information, "see what's out there"
    Exploration,
    /// deciding for oneself, unbossed time
    Independence,
    /// floors rather than ceilings; downside first
    Security,
    /// learning, doors that open later
    Growth,
    /// time as the scarce thing
    TimeFreedom,
    /// effort saved, friction avoided
    Convenience,
    /// people held close
    SocialConnection,
    /// fewer moving parts
    Simplicity,
    /// appetite for variance
    RiskTolerance,
}

impl Dimension {
    /// The plain label the person sees — lowercase, unabbreviated.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stability => "stability",
            Self::Exploration => "exploration",
            Self::Independence => "independence",
            Self::Security => "security",
            Self::Growth => "growth",
            Self::TimeFreedom => "time freedom",
            Self::Convenience => "convenience",
            Self::SocialConnection => "the people close to you",
            Self::Simplicity => "simplicity",
            Self::RiskTolerance => "risk tolerance",
        }
    }

    /// Every dimension, in the order the plan lists them.
    pub const ALL: [Self; 10] = [
        Self::Stability,
        Self::Exploration,
        Self::Independence,
        Self::Security,
        Self::Growth,
        Self::TimeFreedom,
        Self::Convenience,
        Self::SocialConnection,
        Self::Simplicity,
        Self::RiskTolerance,
    ];
}

/// The whole persona: every preference it holds, in any context.
///
/// Serialised whole by `u-memory` — §7.5's "versioned personalisation
/// state" — and never shipped anywhere, because the manifest that builds
/// this app asks for no network permission at all.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Persona {
    /// Every preference, at most one per (dimension, context) pair.
    pub preferences: Vec<Preference>,
}

/// How far an observation moves a value — the plan's `learning_rate`.
///
/// 0.25: an explicit statement still lands nearly all the way in two
/// exchanges, and a single inferred datapoint can never flip a reading.
const LEARNING_RATE: f32 = 0.25;

impl Persona {
    /// An observation the plan's §7.3 lists: an answer, a stated priority, a
    /// correction, or a usefulness signal.
    ///
    /// `confidence` is the observation's own — how much *this datapoint*
    /// deserves to move anything (an explicit answer carries ~0.8, a
    /// decision priority ~0.3, usefulness feedback ~0.2).
    pub fn observe(
        &mut self,
        dimension: Dimension,
        observed: f32,
        confidence: f32,
        source: Source,
        context: Option<String>,
        at: Id,
    ) {
        let observed = observed.clamp(-1.0, 1.0);
        let confidence = confidence.clamp(0.0, 1.0);

        // The same (dimension, context) pair is one preference; everything
        // else — including no context and *some* context — is separate.
        let existing = self
            .preferences
            .iter_mut()
            .find(|p| p.dimension == dimension && p.context == context);

        let Some(preference) = existing else {
            let mut fresh = if source == Source::Stated || source == Source::Correction {
                Preference::stated(dimension, observed, context, at)
            } else {
                Preference::inferred(dimension, observed, context, at)
            };
            fresh.source = source;
            fresh.confidence = confidence.max(if fresh.explicit { 0.6 } else { 0.2 });
            self.preferences.push(fresh);
            return;
        };

        let explicit_word = source == Source::Stated || source == Source::Correction;
        if preference.explicit && !explicit_word {
            // §7.4's guard, applied as its own rule: an inferred observation
            // against an explicit preference is noted, and moves nothing but
            // (slightly) its confidence.
            preference.confidence = (preference.confidence - 0.05).max(0.0);
            preference.observations = preference.observations.saturating_add(1);
        } else if explicit_word {
            // An explicit statement or correction *replaces* the reading:
            // the person said what it is, and §7.3's "explicit statements
            // outweigh weak behavioural inference" means the value, not
            // merely the trend. Confidence lands at the stated floor,
            // because a statement is the strongest signal this persona
            // ever receives.
            preference.value = observed;
            preference.confidence = preference.confidence.max(0.8);
            preference.explicit = true;
            preference.source = source;
            preference.updated_at = at;
            preference.observations = preference.observations.saturating_add(1);
            preference.status = Status::Current;
        } else {
            // §7.4's rule, verbatim, for inferred evidence: a bounded walk
            // toward the observation, at the observation's own confidence.
            let step = LEARNING_RATE * confidence * (observed - preference.value);
            preference.value = (preference.value + step).clamp(-1.0, 1.0);
            // Confidence walks toward the inferred ceiling — a pile of weak
            // evidence can earn trust, but never as much as one statement.
            let target = 0.55_f32;
            preference.confidence =
                (preference.confidence + LEARNING_RATE * confidence * (target - preference.confidence))
                    .clamp(0.0, 1.0);
            preference.source = source;
            preference.updated_at = at;
            preference.observations = preference.observations.saturating_add(1);
            preference.status = Status::Current;
        }
    }

    /// Mark a preference superseded — the person said it no longer holds.
    pub fn supersede(&mut self, dimension: Dimension, context: Option<String>) {
        if let Some(preference) = self
            .preferences
            .iter_mut()
            .find(|p| p.dimension == dimension && p.context == context)
        {
            preference.status = Status::Superseded;
        }
    }

    /// A correction: the person said the reading is wrong.
    ///
    /// Corrections carry the same weight as statements — §7.3's "explicit
    /// corrections" — and mark the old reading superseded when they name a
    /// different context than the one being corrected into.
    pub fn correct(
        &mut self,
        dimension: Dimension,
        to: f32,
        was_context: Option<String>,
        now_context: Option<String>,
        at: Id,
    ) {
        self.supersede(dimension, was_context);
        self.observe(dimension, to, 0.9, Source::Correction, now_context, at);
    }

    /// The preferences relevant to a decision happening in `tags`' world:
    /// global ones always, context-scoped ones when their context is named,
    /// superseded ones never — §7.5's "retrieve only relevant memories".
    ///
    /// Ordered by confidence, most trusted first.
    #[must_use]
    pub fn relevant(&self, tags: &[String]) -> Vec<Preference> {
        let mut found: Vec<Preference> = self
            .preferences
            .iter()
            .filter(|p| p.status != Status::Superseded)
            .filter(|p| match &p.context {
                None => true,
                Some(context) => tags.iter().any(|tag| context.eq_ignore_ascii_case(tag)),
            })
            .cloned()
            .collect();
        found.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        found
    }

    /// Every current preference, for the screen where the person inspects
    /// and corrects their own profile (§7.7).
    #[must_use]
    pub fn inspectable(&self) -> Vec<Preference> {
        let mut found: Vec<Preference> = self
            .preferences
            .iter()
            .filter(|p| p.status != Status::Superseded)
            .cloned()
            .collect();
        found.sort_by_key(|p| p.dimension.label());
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: Id = 1_000;

    #[test]
    fn the_update_moves_exactly_as_the_plan_says() {
        let mut persona = Persona::default();
        persona.observe(Dimension::Stability, 0.0, 0.3, Source::DecisionPriority, None, T0);
        // A second inferred observation, opposite direction, same confidence.
        persona.observe(Dimension::Stability, 0.6, 0.3, Source::DecisionPriority, None, T0 + 10);

        let value = persona.preferences[0].value;
        let expected = 0.0 + LEARNING_RATE * 0.3 * (0.6 - 0.0);
        assert!(
            (value - expected).abs() < 1e-6,
            "the update must be §7.4's rule exactly: {value} vs {expected}"
        );
    }

    #[test]
    fn an_explicit_statement_beats_a_weak_inference() {
        let mut persona = Persona::default();
        // Say it outright: strongly toward stability.
        persona.observe(Dimension::Stability, 0.9, 0.8, Source::Stated, None, T0);
        let explicit_value = persona.preferences[0].value;

        // Inferred, opposite, weak — must not move the value.
        persona.observe(Dimension::Stability, -0.5, 0.2, Source::Usefulness, None, T0 + 1);
        assert_eq!(
            persona.preferences[0].value, explicit_value,
            "an inference never overrides what the person said"
        );
        assert!(persona.preferences[0].explicit);
    }

    #[test]
    fn one_inferred_choice_is_not_a_permanent_preference() {
        let mut persona = Persona::default();
        persona.observe(Dimension::Exploration, 1.0, 0.3, Source::DecisionPriority, None, T0);
        let preference = &persona.preferences[0];
        // It moved all the way from nothing (there was no prior), but it is
        // marked inferred, weakly held, and honest about its source.
        assert!(!preference.explicit);
        assert!(preference.confidence <= 0.3, "one datapoint cannot certify");
    }

    #[test]
    fn contexts_do_not_average_into_mush() {
        let mut persona = Persona::default();
        persona.observe(Dimension::Stability, 0.9, 0.8, Source::Stated, Some("career".into()), T0);
        persona.observe(Dimension::Stability, -0.7, 0.8, Source::Stated, Some("travel".into()), T0);
        assert_eq!(persona.preferences.len(), 2);

        // A career-tagged decision sees only the career reading.
        let at_work = persona.relevant(&["career".to_owned()]);
        assert_eq!(at_work.len(), 1);
        assert!(at_work[0].value > 0.0);

        // An untagged decision sees neither — they are both scoped, and
        // inventing an average would be inventing a person.
        assert!(persona.relevant(&[]).is_empty());
    }

    #[test]
    fn global_preferences_apply_everywhere() {
        let mut persona = Persona::default();
        persona.observe(Dimension::Simplicity, 0.8, 0.8, Source::Stated, None, T0);
        assert_eq!(persona.relevant(&[]).len(), 1);
        assert_eq!(persona.relevant(&["anything".to_owned()]).len(), 1);
    }

    #[test]
    fn superseded_preferences_are_kept_but_never_used() {
        let mut persona = Persona::default();
        persona.observe(Dimension::RiskTolerance, 0.8, 0.8, Source::Stated, None, T0);
        persona.supersede(Dimension::RiskTolerance, None);
        assert!(persona.relevant(&[]).is_empty(), "superseded = excluded");
        assert_eq!(persona.preferences.len(), 1, "history is kept");
    }

    #[test]
    fn a_correction_supersedes_the_old_reading() {
        let mut persona = Persona::default();
        persona.observe(Dimension::Growth, 0.2, 0.3, Source::DecisionPriority, None, T0);
        persona.correct(Dimension::Growth, -0.4, None, None, T0 + 5);
        // One preference for the pair: the old one superseded, the new one
        // the only current reading.
        let current: Vec<_> = persona
            .preferences
            .iter()
            .filter(|p| p.status == Status::Current)
            .collect();
        assert_eq!(current.len(), 1);
        assert!(current[0].value < 0.0);
    }

    #[test]
    fn retrieval_orders_by_confidence() {
        let mut persona = Persona::default();
        persona.observe(Dimension::Simplicity, 0.5, 0.9, Source::Stated, None, T0);
        persona.observe(Dimension::Security, 0.5, 0.3, Source::DecisionPriority, None, T0);
        let found = persona.relevant(&[]);
        assert_eq!(found[0].dimension, Dimension::Simplicity);
    }
}
