//! ST-03 end to end: a prompt, through a model, *proved*, then applied.
//!
//! # What this file proves
//!
//! The unit tests in `say_ai.rs` prove the loop's arithmetic — one retry,
//! refusal after two, the model told the exact diagnostics. This file proves
//! the whole of it against the real thing:
//!
//! * a [`RecordedFake`] standing in for the localhost model, handed to the
//!   studio exactly the way `LocalHttpModel` is — through [`AnyModel`], so
//!   the code under test cannot tell which it was given;
//! * the real Say compiler, the real `rustc`, the real `dlopen`, the real
//!   rasteriser — the same pipeline a hand-written buffer goes through;
//! * the studio itself: the command that opens the box, the submit that
//!   starts the worker, the per-frame poll that picks the answer up, the
//!   apply that lands it in the buffer as one undoable edit.
//!
//! The three golden paths are the three outcomes the loop has:
//!
//! 1. [`an_approved_proposal_lands_in_the_buffer`] — verified, shown as a
//!    diff, applied, undone.
//! 2. [`a_bad_answer_then_a_good_one_retries_exactly_once`] — the diagnostic
//!    fed back, the second answer accepted.
//! 3. [`two_bad_answers_refuse_without_touching_the_buffer`] — refusal with
//!    both attempts' diagnostics in the Output panel, buffer untouched.
//!
//! # Why the fake is the only honest model here
//!
//! A golden test against a real model is a test that flakes by design: the
//! model's answer changes, so what the assertions can check collapses to
//! "something happened". The recorded fake pins the *conversation* — what the
//! model was asked, what it answered, what it was told when the answer did
//! not verify — which is the part this module owns. The localhost HTTP
//! client's own wire behaviour is covered by the unit tests' hand-rolled
//! replies.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use vieww_element::Runtime;
use viewwstudio::buffer::{Buffer, Workspace};
use viewwstudio::command::Command;
use viewwstudio::compile::{compile, Session, Toolchain};
use viewwstudio::loaded::{LoadError, Preview};
use viewwstudio::say_ai::{AnyModel, Outcome, RecordedFake};
use viewwstudio::state::Studio;

/// The buffer the studio starts with — the shipped counter.
const COUNTER_SAY: &str =
    include_str!("../../../crates/vieww-say-codegen/tests/fixtures/counter.say");

/// The fake's "good answer" — the shipped profile screen: a different file,
/// so the approval diff has real `-` and `+` lines in it.
const PROFILE_SAY: &str =
    include_str!("../../../crates/vieww-say-codegen/tests/fixtures/profile.say");

/// `target/debug` of this workspace — see `tests/pipeline.rs`, which this
/// borrows the toolchain-resolution machinery from.
fn target() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary");
    exe.parent()
        .and_then(Path::parent)
        .expect("target/debug")
        .to_path_buf()
}

/// The smallest screen that compiles, for probing which `vieww` candidate
/// this test binary actually shares types with — the same probe and the same
/// reason as `tests/pipeline.rs`: `discover`'s newest-first ordering is a
/// guess, and a proposal that fails verification for an ABI reason the studio
/// had already solved would be a false failure.
const PROBE: &str = r#"
use vieww::prelude::*;
#[derive(Debug)]
pub struct Probe;
impl Widget for Probe {
    fn debug_name(&self) -> &'static str { "Probe" }
    fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode { Container::new().into() }
}
pub fn screen() -> impl Widget { Probe }
"#;

fn matching_candidate(base: &Toolchain) -> Option<usize> {
    static RESOLVED: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *RESOLVED.get_or_init(|| {
        let session = Session::new(0x5A17).ok()?;
        for candidate in 0.. {
            let toolchain = base.with_candidate(candidate)?;
            let Ok(compiled) = compile(&toolchain, &session, PROBE, "probe.rs") else {
                continue;
            };
            let Some(library) = compiled.library.as_ref() else {
                continue;
            };
            match Preview::load(library) {
                Ok(_) => return Some(candidate),
                Err(LoadError::AbiMismatch { .. }) => continue,
                Err(_) => continue,
            }
        }
        None
    })
}

/// The toolchain a proposal should be compiled with, and which of its
/// candidates this test binary shares types with — the pair the studio
/// itself keeps in `Studio::candidate`. Or `None` to skip.
fn toolchain() -> Option<(Toolchain, usize)> {
    let base = match Toolchain::discover(&target()) {
        Ok(toolchain) => toolchain,
        Err(error) => {
            eprintln!("skipping: {error}");
            return None;
        }
    };
    let Some(candidate) = matching_candidate(&base) else {
        eprintln!(
            "skipping: no `vieww` rlib in {} matches this test binary's own compilation",
            target().display()
        );
        return None;
    };
    base.with_candidate(candidate)
        .map(|toolchain| (toolchain, candidate))
}

