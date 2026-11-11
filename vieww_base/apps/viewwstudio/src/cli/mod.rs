//! The studio, without the window: `viewwstudio-cli`.
//!
//! # What this module is
//!
//! The command-line face of the **same engine** the windowed studio runs —
//! not a screenshot utility that happens to share its name. Everything the
//! preview pane, the export sheet, the device row, the New Project dialog
//! and the Problems panel do is library code this crate already owns, and
//! this module is the one entry point for people who want the studio's
//! *capabilities* from a script, a CI job, or a terminal:
//!
//! | Capability           | The studio                    | The CLI                              |
//! |----------------------|-------------------------------|--------------------------------------|
//! | Create projects      | New Project dialog            | `new` / `create`                     |
//! | New screens/widgets  | New Screen/Widget from Template | `screen` / `widget`                |
//! | Frames and screenshots | Preview, export             | `render` (one screen or `--all-screens`) |
//! | Device frames        | the pane's bezel, Save Framed Screenshot | `--frame` on `render`/`animate` |
//! | Device matrix        | Several Devices Side by Side, Save Device Matrix Image | `render --matrix` / `--devices` |
//! | Animation            | the preview, live; Start/Stop Recording | `animate` — the same tree over time, as a GIF, driven by `--tap`/`--do`/`--script` |
//! | Interactive HTML     | the wasm export; Share on Network | `preview` (served, live; `--share` for a phone, with a QR code) and `export --format web` |
//! | Static HTML          | — (the DOM walk)              | `html` — selectable, searchable DOM |
//! | Mobile interfaces    | the device row                | `--device`, on every visual command |
//! | Gestures             | the preview pane              | `preview` (real events) and `test` (injected ones) |
//! | Themes               | preview dark mode             | `--dark`, on every visual command   |
//! | Assets               | the Assets sidebar            | `assets`                             |
//! | Testing              | eyes on the preview           | `test` — determinism, overflow, inputs, goldens |
//! | Export               | the export sheet              | `export` — all nine formats, or `--all` |
//! | Devices              | the device picker             | `devices`                            |
//!
//! The architectural rule the table encodes: **there is one engine, and two
//! ways to drive it.** The compile is the studio's own [`compile`] door —
//! the ABI fingerprint guard, `-C prefer-dynamic`, the candidate search and
//! the project-rlib link all behave exactly as they do behind the Render
//! button — the scaffold is the same files the New Project dialog writes,
//! the export plan is the same `Spec` list the export sheet previews, the
//! devices are the preview pane's own table, and the interactive preview is
//! the same out-of-process child (`viewwstudio-preview`) the pane loads a
//! guest into, driven over a socket instead of a canvas. A bug fixed for
//! the window is fixed here; a refusal the studio makes, this makes with
//! the same words.
//!
//! # The honest limits
//!
//! * `render` proves the screen renders, once, at the size asked — the same
//!   claim ST-03's double-render verification makes, minus the second frame
//!   (that second frame is `test`'s first check).
//! * `html` is the DOM backend's translation of the tree, and the browser
//!   lays it out — it is *not* a pixel copy of the raster, and it is not
//!   interactive: handlers exist in the tree but only the wasm build runs
//!   them. What it is, the canvas backend is not: selectable, searchable,
//!   readable by a crawler.
//! * `animate` is time passing over the mounted tree. A Say screen has no
//!   time-driven vocabulary (no tickers — which is why `test`'s determinism
//!   check works), so its animation is in its *transitions*, which need
//!   events: give it them with `--tap`, `--do` or `--script` (the studio's
//!   recorder writes those scripts), and the GIF shows the transition. A Rust
//!   screen with a `Ticker`, a spring or a scroll-driven animation animates
//!   with or without them.
//! * `preview --share` serves one shared session: everyone who opens the
//!   link drives the same preview, and only machines on the same network can
//!   reach it. A link for anyone, anywhere is the web export, hosted.
//! * `preview` serves the compiled screen at `http://127.0.0.1:<port>` —
//!   pointer, scroll and key events flow from the browser into the same
//!   event path the window uses, and frames flow back. It needs the
//!   `viewwstudio-preview` binary beside this one (`cargo build -p
//!   viewwstudio --bins`), because a guest that aborts must cost the server
//!   a frame, not the process.
//! * `web`/`export --format web` needs the wasm toolchain (`rustup target
//!   add wasm32-unknown-unknown`, `wasm-bindgen`); the planner's refusal
//!   says so in its own words rather than failing halfway through a build.
//! * Golden images in `test` are per-machine: renders use the system's
//!   fonts, and two machines with different fonts render different bytes.
//!   The default golden directory is under `target/` — regenerated, not
//!   committed — for exactly that reason.

mod animate;
mod devices;
mod export_cmd;
mod preview;
mod project_cmd;
mod test_cmd;

pub use animate::run_animate;
pub use devices::{device_flag, run_devices};
pub use export_cmd::{run_export, run_web, ExportName};
pub use preview::run_preview;
pub use project_cmd::{run_assets, run_screen, run_screen_from, run_widget, run_widget_from};
pub use test_cmd::run_test;

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::Duration;

use vieww_element::ElementTree;
use vieww_foundation::{Color, Size};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;
use vieww_say_codegen::Options as SayOptions;
use vieww_widget::{Theme, ThemeData, WidgetNode};

use crate::compile::{self, compile, compile_project, ProjectLib, Session, Toolchain, TIMEOUT};
use crate::components::PROJECT_REACH;
use crate::loaded::{LoadError, Preview};
use crate::scaffold::{self, Dependency, ProjectKind};
use crate::state::Diagnostic;
use crate::task::Spec;

/// The size a visual command runs at when nobody chose one: the studio
/// preview's own phone, the `393×852` of an iPhone (`ui/preview.rs`), rather
/// than a round number nobody ships.
pub const DEFAULT_SIZE: (u32, u32) = (393, 852);

// ---- the command line ------------------------------------------------------

