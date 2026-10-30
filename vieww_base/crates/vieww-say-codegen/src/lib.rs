//! Say — the English-facing screen language for vieww.
//!
//! A `.say` file is the *only* source. This crate turns it into one byte-for-
//! byte deterministic Rust file, and that file is what the studio compiles
//! exactly as it compiles an edited Rust buffer today: same `rustc`
//! invocation, same `pub fn screen()` contract, same `dlopen`. Say changes
//! *what the buffer contains*, not how a buffer becomes a preview.
//!
//! # The shape of the pipeline
//!
//! ```text
//! counter.say ──say-codegen──▶ counter.rs ──studio rustc──▶ cdylib ──dlopen──▶ preview
//! ```
//!
//! Everything the generated file needs is emitted into it — including a small
//! `mod say` of state helpers — so the compile pipeline stays untouched: the
//! guest is linked with `--extern vieww` alone, exactly as any hand-written
//! screen is.
//!
//! # The state model (and why)
//!
//! Generated state lives in one `ElementState` on the generated root widget,
//! created at mount by `create_state`, written by handlers through
//! `ctx.state_handle()`, and polled once a frame by
//! `ElementTree::poll_states` via `take_pending`. `snapshot`/`restore` are
//! generated from the `keep` list, which is what lets a screen's state
//! survive the studio's recompile-and-remount around every Render — the host
//! half of that (`snapshot_states` / `restore_states` around the guest root)
//! already works. A previewed guest cannot reach a `Runtime` (there is no
//! `Inherited<Runtime>` across the preview boundary), so Say does not try:
//! this is the same pattern the studio's own `scaffold.rs` template ships.
//!
//! # What v1 of this generator covers
//!
//! State (`keep`), screens and navigation as a route stack, the core layout
//! and text widgets, buttons, bound controls (`a text field bound to s`),
//! lists (`a list of`, `for each`), `only if` rows, interpolation and `how
//! many`, conditional actions, snackbars and modal dialogs, images, scroll
//! views, tabs, a `the theme:` block, fade and slide screen transitions, and
//! `rust:` blocks that pass Rust through verbatim (ST-20) — every phrase
//! with an exact Rust counterpart, every generated statement preceded by a
//! `// say: file:line` marker. Phrases the vocabulary reserves but this
//! generator does not emit yet answer `E050` with what to do today.

use std::fmt;

mod diag;
mod gen;
mod lex;
mod parse;
mod parse_line;

pub use diag::{Diagnostic, Severity};

/// The result of compiling one `.say` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// The complete Rust source, ready for the studio's compile pipeline.
    pub rust: String,
    /// Warnings the file still carries: it compiles, and the author would
    /// still want to know (a transition the preview cannot play, a block
    /// that lost its indentation). The studio merges these into Problems.
    pub warnings: Vec<Diagnostic>,
}

/// Compile `.say` source into Rust.
///
/// `file_name` is the name the diagnostics and the generated `// say:`
/// markers carry — usually the buffer name, `home.say`.
///
/// # Errors
///
/// Every problem found, not the first one: the parser collects diagnostics
/// and keeps going where it can, so a beginner sees everything wrong with
/// their file in one pass rather than one error per Render.
pub fn compile(file_name: &str, source: &str) -> Result<Generated, Vec<Diagnostic>> {
    compile_with(file_name, source, &Options::default())
}

/// What a compile is told beyond the source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// The folder `an image "…"` paths are read from at run time — the
    /// studio passes the folder the `.say` file is in. Empty (the default)
    /// reads them from the working folder. Written into the generated file,
    /// so the same source and the same options give the same bytes.
    pub asset_root: String,
}

/// [`compile`], with [`Options`].
///
/// # Errors
///
/// As [`compile`].
pub fn compile_with(
    file_name: &str,
    source: &str,
    options: &Options,
) -> Result<Generated, Vec<Diagnostic>> {
    let file = std::rc::Rc::new(file_name.to_owned());
    let lines = lex::lex(source);
    let program = parse::parse(&file, &lines);
    match program {
        Ok(program) => Ok(Generated {
            rust: gen::generate(
                &file,
                &program,
                &gen::GenOptions {
                    asset_root: options.asset_root.clone(),
                },
            ),
            warnings: program.warnings,
        }),
        Err(diagnostics) => Err(diagnostics),
    }
}

/// The word-level registry the editor tools read: the canonical phrases, the
/// properties, the icons and the theme colour slots. One vocabulary, many
/// readers — the generator here, and the studio's completion and docs.
pub mod registry {
    /// The widget phrases, canonical form first.
    pub const WIDGET_PHRASES: &[&str] = &[
        "a card",
        "a row",
        "a column",
        "a stack",
        "a center",
        "a fixed area of W by H",
        "a text",
        "a heading",
        "a label",
        "a button",
        "a text field",
        "a switch",
        "a checkbox",
        "a slider",
        "a list of",
        "an empty state",
        "an icon",
        "a floating action button",
        "an image \"path\"",
        "a scroll view",
        "a set of tabs",
        "a tab \"Name\"",
    ];

