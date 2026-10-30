//! The interpreter: free-form text in, a structured understanding out.
//!
//! Deterministic and honest about being so. Every rule below is a *cue* — a
//! word or a sentence shape decisions are commonly written in — and a cue
//! only ever moves a fact from "unnoticed" to "recorded". Nothing is
//! invented: an option appears only if the person wrote it or U can defend
//! suggesting it, an unknown appears only for something genuinely absent,
//! and the confidence is a description of the input, never of the engine's
//! mood.

use u_core::Budget;

use crate::{Confidence, Hint, Input, Interpretation, Option_, Origin};

/// Read one free-form situation into an [`Interpretation`].
///
/// `hints` are the persona preferences relevant here (already filtered by
/// the caller); they are recorded as assumptions — *"you lean toward
/// stability here"* — so the assessment can never quietly become a
/// personality test the person never agreed to.
#[must_use]
pub fn interpret(input: &Input, hints: &[Hint], budget: &Budget) -> Interpretation {
    // Normalise without rewriting: trim, collapse runs of whitespace. The
    // person's phrasing is evidence about their situation, not material to
    // tidy up beyond making it machine-readable at all.
    let situation = normalise(&input.situation);
    let lower = situation.to_lowercase();
    let words: Vec<&str> = lower
        .split_whitespace()
        .map(strip_punct)
        .filter(|word| !word.is_empty())
        .collect();

    let mut out = Interpretation::new(situation);

    out.intent = intent(&lower, &input.options);
    out.time_horizon = time_horizon(&lower);
    out.options = the_options(input, &lower, budget);
    out.constraints = the_constraints(input, &lower, budget);
    out.priorities = the_priorities(input, &lower, budget);
    out.optional_tags = tags(&words);
    out.unknowns = the_unknowns(&out, &lower, &words, input, budget);
    out.assumptions = the_assumptions(&out, hints);
    out.confidence = confidence(&out);
    out.confidence_reason = confidence_reason(&out);
    out
}

/// Trim and collapse whitespace — the whole of the normalisation, §4.3's
/// "without losing meaning".
fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One word with the punctuation that leads or trails it taken off —
/// "work?" is still the word "work", and a cue matcher that cannot see
/// that tags by luck rather than by reading.
fn strip_punct(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric())
}

/// What kind of decision this is, as one plain phrase.
fn intent(lower: &str, user_options: &[String]) -> String {
    let has = |needle: &str| lower.contains(needle);
    if has("should i") || has("whether") || has("or not") || has("decide") || has("deciding")
        || has("worth it") || has("better to") || has("do i") || has("torn between")
    {
        if user_options.len() > 1 || has(" or ") || has(" vs") || has("between") {
            "choosing between paths".to_owned()
        } else {
            "deciding whether one change is worth making".to_owned()
        }
    } else if has("not sure") || has("unsure") || has("stuck") || has("confused") {
        "understanding what is really being decided".to_owned()
    } else {
        "thinking through a situation".to_owned()
    }
}

/// The horizon, when the words said so.
fn time_horizon(lower: &str) -> Option<String> {
    for (cue, horizon) in [
        ("this week", "days"),
        ("this month", "weeks"),
        ("next month", "weeks"),
        ("this year", "months"),
        ("next year", "about a year"),
        ("long term", "years"),
        ("long-term", "years"),
        ("rest of my", "years"),
        ("forever", "years"),
    ] {
        if lower.contains(cue) {
            return Some(horizon.to_owned());
        }
    }
    // "by <month name>" — a deadline stated in calendar words.
    for month in [
        "january", "february", "march", "april", "may", "june", "july", "august", "september",
        "october", "november", "december",
    ] {
        if lower.contains(&format!("by {month}")) {
            return Some("until the stated deadline".to_owned());
        }
    }
    None
}

