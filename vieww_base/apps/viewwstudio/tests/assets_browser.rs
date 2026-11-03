//! ST-15 end to end: the project's `assets/`, browsed, previewed, renamed and
//! trashed from inside the studio.
//!
//! # What this file proves
//!
//! The unit tests in `assets_browser.rs` prove the pieces — the walk, the
//! signature, the font card, the refusals. This file proves the wires:
//!
//! * `Studio::open_path` routes an image or a font to a preview rather than
//!   to the mojibake a binary buffer gets, and a tab click hands the pane
//!   back — the one rule the whole pane design rests on;
//! * the listing follows the folder through the studio's own rescans, so a
//!   rename or a move-to-trash driven the way the context menu drives it
//!   (name prompt, delete confirmation — both through the public paths the
//!   dialogs call) is reflected without a second implementation of either;
//! * the `WorkspaceBundle` the studio installs resolves a guest's read
//!   against the open project's `assets/` first, which is the runtime half
//!   of "asset browser" — the browser shows what the guest will get;
//! * a Say buffer's `an image "…"` can be inserted from the browser only
//!   when Say can actually resolve it, and a missing one becomes a Problem
//!   at the line that wrote it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use vieww_asset::AssetBundle;
use vieww_element::Runtime;
use viewwstudio::assets::WorkspaceBundle;
use viewwstudio::assets_browser::{Kind, Listing, Preview};
use viewwstudio::compile::{compile, Session, Toolchain};
use viewwstudio::loaded::{LoadError, Preview as LoadedPreview};
use viewwstudio::state::{NameKind, PreviewState, View};
use viewwstudio::Studio;

/// A 3×2 PNG — six hand-picked pixels: red, green, blue over white, black,
/// grey — as 78 literal bytes.
///
/// Every `*.png` in this repo is a `[binary asset]` stub (the tree carries no
/// binaries), so the honest choices were a checked-in fixture that could be
/// lost, or a literal that cannot be. The width and height are asserted below
/// from the preview, so the bytes cannot silently stop being this image.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 2, 8, 2, 0,
    0, 0, 18, 22, 241, 77, 0, 0, 0, 21, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 192, 0, 193,
    255, 255, 131, 232, 134, 134, 6, 0, 71, 207, 7, 123, 211, 250, 178, 66, 0, 0, 0, 0, 73, 69, 78,
    68, 174, 66, 96, 130,
];

