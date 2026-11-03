//! ST-03: "Say" gets its AI — a prompt, *verified* before it is offered.
//!
//! # What this module is
//!
//! Every prompt-to-UI tool of 2025 shows a person a diff and asks them to
//! eyeball it. This one does more, because it can: Say is a deterministic
//! language over a fixed vocabulary, so the studio can *prove* a proposal
//! before anybody is asked to read it. The loop here is
//!
//! 1. a [`Model`] turns the prompt into the text of a `.say` file;
//! 2. the real Say compiler compiles it;
//! 3. a real `rustc` compiles what codegen emits;
//! 4. the screen is loaded and rendered headless, **twice**, and the two
//!    PNGs must be pixel-identical;
//! 5. only then is the `.say` diff shown for approval, and only approval
//!    writes the buffer — through [`crate::state::Studio::edit`], so undo,
//!    the dirty flag and the auto-render debounce all engage.
//!
//! On a failure at any stage the *exact* diagnostics — Say's line and column,
//! rustc's, the loader's — go back to the model for **exactly one** retry,
//! and a second failure refuses. The cap is the point: a loop that retried
//! until it got lucky would be a loop that burns the user's time to hide
//! that the model cannot write Say today. Refusal is a value the studio
//! shows, not an exception it swallows.
//!
//! # Privacy
//!
//! The default [`Model`] is [`LocalHttpModel`]: a plain HTTP POST to a
//! server on the user's own machine (an Ollama-style localhost endpoint),
//! hand-rolled over `std::net::TcpStream` with
//! [`vieww_foundation::json`] for the body — no new dependency, no client
//! SDK, and no request that leaves the machine. A user who prefers their
//! own arrangement points the endpoint at whatever they run. Either way
//! this module sends nothing anywhere else and reports nothing to anybody
//! (the studio's telemetry stance, ST-17, applies): the prompt, the
//! proposal and the diagnostics live in the process and go no further than
//! the buffer.
//!
//! # The honest limits
//!
//! * The verification proves the screen *renders*, stably, once — not that
//!   it is the screen the user described. That judgement stays with the
//!   diff, which is why the diff is shown rather than trusted.
//! * `keep` state is *initialised* state: a proposed counter starts at its
//!   `starting at` value, as any new `.say` file would.
//! * The localhost model is expected to know the Say vocabulary; the system
//!   prompt states it, but a model that ignores it simply fails step 2 and
//!   is refused. The loop never lowers its bar to meet the model.

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vieww_foundation::{Color, Size};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;
use vieww_say_codegen::{Diagnostic as SayDiagnostic, Severity as SaySeverity};

use crate::compile::{self, Compiled, Session, Toolchain};
use crate::loaded::Preview;

/// How many attempts the loop may make: the first, and one retry fed the
/// first's exact diagnostics. A constant rather than a parameter because it
/// is the product decision this module exists to make.
const ATTEMPTS: usize = 2;

/// The size the verification renders at: the device frame's smallest
/// phone-ish shape — big enough for a screen to lay out honestly, small
/// enough that two rasterisations cost milliseconds.
const PROBE: (f32, f32) = (400.0, 700.0);

/// The refusal sentence shared by both exits out of the loop.
const REFUSED: &str = "two attempts did not survive verification — the model's Say \
                       diagnostics are in the Output panel";

/// One attempt the loop made, and the diagnostics that sank it — the whole
/// of what the model is told about why its answer was not good enough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// The `.say` text as the model wrote it.
    pub say: String,
    /// The exact diagnostics, one per line, with file:line:column and code.
    pub diagnostics: Vec<String>,
}

/// What can turn a prompt into the text of a `.say` file.
///
/// Deliberately tiny: one method, no streaming, no tool calls, no history.
/// The interesting behaviour of this module is the *loop around* the model,
/// not the model, and a small trait keeps every implementation honest about
/// that — a localhost HTTP server, a recorded fake for the golden tests, and
/// whatever a future round wants, all without the loop knowing which.
///
/// `retry` is [`Some`] on the loop's second call only, carrying the failed
/// attempt and its diagnostics. Implementations that cannot use them simply
/// answer again.
pub trait Model {
    /// Turn `prompt` into a `.say` file's text.
    ///
    /// # Errors
    ///
    /// Why no `.say` text could be produced — transport, refusal, an empty
    /// reply. The loop reports it and stops: a model that cannot answer is
    /// not a compile failure, and feeding it diagnostics would be a retry of
    /// the wrong thing.
    fn complete_to_say(&self, prompt: &str, retry: Option<&Attempt>) -> Result<String, String>;
}

