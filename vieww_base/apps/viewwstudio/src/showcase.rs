//! Presentation output: device-framed stills and animations, the device
//! matrix as one image, scripted interaction, and the preview recorder.
//!
//! # What this module is
//!
//! The pieces both faces of the studio need to turn a rendered screen into
//! something a person shows someone else:
//!
//! * [`Framer`] — the preview pane's own device shell (its `DeviceShell`
//!   painter, its `cutouts`, its `shell_metrics`) drawn *around* a
//!   rendered screen. The shell and the cutouts are rasterised once per
//!   device by the same `NativeRenderer` every render uses, and each frame of
//!   an animation is only a paste between them — so framing a 75-frame GIF
//!   costs two renders, not 150.
//! * [`matrix_sheet`] — several framed screens side by side on one canvas,
//!   each labelled with its device: the pane's ST-21 matrix, as a PNG.
//! * [`Script`] — interaction over time: `tap "Add one" @1s`,
//!   `drag 200,600 -> 200,200 @2s over 400ms`, `scroll -300 @3s`,
//!   `key enter`, `type "hello"`. Targets are a point in device points or a
//!   *label* the screen's semantics tree carries, resolved at the moment the
//!   step fires, so a script survives a layout change that moves a button.
//!   `animate` plays a script while it samples frames; `render` plays one and
//!   photographs where it ends.
//! * [`Recorder`] — the studio's Record command: the pane's frames and the
//!   input that produced them, kept while recording and written on stop as a
//!   GIF *and* a script in the syntax above, so a recording can be replayed
//!   headlessly (`viewwstudio-cli animate --script`) at any device, theme or
//!   frame.
//!
//! # The honest limits
//!
//! * The frame is drawn at one device point per pixel, the same raster scale
//!   `render` has always used. A larger frame is a larger device, not a
//!   sharper one.
//! * A label target matches the semantics tree's labels — the words a screen
//!   reader would say. A widget with no label (an unlabelled image, a bare
//!   gesture area) is reachable only by a point.
//! * The recorder records what the *isolated* preview draws, because that is
//!   the path where the studio holds the guest's pixels; starting a
//!   recording turns isolation on (see `Studio::toggle_recording`).

use std::fmt::Write as _;
use std::time::Duration;

use vieww_foundation::{
    Alignment, Color, EdgeInsets, KeyEvent, NamedKey, Offset, PointerEvent, PointerId, ScrollEvent,
    Size, TextStyle,
};
use vieww_paint::native::NativeRenderer;
use vieww_render::FrameDriver;
use vieww_widget::prelude::*;
use vieww_widget::{Container, Painting};

use crate::state::Platform;
use crate::ui::preview::{cutouts, shell_metrics, DeviceShell};

/// The page's own stage: the colour the served preview sits on, and the
/// default background wherever a framed picture cannot be transparent (a
/// GIF, a matrix sheet).
pub const STAGE: Color = Color::hex(0x17_161A);

/// The room around a framed device for its shadow and side buttons — the
/// shell paints a 46-point blur 18 points down and buttons 2.5 points out,
/// and a picture that clips its own shadow looks like a rendering bug.
pub const MARGIN: u32 = 64;

/// A picture in straight-alpha RGBA8, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Rgba {
    /// A picture of one colour.
    #[must_use]
    pub fn filled(width: u32, height: u32, color: Color) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            data.extend_from_slice(&[color.r, color.g, color.b, color.a]);
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// `src` drawn over this picture with its top-left at `(x, y)`,
    /// straight-alpha "over", clipped to this picture's bounds.
    pub fn draw_over(&mut self, src: &Self, x: i64, y: i64) {
        for sy in 0..src.height {
            let dy = y + i64::from(sy);
            if dy < 0 || dy >= i64::from(self.height) {
                continue;
            }
            for sx in 0..src.width {
                let dx = x + i64::from(sx);
                if dx < 0 || dx >= i64::from(self.width) {
                    continue;
                }
                let s = (sy as usize * src.width as usize + sx as usize) * 4;
                let d = (dy as usize * self.width as usize + dx as usize) * 4;
                let pixel = [
                    src.data[s],
                    src.data[s + 1],
                    src.data[s + 2],
                    src.data[s + 3],
                ];
                over(&mut self.data[d..d + 4], pixel, 1.0);
            }
        }
    }

    /// The picture as PNG bytes.
    #[must_use]
    pub fn png(&self) -> Vec<u8> {
        vieww_image::codec::png::encode(self.width, self.height, &self.data)
    }

    /// The picture with every pixel drawn over `background` — what a GIF,
    /// which has no partial transparency, needs.
    #[must_use]
    pub fn flattened(&self, background: Color) -> Self {
        let mut out = Self::filled(self.width, self.height, background);
        out.draw_over(self, 0, 0);
        out
    }
}

