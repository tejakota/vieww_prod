//! The studio's render paths, without the window: `viewwstudio-cli`.
//!
//! # What this module is
//!
//! Everything the studio's preview pane does to turn a buffer into a picture
//! — Say codegen, one `rustc`, a `dlopen`, a rasterisation — is library code
//! this crate already owns. The only thing that needs a window is the window.
//! This module is the one entry point for people who want the *output* and
//! not the IDE: a screenshot in a script, a static page in a docs build, a
//! wasm bundle in a deploy step.
//!
//! * `new <name>` — the studio's New Project, written to a directory instead
//!   of a workspace. [`scaffold::create_for`] verbatim.
//! * `render <file>` — the preview pipeline verbatim ([`compile_screen`]):
//!   `.say` → [`vieww_say_codegen`] → `rustc` → `dlopen` → the same CPU
//!   rasteriser `examples/screenshot` and ST-03's verification both use. A
//!   PNG, headless, no display.
//! * `html <file>` — the *same compiled tree*, walked by
//!   `vieww-platform-web-dom` exactly the way `mytlabs`' snapshot example
//!   walks its page: real elements, real text, a page a crawler can read.
//! * `web <project>` — the studio's Web export format verbatim
//!   ([`export::plan`] with [`export::Format::Web`]): the interactive
//!   wasm32 application, not a snapshot of it.
//! * `screens <file>` — the screens a `.say` file defines.
//!
//! # What is shared, and what that buys
//!
//! Nothing here is a reimplementation. The compile is the studio's own
//! [`compile`] door — so the ABI fingerprint guard, `-C prefer-dynamic`, the
//! candidate search and the project-rlib link all behave exactly as they do
//! behind the Render button — the scaffold is the same files the New Project
//! dialog writes, and the web plan is the same `Spec` list the export sheet
//! previews. A bug fixed for the window is fixed here; a refusal the studio
//! makes, this makes with the same words.
//!
//! # The honest limits
//!
//! * `render` proves the screen renders, once, at the size asked — the same
//!   claim ST-03's double-render verification makes, minus the second frame.
//! * `html` is the DOM backend's translation of the tree, and the browser
//!   lays it out — it is *not* a pixel copy of the raster, and it is not
//!   interactive: handlers exist in the tree but only the wasm build runs
//!   them. What it is, the canvas backend is not: selectable, searchable,
//!   readable by a crawler.
//! * `web` needs the wasm toolchain (`rustup target add
//!   wasm32-unknown-unknown`, `wasm-bindgen`); the planner's refusal says so
//!   in its own words rather than failing halfway through a build.

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::Duration;

use vieww_element::ElementTree;
use vieww_foundation::{Color, Size};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;
use vieww_say_codegen::Options as SayOptions;
use vieww_widget::{Theme, ThemeData};

use crate::compile::{self, compile, compile_project, ProjectLib, Session, Toolchain, TIMEOUT};
use crate::components::PROJECT_REACH;
use crate::loaded::{LoadError, Preview};
use crate::scaffold::{self, Dependency, ProjectKind};
use crate::state::Diagnostic;
use crate::task::Spec;
use crate::{export, toolchains};

/// The size a `render`/`html` runs at when nobody chose one: the studio
/// preview's own phone, the `393×852` of an iPhone (`ui/preview.rs`), rather
/// than a round number nobody ships.
pub const DEFAULT_SIZE: (u32, u32) = (393, 852);

// ---- the command line ------------------------------------------------------

/// One parsed invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `new` — scaffold a project.
    New {
        /// The project's name, and the directory written.
        name: String,
        /// A Rust project rather than a Say one.
        rust: bool,
    },
    /// `render` — the preview pipeline to a PNG.
    Render {
        /// A `.say`/`.rs` file, or a project directory.
        input: PathBuf,
        /// Where the PNG lands; `None` for the default, which needs the
        /// input resolved before it can be named.
        out: Option<PathBuf>,
        /// The raster's size, in pixels.
        size: (u32, u32),
        /// The dark theme rather than the light one.
        dark: bool,
    },
    /// `html` — the same tree as a static DOM page.
    Html {
        /// A `.say`/`.rs` file, or a project directory.
        input: PathBuf,
        /// Where the page lands; `None` for the default.
        out: Option<PathBuf>,
        /// The page's width, in CSS pixels.
        size: (u32, u32),
        /// The dark theme rather than the light one.
        dark: bool,
    },
    /// `web` — the studio's wasm32 export, run for real.
    Web {
        /// A project directory (the one with the `Cargo.toml`).
        project: PathBuf,
    },
    /// `screens` — the screens a `.say` file defines.
    Screens {
        /// A `.say` file, or a project directory.
        input: PathBuf,
    },
}

