//! The engine — the plan's `u-app` orchestration, as the app's own module.
//!
//! One place ties the backend crates into the shape the screens call:
//! retrieve the persona's *relevant* preferences (§7.5), run the runtime's
//! single-shot pipeline (§5.6), persist the decision and its assessment
//! (§8), and fold the allowed learning signals back into the persona
//! (§7.3). The screens hold signals; this holds the process between them.
//!
//! # Where it runs
//!
//! On the UI thread, synchronously, because the engine that ships — the
//! deterministic baseline — is instant and allocation-light: a cue read
//! over a few hundred bytes. A model runtime keeps the same shape from
//! the caller's side (one call in, one record out) while moving its own
//! arithmetic to a worker thread behind [`Runtime`](u_ml_runtime::Runtime)
//! — and when it fails, §5.6's tested recovery happens *here*: the
//! baseline that never left answers instead, which is why it is kept
//! beside whatever engine was constructed, not reconstructed on demand.
//!
//! # What it refuses
//!
//! Nothing the person did not offer. The engine reads the situation, the
//! fields they filled, and the preferences they can inspect; it never
//! injects the whole history (§7.5: "do not inject the entire user history
//! into every prompt") and never turns usefulness feedback into a
//! preference (§7.3).

use std::path::Path;

use u_core::{Budget, Error, Id};
use u_decision::{Assessment, Hint, Input};
use u_feedback::Feedback;
use u_memory::{AssessmentRecord, DecisionRecord, FeedbackRecord, Store, Verdict};
use u_ml_runtime::{to_json, Deterministic, Runtime};
use u_persona::Persona;

/// The orchestrated whole: memory, persona, and the runtime behind them.
pub struct Engine {
    store: Store,
    persona: Persona,
    /// Whatever runtime was constructed — the baseline, or a model that
    /// earned its place behind the same trait.
    runtime: Box<dyn Runtime>,
    /// The §5.6 fallback, always present: the same baseline that shipped
    /// before any model existed, kept alive so a model's failure degrades
    /// to *it* rather than to a reconstruction that could behave differently.
    baseline: Deterministic,
    /// The id the next decision will carry.
    next: Id,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The runtime is named, not dumped: which engine this is *called*
        // is the fact a debug log wants, and a model's channels are
        // machinery nobody reads here.
        f.debug_struct("Engine")
            .field("runtime", &self.runtime.name())
            .field("baseline", &self.baseline)
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Open the engine at a data directory — on Android the activity's
    /// private directory, on a desktop the platform's data dir — on the
    /// deterministic baseline.
    ///
    /// # Errors
    /// Whatever opening the store or reading the persona can refuse:
    /// [`Error::Schema`] for a future store, [`Error::Corrupt`] for a
    /// persona that cannot be read whole.
    pub fn at(dir: impl AsRef<Path>) -> Result<Self, Error> {
        Self::at_with(dir, Box::new(Deterministic::default()))
    }

    /// Open the engine on an explicitly chosen runtime — the constructor
    /// a model arrives through, and the only line that changes the day
    /// one is constructed in anger.
    ///
    /// # Errors
    /// As [`Self::at`].
    pub fn at_with(dir: impl AsRef<Path>, runtime: Box<dyn Runtime>) -> Result<Self, Error> {
        let store = Store::at(dir)?;
        let persona = store.load_persona()?;
        // The next decision's id: one past the newest on disk, so ids stay
        // monotonic across restarts without a counter file.
        let next = store
            .latest_decision()?
            .map_or(u_core::now(), |last| last.id + 1);
        Ok(Self {
            store,
            persona,
            runtime,
            baseline: Deterministic::default(),
            next,
        })
    }

    /// An engine for headless renders and tests: a throwaway store, an
    /// empty persona, nothing kept.
    #[must_use]
    pub fn scratch() -> Self {
        Self::at(u_memory::scratch_dir()).expect("a fresh scratch store always opens")
    }

