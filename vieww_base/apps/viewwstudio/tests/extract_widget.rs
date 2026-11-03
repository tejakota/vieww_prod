//! ST-05 end to end: the picked subtree becomes a component, and the
//! preview does not notice.
//!
//! # What this file proves
//!
//! The unit tests in `components.rs` prove the pieces — the span, the
//! replacement, the auto-name, the registry round-trip. This file proves the
//! verbs as a person reaches them, and the one property the whole item
//! stands on:
//!
//! * [`extraction_rewrites_the_buffer_and_wires_the_project`] — the command,
//!   through the real dispatcher, with a pick made the real way (a captured
//!   render tree, a selected row), rewrites the span into `<Card>`, writes
//!   `src/widgets/card.rs` with `pub struct Card` and `impl Widget`, wires
//!   `mod.rs` and `lib.rs`, and takes the subtree into the registry.
//! * [`an_extracted_component_previews_through_the_project`] — the same
//!   extraction, then the two halves of the pipeline the studio runs: the
//!   screen before (the subtree inline, compiled standalone) and the screen
//!   after (the reference, compiled against the project's own rlib exactly
//!   as `Render` links it). The two PNGs are byte-identical — extraction is
//!   invisible where it matters, which is the pixels.
//! * [`a_registry_component_scaffolds_into_another_project`] — the second
//!   verb: a registry component written into a second project, with the
//!   `<Name>` reference placed where the caret is.
//! * [`an_impure_subtree_is_refused_and_nothing_is_written`] — the counter's
//!   column reads the screen's state, and the refusal says so before any
//!   file is touched.
//!
//! # Why the project's rlib is built with `rustc`, not `cargo`
//!
//! The property under test is the *linking*: a `<Name>` reference compiles
//! to `crate::widgets::card::Card`, which the preview rewrites to
//! `fixture::widgets::card::Card` and resolves against the project's rlib —
//! the same `--extern` `Job::spawn` passes. A `cargo build` of the fixture
//! project would add nothing to that property and would cost a second
//! compilation of the whole framework into the sandbox. So the rlib is
//! built by the same `rustc` the studio's own toolchain found, against the
//! same `vieww` rlib the preview links — the one arrangement
//! `ProjectLib::discover` documents as the one that works — and found again
//! through the real `ProjectLib::discover`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use vieww_element::Runtime;
use vieww_foundation::{Color, Size, TargetPlatform};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;
use viewwstudio::buffer::{Buffer, Workspace};
use viewwstudio::command::Command;
use viewwstudio::compile::{compile, compile_project, ProjectLib, Session, Toolchain};
use viewwstudio::components::Component;
use viewwstudio::loaded::{LoadError, Preview};
use viewwstudio::state::{RightTab, Studio};
use viewwstudio::ui::preview::PreviewPane;

/// The fixture screen, as the repository ships it: one pure card subtree
/// between two siblings — line 6 is the extractable one.
const HOME_SAY: &str =
    include_str!("../../../crates/vieww-say-codegen/tests/fixtures/component_home.say");

/// The same screen, pre-generated — mounting it is how a pick is made the
/// real way, off a really-rendered tree, without a `rustc` in every test.
#[allow(dead_code, clippy::all, clippy::pedantic)]
mod home {
    include!("../../../crates/vieww-say-codegen/tests/fixtures/component_home_gen.rs");
}

/// The counter, for the refusal: its column interpolates `count`, so the
/// subtree cannot stand alone.
const COUNTER_SAY: &str =
    include_str!("../../../crates/vieww-say-codegen/tests/fixtures/counter.say");

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod counter {
    include!("../../../crates/vieww-say-codegen/tests/fixtures/counter_gen.rs");
}

/// `target/debug` of this workspace — see `tests/pipeline.rs`.
fn target() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary");
    exe.parent()
        .and_then(Path::parent)
        .expect("target/debug")
        .to_path_buf()
}

// ===================== the toolchain =======================================