/// How to call the tool, as text. The whole of the argument-error path: a
/// parse failure prints this and nothing else, because a usage message that
/// also half-runs a subcommand is a usage message nobody trusts.
#[must_use]
pub fn usage() -> String {
    format!(
        "the studio's render paths, without the window

USAGE:
    viewwstudio-cli new <name> [--rust]        scaffold a project into ./<name>
    viewwstudio-cli render <path> [options]    a .say screen (or project) to a PNG
    viewwstudio-cli html <path> [options]      the same screen as static HTML
    viewwstudio-cli web <project>              the interactive wasm web export
    viewwstudio-cli screens <path>             list the screens a .say file defines

    <path> is a .say file, a .rs screen, or a project directory (its
    src/screens/ is searched; home.say wins when several are there).

OPTIONS (render/html):
    --out <file>      where the result lands
                      (default: <project>/target/render/<name>.png|.html,
                       or beside the input when there is no project)
    --width <px>      raster width  (default {width})
    --height <px>     raster height (default {height})
    --dark            the dark theme rather than the light one

Both render paths need the toolchain the studio itself was built with —
build the workspace (`cargo build -p viewwstudio`) first, the same
requirement the preview pane has.",
        width = DEFAULT_SIZE.0,
        height = DEFAULT_SIZE.1,
    )
}

/// Parse `args` (past the program name) into a [`Command`].
///
/// Hand-rolled, like `main.rs` and every example in this crate: the studio
/// carries one third-party dependency by policy, and an argument parser is
/// not worth becoming the second.
///
/// # Errors
///
/// Empty for "print the usage", otherwise a sentence naming what was wrong.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some(first) = args.first() else {
        return Err(String::new());
    };
    let rest = &args[1..];
    match first.as_str() {
        "new" => {
            let name = rest.iter().find(|a| !a.starts_with("--")).ok_or_else(|| {
                "new needs a name — viewwstudio-cli new <name> [--rust]".to_owned()
            })?;
            Ok(Command::New {
                name: name.clone(),
                rust: rest.iter().any(|a| a == "--rust"),
            })
        }
        "render" | "html" => {
            let input = rest
                .iter()
                .find(|a| !a.starts_with("--"))
                .ok_or_else(|| {
                    format!("{first} needs a .say file, a .rs screen, or a project directory")
                })?
                .clone();
            let mut out = None;
            let mut width = DEFAULT_SIZE.0;
            let mut height = DEFAULT_SIZE.1;
            let mut dark = false;
            let mut flag = rest.iter().skip_while(|a| !a.starts_with("--"));
            while let Some(arg) = flag.next() {
                match arg.as_str() {
                    "--dark" => dark = true,
                    "--out" => {
                        out = Some(absolute(
                            flag.next()
                                .filter(|value| !value.starts_with("--"))
                                .ok_or("--out needs a path after it")?,
                        )?);
                    }
                    "--width" | "--height" => {
                        let raw = flag
                            .next()
                            .ok_or_else(|| format!("{arg} needs a number after it"))?;
                        let value: u32 = raw
                            .parse()
                            .map_err(|_| format!("{arg} needs a number — {raw} is not one"))?;
                        if value == 0 {
                            return Err(format!("{arg} must be at least 1"));
                        }
                        if arg == "--width" {
                            width = value;
                        } else {
                            height = value;
                        }
                    }
                    other => return Err(format!("{first} does not take {other}")),
                }
            }
            let input = absolute(&input)?;
            let size = (width, height);
            Ok(if first == "render" {
                Command::Render {
                    input,
                    out,
                    size,
                    dark,
                }
            } else {
                Command::Html {
                    input,
                    out,
                    size,
                    dark,
                }
            })
        }
        "web" => {
            let project = rest
                .iter()
                .find(|a| !a.starts_with("--"))
                .ok_or_else(|| {
                    "web needs a project directory — the one with the Cargo.toml".to_owned()
                })?
                .clone();
            Ok(Command::Web {
                project: absolute(&project)?,
            })
        }
        "screens" => {
            let input = rest
                .iter()
                .find(|a| !a.starts_with("--"))
                .ok_or_else(|| "screens needs a .say file or a project directory".to_owned())?
                .clone();
            Ok(Command::Screens {
                input: absolute(&input)?,
            })
        }
        "help" | "--help" | "-h" => Err(String::new()),
        other => Err(format!("unknown command {other}")),
    }
}

/// A path as an absolute one, resolved against where the user stands — so a
/// relative `--out` and a relative input mean the same thing to the writer
/// and to the sentence printed about them.
fn absolute(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(&path))
            .map_err(|error| format!("cannot resolve {} — {error}", path.display()))
    }
}

// ---- shared resolution -----------------------------------------------------

/// What `render`/`html`/`screens` decided to compile: a file, and the cargo
/// project it sits in (if any) — the project decides both the default output
/// path and whether a component reference can be linked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Screen {
    path: PathBuf,
    project: Option<PathBuf>,
}

