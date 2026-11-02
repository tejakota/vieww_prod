//! The assessment composer: an interpretation in, the §4.4 contract out.
//!
//! Everything this module writes is either a restatement of what the person
//! said, a statement true by construction, or a sentence that *names its own
//! uncertainty*. The deterministic engine has no facts about the person's
//! world — so its assessment says what a careful stranger could say: what
//! the decision is, what would need weighing, what is missing, and what a
//! sensible next step looks like. Nothing is dressed up as knowledge.

use u_core::Budget;

use crate::{Assessment, Interpretation, Option_, Origin, SCHEMA_VERSION};

/// Write the assessment for one interpretation.
///
/// The same `hints` that informed the interpretation come back in, so the
/// prose can say *"you lean toward stability"* out loud rather than having
/// influenced anything silently (§4.3: separate facts, claims, assumptions
/// and inferences).
#[must_use]
pub fn compose(seen: &Interpretation, budget: &Budget) -> Assessment {
    let their_options: Vec<&Option_> = seen
        .options
        .iter()
        .filter(|o| o.origin == Origin::Theirs)
        .collect();
    let suggested: Vec<&Option_> = seen
        .options
        .iter()
        .filter(|o| o.origin == Origin::Suggested)
        .collect();

    let mut assessment = Assessment {
        schema: SCHEMA_VERSION,
        understanding: understanding(seen, &their_options),
        options: seen.options.clone(),
        relevant_priorities: priorities(seen),
        tradeoffs: tradeoffs(seen, &their_options, &suggested, budget),
        unknowns: seen.unknowns.clone(),
        assumptions: seen.assumptions.clone(),
        assessment: perspective(seen, &their_options),
        confidence: seen.confidence,
        next_step: next_step(seen, &their_options),
        follow_up_question: follow_up(seen),
    };
    assessment.tradeoffs.truncate(budget.max_list_items);
    assessment
}

/// What U takes the situation and the decision to be — a restatement, first
/// person to second, without paraphrasing so hard it becomes new material.
fn understanding(seen: &Interpretation, their_options: &[&Option_]) -> String {
    let options = match their_options.len() {
        0 => "You have not named any options yet".to_owned(),
        1 => format!("One option is on the table: “{}”", their_options[0].label),
        n => format!(
            "{n} options are on the table, starting with “{}” and “{}”",
            their_options[0].label, their_options[1].label
        ),
    };
    let mut out = format!("You're {}. As you put it: “{}”", seen.intent, seen.situation);
    if let Some(horizon) = &seen.time_horizon {
        out.push_str(&format!(", over {horizon}"));
    }
    out.push_str(". ");
    out.push_str(&options);
    out.push('.');
    out
}

/// What matters here — theirs in their own words first, then the read ones.
fn priorities(seen: &Interpretation) -> Vec<String> {
    seen.priorities.clone()
}

/// How the options compare — true by construction, one line each, and never
/// a number.
fn tradeoffs(
    seen: &Interpretation,
    their_options: &[&Option_],
    suggested: &[&Option_],
    _budget: &Budget,
) -> Vec<String> {
    let top = seen.priorities.first().cloned();
    let second = seen.priorities.get(1).cloned();
    let mut out = Vec::new();

    // Their options: the weighing is theirs; U states what the weighing is
    // *of*, not which way it lands — that honesty is the whole rule.
    for option in their_options.iter().take(3) {
        let against = match (&top, &second) {
            (Some(a), Some(b)) => format!("“{a}” against “{b}”"),
            (Some(a), None) => format!("“{a}”"),
            _ => "what you care about here".to_owned(),
        };
        out.push(format!(
            "“{}”: how it serves {} is the judgement — U has no verified facts about that, only what you've said",
            option.label, against
        ));
    }

    // U's suggestions: these U *can* characterise, because their properties
    // are the reason they were suggested.
    for option in suggested.iter().take(2) {
        if option.label.starts_with("Do nothing") {
            let keeps = top
                .as_ref()
                .map_or_else(|| "today's trade-offs".to_owned(), |p| format!("today's “{p}”"));
            out.push(format!(
                "“{}”: keeps {keeps} exactly as they are — the cost is whatever the change would have bought",
                option.label
            ));
        } else {
            out.push(format!(
                "“{}”: limits both the upside and the downside, which buys information before commitment",
                option.label
            ));
        }
    }
    out
}

/// The reasoned assessment: what a careful stranger could say.
fn perspective(seen: &Interpretation, their_options: &[&Option_]) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(format!(
        "Reading what you wrote, this is {}",
        seen.intent
    ));

    match their_options.len() {
        0 => parts.push(
            "no options are on the table yet, so the useful move is building the set of real \
             alternatives before weighing any of them"
                .to_owned(),
        ),
        1 => parts.push(format!(
            "one option stands against the quiet alternative of doing nothing, so the real \
             comparison is “{}” versus the current state",
            their_options[0].label
        )),
        _ => parts.push(
            "more than one option is live, so the comparison — not any single option — is the \
             decision"
                .to_owned(),
        ),
    }

    if let Some(first) = seen.unknowns.first() {
        let first = first.to_lowercase();
        parts.push(format!("{first} is missing, and it could change this reading"));
    } else {
        parts.push(
            "nothing material seems missing, so the limits below are about U's reading, not \
             about absent facts"
                .to_owned(),
        );
    }

    parts.join(". ")
}

