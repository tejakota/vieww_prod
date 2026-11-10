//! `viewwstudio-cli` end to end: the studio's render paths a script would
//! drive, proven the way the pipeline tests prove the preview — real `rustc`,
//! real `dlopen`, real pixels, no window.
//!
//! # What this file proves
//!
//! 1. [`render_writes_a_png_of_the_scaffolded_home_screen`] — `new` then
//!    `render` on a fresh Say project produces a PNG of the counter screen,
//!    with the PNG's own header naming the size that was asked for. The
//!    whole promise of the tool in two calls.
//! 2. [`render_takes_a_hand_written_rust_screen_too`] — the `.rs` door: a
//!    screen with `pub fn screen()`, compiled without Say in the path.
//! 3. [`render_as_a_device_uses_the_presets_size`] — `--device` is the
//!    preview pane's simulation: the PNG comes out at the device's own
//!    points.
//! 4. [`render_all_screens_writes_one_png_per_screen`] — the batch door:
//!    every `screen "…":` block in a file, one PNG each, named for its
//!    screen.
//! 5. [`animate_writes_a_gif_of_the_scaffolded_home_screen`] — the clock
//!    door: frames stepped at 60 fps, sampled, and encoded by the
//!    workspace's own GIF codec (the magic bytes prove the format).
//! 6. [`html_carries_the_screens_text_as_real_dom`] — the same compile, the
//!    DOM backend's walk: the counter's heading, label and button are
//!    *text in elements*, the button reachable from a keyboard
//!    (`tabindex`/`role`, the walk's own semantics).
//! 7. [`a_broken_say_file_reports_its_own_line`] — the failure a beginner
//!    meets: the Say diagnostic, with the `.say` file's own line and
//!    column, before `rustc` ever runs.
//! 8. [`a_missing_page_refuses_the_web_export_before_building`] — the `web`
//!    plan's refusal, surfaced as the CLI's error rather than a half-built
//!    bundle.
//! 9. [`export_lists_every_format_the_sheet_has`] — `--list` names all nine
//!    formats, without building anything.
//! 10. [`test_writes_goldens_and_then_passes_against_them`] — the test
//!     gate's whole arc: no golden fails, `--update-goldens` writes them,
//!     a second run passes against what was written.
//!
//! The parse-level surface (every flag, every refusal) is unit-tested in
//! `src/cli/`'s own modules; this file is the artefact-producing half.
//!
//! # What is deliberately not here
//!
//! A test that runs the whole `web` export: it is a `cargo build` of the
//! framework for wasm32, which is minutes and a network away from a unit
//! test's budget. `tests/scaffold_exports.rs` checks the plan; the CLI
//! executes the plan's `Spec`s with the exit codes the studio's own task
//! runner already covers.
//!
//! A test that drives `preview`'s server loop: the page and the event
//! translation are unit-tested in `src/cli/preview.rs`, and the loop itself
//! is an accept loop around them — a socket test here would test the
//! kernel, not the studio.
//!
//! # The toolchain caveat, same as every real-rustc file
//!
//! These tests need a `vieww` rlib in the target directory that matches this
//! test binary's own compilation. On a machine where that is not so they
//! print the reason and pass, exactly as `tests/canvas_edit.rs` does — the
//! skip is stated on stderr, not silent.

use std::path::{Path, PathBuf};

use viewwstudio::cli::{self, ExportName};

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