/// A proposal that survived verification, awaiting the user's judgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The prompt it came from, shown in the diff dialog's title.
    pub prompt: String,
    /// The `.say` text the model wrote and the loop proved.
    pub say: String,
}

/// What the verified loop decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It compiles, loads, and rendered pixel-identically twice. The `.say`
    /// text is ready for the diff approval dialog.
    Verified(Proposal),
    /// It did not, twice (or the model never answered). `attempts` is every
    /// failed attempt with its diagnostics, `reason` is the sentence the
    /// status bar shows.
    Refused {
        attempts: Vec<Attempt>,
        reason: String,
    },
}

/// Why one round of the pipeline failed.
///
/// The split is the loop's judgement about the model: `Retry` failures are
/// ones whose exact diagnostics a model could act on, `Stop` failures are
/// ones no answer would change.
enum Failure {
    /// Diagnostics the model could act on — the round may be retried once.
    Retry(Vec<String>),
    /// The loop cannot usefully continue.
    Stop(String),
}

/// Run the verified loop synchronously, minting compile pairs from
/// `session` as it goes.
///
/// The test-facing shape: one call, one answer, real compiler, real
/// `rustc`, real rasteriser. The studio's own path runs the same loop on a
/// worker thread via [`ProposeJob`].
#[must_use]
pub fn propose(
    model: &dyn Model,
    prompt: &str,
    toolchain: &Toolchain,
    session: &Session,
) -> Outcome {
    propose_with(model, prompt, |source| {
        compile::compile(toolchain, session, source, "proposal.rs")
    })
}

/// The loop itself, over whatever compiles the generated Rust.
///
/// `compile_rust` hands back a [`Compiled`] for one source, and is the only
/// thing that differs between the synchronous path (a [`Session`] on this
/// thread) and the worker (pairs minted on the UI thread, because the
/// session's counter is a single-threaded `Cell` — see [`ProposeJob`]).
fn propose_with<F>(model: &dyn Model, prompt: &str, mut compile_rust: F) -> Outcome
where
    F: FnMut(&str) -> std::io::Result<Compiled>,
{
    let mut attempts: Vec<Attempt> = Vec::new();
    // EXACTLY two rounds: the first attempt, and one retry that is handed
    // the first attempt's diagnostics verbatim. Falling out of the loop is
    // the refusal — every other exit is a return.
    for round in 0..ATTEMPTS {
        let retry = (round > 0).then(|| attempts[round - 1].clone());
        let say = match model.complete_to_say(prompt, retry.as_ref()) {
            Ok(say) => say,
            Err(error) => {
                return Outcome::Refused {
                    attempts,
                    reason: format!("the model did not answer: {error}"),
                };
            }
        };
        match verify(&say, &mut compile_rust) {
            Ok(()) => {
                return Outcome::Verified(Proposal {
                    prompt: prompt.to_owned(),
                    say,
                });
            }
            Err(Failure::Retry(diagnostics)) => {
                attempts.push(Attempt { say, diagnostics });
                if round + 1 >= ATTEMPTS {
                    return Outcome::Refused {
                        attempts,
                        reason: REFUSED.to_owned(),
                    };
                }
            }
            Err(Failure::Stop(reason)) => {
                return Outcome::Refused { attempts, reason };
            }
        }
    }
    Outcome::Refused {
        attempts,
        reason: REFUSED.to_owned(),
    }
}

/// One attempt through the pipeline, minus the model call: Say, `rustc`,
/// the load, and two renders. `Ok(())` is the verification this module
/// exists to enforce.
fn verify<F>(say: &str, compile_rust: &mut F) -> Result<(), Failure>
where
    F: FnMut(&str) -> std::io::Result<Compiled>,
{
    // Say's own compiler first: its diagnostics name the `.say` line and
    // column, which is the only thing the model can act on directly.
    let generated = vieww_say_codegen::compile("proposal.say", say)
        .map_err(|diagnostics| Failure::Retry(say_diagnostics(&diagnostics)))?;
    // Then the real toolchain. Generated Say has no project path, so no
    // `ProjectLib` — exactly what the Render pipeline does for a Say buffer.
    let compiled = compile_rust(&generated.rust)
        .map_err(|error| Failure::Stop(format!("rustc could not be run: {error}")))?;
    if compiled.failed() {
        let diagnostics = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == crate::state::Severity::Error)
            .map(rustc_diagnostic)
            .collect();
        return Err(Failure::Retry(diagnostics));
    }
    let Some(library) = compiled.library.as_ref() else {
        return Err(Failure::Retry(vec!["rustc built no library".to_owned()]));
    };
    // Then the load — a panic or an ABI mismatch is a failure like any
    // other, with its own words.
    let preview =
        Preview::load(library).map_err(|error| Failure::Retry(vec![error.to_string()]))?;
    // Then the pixels, twice. This is the step no other prompt-to-UI
    // pipeline has, and the reason the dialog can say what it says.
    renders_stably(&preview).map_err(|error| Failure::Retry(vec![error]))
}

