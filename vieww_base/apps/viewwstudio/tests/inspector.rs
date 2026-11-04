//! ST-02 end to end: the property grid is the same editor, typed.
//!
//! # What this file proves
//!
//! The Inspector's writable half on the real counter fixture, through the real
//! mounted preview and the real pick — not on a mock of either:
//!
//! 1. [`reading_every_field_and_writing_it_back_is_the_identity`] — the
//!    round-trip: every field the grid reads off the picked line, written back
//!    unchanged, leaves the buffer byte-identical. A grid that invents text on
//!    the way back through would fail this.
//! 2. [`a_field_write_rewrites_the_one_line_and_it_compiles`] — a spacing edit
//!    through the grid changes exactly the column's line, the file still
//!    compiles through the real Say compiler, and the grid reads the new value
//!    back.
//! 3. [`the_say_line_numbers_survive_the_writes`] — the line numbers the
//!    render-tree ↔ say-tree bridge navigates by are stable across grid
//!    writes, including a clause insertion onto a line that did not carry one.
//! 4. [`a_refused_write_notifies_and_leaves_the_buffer_alone`] — a colour Say
//!    does not know, and an interpolated literal, both refuse with a message
//!    and change nothing.
//!
//! The unit tests in `state/inspector_props.rs` prove the field set per kind
//! and the mirror/acceptance cross-check; this file proves the writes land.

use vieww_element::Runtime;
use vieww_foundation::{Size, TargetPlatform};
use vieww_render::FrameDriver;
use vieww_say_codegen::line::parse_widget_lines;
use viewwstudio::buffer::{Buffer, Workspace};
use viewwstudio::loaded::Preview;
use viewwstudio::state::{FieldValue, RightTab, Studio};
use viewwstudio::ui::preview::PreviewPane;

/// The counter, as the repository ships it: the `.say` file and the Rust the
/// Say compiler generates from it. One fixture, both sides of the pipeline.
const COUNTER_SAY: &str =
    include_str!("../../../crates/vieww-say-codegen/tests/fixtures/counter.say");

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod counter {
    include!("../../../crates/vieww-say-codegen/tests/fixtures/counter_gen.rs");
}

/// A studio whose one open buffer is `counter.say`, read from disk the way a
/// person's is — through `Buffer::open`, so the language is detected rather
/// than asserted, and the grid has a real Say buffer to read and refuse.
fn say_studio(tag: &str) -> Studio {
    let dir = std::env::temp_dir().join(format!("inspector-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temp directory");
    let path = dir.join("counter.say");
    std::fs::write(&path, COUNTER_SAY).expect("the fixture on disk");
    let buffer = Buffer::open(&path).expect("the fixture opens");
    let runtime = Runtime::new();
    let workspace = Workspace::of_buffers(vec![buffer]);
    Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux)
}

/// The same studio with the counter's generated screen mounted in the real
/// preview pane, one frame drawn — exactly the state the preview is in when a
/// person picks a widget.
fn mounted(studio: &Studio) -> FrameDriver {
    let mut driver = FrameDriver::new(Size::new(900.0, 900.0));
    studio
        .preview_screen
        .set(Some(Preview::from_widget(counter::screen())));
    driver.set_root(PreviewPane {
        studio: studio.clone(),
    });
    driver.draw_frame();
    driver
}

/// Capture the guest tree with the Inspector open, and pick the first row
/// whose render object is `name` — the row a click on that widget would pick.
fn pick_row(studio: &Studio, driver: &mut FrameDriver, name: &str) -> usize {
    studio.right_open.set(true);
    studio.right_tab.set(RightTab::Inspector);
    studio.capture_tree(driver);
    let rows = studio.tree_snapshot.borrow();
    let row = rows
        .iter()
        .position(|(_, node)| node.name == name)
        .unwrap_or_else(|| panic!("a {name} row in the captured tree"));
    drop(rows);
    studio.inspect_selected.set(Some(row));
    row
}

/// The buffer text, for before/after comparisons.
fn text(studio: &Studio) -> String {
    studio.active().expect("a buffer").value.text.clone()
}

// ===================== the round trip ======================================

/// Every field the grid offers for the picked column whose value the line
/// itself carries, written back through the grid, is the identity: the buffer
/// does not move by one byte. Fields for properties the line does *not* carry
/// are excluded — writing one of those inserts its clause, which is the next
/// test's job, not this one's.
#[test]
fn reading_every_field_and_writing_it_back_is_the_identity() {
    let studio = say_studio("identity");
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderColumn");
    let before = text(&studio);

    let widget = studio.picked_say_line().expect("the column maps to a line");
    let fields = studio.say_fields().expect("the column has fields");
    for field in &fields {
        let on_the_line = widget.props.iter().any(|p| {
            field
                .prop
                .as_ref()
                .is_some_and(|shape| std::mem::discriminant(shape) == std::mem::discriminant(p))
        });
        if !on_the_line {
            continue;
        }
        studio.say_set(field, &field.value);
    }
    assert_eq!(text(&studio), before, "a read written back is a no-op");
}

// ===================== the write ===========================================