/// Resolve the `<path>` argument into a file to compile.
///
/// A `.say`/`.rs` file is taken as-is. A directory is searched the way the
/// studio's own workspace is laid out: `src/screens/`, where the scaffold
/// puts every screen and where New Screen adds more. `home` wins when
/// several screens are there — it is the one a fresh project opens with, and
/// the one "render my project" means; anything else wants naming on the
/// command line, and the error says which names exist.
fn resolve_input(input: &Path) -> Result<Screen, String> {
    if input.is_file() {
        let kind = input
            .extension()
            .map_or(String::new(), |e| e.to_string_lossy().into_owned());
        if kind != "say" && kind != "rs" {
            return Err(format!(
                "{} is a .{kind} file — render takes a .say screen or a .rs screen",
                input.display()
            ));
        }
        return Ok(Screen {
            project: project_root_of(input),
            path: input.to_path_buf(),
        });
    }
    if input.is_dir() {
        let screens = input.join("src").join("screens");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&screens)
            .map_err(|_| {
                format!(
                    "no src/screens/ under {} — pass a .say file directly",
                    input.display()
                )
            })?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "say" || e == "rs"))
            .collect();
        found.sort();
        return match found.len() {
            0 => Err(format!(
                "no .say or .rs screen under {} — pass the file directly",
                screens.display()
            )),
            _ => {
                let home = found
                    .iter()
                    .find(|path| path.file_stem().is_some_and(|stem| stem == "home"))
                    .unwrap_or(&found[0]);
                Ok(Screen {
                    path: home.clone(),
                    project: Some(input.to_path_buf()),
                })
            }
        };
    }
    Err(format!("{} does not exist", input.display()))
}

/// The nearest ancestor with a `Cargo.toml`, if any — the same question the
/// studio asks of an opened folder, answered by walking up because a screen
/// can sit any number of modules deep.
fn project_root_of(path: &Path) -> Option<PathBuf> {
    let mut dir = path.parent()?;
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// `--out`'s default, once the input is resolved. `<project>/target/render/
/// <stem>.<ext>` when the screen sits inside a cargo project — cargo's own
/// place for generated things, and gitignored by the scaffold — or beside
/// the input when it does not.
fn default_output(screen: &Screen, html: bool) -> PathBuf {
    let extension = if html { "html" } else { "png" };
    let stem = screen
        .path
        .file_stem()
        .map_or_else(|| "screen".to_owned(), |s| s.to_string_lossy().into_owned());
    screen.project.as_ref().map_or_else(
        || screen.path.with_extension(extension),
        |root| {
            root.join("target")
                .join("render")
                .join(stem)
                .with_extension(extension)
        },
    )
}

/// The `[package] name` of a manifest, as a line scan.
///
/// Not a TOML parse: the scaffold writes this file, the shape is one
/// `name = "…"` under one `[package]` heading, and a hand-edited manifest
/// that breaks the shape fails later with cargo's own words rather than a
/// half-parsed guess here.
fn package_name(root: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            if let Some(value) = line
                .strip_prefix("name")
                .and_then(|value| value.trim().strip_prefix('='))
                .map(str::trim)
            {
                return Some(value.trim_matches('"').to_owned());
            }
        }
    }
    None
}

// ---- new -------------------------------------------------------------------

/// `new`: the New Project dialog's writer, pointed at a directory.
///
/// `parent` is where the project directory lands — the command line passes
/// where the user stands, a test passes somewhere it can clean up.
///
/// # Errors
///
/// The name is not usable, or the directory is not empty — the same
/// [`scaffold::Error`] words the dialog shows.
pub fn run_new(parent: &Path, name: &str, rust: bool) -> Result<String, String> {
    let kind = if rust {
        ProjectKind::Rust
    } else {
        ProjectKind::Say
    };
    // A path dependency on this checkout, because that is the only kind the
    // preview can load — the same choice `state/export.rs` makes for the
    // dialog, for the same reason.
    let dependency = scaffold::checkout_root().map_or_else(
        || Dependency::Version(env!("CARGO_PKG_VERSION").to_owned()),
        Dependency::Path,
    );
    let root = parent.join(name);
    let written =
        scaffold::create_for(&root, name, &dependency, kind).map_err(|error| error.to_string())?;
    Ok(if dependency.previewable() {
        format!(
            "created {} ({} files, {}) — vieww is a path dependency, so render works",
            root.display(),
            written.len(),
            kind.title()
        )
    } else {
        format!(
            "created {} ({} files, {}) — vieww is a published version, so web works and render will refuse",
            root.display(),
            written.len(),
            kind.title()
        )
    })
}

// ---- render (PNG) and html --------------------------------------------------

/// The result of [`compile_screen`]: everything both output paths need.
struct Mounted {
    preview: Preview,
    /// How long `rustc` took, for the summary line.
    rustc: Duration,
}

/// A seed no other live session in this process is using.
///
/// [`Session`] refuses two *live* sessions on one seed, because they share a
/// directory and delete each other's work — and a process id is per *process*,
/// so two compiles in one process (a CLI is one; a test binary running this
/// module's integration tests is several, in parallel threads) would collide
/// on it. The counter keeps them apart; the pid keeps two invocations of the
/// tool apart; the shift keeps the counter from ever reaching the pid's bits
/// (Linux pids stay under 2²², and the first counter value is 1, not 0, so
/// even a wrapping counter never reproduces the bare pid).
fn session_seed() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let call = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    u64::from(std::process::id()).wrapping_add(call << 24)
}