/// One parsed invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `new`/`create` — scaffold a project.
    New {
        /// The project's name, and the directory written.
        name: String,
        /// A Rust project rather than a Say one.
        rust: bool,
    },
    /// `screen` — a new screen file, from the New Screen template.
    Screen {
        /// The snake_case module name, checked the way the dialog checks it.
        name: String,
    },
    /// `widget` — a new widget file, from the New Widget template.
    Widget {
        /// The snake_case module name.
        name: String,
    },
    /// `render` — the preview pipeline to a PNG.
    Render {
        /// A `.say`/`.rs` file, or a project directory.
        input: PathBuf,
        /// Where the PNG lands; `None` for the default, which needs the
        /// input resolved before it can be named.
        out: Option<PathBuf>,
        /// The size, and the device whose metrics the screen is told.
        viewport: Viewport,
        /// The dark theme rather than the light one.
        dark: bool,
        /// Start on this screen (a Say idea).
        screen: Option<String>,
        /// One PNG per screen the file defines.
        all_screens: bool,
        /// Frame, background, interaction and matrix.
        present: Presentation,
    },
    /// `html` — the same tree as a static DOM page.
    Html {
        /// A `.say`/`.rs` file, or a project directory.
        input: PathBuf,
        /// Where the page lands; `None` for the default.
        out: Option<PathBuf>,
        /// The page's width, in CSS pixels, and the device behind it.
        viewport: Viewport,
        /// The dark theme rather than the light one.
        dark: bool,
        /// Start on this screen (a Say idea).
        screen: Option<String>,
    },
    /// `animate` — the screen over time, as a GIF.
    Animate {
        /// A `.say`/`.rs` file, or a project directory.
        input: PathBuf,
        /// Where the GIF lands; `None` for the default.
        out: Option<PathBuf>,
        /// The raster's size, and the device behind it.
        viewport: Viewport,
        /// The dark theme rather than the light one.
        dark: bool,
        /// Start on this screen (a Say idea).
        screen: Option<String>,
        /// Frames per second to sample at.
        fps: u32,
        /// How long the sampled window is.
        duration: Duration,
        /// Also write the PNG frames here, for video encoders.
        frames_dir: Option<PathBuf>,
        /// Frame, background and interaction.
        present: Presentation,
    },
    /// `preview` — the interactive preview, served for a browser.
    Preview {
        /// A `.say`/`.rs` file, or a project directory; `None` means the
        /// project around the current directory.
        input: Option<PathBuf>,
        /// The device to frame the screen in — the preview pane's own
        /// simulation, default its phone.
        device: crate::state::Device,
        /// The dark theme rather than the light one.
        dark: bool,
        /// Start on this screen (a Say idea).
        screen: Option<String>,
        /// The port to serve on.
        port: u16,
        /// The raster scale of served frames.
        scale: f32,
        /// Try to open a browser at the served URL.
        open: bool,
        /// Serve to the local network, with a QR code for a phone.
        share: bool,
    },
    /// `export` — the export sheet, run for real.
    Export {
        /// The project directory; `None` means the one around the current
        /// directory.
        project: Option<PathBuf>,
        /// Which format; `None` with neither `--all` nor `--list` is a
        /// usage error, caught in `parse`.
        format: Option<ExportName>,
        /// Every format the machine can plan.
        all: bool,
        /// List the formats and what each needs, building nothing.
        list: bool,
    },
    /// `web` — the studio's wasm32 export, run for real. Kept as its own
    /// spelling because it is the one format that is also a *page*.
    Web {
        /// A project directory (the one with the `Cargo.toml`).
        project: PathBuf,
    },
    /// `test` — compile, render, exercise, compare.
    Test {
        /// A `.say`/`.rs` file, or a project directory; `None` means the
        /// project around the current directory.
        input: Option<PathBuf>,
        /// The size, and the device whose metrics the screen is told.
        viewport: Viewport,
        /// The dark theme rather than the light one.
        dark: bool,
        /// Where golden images live; `None` for the project's default.
        goldens: Option<PathBuf>,
        /// Write the goldens instead of comparing them.
        update: bool,
    },
    /// `screens` — the screens a `.say` file defines.
    Screens {
        /// A `.say` file, or a project directory.
        input: PathBuf,
    },
    /// `devices` — the presets `--device` takes.
    Devices,
    /// `assets` — the project's asset bundle.
    Assets {
        /// A project directory; `None` means the one around the current
        /// directory.
        project: Option<PathBuf>,
    },
}

/// How to call the tool, as text. The whole of the argument-error path: a
/// parse failure prints this and nothing else, because a usage message that
/// also half-runs a subcommand is a usage message nobody trusts.
#[must_use]
pub fn usage() -> String {
    format!(
        "the studio, without the window — the same engine, as commands

USAGE:
    viewwstudio-cli new <name> [--rust]          scaffold a project into ./<name>
                                                 (\"create\" is the same command)
    viewwstudio-cli screen <name>                a new screen file, from the template
    viewwstudio-cli widget <name>                a new widget file, from the template
    viewwstudio-cli render <path> [options]      a .say screen (or project) to a PNG
    viewwstudio-cli render <path> --all-screens  one PNG per screen the file defines
    viewwstudio-cli render <path> --matrix       the screen on several devices, side by side
    viewwstudio-cli html <path> [options]        the same screen as static HTML
    viewwstudio-cli animate <path> [options]     the screen over time, as a GIF
    viewwstudio-cli preview [path] [options]     the interactive preview, in a browser
    viewwstudio-cli export [project] [options]   the export sheet: every format
    viewwstudio-cli web <project>                the interactive wasm web export
    viewwstudio-cli test [path] [options]        compile, render, exercise, compare
    viewwstudio-cli screens <path>               list the screens a .say file defines
    viewwstudio-cli devices                      the device presets --device takes
    viewwstudio-cli assets [project]             the project's asset bundle

    <path> is a .say file, a .rs screen, or a project directory (its
    src/screens/ is searched; home.say wins when several are there).
    preview, test, export and assets take the project around the current
    directory when no path is given.

DEVICE OPTIONS (render/html/animate/preview/test):
    --device <name>   a studio preset, by name or short form: mobile, phone,
                      phone-small, phone-large, tablet, android, android-tablet,
                      laptop, desktop — `devices` lists them all. Sets the size
                      *and* the metrics the screen is told (density, safe area,
                      platform), which is what the preview pane does.
    --landscape       the device on its side (needs --device)
    --width <px>      raster width  (default: the device's, else {width})
    --height <px>     raster height (default: the device's, else {height})
    --dark            the dark theme rather than the light one
    --screen <title>  start on a named screen (a Say idea; `screens` lists them)

RENDER/HTML:
    --out <file>      where the result lands
                      (default: <project>/target/render/<name>.png|.html)

PRESENTATION (render/animate):
    --frame           draw the device around the screen — the preview pane's
                      own bezel, notch and home indicator (needs --device)
    --background <c>  #RRGGBB, or transparent (default: transparent for a
                      PNG, the preview stage for a GIF or a matrix)
    --matrix          render: the screen on the pane's matrix devices (phone,
                      tablet, desktop), each framed, side by side in one PNG
    --devices <a,b>   render: which devices the matrix shows (implies --matrix)
    --settle <t>      render: how long to wait after the last step (default 800ms)

INTERACTION (render/animate) — what happens before the picture, or during
the animation. Steps run in order; a step with no @time comes 800ms after
the one before it:
    --tap <target>    tap a label (Details, or \"Add one\") or a point (120,300)
    --do <step>       any step: tap \"Add one\" @1s
                                hold \"Item\" for 800ms @2s
                                drag 200,600 -> 200,200 over 400ms @3s
                                scroll -300 [at 200,400] @4s
                                key enter|tab|escape|backspace|left|... @5s
                                type \"hello\" @6s
    --script <file>   one step per line, the same syntax; the studio's
                      Record command writes these

ANIMATE:
    --fps <n>         frames per second to sample at (default 25; a number
                      that divides 100 keeps GIF timing exact)
    --duration <t>    how long the window is: 5s, 250ms, 5 (default 3s)
    --out <file>      where the GIF lands (default: target/render/<name>.gif)
    --frames-dir <d>  also write the PNG frames there, for video encoders

PREVIEW:
    --port <n>        the port to serve on (default {port})
    --scale <n>       the raster scale of served frames (default 1)
    --no-open         do not try to open a browser
    --share           serve to the local network too, and show a QR code a
                      phone on the same Wi-Fi can scan

EXPORT:
    --format <name>   desktop, mac-app, deb, windows, web, apk, apk-release,
                      ios-simulator, ipa
    --all             every format this machine can plan; refusals are
                      listed, not fatal
    --list            what each format would need, and what is missing

TEST:
    --update-goldens  write the golden images instead of comparing them
    --goldens <dir>   where goldens live
                      (default: <project>/target/render/goldens)

The render paths need the toolchain the studio itself was built with —
build the workspace (cargo build -p viewwstudio --bins) first, the same
requirement the preview pane has.",
        width = DEFAULT_SIZE.0,
        height = DEFAULT_SIZE.1,
        port = preview::DEFAULT_PORT,
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
        "new" | "create" => {
            let name = positional(rest).ok_or_else(|| {
                format!("{first} needs a name — viewwstudio-cli {first} <name> [--rust]")
            })?;
            Ok(Command::New {
                name: name.clone(),
                rust: rest.iter().any(|a| a == "--rust"),
            })
        }
        "screen" | "widget" => {
            let name = positional(rest)
                .ok_or_else(|| format!("{first} needs a name — viewwstudio-cli {first} <name>"))?;
            Ok(if first == "screen" {
                Command::Screen { name: name.clone() }
            } else {
                Command::Widget { name: name.clone() }
            })
        }
        "render" | "html" | "animate" => parse_visual(first, rest),
        "preview" => parse_preview(rest),
        "export" => parse_export(rest),
        "web" => {
            let project = positional(rest).ok_or_else(|| {
                "web needs a project directory — the one with the Cargo.toml".to_owned()
            })?;
            Ok(Command::Web {
                project: absolute(&project)?,
            })
        }
        "test" => parse_test(rest),
        "screens" => {
            let input = positional(rest)
                .ok_or_else(|| "screens needs a .say file or a project directory".to_owned())?;
            Ok(Command::Screens {
                input: absolute(&input)?,
            })
        }
        "devices" => Ok(Command::Devices),
        "assets" => Ok(Command::Assets {
            project: match positional(rest) {
                Some(path) => Some(absolute(&path)?),
                None => None,
            },
        }),
        "help" | "--help" | "-h" => Err(String::new()),
        other => Err(format!("unknown command {other}")),
    }
}

/// The first argument that is not a flag — the path or name a command
/// acts on, in the position the usage shows it.
fn positional(rest: &[String]) -> Option<String> {
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        if arg.starts_with("--") {
            // A value-taking flag's value is not the positional, even when
            // it comes first: `render --do 'tap "A"' home.say`.
            if VALUE_FLAGS.contains(&arg.as_str()) {
                args.next();
            }
            continue;
        }
        return Some(arg.clone());
    }
    None
}

/// Every flag that consumes the argument after it, across all commands.
const VALUE_FLAGS: [&str; 18] = [
    "--out",
    "--device",
    "--devices",
    "--width",
    "--height",
    "--screen",
    "--fps",
    "--duration",
    "--frames-dir",
    "--port",
    "--scale",
    "--format",
    "--goldens",
    "--background",
    "--do",
    "--tap",
    "--script",
    "--settle",
];

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

// ---- the flags the visual commands share ------------------------------------

/// One pass over a command's flags, in order, values consumed with their
/// flag — the shape the original `render` parser had, factored out because
/// five commands now share it.
struct Flags<'a> {
    args: std::slice::Iter<'a, String>,
}