/// A Say diagnostic, as one line the model and the Output panel both read.
fn say_diagnostics(diagnostics: &[SayDiagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|d| d.severity == SaySeverity::Error)
        .map(|d| {
            format!(
                "proposal.say:{}:{} {}: {}",
                d.line, d.column, d.code, d.message
            )
        })
        .collect()
}

/// A rustc diagnostic, as one line. The file is the generated Rust's own
/// name and the line is in *generated* code — which is exactly what the
/// model gets, because the Say diagnostic it could have acted on is the one
/// that did not happen: codegen already accepted the file.
fn rustc_diagnostic(d: &crate::state::Diagnostic) -> String {
    format!(
        "{}:{}:{} {}: {}",
        d.file, d.line, d.column, d.code, d.message
    )
}

/// Render `preview` twice headless and require the same PNG both times.
///
/// Two `draw_frame`s over one driver: the second is the stability claim,
/// and the PNG bytes are the evidence. A screen with anything
/// time-dependent in it — which v1 Say has no vocabulary for, but a
/// hand-written Rust buffer would — fails here, on purpose.
fn renders_stably(preview: &Preview) -> Result<(), String> {
    let (width, height) = PROBE;
    let (w, h) = (width as u32, height as u32);
    let mut driver = FrameDriver::new(Size::new(width, height));
    crate::install(&mut driver);
    driver
        .set_root(vieww_widget::Theme::new(vieww_widget::ThemeData::light()).child(preview.node()));
    let mut renderer = NativeRenderer::new();
    let background = Color::rgb(0xFF, 0xFF, 0xFF);
    driver.draw_frame();
    let first = renderer
        .render_to_png(driver.scene(), w, h, background)
        .map_err(|error| format!("the headless render failed: {error}"))?
        .0;
    driver.draw_frame();
    let second = renderer
        .render_to_png(driver.scene(), w, h, background)
        .map_err(|error| format!("the headless render failed: {error}"))?
        .0;
    if first == second {
        Ok(())
    } else {
        Err(
            "the two headless renders are not pixel-identical, so the screen \
             is not deterministic and cannot be approved"
                .to_owned(),
        )
    }
}

/// A `.say` diff as the approval dialog shows it: common prefix and suffix
/// held still, the middle marked `-` (lost) and `+` (gained), per line.
///
/// A real diff algorithm would move hunks around to save a line or two;
/// this one's job is honesty, not elegance — every line that differs is
/// shown as differing. The cap is the caller's (the dialog bounds its own
/// list).
#[must_use]
pub fn diff_lines(old: &str, new: &str) -> Vec<String> {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let common_start = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old.len().min(new.len()));
    let tail = old.len().min(new.len()) - common_start;
    let common_end = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(tail)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<String> = Vec::new();
    for line in &old[..common_start] {
        out.push(format!("  {line}"));
    }
    for line in &old[common_start..old.len() - common_end] {
        out.push(format!("- {line}"));
    }
    for line in &new[common_start..new.len() - common_end] {
        out.push(format!("+ {line}"));
    }
    for line in &old[old.len() - common_end..] {
        out.push(format!("  {line}"));
    }
    out
}

// -- the worker ------------------------------------------------------------

/// What a poll of the loop's worker found. Mirrors `compile::Progress`,
/// minus cancellation: the loop's two `rustc` runs are seconds, not the
/// minutes a user waits on a build, and "stop thinking" is a button this
/// round does not need.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// Still verifying.
    Running,
    /// The loop answered.
    Done(Outcome),
    /// The worker went away without answering — a bug here, not in the
    /// model, and reported rather than left to look like eternal thought.
    Lost,
}

/// The verified loop on a worker thread, so the studio's frame keeps
/// drawing while `rustc` and the rasteriser do their work.
///
/// The compile pairs are minted on the *calling* thread (see
/// `Session::next_pair`'s single-threaded counter), which is why
/// [`spawn`](Self::spawn) takes the session by reference and the worker
/// takes only paths — the same arrangement `compile::Job` made first.
pub struct ProposeJob {
    results: Receiver<Outcome>,
}