/// The preview pipeline without the preview pane: file → generated Rust (for
/// Say) → `rustc` → `dlopen` → a loaded [`Preview`].
///
/// This is `Studio::render_preview`'s decision tree — Say codegen with the
/// file's folder as the asset root, the component reach check, the project
/// rlib link, the ABI candidate walk — behind the synchronous [`compile`]
/// door the pipeline tests already drive. A CLI is a test that prints.
///
/// # Errors
///
/// One sentence per failure stage, in the studio's own words where they
/// exist: Say diagnostics name `.say` lines, rustc diagnostics name the
/// generated Rust's, refusals say what to run.
fn compile_screen(screen: &Screen) -> Result<Mounted, String> {
    let text = std::fs::read_to_string(&screen.path)
        .map_err(|error| format!("cannot read {} — {error}", screen.path.display()))?;
    let file_name = screen.path.file_name().map_or_else(
        || "screen.say".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    // **A Say buffer becomes Rust before anything else runs** — and the
    // asset root is the folder the `.say` file is in, exactly as the studio
    // passes it, so `an image "…"` resolves the same way here.
    let is_say = screen.path.extension().is_some_and(|e| e == "say");
    let (source, say_warnings) = if is_say {
        let options = SayOptions {
            asset_root: screen
                .path
                .parent()
                .map_or_else(String::new, |dir| dir.to_string_lossy().into_owned()),
        };
        match vieww_say_codegen::compile_with(&file_name, &text, &options) {
            Ok(generated) => (generated.rust, generated.warnings),
            Err(diagnostics) => {
                return Err(diagnostics_block(&say_diagnostics(&diagnostics)));
            }
        }
    } else {
        // Hand-written Rust: the entry-point check the studio refuses with
        // before rustc ever runs, because rustc cannot explain a missing
        // `screen()` against a line past the end of the file.
        if !compile::entry_point(&text) {
            let lines = text.lines().count().max(1) as u32;
            let diagnostic = compile::missing_entry_for(&file_name, lines, &text);
            return Err(diagnostics_block(&[diagnostic_line(&diagnostic)]));
        }
        (text, Vec::new())
    };
    if !say_warnings.is_empty() {
        // Warnings do not stop the render — the studio merges them into the
        // Problems panel and renders anyway, and a script wants the picture.
        eprintln!("{}", diagnostics_block(&say_diagnostics(&say_warnings)));
    }

    // The toolchain this binary was built with, found the way `main.rs`
    // finds it. The studio prints the fact and still opens; a script is told
    // the fix, because it has nothing else to do with one.
    let target = crate::install::target_dir();
    let base = Toolchain::discover(&target).map_err(|error| {
        format!(
            "no preview compiler — {error}. Build the workspace (cargo build -p viewwstudio) \
             so {} holds the rlibs this binary was linked against",
            target.display()
        )
    })?;
    let session = Session::new(session_seed())
        .map_err(|error| format!("cannot create a session directory to compile into — {error}"))?;

    // **The project's own library, when the screen reaches for it.** For
    // Say, the reach is a `<Name>` component reference — the one phrase
    // whose generated Rust is `crate::widgets::…` — the same gate
    // `Studio::render_preview` uses, for the same reason: linking nothing
    // costs nothing, and a Say screen without components never needs the
    // project. For Rust, the `crate::`/`use super::` heuristic the studio
    // uses (`state/export.rs::needs_project`), reimplemented because it is
    // `pub(super)` and one line.
    let project = if is_say {
        source
            .contains(PROJECT_REACH)
            .then(|| link_project(screen, &base))
    } else {
        (source.contains("crate::") || source.contains("use super::"))
            .then(|| link_project(screen, &base))
    };
    let project = match project {
        Some(result) => Some(result?),
        None => None,
    };

    // **The ABI candidate walk.** More than one `vieww` rlib can sit in the
    // target directory and only one of them is this binary's own
    // compilation; the studio advances a candidate index when a load comes
    // back mismatched, and the pipeline tests probe the same way. With a
    // project linked, the project's own `vieww` replaces the candidate
    // entirely (`Toolchain::with_vieww`), so the walk is a no-op there.
    let mut candidate = 0;
    loop {
        let toolchain = match &project {
            Some(project) => base.with_vieww(
                project
                    .vieww_rlib
                    .clone()
                    .unwrap_or_else(|| base.vieww_rlib.clone()),
            ),
            None => base.with_candidate(candidate).ok_or_else(|| {
                format!(
                    "no `vieww` rlib in {} matches this binary — build the workspace \
                     the way this binary was built (cargo build -p viewwstudio)",
                    target.display()
                )
            })?,
        };
        let compiled = match &project {
            Some(project) => {
                compile_project(&toolchain, &session, &source, &file_name, project, is_say)
            }
            None => compile(&toolchain, &session, &source, &file_name),
        }
        .map_err(|error| format!("rustc could not be run: {error}"))?;
        if compiled.failed() {
            return Err(diagnostics_block(&rustc_diagnostics(&compiled)));
        }
        let duration = compiled.duration;
        let library = compiled
            .library
            .clone()
            .ok_or_else(|| "rustc built no library".to_owned())?;
        if duration > TIMEOUT {
            // Not an error the studio surfaces (it cancels instead), but a
            // script wants to know the compile it waited for was pathological.
            eprintln!("note: rustc took {duration:.1?}, past the studio's {TIMEOUT:?} backstop");
        }
        match Preview::load(&library) {
            Ok(preview) => {
                return Ok(Mounted {
                    preview,
                    rustc: duration,
                })
            }
            Err(LoadError::AbiMismatch { .. }) if project.is_none() => {
                candidate += 1;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Find the project's built rlib, or refuse with the studio's own remedy.
fn link_project(screen: &Screen, base: &Toolchain) -> Result<ProjectLib, String> {
    let root = screen.project.as_deref().ok_or(
        "this screen reaches into the project around it (a component reference), \
         but it is not inside a cargo project",
    )?;
    let name = package_name(root).ok_or_else(|| {
        format!(
            "{} has a Cargo.toml with no package name in it",
            root.display()
        )
    })?;
    ProjectLib::discover(root, &name, &base.deps).ok_or_else(|| {
        format!(
            "this screen uses the project's own components, so it needs {name}'s library \
             built — run `cargo build` in {} once, and render will work from then on \
             without rebuilding it",
            root.display()
        )
    })
}

/// `render`: the loaded screen through the studio's own rasteriser.
///
/// `FrameDriver` + [`crate::install`] + a `Theme` wrapper is the exact mount
/// `say_ai::renders_stably` and `tests/canvas_edit.rs` both use — the one
/// render path the studio has, with its panic policy and its CPU scene.
///
/// # Errors
///
/// The raster refused, or the PNG could not be written.
pub fn run_render(
    input: &Path,
    out: Option<&Path>,
    size: (u32, u32),
    dark: bool,
) -> Result<String, String> {
    let screen = resolve_input(input)?;
    let mounted = compile_screen(&screen)?;
    let out = out.map_or_else(|| default_output(&screen, false), Path::to_path_buf);
    let (width, height) = (size.0.max(1), size.1.max(1));
    let theme = theme_data(dark);
    // A raster size in points fits `f32` exactly, so the cast loses nothing.
    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 pixel sizes are exact in f32"
    )]
    let width_points = width as f32;
    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 pixel sizes are exact in f32"
    )]
    let height_points = height as f32;
    let mut driver = FrameDriver::new(Size::new(width_points, height_points));
    crate::install(&mut driver);
    driver.set_root(Theme::new(theme).child(mounted.preview.node()));
    driver.draw_frame();
    let mut renderer = NativeRenderer::new();
    let (png, _) = renderer
        .render_to_png(driver.scene(), width, height, theme.colors.surface)
        .map_err(|error| format!("the headless render failed: {error}"))?;
    write_out(&out, &png)?;
    Ok(format!(
        "wrote {} ({}×{}, rustc {:.1?})",
        out.display(),
        width,
        height,
        mounted.rustc
    ))
}

/// `html`: the same loaded screen, walked by the DOM backend.
///
/// The mount is the one `mytlabs`' snapshot example uses — an `ElementTree`,
/// the screen under a `Theme`, [`walk`] — which is the same walk the live
/// wasm page does, minus the browser and the module: the tree *is* the page,
/// statically.
///
/// # Errors
///
/// As [`compile_screen`], or the page could not be written.
pub fn run_html(
    input: &Path,
    out: Option<&Path>,
    size: (u32, u32),
    dark: bool,
) -> Result<String, String> {
    let screen = resolve_input(input)?;
    let mounted = compile_screen(&screen)?;
    let out = out.map_or_else(|| default_output(&screen, true), Path::to_path_buf);
    let theme = theme_data(dark);
    let title = screen
        .path
        .file_stem()
        .map_or_else(|| "screen".to_owned(), |s| s.to_string_lossy().into_owned());
    let mut tree = ElementTree::new();
    let root = tree.mount(Theme::new(theme).child(mounted.preview.node()));
    let body: String = walk(&tree, root).iter().map(VNode::to_html).collect();
    let page = page_html(&title, &body, size.0, theme);
    write_out(&out, page.as_bytes())?;
    Ok(format!(
        "wrote {} ({}px wide, static DOM, rustc {:.1?})",
        out.display(),
        size.0,
        mounted.rustc
    ))
}

/// Write an artefact, making the directory it lands in first.
///
/// The default output is `<project>/target/render/`, which nothing creates
/// until the first `cargo build` — a scaffolder writes source files, and
/// `.gitignore` does not make directories. Refusing to write the picture a
/// compile just earned, because a folder cargo would have made is missing,
/// would be the exact kind of pedantry this tool exists to not be.
fn write_out(out: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {} — {error}", parent.display()))?;
    }
    std::fs::write(out, bytes).map_err(|error| format!("cannot write {} — {error}", out.display()))
}