/// Straight-alpha "over" of `src` (scaled by `coverage`) onto `dst`.
fn over(dst: &mut [u8], src: [u8; 4], coverage: f32) {
    let sa = f32::from(src[3]) / 255.0 * coverage.clamp(0.0, 1.0);
    if sa <= 0.0 {
        return;
    }
    let da = f32::from(dst[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    for channel in 0..3 {
        let s = f32::from(src[channel]);
        let d = f32::from(dst[channel]);
        let value = (s * sa + d * da * (1.0 - sa)) / out_a;
        dst[channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// Rasterise a widget tree at `width`×`height` over transparency, through the
/// studio's own mount (`crate::install`) and its one rasteriser.
///
/// # Errors
///
/// The headless render refused.
pub fn rasterize(root: WidgetNode, width: u32, height: u32, dark: bool) -> Result<Rgba, String> {
    let mut driver = FrameDriver::new(Size::new(width as f32, height as f32));
    crate::install(&mut driver);
    driver.set_root(Theme::new(crate::cli::theme_data(dark)).child(root));
    driver.draw_frame();
    let mut renderer = NativeRenderer::new();
    let (pixels, _) = renderer
        .render_to_pixels(driver.scene(), width, height, Color::rgba(0, 0, 0, 0))
        .map_err(|error| format!("the headless render failed: {error}"))?;
    Ok(Rgba {
        width,
        height,
        data: pixels.data().to_vec(),
    })
}

// ---- the device frame --------------------------------------------------------

/// A device frame, rasterised once, ready to put screens in.
///
/// Built from a platform, an orientation and the screen's size in pixels
/// (one per device point), it holds two layers: the shell under the screen
/// and the cutouts (the notch, the punch hole, the home indicator) over it.
/// [`Framer::frame`] pastes a screen between them through the shell's
/// rounded screen mask.
#[derive(Debug, Clone)]
pub struct Framer {
    shell: Rgba,
    overlay: Rgba,
    /// Where the screen's top-left lands in the framed picture.
    pub origin: (u32, u32),
    screen: (u32, u32),
    radius: f32,
}

impl Framer {
    /// The frame for a `width`×`height` screen on `platform`.
    ///
    /// # Errors
    ///
    /// Either layer failed to rasterise.
    pub fn new(
        platform: Platform,
        landscape: bool,
        dark: bool,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let (corner, bezel) = shell_metrics(platform);
        let bezel_px = bezel.round() as u32;
        let device = (width + bezel_px * 2, height + bezel_px * 2);
        let canvas = (device.0 + MARGIN * 2, device.1 + MARGIN * 2);
        let shell_tree: WidgetNode = Container::new()
            .padding(EdgeInsets::all(MARGIN as f32))
            .child(Painting::sized(
                Size::new(device.0 as f32, device.1 as f32),
                DeviceShell {
                    corner,
                    bezel,
                    buttons: bezel > 0.0,
                },
            ))
            .into();
        let shell = rasterize(shell_tree, canvas.0, canvas.1, dark)?;

        let colors = crate::cli::theme_data(dark).colors;
        let screen_size = (width as f32, height as f32);
        let mut layers: Vec<WidgetNode> =
            vec![SizedBox::from_size(Size::new(screen_size.0, screen_size.1)).into()];
        if bezel > 0.0 {
            layers.extend(cutouts(colors, platform, landscape, screen_size));
        }
        let overlay_tree: WidgetNode = Container::new()
            .padding(EdgeInsets::all((MARGIN + bezel_px) as f32))
            .child(
                Stack::new()
                    .alignment(Alignment::TOP_CENTER)
                    .children(layers),
            )
            .into();
        let overlay = rasterize(overlay_tree, canvas.0, canvas.1, dark)?;

        Ok(Self {
            shell,
            overlay,
            origin: (MARGIN + bezel_px, MARGIN + bezel_px),
            screen: (width, height),
            radius: (corner - bezel).max(0.0),
        })
    }

    /// The framed picture's size.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.shell.width, self.shell.height)
    }

    /// `screen` inside the frame, over `background` (`None` keeps the
    /// margin transparent, for a PNG).
    ///
    /// # Errors
    ///
    /// The screen is not the size this frame was built for.
    pub fn frame(&self, screen: &Rgba, background: Option<Color>) -> Result<Rgba, String> {
        if (screen.width, screen.height) != self.screen {
            return Err(format!(
                "a {}×{} screen does not fit a frame built for {}×{}",
                screen.width, screen.height, self.screen.0, self.screen.1
            ));
        }
        let mut out = match background {
            Some(color) => {
                let mut base = Rgba::filled(self.shell.width, self.shell.height, color);
                base.draw_over(&self.shell, 0, 0);
                base
            }
            None => self.shell.clone(),
        };
        let (ox, oy) = self.origin;
        let (w, h) = self.screen;
        for y in 0..h {
            for x in 0..w {
                let coverage = rounded_coverage(x, y, w, h, self.radius);
                if coverage <= 0.0 {
                    continue;
                }
                let s = (y as usize * w as usize + x as usize) * 4;
                let d = ((oy + y) as usize * out.width as usize + (ox + x) as usize) * 4;
                let pixel = [
                    screen.data[s],
                    screen.data[s + 1],
                    screen.data[s + 2],
                    screen.data[s + 3],
                ];
                over(&mut out.data[d..d + 4], pixel, coverage);
            }
        }
        out.draw_over(&self.overlay, 0, 0);
        Ok(out)
    }
}

/// How much of pixel `(x, y)` a `w`×`h` rectangle with corner radius
/// `radius` covers — 1 inside, 0 outside, the edge antialiased by the
/// distance from the corner's centre.
fn rounded_coverage(x: u32, y: u32, w: u32, h: u32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return 1.0;
    }
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let (wf, hf) = (w as f32, h as f32);
    let cx = if px < radius {
        radius
    } else if px > wf - radius {
        wf - radius
    } else {
        return 1.0;
    };
    let cy = if py < radius {
        radius
    } else if py > hf - radius {
        hf - radius
    } else {
        return 1.0;
    };
    let distance = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
    (radius - distance + 0.5).clamp(0.0, 1.0)
}

// ---- the matrix sheet ----------------------------------------------------------

/// The gap between devices on a sheet, and the band under them for labels.
const SHEET_GAP: u32 = 24;
const LABEL_BAND: u32 = 44;

/// Several framed pictures side by side on `background`, top-aligned, each
/// with its label centred under it — the pane's device matrix as one image.
///
/// # Errors
///
/// The label layer failed to rasterise.
pub fn matrix_sheet(items: &[(String, Rgba)], background: Color) -> Result<Rgba, String> {
    let width: u32 = items.iter().map(|(_, p)| p.width).sum::<u32>()
        + SHEET_GAP * (items.len().saturating_sub(1) as u32)
        + SHEET_GAP * 2;
    let tallest = items.iter().map(|(_, p)| p.height).max().unwrap_or(1);
    // Each label sits just under its own device — inside the bottom margin
    // the frame leaves for its shadow — rather than on one line under the
    // tallest, which strands a phone's name far below the phone.
    let label_top = |picture: &Rgba| picture.height.saturating_sub(MARGIN / 2 + 4);
    let height = items
        .iter()
        .map(|(_, p)| label_top(p) + LABEL_BAND)
        .max()
        .unwrap_or(1)
        .max(tallest);

    let mut sheet = Rgba::filled(width.max(1), height.max(1), background);
    let mut labels: Vec<WidgetNode> =
        vec![SizedBox::from_size(Size::new(width as f32, height as f32)).into()];
    let ink = if luminance(background) > 0.5 {
        Color::hex(0x1C_1B1F)
    } else {
        Color::hex(0xE4_E4EA)
    };
    let mut x = SHEET_GAP;
    for (label, picture) in items {
        sheet.draw_over(picture, i64::from(x), 0);
        labels.push(
            Positioned::new()
                .left(x as f32)
                .top(label_top(picture) as f32)
                .width(picture.width as f32)
                .height(LABEL_BAND as f32)
                .child(
                    Center::new().child(Text::new(label.clone()).style(TextStyle {
                        color: ink,
                        ..TextStyle::new(15.0)
                    })),
                )
                .into(),
        );
        x += picture.width + SHEET_GAP;
    }
    let label_layer = rasterize(
        Stack::new()
            .alignment(Alignment::TOP_LEFT)
            .children(labels)
            .into(),
        sheet.width,
        sheet.height,
        luminance(background) <= 0.5,
    )?;
    sheet.draw_over(&label_layer, 0, 0);
    Ok(sheet)
}

fn luminance(color: Color) -> f32 {
    (0.2126 * f32::from(color.r) + 0.7152 * f32::from(color.g) + 0.0722 * f32::from(color.b))
        / 255.0
}

/// `#RRGGBB`, `#RRGGBBAA`, or `transparent`/`none`.
///
/// # Errors
///
/// Anything else, named.
pub fn parse_background(text: &str) -> Result<Option<Color>, String> {
    let raw = text.trim().to_lowercase();
    if raw == "transparent" || raw == "none" {
        return Ok(None);
    }
    let hex = raw.strip_prefix('#').unwrap_or(&raw);
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16);
    match hex.len() {
        6 => match (byte(0), byte(2), byte(4)) {
            (Ok(r), Ok(g), Ok(b)) => Ok(Some(Color::rgba(r, g, b, 255))),
            _ => Err(format!(
                "--background {text} is not a colour — #RRGGBB or transparent"
            )),
        },
        8 => match (byte(0), byte(2), byte(4), byte(6)) {
            (Ok(r), Ok(g), Ok(b), Ok(a)) => Ok(Some(Color::rgba(r, g, b, a))),
            _ => Err(format!(
                "--background {text} is not a colour — #RRGGBB or transparent"
            )),
        },
        _ => Err(format!(
            "--background {text} is not a colour — #RRGGBB or transparent"
        )),
    }
}

// ---- interaction scripts --------------------------------------------------------

/// Where a step acts: a point in device points, or a label the semantics
/// tree carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Point(f32, f32),
    Label(String),
}

/// One thing a person does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Press and release where the target is.
    Tap(Target),
    /// Press, hold for the duration, release — a long press.
    Hold(Target, Duration),
    /// Press at one point, move to another over the duration, release.
    Drag {
        from: Target,
        to: (f32, f32),
        over: Duration,
    },
    /// A wheel scroll by `(dx, dy)` at the target (the screen's centre when
    /// none), in the content-moves convention `test` uses.
    Scroll {
        at: Option<Target>,
        dx: f32,
        dy: f32,
    },
    /// A named key.
    Key(NamedKey),
    /// Text, one character event per character.
    Type(String),
}

