//! Cold start: process entry → the studio's first complete frame, and the
//! session's first compile.
//!
//! The number a launch film can honestly show is not "how long cargo took" or
//! "how long a warm frame costs" — it is how long a user waits between
//! double-clicking the icon and seeing an interface. That is: construct the
//! frame driver, install the studio, build the whole widget tree for the first
//! time, lay it out, and paint it. Everything up to pixels-ready.
//!
//! Since ST-23 it also prints the *other* first wait: the guest template
//! compiled twice through the same `compile` door a Render goes through —
//! cold cache, then warm — which is the effect `--keep-warm` moves behind
//! the splash. See the block at the end for how to read those two lines.
//!
//! ```console
//! cargo run --release -p viewwstudio --example coldstart
//! ```

use std::time::{Duration, Instant};

use vieww_foundation::Size;
use vieww_render::FrameDriver;
use viewwstudio::Studio;

const WINDOW: Size = Size {
    width: 1440.0,
    height: 900.0,
};

fn main() {
    // Taken as early as possible so the measurement includes every allocation,
    // font load and tree construction the first frame depends on.
    let entry = Instant::now();

    let mut driver = FrameDriver::new(WINDOW);
    let t_driver = entry.elapsed();
    viewwstudio::install(&mut driver);
    let t_install = entry.elapsed();
    let runtime = driver.elements().runtime().clone();
    let studio = Studio::new(&runtime);
    studio.note_window_size(WINDOW);
    let installed = entry.elapsed();
    println!(
        "    FrameDriver::new            {:>8.2} ms\n    viewwstudio::install        {:>8.2} ms\n    Studio::new                 {:>8.2} ms",
        t_driver.as_secs_f64() * 1e3,
        (t_install - t_driver).as_secs_f64() * 1e3,
        (installed - t_install).as_secs_f64() * 1e3,
    );

    // The first frame: build the full tree, lay it out, paint it to a scene.
    driver.draw_frame_at(Duration::from_millis(0));
    let first_frame = entry.elapsed();

    // A second frame, to show what the first one paid for that the next does
    // not: caches warm, tree already built, nothing dirty.
    let before_second = Instant::now();
    driver.draw_frame_at(Duration::from_millis(16));
    let second_frame = before_second.elapsed();

    println!("cold start, {WINDOW:?}\n");
    println!(
        "  driver + studio constructed   {:>8.2} ms",
        installed.as_secs_f64() * 1e3
    );
    println!(
        "  first complete frame          {:>8.2} ms   (process entry → scene ready)",
        first_frame.as_secs_f64() * 1e3
    );
    println!(
        "  second frame                  {:>8.2} ms",
        second_frame.as_secs_f64() * 1e3
    );

    // ST-23's other number: the first compile of a session, cold and warm.
    //
    // The frame timings above are the studio's own startup; this is the
    // other wait a first-time user meets — the preview's first `rustc`,
    // reading the whole `vieww` rlib and its ~100 dependencies from a cache
    // nothing has warmed yet (10.6 s on the research's Windows machine,
    // ~0.8 s after). `--keep-warm` exists to pay the first of those behind
    // the splash; this block is how the effect is seen, on any machine,
    // without a GUI: the guest template compiled twice through the same
    // `compile` door a Render goes through.
    //
    // On a machine that just built this tree the "cold" number is optimistic
    // — the page cache already holds the rlib cargo wrote — so read it as
    // the warm end of the range, and the gap between the two lines as what
    // the warm-up moves.
    println!("\n  the first compile of a session (ST-23):\n");
    let target = viewwstudio::install::target_dir();
    match viewwstudio::compile::Toolchain::discover(&target) {
        Ok(toolchain) => match viewwstudio::compile::Session::new(std::process::id().into()) {
            Ok(session) => {
                let report = |label: &str, started: Instant| {
                    println!("  {label:<30}{:>8.2} s", started.elapsed().as_secs_f64());
                };
                let cold = Instant::now();
                let first = viewwstudio::compile::compile(
                    &toolchain,
                    &session,
                    viewwstudio::keepwarm::TEMPLATE,
                    viewwstudio::keepwarm::FILE_NAME,
                );
                report("guest template, cold cache", cold);
                let warm = Instant::now();
                let second = viewwstudio::compile::compile(
                    &toolchain,
                    &session,
                    viewwstudio::keepwarm::TEMPLATE,
                    viewwstudio::keepwarm::FILE_NAME,
                );
                report("same template, warm cache", warm);
                // The template is the studio's own source; a compile that
                // fails is a bug, and the number beside it is a lie.
                for (which, result) in [("cold", &first), ("warm", &second)] {
                    match result {
                        Ok(compiled) if !compiled.failed() => {}
                        Ok(compiled) => eprintln!(
                            "  the {which} compile of the template FAILED: {}",
                            compiled
                                .diagnostics
                                .first()
                                .map_or_else(|| "no diagnostics".to_owned(), |d| d.message.clone())
                        ),
                        Err(error) => {
                            eprintln!("  the {which} compile of the template did not run: {error}")
                        }
                    }
                }
            }
            Err(error) => println!("  no session directory to compile into — {error}"),
        },
        Err(error) => println!("  no preview compiler — {error}"),
    }
}