/// The theme both output paths mount under. One definition, because two
/// would be one more than the studio has.
fn theme_data(dark: bool) -> ThemeData {
    if dark {
        ThemeData::dark()
    } else {
        ThemeData::light()
    }
}

/// The page a walked screen becomes.
///
/// Minimal on purpose: the screen's own elements carry their styling inline
/// (that is what the walk produces), so all the page owes is a viewport, a
/// title, a width to be the screen at, and the background the theme's
/// `surface` says the screen sits on. Anything cleverer belongs to the
/// project's own `web/index.html` — which is what `web` ships.
#[must_use]
pub fn page_html(title: &str, body: &str, width: u32, theme: ThemeData) -> String {
    let background = css_color(theme.colors.surface);
    let ink = css_color(theme.colors.on_surface);
    format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n\
         <!-- Static snapshot generated by viewwstudio-cli html: the same compiled\n\
              widget tree the studio previews, walked by vieww-platform-web-dom. The\n\
              browser lays it out; it is not interactive (handlers need the wasm\n\
              build — viewwstudio-cli web), and it is not a pixel copy of the\n\
              raster (that is viewwstudio-cli render). -->\n\
         <style>\n\
         html, body {{ margin: 0; }}\n\
         body {{ background: {background}; color: {ink}; \
         display: flex; justify-content: center; }}\n\
         #screen {{ width: {width}px; min-height: 100vh; overflow: hidden; }}\n\
         </style>\n\
         </head>\n\
         <body>\n\
         <div id=\"screen\">{body}</div>\n\
         </body>\n\
         </html>\n",
        title = escape_html(title),
        background = background,
        ink = ink,
        width = width,
        body = body,
    )
}

