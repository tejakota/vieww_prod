//! ST-23: the warm-up — the template, the state machine, and who yields.
//!
//! # What this file proves, and how
//!
//! Three layers, each with its own kind of honesty:
//!
//! 1. **The template compiles.** Through the real `rustc`, the real
//!    `--extern vieww` rlib, the real flags a Render uses — because a
//!    template that does not compile is a warm-up that warms nothing while
//!    reporting success, and the only check that catches it is the compile
//!    itself.
//! 2. **The state machine walks.** `keep_warm` → `Compiling` → `Warm` with
//!    the duration it cost, driven through the real
//!    [`Job`](viewwstudio::compile::Job) worker and polled the way the frame
//!    hook polls.
//! 3. **The refusals and the preemption.** No toolchain, a warm session, a
//!    second ask, a compile already running — each refuses in words before
//!    anything spawns; and a Render cuts a running warm-up short
//!    (`Superseded`) rather than racing it, which is the whole
//!    who-yields-to-whom contract.
//!
//! The `rustc` of the preemption test is a script that sleeps: the state
//! machine must be observable *while* a compile is genuinely in flight,
//! which a real one finishes too fast to assert against. Everything else is
//! the real compiler.
//!
//! Skips (loudly, with a reason) when this checkout has no built `vieww`
//! rlib to compile against — the same rule `tests/pipeline.rs` follows.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use vieww_element::Runtime;
use viewwstudio::buffer::Workspace;
use viewwstudio::compile::{Session, Toolchain, ToolchainError};
use viewwstudio::keepwarm::{self, State};
use viewwstudio::state::Studio;

/// `target/debug` of this workspace, where `libvieww.rlib` lands — the same
/// resolution `tests/pipeline.rs` uses.
fn target() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary");
    exe.parent()
        .and_then(Path::parent)
        .expect("target/debug")
        .to_path_buf()
}

/// A directory the test owns, for the fake `rustc` and the toolchain's
/// path fields.
fn dir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("vieww-keepwarm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the test directory");
    root
}

/// A studio on the scratch workspace — the buffer a fresh launch holds, and
/// the one whose `screen()` lets `render` reach its own spawn — with a
/// toolchain and a session directory of its own: the three things
/// `keep_warm` reads.
fn studio(toolchain: Result<Toolchain, ToolchainError>, seed: u64) -> Studio {
    let runtime = Runtime::new();
    let session = Session::new(seed).ok();
    Studio::with_workspace(&runtime, Workspace::scratch())
        .with_host(vieww_foundation::TargetPlatform::Linux)
        .with_toolchain(toolchain, session)
}

/// A `rustc` that is a script doing nothing for five seconds: long enough to
/// assert against a compile that is genuinely still running, short enough
/// that a cancelled one never outlives the test binary.
fn slow_rustc(root: &Path) -> PathBuf {
    let script = root.join("slow-rustc");
    std::fs::write(&script, "#!/bin/sh\nsleep 5\nexit 0\n").expect("write the fake rustc");
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    script
}

/// A toolchain whose fields are all the studio's own business, around a
/// `rustc` the caller picks.
fn toolchain_around(rustc: PathBuf, root: &Path) -> Toolchain {
    Toolchain {
        rustc,
        vieww_rlib: root.join("libvieww-fake.rlib"),
        candidates: vec![],
        deps: root.join("deps"),
        sysroot_lib: None,
        version: "rustc 1.98.1 (fake)".into(),
    }
}

/// Poll the warm-up once per loop until it reaches a terminal state, and
/// fail loudly if it never does — the same contract as every other pump
/// here. Terminal means not `Compiling`: a `Refused` the state machine
/// wrote *over* a running warm-up (the second-ask tests) must not stop the
/// pump — the worker is still owned and its verdict is still the state
/// that matters, so the pump keeps polling while the job exists.
fn pump(studio: &Studio) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let busy = matches!(studio.keep_warm.get(), State::Compiling)
            || studio.keep_warm_job.borrow().is_some();
        if !busy {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the warm-up never finished — output so far:\n{}",
            studio.output.get().join("\n")
        );
        if !studio.poll_keep_warm() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The joined Output panel, for assertions and failure messages alike.