    /// Bring a decision: interpret, assess, persist both, learn what the
    /// decision's own words are allowed to teach.
    ///
    /// A model runtime's failure — unreadable output, a timeout, weights
    /// that vanished — falls back to the deterministic baseline, which is
    /// §5.6's tested recovery, said to the log because a person deserves
    /// to know which engine answered. Cancellation is *not* a failure: the
    /// person left, so nothing is answered and nothing is persisted.
    ///
    /// # Errors
    /// [`Error::Empty`] / [`Error::OverBudget`] from the budget — the
    /// caller shows the person the bound, not a crash — and
    /// [`Error::Cancelled`] when the person left mid-decision.
    pub fn decide(&mut self, input: &Input) -> Result<Assessment, Error> {
        // Relevant preferences only, retrieved by the situation's own
        // inferred tags — see `hints_for`, below, for why the tags are read
        // from a throwaway first pass.
        let hints = self.hints_for(&input.situation);
        let assessment = match self.runtime.decide(input, &hints) {
            Ok(assessment) => assessment,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(error) => {
                // The baseline re-validates the input on its own, so an
                // over-long or empty situation surfaces from it unchanged;
                // a model's own failure is recovered from, and said.
                eprintln!(
                    "U: the {} runtime refused ({error}); the deterministic baseline answered",
                    self.runtime.name()
                );
                self.baseline.decide(input, &hints)?
            }
        };

        // The decision record, exactly as brought.
        let id = self.next;
        self.next += 1;
        let record = DecisionRecord {
            id,
            situation: assessment.understanding.clone(),
            options: input.options.clone(),
            matters: input.matters.clone(),
            constraints: input.constraints.clone(),
            notes: input.notes.clone(),
            tags: self.tags_for(&input.situation),
        };
        // The understanding restates the situation; the record keeps the
        // situation itself, normalised only to its whitespace, because
        // §2.6's history is the person's own words.
        let mut record = record;
        record.situation = input
            .situation
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        self.store.append("decisions", &record)?;
        self.store.append(
            "assessments",
            &AssessmentRecord {
                decision: id,
                assessment: to_json(&assessment)?,
            },
        )?;

        // The allowed learning: a priority they *named* is weak, scoped
        // evidence (§7.3). Nothing else about this decision teaches
        // anything, and the usefulness verdict they may give later teaches
        // nothing at all.
        let context = record.tags.first().cloned();
        u_feedback::learn_priorities(&mut self.persona, &input.matters, context, id);
        let _ = self.store.save_persona(&self.persona);

        Ok(assessment)
    }

    /// Record the person's verdict on an assessment.
    ///
    /// # Errors
    /// [`Error::OverBudget`] for an over-long note; the store's own errors.
    pub fn give_feedback(&mut self, verdict: Verdict, note: Option<&str>) -> Result<(), Error> {
        if let Some(note) = note {
            u_feedback::check_note(note)?;
        }
        let record = FeedbackRecord {
            decision: self.next - 1,
            verdict,
            note: note.filter(|n| !n.trim().is_empty()).map(str::to_owned),
            at: u_core::now(),
        };
        self.store.append("feedback", &record)
    }

    /// The most recent decision, for the strip on home that offers to pick
    /// up where the person left off.
    ///
    /// # Errors
    /// The store's own.
    pub fn latest_decision(&self) -> Result<Option<DecisionRecord>, Error> {
        self.store.latest_decision()
    }

    /// How many decisions the memory holds — home's strip reads it at
    /// startup, so the count is honest the moment the app opens rather
    /// than the first time a decision lands.
    ///
    /// A store that cannot be read counts as none rather than as a
    /// crash: the strip's question is "is there anything to pick up",
    /// and "unknown" is not an answer a first screen may give.
    #[must_use]
    pub fn decision_count(&self) -> u32 {
        self.store
            .load::<DecisionRecord>("decisions")
            .map_or(0, |(all, _)| u32::try_from(all.len()).unwrap_or(u32::MAX))
    }

    /// The persona, for the screen where the person inspects and corrects
    /// what U has learned about them (§7.7).
    #[must_use]
    pub fn persona(&self) -> &Persona {
        &self.persona
    }