/// `#rrggbb`/`rgba(…)` for a [`Color`] — the walk's own `css_color` shape,
/// minus the colour-token table this page has no palette to fill.
fn css_color(color: Color) -> String {
    if color.a == 255 {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    } else {
        format!(
            "rgba({},{},{},{:.4})",
            color.r,
            color.g,
            color.b,
            f32::from(color.a) / 255.0
        )
    }
}

/// The five characters that end a text node or an attribute.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---- web ---------------------------------------------------------------------

/// `web`: the studio's Web export plan, run for real.
///
/// The plan is [`export::plan`] verbatim — the same steps the export sheet
/// previews, the same refusals with the same install lines — executed in
/// order with the first failure's exit status as the sentence a script
/// reads. Output streams as the children print it, because a wasm build is
/// slow enough that silence reads as a hang.
///
/// # Errors
///
/// A refusal (named, with the fix), or a step that failed.
pub fn run_web(project: &Path) -> Result<String, String> {
    let name = package_name(project).ok_or_else(|| {
        format!(
            "{} has no Cargo.toml with a package name in it — web exports a project, not a file",
            project.display()
        )
    })?;
    let env = toolchains::Env::host();
    let plan = export::plan(&env, project, &name, export::Format::Web)
        .map_err(|refusal| refusal.to_string())?;
    println!("{} — {} steps", plan.title, plan.steps.len());
    for step in &plan.steps {
        println!("  {} — {}", step.spec.label, step.spec.command_line());
    }
    for step in &plan.steps {
        run_spec(&step.spec).map_err(|why| format!("{} — {why}", step.spec.label))?;
    }
    Ok(format!(
        "finished — the interactive web app is {}",
        plan.artefact.display()
    ))
}

/// Run one [`Spec`] in the foreground, its output inherited.
///
/// The studio runs these through the jobs queue so the window keeps
/// painting; a CLI *is* the foreground, and a person watching a `cargo
/// build` wants its progress, not a spinner. No timeout of the CLI's own:
/// `Spec::timeout` is the studio's backstop, and a person can Ctrl-C.
fn run_spec(spec: &Spec) -> Result<(), String> {
    let mut command = Process::new(&spec.program);
    command.args(&spec.args).current_dir(&spec.dir);
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    let status = command
        .status()
        .map_err(|error| format!("could not start: {error}"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(format!("failed with exit code {code}")),
        None => Err("killed by a signal".to_owned()),
    }
}

// ---- screens -------------------------------------------------------------------

/// `screens`: the screens a `.say` file defines, one per line.
///
/// Read off the source rather than the generated Rust, because the source is
/// what the person editing it sees: a top-level `screen "Name":` block. The
/// shape is the grammar's own — no indent, the word `screen`, a quoted title
/// — and `strip_prefix` already refuses anything indented.
///
/// # Errors
///
/// The file cannot be read, or is not a `.say` file.
pub fn run_screens(input: &Path) -> Result<String, String> {
    let screen = resolve_input(input)?;
    if screen.path.extension().is_some_and(|e| e != "say") {
        return Err(format!(
            "{} is not a .say file — screens are a Say idea",
            screen.path.display()
        ));
    }
    let text = std::fs::read_to_string(&screen.path)
        .map_err(|error| format!("cannot read {} — {error}", screen.path.display()))?;
    let names: Vec<&str> = text
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("screen \"")?;
            Some(rest.strip_suffix("\":").unwrap_or(rest))
        })
        .collect();
    if names.is_empty() {
        return Err(format!(
            "no screens in {} — a screen block starts with `screen \"Name\":` at the left margin",
            screen.path.display()
        ));
    }
    Ok(names.join("\n"))
}