fn matching_candidate(base: &Toolchain) -> Option<usize> {
    static RESOLVED: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *RESOLVED.get_or_init(|| {
        let session = Session::new(0x5C50).ok()?;
        let probe = r#"
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
        for candidate in 0.. {
            let toolchain = base.with_candidate(candidate)?;
            let Ok(compiled) = compile(&toolchain, &session, probe, "probe.rs") else {
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
            "skipping: no `vieww` rlib in {} matches this test binary's own compilation",
            target().display()
        );
        return None;
    };
    base.with_candidate(candidate)
}

// ===================== the fixture project =================================

/// Point the studio's data directory — the one the registry lives under — at
/// a scratch directory, once per process, so this file's extractions are not
/// written into the developer's real `~/.local/share`.
///
/// A mutex rather than `OnceLock`, because the check-then-set has to be one
/// act: two tests arriving together must not both call `set_var`. After the
/// first set the variable never changes again, so later readers — the
/// studio's session recovery included — race with nothing.
fn data_scratch() {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if std::env::var_os("XDG_DATA_HOME").is_none() {
        let dir = std::env::temp_dir().join(format!("extract-widget-data-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch data directory");
        std::env::set_var("XDG_DATA_HOME", dir);
    }
}

/// A fixture project on disk: `Cargo.toml`, `src/lib.rs` **without** `pub mod
/// widgets` (extraction adds it — that is part of what is proved), and the
/// component fixture as `src/screens/home.say`.
fn fixture_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("extract-widget-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/screens")).expect("the project tree");
    // A real dependency path, so the manifest is not a prop: the `vieww` the
    // studio's own toolchain links is the one this project declares.
    let target = target();
    let vieww_base = target
        .parent()
        .expect("target")
        .parent()
        .expect("vieww_base");
    let vieww = vieww_base
        .join("crates")
        .join("vieww")
        .to_string_lossy()
        .replace('\\', "/");
    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nvieww = {{ path = \"{vieww}\" }}\n"
        ),
    )
    .expect("Cargo.toml");
    std::fs::write(
        root.join("src/lib.rs"),
        "//! The fixture project's library.\n",
    )
    .expect("lib.rs");
    std::fs::write(root.join("src/screens/home.say"), HOME_SAY).expect("home.say");
    root
}

/// Serialises the two tests that extract: both write the registry's `Card`
/// entry, and a read that catches a concurrent write half-done would be a
/// flake about the filesystem, not about extraction.
static EXTRACT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A studio whose one open buffer is the fixture's `home.say`, with the
/// workspace root pointing at the project.
fn studio_over(root: &Path) -> Studio {
    let buffer = Buffer::open(&root.join("src/screens/home.say")).expect("the screen opens");
    let runtime = Runtime::new();
    let workspace = Workspace::of_buffers(vec![buffer]);
    let studio = Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux);
    studio.root.set(Some(root.to_path_buf()));
    studio
}

/// The studio with the fixture screen mounted in the real preview pane, one
/// frame drawn — the state the preview is in when a person picks a widget.
fn mounted(studio: &Studio) -> FrameDriver {
    let mut driver = FrameDriver::new(Size::new(900.0, 900.0));
    studio
        .preview_screen
        .set(Some(Preview::from_widget(home::screen())));
    driver.set_root(PreviewPane {
        studio: studio.clone(),
    });
    driver.draw_frame();
    driver
}

/// Capture the guest tree with the Inspector open, and pick the first row
/// whose render object is `name` — the row a click on that widget would pick.
fn pick_row(studio: &Studio, driver: &mut FrameDriver, name: &str) {
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
}

// ===================== the first verb ======================================