impl<'a> Flags<'a> {
    fn new(args: &'a [String]) -> Self {
        Self { args: args.iter() }
    }

    /// The next flag, or `None` at the end of the list. Positionals are
    /// skipped, not errors — they were collected separately, and the usage
    /// shows them before the flags; a tool that refused `render home.say
    /// --dark` because `home.say` "is not a flag" would be unbearable.
    fn next(&mut self) -> Option<&'a str> {
        self.args
            .find(|arg| arg.starts_with("--"))
            .map(String::as_str)
    }

    /// The value a flag carries, refusing one that is another flag.
    fn value(&mut self, flag: &str) -> Result<&'a str, String> {
        match self.args.next() {
            Some(value) if !value.starts_with("--") => Ok(value),
            _ => Err(format!("{flag} needs a value after it")),
        }
    }

    /// A `--flag <number>`, parsed and bounded.
    fn number(&mut self, flag: &str, min: f64, max: f64) -> Result<f64, String> {
        let raw = self.value(flag)?;
        let value: f64 = raw
            .parse()
            .map_err(|_| format!("{flag} needs a number — {raw} is not one"))?;
        if value < min || value > max {
            return Err(format!(
                "{flag} must be between {min} and {max} — {raw} is not"
            ));
        }
        Ok(value)
    }
}

/// Where a visual command renders: a size, and optionally the device whose
/// metrics (density, safe area, platform conventions) the screen is told.
///
/// The device half is opt-in on purpose: a plain size is what every `render`
/// produced before `--device` existed, and those scripts' PNGs must not
/// change bytes because a table gained a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The preset that was named, already rotated when `--landscape` was
    /// passed. `None` is a plain size.
    pub device: Option<crate::state::Device>,
    /// The raster's size, in pixels.
    pub size: (u32, u32),
}

impl Viewport {
    /// The default: the preview's own phone, as points — no metrics
    /// simulation, the behaviour `render` has always had.
    #[must_use]
    pub const fn plain() -> Self {
        Self {
            device: None,
            size: DEFAULT_SIZE,
        }
    }

    /// A viewport from a device preset, rotated by `landscape`.
    #[must_use]
    pub fn of_device(device: crate::state::Device, landscape: bool) -> Self {
        let device = if landscape { device.rotated() } else { device };
        Self {
            // A device's points are whole numbers in the table, and the
            // raster is one pixel per point.
            size: (device.width.round() as u32, device.height.round() as u32),
            device: Some(device),
        }
    }
}

/// How a visual result is presented, beyond the screen itself: in a device
/// frame, on a background, after an interaction, across several devices.
///
/// One struct for the flags `render` and `animate` share, so their command
/// variants and run functions grow by one field rather than five.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Presentation {
    /// Draw the preview pane's device frame around the screen (`--frame`).
    pub frame: bool,
    /// `--background`: `None` for the default (transparent for a PNG, the
    /// stage colour for a GIF or a matrix), `Some(None)` for transparent.
    pub background: Option<Option<Color>>,
    /// The interaction to play (`--do`, `--tap`, `--script`).
    pub script: crate::showcase::Script,
    /// `--matrix`: render on each of these devices, framed, side by side.
    pub matrix: Option<Vec<crate::state::Device>>,
    /// How long to let the screen settle after the script's last step
    /// before a still is taken (`--settle`).
    pub settle: Duration,
}

impl Presentation {
    /// The settle when nobody chose one: long enough for a 300 ms transition
    /// and a spring's tail.
    pub const DEFAULT_SETTLE: Duration = Duration::from_millis(800);
}

