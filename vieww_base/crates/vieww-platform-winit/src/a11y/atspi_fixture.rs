//! The AccessKit side of the AT-SPI seam, dumped as JSON for the Linux
//! screen-reader harness (`ci/check/a11y-linux.sh`, FW-10b).
//!
//! # What this is, and what it is not
//!
//! This is the tree **as this crate hands it to AccessKit** —
//! [`tree_update`](super::tree_update) over the real demo screen
//! (`examples/shared/screen.rs`, pulled in with `#[path]` exactly the way
//! every entry point pulls it in) — which is the tree AccessKit's Unix
//! adapter serialises onto the AT-SPI bus, and therefore the tree Orca (or
//! any other AT-SPI client) reads. What this is *not* is a reading of the
//! bus itself: the adapter's own mapping from AccessKit roles to AT-SPI
//! roles is AccessKit's code, not vieww's, and only the live leg of the
//! harness — a real desktop session with `pyatspi` — exercises it. The
//! fixture this module writes is what the harness's dry-run asserts over,
//! and the first live recording is what confirms the mapping the harness's
//! python carries for it.
//!
//! # Why the dump is a test and not a binary
//!
//! The same reason `a11y.rs`'s other tests exist: the assertions below run
//! on every `cargo test -p vieww-platform-winit`, on any machine, with no
//! window and no screen reader — which is the only way the AccessKit side
//! of this seam is checked at all outside a desktop session. Writing the
//! fixture is the *optional* half (`VIEWW_ATSPI_FIXTURE=<file>`), and it
//! happens inside the test so the fixture and the assertions can never
//! disagree about which tree they describe.

use std::time::Duration;

use vieww_foundation::Size;
use vieww_render::FrameDriver;
use vieww_widget::prelude::*;

use super::tree_update;
use crate::scale::Scale;

/// The demo screen, unchanged, exactly as the desktop and phone entry points
/// build it — one copy, so the fixture cannot drift from the app the live leg
/// runs (`#[path]`, the same trick `examples/desktop.rs` uses).
// The fixture only reads the tree, so most of the app's screen API is unused here.
#[allow(dead_code)]
#[path = "../../examples/shared/screen.rs"]
mod screen;

/// The size the desktop entry point previews the phone screen at; the tree is
/// the same at any size, but the bounds in the fixture should be the ones a
/// reader of the dump can compare against a desktop run.
const PREVIEW: Size = Size {
    width: 420.0,
    height: 880.0,
};

/// Build the demo's tree and hand it to AccessKit, the way `App` would.
fn demo_update() -> accesskit::TreeUpdate {
    let mut driver = FrameDriver::new(PREVIEW);
    let demo = screen::Demo::new(&mut driver);
    // `SizedBox::shrink()` is the documented "no probe" argument: the probe
    // only exists so `device_tests` can read view metrics, which a dump does
    // not need, and leaving it out is what every non-suite caller passes.
    driver.set_root(demo.screen(
        "A frame, on a desktop.",
        vieww_foundation::TargetPlatform::Linux,
        SizedBox::shrink(),
    ));
    driver.draw_frame_at(Duration::ZERO);
    // One frame is enough for the tree — semantics are published by the first
    // layout, not by any later frame — but an animating tree would still be
    // mid-entrance at zero, so let the entrance finish the way the audit mode
    // in `examples/features/harness` does, at a fixed cadence.
    if driver.is_animating() {
        const AUDIT_MS: f64 = 1000.0;
        const STEP_MS: f64 = 1000.0 / 60.0;
        let mut at = 0.0;
        while at < AUDIT_MS {
            at = (at + STEP_MS).min(AUDIT_MS);
            driver.draw_frame_at(Duration::from_secs_f64(at / 1000.0));
        }
    }
    let semantics = driver.semantics();
    tree_update(&semantics, Scale::new(1.0)).expect("a laid-out tree has a root")
}

/// One node as the JSON the harness reads: only the fields the harness's
/// assertions speak — role, name, value, state, bounds, children — because a
/// dump of everything AccessKit carries would be a mirror of somebody else's
/// struct rather than a contract. The role is AccessKit's own `Debug` name
/// ("Button", "TextInput"), because that is the vocabulary *this* side of
/// the seam speaks; the AT-SPI vocabulary is what the live leg reads, and
/// the harness's python is the one place that knows both spellings.
fn node_json(id: accesskit::NodeId, node: &accesskit::Node) -> String {
    let mut out = String::new();
    let generation = id.0 >> 32;
    let index = id.0 & u64::from(u32::MAX);
    out.push_str(&format!("{{\"id\": \"{generation}.{index}\""));
    out.push_str(&format!(", \"role\": \"{:?}\"", node.role()));
    if let Some(label) = node.label() {
        out.push_str(&format!(", \"label\": {}", json_string(label)));
    }
    if let Some(value) = node.value() {
        out.push_str(&format!(", \"value\": {}", json_string(value)));
    }
    if let Some(toggled) = node.toggled() {
        out.push_str(&format!(
            ", \"toggled\": \"{}\"",
            match toggled {
                accesskit::Toggled::True => "true",
                accesskit::Toggled::False => "false",
                accesskit::Toggled::Mixed => "mixed",
            }
        ));
    }
    if node.is_selected() == Some(true) {
        out.push_str(", \"selected\": true");
    }
    if node.is_disabled() {
        out.push_str(", \"disabled\": true");
    }
    if node.supports_action(accesskit::Action::Focus) {
        out.push_str(", \"focusable\": true");
    }
    if node.supports_action(accesskit::Action::Click) {
        out.push_str(", \"clickable\": true");
    }
    if let Some(bounds) = node.bounds() {
        out.push_str(&format!(
            ", \"bounds\": [{:.1}, {:.1}, {:.1}, {:.1}]",
            bounds.x0, bounds.y0, bounds.x1, bounds.y1
        ));
    }
    let children: Vec<String> = node
        .children()
        .iter()
        .map(|child| format!("\"{}.{}\"", child.0 >> 32, child.0 & u64::from(u32::MAX)))
        .collect();
    out.push_str(&format!(", \"children\": [{}]", children.join(", ")));
    out.push('}');
    out
}

