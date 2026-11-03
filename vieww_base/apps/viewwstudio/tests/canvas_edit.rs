//! ST-01 end to end: canvas editing is buffer editing, pointed at.
//!
//! # What this file proves
//!
//! The preview's promise as a workspace: a picked render node maps back to
//! the `.say` line that produced it, a drag on a handle rewrites that one
//! line through the ordinary edit path, the file still compiles through the
//! real Say compiler — and the *pixels* move by exactly the delta the handle
//! wrote, measured on a real rasterisation of a really-compiled preview, not
//! on a mock of one.
//!
//! The unit tests in `state/canvas_edit.rs` prove the row→line matcher on
//! synthetic shapes. This file proves the whole chain on the counter fixture
//! the repository already ships:
//!
//! 1. [`a_padding_drag_rewrites_the_say_line`] — mount the generated counter,
//!    pick its column, drag the padding handle, and the eighth line of the
//!    buffer — and only that line — changes, and the result compiles.
//! 2. [`arrow_keys_nudge_only_while_the_mode_is_armed`] — the keyboard path
//!    through the real shortcut dispatcher, and its refusal to act when the
//!    mode is off.
//! 3. [`the_preview_png_shifts_by_exactly_the_delta`] — the same edit a
//!    spacing drag makes, taken all the way through `rustc` into two loaded
//!    previews: every render row either holds still or moves by exactly the
//!    delta, and the pixels that changed lie inside the union of where the
//!    moved rows were and where they went.
//!
//! # Why the pixels are compared rather than trusted
//!
//! A tree that says "8 points down" and a picture that moved 6 is the exact
//! failure mode direct manipulation ships with: the number is right where
//! somebody can read it and wrong where everybody can see it. So the tree
//! assertion and the pixel assertion are separate, and the second is derived
//! from the first rather than agreeing with it by construction.

use std::path::{Path, PathBuf};
use std::time::Duration;

use vieww_element::Runtime;
use vieww_foundation::{Color, KeyEvent, Modifiers, NamedKey, Offset, Rect, Size, TargetPlatform};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;
use vieww_say_codegen::line::{parse_widget_lines, LineProp, WidgetKindName};
use viewwstudio::buffer::{Buffer, Workspace};
use viewwstudio::command::Command;
use viewwstudio::compile::{compile, Session, Toolchain, TIMEOUT};
use viewwstudio::loaded::{LoadError, Preview};
use viewwstudio::state::{Handle, RightTab, Studio};
use viewwstudio::ui::preview::PreviewPane;
use viewwstudio::ui::shortcuts;

/// The counter, as the repository ships it: the `.say` file and the Rust the
/// Say compiler generates from it. One fixture, both sides of the pipeline.
const COUNTER_SAY: &str =
    include_str!("../../../crates/vieww-say-codegen/tests/fixtures/counter.say");

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod counter {
    include!("../../../crates/vieww-say-codegen/tests/fixtures/counter_gen.rs");
}

// ===================== the studio side ====================================

/// A studio whose one open buffer is `counter.say`, read from disk the way a
/// person's is — through `Buffer::open`, so the language is detected rather
/// than asserted, and `canvas_edit` has a real Say buffer to refuse and accept.
fn say_studio(tag: &str) -> Studio {
    let dir = std::env::temp_dir().join(format!("canvas-edit-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temp directory");
    let path = dir.join("counter.say");
    std::fs::write(&path, COUNTER_SAY).expect("the fixture on disk");
    let buffer = Buffer::open(&path).expect("the fixture opens");
    let runtime = Runtime::new();
    let workspace = Workspace::of_buffers(vec![buffer]);
    Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux)
}

/// The same studio with the counter's generated screen mounted in the real
/// preview pane, one frame drawn, and the tree captured — exactly the state
/// the preview is in when a person picks a widget.
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