// ---- diagnostics, as a script reads them --------------------------------------

use vieww_platform_web_dom::tree::{walk, VNode};
use vieww_say_codegen::Diagnostic as SayDiagnostic;

/// Say diagnostics, as lines — the same fields the Problems panel shows,
/// with the `.say` file's own line and column, which is the only thing a
/// person (or a model, ST-03) can act on directly.
fn say_diagnostics(diagnostics: &[SayDiagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| {
            format!(
                "{}:{}:{} {}: {}",
                d.file, d.line, d.column, d.code, d.message
            )
        })
        .collect()
}

/// rustc's diagnostics, as lines.
fn rustc_diagnostics(compiled: &compile::Compiled) -> Vec<String> {
    compiled.diagnostics.iter().map(diagnostic_line).collect()
}

/// One studio [`Diagnostic`] as one line, help text included when there is
/// some — the help is where the remedy lives, and a CLI has no hover.
fn diagnostic_line(diagnostic: &Diagnostic) -> String {
    let base = format!(
        "{}:{}:{} {}: {}",
        diagnostic.file, diagnostic.line, diagnostic.column, diagnostic.code, diagnostic.message
    );
    match &diagnostic.help {
        Some(help) => format!("{base}\n  help: {help}"),
        None => base,
    }
}

/// A block of diagnostic lines, so a script's stderr keeps them together as
/// one failure.
fn diagnostics_block(lines: &[String]) -> String {
    if lines.is_empty() {
        "the compile failed".to_owned()
    } else {
        lines.join("\n")
    }
}