/// A spacing edit through the grid rewrites the column's line and only it,
/// the file compiles through the real Say compiler, and the grid reads the
/// new value back off the line.
#[test]
fn a_field_write_rewrites_the_one_line_and_it_compiles() {
    let studio = say_studio("write");
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderColumn");
    let before = text(&studio);

    let fields = studio.say_fields().expect("the column has fields");
    let spacing = fields
        .iter()
        .find(|f| f.label == "spacing")
        .expect("a spacing field");
    assert_eq!(
        spacing.value,
        FieldValue::Number(16.0),
        "read from the line"
    );
    studio.say_set(spacing, &FieldValue::Number(24.0));

    let after = text(&studio);
    assert_eq!(
        after.lines().count(),
        before.lines().count(),
        "one line rewritten, none added or removed"
    );
    let changed: Vec<(usize, (&str, &str))> = before
        .lines()
        .zip(after.lines())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .collect();
    assert_eq!(changed.len(), 1, "exactly one line differs: {changed:?}");
    assert!(
        changed[0].1 .1.contains("spaced 24"),
        "the column's line carries the new spacing: {}",
        changed[0].1 .1
    );
    vieww_say_codegen::compile("counter.say", &after).expect("the edited file compiles");

    // And the grid reads it back: the value the file holds is the value the
    // next pick would show.
    let fields = studio.say_fields().expect("the column still has fields");
    let spacing = fields
        .iter()
        .find(|f| f.label == "spacing")
        .expect("a spacing field");
    assert_eq!(spacing.value, FieldValue::Number(24.0));
}

// ===================== the line numbers ====================================

/// The line numbers the bridge navigates by survive grid writes — including
/// an insertion onto a line that did not carry the clause, which is the write
/// that could have moved a line if the rewrite were not one-line-local.
#[test]
fn the_say_line_numbers_survive_the_writes() {
    let studio = say_studio("lines");
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderColumn");
    let before = text(&studio);
    let before_lines: Vec<u32> = parse_widget_lines(&before)
        .into_iter()
        .map(|widget| widget.line)
        .collect();

    let fields = studio.say_fields().expect("the column has fields");
    // An absent clause: corner radius is offered at the framework's default
    // and not on the fixture's line, so writing it inserts the clause.
    let radius = fields
        .iter()
        .find(|f| f.label == "corner radius")
        .expect("a corner radius field");
    studio.say_set(radius, &FieldValue::Number(12.0));
    // And a present one, for the replacement path in the same breath.
    let fields = studio.say_fields().expect("fields after the insertion");
    let spacing = fields
        .iter()
        .find(|f| f.label == "spacing")
        .expect("a spacing field");
    studio.say_set(spacing, &FieldValue::Number(8.0));

    let after = text(&studio);
    let after_lines: Vec<u32> = parse_widget_lines(&after)
        .into_iter()
        .map(|widget| widget.line)
        .collect();
    assert_eq!(
        before_lines, after_lines,
        "every widget is where the bridge expects it"
    );
    assert_eq!(
        before.lines().count(),
        after.lines().count(),
        "no line was added or removed"
    );
    vieww_say_codegen::compile("counter.say", &after).expect("the edited file compiles");
}

// ===================== the refusals =======================================

/// Two refusals, both refusing out loud and both leaving the buffer alone: a
/// colour the Say vocabulary does not have, and the interpolated literal the
/// line API will not rewrite for fear of dropping the `\(…)` silently.
#[test]
fn a_refused_write_notifies_and_leaves_the_buffer_alone() {
    let studio = say_studio("refused");
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderColumn");
    let before = text(&studio);

    let fields = studio.say_fields().expect("the column has fields");
    let color = fields
        .iter()
        .find(|f| f.label == "color")
        .expect("a color field");
    studio.say_set(color, &FieldValue::Color("teal".to_owned()));
    let notice = studio.notice.get().expect("a refusal says why");
    assert!(
        notice.contains("teal"),
        "the message names what was refused: {notice}"
    );
    assert_eq!(text(&studio), before, "nothing was written");

    // The label's literal interpolates `\(count)`: read-only, and a write
    // attempt says so rather than dropping the interpolation. The counter's
    // two text rows are the heading first and the label second.
    let rows = studio.tree_snapshot.borrow();
    let texts: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, (_, node))| node.name == "RenderText")
        .map(|(i, _)| i)
        .collect();
    drop(rows);
    let label = *texts.get(1).expect("the label after the heading");
    studio.inspect_selected.set(Some(label));
    let fields = studio.say_fields().expect("the label has fields");
    let literal = fields
        .iter()
        .find(|f| f.label == "text")
        .expect("a text field");
    assert!(
        !literal.writable,
        "an interpolated literal is marked read-only"
    );
    studio.notice.set(None);
    studio.say_set(literal, &FieldValue::Text("no".to_owned()));
    assert!(
        studio.notice.get().is_some(),
        "the write attempt says why it will not"
    );
    assert_eq!(text(&studio), before, "still nothing was written");
}

/// The 600 ms auto-render debounce engages for a grid write exactly as for a
/// keystroke — the grid is not a path around the editor's own pipeline.
#[test]
fn a_grid_write_engages_the_auto_render_debounce() {
    let studio = say_studio("debounce");
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderColumn");
    studio.auto_render.set(true);
    assert!(
        studio.render_due.borrow().is_none(),
        "nothing is pending before the write"
    );

    let fields = studio.say_fields().expect("the column has fields");
    let spacing = fields
        .iter()
        .find(|f| f.label == "spacing")
        .expect("a spacing field");
    studio.say_set(spacing, &FieldValue::Number(32.0));
    assert!(
        studio.render_due.borrow().is_some(),
        "the grid write armed the auto-render debounce"
    );
}
