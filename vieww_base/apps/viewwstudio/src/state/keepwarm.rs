//! ST-23's state layer: starting the warm-up, and picking it up when it ends.
//!
//! The same shape as [`poll_compile`](Studio::poll_compile) for the same
//! reasons: the compile runs on a worker, the frame hook polls a channel
//! once per frame, and nothing in the UI thread ever waits on `rustc`. The
//! pure half — the template, the state vocabulary, and the argument for
//! warming at all — is [`crate::keepwarm`]; this file is only the loop that
//! connects a [`Job`](crate::compile::Job) to the studio.
//!
//! # Who yields to whom
//!
//! A Render preempts the warm-up, never the reverse: `render` cancels a
//! running template compile and records [`State::Superseded`], because the
//! compile the user asked for warms the same caches and two `rustc`s
//! competing for one cold disk is the opposite of the point. The warm-up
//! itself refuses to start while any compile is running for the same reason
//! — the guard chain in [`keep_warm`](Studio::keep_warm) says so in words
//! rather than silently queueing behind it.

use super::*;
use crate::keepwarm::{self, State, FILE_NAME};

impl Studio {
    /// Start the background precompile of the guest template (ST-23).
    ///
    /// `--keep-warm` on the command line; a public method rather than a
    /// private wire so the coldstart example and the tests can drive exactly
    /// what a launch drives. Refusal-first like everything in this studio:
    /// no toolchain, no session, an already-warm session, or a compile
    /// already in flight each set [`State::Refused`] with the sentence that
    /// says why, before anything is spawned.
    pub fn keep_warm(&self) {
        // Already running: a second `--keep-warm` (or a future command
        // reaching the same method) is told so rather than starting a second
        // `rustc` over the first.
        if self.keep_warm_job.borrow().is_some() {
            self.keep_warm.set(State::Refused(
                "keep-warm: the template is already compiling".into(),
            ));
            return;
        }
        if matches!(self.keep_warm.get(), State::Warm(_)) {
            self.keep_warm.set(State::Refused(
                "keep-warm: the caches are already warm — a second template compile has \
                 nothing to add"
                    .into(),
            ));
            return;
        }
        // A compile in flight is the warm-up already happening, by the thing
        // that matters. Standing down is the honest answer, not a queue.
        if self.job.borrow().is_some() {
            self.keep_warm.set(State::Refused(
                "keep-warm: a compile is already running, and it warms the same caches the \
                 template would"
                    .into(),
            ));
            return;
        }
        let Ok(toolchain) = self.toolchain.as_ref() else {
            let why = self.toolchain.as_ref().as_ref().err().map_or_else(
                || "no preview compiler".to_owned(),
                std::string::ToString::to_string,
            );
            self.keep_warm
                .set(State::Refused(format!("keep-warm: {why}")));
            return;
        };
        let Some(session) = self.session.as_ref() else {
            self.keep_warm.set(State::Refused(
                "keep-warm: no session directory to compile into — the studio could not \
                 create one in the system temp directory"
                    .into(),
            ));
            return;
        };
        let waker = self.waker.clone();
        let wake = move || {
            use vieww_foundation::task::FrameWaker as _;
            waker.wake();
        };
        // `say: false` and `project: None`: the template links `vieww` and
        // nothing else, which is the whole premise of what it warms.
        match Job::spawn(
            toolchain,
            session,
            keepwarm::TEMPLATE,
            keepwarm::FILE_NAME,
            None,
            false,
            wake,
        ) {
            Ok(job) => {
                *self.keep_warm_job.borrow_mut() = Some(job);
                self.keep_warm.set(State::Compiling);
                self.append_output(vec![format!(
                    "keep-warm: compiling the guest template in the background — {}",
                    toolchain.vieww_rlib.display()
                )]);
            }
            Err(error) => {
                self.keep_warm.set(State::Failed(format!(
                    "keep-warm: the template's compile could not start — {error}"
                )));
            }
        }
    }

    /// Pick up a finished warm-up, if one has finished (ST-23).
    ///
    /// Called once per frame from the platform hook, beside
    /// [`poll_compile`](Self::poll_compile); one non-blocking channel read
    /// on every frame nobody asked for warmth. Returns whether anything
    /// changed, like every poll in this studio.
    pub fn poll_keep_warm(&self) -> bool {
        let progress = {
            let borrowed = self.keep_warm_job.borrow();
            let Some(job) = borrowed.as_ref() else {
                return false;
            };
            job.poll()
        };
        match progress {
            Progress::Running => false,
            Progress::Lost => {
                self.keep_warm_job.borrow_mut().take();
                self.keep_warm.set(State::Failed(
                    "keep-warm: the compile worker stopped without answering".into(),
                ));
                self.append_output(vec![self.keep_warm.get().to_string()]);
                true
            }
            Progress::Done(result) => {
                let Some(job) = self.keep_warm_job.borrow_mut().take() else {
                    return false;
                };
                self.finish_keep_warm(result, job.started.elapsed());
                true
            }
        }
    }

    /// The verdict half: a finished compile becomes a state and a sentence.
    fn finish_keep_warm(
        &self,
        result: std::io::Result<compile::Compiled>,
        took: std::time::Duration,
    ) {
        let compiled = match result {
            Ok(compiled) => compiled,
            Err(error) => {
                self.keep_warm.set(State::Failed(format!(
                    "keep-warm: the template's compile did not run — {error}"
                )));
                self.append_output(vec![self.keep_warm.get().to_string()]);
                return;
            }
        };
        // The template is the studio's own source, so anything short of a
        // clean compile is a bug in [`keepwarm::TEMPLATE`] — said as one,
        // with the diagnostic, rather than reported as warmth.
        if compiled.failed() {
            let first = compiled
                .diagnostics
                .first()
                .map_or_else(|| "no diagnostics".to_owned(), |d| d.message.clone());
            self.keep_warm.set(State::Failed(format!(
                "keep-warm: the guest template did not compile — {first} ({FILE_NAME})"
            )));
            self.append_output(vec![self.keep_warm.get().to_string()]);
            return;
        }
        self.keep_warm.set(State::Warm(took));
        self.append_output(vec![format!(
            "keep-warm: the guest template compiled in {:.1} s — the first Render's \
             cold-cache cost is already paid",
            took.as_secs_f64()
        )]);
    }

    /// Stand the warm-up down for a Render that is starting.
    ///
    /// The call site is `render` itself, after its own one-at-a-time guard:
    /// a Render reaching this point is about to compile, and the compile the
    /// user asked for warms everything the template would have.
    pub(super) fn supersede_keep_warm(&self) {
        let Some(job) = self.keep_warm_job.borrow_mut().take() else {
            return;
        };
        job.cancel();
        let ran = job.elapsed();
        self.keep_warm.set(State::Superseded);
        self.append_output(vec![format!(
            "keep-warm: cut short by a Render after {:.1} s — the caches it warmed are \
             what that Render starts from",
            ran.as_secs_f64()
        )]);
    }
}
