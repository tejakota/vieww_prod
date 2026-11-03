//! ST-19 end to end: the Live Preview's sketch, materialized into real Say
//! and taken through the real compiler.
//!
//! # What this file proves
//!
//! The unit tests in `live_materialize.rs` prove the translation — the
//! template's screens become `screen_*` functions, the buttons become
//! `open the screen` actions, the labels cannot walk out of their quotes.
//! This file proves the part that makes it *the app* rather than another
//! approximation:
//!
//! * the emitted `.say` goes through the real Say compiler **and a real
//!   `rustc`**, loads, and renders — the same pipeline `Render` runs, so
//!   "materialize" cannot hand somebody a file the studio then refuses;
//! * the studio's own path works: the command writes `src/screens/flow.say`
//!   into the workspace, opens it, and never overwrites it on a second run;
//! * the buttons really navigate: tapping one screen's button in the mounted
//!   materialized flow lands on the screen it named, read back through the
//!   state capture the remote preview made.
//!
//! # Why the navigation is asserted through state, not pixels
//!
//! A flow's whole point is "the way between them", and the state snapshot
//! carries the Say navigation stack — `stack=…` in the snapshot string — so
//! the assertion reads where the app actually is rather than squinting at
//! pixels to guess it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use vieww_element::Runtime;
use vieww_foundation::{Color, Offset, Rect, Size, TargetPlatform};
use vieww_render::FrameDriver;
use viewwstudio::command::Command;
use viewwstudio::compile::{compile, Session, Toolchain};
use viewwstudio::loaded::{LoadError, Preview};
use viewwstudio::state::Studio;

/// `target/debug` of this workspace — see `tests/pipeline.rs`.
fn target() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary");
    exe.parent()
        .and_then(Path::parent)
        .expect("target/debug")
        .to_path_buf()
}

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
        let session = Session::new(0x5E19).ok()?;
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
            "skipping: no `vieww` rlib in {} matches this test binary's own compilation",
            target().display()
        );
        return None;
    };
    base.with_candidate(candidate)
}

/// The flow the template ships, as the studio's own command produces it:
/// parse `live.rs`, emit Say, and hand back the emitted text.
fn materialized_template() -> String {
    let doc = viewwstudio::livedoc::parse(viewwstudio::livedoc::TEMPLATE)
        .unwrap_or_else(|e| panic!("the template parses: {e:?}"));
    viewwstudio::live_materialize::say_of(&doc)
}

#[test]
fn the_materialized_flow_compiles_loads_and_renders() {
    let Some(toolchain) = toolchain() else {
        return;
    };
    let say = materialized_template();
    let generated = vieww_say_codegen::compile("flow.say", &say).unwrap_or_else(|diagnostics| {
        panic!("the materialized flow is valid Say: {diagnostics:?}")
    });

    // And through a real `rustc`, the way Render would.
    let session = Session::new(0x5E1A).expect("a temp directory");
    let compiled = compile(&toolchain, &session, &generated.rust, "flow.rs").expect("rustc ran");
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != viewwstudio::state::Severity::Error),
        "{:?}",
        compiled.diagnostics
    );
    let library = compiled.library.expect("a library");

    // It loads and renders: two frames, both stable, the same demand the
    // Say prompt's verification loop makes.
    let preview = Preview::load(&library).expect("the materialized flow loads");
    let mut driver = FrameDriver::new(Size::new(412.0, 915.0));
    viewwstudio::install(&mut driver);
    driver
        .set_root(vieww_widget::Theme::new(vieww_widget::ThemeData::light()).child(preview.node()));
    let mut renderer = vieww_paint::native::NativeRenderer::new();
    driver.draw_frame_at(Duration::ZERO);
    let first = renderer
        .render_to_png(driver.scene(), 412, 915, Color::WHITE)
        .expect("the flow renders")
        .0;
    driver.draw_frame_at(Duration::from_millis(16));
    let second = renderer
        .render_to_png(driver.scene(), 412, 915, Color::WHITE)
        .expect("the flow renders again")
        .0;
    assert_eq!(first, second, "the flow is deterministic across frames");

    // And the navigation is real: tapping the Home screen's "Open settings"
    // button moves the app onto the Settings screen, read back through the
    // element state's own snapshot of the navigation stack.
    //
    // The button is found by its label in the render tree — the same walk the
    // Inspector's outline performs — rather than by a hand-summed offset, so
    // the test taps the button wherever the sketch's own widgets laid it out.
    let tree = driver.renders();
    let described = tree
        .describe_subtree(tree.root().expect("a render root"), 100_000)
        .into_iter()
        .map(|(depth, node)| (depth as i64, node))
        .collect::<Vec<_>>();
    let mut origins: Vec<Offset> = vec![Offset::ZERO; described.len()];
    let mut button = None;
    for (index, (depth, node)) in described.iter().enumerate() {
        let parent = (0..index).rev().find(|p| described[*p].0 + 1 == *depth);
        let base = parent.map_or(Offset::ZERO, |p| origins[p]);
        origins[index] = Offset::new(base.dx + node.offset.dx, base.dy + node.offset.dy);
        let label = node
            .properties
            .iter()
            .find(|(key, _)| *key == "text")
            .map(|(_, value)| value.trim_matches('"'))
            .unwrap_or_default();
        if label == "Open settings" {
            button = Some(Rect::from_origin_size(origins[index], node.size));
        }
    }
    let button = button.expect("the Home screen's Open settings button");
    let where_ = Offset::new(
        button.origin().dx + button.size().width / 2.0,
        button.origin().dy + button.size().height / 2.0,
    );
    let tap = |driver: &mut FrameDriver, at: Duration, where_: Offset| {
        use vieww_foundation::PointerEvent;
        driver.handle_hover(Some(where_));
        driver.handle_pointer(&PointerEvent::down(
            vieww_foundation::PointerId(1),
            where_,
            at,
        ));
        driver.handle_pointer(&PointerEvent::up(
            vieww_foundation::PointerId(1),
            where_,
            at,
        ));
        driver.draw_frame_at(at);
    };
    tap(&mut driver, Duration::from_millis(32), where_);

    let root = driver
        .elements()
        .find("SayApp")
        .expect("the materialized app's root")
        .id();
    let snapshot = driver.elements().snapshot_states(root);
    let where_ = snapshot
        .iter()
        .map(|(_, state)| state.as_str())
        .collect::<Vec<_>>()
        .join(",");
    assert!(
        where_.contains("stack=settings"),
        "the button navigated to Settings: {where_}"
    );
}

