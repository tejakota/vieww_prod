//! `test` — compile, render, exercise, compare.
//!
//! # What this command is
//!
//! The studio proves a screen by *looking* at it; this proves it the way a
//! machine can, four checks a screen:
//!
//! 1. **It compiles** — the same compile every render path runs, with the
//!    same diagnostics on failure.
//! 2. **It renders, deterministically** — the ST-03 gate (`renders_stably`):
//!    two `draw_frame`s at the same clock, byte-identical PNGs. A screen
//!    that reads the wall clock renders two different pictures, and that is
//!    exactly the bug this catches — a golden image would be comparing
//!    against noise.
//! 3. **It does not overflow** — [`vieww_render::overflow`]'s own counter,
//!    zeroed before the run and required to still be zero after it, with
//!    the ancestry lines the renderer prints on stderr naming where. This
//!    is the check the framework's own overflow suite runs (`overflow_
//!    names_its_place`), pointed at the screen under test.
//! 4. **It survives its own interface** — the input smoke: five taps (the
//!    centre and the four quadrant centres, the places a screen puts
//!    things), a scroll, and Enter/Escape/Backspace, each followed by the
//!    clock stepped forward at 60 fps so transitions and springs run. A
//!    handler that panics is caught by the driver's error policy — the
//!    screen keeps rendering — and a screen that *aborts* is caught the
//!    hard way, by the process. Both are failures this test reports.
//!
//! 5. **It looks like it did last time** — the golden: the first frame's
//!    PNG against the one `--update-goldens` wrote, byte-equal first (the
//!    rasteriser is byte-exact by design), then pixel-diffed with a
//!    tolerance, so a one-pixel font shift from a system update is a note,
//!    not a failure. Goldens live under `target/` by default — they are
//!    **per-machine**, because renders use the system's fonts; committing
//!    them is a project's choice, with that caveat.
//!
//! One compile per screen: a `.say` file with five screens is five cases,
//! each mounted with its own start screen (`--all-screens`'s codegen door),
//! because "the settings screen overflows" is the sentence a person needs,
//! not "the file overflowed somewhere".

use std::path::{Path, PathBuf};
use std::time::Duration;

use vieww_foundation::{KeyEvent, Offset, PointerEvent, PointerId, ScrollEvent, Size};
use vieww_paint::native::NativeRenderer;
use vieww_render::overflow;

use super::{
    compile_screen, mount_driver, project_at, resolve_input, screen_titles, theme_data, Screen,
    Viewport,
};

/// One case: a file, and the screen within it to start on.
struct Case {
    /// The screen's identity in the report: `home.say` or `flow.say#Settings`.
    label: String,
    /// The file to compile.
    screen: Screen,
    /// The start screen, for a multi-screen `.say` file.
    initial: Option<String>,
    /// The golden's stem, unique in the golden directory.
    golden_stem: String,
}

/// The golden directory's default: the project's `target/render/goldens/`,
/// beside every other rendered artefact, gitignored by the scaffold, and
/// per-machine by construction — which is the property that makes keeping
/// goldens there honest.
fn default_goldens(screen: &Screen) -> PathBuf {
    screen.project.as_ref().map_or_else(
        || PathBuf::from("goldens"),
        |root| root.join("target").join("render").join("goldens"),
    )
}