/// The line a nudge rewrote, as the Say parser reads it back — the value the
/// file actually carries, not the value the handle meant to write.
fn props_of_line(studio: &Studio, line: u32) -> Vec<LineProp> {
    let text = &studio.active().expect("a buffer").value.text;
    parse_widget_lines(text)
        .into_iter()
        .find(|widget| widget.line == line)
        .unwrap_or_else(|| panic!("a widget on line {line}"))
        .props
}

// ===================== the drag ===========================================

#[test]
fn a_padding_drag_rewrites_the_say_line() {
    let studio = say_studio("drag");
    let mut driver = mounted(&studio);
    let row = pick_row(&studio, &mut driver, "RenderColumn");

    // The command, not the signal: arming canvas editing is a real user path
    // through the command dispatcher, with the Inspector it depends on.
    studio.run(Command::ToggleCanvasEdit);
    assert!(studio.canvas_edit.peek(), "the command armed the mode");

    // The picked row maps to the line that drew it: the `a column` on line 8.
    let widget = studio.picked_say_line().expect("the column maps to a line");
    assert_eq!(widget.line, 8, "the line the render tree grew from");
    assert_eq!(widget.kind, WidgetKindName::Column);

    // A column takes all four handles — spacing is a row's and a column's.
    let handles = studio.canvas_handles().expect("handles for the column");
    let labels: Vec<&str> = handles.iter().map(|(h, _)| h.label()).collect();
    assert_eq!(labels, ["padding", "spacing", "width", "height"]);
    let picked = studio.picked_row_rect().expect("the column's rectangle");
    let row_rect = {
        let rows = studio.tree_snapshot.borrow();
        let (depth, node) = &rows[row];
        let mut origin = Offset::ZERO;
        let mut at = row;
        while let Some(parent) = viewwstudio::outline::parent(&rows[..], at) {
            origin.dx += rows[parent].1.offset.dx;
            origin.dy += rows[parent].1.offset.dy;
            at = parent;
        }
        let _ = depth;
        Rect::from_origin_size(origin + node.offset, node.size)
    };
    assert_eq!(
        (picked.left, picked.top, picked.right, picked.bottom),
        (row_rect.left, row_rect.top, row_rect.right, row_rect.bottom),
        "the handles annotate the row the Inspector shows"
    );

    // The drag itself: eight points in on both axes.
    studio.canvas_drag_start(Handle::Padded);
    studio.canvas_drag_update(8.0, 8.0);
    studio.canvas_drag_end();

    // The buffer: one line rewritten, every other line byte-identical, the
    // line count unchanged — the `// say:` line numbers stay stable.
    let text = studio.active().expect("a buffer").value.text.clone();
    let before: Vec<&str> = COUNTER_SAY.lines().collect();
    let after: Vec<&str> = text.lines().collect();
    assert_eq!(after.len(), before.len(), "no line was added or removed");
    for (index, (old, new)) in before.iter().zip(&after).enumerate() {
        if index == 7 {
            assert_eq!(
                *new, "    a column, spaced 16, children aligned to the start, padded 8:",
                "the eighth line, and only it, was rewritten"
            );
        } else {
            assert_eq!(old, new, "line {} is untouched", index + 1);
        }
    }
    assert!(
        studio.active().expect("a buffer").dirty,
        "the buffer is edited, not merely modified in memory"
    );

    // The rewritten file still compiles, through the real Say compiler.
    let generated =
        vieww_say_codegen::compile("counter.say", &text).expect("the rewritten file compiles");
    assert!(
        generated.warnings.is_empty(),
        "and without picking up warnings: {:?}",
        generated.warnings
    );

    // A text line refuses the spacing handle: the parser would reject it, so
    // the canvas never offers it.
    let heading = pick_row(&studio, &mut driver, "RenderText");
    let _ = heading;
    studio.capture_tree(&mut driver);
    let heading_row = studio
        .tree_snapshot
        .borrow()
        .iter()
        .position(|(_, node)| {
            node.name == "RenderText"
                && node
                    .properties
                    .iter()
                    .any(|(key, value)| *key == "text" && value.contains("Counter"))
        })
        .expect("the heading's row");
    studio.inspect_selected.set(Some(heading_row));
    let widget = studio
        .picked_say_line()
        .expect("the heading maps to a line");
    assert_eq!(widget.line, 9, "the `a heading` on line 9");
    let handles = studio.canvas_handles().expect("handles for the heading");
    let labels: Vec<&str> = handles.iter().map(|(h, _)| h.label()).collect();
    assert_eq!(labels, ["padding", "width", "height"], "no spacing handle");
    assert!(!Handle::Spaced.accepted_by(&widget));

    // The button's own rows inherit the enclosing column's line — a
    // composite line the matcher cannot see is read as its parent, which is
    // the nearest true statement about who drew the box.
    let button_row = studio
        .tree_snapshot
        .borrow()
        .iter()
        .position(|(_, node)| {
            node.name == "RenderText"
                && node
                    .properties
                    .iter()
                    .any(|(key, value)| *key == "text" && value.contains("Add one"))
        })
        .expect("the button's label row");
    studio.inspect_selected.set(Some(button_row));
    let widget = studio
        .picked_say_line()
        .expect("the button's row maps to a line");
    assert_eq!(widget.line, 8, "the enclosing column, never a wrong line");
}

