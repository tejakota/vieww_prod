//! `Qwen` — Qwen3-0.6B behind the [`Runtime`] trait, which is the whole
//! of the plan's Phase 2 in one sentence.
//!
//! # The shape the engine already promised
//!
//! `engine.rs` in the app committed to a design a year of screens could
//! survive: *the model's work runs on a worker thread with a
//! cancellation token inside the runtime itself; the module's signature
//! stays one call in, one record out*. This is that design, made real:
//!
//! * The caller (UI thread, bench, test) calls [`Runtime::interpret`] /
//!   [`Runtime::assess`] / [`Runtime::decide`] and blocks — bounded by the
//!   cohort's timeout, not by the model's patience.
//! * The worker owns the weights, the tokenizer and the KV cache; nothing
//!   big is ever reachable from the calling thread.
//! * [`Qwen::cancel`] raises a flag the generation loop checks between
//!   tokens and *between prefill chunks* — the person leaving the screen
//!   is noticed in tens of milliseconds, not after a finished answer.
//!
//! # The §5.6 safeguards, as mechanics
//!
//! * **Caps on input and output** — the prompt is tokenised and checked
//!   against the cohort's context bound before any arithmetic; generation
//!   stops at the token ceiling or on the model's own turn-end token.
//! * **Timeouts** — a round trip that outlives its budget is cancelled and
//!   returns [`Error::Cancelled`].
//! * **No retries** — one call, one answer, one error; the caller decides
//!   what a failure means.
//! * **Validated structured output** — the model's text must parse as the
//!   §4.4 contract (or the §4.2 representation) *and* pass
//!   [`Assessment::contract_violations`] before it leaves this crate;
//!   anything else is [`Error::Output`], and the caller falls back to the
//!   deterministic engine. Recovery is tested, not hoped for.
//! * **No cloud** — there is no second path. The only fallback is the
//!   baseline that was already shipping.
//!
//! # Whose words the record keeps
//!
//! [`parse_interpretation`] stamps the *person's own* situation into the
//! record, normalised to whitespace only — the model's paraphrase lives in
//! `intent` and `understanding`, never in the field §2.6 calls history.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use u_core::{Budget, Error};
use u_decision::{
    Assessment, Confidence, Hint, Input, Interpretation, Option_, Origin, SCHEMA_VERSION,
};

use crate::cohort::Caps;
use crate::generation::{Sampling, Stopper};
use crate::model::Weights;
use crate::prompt;
use crate::quantization::{Manifest, Quant, TokenizerManifest};
use crate::tokenizer::Tokenizer;
use crate::Runtime;

/// One job for the worker — the whole §4.3 pipeline, or one stage of it.
enum Job {
    Interpret {
        input: Input,
    },
    Assess {
        seen: Interpretation,
        hints: Vec<Hint>,
    },
}

/// A finished job, tagged with the id it answered — so a reply arriving
/// after the caller gave up waiting is recognised as stale and dropped,
/// rather than corrupting the next request's answer.
struct Reply {
    job: u64,
    result: Result<Payload, Error>,
}

enum Payload {
    Interpretation(Box<Interpretation>),
    Assessment(Box<Assessment>),
}

/// What one generation actually cost — counted where it happened, on
/// the worker, and reported to whoever asked.
///
/// The bench lives off these; so would an event log's honest "the model
/// read 412 tokens and wrote 96" line. Prefill and decode are separate
/// because they are bound by different things (arithmetic vs memory
/// bandwidth) and a cohort decision that cannot see the difference is
/// guessing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timings {
    /// Prompt positions processed before the first token.
    pub prefill_tokens: usize,
    /// Seconds that prefill took.
    pub prefill_seconds: f32,
    /// Tokens the model wrote.
    pub decode_tokens: usize,
    /// Seconds the writing took.
    pub decode_seconds: f32,
}

impl Timings {
    /// Prefill throughput, tokens per second — `None` when nothing was
    /// prefilled.
    #[must_use]
    pub fn prefill_per_second(&self) -> Option<f32> {
        (self.prefill_tokens > 0 && self.prefill_seconds > 0.0)
            .then(|| self.prefill_tokens as f32 / self.prefill_seconds)
    }

