//! The safety layer: what a health search must say before it says anything
//! else.
//!
//! Two parts. A standing line — this is information, not medical advice —
//! that every answer carries. And red flags: queries that describe a
//! possible emergency (chest pain, stroke signs, trouble breathing, thoughts
//! of self-harm, overdose) put an emergency card above the results and the
//! answer, before a single article. The list is deliberately short and
//! phrase-based; a false positive costs one card, a false negative could
//! cost much more, so the matcher is generous (substring over normalised
//! text) and the test pins every phrase.

/// The line every answer carries.
pub const DISCLAIMER: &str =
    "Information only, not medical advice. Talk to a clinician about your own health.";

/// What an emergency card says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alert {
    /// One short heading.
    pub title: &'static str,
    /// What to do, in one sentence.
    pub action: &'static str,
}

const CALL: &str =
    "Call your local emergency number now (112 in India and the EU, 911 in the US, 999 in the UK).";

const EMERGENCY: Alert = Alert {
    title: "This could be an emergency",
    action: CALL,
};

const CRISIS: Alert = Alert {
    title: "You don't have to go through this alone",
    action: "If you might act on these thoughts, call your local emergency number now, or a crisis line (Tele-MANAS 14416 in India, 988 in the US).",
};

/// Phrases that describe a possible medical emergency.
const EMERGENCY_PHRASES: &[&str] = &[
    "chest pain",
    "chest pressure",
    "chest tightness",
    "heart attack",
    "cant breathe",
    "can't breathe",
    "cannot breathe",
    "trouble breathing",
    "difficulty breathing",
    "short of breath",
    "shortness of breath",
    "choking",
    "stroke",
    "face drooping",
    "slurred speech",
    "sudden numbness",
    "worst headache",
    "unconscious",
    "passed out",
    "seizure",
    "overdose",
    "poisoning",
    "anaphylaxis",
    "throat closing",
    "severe bleeding",
    "coughing blood",
    "vomiting blood",
];

/// Phrases about self-harm: the crisis card, not the ambulance one.
const CRISIS_PHRASES: &[&str] = &[
    "suicide",
    "suicidal",
    "kill myself",
    "end my life",
    "self harm",
    "self-harm",
    "hurt myself",
    "want to die",
];

/// The card a query should show first, if any.
#[must_use]
pub fn red_flag(query: &str) -> Option<Alert> {
    let q: String = query
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.is_empty() {
        return None;
    }
    if CRISIS_PHRASES.iter().any(|p| q.contains(p)) {
        return Some(CRISIS);
    }
    if EMERGENCY_PHRASES.iter().any(|p| q.contains(p)) {
        return Some(EMERGENCY);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phrase_raises_its_card() {
        for p in EMERGENCY_PHRASES {
            assert_eq!(red_flag(p), Some(EMERGENCY), "{p}");
            assert_eq!(
                red_flag(&format!("  {}?! ", p.to_uppercase())),
                Some(EMERGENCY),
                "{p}"
            );
        }
        for p in CRISIS_PHRASES {
            assert_eq!(red_flag(p), Some(CRISIS), "{p}");
        }
    }

    #[test]
    fn ordinary_questions_raise_nothing() {
        for q in [
            "migraine",
            "kidney stones",
            "tired all the time",
            "",
            "heart",
            "insomnia",
        ] {
            assert_eq!(red_flag(q), None, "{q}");
        }
    }

    #[test]
    fn phrases_inside_longer_questions_still_count() {
        assert_eq!(red_flag("sharp chest pain when I walk"), Some(EMERGENCY));
        assert_eq!(red_flag("my dad has slurred speech"), Some(EMERGENCY));
    }
}