// ===================== the keyboard =======================================

/// An arrow key going down, as the render object would hand it to the
/// shortcut layer.
fn arrow(key: NamedKey, modifiers: Modifiers) -> KeyEvent {
    KeyEvent::named(key, Duration::ZERO).with_modifiers(modifiers)
}

#[test]
fn arrow_keys_nudge_only_while_the_mode_is_armed() {
    let studio = say_studio("keys");
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderColumn");

    // Off: the canvas does not own the arrows. Alt, so the Inspector's own
    // arrow handling (empty modifiers only) cannot fire either — what is
    // being refused is the canvas, not the shortcut table.
    let untouched = studio.active().expect("a buffer").value.text.clone();
    assert!(
        !shortcuts::dispatch(
            &studio,
            &arrow(NamedKey::ArrowRight, Modifiers::ALT),
            studio.host
        ) || studio.active().expect("a buffer").value.text == untouched,
        "with the mode off no arrow rewrites the buffer"
    );
    assert_eq!(
        studio.active().expect("a buffer").value.text,
        untouched,
        "the buffer is untouched"
    );

    // Re-pin the pick (the unarmed arrows may have moved the Inspector's
    // outline selection), then arm through the real command.
    pick_row(&studio, &mut driver, "RenderColumn");
    studio.run(Command::ToggleCanvasEdit);

    // Width follows the x axis: one point, then ten with Shift.
    assert!(shortcuts::dispatch(
        &studio,
        &arrow(NamedKey::ArrowRight, Modifiers::NONE),
        studio.host
    ));
    assert!(shortcuts::dispatch(
        &studio,
        &arrow(NamedKey::ArrowRight, Modifiers::SHIFT),
        studio.host
    ));
    // Alt moves to padding and spacing: left grows padding, up grows spacing.
    assert!(shortcuts::dispatch(
        &studio,
        &arrow(NamedKey::ArrowRight, Modifiers::ALT),
        studio.host
    ));
    assert!(shortcuts::dispatch(
        &studio,
        &arrow(NamedKey::ArrowUp, Modifiers::ALT),
        studio.host
    ));

    // What the line now carries, read back through the real parser.
    let props = props_of_line(&studio, 8);
    let expect = [
        LineProp::Spaced(17.0),
        LineProp::ChildrenAligned("the start".to_owned()),
        LineProp::Wide(11.0),
        LineProp::Padded(1.0),
    ];
    assert_eq!(props, expect, "16+1 spacing, 1+10 width, 1 padding");

    // Clamped at zero: a nudge past the floor stays at the floor, because
    // `padded -7` is not a line the next Render would accept.
    for _ in 0..3 {
        assert!(shortcuts::dispatch(
            &studio,
            &arrow(NamedKey::ArrowLeft, Modifiers::ALT),
            studio.host
        ));
    }
    let props = props_of_line(&studio, 8);
    assert!(
        props.contains(&LineProp::Padded(0.0)),
        "padding clamped at zero, not negative: {props:?}"
    );

    // Every other line is still byte-identical, and the file still compiles.
    let text = studio.active().expect("a buffer").value.text.clone();
    let before: Vec<&str> = COUNTER_SAY.lines().collect();
    let after: Vec<&str> = text.lines().collect();
    assert_eq!(after.len(), before.len());
    for (index, (old, new)) in before.iter().zip(&after).enumerate() {
        if index != 7 {
            assert_eq!(old, new, "line {} is untouched", index + 1);
        }
    }
    vieww_say_codegen::compile("counter.say", &text).expect("the nudged file compiles");
}