/// The command, end to end: the pick is real, the span is the subtree's, the
/// buffer keeps every other line byte-identical, the project gains the
/// component file and its wiring, and the registry takes the subtree.
#[test]
fn extraction_rewrites_the_buffer_and_wires_the_project() {
    data_scratch();
    let _extract = EXTRACT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = fixture_project("extract");
    let studio = studio_over(&root);
    let mut driver = mounted(&studio);

    // The pick, the real way: the card is the one `RenderDecoratedBox`.
    pick_row(&studio, &mut driver, "RenderDecoratedBox");
    let picked = studio.picked_say_line().expect("the card maps to a line");
    assert_eq!(picked.line, 6, "the `a card` on line 6");
    assert_eq!(picked.kind, vieww_say_codegen::line::WidgetKindName::Card);

    studio.run(Command::ExtractWidget);
    let notice = studio.notice.get().expect("the command said what it did");
    assert!(
        notice.contains("Extracted Card into src/widgets/card.rs"),
        "the notice names the file: {notice}"
    );

    // The buffer: line 6 became the reference, indented as it was, and every
    // other line is byte-identical — the `// say:` line numbers stay stable.
    let buffer = studio.active().expect("the buffer is still open");
    let before: Vec<&str> = HOME_SAY.lines().collect();
    let after: Vec<&str> = buffer.value.text.lines().collect();
    assert_eq!(after.len(), before.len() - 2, "three lines became one");
    for (index, line) in after.iter().enumerate() {
        let expected = match index {
            5 => "        <Card>",
            6 => before[8],
            _ => before[index],
        };
        assert_eq!(*line, expected, "line {} of the rewritten buffer", index + 1);
    }
    assert!(buffer.dirty, "the buffer is edited, not merely modified in memory");
    assert!(
        studio.active().expect("the buffer").value.text.contains("        <Card>\n"),
        "the reference carries the span's own indentation"
    );

    // The rewritten screen is still Say the compiler accepts — `<Card>` and
    // all.
    let generated = vieww_say_codegen::compile("home.say", &buffer.value.text)
        .expect("the reference is valid Say");
    assert!(
        generated.rust.contains("crate::widgets::card::Card"),
        "the reference reaches the project's module: {}",
        generated.rust
    );
    assert!(
        generated.rust.contains("// say: home.say:6  <Card>"),
        "the marker shows the reference as written"
    );

    // The project: the component file, the module list, the library's `pub mod`.
    let widget = std::fs::read_to_string(root.join("src/widgets/card.rs"))
        .expect("the component file was written");
    assert!(widget.contains("pub struct Card;"), "{widget}");
    assert!(widget.contains("impl Widget for Card"), "{widget}");
    assert!(widget.contains("use vieww::prelude::*;"), "{widget}");
    let modules = std::fs::read_to_string(root.join("src/widgets/mod.rs"))
        .expect("mod.rs was written");
    assert!(modules.contains("pub mod card;"), "{modules}");
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("lib.rs still there");
    assert!(lib.contains("pub mod widgets;"), "extraction wired the library: {lib}");

    // The registry: the subtree, dedented, with its origin recorded.
    let components = studio.registry_components();
    let card = components
        .iter()
        .find(|component| component.name == "Card")
        .expect("the extraction reached the registry");
    assert_eq!(card.from, "home.say");
    assert!(
        card.source.starts_with("a card, padded 12"),
        "the registry holds the dedented subtree: {:?}",
        card.source
    );
    assert!(
        card.source.contains("a text \"Extract me\", color #3366FF"),
        "the whole subtree, not just its head: {:?}",
        card.source
    );

    std::fs::remove_dir_all(&root).ok();
}

