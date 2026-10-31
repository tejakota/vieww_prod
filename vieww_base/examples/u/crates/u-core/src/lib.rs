//! `u-core` — the shared vocabulary of U's decision backend.
//!
//! Everything every other backend crate says: what a record is called, when
//! it happened, what can go wrong, and the bounds a runtime must respect.
//! Nothing here knows about decisions, personas or models — those crates
//! build on this one, and keeping the leaf small is what keeps their
//! dependencies honest.
//!
//! The plan this implements: *U — Complete Product and Engineering Plan*,
//! §9 (`u-core: shared types, configuration and errors`) and §5.6 (runtime
//! safeguards).

use std::fmt;

/// What a persisted record is called: wall-clock seconds at the moment it
/// was created. Monotonic enough to order by, unique enough in practice for
/// a single device writing one line at a time, and meaningless enough that
/// it cannot be mistaken for a fact about the person.
pub type Id = u64;

/// Now, in seconds since the Unix epoch.
///
/// The one clock the backend keeps. Nothing else in these crates reads the
/// system clock: every timestamp is passed *in*, so a test can hold time
/// still and a replay can restore it exactly.
#[must_use]
pub fn now() -> Id {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Everything the backend refuses to guess through.
///
/// Every variant names a condition the caller can *do something about* —
/// shorten the input, free the model, migrate the store — because an error
/// that cannot be acted on is a crash wearing a nicer name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input was empty or whitespace where content was required.
    Empty(&'static str),
    /// The input, or a piece of it, exceeded a [`Budget`] bound.
    ///
    /// The runtime safeguards of §5.6: caps on input and output are the
    /// app's promise that a runaway request cannot eat a phone's memory,
    /// and the error is the honest form of that promise.
    OverBudget {
        /// What was measured.
        what: &'static str,
        /// The size it reached.
        got: usize,
        /// The bound it crossed.
        bound: usize,
    },
    /// A stored record could not be read back.
    ///
    /// Carries the count of lines already recovered rather than the raw IO
    /// error: the store's contract is that one corrupt line costs that line
    /// and nothing else, and the number is what the caller shows a person.
    Corrupt {
        /// How many records were recovered intact before the damage.
        recovered: usize,
    },
    /// The store's schema is newer than this build understands.
    ///
    /// A forward-compatibility refusal rather than a best-effort parse: an
    /// old binary guessing at a new schema is how a person's decision
    /// history becomes noise that still *looks* like their history.
    Schema {
        /// The version found on disk.
        found: u32,
        /// The newest version this build reads.
        reads: u32,
    },
    /// The record being asked for is not there.
    Missing,
    /// The runtime is not present — no model was ever loaded.
    ///
    /// Distinct from a model that *failed*: the deterministic baseline is
    /// always present, so this is reached only by asking a model runtime
    /// for something it was never given.
    NoRuntime(&'static str),
    /// The work was cancelled before it finished — the person left, or the
    /// caller gave up waiting.
    ///
    /// §5.6's "cancel work when requested or when the user leaves": the
    /// result is abandoned by design, so the error is the caller's signal
    /// to abandon it too, not to retry (there are no unbounded retries).
    Cancelled,
    /// A model's output could not be read as the contract it was asked for.
    ///
    /// The tested-recovery half of §5.6: the caller falls back to the
    /// deterministic engine rather than showing a person prose that only
    /// looks like an assessment.
    Output {
        /// What could not be read — "the model's assessment", and so on.
        what: &'static str,
    },
    /// Weights (or a tokenizer) failed the digest check against their
    /// manifest.
    ///
    /// §5.1's "verify the exact revision" gate: the file is present and
    /// wrong, which is a different fact from absent — the caller deletes
    /// and re-fetches rather than loading bytes that are not the ones the
    /// manifest vouched for.
    Unverified {
        /// What did not match — "the weights", "the tokenizer".
        what: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(what) => write!(f, "{what} is empty — nothing to work with"),
            Self::OverBudget { what, got, bound } => {
                write!(f, "{what} is {got}, over the {bound} bound")
            }
            Self::Corrupt { recovered } => write!(
                f,
                "a stored record could not be read back; {recovered} intact records kept"
            ),
            Self::Schema { found, reads } => {
                write!(
                    f,
                    "store schema {found} is newer than this build reads ({reads})"
                )
            }
            Self::Missing => write!(f, "no such record"),
            Self::NoRuntime(what) => write!(f, "no runtime for {what}"),
            Self::Cancelled => write!(f, "the work was cancelled before it finished"),
            Self::Output { what } => {
                write!(
                    f,
                    "{what} could not be read as the contract asks — falling back"
                )
            }
            Self::Unverified { what } => write!(
                f,
                "{what} did not match their manifest — refusing to load them"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// The bounds every runtime respects, whatever is behind it.
///
/// A *contract* rather than a suggestion: the deterministic engine enforces
/// them exactly as a model runtime will, because the day a 0.6B model lands
/// behind the same trait is not the day to discover the caps were never
/// real. Sizes are in characters for input (what a person typed) and items
/// for output (how many of anything an assessment may list) — never tokens,
/// which are the model runtime's private business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The most characters of free-form situation the backend will read.
    pub max_input_chars: usize,
    /// The most options an assessment may carry, user's and suggested together.
    pub max_options: usize,
    /// The most entries in any one list an assessment may carry —
    /// priorities, trade-offs, unknowns, assumptions.
    pub max_list_items: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Budget {
    /// The bounds the plan's §5.6 asks for, sized for a phone.
    ///
    /// Two thousand characters is two long paragraphs — more situation than
    /// anyone types into a phone before wanting an answer — and every list
    /// is capped where a person stops reading rather than where memory
    /// runs out, because the failure mode worth engineering against is an
    /// assessment nobody finishes.
    pub const DEFAULT: Self = Self {
        max_input_chars: 2_000,
        max_options: 6,
        max_list_items: 5,
    };

    /// Check a situation against the input bound, and against emptiness.
    ///
    /// # Errors
    /// [`Error::Empty`] when the text is nothing but whitespace — there is
    /// no decision to read — and [`Error::OverBudget`] when it is longer
    /// than `max_input_chars`.
    pub fn check_input(&self, text: &str) -> Result<(), Error> {
        let got = text.trim().chars().count();
        if got == 0 {
            Err(Error::Empty("the situation"))
        } else if got > self.max_input_chars {
            Err(Error::OverBudget {
                what: "the situation",
                got,
                bound: self.max_input_chars,
            })
        } else {
            Ok(())
        }
    }

    /// Truncate a list to the output bound, in place.
    pub fn clamp_list<T>(&self, list: &mut Vec<T>) {
        if list.len() > self.max_list_items {
            list.truncate(self.max_list_items);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_situation_is_refused_by_name() {
        assert_eq!(
            Budget::DEFAULT.check_input("   "),
            Err(Error::Empty("the situation"))
        );
        // The error is at least actionable, which is the point of the type.
        assert!(Error::Empty("the situation")
            .to_string()
            .contains("situation"));
    }

    #[test]
    fn the_input_bound_counts_characters_after_trimming() {
        let padded = format!("{}over", " ".repeat(Budget::DEFAULT.max_input_chars));
        assert!(
            Budget::DEFAULT.check_input(&padded).is_ok(),
            "padding is not content"
        );
        let over = "x".repeat(Budget::DEFAULT.max_input_chars + 1);
        assert!(matches!(
            Budget::DEFAULT.check_input(&over),
            Err(Error::OverBudget { got, bound, .. })
                if got == Budget::DEFAULT.max_input_chars + 1 && bound == Budget::DEFAULT.max_input_chars
        ));
        assert!(Budget::DEFAULT
            .check_input("a considered situation")
            .is_ok());
    }

    #[test]
    fn a_list_is_clamped_not_rejected() {
        let mut list = vec![1, 2, 3, 4, 5, 6, 7];
        Budget::DEFAULT.clamp_list(&mut list);
        assert_eq!(list.len(), Budget::DEFAULT.max_list_items);
    }

    #[test]
    fn cancellation_and_unreadable_output_are_named_and_actionable() {
        // Both must say what the caller should do, in words a person reads.
        assert!(Error::Cancelled.to_string().contains("cancelled"));
        let said = Error::Output {
            what: "the model's assessment",
        }
        .to_string();
        assert!(said.contains("assessment"));
        assert!(said.contains("falling back"));
    }

    #[test]
    fn unverified_weights_are_refused_by_name() {
        let said = Error::Unverified {
            what: "the weights",
        }
        .to_string();
        assert!(said.contains("did not match"));
        assert!(said.contains("refusing"));
    }
}