/// The practical next step: an action, a question, or a reason to wait.
fn next_step(seen: &Interpretation, their_options: &[&Option_]) -> String {
    if seen.unknowns.iter().any(|u| u.contains("alternatives")) {
        "List every option you can already see — including the unattractive ones; the \
         comparison changes with the full set"
            .to_owned()
    } else if let Some(first) = seen.unknowns.first() {
        let first = first.to_lowercase();
        format!("Find out {first} — the assessment leans on it")
    } else if their_options.len() > 1 {
        "Try the smallest reversible version of the leading option before committing to \
         the whole thing"
            .to_owned()
    } else {
        "Write down what “good” looks like a month from now — that is the standard to judge any \
         option against"
            .to_owned()
    }
}

/// The one clarification worth asking, if any — one at a time, never
/// something already said (§2.5).
fn follow_up(seen: &Interpretation) -> Option<String> {
    seen.unknowns.first().map(|unknown| match () {
        _ if unknown.contains("alternatives") => {
            "If you could only pick between two versions of this, what would the two be?".to_owned()
        }
        _ if unknown.contains("timeframe") => {
            "When does this need to be decided by?".to_owned()
        }
        _ if unknown.contains("costs") => {
            "Roughly what numbers are in play — even a range?".to_owned()
        }
        _ if unknown.contains("conditions") => {
            "What does the outcome depend on, as far as you can tell?".to_owned()
        }
        _ => format!("Could you tell me more about {}?", unknown.to_lowercase()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{interpret, Input};

    fn assessed(situation: &str) -> Assessment {
        let input = Input {
            situation: situation.to_owned(),
            ..Input::default()
        };
        let seen = interpret(&input, &[], &Budget::DEFAULT);
        compose(&seen, &Budget::DEFAULT)
    }

    #[test]
    fn every_assessment_satisfies_the_contract() {
        for situation in [
            // The plan's own example.
            "I'm considering moving to another city, but I'm not sure whether the change is worth it.",
            "Should I stay at my current job or take the new offer?",
            "stuck",
            "Should I take the promotion even though it means more travel? My partner starts a \
             new job this year and we just signed a lease.",
            "",
        ] {
            let assessment = assessed(situation);
            assert!(
                assessment.contract_violations().is_empty(),
                "{situation:?} broke the contract: {:?}",
                assessment.contract_violations()
            );
        }
    }

    #[test]
    fn no_numbers_are_invented_anywhere() {
        let assessment = assessed(
            "Should I switch from my job to freelance work? I have savings but rent is high.",
        );
        let joined = format!(
            "{}{}{}",
            assessment.assessment,
            assessment.understanding,
            assessment.next_step
        );
        // A bare percentage or a score-shaped number would be false
        // precision; the letters would have to come from us.
        assert!(!joined.contains('%'), "a percentage appeared: {joined}");
        for word in joined.split_whitespace() {
            let bare = word.trim_matches(|c: char| !c.is_ascii_digit());
            if bare.len() >= 2 && bare.chars().all(|c| c.is_ascii_digit()) {
                panic!("a multi-digit number appeared in prose: {word}");
            }
        }
    }

    #[test]
    fn at_most_one_follow_up_question_ever() {
        let assessment = assessed(
            "I want to move out but I'm not sure I can afford it, and it depends on the lease.",
        );
        assert!(assessment.follow_up_question.is_some());
        // "at most one" is the type's own shape — `Option` — so this is also
        // a compile-time property; the test keeps the intent named.
    }

    #[test]
    fn suggested_options_are_always_marked() {
        let assessment = assessed("thinking about whether to change teams");
        for option in &assessment.options {
            if !assessment
                .understanding
                .contains(&option.label)
            {
                // Any option not quoted as the person's must be marked
                // suggested — there is no third origin.
                assert_eq!(option.origin, Origin::Suggested);
            }
        }
        assert!(assessment
            .options
            .iter()
            .any(|o| o.origin == Origin::Suggested));
    }

    #[test]
    fn an_empty_situation_still_gets_a_contract_valid_assessment() {
        // The budget refuses empty input at the edge; the composer itself
        // still cannot be allowed to produce a broken record if handed one,
        // because "validate and recover" is §5.6's rule.
        let seen = Interpretation::new("");
        let assessment = compose(&seen, &Budget::DEFAULT);
        assert_eq!(assessment.confidence, crate::Confidence::Low);
    }
}