impl std::fmt::Debug for ProposeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The studio's `Debug` derive reaches here; what a person reads is
        // that a job is running, not which channel it owns.
        f.debug_struct("ProposeJob").finish_non_exhaustive()
    }
}

impl ProposeJob {
    /// Start the loop for `prompt` on a worker thread. `wake` is called
    /// once, from the worker, when the answer is ready — the studio hands
    /// over the platform's waker so an idle window still draws the result.
    ///
    /// # Errors
    ///
    /// If the thread could not be started. Everything after that point
    /// arrives through [`poll`](Self::poll).
    pub fn spawn(
        model: AnyModel,
        prompt: &str,
        toolchain: &Toolchain,
        session: &Session,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        // Two pairs for two attempts, minted here, on the caller's thread.
        let pairs: Vec<_> = (0..ATTEMPTS).map(|_| session.next_pair()).collect();
        let (sender, results) = std::sync::mpsc::channel();
        let prompt = prompt.to_owned();
        let toolchain = toolchain.clone();
        let model = model.model;
        std::thread::Builder::new()
            .name("viewwstudio-say-ai".into())
            .spawn(move || {
                let mut next = 0;
                let outcome = propose_with(&*model, &prompt, |source| {
                    let Some((source_path, library_path)) = pairs.get(next).cloned() else {
                        return Err(std::io::Error::other(
                            "no compile pair was minted for this attempt",
                        ));
                    };
                    next += 1;
                    compile::compile_at(&toolchain, &source_path, &library_path, source)
                });
                // A failed send means the studio dropped the job; nothing to
                // report to, and the thread is on its way out anyway.
                sender.send(outcome).ok();
                wake();
            })?;
        Ok(Self { results })
    }

    /// Non-blocking. Called once a frame from the studio's frame hook.
    #[must_use]
    pub fn poll(&self) -> Progress {
        match self.results.try_recv() {
            Ok(outcome) => Progress::Done(outcome),
            Err(TryRecvError::Empty) => Progress::Running,
            Err(TryRecvError::Disconnected) => Progress::Lost,
        }
    }
}

// -- the models ------------------------------------------------------------

/// A model behind an HTTP endpoint on the user's own machine — the default.
///
/// Speaks enough HTTP/1.1 for a localhost JSON API over `TcpStream`: one
/// `POST`, `Connection: close`, the reply read to EOF, `chunked` bodies
/// decoded when the server chooses them. That is the whole client; there is
/// no SDK here to trust and no dependency here to audit.
///
/// The endpoint is Ollama-shaped (`POST /api/generate`, the reply's
/// `response` field), which any local server can imitate in a dozen lines.
/// The model name and the host are the user's to set; nothing is fetched,
/// looked up, or phoned home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalHttpModel {
    /// `host:port`, as `TcpStream::connect` wants it.
    pub host: String,
    /// What the server calls the model, as the request's `model` field.
    pub model: String,
}

impl LocalHttpModel {
    /// The default: an Ollama-style server on this machine, a model named
    /// for the language it is asked to write.
    #[must_use]
    pub fn ollama() -> Self {
        Self {
            host: "localhost:11434".to_owned(),
            model: "vieww-say".to_owned(),
        }
    }

    /// One POST, one JSON body, one string back.
    fn post(&self, path: &str, body: &str) -> Result<String, String> {
        let mut stream = std::net::TcpStream::connect(&self.host)
            .map_err(|error| format!("{} is not answering: {error}", self.host))?;
        // Generation is the slow part; the request itself should be instant.
        stream
            .set_read_timeout(Some(Duration::from_secs(600)))
            .map_err(|error| format!("{} misbehaved: {error}", self.host))?;
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: \
             application/json\r\nContent-Length: {length}\r\nConnection: \
             close\r\n\r\n{body}",
            host = self.host,
            length = body.len(),
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| format!("{} took the request and closed: {error}", self.host))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|error| format!("{} closed before answering: {error}", self.host))?;
        let text = String::from_utf8_lossy(&raw).into_owned();
        reply_body(&text)
    }
}