    /// Decode throughput, tokens per second — `None` when nothing was
    /// written.
    #[must_use]
    pub fn decode_per_second(&self) -> Option<f32> {
        (self.decode_tokens > 0 && self.decode_seconds > 0.0)
            .then(|| self.decode_tokens as f32 / self.decode_seconds)
    }
}

/// Qwen3-0.6B, on this device, behind the trait the screens already call.
///
/// Built by [`Qwen::load`], which refuses every shortcut the plan names:
/// absent weights, wrong digest, absent markers in the tokenizer. The
/// `model` feature gates the whole type — an APK build today resolves
/// none of it, exactly as the release gates of §10 require until a
/// physical-device report exists.
pub struct Qwen {
    name: &'static str,
    tx: Sender<(u64, Job)>,
    rx: Receiver<Reply>,
    cancel: Arc<AtomicBool>,
    timeout: Duration,
    timings: Arc<std::sync::Mutex<Timings>>,
}

impl std::fmt::Debug for Qwen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The channels and the worker are machinery, not identity: what a
        // person debugs is which model this is and what it is bounded by.
        f.debug_struct("Qwen")
            .field("name", &self.name)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Qwen {
    /// Load the model a manifest vouches for, from a directory holding
    /// the exact files it names, under a cohort's caps.
    ///
    /// Both files are digest-checked *before* anything is parsed — a
    /// tampered file costs one hash pass, never a load.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] for anything absent or unreadable;
    /// [`Error::Unverified`] for a digest that does not match; never a
    /// panic, because a phone discards stderr.
    pub fn load(
        manifest: &Manifest,
        tokenizer: &TokenizerManifest,
        models_dir: &Path,
        caps: Caps,
    ) -> Result<Self, Error> {
        if manifest.family != "qwen3" || manifest.parameters != "0.6B" {
            return Err(Error::NoRuntime("a manifest this runtime knows"));
        }
        let weights_path = models_dir.join(&manifest.file);
        let tokenizer_path = models_dir.join(&tokenizer.file);
        crate::quantization::verify_digest(&weights_path, &manifest.sha256)?;
        crate::quantization::verify_digest(&tokenizer_path, &tokenizer.sha256)?;

        let name = match manifest.quant {
            Quant::Q2K => "qwen3-0.6b-q2k",
            Quant::Q3K => "qwen3-0.6b-q3k",
            Quant::Q4K => "qwen3-0.6b-q4k",
        };
        let tok = Tokenizer::load(&tokenizer_path)?;
        let weights = Weights::load(&weights_path)?;
        let eos = tok.eos_ids();

        let (job_tx, job_rx) = std::sync::mpsc::channel::<(u64, Job)>();
        let (reply_tx, reply_rx) = std::sync::mpsc::channel::<Reply>();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let budget = Budget::DEFAULT;
        let timings = Arc::new(std::sync::Mutex::new(Timings::default()));
        let worker_timings = Arc::clone(&timings);

        std::thread::Builder::new()
            .name("u-qwen".into())
            .spawn(move || {
                let mut worker = Worker {
                    tokenizer: tok,
                    weights,
                    eos,
                    caps,
                    budget,
                    cancel: worker_cancel,
                    timings: worker_timings,
                };
                // The loop ends when the caller goes away: a dropped
                // sender is the shutdown protocol, and no job outlives
                // its own cancellation flag by more than a token.
                while let Ok((id, job)) = job_rx.recv() {
                    let result = worker.run(job);
                    if reply_tx.send(Reply { job: id, result }).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| Error::NoRuntime("a model worker thread"))?;

        Ok(Self {
            name,
            tx: job_tx,
            rx: reply_rx,
            cancel,
            timeout: caps.timeout,
            timings,
        })
    }

    /// What the most recent generation cost, as counted at the source.
    ///
    /// Zero until something has run. The value is replaced, not
    /// accumulated: one decision, one truth.
    #[must_use]
    pub fn last_timings(&self) -> Timings {
        *self.timings.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Cancel the running job, if there is one.
    ///
    /// Semantics, stated honestly: this cancels *the work in flight*. A
    /// flag raised between jobs is simply gone when the next job starts —
    /// the caller that wants no next job does not send one.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Send a job and wait for its answer, bounded by the timeout.
    ///
    /// Stale replies — a job that was cancelled but finished anyway — are
    /// dropped by their job id, which is what keeps the channel honest
    /// without making the caller wait for work it already abandoned.
    fn round_trip(&self, job: Job) -> Result<Payload, Error> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        self.tx
            .send((id, job))
            .map_err(|_| Error::NoRuntime("the model worker"))?;
        let started = Instant::now();
        loop {
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(reply) => {
                    if reply.job == id {
                        return reply.result;
                    }
                    // A stale answer to a job the caller already gave up
                    // on: discard it and keep waiting for ours.
                }
                Err(RecvTimeoutError::Timeout) => {
                    if started.elapsed() > self.timeout {
                        self.cancel.store(true, Ordering::Relaxed);
                        return Err(Error::Cancelled);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Error::NoRuntime("the model worker"));
                }
            }
        }
    }
}

impl Runtime for Qwen {
    fn name(&self) -> &'static str {
        self.name
    }

    /// The trait's cancellation reaches the worker's flag through the
    /// inherent method of the same name — spelled `Qwen::cancel` so the
    /// two never shadow each other by accident.
    fn cancel(&self) {
        Qwen::cancel(self);
    }

    fn interpret(&self, input: &Input, _hints: &[Hint]) -> Result<Interpretation, Error> {
        // Hints are deliberately unread here: §7.5 puts personalisation
        // in the assessment, and a model that saw preferences while
        // reading the situation would smuggle them into the record of
        // what the person actually said. The trait passes them anyway,
        // because the deterministic engine reads them here — and the
        // disagreement is documented, not hidden.
        input.check(&Budget::DEFAULT)?;
        match self.round_trip(Job::Interpret {
            input: input.clone(),
        })? {
            Payload::Interpretation(seen) => Ok(*seen),
            Payload::Assessment(_) => Err(Error::Output {
                what: "the model's interpretation",
            }),
        }
    }

    fn assess(&self, seen: &Interpretation, hints: &[Hint]) -> Result<Assessment, Error> {
        match self.round_trip(Job::Assess {
            seen: seen.clone(),
            hints: hints.to_vec(),
        })? {
            Payload::Assessment(said) => Ok(*said),
            Payload::Interpretation(_) => Err(Error::Output {
                what: "the model's assessment",
            }),
        }
    }
}