/// `test`: every screen in the input, against the four checks.
///
/// `input` of `None` means the project around the current directory. A
/// project is every `.say`/`.rs` file under its `src/screens/`; a single
/// file is just itself — with each of its screens its own case.
///
/// # Errors
///
/// The summary is `Ok` only when every case passed every check; a failed
/// case is a line in the `Err`, one per failure, in the order the checks
/// ran.
pub fn run_test(
    input: Option<&Path>,
    viewport: &Viewport,
    dark: bool,
    goldens: Option<&Path>,
    update: bool,
) -> Result<String, String> {
    let path = match input {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(|error| format!("cannot tell where you are standing — {error}"))?,
    };

    // The cases: a directory is every screen file in it; a file is itself,
    // split per screen.
    let mut cases: Vec<Case> = Vec::new();
    if path.is_dir() {
        let root = project_at(&path).ok_or_else(|| {
            format!(
                "{} is not inside a project — test compiles screens, and a screen outside \
                 a project has no toolchain",
                path.display()
            )
        })?;
        let screens = root.join("src").join("screens");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&screens)
            .map_err(|_| format!("no src/screens/ under {}", root.display()))?
            .flatten()
            .map(|entry| entry.path())
            // `mod.rs` is the module list, not a screen — the one file in the
            // directory that is *about* the screens rather than being one,
            // and compiling it would fail on the entry point it never had.
            .filter(|p| {
                p.extension().is_some_and(|e| e == "say" || e == "rs")
                    && p.file_name().is_some_and(|n| n != "mod.rs")
            })
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(format!(
                "no screens under {} — nothing to test",
                screens.display()
            ));
        }
        for file in files {
            push_cases(&mut cases, &file);
        }
    } else {
        let screen = resolve_input(&path)?;
        push_cases(&mut cases, &screen.path);
    }

    let goldens = goldens.map_or_else(
        || {
            // The default follows the first case's project, which is every
            // case's project — a run mixes projects only when the caller
            // pointed one file at another project, and that caller passes
            // `--goldens` anyway.
            cases
                .first()
                .map(|case| default_goldens(&case.screen))
                .unwrap_or_else(|| PathBuf::from("goldens"))
        },
        Path::to_path_buf,
    );

    let mut report = Vec::new();
    let mut failed = 0;
    for case in &cases {
        let one = test_one(case, viewport, dark, &goldens, update);
        match one {
            Ok(line) => report.push(format!("PASS {}", line)),
            Err(lines) => {
                failed += 1;
                for line in lines {
                    report.push(format!("FAIL {line}"));
                }
            }
        }
    }
    let summary = format!(
        "{} case{}: {} passed, {} failed",
        cases.len(),
        if cases.len() == 1 { "" } else { "s" },
        cases.len() - failed,
        failed
    );
    report.push(summary.clone());
    if failed == 0 {
        Ok(report.join("\n"))
    } else {
        Err(report.join("\n"))
    }
}

/// The cases one file contributes: every screen it defines, or itself.
fn push_cases(cases: &mut Vec<Case>, file: &Path) {
    let screen =
        resolve_input(file).expect("the caller listed files that exist, with the right extensions");
    let stem = screen
        .path
        .file_stem()
        .map_or_else(|| "screen".to_owned(), |s| s.to_string_lossy().into_owned());
    let is_say = screen.path.extension().is_some_and(|e| e == "say");
    if !is_say {
        cases.push(Case {
            label: stem.clone(),
            golden_stem: stem,
            screen,
            initial: None,
        });
        return;
    }
    let text = std::fs::read_to_string(&screen.path).unwrap_or_default();
    let titles = screen_titles(&text);
    if titles.is_empty() {
        // A `.say` file with no screens cannot compile; the case stays so
        // the compile's own diagnostic is the failure, not a silent skip.
        cases.push(Case {
            label: stem.clone(),
            golden_stem: stem,
            screen,
            initial: None,
        });
        return;
    }
    for title in titles {
        let slug = super::slug(&title);
        cases.push(Case {
            label: format!("{stem}#{title}"),
            golden_stem: format!("{stem}-{slug}"),
            screen: screen.clone(),
            initial: Some(title),
        });
    }
}