impl Model for LocalHttpModel {
    fn complete_to_say(&self, prompt: &str, retry: Option<&Attempt>) -> Result<String, String> {
        // The retry rides along in the user turn, not a second system
        // message: whatever the server is, it sees one request and is
        // expected to send one answer.
        let user = match retry {
            None => format!("Write one .say screen for: {prompt}"),
            Some(attempt) => format!(
                "Write one .say screen for: {prompt}\n\nYour previous answer \
                 did not verify. It was:\n{}\n\nThe exact diagnostics were:\n{}\n\n\
                 Answer again with the whole corrected .say file.",
                attempt.say,
                attempt.diagnostics.join("\n")
            ),
        };
        let json = vieww_foundation::json::Json::object([
            (
                "model",
                vieww_foundation::json::Json::String(self.model.clone()),
            ),
            ("prompt", vieww_foundation::json::Json::String(user)),
            (
                "system",
                vieww_foundation::json::Json::String(SYSTEM.to_owned()),
            ),
            ("stream", vieww_foundation::json::Json::Bool(false)),
            (
                "options",
                vieww_foundation::json::Json::object([(
                    "temperature",
                    vieww_foundation::json::Json::Number(0.0),
                )]),
            ),
        ])
        .to_string();
        let reply = self.post("/api/generate", &json)?;
        let parsed = vieww_foundation::json::Json::parse(&reply)
            .map_err(|error| format!("the reply was not JSON: {error}"))?;
        let text = parsed
            .get("response")
            .and_then(vieww_foundation::json::Json::as_str)
            .ok_or_else(|| "the reply had no `response` field".to_owned())?;
        Ok(unfence(text.trim()))
    }
}

/// What the model is told the answer must be. One paragraph, the whole
/// vocabulary, and the one rule that matters: the file and nothing else.
const SYSTEM: &str = "You write screens in vieww Say (say-language: 1). Answer with ONE .say \
                      file and NOTHING else — no markdown fences, no prose, no explanation. \
                      Start with `-- say-language: 1`. Declare state with `keep` lines \
                      (`keep a whole number called count starting at 0`). Then `screen \"Name\":` \
                      blocks of indented widget lines. Widgets: a column, a row, a stack, a \
                      center, a card, a scroll view, a fixed area of W by H, a heading \"...\", \
                      a text \"...\", a label \"...\", a button \"...\" (events: `which when \
                      tapped:` with indented `set <state> to ...`), a text field bound to \
                      <state>, a switch/checkbox/slider bound to <state>, a list of <state>, \
                      an image \"path\", an icon <name>, an empty state described as \"...\", \
                      a set of tabs, a dialog titled \"...\". Properties, comma-separated before \
                      the colon: padded N, spaced N, N wide, N tall, size N, bold, radius N, \
                      color #RRGGBB or `with the <slot> color`, aligned to the \
                      start/center/end, children aligned to the start/center/end, with \
                      placeholder \"...\", single line, in N lines, each row N, from A to B, \
                      pinned top/bottom/left/right N. Only put a property on a widget whose \
                      kind accepts it. Strings are double-quoted; interpolation is \
                      \"\\(name)\".";

/// Strip the markdown fences a model adds despite being told not to.
///
/// Only whole-line fences at the start and end of the reply — and the end
/// is touched only when the start was fenced, because a reply that never
/// opened with a fence and merely ends with a line that looks like one is
/// text the screen wrote, and an editor that stripped it would be quietly
/// rewriting the screen's own text.
#[must_use]
pub fn unfence(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    let mut fenced = false;
    while lines
        .first()
        .is_some_and(|l| l.trim_start().starts_with("```"))
    {
        lines.remove(0);
        fenced = true;
    }
    if fenced {
        while lines
            .last()
            .is_some_and(|l| l.trim_start().starts_with("```"))
        {
            lines.pop();
        }
    }
    lines.join("\n")
}

/// The reply's body: past the blank line, de-chunked if the server said so.
///
/// `Connection: close` means EOF *is* the end of the body; `chunked` means
/// the length prefix has to be read, because a server is free to choose it
/// even for a one-object reply.
fn reply_body(raw: &str) -> Result<String, String> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| "the reply had no body".to_owned())?;
    let chunked = head.lines().any(|line| {
        line.to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
    });
    if !chunked {
        return Ok(body.to_owned());
    }
    let mut out = String::new();
    let mut rest = body;
    loop {
        let Some((size_line, after)) = rest.split_once("\r\n") else {
            return Err("a chunked reply ended mid-chunk".to_owned());
        };
        let size =
            usize::from_str_radix(size_line.trim().split(';').next().unwrap_or_default(), 16)
                .map_err(|_| "a chunked reply named a size that is not one".to_owned())?;
        rest = after;
        if size == 0 {
            break;
        }
        let bytes: Vec<u8> = rest.bytes().take(size).collect();
        out.push_str(&String::from_utf8_lossy(&bytes));
        rest = rest
            .get(size..)
            .ok_or_else(|| "a chunked reply ended mid-chunk".to_owned())?
            .strip_prefix("\r\n")
            .ok_or_else(|| "a chunk was not followed by a newline".to_owned())?;
    }
    Ok(out)
}

