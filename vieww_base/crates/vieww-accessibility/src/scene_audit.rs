//! Contrast judged over the [`Scene`] as it was actually painted — the
//! companion of [`audit`](super::audit()), which judges the semantics tree.
//!
//! # Why a scene-level check exists at all
//!
//! `theme_audit` checks a `ColorScheme`'s *pairs*, which is exactly right for
//! the built-in schemes and exactly wrong for an application: real text gets
//! its colour from a theme but its *background* from whatever container it
//! happened to be laid out inside, and "on_surface over surface" tells nobody
//! what a grey label on a tinted card actually renders as. The scene is the
//! last artifact before pixels where colour and geometry have already met —
//! auditing *there* is the only static place that can say "this text, as
//! drawn, is 2.9:1".
//!
//! # The painter's-algorithm rule, and its honest limits
//!
//! Commands are recorded in paint order, so the backdrop of a glyph run is
//! the **nearest earlier command that covers it**. That is the rule; the
//! honesty is in what "covers" can resolve to:
//!
//! - An earlier opaque **solid** `FillRect`/`FillPath` (identity transform)
//!   covering the run's bounds is the backdrop, and the pair is judged.
//! - A translucent fill, a gradient, an image, or a run living inside a
//!   compositing layer (or under a transform) is **indeterminate** here —
//!   the true backdrop is a composite this walk refuses to approximate, and
//!   a guessed backdrop is how an audit ends up asserting ratios nobody
//!   will ever see. Those runs are counted in
//!   [`TextContrastReport::skipped`], never silently dropped.
//! - Strokes, shadows and other glyph runs never count as backdrops: they
//!   are ink on top of the page, not the page.
//!
//! Nothing behind the run at all — text drawn straight onto the window's
//! clear colour — is indeterminate too, for the same reason: the clear
//! colour belongs to the platform, not to the scene.
//!
//! # What "large text" means here
//!
//! [`GlyphRun::size`] is the em size in logical pixels, which is the size
//! WCAG's 24px large-text boundary is stated against. Boldness is not in the
//! scene, so the *small*-text threshold (4.5:1) is used below 24px — the
//! conservative direction: bold 18px text judged as small can only fail more
//! often than the standard would, never less.

use vieww_foundation::{Color, Rect};
use vieww_paint::{Command, Paint, Scene};
use vieww_render::FrameDriver;

use crate::contrast::{contrast_ratio, passes, WcagLevel};

/// The em size at and above which WCAG treats text as large (its thresholds
/// relax from 4.5:1 to 3:1).
const LARGE_TEXT_EM: f32 = 24.0;

/// One glyph run whose painted contrast failed its WCAG AA threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct TextContrastFinding {
    /// The text colour as painted (alpha already composited over the
    /// background, because that is the colour a person actually sees).
    pub text: Color,
    /// The backdrop the run was judged against.
    pub background: Color,
    /// The run's em size, which selected the threshold.
    pub em_size: f32,
    /// The measured ratio and the threshold it failed.
    pub ratio: f32,
    pub required: f32,
    /// The run's bounds in scene coordinates, for pointing at the text.
    pub bounds: Rect,
}

/// The result of [`text_contrast`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextContrastReport {
    pub failures: Vec<TextContrastFinding>,
    /// How many runs were judged.
    pub checked: usize,
    /// How many were honestly unjudgeable here — composites, gradients,
    /// images, layers, transforms, or nothing behind them at all.
    pub skipped: usize,
}