/// A studio whose one open buffer is `counter.say` on disk, with the
/// toolchain and session the prompt loop needs — the state a person's studio
/// is in when they open the prompt box: it has rendered at least once, so
/// the candidate the fingerprint search settled on is the one it uses.
fn prompt_studio(tag: &str, toolchain: &Toolchain, candidate: usize) -> Studio {
    let dir = std::env::temp_dir().join(format!("say-ai-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temp directory");
    let path = dir.join("counter.say");
    std::fs::write(&path, COUNTER_SAY).expect("the fixture on disk");
    let buffer = Buffer::open(&path).expect("the fixture opens");
    let runtime = Runtime::new();
    let workspace = Workspace::of_buffers(vec![buffer]);
    let mut studio = Studio::with_workspace(&runtime, workspace)
        .with_host(vieww_foundation::TargetPlatform::Linux);
    studio.toolchain = std::rc::Rc::new(Ok(toolchain.clone()));
    studio.candidate.set(candidate);
    studio.session = std::rc::Rc::new(Some(Session::new(0xA103).expect("a temp directory")));
    studio
}

/// Give the studio a model with recorded answers, and hand back the shared
/// fake so the test can read the conversation it recorded.
fn model_of(studio: &Studio, fake: RecordedFake) -> Arc<RecordedFake> {
    let shared = Arc::new(fake);
    studio.say_model.set(AnyModel::new(shared.clone()));
    shared
}

/// The prompt typed, submitted, and the answer polled for — the frame hook's
/// share of the loop, run eagerly.
fn submit_and_wait(studio: &Studio, prompt: &str) -> bool {
    studio.run(Command::SayPrompt);
    assert!(
        studio.say_prompt_open.get(),
        "the command opened the prompt box"
    );
    studio.set_say_prompt_text(prompt.to_owned());
    studio.say_ai_submit();
    assert!(
        !studio.say_prompt_open.get(),
        "submitting closed the box — the verification owns the screen now"
    );
    assert!(studio.say_job.borrow().is_some(), "the worker is running");
    // The frame hook polls once a frame; here, poll until the loop answers.
    // Two `rustc` runs are the slowest this gets.
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if studio.poll_say_ai() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("the verified loop did not answer within 120s");
}

#[test]
fn an_approved_proposal_lands_in_the_buffer() {
    let Some((toolchain, candidate)) = toolchain() else {
        return;
    };
    let studio = prompt_studio("apply", &toolchain, candidate);
    let fake = model_of(&studio, RecordedFake::new(vec![Ok(PROFILE_SAY.to_owned())]));

    submit_and_wait(&studio, "a profile screen with posts and likes");

    // The model saw the prompt and nothing else — no retry, because the
    // first answer survived.
    let calls = fake.calls();
    assert_eq!(calls.len(), 1, "one call: {calls:?}");
    assert_eq!(calls[0].0, "a profile screen with posts and likes");
    assert!(calls[0].1.is_none(), "the first call carries no retry");

    // The proposal is on the table, with the diff that approval will apply.
    let proposal = studio
        .say_proposal
        .get()
        .expect("a verified proposal awaits approval");
    assert_eq!(proposal.say, PROFILE_SAY);
    let diff = viewwstudio::say_ai::diff_lines(COUNTER_SAY, &proposal.say);
    assert!(
        diff.iter().any(|l| l.starts_with("- ")),
        "the diff shows what is lost: {}",
        diff.join("\n")
    );
    assert!(
        diff.iter().any(|l| l.starts_with("+ ")),
        "the diff shows what is gained"
    );

    // Approval: one edit, the buffer replaced, undo takes it back.
    assert!(
        !studio.active().expect("a buffer").dirty,
        "the buffer was untouched while the proposal waited"
    );
    studio.say_ai_apply();
    let applied = studio.active().expect("a buffer");
    assert_eq!(applied.value.text, PROFILE_SAY, "the proposal landed");
    assert!(applied.dirty, "applying is an edit, not a write");
    assert!(
        studio.say_proposal.get().is_none(),
        "the dialog is closed once the answer is given"
    );
    studio.undo();
    assert_eq!(
        studio.active().expect("a buffer").value.text,
        COUNTER_SAY,
        "undo takes the proposal back out"
    );
}

#[test]
fn a_bad_answer_then_a_good_one_retries_exactly_once() {
    let Some((toolchain, candidate)) = toolchain() else {
        return;
    };
    let studio = prompt_studio("retry", &toolchain, candidate);
    let fake = model_of(
        &studio,
        RecordedFake::new(vec![
            Ok("this is not a .say file at all".to_owned()),
            Ok(PROFILE_SAY.to_owned()),
        ]),
    );

    submit_and_wait(&studio, "a profile screen");

    // The retry happened, and it was fed the exact diagnostics of the first
    // attempt — Say's own, naming the proposal's file, line and column.
    let calls = fake.calls();
    assert_eq!(calls.len(), 2, "exactly one retry: {calls:?}");
    let retry = calls[1].1.as_ref().expect("the retry carries the attempt");
    assert_eq!(retry.say, "this is not a .say file at all");
    assert!(
        retry.diagnostics.iter().any(|d| d.contains("proposal.say")),
        "the diagnostics name the .say file: {:?}",
        retry.diagnostics
    );

    // And the second answer is what the studio is holding.
    let proposal = studio.say_proposal.get().expect("the good answer verified");
    assert_eq!(proposal.say, PROFILE_SAY);
    assert!(
        !studio.active().expect("a buffer").dirty,
        "nothing landed without approval"
    );
}

#[test]
fn two_bad_answers_refuse_without_touching_the_buffer() {
    let Some((toolchain, candidate)) = toolchain() else {
        return;
    };
    let studio = prompt_studio("refuse", &toolchain, candidate);
    let fake = model_of(
        &studio,
        RecordedFake::new(vec![
            Ok("garbage one".to_owned()),
            Ok("garbage two".to_owned()),
        ]),
    );

    submit_and_wait(&studio, "a screen");

    // Two attempts, no third — the cap is the product decision this loop
    // exists to make, and this is its golden test.
    assert_eq!(fake.calls().len(), 2, "the model was asked exactly twice");

    // No proposal is on the table.
    assert!(
        studio.say_proposal.get().is_none(),
        "a refused loop offers nothing to approve"
    );
    // The buffer is untouched — refusal is not an edit.
    assert_eq!(studio.active().expect("a buffer").value.text, COUNTER_SAY);
    assert!(!studio.active().expect("a buffer").dirty);

    // The refusal is a value: a status-bar sentence and both attempts'
    // diagnostics in the Output panel, where they can be read and copied.
    let notice = studio.notice.get().expect("a refusal sentence");
    assert!(
        notice.contains("did not survive verification"),
        "the notice says what happened: {notice}"
    );
    let output = studio.output.get();
    let joined = output.join("\n");
    assert!(
        joined.contains("attempt 1:"),
        "the Output panel names the first attempt: {joined}"
    );
    assert!(joined.contains("attempt 2:"), "and the second: {joined}");
    assert!(
        joined.contains("proposal.say"),
        "with the Say diagnostics attached: {joined}"
    );
    // And the worker is gone, so the next prompt can start.
    assert!(
        studio.say_job.borrow().is_none(),
        "the worker slot is free again"
    );
}

/// The box's own refusals, which need no toolchain: a non-Say buffer is told
/// so, in the status bar, and no worker is started.
#[test]
fn the_prompt_refuses_a_buffer_that_is_not_say() {
    let runtime = Runtime::new();
    // A scratch buffer is Rust by construction — see `Buffer::scratch` —
    // which is exactly the wrong language for the prompt.
    let workspace = Workspace::of_buffers(vec![Buffer::scratch("screen.rs", "fn main() {}")]);
    let studio = Studio::with_workspace(&runtime, workspace)
        .with_host(vieww_foundation::TargetPlatform::Linux);
    studio.run(Command::SayPrompt);
    studio.set_say_prompt_text("a screen".to_owned());
    studio.say_ai_submit();
    assert!(
        studio.say_job.borrow().is_none(),
        "no worker was started for a Rust buffer"
    );
    let notice = studio.notice.get().expect("a refusal sentence");
    assert!(
        notice.contains(".say buffer"),
        "the notice says which buffer the prompt needs: {notice}"
    );
}

/// The loop's outcomes stay comparable, so the golden tests above can assert
/// on them directly rather than through their effects only.
#[test]
fn outcomes_are_values() {
    let refused = Outcome::Refused {
        attempts: vec![viewwstudio::say_ai::Attempt {
            say: "one".to_owned(),
            diagnostics: vec!["proposal.say:1:1 say-0001: expected a screen".to_owned()],
        }],
        reason: "because".to_owned(),
    };
    assert_eq!(refused.clone(), refused);
    assert_ne!(
        refused,
        Outcome::Verified(viewwstudio::say_ai::Proposal {
            prompt: "p".to_owned(),
            say: "s".to_owned(),
        })
    );
}