/// An action at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub at: Duration,
    pub action: Action,
}

/// A whole interaction, steps in time order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Script {
    pub steps: Vec<Step>,
}

/// When a step names no time: half a second in for the first, and this long
/// after the previous one for the rest — long enough for a 300 ms
/// transition to finish before the next thing happens.
const FIRST_STEP: Duration = Duration::from_millis(500);
const STEP_GAP: Duration = Duration::from_millis(800);

impl Script {
    /// Parse one step per line; blank lines and `#`/`--` comments are skipped.
    ///
    /// # Errors
    ///
    /// The first line that does not parse, by number.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut script = Self::default();
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("--") {
                continue;
            }
            script
                .push_line(trimmed)
                .map_err(|error| format!("line {}: {error}", index + 1))?;
        }
        Ok(script)
    }

    /// Add one step, written the way a script line is.
    ///
    /// # Errors
    ///
    /// The line does not parse, or it names a time before the previous step.
    pub fn push_line(&mut self, line: &str) -> Result<(), String> {
        let (body, at) = split_time(line)?;
        let action = parse_action(body.trim())?;
        let at = match at {
            Some(at) => at,
            None => self.steps.last().map_or(FIRST_STEP, |last| {
                last.at + STEP_GAP + action_length(&last.action)
            }),
        };
        if let Some(last) = self.steps.last() {
            if at < last.at {
                return Err(format!(
                    "@{} is before the previous step's @{} — steps run in order",
                    seconds(at),
                    seconds(last.at)
                ));
            }
        }
        self.steps.push(Step { at, action });
        Ok(())
    }

    /// When the last step finishes, so a window can be sized around it.
    #[must_use]
    pub fn end(&self) -> Duration {
        self.steps
            .last()
            .map_or(Duration::ZERO, |s| s.at + action_length(&s.action))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The script in its own syntax, one step per line — what the recorder
    /// writes, and what [`Script::parse`] reads back to the same steps.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for step in &self.steps {
            let _ = writeln!(out, "{} @{}", action_text(&step.action), seconds(step.at));
        }
        out
    }
}

fn seconds(at: Duration) -> String {
    let ms = at.as_millis();
    if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

/// How long an action keeps the pointer busy.
fn action_length(action: &Action) -> Duration {
    match action {
        Action::Tap(_) => TAP_LENGTH,
        Action::Hold(_, d) | Action::Drag { over: d, .. } => *d,
        _ => Duration::ZERO,
    }
}

/// Split `... @1.5s` (or `@250ms`, or `@2`) off the end of a line.
fn split_time(line: &str) -> Result<(&str, Option<Duration>), String> {
    // The last `@` outside quotes.
    let mut quoted = false;
    let mut split = None;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '@' if !quoted => split = Some(index),
            _ => {}
        }
    }
    match split {
        Some(index) => {
            let time = parse_time(line[index + 1..].trim())?;
            Ok((&line[..index], Some(time)))
        }
        None => Ok((line, None)),
    }
}