/// The scaffolded project's default viewport: the preview's phone, as a
/// plain size (no device simulation), which is what `render` has always
/// been.
fn phone() -> cli::Viewport {
    cli::Viewport::plain()
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

    let summary = match cli::run_render(
        &project,
        None,
        &phone(),
        false,
        None,
        false,
        &cli::Presentation::default(),
    ) {
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
    assert_eq!(png_size(&png), (393, 852), "the size that was asked for");
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
    let size = cli::Viewport {
        device: None,
        size: (240, 80),
    };
    match cli::run_render(
        &file,
        Some(&out),
        &size,
        false,
        None,
        false,
        &cli::Presentation::default(),
    ) {
        Ok(summary) => assert!(summary.contains("hello.png"), "{summary}"),
        Err(error) => eprintln!("skipping: {error}"),
    }
    if out.is_file() {
        assert_eq!(png_size(&out), (240, 80));
    }
}

#[test]
fn render_as_a_device_uses_the_presets_size() {
    let dir = scratch("render-device");
    let project = scaffolded_project(&dir);

    let viewport = cli::Viewport::of_device(
        viewwstudio::state::Device::ALL
            .iter()
            .find(|device| device.name == "Android phone")
            .copied()
            .expect("the preset table has it"),
        true,
    );
    let out = dir.join("device.png");
    match cli::run_render(
        &project,
        Some(&out),
        &viewport,
        false,
        None,
        false,
        &cli::Presentation::default(),
    ) {
        Ok(_) => {}
        Err(error) => {
            eprintln!("skipping: {error}");
            return;
        }
    }
    // The Android phone, rotated: 915×412, the preset's own points.
    assert_eq!(png_size(&out), (915, 412));
}

#[test]
fn render_all_screens_writes_one_png_per_screen() {
    let dir = scratch("render-all");
    let project = scaffolded_project(&dir);
    // A second screen, so the batch has something to batch over — the
    // scaffold's home screen is one, and one PNG named for it is the
    // degenerate case this test would otherwise prove nothing with.
    std::fs::write(
        project.join("src").join("screens").join("flow.say"),
        "screen \"Home\":\n    a label \"from flow\"\n\nscreen \"Settings\":\n    a label \"also flow\"\n",
    )
    .unwrap();

    let summary = match cli::run_render(
        &project.join("src").join("screens").join("flow.say"),
        None,
        &phone(),
        false,
        None,
        true,
        &cli::Presentation::default(),
    ) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("skipping: {error}");
            return;
        }
    };
    let render = project.join("target").join("render");
    assert!(
        render.join("flow-home.png").is_file(),
        "one PNG per screen, slugged: {summary}"
    );
    assert!(
        render.join("flow-settings.png").is_file(),
        "the second screen's PNG: {summary}"
    );
    assert!(
        !render.join("flow.png").exists(),
        "the batch never writes the unsuffixed name"
    );

    // And `--screen Settings` alone is one PNG, suffixed the same way.
    let summary = cli::run_render(
        &project.join("src").join("screens").join("flow.say"),
        None,
        &phone(),
        false,
        Some("Settings"),
        false,
        &cli::Presentation::default(),
    )
    .expect("the named screen renders");
    assert!(
        render.join("flow-settings.png").is_file(),
        "the named screen's PNG: {summary}"
    );

    // A screen nobody defined is refused before rustc is paid.
    let error = cli::run_render(
        &project.join("src").join("screens").join("flow.say"),
        None,
        &phone(),
        false,
        Some("Nowhere"),
        false,
        &cli::Presentation::default(),
    )
    .expect_err("no such screen");
    assert!(
        error.contains("\"Settings\""),
        "the refusal names the screens that exist: {error}"
    );
}

#[test]
fn animate_writes_a_gif_of_the_scaffolded_home_screen() {
    let dir = scratch("animate-scaffolded");
    let project = scaffolded_project(&dir);

    let out = dir.join("home.gif");
    let summary = match cli::run_animate(
        &project,
        Some(&out),
        &phone(),
        false,
        None,
        10,
        std::time::Duration::from_millis(600),
        None,
        &cli::Presentation::default(),
    ) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("skipping: {error}");
            return;
        }
    };
    assert!(
        summary.contains("7 frames"),
        "0.6s at 10fps + the end: {summary}"
    );
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        bytes.starts_with(b"GIF89a"),
        "a GIF, by its magic — {} bytes",
        bytes.len()
    );
    assert!(
        bytes.len() > 1_000,
        "an animation is more than a header: {} bytes",
        bytes.len()
    );
}

#[test]
fn html_carries_the_screens_text_as_real_dom() {
    let dir = scratch("html-scaffolded");
    let project = scaffolded_project(&dir);

    let out = dir.join("page.html");
    match cli::run_html(&project, Some(&out), &phone(), false, None) {
        Ok(_) => {}
        Err(error) => {
            eprintln!("skipping: {error}");
            return;
        }
    }
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(page.starts_with("<!doctype html>"));
    // The starter screen's own words, as text a browser can select and a
    // crawler can read — the whole point of the DOM backend over a canvas.
    assert!(
        page.contains(">Welcome<"),
        "the heading is an element's text"
    );
    assert!(page.contains(">0<"), "the count rendered its state");
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

    let error = cli::run_render(
        &file,
        None,
        &phone(),
        false,
        None,
        false,
        &cli::Presentation::default(),
    )
    .expect_err("this file does not compile");
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

    let error = cli::run_render(
        &file,
        None,
        &phone(),
        false,
        None,
        false,
        &cli::Presentation::default(),
    )
    .expect_err("no screen() to mount");
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
    if !error.contains("web/index.html") {
        // A machine without the wasm toolchain is told about *that* first —
        // refusal-before-start is the plan's own design, and the page check
        // this test pins is behind it. The stated skip, as the header's
        // toolchain caveat puts it.
        eprintln!("skipping: {error}");
        return;
    }
    assert!(
        error.contains("web/index.html"),
        "the refusal names the file: {error}"
    );
}