impl TextContrastReport {
    /// Whether every judgeable run passed. Skips do not fail a run — they
    /// are reported, and a caller who wants them resolved needs a renderer,
    /// not an audit.
    #[must_use]
    pub fn passes(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Whether `fill` is a solid, opaque, untransformed colour that can serve
/// as a backdrop, and the rect it covers.
fn solid_opaque_fill(command: &Command) -> Option<(Color, Rect)> {
    let (paint, rect, transform) = match command {
        Command::FillRect {
            rect,
            paint,
            transform,
            ..
        } => (paint, *rect, *transform),
        Command::FillPath {
            path,
            paint,
            transform,
            ..
        } => (paint, path.bounds(), *transform),
        _ => return None,
    };
    if transform != vieww_foundation::Transform::IDENTITY {
        return None;
    }
    opaque_solid(paint).map(|color| (color, rect))
}

/// A paint that draws one flat, fully opaque colour — the only paint a
/// contrast ratio can be computed against without guessing.
fn opaque_solid(paint: &Paint) -> Option<Color> {
    if paint.gradient.is_some() || paint.color.is_transparent() {
        return None;
    }
    // Not `> 0` on the alpha channel alone: any alpha short of fully opaque
    // makes the *real* backdrop a composite of what is underneath, which is
    // exactly the thing this walk does not guess at.
    (paint.color.a == u8::MAX).then_some(paint.color)
}

/// How far a run's box may poke past a backdrop's edge and still be judged
/// as sitting on it, as a fraction of the run's em size.
///
/// A glyph run's vertical extent comes from the font's ascent and descent,
/// which are *line* metrics, not ink metrics: they bound every glyph the face
/// could ever draw, and overshoot what the actual string paints — typically
/// by a few percent of the em, more in faces with generous line spacing. The
/// layout that placed the run may also have rounded its own baseline against
/// a different ascent than the one the run reports, by a sub-pixel or two.
/// Demanding the backdrop cover the *metric* box would therefore skip text
/// whose ink is entirely inside it, which is the common case; demanding it
/// cover the ink is impossible, because the scene has no per-glyph ink boxes.
/// A quarter-em of slack is the standard typographic margin between a line
/// box and what any real glyph fills — ink that pokes farther than that past
/// an edge is genuinely off its backdrop, and the run is honestly skipped.
const METRIC_SLACK_EM: f32 = 0.25;

/// Whether a run drawn after `command` would sit on top of `command`'s
/// geometry — i.e. whether `command`'s ink is the run's backdrop.
///
/// Rect containment against the *command's* covering rectangle, with the
/// run's box shrunk by [`METRIC_SLACK_EM`] for the reason that constant's
/// doc gives: a stroke's bounding box would "cover" text it merely
/// surrounds, which is why strokes never reach this function — only fills
/// and images, whose bounds are the area they actually paint.
fn covers(rect: Rect, run: Rect, em: f32) -> bool {
    let slack = em * METRIC_SLACK_EM;
    let left = run.left + slack;
    let right = run.right - slack;
    let top = run.top + slack;
    let bottom = run.bottom - slack;
    // A run shorter than its own slack collapses rather than inverting: an
    // empty box would "cover" against nothing, and an inverted one would
    // cover against everything.
    if left > right || top > bottom {
        return false;
    }
    rect.left <= left && rect.right >= right && rect.top <= top && rect.bottom >= bottom
}

/// The bounding box of a glyph run, in the scene's coordinate space.
fn run_bounds(run: &vieww_foundation::GlyphRun) -> Rect {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for glyph in run.glyphs.iter() {
        left = left.min(run.origin.dx + glyph.offset.dx);
        right = right.max(run.origin.dx + glyph.offset.dx);
    }
    // An empty run (every glyph missing, or none at all) has no geometry to
    // judge; the caller treats a zero-width box as unjudgeable.
    if left == f32::INFINITY {
        left = run.origin.dx;
        right = run.origin.dx;
    }
    // Glyph offsets are *pen* positions: the ink starts a little left of the
    // first pen position and runs to roughly the last one. Judging the box
    // from pen to pen is a hair narrower than the ink; the slack in
    // [`covers`] absorbs that difference in the safe direction (see
    // [`METRIC_SLACK_EM`]).
    Rect::new(
        left,
        run.origin.dy - run.ascent,
        right,
        run.origin.dy + run.descent,
    )
}

/// Judge every glyph run in `scene` for WCAG AA contrast against its own
/// backdrop, per this module's doc on what can and cannot be resolved.
#[must_use]
pub fn text_contrast(scene: &Scene) -> TextContrastReport {
    let commands = scene.commands();
    let mut report = TextContrastReport::default();

    // The compositing-layer depth at each command, resolved in one forward
    // pass: a run inside a layer is composited with that layer's alpha, so
    // its true backdrop is a composite this walk refuses to approximate.
    let mut depth: Vec<u32> = Vec::with_capacity(commands.len());
    let mut open = 0_u32;
    for command in commands {
        depth.push(open);
        match command {
            Command::PushLayer { .. } => open += 1,
            Command::PopLayer => open = open.saturating_sub(1),
            _ => {}
        }
    }

    for (index, command) in commands.iter().enumerate() {
        let Command::DrawGlyphs { run, .. } = command else {
            continue;
        };
        if depth[index] > 0 {
            report.skipped += 1;
            continue;
        }

        let bounds = run_bounds(run);
        if bounds.width() <= 0.0 {
            report.skipped += 1;
            continue;
        }

        // The nearest earlier ink that covers this run is the backdrop.
        let mut backdrop: Option<Color> = None;
        for earlier in commands[..index].iter().rev() {
            let (covers_run, resolvable) = match earlier {
                Command::FillRect { .. } | Command::FillPath { .. } => {
                    match solid_opaque_fill(earlier) {
                        Some((color, rect)) => (covers(rect, bounds, run.size), Some(color)),
                        // An unresolvable fill that covers the run is a real
                        // answer — "indeterminate" — and must stop the search
                        // rather than let it continue to whatever is *under*
                        // the translucent fill, which would flatter the ratio.
                        None => {
                            let rect = match earlier {
                                Command::FillRect { rect, .. } => *rect,
                                Command::FillPath { path, .. } => path.bounds(),
                                _ => unreachable!("matched above"),
                            };
                            (covers(rect, bounds, run.size), None)
                        }
                    }
                }
                Command::DrawImage { rect, .. } => (covers(*rect, bounds, run.size), None),
                // Ink, markers and shadows are never backdrops, whatever
                // their bounding boxes pretend.
                _ => (false, None),
            };
            if !covers_run {
                continue;
            }
            backdrop = resolvable;
            break;
        }

        let Some(background) = backdrop else {
            report.skipped += 1;
            continue;
        };

        // Alpha in the text colour is real: what a person sees is the text
        // composited over its backdrop, so that is the pair to judge.
        let text = if run.color.a == u8::MAX {
            run.color
        } else {
            run.color.over(background)
        };
        let ratio = contrast_ratio(text, background);
        let large = run.size >= LARGE_TEXT_EM;
        if passes(ratio, WcagLevel::Aa, large) {
            report.checked += 1;
        } else {
            report.failures.push(TextContrastFinding {
                text,
                background,
                em_size: run.size,
                ratio,
                required: if large { 3.0 } else { 4.5 },
                bounds,
            });
        }
    }
    report
}

/// `text_contrast` over a driver's current frame — the one-argument form a
/// harness or devtools panel wants.
#[must_use]
pub fn driver_text_contrast(driver: &FrameDriver) -> TextContrastReport {
    text_contrast(driver.scene())
}

#[cfg(test)]
mod tests {
    use vieww_foundation::Size;
    use vieww_test_harness::TestHarness;
    use vieww_widget::prelude::*;

    use super::*;

    fn harness(size: Size) -> TestHarness {
        TestHarness::new(size)
    }

    /// Grey-on-white text at 16px: the canonical AA failure.
    #[test]
    fn low_contrast_text_is_flagged() {
        let mut h = harness(Size::new(200.0, 100.0));
        h.mount(
            Container::new().color(Color::WHITE).child(
                Text::new("low contrast")
                    .size(16.0)
                    .color(Color::rgb(188, 188, 188)),
            ),
        );
        h.tick_and_settle(4);
        let report = driver_text_contrast(h.driver());
        assert!(
            !report.failures.is_empty(),
            "grey on white must fail AA: {report:?}"
        );
        let f = &report.failures[0];
        assert!(f.ratio < 4.5, "the ratio should be under the threshold");
        assert_eq!(f.required, 4.5);
        assert_eq!(f.background, Color::WHITE);
    }

    /// Proper theme-colour text on the same background passes.
    #[test]
    fn sufficient_contrast_text_is_not_flagged() {
        let mut h = harness(Size::new(200.0, 100.0));
        h.mount(
            Container::new().color(Color::WHITE).child(
                Text::new("readable")
                    .size(16.0)
                    .color(Color::rgb(20, 20, 20)),
            ),
        );
        h.tick_and_settle(4);
        let report = driver_text_contrast(h.driver());
        assert!(report.passes(), "dark on white must pass: {report:?}");
        assert!(report.checked >= 1, "the run was actually judged");
    }

    /// Large text relaxes to 3:1 — the same grey that fails at 16px passes
    /// at 32px only if the ratio is over 3, and the report must have used
    /// the large threshold to say so.
    #[test]
    fn large_text_uses_the_relaxed_threshold() {
        let mut h = harness(Size::new(300.0, 100.0));
        // 3.9:1-ish grey: fails 4.5, passes 3.0.
        let grey = Color::rgb(140, 140, 140);
        h.mount(
            Container::new()
                .color(Color::WHITE)
                .child(Text::new("big enough").size(32.0).color(grey)),
        );
        h.tick_and_settle(4);
        let report = driver_text_contrast(h.driver());
        assert!(
            report.passes(),
            "3.9:1 at 32px is large text and passes: {report:?}"
        );
        // And the small threshold would have failed it, proving the size
        // actually selected the threshold.
        assert!(contrast_ratio(grey, Color::WHITE) < 4.5);
    }

    /// Text over a gradient is honestly skipped, never guessed.
    #[test]
    fn text_over_a_gradient_is_skipped_not_judged() {
        let mut h = harness(Size::new(300.0, 120.0));
        let gradient = Gradient::linear(Offset::new(0.0, 0.0), Offset::new(300.0, 0.0))
            .with_stops(&[(0.0, Color::WHITE), (1.0, Color::rgb(120, 120, 120))]);
        h.mount(
            Container::new().gradient(gradient).child(
                Text::new("on a ramp")
                    .size(16.0)
                    .color(Color::rgb(150, 150, 150)),
            ),
        );
        h.tick_and_settle(4);
        let report = driver_text_contrast(h.driver());
        assert!(report.passes());
        assert!(report.skipped >= 1, "the run is counted as unjudgeable");
        assert_eq!(report.checked, 0);
    }

    /// Text inside a compositing layer (an opacity group) is skipped: its
    /// real backdrop is the group composited over the page, which this walk
    /// refuses to approximate — a faded layer judged at full strength is a
    /// false pass, and false passes are the one thing an audit must never
    /// produce.
    #[test]
    fn text_inside_an_opacity_layer_is_skipped() {
        let mut h = harness(Size::new(200.0, 100.0));
        h.mount(Container::new().color(Color::WHITE).child(
            Opacity::new(0.5).child(Text::new("faded").size(16.0).color(Color::rgb(60, 60, 60))),
        ));
        h.tick_and_settle(4);
        let report = driver_text_contrast(h.driver());
        assert_eq!(report.checked, 0, "layer content is not judged: {report:?}");
        assert!(report.skipped >= 1);
    }

    /// Text with nothing opaque behind it — the window's clear colour — is
    /// skipped, because the platform owns that colour, not the scene.
    #[test]
    fn text_on_the_bare_window_is_skipped() {
        let mut h = harness(Size::new(200.0, 100.0));
        h.mount(
            Text::new("floating")
                .size(16.0)
                .color(Color::rgb(150, 150, 150)),
        );
        h.tick_and_settle(4);
        let report = driver_text_contrast(h.driver());
        assert_eq!(report.checked, 0);
        assert!(report.skipped >= 1);
    }

    /// Translucent text is composited before judging — a 50% white on
    /// white is still white, still 1:1, still a failure.
    #[test]
    fn translucent_text_is_composited_first() {
        let mut h = harness(Size::new(200.0, 100.0));
        h.mount(
            Container::new().color(Color::WHITE).child(
                Text::new("half there")
                    .size(16.0)
                    .color(Color::rgba(255, 255, 255, 128)),
            ),
        );
        h.tick_and_settle(4);
        let report = driver_text_contrast(h.driver());
        assert!(!report.passes(), "50% white on white is 1:1: {report:?}");
        assert_eq!(report.failures[0].ratio, 1.0);
    }
}
