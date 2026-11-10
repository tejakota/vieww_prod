//! `animate` — the screen over time, as a GIF.
//!
//! # What this command is
//!
//! The preview pane shows a screen *moving*; `render` catches it standing
//! still. This is the gap between them: the same compile, the same mount,
//! the same rasteriser — with the clock advanced the way the feature
//! harness advances it (`examples/features/harness`), because **springs
//! integrate between frames**: sampling a spring at `t = 5s` in one jump is
//! a different physical system from stepping it 300 times at 60 fps, and
//! the stepped one is the one a window shows.
//!
//! So the loop is: sample times at `1/fps`, and between consecutive sample
//! times, draw at 60 fps sub-steps, exactly as `VIEWW_SHOT`'s strip mode
//! does. Each sampled frame is rasterised once, and the frames become a GIF
//! through the workspace's own encoder (`vieww_image::codec::gif`) — no
//! ffmpeg, no second toolchain, the same dependency policy the studio has.
//!
//! # The honest limits
//!
//! A Say screen has no time-driven vocabulary — no tickers, no timers — so
//! a Say screen's animation lives in its navigation transitions, its
//! dialogs and its snackbars, all of which need *events*, not time. A Rust
//! screen with a `Ticker`, a spring, a scroll-driven animation or a
//! transition animates for real. When the mounted tree reports that nothing
//! is animating, the command says so and writes the GIF anyway: a held
//! frame is the truth, and the note is where a person discovers that Say's
//! motion is interactive motion (`preview`) rather than timeline motion.

use std::path::Path;
use std::time::Duration;

use vieww_foundation::Color;
use vieww_image::codec::gif;
use vieww_paint::native::NativeRenderer;

use super::{
    compile_screen, default_output, frame_for, mount_driver, resolve_input, theme_data, write_out,
    Presentation, Screen, Viewport,
};

/// The sample rate when nobody chose one. 25 rather than the more obvious
/// 30 or 60, because GIF stores delays in centiseconds: 25 fps is exactly
/// 4 cs a frame, while 30 and 60 round, and a rounding that always goes the
/// same direction is an animation that plays measurably slower than asked.
pub(crate) const DEFAULT_FPS: u32 = 25;

/// The window when nobody chose one: three seconds — long enough for a
/// transition (300 ms) to happen four times over, short enough that the
/// default GIF is a few hundred kilobytes rather than a download.
pub(crate) const DEFAULT_DURATION: Duration = Duration::from_secs(3);

/// The sub-step the clock advances by between samples — 60 fps, the frame
/// rate the window path runs at and the one springs are tuned against.
const STEP: f64 = 1.0 / 60.0;

/// Every frame held in RGBA before the encode — the guard rail's unit. At
/// the default 25 fps × 3 s × 393×852 this is ~100 MB; four times that is
/// where a laptop starts to notice, so that is where the refusal is.
const FRAME_BUDGET_BYTES: usize = 1_500_000_000;

/// `--duration` as text: `5s`, `250ms`, `2.5s`, or a bare `5` meaning
/// seconds — the spellings the usage promises, and nothing else, because a
/// duration that parses two ways is a script that runs two speeds.
///
/// # Errors
///
/// The text is none of the promised shapes, or the window it names is
/// shorter than one frame or longer than a minute.
pub(crate) fn parse_duration(text: &str) -> Result<Duration, String> {
    let raw = text.trim().to_lowercase();
    let (number, unit) = match raw.strip_suffix("ms") {
        Some(digits) => (digits, "ms"),
        None => match raw.strip_suffix('s') {
            Some(digits) => (digits, "s"),
            None => (raw.as_str(), "s"),
        },
    };
    let value: f64 = number
        .parse()
        .map_err(|_| format!("--duration needs a number — {text} is not one"))?;
    if value < 0.0 {
        return Err(format!("--duration cannot run backwards — {text}"));
    }
    let seconds = if unit == "ms" { value / 1000.0 } else { value };
    if seconds * f64::from(DEFAULT_FPS) < 1.0 {
        return Err(format!(
            "--duration {text} is shorter than one frame at {DEFAULT_FPS} fps"
        ));
    }
    if seconds > 60.0 {
        return Err(format!(
            "--duration {text} is more than a minute — that is what --frames-dir and a \
             video encoder are for"
        ));
    }
    Ok(Duration::from_secs_f64(seconds))
}