/// The property the whole item stands on: the screen before (the subtree
/// inline, compiled standalone) and the screen after (the reference,
/// compiled against the project's own rlib the way `Render` links it)
/// produce the same picture, byte for byte.
#[test]
fn an_extracted_component_previews_through_the_project() {
    let Some(toolchain) = toolchain() else {
        return;
    };
    data_scratch();
    let _extract = EXTRACT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = fixture_project("preview");
    let studio = studio_over(&root);
    let mut driver = mounted(&studio);
    pick_row(&studio, &mut driver, "RenderDecoratedBox");
    studio.run(Command::ExtractWidget);
    let after_say = studio.active().expect("the buffer").value.text.clone();
    assert!(after_say.contains("<Card>"), "the extraction happened");

    // The screen before: the subtree inline, compiled standalone — no
    // project, exactly what `render_preview` does for a componentless Say
    // screen.
    let before_generated =
        vieww_say_codegen::compile("home.say", HOME_SAY).expect("the fixture compiles");
    let before_session = Session::new(0x5C05).expect("a temp directory");
    let before_compiled =
        compile(&toolchain, &before_session, &before_generated.rust, "home.rs").expect("rustc ran");
    assert!(
        !before_compiled.failed(),
        "the inline screen compiles: {:?}",
        before_compiled.diagnostics
    );

    // The project's library, built by the toolchain the studio itself found:
    // `--crate-type rlib` against the same `vieww` the preview links, which
    // is the arrangement `ProjectLib::discover` documents as the one that
    // works (see this file's header for why not `cargo`).
    let deps = root.join("target").join("debug").join("deps");
    std::fs::create_dir_all(&deps).expect("the project's deps directory");
    let rlib = deps.join("libfixture.rlib");
    let built = std::process::Command::new(&toolchain.rustc)
        .args(["--edition", "2021", "--crate-type", "rlib", "--crate-name", "fixture"])
        .arg(root.join("src/lib.rs"))
        .arg("--extern")
        .arg(format!("vieww={}", toolchain.vieww_rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", toolchain.deps.display()))
        .arg("-o")
        .arg(&rlib)
        .output()
        .expect("rustc ran");
    assert!(
        built.status.success(),
        "the project's library built: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let project = ProjectLib::discover(&root, "fixture", &toolchain.deps)
        .expect("the studio finds the library it built");

    // The screen after: the reference, linked against that rlib through the
    // same rewrite and the same `--extern` `Job::spawn` passes.
    let after_generated =
        vieww_say_codegen::compile("home.say", &after_say).expect("the reference compiles");
    assert!(
        after_generated.rust.contains("crate::widgets::card::Card"),
        "the reference reaches the project's module"
    );
    let after_session = Session::new(0x5C06).expect("a temp directory");
    let after_compiled = compile_project(
        &toolchain,
        &after_session,
        &after_generated.rust,
        "home.rs",
        &project,
        true,
    )
    .expect("rustc ran");
    assert!(
        !after_compiled.failed(),
        "the screen with the reference compiles against the project: {:?}",
        after_compiled.diagnostics
    );

    // Both load, both render, and the pictures are byte-identical: the
    // component builds exactly what the subtree built inline, so the
    // extraction is invisible where it matters.
    let before_preview = Preview::load(before_compiled.library.as_ref().expect("a library"))
        .expect("the inline screen loads");
    let after_preview = Preview::load(after_compiled.library.as_ref().expect("a library"))
        .expect("the referenced screen loads");
    let png_of = |preview: Preview| -> Vec<u8> {
        let mut driver = FrameDriver::new(Size::new(400.0, 700.0));
        viewwstudio::install(&mut driver);
        driver.set_root(
            vieww_widget::Theme::new(vieww_widget::ThemeData::light()).child(preview.node()),
        );
        driver.draw_frame_at(Duration::ZERO);
        let mut renderer = NativeRenderer::new();
        renderer
            .render_to_png(driver.scene(), 400, 700, Color::WHITE)
            .expect("the screen renders")
            .0
    };
    let png_before = png_of(before_preview);
    let png_after = png_of(after_preview);
    assert_eq!(
        png_before, png_after,
        "the preview did not change when the subtree became a component"
    );

    std::fs::remove_dir_all(&root).ok();
}

// ===================== the second verb =====================================

/// A registry component scaffolds into a *second* project: the Rust is
/// written, the modules wired, and `<Note>` lands where the caret is — as
/// its own line, indented as the caret's line is.
#[test]
fn a_registry_component_scaffolds_into_another_project() {
    data_scratch();
    let root = fixture_project("scaffold");
    // A second screen to place the component in, so the scaffold is not
    // writing into the extraction's own leftovers.
    let screen = "-- say-language: 1\nscreen \"Settings\":\n    a column:\n        a text \"v1\"\n";
    std::fs::write(root.join("src/screens/settings.say"), screen).expect("the second screen");
    let buffer =
        Buffer::open(&root.join("src/screens/settings.say")).expect("the screen opens");
    let runtime = Runtime::new();
    let workspace = Workspace::of_buffers(vec![buffer]);
    let studio = Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux);
    studio.root.set(Some(root.to_path_buf()));

    // The caret at the end of the column's line: the reference goes under
    // it, as its own line.
    let at = screen.find("a column:").expect("the line") + "a column:".len();
    let mut value = vieww_foundation::TextEditingValue::new(screen.to_owned());
    value.selection = vieww_foundation::TextSelection::collapsed(at);
    studio.edit(value);

    studio.scaffold_component(&Component {
        name: "Note".to_owned(),
        source: "a card, padded 12:\n    a text \"hi\"\n".to_owned(),
        from: "home.say".to_owned(),
    });
    let notice = studio.notice.get().expect("the scaffold said what it did");
    assert!(
        notice.contains("Placed <Note>"),
        "the notice names the reference: {notice}"
    );

    // The project gained the component, wired as an extraction wires it.
    let widget = std::fs::read_to_string(root.join("src/widgets/note.rs"))
        .expect("the component file was written");
    assert!(widget.contains("pub struct Note;"), "{widget}");
    assert!(widget.contains("impl Widget for Note"), "{widget}");
    assert!(
        std::fs::read_to_string(root.join("src/widgets/mod.rs"))
            .expect("mod.rs")
            .contains("pub mod note;")
    );
    assert!(
        std::fs::read_to_string(root.join("src/lib.rs"))
            .expect("lib.rs")
            .contains("pub mod widgets;")
    );

    // The buffer gained the reference, on its own line — and the result is
    // Say the compiler accepts.
    let text = studio.active().expect("the buffer").value.text.clone();
    assert!(
        text.contains("    a column:\n        <Note>\n"),
        "the reference went inside the caret's container, at its children's indent: {text:?}"
    );
    vieww_say_codegen::compile("settings.say", &text)
        .expect("the scaffolded screen is valid Say");

    // And a second scaffold into the same project places another reference
    // without touching the component the project now owns: the notice ends
    // on "Placed", and the file on disk is the user's edit, not a rewrite.
    std::fs::write(root.join("src/widgets/note.rs"), "// the user's edit\n").expect("the edit");
    studio.scaffold_component(&Component {
        name: "Note".to_owned(),
        source: "a card, padded 12:\n    a text \"hi\"\n".to_owned(),
        from: "home.say".to_owned(),
    });
    let notice = studio.notice.get().expect("the second scaffold said something");
    assert!(
        notice.contains("Placed <Note>"),
        "the second scaffold placed a reference too: {notice}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/widgets/note.rs")).expect("the file"),
        "// the user's edit\n",
        "an existing component is never overwritten"
    );
    let text = studio.active().expect("the buffer").value.text.clone();
    assert_eq!(text.matches("<Note>").count(), 2, "two references, one component: {text}");

    std::fs::remove_dir_all(&root).ok();
}