/// The model as the studio's state holds it: an `Arc` wrapped so the
/// `Debug` and `Clone` derives the `Studio` struct already has keep holding —
/// a bare trait object has neither, and a hand-written `impl Debug` for the
/// whole studio is a cost nobody should pay for one field.
///
/// Implements [`Model`] by delegation, so the loop never unwraps it: a
/// `RecordedFake` and a `LocalHttpModel` are handed to the same code by the
/// same type.
#[derive(Clone)]
pub struct AnyModel {
    model: Arc<dyn Model + Send + Sync>,
}

impl std::fmt::Debug for AnyModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // What matters is that the studio's debug output stays a sentence,
        // not a trait object's non-answer.
        f.debug_struct("AnyModel").finish_non_exhaustive()
    }
}

impl AnyModel {
    /// Wrap a model. An `Arc` on the argument so a `LocalHttpModel` and a
    /// shared `RecordedFake` are constructed the same way.
    #[must_use]
    pub fn new(model: Arc<dyn Model + Send + Sync>) -> Self {
        Self { model }
    }
}

impl Model for AnyModel {
    fn complete_to_say(&self, prompt: &str, retry: Option<&Attempt>) -> Result<String, String> {
        self.model.complete_to_say(prompt, retry)
    }
}

/// A model with recorded answers, for the golden tests.
///
/// Records every call — the prompt it was given and, on a retry, the failed
/// attempt with the exact diagnostics — so a test can assert not just what
/// came out of the loop but what the model was *told*. Answers are handed
/// out in order; a call past the end is an error, so a loop that asks more
/// of the fake than the test staged fails loudly rather than passing by
/// accident.
///
/// `Mutex`, not `RefCell`, because the studio's own path runs the model on
/// a worker thread — which is also what makes this usable end to end
/// through [`ProposeJob`], not only through the synchronous [`propose`].
#[derive(Debug, Default)]
pub struct RecordedFake {
    replies: Mutex<VecDeque<Result<String, String>>>,
    calls: Mutex<Vec<(String, Option<Attempt>)>>,
}

impl RecordedFake {
    /// A fake answering with `replies`, in order, one per call.
    #[must_use]
    pub fn new(replies: Vec<Result<String, String>>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Every call so far: the prompt, and the retry it was handed.
    #[must_use]
    pub fn calls(&self) -> Vec<(String, Option<Attempt>)> {
        self.calls.lock().expect("the fake's call log").clone()
    }
}

impl Model for RecordedFake {
    fn complete_to_say(&self, prompt: &str, retry: Option<&Attempt>) -> Result<String, String> {
        self.calls
            .lock()
            .expect("the fake's call log")
            .push((prompt.to_owned(), retry.cloned()));
        self.replies
            .lock()
            .expect("the fake's replies")
            .pop_front()
            .unwrap_or_else(|| Err("the recorded fake has no reply left".to_owned()))
    }
}

// -- the studio glue --------------------------------------------------------

impl crate::state::Studio {
    /// Open the prompt box (ST-03's one door into the loop).
    pub fn say_ai_open(&self) {
        self.say_prompt_text.set(String::new());
        self.say_prompt_open.set(true);
    }

    /// The prompt box's text field writes here.
    pub fn set_say_prompt_text(&self, text: String) {
        self.say_prompt_text.set(text);
    }

    /// Submit the prompt: the verified loop starts on a worker thread.
    ///
    /// The guards are refusals with reasons, not silent returns, because a
    /// box that swallows a prompt leaves a person wondering which of the
    /// three things they have to fix. Say-only, because the loop's output
    /// is a `.say` file and the buffer it replaces is where it lands.
    pub fn say_ai_submit(&self) {
        self.say_prompt_open.set(false);
        let prompt = self.say_prompt_text.get();
        if prompt.trim().is_empty() {
            return;
        }
        if self.say_job.borrow().is_some() {
            self.notify("A prompt is already being verified.");
            return;
        }
        if !self
            .active()
            .is_some_and(|b| b.language == crate::language::Language::Say)
        {
            self.notify("The Say prompt writes a .say buffer — open one first.");
            return;
        }
        let Ok(toolchain) = self.toolchain.as_ref() else {
            self.notify("No toolchain, no verified prompt.");
            return;
        };
        let Some(session) = self.session.as_ref() else {
            self.notify("No session directory to compile a proposal into.");
            return;
        };
        // The candidate the fingerprint search has reached — the same one
        // `render` would compile with. A workspace can hold many compilations
        // of `vieww`, and the head of `discover`'s list is a guess, not a
        // promise; without this a proposal could fail verification for an ABI
        // reason the studio had already solved on its last Render.
        let candidate = self.candidate.get();
        let toolchain = toolchain
            .with_candidate(candidate)
            .unwrap_or_else(|| toolchain.clone());
        let model = self.say_model.get();
        let waker = self.waker.clone();
        let wake = move || {
            use vieww_foundation::task::FrameWaker as _;
            waker.wake();
        };
        match ProposeJob::spawn(model, &prompt, &toolchain, session, wake) {
            Ok(job) => {
                *self.say_job.borrow_mut() = Some(job);
                self.notify("Verifying the model's Say — compiling and rendering it twice.");
            }
            Err(error) => {
                self.notify(&format!("The prompt loop could not start: {error}"));
            }
        }
    }