// ===================== the pixels =========================================

/// `target/debug` of this workspace, where `libvieww.rlib` lands — the same
/// discovery the studio itself performs.
fn target() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary");
    exe.parent()
        .and_then(Path::parent)
        .expect("target/debug")
        .to_path_buf()
}

/// The smallest screen that compiles, for probing which `vieww` candidate
/// this test binary actually shares types with — copied from `pipeline.rs`,
/// whose comment on why the first candidate is not necessarily it applies
/// verbatim here.
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
        let session = Session::new(0xC0DE).ok()?;
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

fn toolchain() -> Option<Toolchain> {
    let base = match Toolchain::discover(&target()) {
        Ok(toolchain) => toolchain,
        Err(error) => {
            eprintln!("skipping: {error}");
            return None;
        }
    };
    let Some(candidate) = matching_candidate(&base) else {
        eprintln!(
            "skipping: no `vieww` rlib in {} matches this test binary's own \
             compilation — build the workspace the way this binary was built",
            target().display()
        );
        return None;
    };
    Some(base.with_candidate(candidate).expect("in range"))
}

/// One render row with its rectangle in window coordinates: the accumulated
/// offsets of the pre-order walk, the same arithmetic the Inspector's own
/// outline performs.
struct Row {
    name: String,
    rect: Rect,
    text: Option<String>,
}

fn rows(driver: &FrameDriver) -> Vec<Row> {
    let tree = driver.renders();
    let root = tree.root().expect("a root");
    let described = tree.describe_subtree(root, 100_000);
    let mut origins = vec![Offset::ZERO; described.len()];
    let mut out = Vec::with_capacity(described.len());
    for index in 0..described.len() {
        let (depth, node) = &described[index];
        let parent = (0..index).rev().find(|p| described[*p].0 + 1 == *depth);
        let base = parent.map_or(Offset::ZERO, |p| origins[p]);
        origins[index] = Offset::new(base.dx + node.offset.dx, base.dy + node.offset.dy);
        let text = node
            .properties
            .iter()
            .find(|(key, _)| *key == "text")
            .map(|(_, value)| value.trim_matches('"').to_owned());
        out.push(Row {
            name: node.name.to_owned(),
            rect: Rect::from_origin_size(origins[index], node.size),
            text,
        });
    }
    out
}

/// Compile and load one `.say` text, and render one frame of it: the whole
/// studio Render pipeline minus the studio, which the two tree tests above
/// already cover.
fn rendered(toolchain: &Toolchain, session: &Session, say: &str) -> FrameDriver {
    let generated = vieww_say_codegen::compile("counter.say", say).expect("the say compiles");
    let compiled = compile(toolchain, session, &generated.rust, "counter.rs").expect("rustc ran");
    assert!(
        !compiled.failed(),
        "the generated rust compiles: {:?}",
        compiled.diagnostics
    );
    assert!(compiled.duration < TIMEOUT);
    let library = compiled.library.as_ref().expect("a library");
    let preview = Preview::load(library).expect("it loads");
    let mut driver = FrameDriver::new(Size::new(400.0, 700.0));
    viewwstudio::install(&mut driver);
    driver
        .set_root(vieww_widget::Theme::new(vieww_widget::ThemeData::light()).child(preview.node()));
    driver.draw_frame();
    driver
}