/// The repo's one real font: the DejaVu Sans Mono subset the text stack ships.
///
/// Reached by manifest-relative path rather than checked in a second time,
/// because a copy is how fixtures drift from the font the studio actually
/// draws with.
const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/vieww-text/assets/DejaVuSansMono-Bold-subset.ttf"
);

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "viewwstudio-assets-browser-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("assets")).expect("a temp dir with an assets/ folder");
        Self(path)
    }

    /// Write `bytes` into `assets/<name>` and return the canonical path.
    fn asset(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("assets").join(name);
        std::fs::write(&path, bytes).expect("write the asset");
        std::fs::canonicalize(&path).unwrap_or(path)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn studio_in(dir: &Dir) -> Studio {
    let runtime = Runtime::new();
    let studio = Studio::new(&runtime);
    studio.open_workspace(&dir.0);
    studio
}

/// The relative paths of a listing, in walk order.
fn relatives(listing: &Listing) -> Vec<String> {
    listing.entries.iter().map(|e| e.relative.clone()).collect()
}

// ---------------------------------------------------------------------------
// Opening
// ---------------------------------------------------------------------------

#[test]
fn an_image_opens_as_a_preview_rather_than_as_mojibake() {
    let dir = Dir::new("open-image");
    let png = dir.asset("avatar.png", PNG);
    let studio = studio_in(&dir);

    let before = studio.buffers.get().len();
    studio.open_path(png.clone());

    // A preview, with the header's truth about the file beside the pixels.
    let view = studio.asset_view.get().expect("an asset preview");
    assert_eq!(view.name, "avatar.png");
    match &view.preview {
        Ok(Preview::Image {
            format,
            width,
            height,
            ..
        }) => {
            assert_eq!((*width, *height), (3, 2), "the header's own dimensions");
            assert!(
                matches!(format, vieww_asset::ImageFormat::Png),
                "the codec's own format"
            );
        }
        other => panic!("an image preview, not {other:?}"),
    }
    assert_eq!(view.bytes, PNG.len() as u64);

    // And not a buffer: an asset has no tab, no caret and no history.
    assert_eq!(
        studio.buffers.get().len(),
        before,
        "an asset must not become a binary buffer"
    );

    // The listing's entry is reachable from the preview's path, which is the
    // pane's way to "insert this asset at the caret".
    let entry = studio
        .asset_entry(&png)
        .expect("the preview's path is a listed asset");
    assert_eq!(entry.kind, Kind::Image);
    assert_eq!(entry.relative, "avatar.png");

    // The pane holds one thing: a buffer tab click hands it back to code.
    studio.focus_buffer(0);
    assert!(
        studio.asset_view.get().is_none(),
        "focusing a buffer closes the preview"
    );
    // And so does the preview's own close button.
    studio.open_path(png);
    studio.close_asset();
    assert!(studio.asset_view.get().is_none());
}

#[test]
fn a_font_opens_as_its_own_card() {
    let dir = Dir::new("open-font");
    let bytes = std::fs::read(FONT).expect("the repo's DejaVu subset");
    let font = dir.asset("type.ttf", &bytes);
    let studio = studio_in(&dir);

    studio.open_path(font);
    match &studio.asset_view.get().expect("a font preview").preview {
        Ok(Preview::Font(card)) => {
            assert!(card.family.contains("DejaVu Sans Mono"), "{card:?}");
            assert!(card.style.contains("Bold"), "{card:?}");
            assert_eq!(card.weight, 700, "{card:?}");
            assert!(card.monospaced, "{card:?}");
            assert!(card.glyphs > 0, "{card:?}");
        }
        other => panic!("a font card, not {other:?}"),
    }
}

#[test]
fn the_assets_view_sits_beside_the_explorer_and_names_itself() {
    // The activity bar is built from `View::ALL`, and the session stores a
    // title — both have to know the view, or the bar shows it and the session
    // cannot restore it (or vice versa).
    assert_eq!(View::from_title("Assets"), Some(View::Assets));
    assert_eq!(View::ALL[0], View::Explorer);
    assert_eq!(View::ALL[1], View::Assets, "beside the Explorer");
}

// ---------------------------------------------------------------------------
// The listing, and the folder it follows
// ---------------------------------------------------------------------------

#[test]
fn the_listing_follows_the_folder_through_the_studio_paths() {
    let dir = Dir::new("listing");
    dir.asset("avatar.png", PNG);
    let studio = studio_in(&dir);

    // Filled by `adopt_workspace`, not by waiting for a rescan.
    let listing = studio.assets.get();
    assert_eq!(relatives(&listing), ["avatar.png"]);

    // A file that appears is listed after the next rescan; a hidden one never
    // is — the walk's rule, seen from the studio's front door.
    dir.asset("logo.png", PNG);
    std::fs::write(dir.0.join("assets/.DS_Store"), b"junk").expect("hidden file");
    studio.rescan_tree();
    assert_eq!(
        relatives(&studio.assets.get()),
        ["avatar.png", "logo.png"],
        "the hidden file is skipped, the new one is listed"
    );

    // And one that is gone is gone: delete through the confirmation the
    // context menu's "Move to Trash…" runs, which is `file_tree::delete_to_trash`
    // under the dialog.
    let logo = dir.0.join("assets/logo.png");
    studio.ask_to_delete(logo.clone());
    studio.confirm_delete();
    assert_eq!(relatives(&studio.assets.get()), ["avatar.png"]);
    // To the trash, not `unlink`: the bytes are still on disk, under the
    // workspace, in the folder the tree and the listing both skip.
    let trash = dir.0.join(".viewwstudio-trash");
    let trashed = std::fs::read_dir(&trash)
        .expect("the trash folder")
        .flatten()
        .flat_map(|entry| match std::fs::read_dir(entry.path()) {
            Ok(entries) => entries.flatten().collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(trashed, ["logo.png"], "moved, not unlinked");
}

#[test]
fn a_rename_driven_the_way_the_dialog_drives_it_reaches_the_listing() {
    let dir = Dir::new("rename");
    let png = dir.asset("avatar.png", PNG);
    let studio = studio_in(&dir);

    // The context menu's "Rename…" opens the name prompt on the path; the
    // dialog's confirm is `confirm_name_prompt`. Driving both from here is
    // the same code the dialog runs, with the pointer taken out.
    studio.ask_for_name(NameKind::Rename, &png);
    studio.set_prompt_name("hero.png".to_owned());
    studio.confirm_name_prompt();

    assert_eq!(relatives(&studio.assets.get()), ["hero.png"]);
    assert!(
        dir.0.join("assets/hero.png").is_file(),
        "renamed on disk, under assets/"
    );
}

// ---------------------------------------------------------------------------
// The runtime half: what a guest's read resolves to
// ---------------------------------------------------------------------------

#[test]
fn the_workspace_bundle_resolves_the_projects_assets_first() {
    let dir = Dir::new("bundle");
    let png = dir.asset("avatar.png", PNG);
    let _ = png; // written; the assertions below read by bundle path
    let runtime = Runtime::new();

    // The same two lines `main` runs: wrap the platform's bundle — here an
    // empty `Services`, which is the studio-with-no-fallback case — and hand
    // the wrapper to the studio so the workspace can change underneath it.
    let mut services = vieww_foundation::Services::new();
    let bundle = WorkspaceBundle::install(&mut services);
    let studio = Studio::new(&runtime).with_workspace_bundle(bundle.clone());
    studio.open_workspace(&dir.0);

    // The project half is the workspace's own assets/, canonicalised the
    // way `open_workspace` canonicalises the root before adopting it.
    assert_eq!(
        bundle.project_root().as_deref(),
        Some(
            std::fs::canonicalize(dir.0.join("assets"))
                .expect("assets/")
                .as_path()
        ),
        "the wrapper points at the open workspace's assets/"
    );
    // And a guest asking by bundle path gets the project's bytes.
    assert_eq!(bundle.open("avatar.png").unwrap(), PNG.to_vec());
}

// ---------------------------------------------------------------------------
// Say and its two conventions
// ---------------------------------------------------------------------------

#[test]
fn an_image_line_is_inserted_only_where_say_can_resolve_it() {
    let dir = Dir::new("insert");
    dir.asset("cat.png", PNG);
    std::fs::create_dir_all(dir.0.join("src/screens")).expect("a screens folder");
    // A block line: the insert should become its child, indented, after it.
    std::fs::write(
        dir.0.join("flow.say"),
        "-- say-language: 1\nscreen \"home\":\n",
    )
    .expect("the .say file");
    let studio = studio_in(&dir);
    studio.open_path(dir.0.join("flow.say"));
    let entry = studio
        .assets
        .get()
        .find("cat.png")
        .cloned()
        .expect("the asset");

    studio.insert_say_image(&entry);
    let text = studio.active().expect("the flow buffer").value.text.clone();
    assert_eq!(
        text, "-- say-language: 1\nscreen \"home\":\n    an image \"assets/cat.png\"\n",
        "inserted as an indented child line under the block"
    );

    // The same asset, a .say one folder deeper: Say reads images from beside
    // the .say file and refuses `..`, so the insert refuses too, says why in
    // the notification, and leaves the buffer alone.
    std::fs::write(
        dir.0.join("src/screens/deep.say"),
        "-- say-language: 1\nscreen \"deep\":\n",
    )
    .expect("deep");
    studio.open_path(dir.0.join("src/screens/deep.say"));
    let before = studio.active().expect("the deep buffer").value.text.clone();
    studio.insert_say_image(&entry);
    assert_eq!(
        studio.active().expect("the deep buffer").value.text,
        before,
        "a refused insert does not touch the buffer"
    );
    let notice = studio.notice.get().expect("a refusal said out loud");
    assert!(
        notice.contains("not beside the .say file"),
        "the notice explains the convention: {notice}"
    );

    // And a file with no screen below the caret — only the pragma — refuses
    // rather than writing an image line at top level, where Say has no
    // widgets at all.
    std::fs::write(dir.0.join("bare.say"), "-- say-language: 1\n").expect("bare");
    studio.open_path(dir.0.join("bare.say"));
    let before = studio.active().expect("the bare buffer").value.text.clone();
    studio.insert_say_image(&entry);
    assert_eq!(
        studio.active().expect("the bare buffer").value.text,
        before,
        "no screen, no line"
    );
    let notice = studio.notice.get().expect("a refusal said out loud");
    assert!(
        notice.contains("no screen below the caret"),
        "the notice says what to do: {notice}"
    );
}

#[test]
fn a_missing_image_reference_is_a_problem_at_the_line_that_wrote_it() {
    let dir = Dir::new("missing");
    dir.asset("there.png", PNG);
    let studio = studio_in(&dir);

    let source = "-- say-language: 1\nscreen \"home\":\n    an image \"there.png\"\n    an image \"gone.png\"\n";
    let diagnostics =
        studio.missing_image_references("flow.say", source, Some(&dir.0.join("assets")));
    // gone.png is flagged, there.png is not, and the line is Say's own line
    // number, not the diagnostic's invention.
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 4, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "asset-missing");
    assert!(diagnostics[0].message.contains("gone.png"));

    // A half-edited file still gets its image lines checked: the parse is
    // the lenient one the canvas and inspector use, not the compiler's — the
    // image's quote is not closed yet, and the line is still found, at its
    // own line.
    let half_edited = "-- say-language: 1\nscreen \"home\":\n    an image \"also-gone.png";
    let diagnostics =
        studio.missing_image_references("flow.say", half_edited, Some(&dir.0.join("assets")));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].line, 3, "{diagnostics:?}");
}

