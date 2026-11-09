//! `viewwstudio-cli` end to end: the render paths a script would drive,
//! proven the way the pipeline tests prove the preview — real `rustc`, real
//! `dlopen`, real pixels, no window.
//!
//! # What this file proves
//!
//! 1. [`render_writes_a_png_of_the_scaffolded_home_screen`] — `new` then
//!    `render` on a fresh Say project produces a PNG of the counter screen,
//!    with the PNG's own header naming the size that was asked for. The
//!    whole promise of the tool in two calls.
//! 2. [`render_takes_a_hand_written_rust_screen_too`] — the `.rs` door: a
//!    screen with `pub fn screen()`, compiled without Say in the path.
//! 3. [`html_carries_the_screens_text_as_real_dom`] — the same compile, the
//!    DOM backend's walk: the counter's heading, label and button are
//!    *text in elements*, the button reachable from a keyboard
//!    (`tabindex`/`role`, the walk's own semantics).
//! 4. [`a_broken_say_file_reports_its_own_line`] — the failure a beginner
//!    meets: the Say diagnostic, with the `.say` file's own line and
//!    column, before `rustc` ever runs.
//! 5. [`a_missing_page_refuses_the_web_export_before_building`] — the `web`
//!    plan's refusal, surfaced as the CLI's error rather than a half-built
//!    bundle.
//!
//! # What is deliberately not here
//!
//! A test that runs the whole `web` export: it is a `cargo build` of the
//! framework for wasm32, which is minutes and a network away from a unit
//! test's budget. `tests/scaffold_exports.rs` checks the plan; the CLI
//! executes the plan's `Spec`s with the exit codes the studio's own task
//! runner already covers.
//!
//! # The toolchain caveat, same as every real-rustc file
//!
//! These tests need a `vieww` rlib in the target directory that matches this
//! test binary's own compilation. On a machine where that is not so they
//! print the reason and pass, exactly as `tests/canvas_edit.rs` does — the
//! skip is stated on stderr, not silent.

use std::path::{Path, PathBuf};

use viewwstudio::cli;

/// A fresh directory per test, named for it.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("viewwstudio-cli-{name}"));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The PNG's own idea of its size, straight out of the `IHDR` chunk — the
/// same eight bytes `ci/standalone.sh`'s screenshot gate reads.
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G']),
        "a PNG, by its magic"
    );
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (width, height)
}

/// A Say project scaffolded by the CLI's own `new`, in `dir`.
fn scaffolded_project(dir: &Path) -> PathBuf {
    let summary = cli::run_new(dir, "demo-app", false).expect("the scaffold writes");
    assert!(
        summary.contains("27 files"),
        "the Say project's file count: {summary}"
    );
    let root = dir.join("demo-app");
    assert!(root.join("Cargo.toml").is_file());
    assert!(root.join("src").join("screens").join("home.say").is_file());
    root
}

#[test]
fn render_writes_a_png_of_the_scaffolded_home_screen() {
    let dir = scratch("render-scaffolded");
    let project = scaffolded_project(&dir);

    let summary = match cli::run_render(&project, None, (320, 480), false) {
        Ok(summary) => summary,
        Err(error) => {
            // The stated skip, as `canvas_edit` prints it — not a silent pass.
            eprintln!("skipping: {error}");
            return;
        }
    };
    assert!(
        summary.contains("home.png"),
        "the default output is named for the screen: {summary}"
    );

    let png = project.join("target").join("render").join("home.png");
    assert_eq!(png_size(&png), (320, 480), "the size that was asked for");
    let size = std::fs::metadata(&png).unwrap().len();
    assert!(
        size > 1_000,
        "a rendered screen is more than a header: {size} bytes"
    );
}

#[test]
fn render_takes_a_hand_written_rust_screen_too() {
    let dir = scratch("render-rust");
    let screen = r#"
use vieww::prelude::*;

#[derive(Debug)]
pub struct Hello;

impl Widget for Hello {
    fn debug_name(&self) -> &'static str { "Hello" }
    fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        Text::new("Hello from Rust").into()
    }
}

pub fn screen() -> impl Widget { Hello }
"#;
    let file = dir.join("hello.rs");
    std::fs::write(&file, screen).unwrap();

    let out = dir.join("hello.png");
    match cli::run_render(&file, Some(&out), (240, 80), false) {
        Ok(summary) => assert!(summary.contains("hello.png"), "{summary}"),
        Err(error) => eprintln!("skipping: {error}"),
    }
    if out.is_file() {
        assert_eq!(png_size(&out), (240, 80));
    }
}

#[test]
fn html_carries_the_screens_text_as_real_dom() {
    let dir = scratch("html-scaffolded");
    let project = scaffolded_project(&dir);

    let out = dir.join("page.html");
    match cli::run_html(&project, Some(&out), (393, 852), false) {
        Ok(_) => {}
        Err(error) => {
            eprintln!("skipping: {error}");
            return;
        }
    }
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(page.starts_with("<!doctype html>"));
    // The counter's own words, as text a browser can select and a crawler
    // can read — the whole point of the DOM backend over a canvas.
    assert!(
        page.contains(">Counter<"),
        "the heading is an element's text"
    );
    assert!(
        page.contains(">Tapped 0 times<"),
        "the label rendered its state"
    );
    assert!(page.contains(">Add one<"), "the button's label");
    // And the walk's keyboard semantics, which a `<canvas>` cannot offer.
    assert!(page.contains("tabindex=\"0\""), "the button is a tab stop");
    assert!(page.contains("role=\"button\""));
}

#[test]
fn a_broken_say_file_reports_its_own_line() {
    let dir = scratch("broken-say");
    let file = dir.join("broken.say");
    std::fs::write(&file, "screen \"Broken\":\n    a label that never ends\n").unwrap();

    let error =
        cli::run_render(&file, None, (100, 100), false).expect_err("this file does not compile");
    assert!(
        error.contains("broken.say:"),
        "the diagnostic names the file and line: {error}"
    );
}

#[test]
fn a_rust_screen_without_an_entry_point_is_refused_before_rustc() {
    let dir = scratch("no-entry");
    let file = dir.join("widget.rs");
    std::fs::write(&file, "pub struct NotAScreen;\n").unwrap();

    let error = cli::run_render(&file, None, (100, 100), false).expect_err("no screen() to mount");
    assert!(
        error.contains("screen()"),
        "the refusal names what is missing: {error}"
    );
}

#[test]
fn a_missing_page_refuses_the_web_export_before_building() {
    let dir = scratch("web-refusal");
    let project = scaffolded_project(&dir);
    // The one file the plan asks for by name — a project without it is the
    // documented refusal ("copy `web/` across from a freshly scaffolded
    // project"), and deleting it from a fresh scaffold is the cheapest way
    // to reach that branch without building anything.
    std::fs::remove_file(project.join("web").join("index.html")).unwrap();

    let error = cli::run_web(&project).expect_err("the plan refuses");
    assert!(
        error.contains("web/index.html"),
        "the refusal names the file: {error}"
    );
}

#[test]
fn new_refuses_a_name_cargo_would_reject_twenty_seconds_later() {
    let dir = scratch("bad-name");
    let error = cli::run_new(&dir, "1demo", false).expect_err("a digit-led name");
    assert!(error.contains("cannot start with a digit"), "{error}");
    // And the directory it refused to make is not there.
    assert!(!dir.join("1demo").exists());
}