/// The studio's own path: the command writes `src/screens/flow.say`, opens
/// it, and a second run refuses to overwrite what is now the user's file.
#[test]
fn the_command_writes_the_flow_once_and_never_overwrites() {
    let dir = std::env::temp_dir().join(format!(
        "live-materialize-{}-{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(&dir).expect("a temp workspace");
    std::fs::write(dir.join("live.rs"), viewwstudio::livedoc::TEMPLATE)
        .expect("the flow file on disk");

    let runtime = Runtime::new();
    let workspace = viewwstudio::buffer::Workspace::scratch();
    let studio = Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux);
    studio.root.set(Some(dir.clone()));

    studio.run(Command::MaterializeFlow);
    let path = dir.join("src/screens/flow.say");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the command wrote the flow: {e}"));
    assert!(
        text.contains("screen \"Home\""),
        "the written file is the materialized flow"
    );
    // And it is open, as a Say buffer.
    let active = studio.active().expect("the flow is open");
    assert_eq!(active.name, "flow.say");
    assert_eq!(
        active.language,
        viewwstudio::language::Language::Say,
        "detected from the extension, the way a person's file is"
    );
    assert_eq!(active.value.text, text);

    // A second run does not overwrite: the file is the user's now, whatever
    // live.rs says in the meantime.
    std::fs::write(&path, text.replace("\"Inbox\"", "\"Mine\"")).expect("the user's edit");
    studio.run(Command::MaterializeFlow);
    let after = std::fs::read_to_string(&path).expect("the file still exists");
    assert!(
        after.contains("\"Mine\""),
        "the user's edit survived a second materialize"
    );
    let notice = studio.notice.get().expect("the refusal says why");
    assert!(
        notice.contains("already exists"),
        "the notice names the file that is in the way: {notice}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Without a workspace there is nothing to materialize into, and the studio
/// says so rather than writing wherever it happens to be.
#[test]
fn materializing_without_a_folder_is_refused() {
    let runtime = Runtime::new();
    let workspace = viewwstudio::buffer::Workspace::scratch();
    let studio = Studio::with_workspace(&runtime, workspace).with_host(TargetPlatform::Linux);
    assert!(studio.root.get().is_none());
    studio.run(Command::MaterializeFlow);
    let notice = studio.notice.get().expect("a refusal");
    assert!(
        notice.contains("Open a folder first"),
        "the notice says what is missing: {notice}"
    );
    assert!(
        studio.active().is_none() || studio.active().is_some_and(|b| b.name != "flow.say"),
        "no buffer was opened"
    );
}