// ---------------------------------------------------------------------------
// The Problem, at the render that would have shown the placeholder
// ---------------------------------------------------------------------------

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

/// The toolchain candidate whose `vieww` rlib matches this test binary's own
/// compilation — the walk `remote.rs` and `live_materialize.rs` walk too.
fn matching_candidate(base: &Toolchain) -> Option<usize> {
    static RESOLVED: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *RESOLVED.get_or_init(|| {
        let session = Session::new(0xA15C).ok()?;
        for candidate in 0.. {
            let toolchain = base.with_candidate(candidate)?;
            let Ok(compiled) = compile(&toolchain, &session, PROBE, "probe.rs") else {
                continue;
            };
            let Some(library) = compiled.library.as_ref() else {
                continue;
            };
            match LoadedPreview::load(library) {
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

/// The whole point of the report, proven through the real pipeline: a Say
/// screen with an image that is not there still compiles — rustc accepts the
/// line, the generated code draws a placeholder at runtime — and the render
/// that would have shown the placeholder says so in the Problems panel
/// instead, at the line that wrote it.
#[test]
fn a_missing_image_reference_becomes_a_problem_when_the_screen_renders() {
    let Some(toolchain) = toolchain() else { return };
    let dir = Dir::new("render-problems");
    dir.asset("there.png", PNG);
    std::fs::write(
        dir.0.join("flow.say"),
        "-- say-language: 1\nscreen \"home\":\n    an image \"assets/there.png\"\n    an image \"gone.png\"\n",
    )
    .expect("the .say file");

    let runtime = Runtime::new();
    let studio = Studio::new(&runtime).with_toolchain(Ok(toolchain), Session::new(0xA15D).ok());
    // The guest loads in a child, as `remote.rs`'s studio-render test does —
    // the assertion is about diagnostics, and a child cannot take the test
    // binary down on the way to them.
    studio.isolated_preview.set(true);
    studio.open_workspace(&dir.0);
    studio.open_path(dir.0.join("flow.say"));
    studio.render();

    // `poll_compile` finishes a Render — and on an ABI mismatch starts the
    // next candidate's — so poll until the state settles, as `remote.rs`
    // does. The deadline is rustc's, not the studio's.
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while studio.preview.get() == PreviewState::Compiling {
        studio.poll_compile();
        assert!(
            std::time::Instant::now() < deadline,
            "the Render finished: {:?}",
            studio.output.get()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        studio.preview.get(),
        PreviewState::Rendered,
        "{:?}",
        studio.output.get()
    );

    // The Problems panel holds the missing one, at its own line, and not the
    // one that resolves — the render is what carries the report, so this is
    // the merge a plain `finish` used to overwrite.
    let diagnostics = studio.diagnostics.get();
    let missing: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "asset-missing")
        .collect();
    assert_eq!(missing.len(), 1, "{diagnostics:?}");
    assert_eq!(missing[0].file, "flow.say", "{diagnostics:?}");
    assert_eq!(missing[0].line, 4, "{diagnostics:?}");
    assert!(
        missing[0].message.contains("gone.png"),
        "{}",
        missing[0].message
    );

    // And a re-render of the same screen does not double them: the stash is
    // taken, not read.
    studio.render();
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while studio.preview.get() == PreviewState::Compiling {
        studio.poll_compile();
        assert!(
            std::time::Instant::now() < deadline,
            "the second Render finished: {:?}",
            studio.output.get()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let diagnostics = studio.diagnostics.get();
    let still: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "asset-missing")
        .collect();
    assert_eq!(still.len(), 1, "taken once, not repeated: {diagnostics:?}");
}