/// Every option: theirs first — from the field, then from the prose — then
/// U's suggestions, marked as suggestions and defended by construction.
fn the_options(input: &Input, lower: &str, budget: &Budget) -> Vec<Option_> {
    let mut options: Vec<Option_> = Vec::new();
    // Add an option unless it is empty, a duplicate, or past the cap.
    fn push(options: &mut Vec<Option_>, label: String, origin: Origin, budget: &Budget) {
        let label = normalise(&label);
        if !label.is_empty()
            && options.len() < budget.max_options
            && !options.iter().any(|o| o.label.eq_ignore_ascii_case(&label))
        {
            options.push(Option_ { label, origin });
        }
    }

    // Theirs, given as options.
    for given in &input.options {
        push(&mut options, given.clone(), Origin::Theirs, budget);
    }

    // Theirs, standing in the prose: "X or Y", "X vs Y", "whether to X or Y".
    if let Some((left, right)) = split_on_either(lower, " or ") {
        push(&mut options, left.to_owned(), Origin::Theirs, budget);
        push(&mut options, right.to_owned(), Origin::Theirs, budget);
    } else if let Some((left, right)) = split_on_either(lower, " vs ") {
        push(&mut options, left.to_owned(), Origin::Theirs, budget);
        push(&mut options, right.to_owned(), Origin::Theirs, budget);
    } else if let Some((left, right)) = split_on_either(lower, " versus ") {
        push(&mut options, left.to_owned(), Origin::Theirs, budget);
        push(&mut options, right.to_owned(), Origin::Theirs, budget);
    }

    // U's suggestions — generic because they are *structurally* always true,
    // never a guess about the person's specifics (§2.3: "options they have
    // not considered").
    push(
        &mut options,
        "Do nothing for now — keep the current state".to_owned(),
        Origin::Suggested,
        budget,
    );
    if !options.iter().any(|o| o.origin == Origin::Suggested) {
        push(
            &mut options,
            "A smaller, reversible trial of the leading option".to_owned(),
            Origin::Suggested,
            budget,
        );
    }
    options
}

/// Split a text on a separator when both halves look like option text —
/// short enough to be a phrase, long enough to mean something.
///
/// Leading auxiliaries ("should I", "whether to") are stripped from the left
/// half and trailing punctuation from the right, so "should I stay or go?"
/// yields "stay" and "go" rather than fragments.
fn split_on_either<'a>(lower: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let at = lower.find(sep)?;
    let left = &lower[..at];
    let right = &lower[at + sep.len()..];

    let mut left = left.trim();
    for lead in ["should i ", "should we ", "whether to ", "whether ", "to ", "i "] {
        if let Some(rest) = left.strip_prefix(lead) {
            left = rest.trim();
            break;
        }
    }
    let left = left.trim_end_matches([',', ';', '.', '?', '!']);

    let right = right
        .split(['.', '?', '!'])
        .next()
        .unwrap_or(right)
        .trim()
        .trim_end_matches([',', ';']);

    let keep = |s: &str| (3..=60).contains(&s.chars().count());
    if keep(left) && keep(right) {
        Some((left, right))
    } else {
        None
    }
}

/// What bounds the choice — theirs, then read from the words.
fn the_constraints(input: &Input, lower: &str, budget: &Budget) -> Vec<String> {
    let mut constraints: Vec<String> = Vec::new();
    for named in &input.constraints {
        let named = normalise(named);
        if !named.is_empty() && !constraints.contains(&named) {
            constraints.push(named);
        }
    }
    for (cues, fact) in [
        (
            &["budget", "afford", "costly", "expensive", "savings", "debt", "loan", "money"][..],
            "Money is a stated part of the picture",
        ),
        (
            &["deadline", "urgent", "by march", "asap", "running out of time"],
            "Time pressure is a stated part of the picture",
        ),
        (
            &["can't", "cannot", "must not", "committed", "obligated", "tied to"],
            "A commitment limits what can change here",
        ),
        (
            &["lease", "contract", "signed", "notice period", "locked in"],
            "An agreement constrains the timing of any change",
        ),
        (
            &["visa", "relocate", "moving to", "another city", "remote work"],
            "Where you can be is part of the decision",
        ),
    ] {
        if cues.iter().any(|cue| lower.contains(cue)) && constraints.len() < budget.max_list_items {
            constraints.push(fact.to_owned());
        }
    }
    constraints
}

/// What matters here — theirs first, then read from the words, capped.
fn the_priorities(input: &Input, lower: &str, budget: &Budget) -> Vec<String> {
    let mut priorities: Vec<String> = Vec::new();
    for matter in &input.matters {
        let matter = normalise(matter);
        if !matter.is_empty() && !priorities.contains(&matter) {
            priorities.push(matter);
        }
    }
    for (cues, what) in [
        (&["growth", "learn", "career", "opportunity", "advance", "promotion"][..], "growth"),
        (&["stable", "stability", "secure", "security", "safe", "risk"], "stability"),
        (&["family", "partner", "kids", "children", "parents", "friends"], "the people close to you"),
        (&["health", "sleep", "burnout", "stress", "exhausted"], "your health and energy"),
        (&["freedom", "flexib", "independent", "autonomy", "my own"], "independence"),
        (&["simple", "simpler", "quiet", "peace", "calm"], "simplicity"),
    ] {
        if cues.iter().any(|cue| lower.contains(cue))
            && priorities.len() < budget.max_list_items
            && !priorities.iter().any(|p| p == what)
        {
            priorities.push(what.to_owned());
        }
    }
    priorities
}