impl Drop for Qwen {
    fn drop(&mut self) {
        // Notice the flag at the next token (or prefill chunk) boundary;
        // the channel close then ends the thread. A worker is never
        // joined here — a model mid-arithmetic may take its half-second,
        // and the app should not pay it on the UI thread.
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The model, the tokenizer and everything the generation loop needs —
/// all owned by one thread, all unreachable from the caller.
struct Worker {
    tokenizer: Tokenizer,
    weights: Weights,
    eos: Vec<u32>,
    caps: Caps,
    budget: Budget,
    cancel: Arc<AtomicBool>,
    timings: Arc<std::sync::Mutex<Timings>>,
}

impl Worker {
    fn run(&mut self, job: Job) -> Result<Payload, Error> {
        // A new job is a new life: the previous cancellation is spent.
        self.cancel.store(false, Ordering::Relaxed);
        self.weights.clear_cache();
        match job {
            Job::Interpret { input } => {
                let text = prompt::interpret(&input, &self.budget);
                let said = self.generate(&text)?;
                if std::env::var_os("U_DUMP").is_some() {
                    eprintln!("--- interpret stage raw output ---\n{said}\n--- end ---");
                }
                let seen = parse_interpretation(&said, &input.situation, &self.budget)?;
                Ok(Payload::Interpretation(Box::new(seen)))
            }
            Job::Assess { seen, hints } => {
                let text = prompt::assess(&seen, &hints, &self.budget);
                let said = self.generate(&text)?;
                if std::env::var_os("U_DUMP").is_some() {
                    eprintln!("--- assess stage raw output ---\n{said}\n--- end ---");
                }
                let assessment = parse_assessment(&said, &self.budget)?;
                Ok(Payload::Assessment(Box::new(assessment)))
            }
        }
    }

    /// One generation: prompt in, prose out — with every §5.6 bound
    /// applied inside the loop rather than around it.
    fn generate(&mut self, text: &str) -> Result<String, Error> {
        let ids = self.tokenizer.encode(text)?;
        // The prompt must leave room to write: shrink the generation
        // allowance before refusing, because a situation that used the
        // whole window deserves a short answer more than an error.
        let mut max_new = self.caps.max_new_tokens;
        if ids.len() + max_new > self.caps.max_context_tokens {
            max_new = self.caps.max_context_tokens.saturating_sub(ids.len());
            if max_new < 128 {
                return Err(Error::OverBudget {
                    what: "the prompt",
                    got: ids.len(),
                    bound: self.caps.max_context_tokens,
                });
            }
        }

        // Prefill, chunked so a cancellation is noticed mid-prompt too —
        // §10's "cancellation responsiveness" starts here, not at the
        // first generated token.
        let chunk = 64;
        let mut logits = None;
        let mut offset = 0usize;
        let prefill_started = Instant::now();
        for piece in ids.chunks(chunk) {
            if self.cancel.load(Ordering::Relaxed) {
                self.weights.clear_cache();
                return Err(Error::Cancelled);
            }
            logits = Some(self.weights.logits(piece, offset)?);
            offset += piece.len();
        }
        let prefill_seconds = prefill_started.elapsed().as_secs_f32();
        let mut logits = logits.ok_or(Error::OverBudget {
            what: "the prompt",
            got: 0,
            bound: self.caps.max_context_tokens,
        })?;

        // Decode, one token at a time, checked at every step.
        let stopper = Stopper::new(self.eos.clone(), max_new, Arc::clone(&self.cancel));
        let sampling = Sampling::GREEDY;
        let mut produced: Vec<u32> = Vec::new();
        let decode_started = Instant::now();
        loop {
            let row = logits
                .to_vec1::<f32>()
                .map_err(|_| Error::NoRuntime("a forward the model completed"))?;
            let token = sampling.pick(&row);
            if stopper.ends_turn(token) {
                break;
            }
            produced.push(token);
            if !stopper.continues(produced.len()) {
                break;
            }
            logits = self.weights.logits(&[token], offset)?;
            offset += 1;
        }
        let decode_seconds = decode_started.elapsed().as_secs_f32();
        *self.timings.lock().unwrap_or_else(|p| p.into_inner()) = Timings {
            prefill_tokens: ids.len(),
            prefill_seconds,
            decode_tokens: produced.len(),
            decode_seconds,
        };
        self.tokenizer.decode(&produced)
    }
}

// — Parsing: the model's text, read as the contract or refused ————

/// Pull the first balanced JSON object out of text that may be wrapped in
/// fences, preceded by chatter, or trailed by rambling — all three of
/// which a 0.6B model produces often enough to design for.
fn json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Strings under a key, tolerating a bare string where a list should be —
/// a small model's most common shape error, and one where the honest
/// reading is obvious.
fn strings(value: &serde_json::Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.as_str().map(str::to_owned))
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// An optional string under a key: `null`, absent, or empty all mean
/// "none" — a model that writes `"follow_up_question": ""` has not asked
/// a question, and reading it as one would be inventing dialogue.
fn opt_string(value: &serde_json::Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_owned())
        }
        _ => None,
    }
}

