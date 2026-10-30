//! The question bank: what home asks, one decision at a time.
//!
//! Each [`Ask`] is a small forced choice — the shape of a decision with the
//! stakes left out, which is the only shape a prototype can ask twelve times
//! without becoming a survey. Answering is the training: the answer is an
//! event in the log, shown back and counted, and that is all. The point of
//! the `tag` is the step after this prototype: an answer is an event, the
//! tag is the domain it belongs to, and the domains are where a θ parameter
//! will eventually live.
//!
//! The bank serves two phases, and the difference is everything: the first
//! pass is the **sketch** — twelve answers, counted on the card, completed
//! once and never repeated — and every pass after it is the **daily
//! rhythm**, where the questions rotate for variety and nothing is counted
//! by the bank again. A bank that counted its own rotation would be a
//! questionnaire that restarts at one, which is the bug this shape exists
//! to not be.
//!
//! The order is hand-set so consecutive questions never repeat a domain or a
//! mood — a run of money questions reads as a debt collection, not a persona.

/// One training question: a situation, two answers of equal weight, and the
/// domain the eventual event log will file the answer under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ask {
    /// The situation, ending in an ellipsis the answers complete.
    pub prompt: &'static str,
    /// The first answer.
    pub a: &'static str,
    /// The second answer.
    pub b: &'static str,
    /// The domain the answer belongs to — the event log's key.
    pub tag: &'static str,
}

/// The bank: one pass for the sketch, then rotation for the daily rhythm.
pub const ASKS: [Ask; 12] = [
    Ask {
        prompt: "A meeting runs long into your lunch. Do you…",
        a: "Stay and finish",
        b: "Leave on time",
        tag: "time",
    },
    Ask {
        prompt: "A free evening appears. You would rather…",
        a: "See people",
        b: "Be alone",
        tag: "energy",
    },
    Ask {
        prompt: "A friend asks to borrow a meaningful sum. You…",
        a: "Lend it",
        b: "Decline politely",
        tag: "money",
    },
    Ask {
        prompt: "An idea has you at midnight. You…",
        a: "Start now",
        b: "Sleep on it",
        tag: "impulse",
    },
    Ask {
        prompt: "An old plan is not working. You…",
        a: "Push harder",
        b: "Let it go",
        tag: "persistence",
    },
    Ask {
        prompt: "A safer path and a bolder one open. You take…",
        a: "The safer one",
        b: "The bolder one",
        tag: "risk",
    },
    Ask {
        prompt: "Perfect weather, pending work. You…",
        a: "Go outside",
        b: "Finish first",
        tag: "discipline",
    },
    Ask {
        prompt: "Someone mildly wrongs you. You…",
        a: "Say something",
        b: "Let it pass",
        tag: "conflict",
    },
    Ask {
        prompt: "A task could be done well or done today. You…",
        a: "Done well",
        b: "Done today",
        tag: "standards",
    },
    Ask {
        prompt: "A discount ends tonight. You…",
        a: "Buy now",
        b: "Walk away",
        tag: "money",
    },
    Ask {
        prompt: "A weekend with nothing planned. You…",
        a: "Plan it out",
        b: "Let it unfold",
        tag: "control",
    },
    Ask {
        prompt: "A stranger's idea beats yours. You…",
        a: "Back theirs",
        b: "Defend yours",
        tag: "pride",
    },
];