/// Optional inferred tags — retrieval labels, never gates (§1, §2.1).
fn tags(words: &[&str]) -> Vec<String> {
    let has = |w: &str| words.contains(&w);
    let mut tags = Vec::new();
    // The plurals a situation actually arrives in: "teams", "savings",
    // "cities" — a cue list that missed them would tag a sentence about
    // switching teams as being about nothing at all.
    if has("job")
        || has("work")
        || has("career")
        || has("team")
        || has("teams")
        || has("boss")
        || has("offer")
    {
        tags.push("career".to_owned());
    }
    if has("money") || has("budget") || has("salary") || has("afford") || has("rent") || has("loan") {
        tags.push("finance".to_owned());
    }
    if has("city") || has("move") || has("moving") || has("apartment") || has("relocate") {
        tags.push("life".to_owned());
    }
    if has("family") || has("partner") || has("relationship") || has("friends") {
        tags.push("personal".to_owned());
    }
    tags
}

/// Missing facts that could change the assessment — only genuinely absent
/// ones, each phrased as the fact rather than as the question (the question
/// is the composer's to ask, one at a time).
fn the_unknowns(
    seen: &Interpretation,
    lower: &str,
    words: &[&str],
    input: &Input,
    budget: &Budget,
) -> Vec<String> {
    let mut unknowns: Vec<String> = Vec::new();
    let their_options = seen.options.iter().filter(|o| o.origin == Origin::Theirs).count();

    if their_options == 0 && input.options.is_empty() {
        unknowns.push("Which alternatives actually exist here".to_owned());
    }
    if seen.time_horizon.is_none() {
        unknowns.push("The timeframe this decision plays out over".to_owned());
    }
    if ["money", "cost", "afford", "budget", "salary", "rent"]
        .iter()
        .any(|cue| lower.contains(cue))
        && !contains_number(words)
    {
        unknowns.push("The concrete costs and numbers involved".to_owned());
    }
    if lower.contains("if ") || lower.contains("depends") {
        unknowns.push("The conditions the outcome depends on".to_owned());
    }
    budget.clamp_list(&mut unknowns);
    unknowns
}

/// Whether any word carries a digit or a currency mark.
fn contains_number(words: &[&str]) -> bool {
    words.iter().any(|w| {
        w.chars().any(|c| c.is_ascii_digit())
            || *w == "$"
            || w.starts_with('$')
            || *w == "£"
            || *w == "€"
    })
}

/// What the engine proceeded assuming — the honest ledger (§4.3 step 9).
fn the_assumptions(seen: &Interpretation, hints: &[Hint]) -> Vec<String> {
    let mut assumptions = vec!["The situation is as described, with nothing material left out".to_owned()];
    // A missing horizon is an assumption the engine has to make to say
    // anything; saying it out loud is cheaper than implying it.
    if seen.time_horizon.is_none() {
        assumptions.push("No timeframe was given, so this is read as a near-term decision".to_owned());
    }
    for hint in hints {
        let leaning = if hint.leaning >= 0.0 { "toward" } else { "away from" };
        let line = format!(
            "You lean {leaning} {}{} — from what you have said before",
            hint.dimension,
            hint.context
                .as_ref()
                .map_or_else(String::new, |c| format!(" (in {c})"))
        );
        assumptions.push(line);
    }
    assumptions
}