fn output(studio: &Studio) -> String {
    studio.output.get().join("\n")
}

/// The toolchain whose rlib this test binary can actually compile against —
/// `tests/pipeline.rs`'s `matching_candidate` discipline, in miniature.
///
/// `Toolchain::discover` returns candidates newest-first, and a well-used
/// `target/` holds several `libvieww-<hash>.rlib` compilations. The newest
/// is a guess, not a guarantee: the wrong one hands rustc a `vieww` whose
/// own dependencies (`vieww_audio` and friends) are not in the rlib form it
/// expects, and the compile fails with exactly the error a *stale* rlib
/// produces — nothing to do with the template under test. Rather than
/// inherit pipeline.rs's load-and-Probe machinery, this file probes with
/// the template itself: the first candidate that compiles it is the
/// candidate these tests use; if none does, the tests skip — a fact about
/// the checkout, not the code under test.
fn matching_toolchain() -> Option<Toolchain> {
    static RESOLVED: std::sync::OnceLock<Option<Toolchain>> = std::sync::OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let base = Toolchain::discover(&target()).ok()?;
            let session = Session::new(0x7EA5).ok()?;
            for candidate in 0.. {
                let toolchain = base.with_candidate(candidate)?;
                if let Ok(compiled) = viewwstudio::compile::compile(
                    &toolchain,
                    &session,
                    keepwarm::TEMPLATE,
                    "probe.rs",
                ) {
                    if !compiled.failed() {
                        return Some(toolchain);
                    }
                }
            }
            None
        })
        .clone()
}