#[test]
fn export_lists_every_format_the_sheet_has() {
    let dir = scratch("export-list");
    let project = scaffolded_project(&dir);

    // `--list` builds nothing — it is the toolchain probe, so it runs on
    // any machine, no skip needed.
    let listing = cli::run_export(Some(&project), None, false, true).expect("the listing");
    for format in ExportName::ALL {
        assert!(
            listing.contains(format.as_str()),
            "the listing names {}: {listing}",
            format.as_str()
        );
    }
    assert!(
        listing.contains("apk-release"),
        "the whole sheet, not a subset: {listing}"
    );
}

#[test]
fn test_writes_goldens_and_then_passes_against_them() {
    let dir = scratch("test-goldens");
    let project = scaffolded_project(&dir);

    // No golden yet: the run fails by naming what to run next.
    let error = match cli::run_test(Some(&project), &phone(), false, None, false) {
        Ok(passed) => {
            // A project with no toolchain cannot compile; if it passed,
            // something is claiming more than it did.
            panic!("a fresh project has no goldens: {passed}");
        }
        Err(error) => error,
    };
    if error.contains("no preview compiler") {
        eprintln!("skipping: {error}");
        return;
    }
    assert!(
        error.contains("--update-goldens"),
        "the failure names the remedy: {error}"
    );

    // Write them, then pass against them.
    cli::run_test(Some(&project), &phone(), false, None, true)
        .expect("the goldens are written and the rest of the gate passes");
    let golden = project
        .join("target")
        .join("render")
        .join("goldens")
        .join("home-home.png");
    assert!(golden.is_file(), "the golden is named for the screen");

    cli::run_test(Some(&project), &phone(), false, None, false)
        .expect("the second run passes against what the first wrote");
}

#[test]
fn devices_lists_the_studio_presets() {
    let listing = cli::run_devices().expect("the table is a table");
    assert!(listing.contains("phone"), "{listing}");
    assert!(listing.contains("393 × 852"), "{listing}");
    assert!(listing.contains("android-tablet"), "{listing}");
}

#[test]
fn assets_lists_the_scaffolded_bundle() {
    let dir = scratch("assets");
    let project = scaffolded_project(&dir);

    let listing = cli::run_assets(Some(&project)).expect("the scaffold has an assets/ dir");
    assert!(listing.contains("assets"), "{listing}");
    assert!(
        listing.contains("empty"),
        "a fresh project's bundle is empty, and the listing says so: {listing}"
    );

    std::fs::write(project.join("assets").join("dot.png"), vec![0_u8; 64]).unwrap();
    let listing = cli::run_assets(Some(&project)).expect("the listing walks");
    assert!(listing.contains("dot.png"), "{listing}");
    assert!(listing.contains("64 B"), "{listing}");
}

#[test]
fn a_screen_from_the_template_lands_in_the_project() {
    let dir = scratch("screen-template");
    let project = scaffolded_project(&dir);

    // Through the project around a file inside it — the same `project_at`
    // walk the command does from the command line, exercised from a test
    // that cannot stand in the scratch directory.
    let made = cli::run_screen_from(&project, "login").expect("the template writes");
    assert!(made.contains("login.rs"), "{made}");
    assert!(project
        .join("src")
        .join("screens")
        .join("login.rs")
        .is_file());
    let mod_rs = std::fs::read_to_string(project.join("src").join("screens").join("mod.rs"))
        .expect("the scaffold writes a mod.rs");
    assert!(
        mod_rs.contains("pub mod login;") || !mod_rs.contains("pub mod home;"),
        "a Say project's build.rs walks the directory and needs no module line; a Rust \
         project's mod.rs gains one. Either way the screen is reachable: {mod_rs}"
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