// ===================== the refusal =========================================

/// The counter's column reads the screen's `count`, so it cannot stand
/// alone — and the refusal happens before a single file is written.
#[test]
fn an_impure_subtree_is_refused_and_nothing_is_written() {
    data_scratch();
    let root = fixture_project("refuse");
    std::fs::write(root.join("src/screens/home.say"), COUNTER_SAY).expect("the counter fixture");
    let buffer = Buffer::open(&root.join("src/screens/home.say")).expect("the fixture opens");
    let runtime = Runtime::new();
    let workspace = Workspace::of_buffers(vec![buffer]);
    let studio = Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux);
    studio.root.set(Some(root.to_path_buf()));

    // Mount the counter's own generated screen and pick its column.
    let mut driver = FrameDriver::new(Size::new(900.0, 900.0));
    studio
        .preview_screen
        .set(Some(Preview::from_widget(counter::screen())));
    driver.set_root(PreviewPane {
        studio: studio.clone(),
    });
    driver.draw_frame();
    pick_row(&studio, &mut driver, "RenderColumn");
    let picked = studio.picked_say_line().expect("the column maps to a line");
    assert_eq!(picked.line, 8, "the `a column` on line 8");

    studio.run(Command::ExtractWidget);
    let notice = studio.notice.get().expect("the refusal says why");
    assert!(
        notice.contains("E100") && notice.contains("cannot become a component"),
        "the refusal names the rule: {notice}"
    );

    // Nothing was written: no `src/widgets`, no registry entry, and the
    // buffer is the counter, untouched.
    assert!(
        !root.join("src/widgets").exists(),
        "a refused extraction writes no component"
    );
    assert_eq!(
        studio.active().expect("the buffer").value.text,
        COUNTER_SAY,
        "a refused extraction does not edit the buffer"
    );
    assert!(
        studio
            .registry_components()
            .iter()
            .all(|component| component.name != "Column"),
        "a refused extraction does not reach the registry"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// Without a folder there is nothing to extract into, and without a pick
/// there is nothing to extract — both said as notices, not as silent no-ops.
#[test]
fn extraction_needs_a_folder_and_a_pick() {
    data_scratch();
    let runtime = Runtime::new();
    let workspace = Workspace::scratch();
    let studio = Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux);
    assert!(studio.root.get().is_none());

    studio.run(Command::ExtractWidget);
    let notice = studio.notice.get().expect("a refusal");
    assert!(
        notice.contains("Open a folder first"),
        "the notice says what is missing: {notice}"
    );

    // A folder with no pick: the buffer is open, but nothing on the canvas
    // was ever selected.
    let root = fixture_project("nopoly");
    let studio = studio_over(&root);
    studio.run(Command::ExtractWidget);
    let notice = studio.notice.get().expect("a refusal");
    assert!(
        notice.contains("Pick a widget"),
        "the notice says what is missing: {notice}"
    );
    assert!(
        !root.join("src/widgets").exists(),
        "nothing was written without a pick"
    );

    std::fs::remove_dir_all(&root).ok();
}