/// The bounding box of the pixels that differ between two same-size
/// rasterisations, as `(left, top, right, bottom)` in pixel coordinates.
fn changed_bbox(before: &vieww_paint::native::Pixels, after: &vieww_paint::native::Pixels) -> Rect {
    assert_eq!(before.width(), after.width());
    assert_eq!(before.height(), after.height());
    let (left, top, right, bottom) = (before.width(), before.height(), 0u32, 0u32);
    let mut bbox = (left, top, right, bottom);
    let mut changed = false;
    for y in 0..before.height() {
        for x in 0..before.width() {
            let a = before.pixel(x, y);
            let b = after.pixel(x, y);
            if a.r != b.r || a.g != b.g || a.b != b.b || a.a != b.a {
                if !changed {
                    bbox = (x, y, x, y);
                    changed = true;
                } else {
                    bbox.0 = bbox.0.min(x);
                    bbox.1 = bbox.1.min(y);
                    bbox.2 = bbox.2.max(x);
                    bbox.3 = bbox.3.max(y);
                }
            }
        }
    }
    assert!(
        changed,
        "the two renders are pixel-identical — nothing moved"
    );
    Rect::new(
        bbox.0 as f32,
        bbox.1 as f32,
        bbox.2 as f32 + 1.0,
        bbox.3 as f32 + 1.0,
    )
}