/// Confidence, read case-insensitively and defaulting to the *least*
/// certain word — a model that says "probably high" is a model that is
/// not high.
fn confidence(value: &serde_json::Value) -> Confidence {
    match value.get("confidence").and_then(serde_json::Value::as_str) {
        Some(s) => match s.trim().to_ascii_lowercase().as_str() {
            "high" => Confidence::High,
            "medium" | "moderate" => Confidence::Medium,
            _ => Confidence::Low,
        },
        None => Confidence::Low,
    }
}

/// Where an option came from, read on the safe side: anything the model
/// did not clearly mark as the person's own is a *suggestion*, because
/// passing a suggestion off as the person's own option is the one lie
/// §2.4 forbids outright, while the reverse error only under-claims.
fn origin(value: &serde_json::Value) -> Origin {
    let said = value
        .get("origin")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match said.as_str() {
        "theirs" | "user" | "person" | "given" | "their" => Origin::Theirs,
        _ => Origin::Suggested,
    }
}

/// Options: theirs first if the model said so, suggestions after — and
/// the whole list clamped to the budget's ceiling.
fn options(value: &serde_json::Value, budget: &Budget) -> Vec<Option_> {
    let mut out = Vec::new();
    if let Some(serde_json::Value::Array(items)) = value.get("options") {
        for item in items {
            if let Some(label) = item.get("label").and_then(serde_json::Value::as_str) {
                let label = label.trim();
                if label.is_empty() {
                    continue;
                }
                out.push(Option_ {
                    label: label.to_owned(),
                    origin: origin(item),
                });
            }
        }
    }
    // Theirs first, stable within each group: the person's own options
    // never sit below a suggestion in the list a person reads.
    out.sort_by_key(|o| !matches!(o.origin, Origin::Theirs));
    out.truncate(budget.max_options);
    out
}