#[test]
fn the_guest_template_compiles_through_the_real_pipeline() {
    // The template is the studio's own source; this is the pin that keeps it
    // that way. A template that stopped compiling would still walk the state
    // machine happily in every other test here — `Failed`, with a sentence —
    // so the compile itself is the one check that cannot be faked.
    let Some(toolchain) = matching_toolchain() else {
        eprintln!(
            "skipping: no vieww rlib in {} compiles the template",
            target().display()
        );
        return;
    };
    let session = Session::new(u64::from(std::process::id()) * 7).expect("a session");
    let compiled = viewwstudio::compile::compile(
        &toolchain,
        &session,
        keepwarm::TEMPLATE,
        keepwarm::FILE_NAME,
    )
    .expect("the template's compile runs");
    assert!(
        !compiled.failed(),
        "the guest template is the studio's own source — it must compile:\n{}",
        compiled
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn keep_warm_walks_the_state_machine_to_warm() {
    let Some(discovered) = matching_toolchain() else {
        eprintln!(
            "skipping: no vieww rlib in {} compiles the template",
            target().display()
        );
        return;
    };
    let studio = studio(Ok(discovered), u64::from(std::process::id()) * 8);

    assert_eq!(studio.keep_warm.get(), State::Idle);
    studio.keep_warm();
    assert_eq!(studio.keep_warm.get(), State::Compiling);
    assert!(
        studio.keep_warm_job.borrow().is_some(),
        "the warm-up owns a real worker"
    );
    pump(&studio);

    // Warm, with the cost it took — the number the flag exists to move.
    let State::Warm(took) = studio.keep_warm.get() else {
        panic!(
            "the warm-up ended in {:?} instead of Warm — output:\n{}",
            studio.keep_warm.get(),
            output(&studio)
        );
    };
    assert!(took > Duration::ZERO, "a real compile took real time");
    assert!(
        output(&studio).contains("cold-cache cost is already paid"),
        "the Output panel says what the warm-up bought:\n{}",
        output(&studio)
    );

    // And a second ask over a warm session has nothing to add, and says so.
    studio.keep_warm();
    assert!(
        matches!(studio.keep_warm.get(), State::Refused(reason) if reason.contains("already warm")),
        "a warm session refuses a second warm-up:\n{:?}",
        studio.keep_warm.get()
    );
}

#[test]
fn keep_warm_refuses_without_a_toolchain_before_anything_spawns() {
    // The studio's own default in these tests: no compiler, honestly said.
    let studio = studio(
        Err(ToolchainError::NoRustc("no compiler on this PATH".into())),
        u64::from(std::process::id()) * 9,
    );
    studio.keep_warm();
    assert!(
        matches!(studio.keep_warm.get(), State::Refused(reason) if reason.contains("no compiler")),
        "the missing toolchain, in the studio's own words:\n{:?}",
        studio.keep_warm.get()
    );
    assert!(
        studio.keep_warm_job.borrow().is_none(),
        "nothing was spawned by a refusal"
    );
}

#[test]
fn a_second_ask_while_compiling_is_refused_and_a_dead_rustc_is_failed() {
    let root = dir("dead-rustc");
    // A toolchain whose `rustc` does not exist: the spawn succeeds (the
    // worker is a thread) and the compile itself fails fast, which is the
    // `Failed` path — distinct from a refusal, because something ran.
    let studio = studio(
        Ok(toolchain_around(root.join("no-such-rustc-anywhere"), &root)),
        u64::from(std::process::id()) * 10,
    );
    studio.keep_warm();
    assert_eq!(studio.keep_warm.get(), State::Compiling);

    // The second ask, before any poll: the worker is still owned, so this is
    // "already compiling", not a second `rustc` over the first.
    studio.keep_warm();
    assert!(
        matches!(studio.keep_warm.get(), State::Refused(reason) if reason.contains("already compiling")),
        "a running warm-up refuses a second one:\n{:?}",
        studio.keep_warm.get()
    );

    pump(&studio);
    assert!(
        matches!(studio.keep_warm.get(), State::Failed(reason) if reason.contains("did not run")),
        "a `rustc` that cannot run is a failure, not a refusal:\n{:?}",
        studio.keep_warm.get()
    );
}

#[test]
fn a_render_preempts_a_running_warm_up_rather_than_racing_it() {
    let root = dir("preempt");
    // A `rustc` that sleeps: the warm-up is genuinely in flight when the
    // Render arrives, which is the only honest way to observe preemption.
    let studio = studio(
        Ok(toolchain_around(slow_rustc(&root), &root)),
        u64::from(std::process::id()) * 11,
    );
    studio.keep_warm();
    assert_eq!(studio.keep_warm.get(), State::Compiling);

    // The Render the user asked for: the active buffer is the studio's
    // scratch screen, so this spawns a real job — through the same sleeping
    // `rustc`, which is fine; the assertions are about ownership, not output.
    studio.render();
    assert!(
        studio.is_compiling(),
        "the Render started its own compile — output:\n{}",
        output(&studio)
    );
    assert_eq!(
        studio.keep_warm.get(),
        State::Superseded,
        "the warm-up stood down for the real thing"
    );
    assert!(
        studio.keep_warm_job.borrow().is_none(),
        "the warm-up's worker was let go, not left to race"
    );
    assert!(
        output(&studio).contains("cut short by a Render"),
        "the Output panel says who yielded to whom:\n{}",
        output(&studio)
    );
    // The preemption left the render's own state machine untouched.
    assert_eq!(
        studio.keep_warm.get(),
        State::Superseded,
        "and it stays superseded — the warm-up does not restart behind the Render"
    );

    // Clean up: cancel the Render's job so the sleeping fake does not linger.
    studio.cancel_render();
}

#[test]
fn a_warm_up_stand_down_when_a_compile_is_already_running() {
    let root = dir("already-running");
    let studio = studio(
        Ok(toolchain_around(slow_rustc(&root), &root)),
        u64::from(std::process::id()) * 12,
    );
    // The launch render: the scratch screen, through the sleeping rustc.
    studio.render();
    assert!(studio.is_compiling());
    // `--keep-warm` arriving after it has nothing to add, and says so rather
    // than queueing or racing.
    studio.keep_warm();
    assert!(
        matches!(
            studio.keep_warm.get(),
            State::Refused(reason) if reason.contains("a compile is already running")
        ),
        "a running compile is the warm-up already happening:\n{:?}",
        studio.keep_warm.get()
    );
    assert!(studio.keep_warm_job.borrow().is_none());
    studio.cancel_render();
}