/// A JSON string literal, hand-escaped — the repository's rule is no new
/// dependencies, and the label this escapes is the demo's own text.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The whole tree as the fixture document the harness reads.
fn fixture_json(update: &accesskit::TreeUpdate) -> String {
    let mut out = String::from(
        "{\n  \"source\": \"vieww-platform-winit tree_update over examples/shared/screen.rs \
         (the AccessKit side of the AT-SPI seam; roles are accesskit Debug names)\",\n",
    );
    out.push_str("  \"scale\": 1.0,\n");
    let root = update
        .tree
        .as_ref()
        .map_or(accesskit::NodeId(0), |tree| tree.root);
    out.push_str(&format!(
        "  \"root\": \"{}.{}\",\n",
        root.0 >> 32,
        root.0 & u64::from(u32::MAX)
    ));
    out.push_str("  \"nodes\": [\n");
    let nodes: Vec<String> = update
        .nodes
        .iter()
        .map(|(id, node)| format!("    {}", node_json(*id, node)))
        .collect();
    out.push_str(&nodes.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

/// The AccessKit side of the demo's tree, asserted here on every test run —
/// no window, no bus, no screen reader, which is the only machine every
/// contributor has.
#[test]
fn the_demo_tree_reaches_accesskit_intact() {
    let update = demo_update();

    // The root is a window, and the tree is not the placeholder: a tree that
    // collapsed to one featureless window is exactly what a broken bridge
    // looks like from the outside (see a11y-android.sh's header for the
    // Android twin of that failure).
    let root = update
        .tree
        .as_ref()
        .map_or(accesskit::NodeId(0), |tree| tree.root);
    let nodes: std::collections::HashMap<accesskit::NodeId, &accesskit::Node> =
        update.nodes.iter().map(|(id, n)| (*id, n)).collect();
    assert!(
        update.nodes.len() > 10,
        "the demo tree reached AccessKit with {} node(s) — a tree this small is \
         the placeholder, not the demo",
        update.nodes.len()
    );
    assert_eq!(
        nodes[&root].role(),
        accesskit::Role::Window,
        "the root of the published tree is not a window"
    );

    // Every child id resolves: a dangling child is a node a screen reader
    // walks into and crashes on, and it is this crate's bug, not AccessKit's.
    for (id, node) in &nodes {
        for child in node.children() {
            assert!(
                nodes.contains_key(child),
                "node {id:?} lists a child {child:?} that is not in the update"
            );
        }
    }

    // The button the whole suite family names: present, labelled, focusable.
    let tap = nodes
        .values()
        .find(|node| node.label() == Some("Tap me"))
        .expect("the demo's button never reached the AccessKit side of the seam");
    assert_eq!(tap.role(), accesskit::Role::Button);
    assert!(
        tap.supports_action(accesskit::Action::Focus),
        "\"Tap me\" is not focusable — a screen reader cannot stop on it"
    );

    // The field the IME legs type into, carrying its seeded value.
    let field = nodes
        .values()
        .find(|node| node.value() == Some("tap to type"))
        .expect("the demo's field never reached the AccessKit side of the seam");
    assert!(
        matches!(field.role(), accesskit::Role::TextInput),
        "the node holding \"tap to type\" is {:?}, not a text input",
        field.role()
    );

    // Every interactive node is named: the audit's own rule, restated at the
    // seam so a regression in the *conversion* (a label dropped between the
    // semantics tree and AccessKit) cannot hide behind a green tree-side audit.
    for node in nodes.values() {
        let interactive = matches!(
            node.role(),
            accesskit::Role::Button
                | accesskit::Role::TextInput
                | accesskit::Role::CheckBox
                | accesskit::Role::RadioButton
                | accesskit::Role::Switch
                | accesskit::Role::Slider
                | accesskit::Role::Tab
                | accesskit::Role::MenuItem
                | accesskit::Role::Link
        );
        if interactive {
            let named = node.label().is_some_and(|label| !label.is_empty())
                || node.value().is_some_and(|value| !value.is_empty());
            assert!(
                named,
                "an interactive {:?} reached AccessKit with no name — a screen \
                 reader announces it as its role and nothing else",
                node.role()
            );
        }
    }

    // The fixture itself: written only when asked, so a normal test run
    // writes nothing and the harness's dry-run is the only caller.
    if let Some(path) = std::env::var_os("VIEWW_ATSPI_FIXTURE") {
        std::fs::write(&path, fixture_json(&update)).unwrap_or_else(|error| {
            panic!("could not write the AT-SPI fixture to {path:?}: {error}")
        });
    }
}