// ---- tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(words: &[&str]) -> Vec<String> {
        words.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn no_arguments_is_usage() {
        assert_eq!(parse(&[]), Err(String::new()));
    }

    #[test]
    fn help_is_usage() {
        for spelling in ["help", "--help", "-h"] {
            assert_eq!(parse(&arg(&[spelling])), Err(String::new()), "{spelling}");
        }
    }

    #[test]
    fn unknown_command_is_named() {
        assert!(parse(&arg(&["frobnicate"]))
            .unwrap_err()
            .contains("unknown command frobnicate"));
    }

    #[test]
    fn new_reads_the_name_and_the_kind() {
        assert_eq!(
            parse(&arg(&["new", "demo"])),
            Ok(Command::New {
                name: "demo".into(),
                rust: false
            })
        );
        assert_eq!(
            parse(&arg(&["new", "demo", "--rust"])),
            Ok(Command::New {
                name: "demo".into(),
                rust: true
            })
        );
        assert!(parse(&arg(&["new"])).is_err());
    }

    #[test]
    fn render_reads_every_flag() {
        let command = parse(&arg(&[
            "render", "home.say", "--out", "shot.png", "--width", "800", "--height", "600",
            "--dark",
        ]))
        .unwrap();
        match command {
            Command::Render {
                input,
                out,
                size,
                dark,
            } => {
                assert!(input.ends_with("home.say"));
                assert!(
                    input.is_absolute(),
                    "the input is resolved, not left relative"
                );
                assert_eq!(out, Some(absolute("shot.png").unwrap()));
                assert_eq!(size, (800, 600));
                assert!(dark);
            }
            other => panic!("a render, not {other:?}"),
        }
    }

    #[test]
    fn a_flag_value_that_looks_like_a_flag_is_refused() {
        assert!(parse(&arg(&["render", "home.say", "--out", "--dark"]))
            .unwrap_err()
            .contains("--out needs a path"));
    }

    #[test]
    fn render_flags_that_need_values_say_so() {
        assert!(parse(&arg(&["render", "home.say", "--width"]))
            .unwrap_err()
            .contains("--width needs a number"));
        assert!(parse(&arg(&["render", "home.say", "--width", "wide"]))
            .unwrap_err()
            .contains("wide is not one"));
        assert!(parse(&arg(&["render", "home.say", "--width", "0"]))
            .unwrap_err()
            .contains("at least 1"));
        assert!(parse(&arg(&["render", "home.say", "--spiral"]))
            .unwrap_err()
            .contains("does not take --spiral"));
    }

    #[test]
    fn usage_names_every_command() {
        let usage = usage();
        for command in ["new", "render", "html", "web", "screens"] {
            assert!(usage.contains(command), "usage mentions {command}");
        }
    }

    // ---- resolution --------------------------------------------------------

    #[test]
    fn a_loose_say_file_has_no_project() {
        let dir = std::env::temp_dir().join("vieww-cli-resolve");
        std::fs::create_dir_all(&dir).unwrap();
        let say = dir.join("loose.say");
        std::fs::write(&say, "").unwrap();
        let screen = resolve_input(&say).unwrap();
        assert_eq!(screen.path, say);
        assert!(screen.project.is_none(), "nothing above owns it");
    }

    #[test]
    fn a_project_directory_picks_home_over_alphabetical_first() {
        let root = std::env::temp_dir().join("vieww-cli-resolve-project");
        std::fs::remove_dir_all(&root).ok();
        let screens = root.join("src").join("screens");
        std::fs::create_dir_all(&screens).unwrap();
        std::fs::write(screens.join("aaa.say"), "").unwrap();
        std::fs::write(screens.join("home.say"), "").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        let screen = resolve_input(&root).unwrap();
        assert_eq!(screen.path, screens.join("home.say"));
        assert_eq!(screen.project.as_deref(), Some(root.as_path()));
    }

    #[test]
    fn an_empty_screens_directory_is_a_refusal_that_says_what_was_searched() {
        let root = std::env::temp_dir().join("vieww-cli-resolve-empty");
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("src").join("screens")).unwrap();
        let error = resolve_input(&root).unwrap_err();
        assert!(error.contains("src/screens"), "{error}");
    }

    #[test]
    fn the_default_output_lands_in_the_projects_target() {
        let root = PathBuf::from("/w/demo");
        let screen = Screen {
            path: root.join("src").join("screens").join("home.say"),
            project: Some(root.clone()),
        };
        assert_eq!(
            default_output(&screen, false),
            root.join("target").join("render").join("home.png")
        );
        assert_eq!(
            default_output(&screen, true),
            root.join("target").join("render").join("home.html")
        );
        let loose = Screen {
            path: PathBuf::from("/w/loose.say"),
            project: None,
        };
        assert_eq!(default_output(&loose, false), PathBuf::from("/w/loose.png"));
    }

    #[test]
    fn the_package_name_is_read_from_the_manifest() {
        let root = std::env::temp_dir().join("vieww-cli-manifest");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n\n[dependencies]\nvieww = \"0\"\n",
        )
        .unwrap();
        assert_eq!(package_name(&root).as_deref(), Some("my-app"));
        // A `name` in the wrong table is not the package's name.
        std::fs::write(
            root.join("Cargo.toml"),
            "[dependencies]\nname = \"not-the-package\"\n",
        )
        .unwrap();
        assert_eq!(package_name(&root), None);
    }

    // ---- the page ------------------------------------------------------------

    #[test]
    fn the_page_carries_the_body_and_escapes_the_title() {
        let page = page_html(
            "Home & \"Away\"",
            "<span style=\"display:block;\">Hello</span>",
            393,
            ThemeData::light(),
        );
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<title>Home &amp; &quot;Away&quot;</title>"));
        assert!(page.contains("id=\"screen\"><span style=\"display:block;\">Hello</span>"));
        assert!(page.contains("width: 393px"));
        assert!(
            page.contains("#fffbfe"),
            "the theme's surface is the background"
        );
    }

    // ---- screens ----------------------------------------------------------------

    #[test]
    fn screens_lists_top_level_blocks_only() {
        let dir = std::env::temp_dir().join("vieww-cli-screens");
        std::fs::create_dir_all(&dir).unwrap();
        let say = dir.join("flow.say");
        std::fs::write(
            &say,
            "keep a number called n starting at 1\n\nscreen \"Home\":\n    a label \"hi\"\n\nscreen \"Settings\":\n    a label \"bye\"\n",
        )
        .unwrap();
        assert_eq!(run_screens(&say).unwrap(), "Home\nSettings");
    }

    #[test]
    fn a_file_with_no_screens_names_the_shape_it_wants() {
        let dir = std::env::temp_dir().join("vieww-cli-screens-empty");
        std::fs::create_dir_all(&dir).unwrap();
        let say = dir.join("notes.say");
        std::fs::write(&say, "a column:\n    a label \"not a screen\"\n").unwrap();
        let error = run_screens(&say).unwrap_err();
        assert!(error.contains("screen \"Name\""), "{error}");
    }

    #[test]
    fn a_missing_file_is_named() {
        let error = resolve_input(Path::new("/nowhere/never.say")).unwrap_err();
        assert!(error.contains("does not exist"), "{error}");
    }

    #[test]
    fn a_wrong_extension_is_refused_with_its_own() {
        let dir = std::env::temp_dir().join("vieww-cli-extension");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, "").unwrap();
        let error = resolve_input(&file).unwrap_err();
        assert!(error.contains(".txt file"), "{error}");
    }
}
