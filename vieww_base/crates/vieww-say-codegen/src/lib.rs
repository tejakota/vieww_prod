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
pub mod line;
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
    /// The screen the compiled app **starts on**, when it is not the first
    /// one — a title exactly as `screen "…":` wrote it.
    ///
    /// This is the studio CLI's door (`render --screen`, `animate --screen`,
    /// `preview --screen`, and one render per screen for `--all-screens` and
    /// `test`): the same compile, the same navigation, with the stack seeded
    /// so `go back` still lands on the home screen — deep-linked, in the
    /// navigator's own vocabulary. `None` (the default) starts on the first
    /// screen, which is the preview pane's behaviour and the built app's.
    ///
    /// A title no `screen` block carries is a compile error (E090), not a
    /// silent fall-back to the first screen: a render asked for "Settings"
    /// that quietly shows Home is the confidently wrong answer.
    pub initial_screen: Option<String>,
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
        Ok(program) => {
            // **A named start screen is checked against the source, here.**
            // The alternative — ignoring a title nothing carries — is a
            // `render --screen "Settings"` that quietly draws Home, which
            // is worse than an error: it looks like the screen is empty.
            // E090, the navigation code, because this is a navigation
            // question: `open the screen "X"` with no screen "X".
            if let Some(title) = &options.initial_screen {
                if !program.screens.iter().any(|s| &s.title == title) {
                    let known: Vec<&str> =
                        program.screens.iter().map(|s| s.title.as_str()).collect();
                    return Err(vec![Diagnostic::error(
                        &file,
                        program.screens.first().map_or(1, |s| s.line),
                        1,
                        crate::diag::codes::E090,
                        format!(
                            "there is no screen called \"{title}\" — this file defines {}",
                            known
                                .iter()
                                .map(|t| format!("\"{t}\""))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )]);
                }
            }
            Ok(Generated {
                rust: gen::generate(
                    &file,
                    &program,
                    &gen::GenOptions {
                        asset_root: options.asset_root.clone(),
                        initial_screen: options.initial_screen.clone(),
                    },
                ),
                warnings: program.warnings,
            })
        }
        Err(diagnostics) => Err(diagnostics),
    }
}