/// `animate`: compile the screen, step the clock through the window, and
/// write the frames as a looping GIF.
///
/// # Errors
///
/// As the compile, plus: the frame budget (the GIF encoder holds every
/// frame in memory, and the refusal names the knobs that shrink it), or the
/// GIF could not be written.
#[expect(
    clippy::too_many_arguments,
    reason = "the arguments are `--out`, the device options and the animate-only flags, \
             one parameter each — the command line's own shape, which a struct would \
             only rename"
)]
pub fn run_animate(
    input: &Path,
    out: Option<&Path>,
    viewport: &Viewport,
    dark: bool,
    screen_name: Option<&str>,
    fps: u32,
    duration: Duration,
    frames_dir: Option<&Path>,
    present: &Presentation,
) -> Result<String, String> {
    let screen: Screen = resolve_input(input)?;
    let mounted = compile_screen(&screen, screen_name)?;
    let out = out.map_or_else(
        || default_output(&screen, "gif", screen_name),
        Path::to_path_buf,
    );

    let (width, height) = (viewport.size.0.max(1), viewport.size.1.max(1));
    // A script longer than the window would be cut off mid-gesture; the
    // window grows to hold it (plus a second to see where it lands) rather
    // than silently dropping the steps a person wrote.
    let duration = if present.script.is_empty() {
        duration
    } else {
        duration.max(present.script.end() + Duration::from_secs(1))
    };
    let framer = if present.frame {
        Some(frame_for(viewport, dark)?)
    } else {
        None
    };
    let (out_w, out_h) = framer.as_ref().map_or((width, height), |f| f.size());
    let seconds = duration.as_secs_f64();
    let samples = (seconds * f64::from(fps)).round() as usize;
    // The sample at t=0 plus the sample at t=duration, and every tick
    // between: `--duration 3s --fps 25` is 76 frames, not 75, because the
    // last frame is the point of a window that ends.
    let frames = samples + 1;
    let frame_bytes = out_w as usize * out_h as usize * 4;
    if frames * frame_bytes > FRAME_BUDGET_BYTES {
        return Err(format!(
            "{frames} frames at {width}×{height} is {} MiB of RGBA held before the encode \
             — lower --fps, --duration, or the size",
            frames * frame_bytes / (1024 * 1024)
        ));
    }

    let theme = theme_data(dark);
    let base: Color = theme.colors.surface;
    // A GIF has no partial transparency, so a framed one sits on the stage
    // unless a background was named; an unframed one is opaque already.
    let backdrop = present
        .background
        .unwrap_or(Some(crate::showcase::STAGE))
        .unwrap_or(crate::showcase::STAGE);
    let mut driver = mount_driver(viewport, dark, mounted.preview.node());

    // The first draw decides the note: a tree that reports nothing
    // animating will hold one frame for the whole window, and a person
    // asking for animation deserves to be told why they got a still.
    driver.draw_frame();
    let animating = driver.is_animating();
    if !animating && present.script.is_empty() {
        eprintln!(
            "note: {} reports no animation of its own — a Say screen's motion is in its \
             transitions — give it some input with --tap/--do/--script, or use `preview`; a \
             held frame is the truth",
            screen.path.file_name().map_or_else(
                || "the screen".to_owned(),
                |n| n.to_string_lossy().into_owned()
            ),
        );
    }

    let mut renderer = NativeRenderer::new();
    let mut encoded: Vec<(Vec<u8>, u16)> = Vec::with_capacity(frames);
    // GIF delays are integer centiseconds, so the delay a frame carries is
    // the rounded one and the *sampling* stays at the asked rate — the GIF
    // is as close to real time as the format can say.
    let delay_cs = (100.0 / f64::from(fps)).round() as u16;

    if let Some(dir) = frames_dir {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {} — {error}", dir.display()))?;
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "u32 pixel sizes are exact in f32"
    )]
    let mut player = crate::showcase::Player::new(
        &present.script,
        vieww_foundation::Size::new(width as f32, height as f32),
    );
    let mut first: Option<Vec<u8>> = None;
    let mut moved = false;
    let mut clock = 0.0_f64;
    for frame in 0..frames {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a frame index is exact in f64 at any count this command allows"
        )]
        let due = frame as f64 / f64::from(fps);
        // Stepped, not jumped — the harness's own reason: springs integrate
        // between frames, and a sample at `due` without the steps between
        // here and there is a different system.
        while clock < due {
            clock = (clock + STEP).min(due);
            let now = Duration::from_secs_f64(clock);
            player.advance(&mut driver, now)?;
            driver.draw_frame_at(now);
        }
        player.advance(&mut driver, Duration::from_secs_f64(due))?;
        driver.draw_frame_at(Duration::from_secs_f64(due));
        let (pixels, _) = renderer
            .render_to_pixels(driver.scene(), width, height, base)
            .map_err(|error| format!("the headless render failed: {error}"))?;
        let screen = crate::showcase::Rgba {
            width,
            height,
            data: pixels.data().to_vec(),
        };
        match &first {
            None => first = Some(screen.data.clone()),
            Some(start) => moved |= *start != screen.data,
        }
        let picture = match &framer {
            Some(framer) => framer.frame(&screen, Some(backdrop))?,
            None => screen,
        };
        if let Some(dir) = frames_dir {
            let png = vieww_image::codec::png::encode(out_w, out_h, &picture.data);
            let path = dir.join(format!("frame_{frame:04}.png"));
            write_out(&path, &png)?;
        }
        encoded.push((picture.data, delay_cs));
    }

    // The encoder takes u16 dimensions; every preset and every sane explicit
    // size is far inside that, and the try_into says so if one is not.
    let (gif_w, gif_h) = (
        u16::try_from(out_w).map_err(|_| format!("{out_w} is wider than a GIF can be"))?,
        u16::try_from(out_h).map_err(|_| format!("{out_h} is taller than a GIF can be"))?,
    );
    let bytes = gif::encode(gif_w, gif_h, &encoded, 0);
    write_out(&out, &bytes)?;
    let frames_written = encoded.len();
    Ok(format!(
        "wrote {} ({} frames, {:.1}s at {} fps, {}×{}{}{}, rustc {:.1?}){}",
        out.display(),
        frames_written,
        seconds,
        fps,
        out_w,
        out_h,
        if framer.is_some() { ", framed" } else { "" },
        if present.script.is_empty() {
            String::new()
        } else {
            format!(", {} steps played", present.script.steps.len())
        },
        mounted.rustc,
        if moved || animating {
            ""
        } else {
            ", held still"
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_in_every_promised_shape() {
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("5").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("2.5s").unwrap(), Duration::from_millis(2500));
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
    }

    #[test]
    fn a_bad_duration_is_named() {
        assert!(parse_duration("forever").unwrap_err().contains("not one"));
        assert!(
            parse_duration("5m").unwrap_err().contains("not one"),
            "m alone is not a unit this command promised"
        );
        assert!(parse_duration("-2s").unwrap_err().contains("backwards"));
        // Shorter than one frame at the default rate.
        assert!(parse_duration("1ms").unwrap_err().contains("one frame"));
        // Longer than the command exists for.
        assert!(parse_duration("90s").unwrap_err().contains("a minute"));
    }

    #[test]
    fn the_frame_count_includes_both_ends() {
        // 3s at 25 fps is 75 intervals and 76 frames — the window's end is
        // itself a sample, which is what the delay math and the GIF's length
        // both assume.
        let samples = (DEFAULT_DURATION.as_secs_f64() * f64::from(DEFAULT_FPS)).round() as usize;
        assert_eq!(samples + 1, 76);
    }
}
