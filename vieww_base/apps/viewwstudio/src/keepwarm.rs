//! ST-23: the first compile's cold-cache cost, paid before anybody asks for it.
//!
//! # The two numbers this exists for
//!
//! The research measured two waits a first-time user hits: the studio's own
//! first frame (~4× slower on Windows than Linux — a platform-startup cost
//! the **framework** owns, tracked as FW-16 and deliberately not touched
//! here), and the **first compile of a session** — 10.6 s cold-cache against
//! ~0.8 s after, because a preview's `rustc` has to read the whole `vieww`
//! rlib, its ~100 dependency rlibs, and drive the linker, and none of that
//! is in the OS page cache until something has compiled once.
//!
//! This module is the studio-side half of the second number:
//! [`State::Warm`] means that cost has already been paid. `--keep-warm` on
//! the command line compiles [`TEMPLATE`] — the smallest screen that still
//! links `vieww` — on the same worker, through the same
//! [`Job`](crate::compile::Job) machinery, with the same `rustc` and the
//! same `--extern` rlib a Render uses, while the user is still looking at
//! the splash. When the first *real* Render comes, the cache is the warm
//! one: the 10.6 s wait becomes the 0.8 s one, which is the whole deliverable.
//!
//! # Why a template, and why the launch render is not enough
//!
//! The studio already renders once at launch — but it renders *the active
//! buffer*, and everything that makes that render refuse (a `flow.rs` file,
//! a restored buffer with no `screen()`, a Say buffer with a syntax error)
//! is a launch that warmed nothing. The template cannot refuse: it is the
//! studio's own source, pinned by a test that compiles it for real, and it
//! is minimal on purpose — every line of guest code is time the measurement
//! does not want attributed to `vieww`.
//!
//! # What this deliberately does not claim
//!
//! * **No persistent build cache.** Each compile still runs its own `rustc`;
//!   what the warm-up shares is the *machine's* page cache, nothing on disk.
//!   A cross-process artefact cache is a different, larger project (sccache)
//!   and is not smuggled in here under a keep-warm name.
//! * **Nothing about the first frame.** The Windows first-frame gap is
//!   FW-16's — pipeline caches and device bring-up live in the framework,
//!   and the studio side of that linkage is exactly one sentence: warm the
//!   compile cache (this module) while the platform brings the GPU up, and
//!   neither wait serializes behind the other.
//! * **No mount.** The template's `.so` is compiled and then ignored — the
//!   preview pane stays empty until a real Render fills it, because showing
//!   a screen the user never wrote would be a lie with pixels.

use std::fmt;
use std::time::Duration;

/// The guest the warm-up compiles: the smallest screen that still links
/// `vieww`.
///
/// The scratch buffer's own shape, trimmed — a derived `Widget` struct with
/// `debug_name`/`kind`/`build`, wrapped by `screen()` — because that is the
/// contract [`ENTRY`](crate::compile) appends a call to, and because staying
/// close to what a real first Render compiles keeps the warm-up honest about
/// what it warmed. Pinned compilable by `tests/keepwarm.rs`, which runs the
/// real `rustc` over it exactly as a Render would.
pub const TEMPLATE: &str = "\
use vieww::prelude::*;

// `Widget: Any + Debug`, so this derive is not optional — the warm-up's one
// job is to link `vieww`, and a template that cannot is a bug this comment
// exists to make obvious.
#[derive(Debug)]
pub struct Screen;

impl Widget for Screen {
    fn debug_name(&self) -> &'static str {
        \"Screen\"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let theme = ThemeData::of(ctx);

        Container::new()
            .color(theme.colors.surface)
            .alignment(Alignment::CENTER)
            .child(Text::new(\"warm\").style(theme.text.headline))
            .into()
    }
}

pub fn screen() -> impl Widget {
    Screen
}
";

/// The name diagnostics from the warm-up are attributed to.
///
/// Not a real file anywhere — the template lives in this module — but the
/// one honest label for a problem that can only be a studio bug: the
/// template is ours, and if it does not compile, the sentence should say
/// whose it is.
pub const FILE_NAME: &str = "keep-warm.rs";

/// Where the warm-up stands. A state machine rather than an `Option<Job>`
/// because the interesting answers are not "running or not": they are *why
/// it never started* (no toolchain, no session, a compile already running)
/// and *how it ended* (warm, cut short by a Render, failed) — each a
/// different sentence in the Output panel and a different assertion in the
/// tests.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Nothing has been asked for. The studio's default: the flag is opt-in.
    Idle,
    /// The template is compiling on the worker.
    Compiling,
    /// The template compiled, and what it cost — the cost the first Render
    /// no longer pays.
    Warm(Duration),
    /// A Render started while the template was still compiling, and the
    /// warm-up stood down: the compile the user asked for warms the same
    /// caches, and two `rustc`s competing for one disk is the opposite of
    /// the point.
    Superseded,
    /// The warm-up never started, and the sentence that says why — decided
    /// before anything was spawned, like every refusal in this studio.
    Refused(String),
    /// The warm-up started and ended badly: `rustc` would not run, or the
    /// template did not compile (a studio bug — see [`TEMPLATE`]).
    Failed(String),
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "the first compile has not been warmed up"),
            Self::Compiling => write!(f, "warming the first compile's caches"),
            Self::Warm(took) => write!(
                f,
                "the first compile's caches are warm ({:.1} s)",
                took.as_secs_f64()
            ),
            Self::Superseded => write!(
                f,
                "the warm-up was cut short by a Render — the compile you asked for warms the \
                 same caches"
            ),
            Self::Refused(why) | Self::Failed(why) => write!(f, "{why}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template is the contract [`crate::compile::ENTRY`] needs: a
    /// `pub fn screen()` the appended entry point can call. A template that
    /// lost it would refuse in `render`'s own entry-point check — a warm-up
    /// that warms nothing, found only by reading the Output panel.
    #[test]
    fn the_template_carries_the_entry_point_contract() {
        assert!(crate::compile::entry_point(TEMPLATE));
        assert!(TEMPLATE.contains("use vieww::prelude"));
        // And nothing a Say screen or a project library would need: the
        // warm-up's compile must be linkable against `vieww` alone, or it
        // measures a project the user does not have.
        assert!(!TEMPLATE.contains("crate::"));
        assert!(!TEMPLATE.contains("use super::"));
    }

    /// Every state renders as a sentence, because the one that lands in the
    /// Output panel is read by a person deciding whether to wait.
    #[test]
    fn every_state_is_a_sentence() {
        for state in [
            State::Idle,
            State::Compiling,
            State::Warm(Duration::from_millis(1234)),
            State::Superseded,
            State::Refused("no toolchain".into()),
            State::Failed("rustc went away".into()),
        ] {
            let said = state.to_string();
            assert!(
                !said.is_empty() && said.chars().next().is_some_and(char::is_lowercase) || {
                    // Numbers may lead a fraction; the sentence around them still
                    // reads as one.
                    said.contains(' ')
                },
                "{state:?} said nothing readable: {said}"
            );
        }
        // The two states a person most needs to tell apart say the number
        // and the cut both.
        assert!(State::Warm(Duration::from_millis(1234))
            .to_string()
            .contains("1.2 s"));
        assert!(State::Superseded.to_string().contains("cut short"));
    }
}