    /// The blocks that are not widgets: the theme, and Rust passed through.
    pub const BLOCKS: &[&str] = &[
        "the theme:",
        "rust:",
        "```rust … ```",
        "with a fade transition",
        "with a slide transition",
    ];

    /// The colour slots a `the theme:` line can set.
    pub const THEME_SLOTS: &[&str] = &[
        "primary",
        "on primary",
        "surface",
        "on surface",
        "surface variant",
        "on surface variant",
        "outline",
        "error",
        "on error",
    ];

    /// The properties one widget line can carry, after a comma.
    pub const PROPERTIES: &[&str] = &[
        "padded N",
        "spaced N",
        "children aligned to the start",
        "size N",
        "bold",
        "with the primary color",
        "color #RRGGBB",
        "radius N",
        "N wide",
        "N tall",
        "with placeholder \"…\"",
        "single line",
        "in N lines",
        "each row N",
        "described as \"…\"",
        "labelled \"…\"",
        "from A to B",
        "pinned top N",
        "only if …",
    ];

    /// The action lines an event block accepts.
    pub const ACTIONS: &[&str] = &[
        "set n to E",
        "add E to n",
        "subtract E from n",
        "toggle b",
        "append E to L",
        "remove the item at E from L",
        "clear L",
        "open the screen \"X\"",
        "go back",
        "show a snackbar \"M\"",
        "close this dialog",
    ];

    /// The built-in icons, exactly the ten `vieww_widget::icons` constructors.
    pub const ICONS: &[&str] = &[
        "the check icon",
        "the close icon",
        "the plus icon",
        "the minus icon",
        "the left chevron",
        "the right chevron",
        "the up chevron",
        "the down chevron",
        "the back chevron",
        "the forward chevron",
    ];
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {} {}",
            self.file, self.line, self.column, self.code, self.message
        )
    }
}

#[cfg(test)]
mod tests {
    /// The fixture is the contract: `counter_gen.rs` is `saygen`'s output for
    /// `counter.say`, committed so a generator change that alters the output
    /// shows up as a reviewable diff. If this test fails, either the
    /// generator changed (re-run saygen and commit the new fixture
    /// deliberately) or the fixture drifted (fix the fixture).
    ///
    /// **Line endings are normalised before comparing.** The generator emits
    /// `\n`, and a Windows checkout can hand this `\r\n` — `.gitattributes`
    /// asks for LF, but a clone made before it existed, or a `core.autocrlf`
    /// left on, still delivers CRLF, and this test then failed on the Windows
    /// runner with two identical-looking 8KB strings in the message. What is
    /// under test is the generated *code*, not which bytes git chose to write
    /// the fixture with.
    #[test]
    fn the_counter_fixture_is_current() {
        let source = include_str!("../tests/fixtures/counter.say");
        let expected = include_str!("../tests/fixtures/counter_gen.rs");
        let generated = crate::compile("counter.say", source).expect("the fixture compiles");
        assert_eq!(
            generated.rust.replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "the committed fixture is stale"
        );
    }

    /// Byte-identical output on every run is the codegen contract (spec
    /// §15.6): a rebuild cache keyed on content hash only works if the
    /// same input never produces two outputs.
    #[test]
    fn generation_is_deterministic() {
        let source = include_str!("../tests/fixtures/counter.say");
        let first = crate::compile("counter.say", source).unwrap().rust;
        let second = crate::compile("counter.say", source).unwrap().rust;
        assert_eq!(first, second);
    }

    /// ST-20: one `.say` + `_gen.rs` pair per new phrase, each held to the
    /// same contract as the counter. `tests/vocabulary.rs` compiles every
    /// one and drives it in a frame driver.
    #[test]
    fn every_vocabulary_fixture_is_current() {
        let fixtures: [(&str, &str, &str); 7] = [
            (
                "profile.say",
                include_str!("../tests/fixtures/profile.say"),
                include_str!("../tests/fixtures/profile_gen.rs"),
            ),
            (
                "image.say",
                include_str!("../tests/fixtures/image.say"),
                include_str!("../tests/fixtures/image_gen.rs"),
            ),
            (
                "scroll.say",
                include_str!("../tests/fixtures/scroll.say"),
                include_str!("../tests/fixtures/scroll_gen.rs"),
            ),
            (
                "tabs.say",
                include_str!("../tests/fixtures/tabs.say"),
                include_str!("../tests/fixtures/tabs_gen.rs"),
            ),
            (
                "theme.say",
                include_str!("../tests/fixtures/theme.say"),
                include_str!("../tests/fixtures/theme_gen.rs"),
            ),
            (
                "transition.say",
                include_str!("../tests/fixtures/transition.say"),
                include_str!("../tests/fixtures/transition_gen.rs"),
            ),
            (
                "rust.say",
                include_str!("../tests/fixtures/rust.say"),
                include_str!("../tests/fixtures/rust_gen.rs"),
            ),
        ];
        for (name, source, expected) in fixtures {
            let generated =
                crate::compile(name, source).unwrap_or_else(|d| panic!("{name} compiles: {d:?}"));
            assert!(
                generated.warnings.is_empty(),
                "{name}: {:?}",
                generated.warnings
            );
            assert_eq!(
                generated.rust.replace("\r\n", "\n"),
                expected.replace("\r\n", "\n"),
                "the committed {name} fixture is stale — re-run saygen"
            );
            assert_eq!(generated.rust, crate::compile(name, source).unwrap().rust);
        }
    }