/// Parse the flags `render`, `html` and `animate` share, plus each one's
/// own.
fn parse_visual(command: &str, rest: &[String]) -> Result<Command, String> {
    let input = positional(rest).ok_or_else(|| {
        format!("{command} needs a .say file, a .rs screen, or a project directory")
    })?;
    let input = absolute(&input)?;

    let mut out = None;
    let mut device: Option<String> = None;
    let mut landscape = false;
    let mut width = None;
    let mut height = None;
    let mut dark = false;
    let mut screen: Option<String> = None;
    let mut all_screens = false;
    let mut fps = animate::DEFAULT_FPS;
    let mut duration = animate::DEFAULT_DURATION;
    let mut frames_dir = None;
    let mut present = Presentation {
        settle: Presentation::DEFAULT_SETTLE,
        ..Presentation::default()
    };
    let mut matrix = false;
    let mut matrix_devices: Option<String> = None;

    let mut flags = Flags::new(rest);
    while let Some(flag) = flags.next() {
        match flag {
            "--dark" => dark = true,
            "--landscape" => landscape = true,
            "--all-screens" => all_screens = true,
            "--out" => {
                out = Some(absolute(flags.value("--out")?)?);
            }
            "--device" => device = Some(flags.value("--device")?.to_owned()),
            "--screen" => screen = Some(flags.value("--screen")?.to_owned()),
            "--width" => width = Some(flags.number("--width", 1.0, 65_535.0)? as u32),
            "--height" => height = Some(flags.number("--height", 1.0, 65_535.0)? as u32),
            "--fps" => fps = flags.number("--fps", 1.0, 60.0)? as u32,
            "--duration" => duration = animate::parse_duration(flags.value("--duration")?)?,
            "--frames-dir" => frames_dir = Some(absolute(flags.value("--frames-dir")?)?),
            "--frame" if command != "html" => present.frame = true,
            "--background" if command != "html" => {
                present.background = Some(crate::showcase::parse_background(
                    flags.value("--background")?,
                )?);
            }
            "--matrix" if command == "render" => matrix = true,
            "--devices" if command == "render" => {
                matrix = true;
                matrix_devices = Some(flags.value("--devices")?.to_owned());
            }
            "--do" if command != "html" => present.script.push_line(flags.value("--do")?)?,
            "--tap" if command != "html" => {
                let target = flags.value("--tap")?;
                // A bare word is a label: `--tap Details` means the button,
                // and only a quoted or x,y target is passed through as is.
                let target = if target.starts_with('"') || target.contains(',') {
                    target.to_owned()
                } else {
                    format!("\"{target}\"")
                };
                present.script.push_line(&format!("tap {target}"))?;
            }
            "--script" if command != "html" => {
                let path = absolute(flags.value("--script")?)?;
                let text = std::fs::read_to_string(&path)
                    .map_err(|error| format!("cannot read {} — {error}", path.display()))?;
                let script = crate::showcase::Script::parse(&text)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                for step in script.steps {
                    if present
                        .script
                        .steps
                        .last()
                        .is_some_and(|last| step.at < last.at)
                    {
                        return Err(format!(
                            "{} starts before the steps given ahead of it — put --script first",
                            path.display()
                        ));
                    }
                    present.script.steps.push(step);
                }
            }
            "--settle" if command == "render" => {
                present.settle = animate::parse_duration(flags.value("--settle")?)?;
            }
            other => return Err(format!("{command} does not take {other}")),
        }
    }

    // The device decides the size; an explicit size decides it too; both at
    // once is two answers to one question, and the error says whose to drop.
    if device.is_some() && (width.is_some() || height.is_some()) {
        return Err(
            "--device brings its own size — drop --width/--height, or name the size and not the device".to_owned(),
        );
    }
    let viewport = match (&device, width, height) {
        (Some(name), _, _) => Viewport::of_device(devices::by_name(name)?, landscape),
        (None, Some(w), Some(h)) => {
            if landscape {
                return Err("--landscape turns a device on its side — it needs --device".to_owned());
            }
            Viewport {
                device: None,
                size: (w, h),
            }
        }
        (None, Some(_), None) | (None, None, Some(_)) => {
            return Err("--width and --height come together — one alone is not a size".to_owned());
        }
        (None, None, None) => {
            if landscape {
                return Err("--landscape turns a device on its side — it needs --device".to_owned());
            }
            Viewport::plain()
        }
    };

    if matrix {
        if device.is_some() {
            return Err(
                "--matrix renders on several devices — name them with --devices, not --device"
                    .to_owned(),
            );
        }
        if all_screens {
            return Err("--matrix and --all-screens each make many pictures — pick one".to_owned());
        }
        let devices = match &matrix_devices {
            Some(list) => list
                .split(',')
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(|name| devices::by_name(name).map(|d| if landscape { d.rotated() } else { d }))
                .collect::<Result<Vec<_>, _>>()?,
            None => crate::state::Studio::default_matrix(),
        };
        if devices.is_empty() {
            return Err("--devices needs at least one device — `devices` lists them".to_owned());
        }
        present.matrix = Some(devices);
        present.frame = true;
    }
    if present.frame && viewport.device.is_none() && present.matrix.is_none() {
        return Err(
            "--frame draws a device around the screen — it needs --device to know which".to_owned(),
        );
    }

    Ok(match command {
        "render" => Command::Render {
            input,
            out,
            viewport,
            dark,
            screen,
            all_screens,
            present,
        },
        "html" => Command::Html {
            input,
            out,
            viewport,
            dark,
            screen,
        },
        _ => Command::Animate {
            input,
            out,
            viewport,
            dark,
            screen,
            fps,
            duration,
            frames_dir,
            present,
        },
    })
}

/// `preview`'s own parse: the device defaults to the preview pane's phone,
/// because a preview without a device is the one thing the pane never shows.
fn parse_preview(rest: &[String]) -> Result<Command, String> {
    let input = match positional(rest) {
        Some(path) => Some(absolute(&path)?),
        None => None,
    };

    let mut device = devices::PREVIEW_DEFAULT.to_owned();
    let mut landscape = false;
    let mut dark = false;
    let mut screen = None;
    let mut port = preview::DEFAULT_PORT;
    let mut scale = 1.0_f32;
    let mut open = true;
    let mut share = false;

    let mut flags = Flags::new(rest);
    while let Some(flag) = flags.next() {
        match flag {
            "--dark" => dark = true,
            "--landscape" => landscape = true,
            "--no-open" => open = false,
            "--share" => share = true,
            "--device" => device = flags.value("--device")?.to_owned(),
            "--screen" => screen = Some(flags.value("--screen")?.to_owned()),
            "--port" => port = flags.number("--port", 1.0, 65_535.0)? as u16,
            "--scale" => scale = flags.number("--scale", 0.25, 4.0)? as f32,
            "--out" | "--width" | "--height" => {
                return Err(
                    "preview serves a page rather than writing a file — there is no --out"
                        .to_owned(),
                );
            }
            other => return Err(format!("preview does not take {other}")),
        }
    }
    let device = if landscape {
        devices::by_name(&device)?.rotated()
    } else {
        devices::by_name(&device)?
    };
    Ok(Command::Preview {
        input,
        device,
        dark,
        screen,
        port,
        scale,
        open,
        share,
    })
}

/// `export`'s own parse: a format, or `--all`, or `--list` — and nothing,
/// which is no format to run, is a usage error rather than a guess.
fn parse_export(rest: &[String]) -> Result<Command, String> {
    let project = match positional(rest) {
        Some(path) => Some(absolute(&path)?),
        None => None,
    };
    let mut format = None;
    let mut all = false;
    let mut list = false;

    let mut flags = Flags::new(rest);
    while let Some(flag) = flags.next() {
        match flag {
            "--all" => all = true,
            "--list" => list = true,
            "--format" => format = Some(ExportName::parse(flags.value("--format")?)?),
            other => return Err(format!("export does not take {other}")),
        }
    }
    if format.is_some() && all {
        return Err("--format names one format and --all runs them all — pick one".to_owned());
    }
    if format.is_none() && !all && !list {
        return Err(
            "export needs --format <name>, --all, or --list — nothing to run was named".to_owned(),
        );
    }
    Ok(Command::Export {
        project,
        format,
        all,
        list,
    })
}

/// `test`'s own parse.
fn parse_test(rest: &[String]) -> Result<Command, String> {
    let input = match positional(rest) {
        Some(path) => Some(absolute(&path)?),
        None => None,
    };

    let mut device: Option<String> = None;
    let mut landscape = false;
    let mut width = None;
    let mut height = None;
    let mut dark = false;
    let mut goldens = None;
    let mut update = false;

    let mut flags = Flags::new(rest);
    while let Some(flag) = flags.next() {
        match flag {
            "--dark" => dark = true,
            "--landscape" => landscape = true,
            "--update-goldens" => update = true,
            "--device" => device = Some(flags.value("--device")?.to_owned()),
            "--goldens" => goldens = Some(absolute(flags.value("--goldens")?)?),
            "--width" => width = Some(flags.number("--width", 1.0, 65_535.0)? as u32),
            "--height" => height = Some(flags.number("--height", 1.0, 65_535.0)? as u32),
            other => return Err(format!("test does not take {other}")),
        }
    }
    if device.is_some() && (width.is_some() || height.is_some()) {
        return Err(
            "--device brings its own size — drop --width/--height, or name the size and not the device".to_owned(),
        );
    }
    let viewport = match (&device, width, height) {
        (Some(name), _, _) => Viewport::of_device(devices::by_name(name)?, landscape),
        (None, Some(w), Some(h)) => Viewport {
            device: None,
            size: (w, h),
        },
        (None, Some(_), None) | (None, None, Some(_)) => {
            return Err("--width and --height come together — one alone is not a size".to_owned());
        }
        (None, None, None) => Viewport::plain(),
    };
    Ok(Command::Test {
        input,
        viewport,
        dark,
        goldens,
        update,
    })
}

// ---- shared resolution -----------------------------------------------------

/// What a visual command decided to compile: a file, and the cargo project
/// it sits in (if any) — the project decides both the default output path
/// and whether a component reference can be linked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Screen {
    /// The `.say` or `.rs` file to compile.
    pub(crate) path: PathBuf,
    /// The nearest ancestor with a `Cargo.toml`, if any.
    pub(crate) project: Option<PathBuf>,
}