/// The §4.2 representation, read out of what the model wrote — with the
/// person's own situation stamped over whatever paraphrase the model
/// returned, because §2.6's history is the person's words, not U's.
///
/// # Errors
/// [`Error::Output`] when there is no JSON object to read, or the object
/// cannot carry a record (missing `intent` or `confidence_reason`): the
/// caller falls back rather than showing a half-remembered reading.
pub fn parse_interpretation(
    text: &str,
    situation: &str,
    budget: &Budget,
) -> Result<Interpretation, Error> {
    let raw = json_object(text).ok_or(Error::Output {
        what: "the model's interpretation",
    })?;
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| Error::Output {
        what: "the model's interpretation",
    })?;
    let intent = opt_string(&value, "intent").ok_or(Error::Output {
        what: "the model's interpretation",
    })?;
    let confidence_reason = opt_string(&value, "confidence_reason").ok_or(Error::Output {
        what: "the model's interpretation",
    })?;

    let mut seen = Interpretation::new(situation.split_whitespace().collect::<Vec<_>>().join(" "));
    seen.schema = SCHEMA_VERSION;
    seen.intent = intent;
    seen.options = options(&value, budget);
    seen.constraints = strings(&value, "constraints");
    seen.priorities = strings(&value, "priorities");
    seen.context = strings(&value, "context");
    seen.time_horizon = opt_string(&value, "time_horizon");
    seen.unknowns = strings(&value, "unknowns");
    seen.assumptions = strings(&value, "assumptions");
    seen.optional_tags = strings(&value, "optional_tags");
    seen.confidence = confidence(&value);
    seen.confidence_reason = confidence_reason;
    budget.clamp_list(&mut seen.constraints);
    budget.clamp_list(&mut seen.priorities);
    budget.clamp_list(&mut seen.context);
    budget.clamp_list(&mut seen.unknowns);
    budget.clamp_list(&mut seen.assumptions);
    Ok(seen)
}