#[test]
fn the_preview_png_shifts_by_exactly_the_delta() {
    let Some(toolchain) = toolchain() else {
        return;
    };

    // The edit a spacing drag makes: through the studio, so what reaches
    // rustc is what a person's drag would have written.
    let studio = say_studio("png");
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderColumn");
    studio.run(Command::ToggleCanvasEdit);
    studio.canvas_drag_start(Handle::Spaced);
    studio.canvas_drag_update(0.0, 8.0);
    studio.canvas_drag_end();
    let after_say = studio.active().expect("a buffer").value.text.clone();
    assert!(
        after_say.lines().nth(7).unwrap().contains("spaced 24"),
        "the drag wrote spacing 24: {}",
        after_say.lines().nth(7).unwrap()
    );

    let before_session = Session::new(0xD1CE).expect("a temp directory");
    let after_session = Session::new(0xD2CE).expect("a temp directory");
    let before = rendered(&toolchain, &before_session, COUNTER_SAY);
    let after = rendered(&toolchain, &after_session, &after_say);

    // Every row either holds still or moves down by a whole multiple of the
    // delta — the tree first, because a tree that lies is what the pixels are
    // about to check. Spacing accumulates down a column: the first child
    // holds still, the second moves by the delta, the third by twice it, and
    // so on. A move by anything else — 6, 12, a fraction — is the failure
    // mode this test exists to catch.
    let (rows_before, rows_after) = (rows(&before), rows(&after));
    assert_eq!(
        rows_before.len(),
        rows_after.len(),
        "spacing changes offsets, not structure"
    );
    let mut union = None::<Rect>;
    let mut moved = 0usize;
    for (a, b) in rows_before.iter().zip(&rows_after) {
        assert_eq!(a.name, b.name, "the same widgets in the same order");
        let (dx, dy) = (b.rect.left - a.rect.left, b.rect.top - a.rect.top);
        let steps = dy / 8.0;
        assert!(
            dx.abs() < 0.01 && dy > -0.01 && (steps - steps.round()).abs() < 0.01,
            "{} moved by ({:.2}, {:.2}), not horizontally still and down a \
             whole multiple of 8",
            a.name,
            dx,
            dy
        );
        if dy > 0.01 {
            moved += 1;
            let span = a.rect.union(b.rect);
            union = Some(match union {
                None => span,
                Some(so_far) => so_far.union(span),
            });
        }
    }
    let label_before = rows_before
        .iter()
        .find(|r| r.text.as_deref().is_some_and(|t| t.contains("Tapped")))
        .expect("the label row");
    let label_after = rows_after
        .iter()
        .find(|r| r.text.as_deref().is_some_and(|t| t.contains("Tapped")))
        .expect("the label row");
    assert_eq!(
        (
            label_after.rect.left - label_before.rect.left,
            label_after.rect.top - label_before.rect.top
        ),
        (0.0, 8.0),
        "the label moved by exactly the drag"
    );
    let heading_before = rows_before
        .iter()
        .find(|r| r.text.as_deref() == Some("Counter"))
        .expect("the heading row");
    let heading_after = rows_after
        .iter()
        .find(|r| r.text.as_deref() == Some("Counter"))
        .expect("the heading row");
    assert_eq!(
        (
            heading_after.rect.left - heading_before.rect.left,
            heading_after.rect.top - heading_before.rect.top
        ),
        (0.0, 0.0),
        "the first child holds still: spacing is between children"
    );
    let button_before = rows_before
        .iter()
        .find(|r| r.text.as_deref() == Some("Add one"))
        .expect("the button's label row");
    let button_after = rows_after
        .iter()
        .find(|r| r.text.as_deref() == Some("Add one"))
        .expect("the button's label row");
    assert_eq!(
        (
            button_after.rect.left - button_before.rect.left,
            button_after.rect.top - button_before.rect.top
        ),
        (0.0, 16.0),
        "the third child moved by the delta twice: spacing accumulates"
    );
    assert!(moved >= 2, "the label and the button both moved: {moved}");

    // The pixels: rasterise both, require the PNGs to differ, and require
    // every changed pixel to lie inside where the moved rows were or went —
    // a pixel that changed outside them would be a move nobody wrote. The
    // margin is one pixel of antialias at the boxes' edges.
    let mut renderer = NativeRenderer::new();
    let background = Color::rgb(0xFF, 0xFF, 0xFF);
    let (png_before, _) = renderer
        .render_to_png(before.scene(), 400, 700, background)
        .expect("rasterised");
    let (png_after, _) = renderer
        .render_to_png(after.scene(), 400, 700, background)
        .expect("rasterised");
    assert_ne!(
        png_before, png_after,
        "the picture changed when the buffer did"
    );
    let (pixels_before, _) = renderer
        .render_to_pixels(before.scene(), 400, 700, background)
        .expect("rasterised");
    let (pixels_after, _) = renderer
        .render_to_pixels(after.scene(), 400, 700, background)
        .expect("rasterised");
    let bbox = changed_bbox(&pixels_before, &pixels_after);
    let expected = union.expect("moved rows to compare against").inflate(1.0);
    assert!(
        contains_rect(expected, bbox),
        "the changed pixels lie inside the moved rows' span\n  changed: \
         {:.1}..{:.1} x {:.1}..{:.1}\n  expected: {:.1}..{:.1} x {:.1}..{:.1}",
        bbox.left,
        bbox.right,
        bbox.top,
        bbox.bottom,
        expected.left,
        expected.right,
        expected.top,
        expected.bottom
    );
    assert!(
        bbox.top + 1.0 >= label_before.rect.top - 1.0,
        "the change starts where the label was: {:.1} vs {:.1}",
        bbox.top,
        label_before.rect.top
    );
}

// Rect containment — `Rect::union` and `Rect::inflate` are foundation
// methods; this file's one remaining rectangle helper is the containment
// check the pixel comparison reads better with, kept here because this file
// is its only caller.
fn contains_rect(outer: Rect, inner: Rect) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}