/// Resolve the `<path>` argument into a file to compile.
///
/// A `.say`/`.rs` file is taken as-is. A directory is searched the way the
/// studio's own workspace is laid out: `src/screens/`, where the scaffold
/// puts every screen and where New Screen adds more. `home` wins when
/// several screens are there — it is the one a fresh project opens with, and
/// the one "render my project" means; anything else wants naming on the
/// command line, and the error says which names exist.
pub(crate) fn resolve_input(input: &Path) -> Result<Screen, String> {
    if input.is_file() {
        let kind = input
            .extension()
            .map_or(String::new(), |e| e.to_string_lossy().into_owned());
        if kind != "say" && kind != "rs" {
            return Err(format!(
                "{} is a .{kind} file — this takes a .say screen or a .rs screen",
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
pub(crate) fn project_root_of(path: &Path) -> Option<PathBuf> {
    let mut dir = path.parent()?;
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// The project around `dir`, when the command was given no path at all —
/// `preview`, `test`, `export` and `assets` all mean "the one I am standing
/// in" more often than they mean anything else.
pub(crate) fn project_at(dir: &Path) -> Option<PathBuf> {
    if dir.join("Cargo.toml").is_file() {
        return Some(dir.to_path_buf());
    }
    project_root_of(dir)
}

/// `--out`'s default, once the input is resolved. `<project>/target/render/
/// <stem>.<ext>` when the screen sits inside a cargo project — cargo's own
/// place for generated things, and gitignored by the scaffold — or beside
/// the input when it does not. A named screen suffixes the stem, so
/// `--all-screens` never writes two PNGs to one path.
pub(crate) fn default_output(screen: &Screen, extension: &str, named: Option<&str>) -> PathBuf {
    let stem = screen
        .path
        .file_stem()
        .map_or_else(|| "screen".to_owned(), |s| s.to_string_lossy().into_owned());
    let stem = match named {
        Some(title) => format!("{stem}-{}", slug(title)),
        None => stem,
    };
    // Beside the input, named for the screen — `with_file_name` rather than
    // `with_extension`, because a named screen changed the *stem* and
    // `--all-screens` must never write two screens to one path. Built
    // before the branch that moves `stem`.
    let beside_input = screen.path.with_file_name(format!("{stem}.{extension}"));
    screen.project.as_ref().map_or_else(
        || beside_input,
        |root| {
            root.join("target")
                .join("render")
                .join(stem)
                .with_extension(extension)
        },
    )
}

/// A title as a filename piece: lowercase, everything that is not a letter
/// or a digit collapsed to one dash — `Settings & Privacy` →
/// `settings-privacy`, stable for any title the grammar allows.
pub(crate) fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut dash = false;
    for character in title.chars() {
        if character.is_ascii_alphanumeric() {
            out.extend(character.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The `[package] name` of a manifest, as a line scan.
///
/// Not a TOML parse: the scaffold writes this file, the shape is one
/// `name = "…"` under one `[package]` heading, and a hand-edited manifest
/// that breaks the shape fails later with cargo's own words rather than a
/// half-parsed guess here.
pub(crate) fn package_name(root: &Path) -> Option<String> {
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

// ---- the compile pipeline, and its two mount points --------------------------
/// The result of a compile: a screen mounted behind whichever door the
/// command chose, and how long `rustc` took for the summary line.
pub(crate) struct Mounted {
    pub(crate) preview: Preview,
    pub(crate) rustc: Duration,
}

/// Why a mount failed — the ABI candidate walk's signal, versus every other
/// refusal, which no other candidate can fix.
pub(crate) enum MountFailure {
    /// The library was compiled against a different `vieww` — the walk's
    /// cue to try the next candidate rlib.
    AbiMismatch(String),
    /// A refusal to hand to the user unchanged.
    Fatal(String),
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

/// The preview pipeline without the preview pane, mounted in this process:
/// the pane's own choice for itself, and the one `render`, `html`,
/// `animate` and `test` make.
///
/// # Errors
///
/// One sentence per failure stage, in the studio's own words where they
/// exist: Say diagnostics name `.say` lines, rustc diagnostics name the
/// generated Rust's, refusals say what to run.
pub(crate) fn compile_screen(screen: &Screen, initial: Option<&str>) -> Result<Mounted, String> {
    let (preview, rustc) = compile_mounted(screen, initial, &|library| {
        Preview::load(library).map_err(|error| match error {
            LoadError::AbiMismatch { host, guest } => {
                MountFailure::AbiMismatch(format!("host {host:#018x}, preview {guest:#018x}"))
            }
            other => MountFailure::Fatal(other.to_string()),
        })
    })?;
    Ok(Mounted { preview, rustc })
}

/// The pipeline with the mount point pulled out: `load` is the one decision
/// that differs between the commands, because `preview` mounts in the
/// out-of-process child (a guest abort must cost a frame, not the server)
/// while every other command mounts in process.
///
/// Everything else — the codegen, the toolchain walk, the candidate loop —
/// is shared, because there is one engine and two ways to drive it.
pub(crate) fn compile_mounted<T>(
    screen: &Screen,
    initial: Option<&str>,
    load: &dyn Fn(&Path) -> Result<T, MountFailure>,
) -> Result<(T, Duration), String> {
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
            initial_screen: initial.map(ToOwned::to_owned),
        };
        match vieww_say_codegen::compile_with(&file_name, &text, &options) {
            Ok(generated) => (generated.rust, generated.warnings),
            Err(diagnostics) => {
                return Err(diagnostics_block(&say_diagnostics(&diagnostics)));
            }
        }
    } else {
        if initial.is_some() {
            return Err(format!(
                "{} is not a .say file — --screen is a Say idea, and this is a Rust screen",
                screen.path.display()
            ));
        }
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
        match load(&library) {
            Ok(mounted) => return Ok((mounted, duration)),
            Err(MountFailure::AbiMismatch(_)) if project.is_none() => {
                candidate += 1;
            }
            Err(MountFailure::AbiMismatch(detail)) => {
                return Err(format!(
                    "the compiled preview does not match the vieww this process links \
                     ({detail}) — build both from one dependency graph (cargo build -p viewwstudio)"
                ));
            }
            Err(MountFailure::Fatal(message)) => return Err(message),
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

/// The mount every visual command shares: the studio's own install (the
/// panic policy, the fonts), the screen under a `Theme`, and — when a
/// device was named — the device's metrics published above the root, which
/// is what `SafeArea` and anything else platform-shaped reads. This is
/// `say_ai::renders_stably`'s mount, plus the device half of the preview
/// pane's.
pub(crate) fn mount_driver(viewport: &Viewport, dark: bool, root: WidgetNode) -> FrameDriver {
    let (width, height) = (viewport.size.0.max(1), viewport.size.1.max(1));
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
    driver.set_root(Theme::new(theme_data(dark)).child(root));
    if let Some(device) = viewport.device {
        driver.set_view_metrics(device.view_metrics());
    }
    driver
}

/// The screens a `.say` file defines, read off the source the same way
/// `screens` lists them — the grammar's own shape: no indent, the word
/// `screen`, a quoted title. The title ends at its closing quote, so the
/// transition suffix (`screen "X" with a fade transition:`) is not part
/// of the name — a bug this scan carried quietly until `--screen` started
/// feeding these titles to the codegen and every transition-declaring
/// screen came back "no screen called <half the line>".
pub(crate) fn screen_titles(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("screen \"")?;
            let title = rest.split('"').next()?;
            (!title.is_empty()).then(|| title.to_owned())
        })
        .collect()
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
    let ci = register_with_ci(&root, name)
        .map(|line| format!("\n{line}"))
        .unwrap_or_default();
    Ok(if dependency.previewable() {
        format!(
            "created {} ({} files, {}) — vieww is a path dependency, so render works",
            root.display(),
            written.len(),
            kind.title()
        ) + &ci
    } else {
        format!(
            "created {} ({} files, {}) — vieww is a published version, so web works and render will refuse",
            root.display(),
            written.len(),
            kind.title()
        ) + &ci
    })
}

/// Inside a checkout of this repository, give a new project its CI
/// manifest: `ci.toml` from `ci/templates/desktop-app.toml`, then
/// `ci/projects sync`, which adds it to the Run workflow forms' Project
/// dropdown. Without the sync the next CI run would fail on the stale
/// dropdown, which is the point: no project goes unchecked by accident.
///
/// `None` outside such a checkout (or when `ci.toml` exists), else the line
/// to print. Writing the manifest is the part that matters; a sync that
/// cannot run (no `python3`) says so and leaves it to the user.
pub fn register_with_ci(root: &Path, name: &str) -> Option<String> {
    let repo = root
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("ci").join("projects").is_file())?;
    let manifest = root.join("ci.toml");
    if manifest.exists() {
        return None;
    }
    let template =
        std::fs::read_to_string(repo.join("ci").join("templates").join("desktop-app.toml")).ok()?;
    let text = template.replace("{name}", name).replace("{package}", name);
    if let Err(error) = std::fs::write(&manifest, text) {
        return Some(format!("ci: could not write {}: {error}", manifest.display()));
    }
    let synced = Process::new("python3")
        .arg(repo.join("ci").join("projects"))
        .arg("sync")
        .current_dir(repo)
        .output()
        .is_ok_and(|out| out.status.success());
    Some(if synced {
        format!(
            "ci: wrote {} (desktop-app) and added {name} to the workflows' Project dropdown",
            manifest.display()
        )
    } else {
        format!(
            "ci: wrote {} (desktop-app); run `ci/projects sync` to add {name} to the workflows",
            manifest.display()
        )
    })
}

// ---- render (PNG) and html --------------------------------------------------

/// The screen as it stands after `script` has played, rasterised at the
/// viewport's size: the one picture `render`, `render --frame` and
/// `render --matrix` all start from.
///
/// With no script it is the first frame, exactly the picture `render` has
/// always produced. With one, the clock steps at 60 fps through every step
/// and `settle` past the last, so transitions finish and springs come to
/// rest before the picture is taken.
pub(crate) fn shoot(
    node: WidgetNode,
    viewport: &Viewport,
    dark: bool,
    script: &crate::showcase::Script,
    settle: Duration,
) -> Result<crate::showcase::Rgba, String> {
    let (width, height) = (viewport.size.0.max(1), viewport.size.1.max(1));
    let theme = theme_data(dark);
    let mut driver = mount_driver(viewport, dark, node);
    driver.draw_frame();
    if !script.is_empty() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "u32 pixel sizes are exact in f32"
        )]
        let size = Size::new(width as f32, height as f32);
        let mut player = crate::showcase::Player::new(script, size);
        let end = (script.end() + settle).as_secs_f64();
        let mut clock = 0.0_f64;
        while clock < end || !player.is_done() {
            clock += 1.0 / 60.0;
            let now = Duration::from_secs_f64(clock);
            player.advance(&mut driver, now)?;
            driver.draw_frame_at(now);
            if clock > end + 60.0 {
                break;
            }
        }
    }
    let mut renderer = NativeRenderer::new();
    let (pixels, _) = renderer
        .render_to_pixels(driver.scene(), width, height, theme.colors.surface)
        .map_err(|error| format!("the headless render failed: {error}"))?;
    Ok(crate::showcase::Rgba {
        width,
        height,
        data: pixels.data().to_vec(),
    })
}

/// The device a framed picture is framed as: the viewport's own.
fn frame_for(viewport: &Viewport, dark: bool) -> Result<crate::showcase::Framer, String> {
    let device = viewport
        .device
        .ok_or_else(|| "--frame needs --device to know which device to draw".to_owned())?;
    crate::showcase::Framer::new(
        device.platform,
        device.landscape,
        dark,
        viewport.size.0.max(1),
        viewport.size.1.max(1),
    )
}

/// One screen, one PNG — the unit `render` and `render --all-screens` share.
fn render_one(
    screen: &Screen,
    out: Option<&Path>,
    viewport: &Viewport,
    dark: bool,
    initial: Option<&str>,
    present: &Presentation,
) -> Result<String, String> {
    let mounted = compile_screen(screen, initial)?;
    let out = out.map_or_else(|| default_output(screen, "png", initial), Path::to_path_buf);
    let shot = shoot(
        mounted.preview.node(),
        viewport,
        dark,
        &present.script,
        present.settle,
    )?;
    let picture = if present.frame {
        let framer = frame_for(viewport, dark)?;
        framer.frame(&shot, present.background.flatten())?
    } else {
        match present.background.flatten() {
            Some(color) => shot.flattened(color),
            None => shot,
        }
    };
    write_out(&out, &picture.png())?;
    Ok(format!(
        "wrote {} ({}×{}{}{}, rustc {:.1?})",
        out.display(),
        picture.width,
        picture.height,
        if present.frame { ", framed" } else { "" },
        if present.script.is_empty() {
            String::new()
        } else {
            format!(", after {} steps", present.script.steps.len())
        },
        mounted.rustc
    ))
}

/// `render --matrix`: one compile, the screen mounted on every device in
/// turn, each framed, laid side by side on one sheet with its name under it
/// — the preview pane's device matrix (ST-21) as a picture.
fn render_matrix(
    screen: &Screen,
    out: Option<&Path>,
    devices: &[crate::state::Device],
    dark: bool,
    initial: Option<&str>,
    present: &Presentation,
) -> Result<String, String> {
    let mounted = compile_screen(screen, initial)?;
    let out = out.map_or_else(
        || {
            let named = default_output(screen, "png", initial);
            let stem = named
                .file_stem()
                .map_or_else(|| "screen".to_owned(), |s| s.to_string_lossy().into_owned());
            named.with_file_name(format!("{stem}-matrix.png"))
        },
        Path::to_path_buf,
    );
    let mut items = Vec::with_capacity(devices.len());
    for device in devices {
        let viewport = Viewport::of_device(*device, false);
        let shot = shoot(
            mounted.preview.node(),
            &viewport,
            dark,
            &present.script,
            present.settle,
        )?;
        let framed = frame_for(&viewport, dark)?.frame(&shot, None)?;
        items.push((format!("{} · {}", device.name, device.size_label()), framed));
    }
    let background = present
        .background
        .unwrap_or(Some(crate::showcase::STAGE))
        .unwrap_or(Color::rgba(0, 0, 0, 0));
    let sheet = crate::showcase::matrix_sheet(&items, background)?;
    write_out(&out, &sheet.png())?;
    Ok(format!(
        "wrote {} ({} devices side by side, {}×{}, rustc {:.1?})",
        out.display(),
        devices.len(),
        sheet.width,
        sheet.height,
        mounted.rustc
    ))
}

/// `render`: the loaded screen through the studio's own rasteriser.
///
/// `FrameDriver` + [`crate::install`] + a `Theme` wrapper is the exact mount
/// `say_ai::renders_stably` and `tests/canvas_edit.rs` both use — the one
/// render path the studio has, with its panic policy and its CPU scene.
///
/// With `all_screens`, every screen the file defines is compiled and
/// rendered in turn — a batch of screenshots, one rustc each, named
/// `<stem>-<screen>.png` so no two land on one path.
///
/// # Errors
///
/// The raster refused, or the PNG could not be written.
pub fn run_render(
    input: &Path,
    out: Option<&Path>,
    viewport: &Viewport,
    dark: bool,
    screen_name: Option<&str>,
    all_screens: bool,
    present: &Presentation,
) -> Result<String, String> {
    let screen = resolve_input(input)?;
    if screen_name.is_some() && all_screens {
        return Err(
            "--screen names the one screen to start on, and --all-screens renders them all — pick one".to_owned(),
        );
    }
    if out.is_some() && all_screens {
        return Err(
            "--out names one file, and --all-screens writes one per screen — let the default naming place them".to_owned(),
        );
    }
    if all_screens {
        let titles = titles_of(&screen)?;
        let mut lines = Vec::with_capacity(titles.len());
        for title in &titles {
            lines.push(render_one(
                &screen,
                None,
                viewport,
                dark,
                Some(title),
                present,
            )?);
        }
        return Ok(lines.join("\n"));
    }
    if let Some(title) = screen_name {
        // The codegen's E090 names the screens that exist; check here first
        // so a typo is refused before rustc is ever paid for.
        let titles = titles_of(&screen)?;
        if !titles.iter().any(|t| t == title) {
            return Err(format!(
                "there is no screen called \"{title}\" — {} defines {}",
                screen.path.display(),
                titles
                    .iter()
                    .map(|t| format!("\"{t}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if let Some(devices) = &present.matrix {
        return render_matrix(&screen, out, devices, dark, screen_name, present);
    }
    render_one(&screen, out, viewport, dark, screen_name, present)
}

/// The screens the resolved file defines, with the `.rs` refusal in the
/// CLI's own words — `--screen` and `--all-screens` are Say ideas.
fn titles_of(screen: &Screen) -> Result<Vec<String>, String> {
    if screen.path.extension().is_some_and(|e| e != "say") {
        return Err(format!(
            "{} is not a .say file — screens are a Say idea",
            screen.path.display()
        ));
    }
    let text = std::fs::read_to_string(&screen.path)
        .map_err(|error| format!("cannot read {} — {error}", screen.path.display()))?;
    Ok(screen_titles(&text))
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
    viewport: &Viewport,
    dark: bool,
    screen_name: Option<&str>,
) -> Result<String, String> {
    let screen = resolve_input(input)?;
    let mounted = compile_screen(&screen, screen_name)?;
    let out = out.map_or_else(
        || default_output(&screen, "html", screen_name),
        Path::to_path_buf,
    );
    let theme = theme_data(dark);
    let title = screen
        .path
        .file_stem()
        .map_or_else(|| "screen".to_owned(), |s| s.to_string_lossy().into_owned());
    let mut tree = ElementTree::new();
    let root = tree.mount(Theme::new(theme).child(mounted.preview.node()));
    let body: String = walk(&tree, root).iter().map(VNode::to_html).collect();
    let page = page_html(&title, &body, viewport.size.0, theme);
    write_out(&out, page.as_bytes())?;
    Ok(format!(
        "wrote {} ({}px wide, static DOM, rustc {:.1?})",
        out.display(),
        viewport.size.0,
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
pub(crate) fn write_out(out: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {} — {error}", parent.display()))?;
    }
    std::fs::write(out, bytes).map_err(|error| format!("cannot write {} — {error}", out.display()))
}

/// The theme both output paths mount under. One definition, because two
/// would be one more than the studio has.
pub(crate) fn theme_data(dark: bool) -> ThemeData {
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
pub(crate) fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
    let names = titles_of(&screen)?;
    if names.is_empty() {
        return Err(format!(
            "no screens in {} — a screen block starts with `screen \"Name\":` at the left margin",
            screen.path.display()
        ));
    }
    Ok(names.join("\n"))
}

/// Run one [`Spec`] in the foreground, its output inherited.
///
/// The studio runs these through the jobs queue so the window keeps
/// painting; a CLI *is* the foreground, and a person watching a `cargo
/// build` wants its progress, not a spinner. No timeout of the CLI's own:
/// `Spec::timeout` is the studio's backstop, and a person can Ctrl-C.
pub(crate) fn run_spec(spec: &Spec) -> Result<(), String> {
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
            parse(&arg(&["create", "demo", "--rust"])),
            Ok(Command::New {
                name: "demo".into(),
                rust: true
            })
        );
        assert!(parse(&arg(&["new"])).is_err());
    }

    #[test]
    fn a_new_project_in_the_repository_gets_a_ci_manifest() {
        let repo = std::env::temp_dir().join("vieww-cli-ci-register");
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("ci/templates")).unwrap();
        std::fs::write(repo.join("ci/projects"), "").unwrap();
        std::fs::write(
            repo.join("ci/templates/desktop-app.toml"),
            "name = \"{name}\"\nkind = \"desktop-app\"\n",
        )
        .unwrap();
        let root = repo.join("apps/demo");
        std::fs::create_dir_all(&root).unwrap();
        let line = register_with_ci(&root, "demo").expect("inside the checkout");
        assert!(line.starts_with("ci: wrote"), "{line}");
        assert_eq!(
            std::fs::read_to_string(root.join("ci.toml")).unwrap(),
            "name = \"demo\"\nkind = \"desktop-app\"\n"
        );
        // Not twice, and not outside a checkout.
        assert!(register_with_ci(&root, "demo").is_none());
        let elsewhere = std::env::temp_dir().join("vieww-cli-ci-none/demo");
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert!(register_with_ci(&elsewhere, "demo").is_none());
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn screen_and_widget_read_a_name() {
        assert_eq!(
            parse(&arg(&["screen", "login"])),
            Ok(Command::Screen {
                name: "login".into()
            })
        );
        assert_eq!(
            parse(&arg(&["widget", "note_card"])),
            Ok(Command::Widget {
                name: "note_card".into()
            })
        );
        assert!(parse(&arg(&["screen"])).is_err());
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
                viewport,
                dark,
                screen,
                all_screens,
                present,
            } => {
                assert_eq!(
                    present,
                    Presentation {
                        settle: Presentation::DEFAULT_SETTLE,
                        ..Presentation::default()
                    },
                    "no presentation flags, the plain render"
                );
                assert!(input.ends_with("home.say"));
                assert!(
                    input.is_absolute(),
                    "the input is resolved, not left relative"
                );
                assert_eq!(out, Some(absolute("shot.png").unwrap()));
                assert_eq!(viewport.size, (800, 600));
                assert_eq!(viewport.device, None, "no --device, no simulation");
                assert!(dark);
                assert_eq!(screen, None);
                assert!(!all_screens);
            }
            other => panic!("a render, not {other:?}"),
        }
    }

    #[test]
    fn a_positional_between_flags_is_not_a_flag() {
        // `render home.say --dark` — the input before the flags, the exact
        // order the usage shows. A regression test for the parse bug that
        // read the positional as an unknown flag.
        let command = parse(&arg(&["render", "home.say", "--dark"])).unwrap();
        assert!(matches!(command, Command::Render { dark: true, .. }));
        // And the other order, because a person typing flags first is not
        // wrong either.
        let command = parse(&arg(&["render", "--dark", "home.say"])).unwrap();
        assert!(matches!(command, Command::Render { dark: true, .. }));
    }

    #[test]
    fn a_flag_value_that_looks_like_a_flag_is_refused() {
        assert!(parse(&arg(&["render", "home.say", "--out", "--dark"]))
            .unwrap_err()
            .contains("--out needs a value"));
    }

    #[test]
    fn render_flags_that_need_values_say_so() {
        assert!(parse(&arg(&["render", "home.say", "--width"]))
            .unwrap_err()
            .contains("--width needs a value"));
        assert!(parse(&arg(&["render", "home.say", "--width", "wide"]))
            .unwrap_err()
            .contains("wide is not one"));
        assert!(parse(&arg(&["render", "home.say", "--width", "0"]))
            .unwrap_err()
            .contains("between 1 and"));
        assert!(parse(&arg(&["render", "home.say", "--spiral"]))
            .unwrap_err()
            .contains("does not take --spiral"));
    }

    #[test]
    fn a_half_size_is_refused_rather_than_guessed() {
        assert!(parse(&arg(&["render", "home.say", "--width", "800"]))
            .unwrap_err()
            .contains("come together"));
    }

    #[test]
    fn a_device_and_an_explicit_size_are_two_answers_to_one_question() {
        assert!(parse(&arg(&[
            "render", "home.say", "--device", "mobile", "--width", "800", "--height", "600"
        ]))
        .unwrap_err()
        .contains("brings its own size"));
    }

    #[test]
    fn landscape_needs_a_device() {
        assert!(parse(&arg(&["render", "home.say", "--landscape"]))
            .unwrap_err()
            .contains("needs --device"));
    }

    #[test]
    fn the_device_sets_the_size_and_the_metrics() {
        let command = parse(&arg(&["render", "home.say", "--device", "mobile"])).unwrap();
        match command {
            Command::Render { viewport, .. } => {
                assert_eq!(viewport.size, (393, 852), "the Phone preset's points");
                let device = viewport.device.expect("the preset is carried");
                assert_eq!(device.name, "Phone");
            }
            other => panic!("a render, not {other:?}"),
        }
        // And rotated: the same device on its side.
        let command = parse(&arg(&[
            "render",
            "home.say",
            "--device",
            "mobile",
            "--landscape",
        ]))
        .unwrap();
        match command {
            Command::Render { viewport, .. } => {
                assert_eq!(viewport.size, (852, 393));
                assert!(viewport.device.expect("carried").landscape);
            }
            other => panic!("a render, not {other:?}"),
        }
    }

    #[test]
    fn an_unknown_device_is_refused_with_the_names() {
        let error = parse(&arg(&["render", "home.say", "--device", "blackberry"])).unwrap_err();
        assert!(error.contains("blackberry"), "{error}");
        assert!(error.contains("phone-small"), "{error}");
    }

    #[test]
    fn animate_reads_its_own_flags() {
        let command = parse(&arg(&[
            "animate",
            "home.say",
            "--fps",
            "60",
            "--duration",
            "5s",
        ]))
        .unwrap();
        match command {
            Command::Animate {
                fps,
                duration,
                screen,
                ..
            } => {
                assert_eq!(fps, 60);
                assert_eq!(duration, Duration::from_secs(5));
                assert_eq!(screen, None);
            }
            other => panic!("an animate, not {other:?}"),
        }
        // The plain-number duration is seconds, the way the usage says.
        match parse(&arg(&["animate", "h.say", "--duration", "2"])).unwrap() {
            Command::Animate { duration, .. } => {
                assert_eq!(duration, Duration::from_secs(2));
            }
            other => panic!("an animate, not {other:?}"),
        }
        assert!(parse(&arg(&["animate", "h.say", "--duration", "forever"])).is_err());
        assert!(parse(&arg(&["animate", "h.say", "--fps", "144"])).is_err());
    }

    #[test]
    fn preview_defaults_to_the_phone_and_refuses_an_out() {
        let command = parse(&arg(&["preview"])).unwrap();
        match command {
            Command::Preview {
                input,
                device,
                port,
                open,
                ..
            } => {
                assert_eq!(input, None, "no path means the project around the cwd");
                assert_eq!(device.name, "Phone");
                assert_eq!(port, preview::DEFAULT_PORT);
                assert!(open);
            }
            other => panic!("a preview, not {other:?}"),
        }
        assert!(parse(&arg(&["preview", "--out", "x.png"])).is_err());
    }

    #[test]
    fn presentation_flags_parse() {
        match parse(&arg(&[
            "render",
            "--do",
            "tap \"Details\" @1s",
            "home.say",
            "--device",
            "phone",
            "--frame",
            "--background",
            "transparent",
            "--tap",
            "Back",
        ]))
        .unwrap()
        {
            Command::Render { input, present, .. } => {
                assert!(input.ends_with("home.say"), "a --do value is not the input");
                assert!(present.frame);
                assert_eq!(present.background, Some(None));
                assert_eq!(present.script.steps.len(), 2);
                assert_eq!(
                    present.script.steps[1].action,
                    crate::showcase::Action::Tap(crate::showcase::Target::Label("Back".into())),
                    "a bare --tap word is a label"
                );
            }
            other => panic!("a render, not {other:?}"),
        }
        assert!(
            parse(&arg(&["render", "h.say", "--frame"]))
                .unwrap_err()
                .contains("--device"),
            "a frame needs a device"
        );
        assert!(
            parse(&arg(&["html", "h.say", "--frame"])).is_err(),
            "html has no frame"
        );
    }

    #[test]
    fn a_matrix_is_framed_and_defaults_to_the_panes_devices() {
        match parse(&arg(&["render", "h.say", "--matrix"])).unwrap() {
            Command::Render { present, .. } => {
                assert!(present.frame);
                let names: Vec<_> = present.matrix.unwrap().iter().map(|d| d.name).collect();
                assert_eq!(names, ["Phone", "Tablet", "Desktop"]);
            }
            other => panic!("a render, not {other:?}"),
        }
        match parse(&arg(&[
            "render",
            "h.say",
            "--devices",
            "phone-small,android",
        ]))
        .unwrap()
        {
            Command::Render { present, .. } => assert_eq!(present.matrix.unwrap().len(), 2),
            other => panic!("a render, not {other:?}"),
        }
        assert!(parse(&arg(&["render", "h.say", "--matrix", "--device", "phone"])).is_err());
    }

    #[test]
    fn preview_shares_only_when_asked() {
        match parse(&arg(&["preview", "--share"])).unwrap() {
            Command::Preview { share, .. } => assert!(share),
            other => panic!("a preview, not {other:?}"),
        }
        match parse(&arg(&["preview"])).unwrap() {
            Command::Preview { share, .. } => assert!(!share, "local only by default"),
            other => panic!("a preview, not {other:?}"),
        }
    }

    #[test]
    fn export_needs_something_to_run() {
        assert!(parse(&arg(&["export"])).is_err());
        assert!(parse(&arg(&["export", "--format", "web", "--all"])).is_err());
        assert!(matches!(
            parse(&arg(&["export", "--format", "apk"])).unwrap(),
            Command::Export {
                format: Some(ExportName::Apk),
                ..
            }
        ));
        assert!(parse(&arg(&["export", "--format", "floppy"])).is_err());
        assert!(matches!(
            parse(&arg(&["export", "--list"])).unwrap(),
            Command::Export { list: true, .. }
        ));
    }

    #[test]
    fn test_reads_its_flags() {
        assert!(matches!(
            parse(&arg(&["test", "--update-goldens"])).unwrap(),
            Command::Test { update: true, .. }
        ));
        assert!(matches!(
            parse(&arg(&["test"])).unwrap(),
            Command::Test { input: None, .. }
        ));
    }

    #[test]
    fn usage_names_every_command() {
        let usage = usage();
        for command in [
            "new", "screen", "widget", "render", "html", "animate", "preview", "export", "web",
            "test", "screens", "devices", "assets",
        ] {
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
            default_output(&screen, "png", None),
            root.join("target").join("render").join("home.png")
        );
        assert_eq!(
            default_output(&screen, "html", None),
            root.join("target").join("render").join("home.html")
        );
        assert_eq!(
            default_output(&screen, "gif", Some("Settings & Privacy")),
            root.join("target")
                .join("render")
                .join("home-settings-privacy.gif"),
            "a named screen suffixes the stem, slugged"
        );
        let loose = Screen {
            path: PathBuf::from("/w/loose.say"),
            project: None,
        };
        assert_eq!(
            default_output(&loose, "png", None),
            PathBuf::from("/w/loose.png")
        );
        // And the loose file with a named screen: suffixed beside it, so a
        // batch never writes two screens over one path.
        assert_eq!(
            default_output(&loose, "png", Some("Settings")),
            PathBuf::from("/w/loose-settings.png")
        );
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
    fn a_transition_suffix_is_not_part_of_the_title() {
        // `screen "X" with a fade transition:` — the title is X. This was
        // a real bug: the scan used to hand `X" with a fade transition:` to
        // `--screen`, and the codegen's E090 refused it with half the line
        // in the quotes.
        assert_eq!(
            screen_titles("screen \"Details\" with a slide transition:\n    a label\n"),
            vec!["Details".to_owned()]
        );
        assert_eq!(
            screen_titles("screen \"Home\":\n    a label\n"),
            vec!["Home".to_owned()]
        );
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