/// The §4.4 assessment, read out of what the model wrote, validated
/// against the contract before it leaves.
///
/// # Errors
/// [`Error::Output`] when there is no JSON, when the spine
/// (`understanding`, `next_step`) is missing or empty, or when the record
/// reports its own contract violations — every one a fall-back condition,
/// never a guess.
pub fn parse_assessment(text: &str, budget: &Budget) -> Result<Assessment, Error> {
    let refuse = || Error::Output {
        what: "the model's assessment",
    };
    let raw = json_object(text).ok_or_else(refuse)?;
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| refuse())?;
    let understanding = opt_string(&value, "understanding").ok_or_else(refuse)?;
    let next_step = opt_string(&value, "next_step").ok_or_else(refuse)?;

    let mut said = Assessment {
        schema: SCHEMA_VERSION,
        understanding,
        options: options(&value, budget),
        relevant_priorities: strings(&value, "relevant_priorities"),
        tradeoffs: strings(&value, "tradeoffs"),
        unknowns: strings(&value, "unknowns"),
        assumptions: strings(&value, "assumptions"),
        assessment: opt_string(&value, "assessment").unwrap_or_default(),
        confidence: confidence(&value),
        next_step,
        follow_up_question: opt_string(&value, "follow_up_question"),
    };
    budget.clamp_list(&mut said.relevant_priorities);
    budget.clamp_list(&mut said.tradeoffs);
    budget.clamp_list(&mut said.unknowns);
    budget.clamp_list(&mut said.assumptions);
    if said.contract_violations().is_empty() {
        Ok(said)
    } else {
        Err(refuse())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_extracted_from_chatter_fences_and_rambling() {
        let said = concat!(
            "Sure! Here's the JSON:\n```json\n",
            r#"{"a": 1, "b": {"c": "}"}}"#,
            "\n```\nHope that helps."
        );
        assert_eq!(json_object(said), Some(r#"{"a": 1, "b": {"c": "}"}}"#));
        assert_eq!(json_object("no braces at all"), None);
        assert_eq!(json_object(r#"{"unterminated": true"#), None);
        // A brace inside a string is not a brace.
        assert_eq!(json_object(r#"{"s": "a}b"}"#), Some(r#"{"s": "a}b"}"#));
    }

    #[test]
    fn a_full_assessment_parses_and_passes_the_contract() {
        let said = r#"{"understanding":"You are weighing a move between cities for work.",
        "options":[{"label":"Move","origin":"Theirs"},{"label":"Visit first","origin":"Suggested"}],
        "relevant_priorities":["career growth","being near family"],
        "tradeoffs":["The move helps growth and costs proximity."],
        "unknowns":["what the new city's rent really is"],
        "assumptions":["the offer described is accurate"],
        "assessment":"The move is worth it if growth outranks proximity for you.",
        "confidence":"medium",
        "next_step":"Spend one weekend in the city before deciding.",
        "follow_up_question":null}"#;
        let a = parse_assessment(said, &Budget::DEFAULT).expect("parses");
        assert_eq!(a.schema, SCHEMA_VERSION);
        assert_eq!(a.options.len(), 2);
        assert!(matches!(a.options[0].origin, Origin::Theirs));
        assert!(matches!(a.options[1].origin, Origin::Suggested));
        assert_eq!(a.confidence, Confidence::Medium);
        assert_eq!(a.follow_up_question, None);
        assert!(a.contract_violations().is_empty());
    }

    #[test]
    fn the_spine_is_required_and_checked() {
        // Missing understanding: refused, not defaulted.
        let no_spine = r#"{"next_step":"Think it over."}"#;
        assert_eq!(
            parse_assessment(no_spine, &Budget::DEFAULT),
            Err(Error::Output {
                what: "the model's assessment"
            })
        );
        // Empty next_step: the contract itself refuses it.
        let empty_spine = r#"{"understanding":"x","next_step":"  "}"#;
        assert!(parse_assessment(empty_spine, &Budget::DEFAULT).is_err());
        // Not JSON at all: refused by name.
        assert_eq!(
            parse_assessment("I could not answer that.", &Budget::DEFAULT),
            Err(Error::Output {
                what: "the model's assessment"
            })
        );
    }

    #[test]
    fn a_bare_string_where_a_list_should_be_is_read_not_refused() {
        let said = r#"{"understanding":"u","next_step":"n","unknowns":"the rent"}"#;
        let a = parse_assessment(said, &Budget::DEFAULT).expect("tolerated");
        assert_eq!(a.unknowns, vec!["the rent".to_owned()]);
    }

    #[test]
    fn origins_default_to_suggested_because_only_that_side_is_safe() {
        let said = r#"{"understanding":"u","next_step":"n",
        "options":[{"label":"Move","origin":"user"},{"label":"Wait","origin":"???"},
        {"label":"Stay","origin":"SUGGESTED"}]}"#;
        let a = parse_assessment(said, &Budget::DEFAULT).expect("parses");
        assert!(matches!(a.options[0].origin, Origin::Theirs));
        assert!(matches!(a.options[1].origin, Origin::Suggested));
        assert!(matches!(a.options[2].origin, Origin::Suggested));
    }

    #[test]
    fn the_persons_own_situation_overwrites_the_models_paraphrase() {
        let said = r#"{"intent":"a relocation decision","confidence_reason":"clear words",
        "situation":"the user wants to relocate for work"}"#;
        let seen = parse_interpretation(
            said,
            "  I'm considering   moving to another city ",
            &Budget::DEFAULT,
        )
        .expect("parses");
        assert_eq!(
            seen.situation, "I'm considering moving to another city",
            "whitespace normalised, words the person's own"
        );
        assert_eq!(seen.intent, "a relocation decision");
        assert_eq!(seen.schema, SCHEMA_VERSION);
    }

    #[test]
    fn an_interpretation_without_intent_or_reason_is_refused() {
        let bare = r#"{"situation":"x"}"#;
        assert_eq!(
            parse_interpretation(bare, "x", &Budget::DEFAULT),
            Err(Error::Output {
                what: "the model's interpretation"
            })
        );
    }

    #[test]
    fn unknown_confidence_words_read_as_low_and_lists_are_clamped() {
        let said = r#"{"intent":"i","confidence_reason":"r","confidence":"probably high!",
        "unknowns":["a","b","c","d","e","f","g"]}"#;
        let seen = parse_interpretation(said, "s", &Budget::DEFAULT).expect("parses");
        assert_eq!(seen.confidence, Confidence::Low);
        assert_eq!(seen.unknowns.len(), Budget::DEFAULT.max_list_items);
    }

    /// The full pipeline against the real model — Phase 2's own exit
    /// criterion, run wherever the developer has fetched the weights
    /// (see `docs/models.md`). Skipped otherwise, and never silently.
    #[test]
    fn the_real_model_decides_a_real_decision_end_to_end() {
        let Some(dir) = std::env::var_os("U_MODELS_DIR").map(std::path::PathBuf::from) else {
            eprintln!("skipping: U_MODELS_DIR is not set");
            return;
        };
        // Best-first: the plan's §5.2 table read bottom-up, so the test
        // proves what the best present quant can do — a developer who
        // fetched only Q2_K gets the honest measurement of exactly that,
        // and the report in docs/models.md is what interprets it.
        let Some(manifest) = Manifest::embedded()
            .iter()
            .rev()
            .find(|m| dir.join(&m.file).is_file())
            .map(|m| m.to_owned())
        else {
            eprintln!("skipping: no weights from the manifests in {dir:?}");
            return;
        };
        let tokenizer = TokenizerManifest::embedded();
        // The desktop cohort's caps, not a phone's: a real decision on a
        // shared CI runner is a ~2-minute fact (see docs/models.md), and
        // a timeout tuned for a phone would cancel the very work the test
        // exists to prove happens.
        let qwen = Qwen::load(
            &manifest,
            &tokenizer,
            &dir,
            crate::cohort::Cohort::SixGbPlus.caps(),
        )
        .expect("the pinned model loads");
        let input = Input {
            situation: "I'm considering moving to another city for work, but I'm not sure the change is worth it.".into(),
            options: vec!["Move to the new city".into(), "Stay here".into()],
            ..Input::default()
        };
        let said = qwen
            .decide(&input, &[])
            .expect("the model answers within the caps");
        assert!(said.contract_violations().is_empty());
        assert!(
            !said.understanding.is_empty(),
            "the contract's spine is present"
        );
        // §2.4, as far as a single run can testify: the person's own
        // options are recognised as their own. Per-origin honesty across
        // many scenarios is the bench's statistic, not one test's
        // assertion — a 0.6B model may mislabel one option on one day
        // without the gate failing on a fact the evaluation set measures.
        assert!(
            said.options
                .iter()
                .any(|o| matches!(o.origin, Origin::Theirs)),
            "the person's own options are recognised: {:?}",
            said.options
        );
    }
}