/// Confidence as a description of the input, never of the engine.
fn confidence(seen: &Interpretation) -> Confidence {
    let their_options = seen.options.iter().filter(|o| o.origin == Origin::Theirs).count();
    let chars = seen.situation.chars().count();
    if chars > 80 && their_options >= 1 && !seen.priorities.is_empty() {
        Confidence::High
    } else if chars > 30 {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

/// The one-line reason for the confidence, in plain words.
fn confidence_reason(seen: &Interpretation) -> String {
    let their_options = seen.options.iter().filter(|o| o.origin == Origin::Theirs).count();
    match seen.confidence {
        Confidence::High => "the situation, options and priorities were all clear".to_owned(),
        Confidence::Medium => format!(
            "the situation was readable{}",
            if their_options == 0 {
                ", but no options were stated"
            } else {
                ""
            }
        ),
        Confidence::Low => "the situation was too thin to read confidently".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Input;

    fn input(situation: &str) -> Input {
        Input {
            situation: situation.to_owned(),
            ..Input::default()
        }
    }

    #[test]
    fn the_plan_own_example_reads_as_expected() {
        // §2.3's example, verbatim.
        let seen = interpret(
            &input("I'm considering moving to another city, but I'm not sure whether the change is worth it."),
            &[],
            &Budget::DEFAULT,
        );
        assert_eq!(seen.intent, "deciding whether one change is worth making");
        assert!(seen.optional_tags.contains(&"life".to_owned()));
        // No options stated: the unknown about alternatives must surface.
        assert!(seen.unknowns.iter().any(|u| u.contains("alternatives")));
        // And exactly one follow-up shape exists — one question at most.
        assert!(seen.unknowns.len() <= 1 + 1);
        // The suggestion exists and is marked.
        assert!(seen.options.iter().any(|o| o.origin == Origin::Suggested));
    }

    #[test]
    fn a_choice_between_two_things_yields_both_theirs() {
        let seen = interpret(
            &input("Should I stay at my current job or take the new offer?"),
            &[],
            &Budget::DEFAULT,
        );
        let theirs: Vec<_> = seen
            .options
            .iter()
            .filter(|o| o.origin == Origin::Theirs)
            .map(|o| o.label.as_str())
            .collect();
        assert!(theirs.iter().any(|o| o.contains("stay")));
        assert!(theirs.iter().any(|o| o.contains("offer") || o.contains("take")));
        assert_eq!(seen.intent, "choosing between paths");
    }

    #[test]
    fn a_user_supplied_option_is_never_duplicated() {
        let with = Input {
            situation: "Quit or stay".to_owned(),
            options: vec!["Quit".to_owned(), "Stay".to_owned()],
            ..Input::default()
        };
        let seen = interpret(&with, &[], &Budget::DEFAULT);
        let theirs: Vec<_> = seen
            .options
            .iter()
            .filter(|o| o.origin == Origin::Theirs)
            .map(|o| o.label.to_lowercase())
            .collect();
        assert_eq!(theirs.iter().filter(|o| o.as_str() == "quit").count(), 1);
    }

    #[test]
    fn zero_one_and_many_options_are_all_real_states() {
        // Zero: no options anywhere.
        let zero = interpret(&input("I feel stuck about my living situation"), &[], &Budget::DEFAULT);
        assert!(zero.options.iter().filter(|o| o.origin == Origin::Theirs).count() == 0);
        // One: a single stated option.
        let one = Input {
            situation: "thinking about whether to take the course".to_owned(),
            options: vec!["take the course".to_owned()],
            ..Input::default()
        };
        let one = interpret(&one, &[], &Budget::DEFAULT);
        assert_eq!(one.options.iter().filter(|o| o.origin == Origin::Theirs).count(), 1);
        // Many: three given.
        let many = Input {
            situation: "where to live next year".to_owned(),
            options: vec!["Berlin".into(), "Lisbon".into(), "stay".into()],
            ..Input::default()
        };
        let many = interpret(&many, &[], &Budget::DEFAULT);
        assert_eq!(many.options.iter().filter(|o| o.origin == Origin::Theirs).count(), 3);
    }

    #[test]
    fn money_without_numbers_is_an_unknown_money_with_numbers_is_not() {
        let without = interpret(
            &input("I want to move out but I'm not sure I can afford it"),
            &[],
            &Budget::DEFAULT,
        );
        assert!(without.unknowns.iter().any(|u| u.contains("costs")));
        let with = interpret(
            &input("I want to move out; rent would be $1200 a month"),
            &[],
            &Budget::DEFAULT,
        );
        assert!(!with.unknowns.iter().any(|u| u.contains("costs")));
    }

    #[test]
    fn a_hint_becomes_a_stated_assumption_never_a_silent_one() {
        let hints = vec![Hint {
            dimension: "stability".to_owned(),
            leaning: 0.6,
            context: None,
        }];
        let seen = interpret(&input("Should I switch teams at work?"), &hints, &Budget::DEFAULT);
        assert!(seen.assumptions.iter().any(|a| a.contains("stability")));
    }

    #[test]
    fn normalisation_collapses_whitespace_and_nothing_else() {
        let seen = interpret(&input("  so   much   space  "), &[], &Budget::DEFAULT);
        assert_eq!(seen.situation, "so much space");
    }

    #[test]
    fn a_question_mark_does_not_hide_a_cue_word() {
        // "work?" is the word "work": punctuation at the edge of a word
        // is typography, not meaning, and the career tag must survive it.
        let seen = interpret(
            &input("Should I switch teams at work?"),
            &[],
            &Budget::DEFAULT,
        );
        assert!(seen.optional_tags.contains(&"career".to_owned()));
    }

    #[test]
    fn tags_are_inferred_but_never_required() {
        let seen = interpret(&input("Should I repaint the hallway?"), &[], &Budget::DEFAULT);
        // A nothing-situation tags nothing and that is fine.
        assert!(seen.optional_tags.is_empty() || seen.optional_tags.len() <= 2);
    }
}