/// `1.5s`, `250ms`, or a bare number of seconds.
fn parse_time(text: &str) -> Result<Duration, String> {
    let raw = text.trim().to_lowercase();
    let (number, ms) = match raw.strip_suffix("ms") {
        Some(n) => (n, true),
        None => (raw.strip_suffix('s').unwrap_or(&raw), false),
    };
    let value: f64 = number
        .trim()
        .parse()
        .map_err(|_| format!("{text} is not a time — 1.5s, 250ms or 2"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{text} is not a time this script can reach"));
    }
    Ok(Duration::from_secs_f64(if ms {
        value / 1000.0
    } else {
        value
    }))
}

fn parse_action(body: &str) -> Result<Action, String> {
    let (verb, rest) = body
        .split_once(char::is_whitespace)
        .map_or((body, ""), |(v, r)| (v, r.trim()));
    match verb.to_lowercase().as_str() {
        "tap" | "click" => Ok(Action::Tap(parse_target(rest)?)),
        "hold" | "longpress" | "long-press" => {
            let (target, over) = split_word(rest, " for ");
            let over = over.map_or(Ok(Duration::from_millis(700)), parse_time)?;
            Ok(Action::Hold(parse_target(target)?, over))
        }
        "drag" | "swipe" => {
            let (path, over) = split_word(rest, " over ");
            let over = over.map_or(Ok(Duration::from_millis(300)), parse_time)?;
            let (from, to) = path
                .split_once("->")
                .ok_or_else(|| format!("drag needs `from -> to` — {rest}"))?;
            let to = match parse_target(to)? {
                Target::Point(x, y) => (x, y),
                Target::Label(_) => {
                    return Err("a drag ends at a point (x,y), not a label".to_owned())
                }
            };
            Ok(Action::Drag {
                from: parse_target(from)?,
                to,
                over,
            })
        }
        "scroll" => {
            let (amount, at) = split_word(rest, " at ");
            let parts: Vec<&str> = amount.split(',').map(str::trim).collect();
            let number = |s: &str| {
                s.parse::<f32>()
                    .map_err(|_| format!("scroll takes dy or dx,dy in points — {amount}"))
            };
            let (dx, dy) = match parts.as_slice() {
                [dy] => (0.0, number(dy)?),
                [dx, dy] => (number(dx)?, number(dy)?),
                _ => return Err(format!("scroll takes dy or dx,dy in points — {amount}")),
            };
            Ok(Action::Scroll {
                at: at.map(parse_target).transpose()?,
                dx,
                dy,
            })
        }
        "key" | "press" => Ok(Action::Key(named_key(rest)?)),
        "type" => Ok(Action::Type(unquote(rest).ok_or_else(|| {
            format!("type takes quoted text — type \"hello\", not type {rest}")
        })?)),
        "" => Err("an empty step".to_owned()),
        other => Err(format!(
            "{other} is not a step — tap, hold, drag, scroll, key or type"
        )),
    }
}

/// `text` split at the first `word` outside quotes.
fn split_word<'a>(text: &'a str, word: &str) -> (&'a str, Option<&'a str>) {
    let mut quoted = false;
    for (index, ch) in text.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        } else if !quoted && text[index..].starts_with(word) {
            return (
                text[..index].trim(),
                Some(text[index + word.len()..].trim()),
            );
        }
    }
    (text.trim(), None)
}

fn unquote(text: &str) -> Option<String> {
    let inner = text.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\""))
}

/// `"Label"` or `x,y`.
fn parse_target(text: &str) -> Result<Target, String> {
    let text = text.trim();
    if let Some(label) = unquote(text) {
        return Ok(Target::Label(label));
    }
    let (x, y) = text
        .split_once(',')
        .ok_or_else(|| format!("{text} is not a target — \"Label\" or x,y"))?;
    let x: f32 = x
        .trim()
        .parse()
        .map_err(|_| format!("{text} is not a target — \"Label\" or x,y"))?;
    let y: f32 = y
        .trim()
        .parse()
        .map_err(|_| format!("{text} is not a target — \"Label\" or x,y"))?;
    Ok(Target::Point(x, y))
}

fn target_text(target: &Target) -> String {
    match target {
        Target::Point(x, y) => format!("{},{}", round1(*x), round1(*y)),
        Target::Label(label) => format!("\"{}\"", label.replace('"', "\\\"")),
    }
}

fn round1(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

fn action_text(action: &Action) -> String {
    match action {
        Action::Tap(t) => format!("tap {}", target_text(t)),
        Action::Hold(t, d) => format!("hold {} for {}", target_text(t), seconds(*d)),
        Action::Drag { from, to, over } => format!(
            "drag {} -> {},{} over {}",
            target_text(from),
            round1(to.0),
            round1(to.1),
            seconds(*over)
        ),
        Action::Scroll { at, dx, dy } => {
            let amount = if *dx == 0.0 {
                format!("{}", round1(*dy))
            } else {
                format!("{},{}", round1(*dx), round1(*dy))
            };
            match at {
                Some(t) => format!("scroll {amount} at {}", target_text(t)),
                None => format!("scroll {amount}"),
            }
        }
        Action::Key(key) => format!("key {}", key_name(*key)),
        Action::Type(text) => format!("type \"{}\"", text.replace('"', "\\\"")),
    }
}

const KEYS: [(&str, NamedKey); 11] = [
    ("enter", NamedKey::Enter),
    ("tab", NamedKey::Tab),
    ("backspace", NamedKey::Backspace),
    ("escape", NamedKey::Escape),
    ("delete", NamedKey::Delete),
    ("left", NamedKey::ArrowLeft),
    ("right", NamedKey::ArrowRight),
    ("up", NamedKey::ArrowUp),
    ("down", NamedKey::ArrowDown),
    ("home", NamedKey::Home),
    ("end", NamedKey::End),
];

fn named_key(name: &str) -> Result<NamedKey, String> {
    let name = name.trim().to_lowercase();
    let name = name.strip_prefix("arrow").unwrap_or(&name);
    let name = if name == "esc" { "escape" } else { name };
    KEYS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, k)| *k)
        .ok_or_else(|| {
            format!(
                "{name} is not a key a script sends — {}",
                KEYS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            )
        })
}