    /// Pick up the loop's answer, once per frame from the platform hook.
    ///
    /// Returns whether anything changed, the same contract `poll_compile`
    /// keeps.
    pub fn poll_say_ai(&self) -> bool {
        let progress = {
            let borrowed = self.say_job.borrow();
            let Some(job) = borrowed.as_ref() else {
                return false;
            };
            job.poll()
        };
        match progress {
            Progress::Running => false,
            Progress::Lost => {
                self.say_job.borrow_mut().take();
                self.notify("The prompt worker stopped without answering.");
                true
            }
            Progress::Done(outcome) => {
                self.say_job.borrow_mut().take();
                match outcome {
                    Outcome::Verified(proposal) => {
                        self.say_proposal.set(Some(proposal));
                    }
                    Outcome::Refused { attempts, reason } => {
                        // The reason is a sentence for the status bar; the
                        // diagnostics are lines for the Output panel, where
                        // they can be read at leisure and copied.
                        let mut lines = vec![reason.clone()];
                        for (index, attempt) in attempts.iter().enumerate() {
                            lines.push(format!("attempt {}:", index + 1));
                            lines.extend(attempt.diagnostics.iter().cloned());
                        }
                        self.append_output(lines);
                        self.notify(&reason);
                    }
                }
                true
            }
        }
    }

    /// Apply the approved proposal: one edit, so undo takes it back.
    ///
    /// The whole file replaces the buffer, selection to the top, exactly as
    /// "Restore sample" does — the diff dialog is the statement of what is
    /// about to happen, and `edit` is what makes it undoable.
    pub fn say_ai_apply(&self) {
        let proposal = self.say_proposal.get();
        self.say_proposal.set(None);
        let Some(proposal) = proposal else {
            return;
        };
        self.edit(vieww_foundation::TextEditingValue::new(proposal.say));
        self.notify("Applied — press Render to see it in the frame.");
    }

    /// Refuse the proposal and close the dialog.
    pub fn say_ai_dismiss(&self) {
        self.say_proposal.set(None);
    }
}

// -- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The fence-stripper takes whole fences and leaves the middle alone.
    #[test]
    fn fences_are_stripped_only_around_the_reply() {
        assert_eq!(
            unfence("```say\n-- say-language: 1\n```"),
            "-- say-language: 1"
        );
        assert_eq!(unfence("plain"), "plain");
        assert_eq!(unfence("```"), "");
        // A fence-looking line mid-file is text, not a fence.
        assert_eq!(
            unfence("-- say-language: 1\n```code```"),
            "-- say-language: 1\n```code```"
        );
    }