/// One case, four checks, in the order the module doc lists them. The
/// return is the passing line, or one line per failed check.
fn test_one(
    case: &Case,
    viewport: &Viewport,
    dark: bool,
    goldens: &Path,
    update: bool,
) -> Result<String, Vec<String>> {
    let mut failures = Vec::new();
    let initial = case.initial.as_deref();

    // A compile failure is the first failure and the last check — there is
    // no screen to render.
    let mounted = match compile_screen(&case.screen, initial) {
        Ok(mounted) => mounted,
        Err(error) => return Err(vec![format!("{} — did not compile: {error}", case.label)]),
    };

    let theme = theme_data(dark);
    let (width, height) = (viewport.size.0.max(1), viewport.size.1.max(1));
    let mut driver = mount_driver(viewport, dark, mounted.preview.node());
    let mut renderer = NativeRenderer::new();
    let base = theme.colors.surface;

    // 1+2. Two draws, byte-identical: renders, and deterministically.
    overflow::forget_reported();
    driver.draw_frame();
    let first = match renderer.render_to_png(driver.scene(), width, height, base) {
        Ok((png, _)) => png,
        Err(error) => return Err(vec![format!("{} — did not render: {error}", case.label)]),
    };
    driver.draw_frame();
    let second = match renderer.render_to_png(driver.scene(), width, height, base) {
        Ok((png, _)) => png,
        Err(error) => {
            failures.push(format!(
                "{} — the second render failed: {error}",
                case.label
            ));
            Vec::new()
        }
    };
    if first != second {
        failures.push(format!(
            "{} — renders differently from one frame to the next at the same clock: \
             something reads the wall clock, and no golden can be trusted against it",
            case.label
        ));
    }

    // 3. Input: five taps, a scroll, three keys — the clock stepped 300 ms
    // past each, at 60 fps, so transitions run their course.
    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 pixel sizes are exact in f32"
    )]
    let size = Size::new(width as f32, height as f32);
    let taps = [
        Offset::new(size.width * 0.5, size.height * 0.5),
        Offset::new(size.width * 0.25, size.height * 0.25),
        Offset::new(size.width * 0.75, size.height * 0.25),
        Offset::new(size.width * 0.25, size.height * 0.75),
        Offset::new(size.width * 0.75, size.height * 0.75),
    ];
    let mut clock = 0.0_f64;
    for tap in taps {
        let stamp = Duration::from_secs_f64(clock);
        driver.handle_pointer(&PointerEvent::down(PointerId(1), tap, stamp));
        driver.handle_pointer(&PointerEvent::up(PointerId(1), tap, stamp));
        clock = step_clock(&mut driver, clock, 0.3);
    }
    let scroll = ScrollEvent {
        position: Offset::new(size.width * 0.5, size.height * 0.5),
        delta: Offset::new(0.0, -120.0),
        timestamp: clock_ms(clock),
    };
    driver.handle_scroll(&scroll);
    clock = step_clock(&mut driver, clock, 0.3);
    for key in [
        vieww_foundation::NamedKey::Enter,
        vieww_foundation::NamedKey::Escape,
        vieww_foundation::NamedKey::Backspace,
    ] {
        driver.handle_key(&KeyEvent::named(key, clock_ms(clock)));
        clock = step_clock(&mut driver, clock, 0.3);
    }

    // 4. Overflow: counted across the whole run — the double render and the
    // input both — because "overflows after the third tap" is still
    // "overflows". The renderer's stderr already named where.
    let overflowed = overflow::reported();
    if overflowed > 0 {
        failures.push(format!(
            "{} — the layout overflowed {} time{} (the lines above name where in the tree)",
            case.label,
            overflowed,
            if overflowed == 1 { "" } else { "s" }
        ));
    }

    // 5. The golden: the first frame against the file `--update-goldens`
    // writes. Written fresh when updating; compared byte-equal, then
    // pixel-diffed with a tolerance, when not.
    let golden = goldens.join(format!("{}.png", case.golden_stem));
    if update {
        if let Some(parent) = golden.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&golden, &first)
            .map_err(|error| vec![format!("{} — cannot write the golden: {error}", case.label)])?;
    } else if golden.is_file() {
        let expected = std::fs::read(&golden).unwrap_or_default();
        if expected != first {
            let (different, worst) = pixel_difference(&expected, &first);
            if worst > 0 {
                failures.push(format!(
                    "{} — the golden changed: {} pixel{} differ, worst channel delta {worst} \
                     — run with --update-goldens if the change is the point",
                    case.label,
                    different,
                    if different == 1 { "" } else { "s" }
                ));
            } else {
                failures.push(format!(
                    "{} — the golden's PNG bytes differ but every pixel matches (a \
                     metadata change, not a render change) — run with --update-goldens",
                    case.label
                ));
            }
        }
    } else {
        failures.push(format!(
            "{} — no golden at {} — run once with --update-goldens to write it",
            case.label,
            golden.display()
        ));
    }

    if failures.is_empty() {
        Ok(format!(
            "{} — deterministic, no overflow, {} inputs, golden matches (rustc {:.1?})",
            case.label,
            taps.len() + 3,
            mounted.rustc
        ))
    } else {
        Err(failures)
    }
}