fn key_name(key: NamedKey) -> &'static str {
    KEYS.iter()
        .find(|(_, k)| *k == key)
        .map_or("enter", |(n, _)| n)
}

// ---- playing a script ------------------------------------------------------------

/// How long a tap holds the pointer down.
const TAP_LENGTH: Duration = Duration::from_millis(60);

/// A raw event waiting for its time.
#[derive(Debug, Clone)]
enum Raw {
    Down(Offset),
    Move(Offset, Offset),
    Up(Offset),
    Scroll(Offset, Offset),
    Key(NamedKey),
    Char(String),
}

/// A script being played into a driver.
#[derive(Debug)]
pub struct Player {
    steps: std::collections::VecDeque<Step>,
    raw: Vec<(Duration, Raw)>,
    size: Size,
    pointer: u64,
}

impl Player {
    /// Ready to play `script` into a screen of `size` device points.
    #[must_use]
    pub fn new(script: &Script, size: Size) -> Self {
        Self {
            steps: script.steps.iter().cloned().collect(),
            raw: Vec::new(),
            size,
            pointer: 1,
        }
    }

    /// Whether anything is still to happen.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.steps.is_empty() && self.raw.is_empty()
    }

    /// Deliver everything due by `now`: fire the steps whose time has come
    /// (resolving label targets against the tree as it is *now*), then the
    /// raw events those steps scheduled.
    ///
    /// # Errors
    ///
    /// A label target the screen does not carry — with the labels it does.
    pub fn advance(&mut self, driver: &mut FrameDriver, now: Duration) -> Result<(), String> {
        while self.steps.front().is_some_and(|s| s.at <= now) {
            let Some(step) = self.steps.pop_front() else {
                break;
            };
            self.schedule(driver, &step)?;
        }
        self.raw.sort_by_key(|(at, _)| *at);
        let due = self.raw.partition_point(|(at, _)| *at <= now);
        let events: Vec<(Duration, Raw)> = self.raw.drain(..due).collect();
        for (at, event) in events {
            let id = PointerId(self.pointer);
            match event {
                Raw::Down(p) => {
                    driver.handle_pointer(&PointerEvent::down(id, p, at));
                }
                Raw::Move(from, to) => {
                    driver.handle_pointer(&PointerEvent::moved(id, from, to, at));
                }
                Raw::Up(p) => {
                    driver.handle_pointer(&PointerEvent::up(id, p, at));
                    self.pointer += 1;
                }
                Raw::Scroll(position, delta) => {
                    driver.handle_scroll(&ScrollEvent {
                        position,
                        delta,
                        timestamp: at,
                    });
                }
                Raw::Key(key) => {
                    driver.handle_key(&KeyEvent::named(key, at));
                }
                Raw::Char(text) => {
                    driver.handle_key(&KeyEvent::character(text, at));
                }
            }
        }
        Ok(())
    }

    fn schedule(&mut self, driver: &FrameDriver, step: &Step) -> Result<(), String> {
        let at = step.at;
        match &step.action {
            Action::Tap(target) => {
                let p = self.resolve(driver, target)?;
                self.raw.push((at, Raw::Down(p)));
                self.raw.push((at + TAP_LENGTH, Raw::Up(p)));
            }
            Action::Hold(target, length) => {
                let p = self.resolve(driver, target)?;
                self.raw.push((at, Raw::Down(p)));
                self.raw.push((at + *length, Raw::Up(p)));
            }
            Action::Drag { from, to, over } => {
                let start = self.resolve(driver, from)?;
                let end = Offset::new(to.0, to.1);
                self.raw.push((at, Raw::Down(start)));
                let steps = ((over.as_secs_f64() * 60.0).ceil() as u32).max(1);
                let mut previous = start;
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let point = Offset::new(
                        start.dx + (end.dx - start.dx) * t,
                        start.dy + (end.dy - start.dy) * t,
                    );
                    let when = at + over.mul_f64(f64::from(i) / f64::from(steps));
                    self.raw.push((when, Raw::Move(previous, point)));
                    previous = point;
                }
                self.raw
                    .push((at + *over + Duration::from_millis(1), Raw::Up(end)));
            }
            Action::Scroll { at: target, dx, dy } => {
                let p = match target {
                    Some(target) => self.resolve(driver, target)?,
                    None => Offset::new(self.size.width / 2.0, self.size.height / 2.0),
                };
                self.raw.push((at, Raw::Scroll(p, Offset::new(*dx, *dy))));
            }
            Action::Key(key) => self.raw.push((at, Raw::Key(*key))),
            Action::Type(text) => {
                for (i, ch) in text.chars().enumerate() {
                    self.raw.push((
                        at + Duration::from_millis(40 * i as u64),
                        Raw::Char(ch.to_string()),
                    ));
                }
            }
        }
        Ok(())
    }

    fn resolve(&self, driver: &FrameDriver, target: &Target) -> Result<Offset, String> {
        match target {
            Target::Point(x, y) => Ok(Offset::new(*x, *y)),
            Target::Label(label) => find_label(driver, label),
        }
    }
}

