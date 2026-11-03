//! ST-19: the Live Preview's upgrade path — the sketch becomes a `.say` file.
//!
//! # Why this module exists
//!
//! The Live Preview draws `live.rs` with the studio's own widgets: no `rustc`,
//! no logic, switches and counters held by the *preview*. It is honest about
//! that (the caution dialog says it), fast, and — left as the only thing — a
//! trap: a person who lays out a whole flow in it and then presses Build gets
//! something that looks similar and behaves differently. The rest of the
//! studio is engineered around "the preview is not a mockup of your screen,
//! it is your screen"; the live pane was the one place that was not true, and
//! the only way out of it used to be starting over by hand.
//!
//! So the live preview now *generates* as well as simulates:
//! [`say_of`] turns a parsed [`LiveDoc`] into a real `.say` file — one `screen`
//! block per flow screen, buttons as `open the screen "…"` actions, toggles
//! and counters as `keep` state with real controls — and
//! [`Studio::materialize_flow`] writes it into the project and opens it. From
//! there it is an ordinary Say buffer: Render compiles it, the property grid
//! edits it, the canvas drags it, and what Build ships is what the preview
//! showed. The live pane stays for speed; the cliff between it and the app is
//! a command instead of a rewrite.
//!
//! # What the translation keeps, and what it says it cannot
//!
//! * `row` becomes a card with the title and detail as texts — the same
//!   visual shape, in the vocabulary Say actually has.
//! * `toggle` and `counter` become `keep` state, because in a real app that is
//!   what they are; the sketch's "the preview holds those" limitation dies
//!   with the sketch.
//! * `mount` and `nav` have no Say counterpart, and **pretending otherwise
//!   would be the mockup trap again**. They are written as comments in the
//!   generated file, named and explained, so the person upgrading sees the two
//!   things they still own by hand rather than a silent hole.
//! * Screen names and labels are escaped and identifier-ized, never trusted:
//!   a label with a quote in it becomes a valid Say string, and a toggle
//!   called "Wi-Fi" becomes `keep a flag called wi_fi`, not a broken file.

use crate::livedoc::{LiveDoc, LiveItem, LiveScreen};

/// The file the flow is materialized into, under the workspace root.
pub const FLOW_SAY: &str = "src/screens/flow.say";