    fn errors(source: &str) -> Vec<String> {
        match crate::compile("t.say", source) {
            Ok(_) => Vec::new(),
            Err(d) => d
                .into_iter()
                .map(|d| format!("{} {}", d.code, d.message))
                .collect(),
        }
    }

    #[test]
    fn a_tab_outside_a_set_of_tabs_is_refused() {
        let e = errors("-- say-language: 1\nscreen \"A\":\n    a column:\n        a tab \"One\":\n            a text \"x\"\n");
        assert!(
            e.iter()
                .any(|m| m.contains("directly under `a set of tabs:`")),
            "{e:?}"
        );
        let e =
            errors("-- say-language: 1\nscreen \"A\":\n    a set of tabs:\n        a text \"x\"\n");
        assert!(e.iter().any(|m| m.contains("holds `a tab")), "{e:?}");
        let e = errors("-- say-language: 1\nscreen \"A\":\n    a set of tabs:\n        a tab \"One\":\n            a tab \"Two\":\n                a text \"x\"\n");
        assert!(
            e.iter().any(|m| m.contains("directly under")),
            "a tab inside a tab: {e:?}"
        );
    }

    #[test]
    fn rust_blocks_must_close_and_hold_code() {
        let e = errors("-- say-language: 1\nscreen \"A\":\n    ```rust\n    Text::new(\"x\")\n");
        assert!(e.iter().any(|m| m.contains("never closed")), "{e:?}");
        let e = errors("-- say-language: 1\nrust:\nscreen \"A\":\n    a text \"x\"\n");
        assert!(e.iter().any(|m| m.contains("empty")), "{e:?}");
    }

    #[test]
    fn theme_lines_name_a_slot_and_a_colour() {
        let e = errors("-- say-language: 1\nthe theme:\n    accent is #FF0000\nscreen \"A\":\n    a text \"x\"\n");
        assert!(e.iter().any(|m| m.contains("colour slot")), "{e:?}");
        let e = errors(
            "-- say-language: 1\nthe theme:\n    primary is red\nscreen \"A\":\n    a text \"x\"\n",
        );
        assert!(e.iter().any(|m| m.contains("#RRGGBB")), "{e:?}");
        let e = errors("-- say-language: 1\nthe theme:\n    primary is #FF0000\n    primary is #00FF00\nscreen \"A\":\n    a text \"x\"\n");
        assert!(e.iter().any(|m| m.contains("already set")), "{e:?}");
    }

    #[test]
    fn an_image_path_stays_inside_its_folder() {
        let e = errors("-- say-language: 1\nscreen \"A\":\n    an image \"../secret.png\"\n");
        assert!(e.iter().any(|m| m.contains("`..`")), "{e:?}");
    }

    #[test]
    fn the_asset_root_is_written_into_the_file() {
        let source = include_str!("../tests/fixtures/image.say");
        let options = crate::Options {
            asset_root: "/work/app/screens".to_owned(),
        };
        let rust = crate::compile_with("image.say", source, &options)
            .unwrap()
            .rust;
        assert!(rust.contains("const SAY_ASSET_ROOT: &str = \"/work/app/screens\";"));
        assert_eq!(
            rust,
            crate::compile_with("image.say", source, &options)
                .unwrap()
                .rust
        );
        let plain = crate::compile("counter.say", include_str!("../tests/fixtures/counter.say"))
            .unwrap()
            .rust;
        assert!(
            !plain.contains("SAY_ASSET_ROOT"),
            "a file without images carries no root"
        );
    }

    /// The review's C1, pinned: the action rows compile as written, which is
    /// what the include! test proves end to end — this one keeps the
    /// failure readable when only the actions changed.
    #[test]
    fn the_counter_parses_without_diagnostics() {
        let source = include_str!("../tests/fixtures/counter.say");
        assert!(crate::compile("counter.say", source).is_ok());
    }
}