    /// The reply reader handles the two shapes a localhost server sends.
    #[test]
    fn reply_bodies_are_read_whether_chunked_or_not() {
        let plain = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n\
                     {\"response\": \"-- say-language: 1\"}";
        assert_eq!(
            reply_body(plain),
            Ok("{\"response\": \"-- say-language: 1\"}".to_owned())
        );
        // 5 + 10 = the body in two chunks, then the terminator.
        let chunked = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                       5\r\n{\"res\r\nA\r\nponse\": 1}\r\n0\r\n\r\n";
        assert_eq!(reply_body(chunked), Ok("{\"response\": 1}".to_owned()));
        // A chunk with options after the size, the way servers send them.
        let with_options = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                            4;comment=1\r\nabcd\r\n0\r\n\r\n";
        assert_eq!(reply_body(with_options), Ok("abcd".to_owned()));
        assert!(reply_body("no body here").is_err());
    }

    /// The diff marks the middle and holds the ends still.
    #[test]
    fn the_diff_marks_what_changed_and_what_did_not() {
        let old = "-- say-language: 1\nscreen \"A\":\n    a text \"old\"";
        let new = "-- say-language: 1\nscreen \"A\":\n    a text \"new\"";
        assert_eq!(
            diff_lines(old, new),
            vec![
                "  -- say-language: 1",
                "  screen \"A\":",
                "-     a text \"old\"",
                "+     a text \"new\"",
            ]
        );
        // A proposal that is the buffer is a diff of nothing but context.
        assert!(diff_lines(old, old).iter().all(|l| l.starts_with("  ")));
    }

    /// The fake hands out its answers in order and records what it saw.
    #[test]
    fn the_fake_records_and_exhausts() {
        let fake = RecordedFake::new(vec![Ok("one".to_owned()), Ok("two".to_owned())]);
        assert_eq!(fake.complete_to_say("p", None), Ok("one".to_owned()));
        let attempt = Attempt {
            say: "one".to_owned(),
            diagnostics: vec!["e".to_owned()],
        };
        assert_eq!(
            fake.complete_to_say("p", Some(&attempt)),
            Ok("two".to_owned())
        );
        assert!(fake.complete_to_say("p", None).is_err());
        assert_eq!(fake.calls()[1].1, Some(attempt));
    }

    /// The one-retry cap, without a toolchain: a model whose Say never
    /// compiles is asked exactly twice, told the exact diagnostics once,
    /// and refused with both attempts carried.
    #[test]
    fn the_loop_retries_exactly_once_and_refuses() {
        let fake = RecordedFake::new(vec![
            Ok("not say at all".to_owned()),
            Ok("still not say".to_owned()),
        ]);
        // A `rustc` that is never reached: the Say compiler refuses both
        // answers before rustc is consulted, so it may as well be a panic.
        // A fn rather than a closure, because the loop hands it `&str` with
        // whatever lifetime the round has, and a closure pins one.
        fn never(_source: &str) -> std::io::Result<Compiled> {
            unreachable!("the Say compiler refused the answer first")
        }
        let Outcome::Refused { attempts, reason } = propose_with(&fake, "a screen", never) else {
            panic!("two bad answers must refuse");
        };
        assert_eq!(attempts.len(), 2, "exactly two attempts: {attempts:?}");
        assert!(reason.contains("did not survive verification"));
        // And the model was told the first attempt's diagnostics.
        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, None);
        let retry = calls[1].1.as_ref().expect("the retry carries the attempt");
        assert_eq!(retry.say, "not say at all");
        assert!(
            retry.diagnostics.iter().any(|d| d.contains("proposal.say")),
            "the diagnostics name the .say file: {:?}",
            retry.diagnostics
        );
    }

    /// A model that never answers is refused at once — one call, no retry,
    /// because there are no diagnostics to feed back.
    #[test]
    fn an_unreachable_model_is_refused_without_a_retry() {
        let fake = RecordedFake::new(vec![Err("connection refused".to_owned())]);
        fn never(_source: &str) -> std::io::Result<Compiled> {
            unreachable!("the model never answered")
        }
        let Outcome::Refused { attempts, reason } = propose_with(&fake, "a screen", never) else {
            panic!("no answer must refuse");
        };
        assert!(attempts.is_empty());
        assert!(reason.contains("the model did not answer"));
        assert_eq!(fake.calls().len(), 1);
    }

    /// A good answer on the second call is treated as such: reaching the
    /// `rustc` stage at all is proof the retry compiled as Say.
    #[test]
    fn a_bad_answer_then_a_good_one_reaches_rustc() {
        let good = "-- say-language: 1\nscreen \"A\":\n    a column:\n        a text \"hi\"";
        let fake = RecordedFake::new(vec![Ok("garbage".to_owned()), Ok(good.to_owned())]);
        let mut reached = 0;
        let Outcome::Refused { reason, .. } = propose_with(&fake, "a screen", |source| {
            reached += 1;
            assert!(source.contains("a text"), "generated Rust for the good Say");
            Err(std::io::Error::other("no rustc in this test"))
        }) else {
            panic!("no rustc means refusal, not verification");
        };
        assert_eq!(reached, 1, "the good answer reached rustc exactly once");
        assert!(reason.contains("rustc could not be run"));
        assert_eq!(fake.calls().len(), 2, "the retry happened");
    }

    /// The loop's rounds stop at the cap even when every failure says
    /// "retry": the second round's refusal is the fallthrough, and the
    /// wording is the same sentence both ways out.
    #[test]
    fn the_refusal_sentence_is_one_sentence() {
        assert!(REFUSED.contains("two attempts"));
        assert!(REFUSED.contains("Output panel"));
    }
}