/// The centre of the on-screen thing whose semantic label is `label` —
/// exactly first, then case-insensitively, then as a prefix, preferring an
/// enabled node that takes a tap.
///
/// # Errors
///
/// Nothing carries that label; the error lists what does.
pub fn find_label(driver: &FrameDriver, label: &str) -> Result<Offset, String> {
    let tree = driver.semantics();
    let nodes = tree.nodes();
    let visible = |n: &&vieww_render::SemanticsNode| n.bounds.width() > 0.0;
    let wanted = label.to_lowercase();
    let tiers: [&dyn Fn(&str) -> bool; 3] = [
        &|l: &str| l == label,
        &|l: &str| l.to_lowercase() == wanted,
        &|l: &str| l.to_lowercase().starts_with(&wanted),
    ];
    for matches in tiers {
        let mut found: Vec<&vieww_render::SemanticsNode> = nodes
            .iter()
            .filter(visible)
            .filter(|n| n.label.as_deref().is_some_and(matches))
            .collect();
        // Prefer the node that does something: an enabled one with actions.
        found.sort_by_key(|n| (!n.enabled, n.actions.is_empty()));
        if let Some(node) = found.first() {
            let b = node.bounds;
            return Ok(Offset::new(
                (b.left + b.right) / 2.0,
                (b.top + b.bottom) / 2.0,
            ));
        }
    }
    let mut known: Vec<String> = nodes
        .iter()
        .filter(visible)
        .filter_map(|n| n.label.clone())
        .filter(|l| !l.trim().is_empty())
        .collect();
    known.dedup();
    known.truncate(24);
    Err(format!(
        "nothing on screen is labelled \"{label}\"{}",
        if known.is_empty() {
            " — and nothing on it carries a label at all; target a point (x,y)".to_owned()
        } else {
            format!(
                " — the screen has {}",
                known
                    .iter()
                    .map(|l| format!("\"{l}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    ))
}

// ---- the recorder ------------------------------------------------------------------

/// The longest recording kept: two minutes, a few hundred megabytes of frames
/// at a phone's size, which is past where a GIF is the right container.
pub const RECORDING_LIMIT: Duration = Duration::from_secs(120);

/// What a recording holds while it runs.
#[derive(Debug, Default)]
pub struct Recorder {
    started: Option<std::time::Instant>,
    frames: Vec<(Duration, Rgba)>,
    script: Script,
    pressed: Option<(Duration, Offset, Offset)>,
    /// Points the pointer moved through while pressed, for a drag.
    moved: bool,
}

thread_local! {
    static RECORDER: std::cell::RefCell<Recorder> = std::cell::RefCell::new(Recorder::default());
}

/// Whether a recording is running on this (the UI) thread.
#[must_use]
pub fn is_recording() -> bool {
    RECORDER.with(|r| r.borrow().started.is_some())
}

/// Start a recording, dropping any unfinished one.
pub fn start_recording() {
    RECORDER.with(|r| {
        *r.borrow_mut() = Recorder {
            started: Some(std::time::Instant::now()),
            ..Recorder::default()
        };
    });
}

/// Keep a frame the pane just drew, if recording. Identical consecutive
/// frames are folded into the previous one's duration.
pub fn record_frame(width: u32, height: u32, rgba: &[u8]) {
    RECORDER.with(|r| {
        let mut r = r.borrow_mut();
        let Some(started) = r.started else { return };
        let at = started.elapsed();
        if at > RECORDING_LIMIT {
            return;
        }
        if r.frames.last().is_some_and(|(_, last)| {
            last.width == width && last.height == height && last.data == rgba
        }) {
            return;
        }
        r.frames.push((
            at,
            Rgba {
                width,
                height,
                data: rgba.to_vec(),
            },
        ));
    });
}

/// Keep an input event the pane forwarded, if recording, as script steps.
pub fn record_event(event: &crate::remote::RemoteEvent) {
    use crate::remote::{Pointer, RemoteEvent};
    RECORDER.with(|r| {
        let mut r = r.borrow_mut();
        let Some(started) = r.started else { return };
        let at = started.elapsed();
        let push = |r: &mut Recorder, at: Duration, action: Action| {
            let at = r.script.steps.last().map_or(at, |s| at.max(s.at));
            r.script.steps.push(Step { at, action });
        };
        match event {
            RemoteEvent::Pointer(Pointer::Down(p)) => {
                r.pressed = Some((at, *p, *p));
                r.moved = false;
            }
            RemoteEvent::Pointer(Pointer::Move(p)) => {
                if let Some((_, start, last)) = r.pressed.as_mut() {
                    *last = *p;
                    if (p.dx - start.dx).abs() > 4.0 || (p.dy - start.dy).abs() > 4.0 {
                        r.moved = true;
                    }
                }
            }
            RemoteEvent::Pointer(Pointer::Up(p)) => {
                let Some((down, start, _)) = r.pressed.take() else {
                    return;
                };
                let held = at.saturating_sub(down);
                let action = if r.moved {
                    Action::Drag {
                        from: Target::Point(start.dx, start.dy),
                        to: (p.dx, p.dy),
                        over: held.max(Duration::from_millis(16)),
                    }
                } else if held >= Duration::from_millis(500) {
                    Action::Hold(Target::Point(start.dx, start.dy), held)
                } else {
                    Action::Tap(Target::Point(start.dx, start.dy))
                };
                push(&mut r, down, action);
            }
            RemoteEvent::Scroll(s) => push(
                &mut r,
                at,
                Action::Scroll {
                    at: Some(Target::Point(s.position.dx, s.position.dy)),
                    dx: s.delta.dx,
                    dy: s.delta.dy,
                },
            ),
            RemoteEvent::Key(key) => {
                if key.state != vieww_foundation::KeyState::Down {
                    return;
                }
                if let Some(text) = key.key.text().filter(|t| !t.is_empty()) {
                    // Consecutive characters become one `type` step.
                    if let Some(Step {
                        action: Action::Type(previous),
                        ..
                    }) = r.script.steps.last_mut()
                    {
                        previous.push_str(text);
                        return;
                    }
                    let text = text.to_owned();
                    push(&mut r, at, Action::Type(text));
                } else if let vieww_foundation::LogicalKey::Named(named) = key.key {
                    if KEYS.iter().any(|(_, k)| *k == named) {
                        push(&mut r, at, Action::Key(named));
                    }
                }
            }
            RemoteEvent::Hover(_) | RemoteEvent::Ime(_) => {}
        }
    });
}

/// A finished recording.
#[derive(Debug)]
pub struct Recording {
    /// Frames with the time each was first shown.
    pub frames: Vec<(Duration, Rgba)>,
    /// When the recording stopped.
    pub length: Duration,
    /// The input, as a replayable script.
    pub script: Script,
}

/// Stop the recording and hand back what it holds; `None` when none ran.
#[must_use]
pub fn stop_recording() -> Option<Recording> {
    RECORDER.with(|r| {
        let mut r = r.borrow_mut();
        let started = r.started.take()?;
        let taken = std::mem::take(&mut *r);
        Some(Recording {
            frames: taken.frames,
            length: started.elapsed().min(RECORDING_LIMIT),
            script: taken.script,
        })
    })
}

impl Recording {
    /// The recording as a looping GIF, each frame held until the next one
    /// arrived, framed in `framer` when one is given. Frames are scaled to
    /// the first frame's size (the pane can change scale mid-recording when
    /// its window is resized).
    ///
    /// # Errors
    ///
    /// No frames were drawn, the frame refused, or the picture is too big
    /// for a GIF.
    pub fn gif(&self, framer: Option<&Framer>, surface: Color) -> Result<Vec<u8>, String> {
        let Some((_, first)) = self.frames.first() else {
            return Err(
                "nothing was drawn while recording — interact with the preview, then stop"
                    .to_owned(),
            );
        };
        let target = framer.map_or((first.width, first.height), |f| f.screen);
        let mut encoded: Vec<(Vec<u8>, u16)> = Vec::with_capacity(self.frames.len());
        let mut size = None;
        for (index, (at, frame)) in self.frames.iter().enumerate() {
            let until = self.frames.get(index + 1).map_or(
                self.length.max(*at + Duration::from_millis(40)),
                |(t, _)| *t,
            );
            let delay = ((until.saturating_sub(*at)).as_millis() / 10).clamp(2, 65_535) as u16;
            // The pane draws the guest over transparency (its own surface
            // shows through); a recording carries the screen's surface.
            let fitted = scale_to(frame, target.0, target.1).flattened(surface);
            let picture = match framer {
                Some(framer) => framer.frame(&fitted, Some(STAGE))?,
                None => fitted.flattened(STAGE),
            };
            size = Some((picture.width, picture.height));
            encoded.push((picture.data, delay));
        }
        let (w, h) = size.unwrap_or((1, 1));
        let (w, h) = (
            u16::try_from(w).map_err(|_| format!("{w} is wider than a GIF can be"))?,
            u16::try_from(h).map_err(|_| format!("{h} is taller than a GIF can be"))?,
        );
        Ok(vieww_image::codec::gif::encode(w, h, &encoded, 0))
    }
}

/// `picture` resampled (box filter when shrinking, nearest when growing) to
/// `width`×`height`.
#[must_use]
pub fn scale_to(picture: &Rgba, width: u32, height: u32) -> Rgba {
    if picture.width == width && picture.height == height {
        return picture.clone();
    }
    let mut data = vec![0_u8; width as usize * height as usize * 4];
    let sx = picture.width as f32 / width as f32;
    let sy = picture.height as f32 / height as f32;
    for y in 0..height {
        let y0 = (y as f32 * sy) as u32;
        let y1 = (((y + 1) as f32 * sy) as u32).clamp(y0 + 1, picture.height);
        for x in 0..width {
            let x0 = (x as f32 * sx) as u32;
            let x1 = (((x + 1) as f32 * sx) as u32).clamp(x0 + 1, picture.width);
            let mut sum = [0_u32; 4];
            let mut count = 0_u32;
            for yy in y0..y1.min(picture.height) {
                for xx in x0..x1.min(picture.width) {
                    let i = (yy as usize * picture.width as usize + xx as usize) * 4;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(picture.data[i + c]);
                    }
                    count += 1;
                }
            }
            let o = (y as usize * width as usize + x as usize) * 4;
            for c in 0..4 {
                data[o + c] = (sum[c] / count.max(1)) as u8;
            }
        }
    }
    Rgba {
        width,
        height,
        data,
    }
}

// ---- sharing ---------------------------------------------------------------------

/// This machine's address on the local network, as other devices there see
/// it — the address of the interface the default route leaves by.
///
/// A UDP `connect` sends nothing; it only asks the OS which interface it
/// would use, which is exactly the question. `None` without a network.
#[must_use]
pub fn lan_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("192.0.2.1", 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// The `viewwstudio-cli` binary beside the running one — how the studio runs
/// the CLI's commands as jobs, the same sibling rule the preview child has.
#[must_use]
pub fn cli_binary() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "viewwstudio-cli.exe"
    } else {
        "viewwstudio-cli"
    };
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join(name), dir.parent()?.join(name)]
        .into_iter()
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_parse_in_every_promised_shape() {
        let script = Script::parse(
            "# a comment\n\
             tap \"Add one\" @1s\n\
             tap 120,300 @1.5s\n\
             hold \"Item\" for 800ms @2s\n\
             drag 200,600 -> 200,200 over 400ms @3s\n\
             scroll -300 @4s\n\
             scroll 0,-120 at 100,100 @4.5s\n\
             key enter @5s\n\
             type \"hi @ there\" @6s\n",
        )
        .unwrap();
        assert_eq!(script.steps.len(), 8);
        assert_eq!(
            script.steps[0],
            Step {
                at: Duration::from_secs(1),
                action: Action::Tap(Target::Label("Add one".into()))
            }
        );
        assert_eq!(
            script.steps[3].action,
            Action::Drag {
                from: Target::Point(200.0, 600.0),
                to: (200.0, 200.0),
                over: Duration::from_millis(400)
            }
        );
        assert_eq!(
            script.steps[7].action,
            Action::Type("hi @ there".into()),
            "an @ inside quotes is text, not a time"
        );
    }

    #[test]
    fn untimed_steps_are_spaced_after_the_previous_one() {
        let script = Script::parse("tap \"A\"\ntap \"B\"").unwrap();
        assert_eq!(script.steps[0].at, FIRST_STEP);
        assert_eq!(script.steps[1].at, FIRST_STEP + STEP_GAP + TAP_LENGTH);
    }

    #[test]
    fn a_script_round_trips_through_its_own_text() {
        let text = "tap \"Add one\" @1s\n\
                    drag 10,20 -> 30,40 over 300ms @2s\n\
                    scroll -120 at 50,60 @3s\n\
                    key escape @4s\n\
                    type \"ok\" @4500ms\n";
        let script = Script::parse(text).unwrap();
        assert_eq!(Script::parse(&script.to_text()).unwrap(), script);
    }

    #[test]
    fn bad_steps_are_named_by_line() {
        let error = Script::parse("tap \"A\" @1s\nwiggle 1,2").unwrap_err();
        assert!(error.starts_with("line 2:"), "{error}");
        assert!(error.contains("tap, hold, drag"), "{error}");
        assert!(Script::parse("tap \"A\" @2s\ntap \"B\" @1s")
            .unwrap_err()
            .contains("before the previous"));
        assert!(Script::parse("key hyper").unwrap_err().contains("enter"));
    }

    #[test]
    fn backgrounds_parse() {
        assert_eq!(parse_background("transparent").unwrap(), None);
        assert_eq!(
            parse_background("#17161a").unwrap(),
            Some(Color::rgba(0x17, 0x16, 0x1a, 255))
        );
        assert!(parse_background("teal-ish").is_err());
    }

    #[test]
    fn rounded_corners_cover_inside_and_not_the_corner() {
        assert_eq!(rounded_coverage(50, 50, 100, 100, 20.0), 1.0);
        assert_eq!(rounded_coverage(0, 0, 100, 100, 20.0), 0.0);
        assert_eq!(rounded_coverage(0, 50, 100, 100, 20.0), 1.0);
        assert_eq!(rounded_coverage(0, 0, 100, 100, 0.0), 1.0);
    }

    #[test]
    fn over_is_straight_alpha() {
        let mut dst = [0, 0, 0, 0];
        over(&mut dst, [255, 0, 0, 255], 0.5);
        assert_eq!(
            dst,
            [255, 0, 0, 128],
            "half coverage onto nothing keeps the colour"
        );
        let mut dst = [0, 0, 255, 255];
        over(&mut dst, [255, 0, 0, 255], 1.0);
        assert_eq!(dst, [255, 0, 0, 255]);
    }

    #[test]
    fn scaling_halves_by_averaging() {
        let picture = Rgba {
            width: 2,
            height: 1,
            data: vec![0, 0, 0, 255, 200, 200, 200, 255],
        };
        assert_eq!(scale_to(&picture, 1, 1).data, vec![100, 100, 100, 255]);
    }

    #[test]
    fn the_recorder_turns_input_into_steps() {
        use crate::remote::{Pointer, RemoteEvent};
        start_recording();
        record_event(&RemoteEvent::Pointer(Pointer::Down(Offset::new(
            10.0, 20.0,
        ))));
        record_event(&RemoteEvent::Pointer(Pointer::Up(Offset::new(10.0, 20.0))));
        record_event(&RemoteEvent::Pointer(Pointer::Down(Offset::new(
            10.0, 20.0,
        ))));
        record_event(&RemoteEvent::Pointer(Pointer::Move(Offset::new(
            10.0, 120.0,
        ))));
        record_event(&RemoteEvent::Pointer(Pointer::Up(Offset::new(10.0, 120.0))));
        record_frame(1, 1, &[1, 2, 3, 255]);
        record_frame(1, 1, &[1, 2, 3, 255]);
        let recording = stop_recording().unwrap();
        assert!(!is_recording());
        assert_eq!(recording.frames.len(), 1, "an unchanged frame is folded");
        assert!(matches!(
            recording.script.steps[0].action,
            Action::Tap(Target::Point(_, _))
        ));
        assert!(matches!(
            recording.script.steps[1].action,
            Action::Drag { .. }
        ));
        // And what it wrote reads back.
        assert!(Script::parse(&recording.script.to_text()).is_ok());
    }

    #[test]
    fn a_frame_surrounds_the_screen_and_keeps_its_pixels() {
        let framer = Framer::new(Platform::Ios, false, false, 120, 240).unwrap();
        let (w, h) = framer.size();
        let (_, bezel) = shell_metrics(Platform::Ios);
        assert_eq!(w, 120 + 2 * (bezel as u32 + MARGIN));
        assert_eq!(h, 240 + 2 * (bezel as u32 + MARGIN));
        let screen = Rgba::filled(120, 240, Color::rgba(10, 200, 30, 255));
        let framed = framer.frame(&screen, None).unwrap();
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [
                framed.data[i],
                framed.data[i + 1],
                framed.data[i + 2],
                framed.data[i + 3],
            ]
        };
        let (ox, oy) = framer.origin;
        assert_eq!(
            at(ox + 60, oy + 120),
            [10, 200, 30, 255],
            "the screen's middle is the screen"
        );
        assert_eq!(at(0, 0)[3], 0, "the margin stays transparent for a PNG");
        assert_ne!(
            at(ox, oy),
            [10, 200, 30, 255],
            "the screen's corner is rounded away"
        );
        assert!(
            framer
                .frame(&Rgba::filled(10, 10, Color::WHITE), None)
                .is_err(),
            "a screen of another size is refused, not stretched"
        );
    }

    #[test]
    fn a_recording_becomes_a_framed_gif() {
        let framer = Framer::new(Platform::Android, false, true, 60, 120).unwrap();
        let recording = Recording {
            frames: vec![
                (
                    Duration::ZERO,
                    Rgba::filled(120, 240, Color::rgba(0, 0, 0, 0)),
                ),
                (
                    Duration::from_millis(500),
                    Rgba::filled(120, 240, Color::rgba(255, 0, 0, 255)),
                ),
            ],
            length: Duration::from_secs(1),
            script: Script::default(),
        };
        let gif = recording.gif(Some(&framer), Color::WHITE).unwrap();
        assert!(
            gif.starts_with(b"GIF89a"),
            "a GIF, scaled from the pane's 2x"
        );
    }
}