/// The whole flow as one `.say` file.
///
/// Deterministic — the same `live.rs` always produces the same bytes, so a
/// second look at the command's output is a diff, not a surprise.
#[must_use]
pub fn say_of(doc: &LiveDoc) -> String {
    let mut out = String::new();
    out.push_str("-- say-language: 1\n");
    out.push_str(
        "-- Materialized from live.rs by \"Materialize the Live Flow as Say\".\n\
         -- The Live Preview drew this flow as a sketch, with its own switches and\n\
         -- counters. This is the same flow as real Say: Render compiles it, and\n\
         -- what Build ships is what the preview showed.\n",
    );

    // `keep` lines are file-level, so the state a whole flow needs is
    // collected first — one declaration per distinct toggle/counter label.
    let mut keeps: Vec<String> = Vec::new();
    for screen in &doc.screens {
        for item in &screen.items {
            match item {
                LiveItem::Toggle(label) => keeps.push(format!(
                    "keep a flag called {} starting at false",
                    ident(label)
                )),
                LiveItem::Counter(label) => keeps.push(format!(
                    "keep a whole number called {} starting at 0",
                    ident(label)
                )),
                _ => {}
            }
        }
    }
    if !keeps.is_empty() {
        out.push('\n');
        for keep in keeps {
            out.push_str(&keep);
            out.push('\n');
        }
    }

    for screen in &doc.screens {
        out.push('\n');
        screen_say(screen, &mut out);
    }

    if !doc.nav.is_empty() {
        // No Say counterpart: an app's bottom bar is the app's own chrome,
        // and a generated tab set would be a different app wearing the flow's
        // name. Said, not faked.
        out.push('\n');
        out.push_str(&format!(
            "-- nav {}: the app's own navigation, which Say does not describe.\n",
            doc.nav
                .iter()
                .map(|name| quote(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

/// One flow screen as one Say screen block.
fn screen_say(screen: &LiveScreen, out: &mut String) {
    out.push_str(&format!("screen {}:\n", quote(&screen.name)));
    out.push_str("    a column, spaced 8, children aligned to the start:\n");
    out.push_str(&format!("        a heading {}\n", quote(&screen.title)));
    for item in &screen.items {
        item_say(item, out);
    }
}

/// One flow item, as the Say lines that keep its shape.
fn item_say(item: &LiveItem, out: &mut String) {
    match item {
        LiveItem::Row { title, detail } => {
            out.push_str("        a card, padded 12, radius 8:\n");
            out.push_str("            a column, spaced 2, children aligned to the start:\n");
            out.push_str(&format!("                a text {}\n", quote(title)));
            out.push_str(&format!("                a text {}\n", quote(detail)));
        }
        LiveItem::Text(body) => {
            out.push_str(&format!("        a text {}\n", quote(body)));
        }
        LiveItem::Button { label, target } => {
            out.push_str(&format!(
                "        a button {} which when tapped:\n",
                quote(label)
            ));
            out.push_str(&format!("            open the screen {}\n", quote(target)));
        }
        LiveItem::Toggle(label) => {
            // The switch and its label, side by side — the sketch's shape,
            // with state the app keeps.
            out.push_str("        a row, padded 12, spaced 8:\n");
            out.push_str(&format!("            a text {}\n", quote(label)));
            out.push_str(&format!("            a switch bound to {}\n", ident(label)));
        }
        LiveItem::Counter(label) => {
            out.push_str("        a row, padded 12, spaced 8:\n");
            out.push_str(&format!("            a text {}\n", quote(label)));
            out.push_str(&format!("            a text \"\\({})\"\n", ident(label)));
            out.push_str(&format!(
                "            a button \"-\" which when tapped:\n                \
                 subtract 1 from {}\n",
                ident(label)
            ));
            out.push_str(&format!(
                "            a button \"+\" which when tapped:\n                \
                 add 1 to {}\n",
                ident(label)
            ));
        }
        LiveItem::Mount(file) => {
            // Not translatable, and not silently dropped: a `mount` is a real
            // screen of the user's own, and Say has no way to embed one. The
            // comment keeps the place and says what to do with it.
            out.push_str(&format!(
                "        -- mount {file}: a screen of your own, as it last rendered.\n"
            ));
            out.push_str("        -- Say cannot embed it: press Render on that file and\n");
            out.push_str("        -- write it into this screen by hand.\n");
        }
    }
}

/// A Say string literal: quoted, with the backslash and quote escaped so a
/// label cannot walk out of its quotes, and `\(` defused so a detail that
/// happens to contain one is not read as interpolation.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A label as a Say identifier: lowercase, words joined by `_`, anything the
/// language would not accept gone. `Wi-Fi` becomes `wi_fi`; `9 lives` becomes
/// `s_9_lives`, because a state name has to start with a letter.
fn ident(label: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    let trimmed = out.trim_matches('_').to_owned();
    let name = if trimmed.is_empty() {
        "state".to_owned()
    } else if trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
    {
        trimmed
    } else {
        format!("s_{trimmed}")
    };
    // A Rust keyword would compile in Say but not in the Rust it generates —
    // `keep a flag called type` would emit `type: bool`. The rare ones a
    // label can produce are escaped with the same prefix as a digit start.
    if matches!(
        name.as_str(),
        "type" | "fn" | "match" | "box" | "self" | "super" | "crate"
    ) {
        format!("s_{name}")
    } else {
        name
    }
}

impl crate::state::Studio {
    /// Write the flow as `src/screens/flow.say` and open it.
    ///
    /// The sketch must parse first — a `live.rs` with a mistake in it has a
    /// line number to fix, and materializing a half-edited file would bake
    /// the mistake into a file that no longer says where it came from. And
    /// an existing `flow.say` is never overwritten: by the time it exists it
    /// is the user's, edited or not, and the notification says what to do
    /// about it rather than doing it for them.
    pub fn materialize_flow(&self) {
        let Some(root) = self.root.peek().clone() else {
            self.notify("Open a folder first — the flow is materialized into the project.");
            return;
        };
        let doc = match self.live_source() {
            crate::live::LiveSource::Doc(doc) => doc,
            crate::live::LiveSource::Broken(error) => {
                self.notify(&format!(
                    "live.rs has a mistake on line {} — fix it before materializing.",
                    error.line
                ));
                return;
            }
            crate::live::LiveSource::Missing => {
                self.notify("There is no live.rs to materialize.");
                return;
            }
        };
        if doc.screens.is_empty() {
            self.notify("live.rs parsed, but it has no screens to materialize.");
            return;
        }
        let path = root.join(FLOW_SAY);
        if path.exists() {
            self.notify(&format!(
                "{} already exists — delete it to materialize the flow again.",
                FLOW_SAY
            ));
            self.open_path(path);
            return;
        }
        let say = say_of(&doc);
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                self.notify(&format!("Could not create {}: {error}", parent.display()));
                return;
            }
        }
        if let Err(error) = std::fs::write(&path, say) {
            self.notify(&format!("Could not write {}: {error}", path.display()));
            return;
        }
        self.rescan_tree();
        self.open_path(path);
        self.notify("The flow is Say now — press Render on it to compile the real thing.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template a new project's `live.rs` gets, parsed and materialized.
    fn materialized_template() -> String {
        let doc = crate::livedoc::parse(crate::livedoc::TEMPLATE)
            .unwrap_or_else(|e| panic!("the template parses: {e:?}"));
        say_of(&doc)
    }

    #[test]
    fn the_template_becomes_say_that_compiles() {
        let say = materialized_template();
        let generated = vieww_say_codegen::compile("flow.say", &say)
            .unwrap_or_else(|diagnostics| panic!("{say}\n did not compile: {diagnostics:?}"));
        assert!(
            generated.warnings.is_empty(),
            "without warnings: {:?}",
            generated.warnings
        );
        // Every flow screen became a screen function: the three the template
        // names, with the identifier Say derives from the name.
        for name in ["home", "detail", "settings"] {
            assert!(
                generated.rust.contains(&format!("fn screen_{name}")),
                "a screen function for {name}"
            );
        }
        // And the navigation survived as real actions, not comments.
        assert!(generated.rust.contains("open_screen(SayScreens::Settings)"));
        assert!(generated.rust.contains("open_screen(SayScreens::Home)"));
        // The sketch's toggle and counter are state the app keeps.
        assert!(say.contains("keep a flag called notifications starting at false"));
        assert!(say.contains("keep a whole number called badge_count starting at 0"));
        assert!(generated.rust.contains("notifications: bool"));
        assert!(generated.rust.contains("badge_count: i64"));
        // `mount` and `nav` are said, not faked.
        assert!(say.contains("-- mount src/screens/home.rs"));
        assert!(say.contains("-- nav \"Home\", \"Detail\", \"Settings\""));
    }

    #[test]
    fn labels_cannot_walk_out_of_their_quotes() {
        let doc = crate::livedoc::LiveDoc {
            screens: vec![crate::livedoc::LiveScreen {
                name: "Qu\"ote".to_owned(),
                title: "Back\\slash \\(interp)".to_owned(),
                items: vec![
                    crate::livedoc::LiveItem::Row {
                        title: "a \"quoted\" title".to_owned(),
                        detail: "plain".to_owned(),
                    },
                    crate::livedoc::LiveItem::Toggle("Wi-Fi".to_owned()),
                    crate::livedoc::LiveItem::Counter("9 lives".to_owned()),
                    crate::livedoc::LiveItem::Button {
                        label: "Go".to_owned(),
                        target: "Qu\"ote".to_owned(),
                    },
                ],
            }],
            nav: Vec::new(),
        };
        let say = say_of(&doc);
        // It compiles — the quotes and backslashes were escaped.
        vieww_say_codegen::compile("flow.say", &say)
            .unwrap_or_else(|diagnostics| panic!("{say}\n did not compile: {diagnostics:?}"));
        // The identifiers are Say-safe and Rust-safe.
        assert!(say.contains("a switch bound to wi_fi"));
        assert!(say.contains("keep a whole number called s_9_lives starting at 0"));
        assert!(say.contains("subtract 1 from s_9_lives"));
    }

    #[test]
    fn an_empty_flow_is_refused_by_say_not_shipped_broken() {
        // The command refuses an empty flow before writing anything; this is
        // the reason why, stated as Say's own answer rather than a policy
        // invented here: a `.say` file with no screen does not compile, so
        // materializing one would hand the user a file the studio rejects.
        let say = say_of(&crate::livedoc::LiveDoc::default());
        let diagnostics = vieww_say_codegen::compile("flow.say", &say)
            .expect_err("a file with no screens is not a screen");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "E010" && d.severity == vieww_say_codegen::Severity::Error),
            "Say's own refusal: {diagnostics:?}"
        );
        assert!(!say.contains("keep "), "no state was invented");
    }

    #[test]
    fn keyword_labels_are_prefixed_not_rejected() {
        assert_eq!(ident("Match"), "s_match");
        assert_eq!(ident("Wi-Fi"), "wi_fi");
        assert_eq!(ident("  spaced  out  "), "spaced_out");
        assert_eq!(ident("!!!"), "state");
        assert_eq!(ident("9"), "s_9");
    }
}