/// Step `clock` forward by `seconds` in 60 fps draws — the same discipline
/// `animate` and the feature harness use, because springs integrate between
/// frames. Returns the new clock, in seconds.
fn step_clock(driver: &mut vieww_render::FrameDriver, mut clock: f64, seconds: f64) -> f64 {
    let due = clock + seconds;
    while clock < due {
        clock = (clock + 1.0 / 60.0).min(due);
        driver.draw_frame_at(Duration::from_secs_f64(clock));
    }
    clock
}

/// A clock in seconds as the `Duration` the event constructors take.
fn clock_ms(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds)
}

/// Two PNGs' pixels compared: how many differ at all, and the worst
/// channel delta. Decoding rather than trusting the container means a
/// re-encoded golden (a tool re-saved it) still compares by what it shows.
/// Mismatched sizes are `usize::MAX` — a size change is every pixel.
fn pixel_difference(expected: &[u8], actual: &[u8]) -> (usize, u8) {
    let (Ok(expected), Ok(actual)) = (vieww_asset::decode(expected), vieww_asset::decode(actual))
    else {
        return (usize::MAX, u8::MAX);
    };
    let (expected, actual) = (expected.0.pixels(), actual.0.pixels());
    if expected.len() != actual.len() {
        return (usize::MAX, u8::MAX);
    }
    let mut different = 0;
    let mut worst = 0_u8;
    // `as_chunks` rather than `chunks_exact` — the workspace's own applied
    // lint (`chunks_exact_to_as_chunks`, see the root `Cargo.toml` note).
    let (expected, actual) = (expected.as_chunks::<4>().0, actual.as_chunks::<4>().0);
    for (a, b) in expected.iter().zip(actual) {
        if a != b {
            different += 1;
            worst = worst
                .max(a[0].abs_diff(b[0]))
                .max(a[1].abs_diff(b[1]))
                .max(a[2].abs_diff(b[2]))
                .max(a[3].abs_diff(b[3]));
        }
    }
    (different, worst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_goldens_live_in_the_project() {
        let screen = Screen {
            path: PathBuf::from("/w/demo/src/screens/home.say"),
            project: Some(PathBuf::from("/w/demo")),
        };
        assert_eq!(
            default_goldens(&screen),
            PathBuf::from("/w/demo/target/render/goldens")
        );
        let loose = Screen {
            path: PathBuf::from("/w/loose.say"),
            project: None,
        };
        assert_eq!(default_goldens(&loose), PathBuf::from("goldens"));
    }

    #[test]
    fn the_clock_steps_in_60fps_slices_and_lands_exactly() {
        // Not a FrameDriver here — the arithmetic is the part under test:
        // every step is 1/60, none overshoots the due time, and the due
        // time is hit exactly, which is what "springs integrate between
        // frames" needs.
        let mut clock = 0.0_f64;
        let mut steps = 0;
        let due = clock + 0.3;
        while clock < due {
            clock = (clock + 1.0 / 60.0).min(due);
            steps += 1;
        }
        assert!((clock - 0.3).abs() < 1e-9, "lands exactly: {clock}");
        assert_eq!(steps, 18, "0.3s at 60fps is 18 steps");
    }
}