/// Compile one extracted subtree into a component file (ST-05).
///
/// The studio's "extract widget" hands over the picked subtree's lines
/// verbatim — indentation relative to the file and all — and this wraps them
/// in a synthetic screen, parses them, refuses everything a component cannot
/// carry, and generates `pub struct <Name>` plus `impl Widget` for
/// `src/widgets/<name>.rs`. The screen left behind references the result as
/// `<Name>`.
///
/// # What a component may not contain, and why
///
/// A component is a **pure subtree**: it is built wherever `<Name>` stands,
/// with nothing but the theme its host already supplies. So it may not read
/// state (`keep` variables, interpolation, `only if`), carry events (a
/// handler reaches the *screen's* element), bind controls, list anything,
/// scroll, or hold tabs — those live on the screen's own `ElementState`, and
/// a subtree that reached for them would stop being the same widget where it
/// stood. Diagnostics carry E100 and the line *within the subtree* (the
/// caller knows where the subtree began); each names the construct that
/// refused.
///
/// # Errors
///
/// Every refusal, and every ordinary parse error in the subtree — not the
/// first one, for the same reason as [`compile`].
pub fn compile_component(
    file_name: &str,
    name: &str,
    subtree: &str,
    options: &Options,
) -> Result<Generated, Vec<Diagnostic>> {
    let file = std::rc::Rc::new(file_name.to_owned());
    // The name is the Rust type: UpperCamel or nothing, so a bad name is
    // refused here rather than by rustc against a file the user never wrote.
    if crate::parse::component_name(&format!("<{name}>")).is_none() {
        return Err(vec![Diagnostic::error(
            &file,
            1,
            1,
            crate::diag::codes::E031,
            format!(
                "`{name}` is not a component name — one UpperCamel word, `Note` or `PhotoGrid`"
            ),
        )]);
    }
    // Dedent to the subtree's own root, then indent every line under one
    // synthetic screen. Blank and comment lines keep their shape.
    let indent = subtree
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    let mut wrapped = String::from("-- say-language: 1\nscreen \"Component\":\n");
    for line in subtree.lines() {
        let dedented = line.get(indent..).unwrap_or(line);
        if dedented.trim().is_empty() {
            wrapped.push('\n');
        } else {
            wrapped.push_str("    ");
            wrapped.push_str(dedented);
            wrapped.push('\n');
        }
    }
    let lines = lex::lex(&wrapped);
    let program = match parse::parse(&file, &lines) {
        Ok(program) => program,
        Err(mut diagnostics) => {
            // A subtree that references a name declared *outside* it — every
            // `keep` variable, every bound control — parses as "nothing is
            // called that" here, because the declaration stayed behind with
            // the screen. That is the refusal, said once up front, with the
            // parse errors naming what was reached for.
            let mut out = vec![Diagnostic::error(
                &file,
                1,
                1,
                crate::diag::codes::E100,
                "the subtree does not stand alone — a component may not reach \
                 the state the screen keeps, and the errors below name what it \
                 reaches for",
            )];
            for diagnostic in diagnostics.drain(..) {
                out.push(Diagnostic {
                    line: diagnostic.line.saturating_sub(2),
                    ..diagnostic
                });
            }
            return Err(out);
        }
    };
    let body = program
        .screens
        .first()
        .map(|screen| screen.body.as_slice())
        .unwrap_or(&[]);
    let refusals = gen::component_refusals(&file, body);
    if !refusals.is_empty() {
        // Line numbers within the wrapped screen are subtree lines plus the
        // two-line head; put them back where the caller's buffer has them.
        return Err(refusals
            .into_iter()
            .map(|mut diagnostic| {
                diagnostic.line = diagnostic.line.saturating_sub(2);
                diagnostic
            })
            .collect());
    }
    Ok(Generated {
        rust: gen::generate_component(
            &file,
            name,
            body,
            &gen::GenOptions {
                asset_root: options.asset_root.clone(),
                // A component is one subtree, not an app; it has no screens
                // to choose between.
                initial_screen: None,
            },
        ),
        warnings: Vec::new(),
    })
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
            ..Default::default()
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

    /// ST-05: a pure subtree becomes a component, and the screen left behind
    /// references it. The two halves of the contract, pinned where a change
    /// to either shows up as a diff.
    #[test]
    fn a_component_reference_and_its_extraction_compile() {
        // The screen, with the subtree still inline-extractable.
        let screen = "-- say-language: 1\n\
                      screen \"Home\":\n\
                      \x20   a column:\n\
                      \x20       a card, padded 12:\n\
                      \x20           a heading \"A note\"\n\
                      \x20           a text \"Extract me\", color #3366FF\n\
                      \x20       a text \"Stays\"\n";
        crate::compile("home.say", screen).expect("the screen before extraction compiles");
        // The picked subtree, as the studio would hand it over: original
        // indentation, taken verbatim out of the buffer.
        let subtree = "    a card, padded 12:\n\
                       \x20       a heading \"A note\"\n\
                       \x20       a text \"Extract me\", color #3366FF\n";
        let component =
            crate::compile_component("home.say", "Note", subtree, &crate::Options::default())
                .expect("a pure subtree extracts");
        assert!(
            component.rust.contains("pub struct Note;"),
            "{}",
            component.rust
        );
        assert!(component.rust.contains("impl Widget for Note"));
        assert!(component.rust.contains("use vieww::prelude::*;"));
        // The theme read the `color #3366FF` text style needs is `ctx`'s own.
        assert!(component.rust.contains("let theme = ThemeData::of(ctx);"));

        // The screen after the span is replaced with the reference.
        let after = "-- say-language: 1\n\
                     screen \"Home\":\n\
                     \x20   a column:\n\
                     \x20       <Note>\n\
                     \x20       a text \"Stays\"\n";
        let generated = crate::compile("home.say", after).expect("the reference compiles");
        assert!(
            generated
                .rust
                .contains("WidgetNode::new(crate::widgets::note::Note)"),
            "the reference reaches the project's module: {}",
            generated.rust
        );
        assert!(
            generated.rust.contains("// say: home.say:4  <Note>"),
            "the marker shows the line as written"
        );
    }

    /// The refusals are the contract's other half: a subtree that reaches
    /// the screen's state, events or interface state is not a component, and
    /// the diagnostic says which line and why.
    ///
    /// Two doors lead out: names the subtree reaches for fail the parse
    /// (`nothing is called \`count\`` — the declaration stayed with the
    /// screen), and constructs that parse standalone are refused by the
    /// purity walk. Both say E100 first.
    #[test]
    fn impure_subtrees_are_refused_with_the_reason() {
        let cases: [(&str, &str); 3] = [
            (
                "a button \"Save\" which when tapped:\n    show a snackbar \"Saved\"\n",
                "events reach the screen's state",
            ),
            (
                "a scroll view:\n    a text \"long\"\n",
                "a scroll view keeps its offset in the screen's state",
            ),
            (
                "a set of tabs:\n    a tab \"One\":\n        a text \"x\"\n",
                "tabs keep their selection in the screen's state",
            ),
        ];
        for (subtree, why) in cases {
            let refusals =
                crate::compile_component("home.say", "Note", subtree, &crate::Options::default())
                    .expect_err("the subtree is refused");
            assert!(
                refusals
                    .iter()
                    .any(|d| d.code == "E100" && d.message.contains(why)),
                "{subtree}: {refusals:?}"
            );
        }
        // A name declared outside the subtree: the parse says so under the
        // E100 that explains why that matters here.
        let reaching = crate::compile_component(
            "home.say",
            "Note",
            "a text field bound to query\n",
            &crate::Options::default(),
        )
        .expect_err("a bound control is refused");
        assert!(
            reaching
                .iter()
                .any(|d| d.code == "E100" && d.message.contains("does not stand alone")),
            "{reaching:?}"
        );
        assert!(reaching.iter().any(|d| d.code == "E031"), "{reaching:?}");
        // And a name that is not a Rust type name is refused before any of
        // that: rustc would otherwise fail against a file nobody wrote.
        let bad = crate::compile_component(
            "home.say",
            "note",
            "a text \"x\"\n",
            &crate::Options::default(),
        )
        .expect_err("a lowercase name is refused");
        assert!(bad.iter().any(|d| d.code == "E031"), "{bad:?}");
    }

    /// The review's C1, pinned: the action rows compile as written, which is
    /// what the include! test proves end to end — this one keeps the
    /// failure readable when only the actions changed.
    #[test]
    fn the_counter_parses_without_diagnostics() {
        let source = include_str!("../tests/fixtures/counter.say");
        assert!(crate::compile("counter.say", source).is_ok());
    }

    /// A named start screen seeds the navigation stack — the CLI's
    /// `--screen` door. The floor stays the first screen (`current`'s
    /// `unwrap_or`), so `go back` from a deep-linked render lands on Home
    /// exactly as it does in the running app.
    #[test]
    fn a_named_start_screen_seeds_the_stack() {
        let source = include_str!("../tests/fixtures/transition.say");
        let options = crate::Options {
            initial_screen: Some("Settings".to_owned()),
            ..Default::default()
        };
        let rust = crate::compile_with("transition.say", source, &options)
            .expect("the fixture names the screen")
            .rust;
        assert!(
            rust.contains("stack: vec![SayScreens::Settings],"),
            "the stack starts on Settings"
        );
        // And the floor is untouched: `current`'s fallback is still the
        // first screen, which is what `go back` pops down to.
        assert!(
            rust.contains("unwrap_or(&SayScreens::Home)"),
            "Home stays the floor"
        );
    }

    /// Without the option, the stack starts empty — which is what every
    /// committed `*_gen.rs` fixture says, so this is the guard that the new
    /// option cannot change the default output by one byte.
    #[test]
    fn without_a_named_start_screen_nothing_changes() {
        let source = include_str!("../tests/fixtures/transition.say");
        let rust = crate::compile("transition.say", source).unwrap().rust;
        assert!(
            rust.contains("stack: Vec::new(),"),
            "the default starts on the first screen, via an empty stack"
        );
    }

    /// A title no `screen` block carries is a refusal that names the ones
    /// that exist — E090, the navigation code, because this is
    /// `open the screen "X"` with no screen "X" wearing a command-line hat.
    #[test]
    fn an_unknown_start_screen_is_refused_with_the_known_ones() {
        let source = include_str!("../tests/fixtures/transition.say");
        let options = crate::Options {
            initial_screen: Some("Nowhere".to_owned()),
            ..Default::default()
        };
        let diagnostics =
            crate::compile_with("transition.say", source, &options).expect_err("no such screen");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E090" && d.message.contains("\"Settings\"")),
            "the refusal names the screens that do exist: {diagnostics:?}"
        );
    }
}