    /// The runtime's name — what a report prints, and what the settings
    /// screen can show honestly. It names the engine *constructed*; when
    /// a model fails and the baseline answers, the log says so at the
    /// moment it happens (§15: no unimplemented feature presented as
    /// available, and no fallback presented as the model).
    #[must_use]
    pub fn runtime_name(&self) -> &'static str {
        self.runtime.name()
    }

    /// Abandon the decision in flight — §5.6's "cancel work when requested
    /// or when the user leaves", handed to whichever runtime is behind the
    /// trait. The baseline has nothing to cancel; a model stops between
    /// tokens, and the `decide` the caller holds comes back cancelled.
    pub fn cancel(&self) {
        self.runtime.cancel();
    }

    /// The preferences relevant to a situation: scoped ones whose context
    /// the situation's own tags name, plus global ones, most confident
    /// first — never the whole persona.
    fn hints_for(&self, situation: &str) -> Vec<Hint> {
        self.persona
            .relevant(&self.tags_for(situation))
            .into_iter()
            .map(|p| Hint {
                dimension: p.dimension.label().to_owned(),
                leaning: p.value,
                context: p.context.clone(),
            })
            .collect()
    }

    /// The tags a situation carries — the same cue read the interpreter
    /// performs, on a throwaway input, so the retrieval and the
    /// interpretation can never disagree about which world this decision
    /// happens in.
    fn tags_for(&self, situation: &str) -> Vec<String> {
        let throwaway = Input {
            situation: situation.to_owned(),
            ..Input::default()
        };
        u_decision::interpret(&throwaway, &[], &Budget::DEFAULT).optional_tags
    }

    /// The person's own feedback vocabulary, mapped to the record's.
    #[must_use]
    pub fn verdict_of(feedback: &Feedback) -> Verdict {
        match feedback {
            Feedback::Helpful => Verdict::Helpful,
            Feedback::NotHelpful | Feedback::NotHelpfulBecause(_) => Verdict::NotHelpful,
        }
    }
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
    fn a_decision_is_assessed_persisted_and_remembered() {
        let mut engine = Engine::scratch();
        let assessment = engine
            .decide(&input("Should I take the offer or stay at my current job?"))
            .expect("the baseline always decides");
        assert!(assessment.contract_violations().is_empty());

        let latest = engine.latest_decision().unwrap().expect("it was saved");
        assert_eq!(
            latest.situation, "Should I take the offer or stay at my current job?",
            "history keeps the person's own words"
        );
    }

    #[test]
    fn an_empty_situation_is_refused_not_decided() {
        let mut engine = Engine::scratch();
        assert_eq!(
            engine.decide(&input("   ")),
            Err(Error::Empty("the situation"))
        );
    }

    #[test]
    fn a_named_priority_is_learned_scoped_and_weakly() {
        let mut engine = Engine::scratch();
        let with_matters = Input {
            situation: "Which team should I join next year".to_owned(),
            matters: vec!["growth".to_owned()],
            ..Input::default()
        };
        engine.decide(&with_matters).unwrap();
        let persona = engine.persona();
        assert_eq!(persona.preferences.len(), 1);
        assert!(!persona.preferences[0].explicit);
        // The situation tags as career, so the evidence is scoped there.
        assert!(persona.preferences[0]
            .context
            .as_deref()
            .is_some_and(|c| c == "career"));
    }

    #[test]
    fn hints_retrieve_by_the_situation_own_world_not_the_whole_person() {
        let mut engine = Engine::scratch();
        // A global preference and a scoped one.
        engine.persona.observe(
            u_persona::Dimension::Simplicity,
            0.8,
            0.8,
            u_persona::Source::Stated,
            None,
            1,
        );
        engine.persona.observe(
            u_persona::Dimension::Stability,
            0.9,
            0.8,
            u_persona::Source::Stated,
            Some("career".to_owned()),
            1,
        );

        // A career-tagged situation sees both…
        let career = engine.hints_for("Should I switch teams at work?");
        assert_eq!(career.len(), 2);
        // …and a nowhere situation sees only the global one.
        let nowhere = engine.hints_for("Where should we holiday?");
        assert_eq!(nowhere.len(), 1);
        assert_eq!(nowhere[0].dimension, "simplicity");
    }

    #[test]
    fn feedback_is_recorded_and_teaches_the_persona_nothing() {
        let mut engine = Engine::scratch();
        engine.decide(&input("Should I move cities?")).unwrap();
        let before = engine.persona().clone();
        engine
            .give_feedback(Verdict::NotHelpful, Some("missed the lease"))
            .unwrap();
        assert_eq!(
            engine.persona(),
            &before,
            "a verdict is memory, not evidence"
        );
    }

    /// A runtime that always fails, so the §5.6 recovery is tested where
    /// it lives — behind the trait, in the engine, with the store real.
    struct Refuses;

    impl Runtime for Refuses {
        fn name(&self) -> &'static str {
            "refuses"
        }
        fn interpret(
            &self,
            _input: &Input,
            _hints: &[Hint],
        ) -> Result<u_decision::Interpretation, Error> {
            Err(Error::Output {
                what: "the model's interpretation",
            })
        }
        fn assess(
            &self,
            _seen: &u_decision::Interpretation,
            _hints: &[Hint],
        ) -> Result<Assessment, Error> {
            Err(Error::Output {
                what: "the model's assessment",
            })
        }
    }

    /// A runtime that is always cancelled, so cancellation is tested as
    /// the different fact it is: not a failure to recover from.
    struct Left;

    impl Runtime for Left {
        fn name(&self) -> &'static str {
            "left"
        }
        fn interpret(
            &self,
            _input: &Input,
            _hints: &[Hint],
        ) -> Result<u_decision::Interpretation, Error> {
            Err(Error::Cancelled)
        }
        fn assess(
            &self,
            _seen: &u_decision::Interpretation,
            _hints: &[Hint],
        ) -> Result<Assessment, Error> {
            Err(Error::Cancelled)
        }
    }

    #[test]
    fn a_failing_model_degrades_to_the_baseline_and_the_decision_still_counts() {
        let mut engine = Engine::at_with(u_memory::scratch_dir(), Box::new(Refuses))
            .expect("a scratch store always opens");
        assert_eq!(engine.runtime_name(), "refuses");
        let assessment = engine
            .decide(&input("Should I take the offer or stay?"))
            .expect("the baseline answers what the model could not");
        assert!(assessment.contract_violations().is_empty());
        // And the record is kept, because the person brought a decision
        // and an answer arrived — whichever engine wrote it.
        assert!(engine.latest_decision().unwrap().is_some());
    }

    #[test]
    fn a_model_input_error_surfaces_unchanged_through_the_fallback() {
        let mut engine = Engine::at_with(u_memory::scratch_dir(), Box::new(Refuses))
            .expect("a scratch store always opens");
        // An empty situation is the person's input, not the model's
        // failure: the same refusal comes back, not a baseline answer.
        assert_eq!(
            engine.decide(&input("   ")),
            Err(Error::Empty("the situation"))
        );
    }

    #[test]
    fn cancellation_persists_nothing_because_the_person_left() {
        let mut engine = Engine::at_with(u_memory::scratch_dir(), Box::new(Left))
            .expect("a scratch store always opens");
        assert_eq!(
            engine.decide(&input("Should I move cities?")),
            Err(Error::Cancelled),
            "leaving is not failing, so nothing is answered"
        );
        assert!(
            engine.latest_decision().unwrap().is_none(),
            "and nothing is written to their history either"
        );
        assert_eq!(engine.decision_count(), 0);
    }

    #[test]
    fn the_baseline_engine_can_be_cancelled_for_free() {
        // The trait's no-op cancel on the shipping engine: harmless,
        // present, and callable without a runtime behind it.
        let engine = Engine::scratch();
        engine.cancel();
    }
}
