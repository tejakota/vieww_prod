//! The studio's own controls, drawn rather than themed.
//!
//! The framework's `Slider` and `Switch` are Material's, and the reference
//! draws neither: its slider is a 4-point track under a deep-teal thumb with
//! eleven tick dots, and its switch is a 40 × 22 pill with a 16-point knob.
//! These are those two controls, at the reference's own geometry, keeping the
//! framework's interaction — drag and tap on the slider, an animated travel
//! on the pill — because a control that only looks right is a prop.

use std::cell::Cell;
use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::{Axis, Color, Offset, Size};
use vieww_widget::prelude::{PaintWith, Painting};
use vieww_widget::Semantics;

use crate::palette::{mix, PILL_KNOB_OFF, PILL_KNOB_ON, PILL_OFF, PILL_ON, SLIDER_MINT};

// ---- the slider -------------------------------------------------------------

/// The slider's own width — the reference's track, in its own units.
const TRACK: f32 = 324.0;
/// The track's thickness.
const THICK: f32 = 4.0;
/// The thumb's radius.
const THUMB: f32 = 9.0;
/// The tick dots' radius.
const DOT: f32 = 2.0;
/// The day range the training slider picks from.
const LOW: f32 = 1.0;
const HIGH: f32 = 12.0;
const STEPS: f32 = 11.0;

/// The training slider: the reference's mint track, deep thumb and eleven
/// tick dots, draggable and tappable, snapping to whole sessions at rest.
///
/// `value` is the current count (`1..=12`); `on_pick` receives the new one;
/// `time` is the clock, for the thumb's own breathing. The caller positions
/// the box — its track runs down its own centre.
#[must_use]
pub fn daily_slider(value: f32, time: f32, on_pick: Rc<dyn Fn(f32)>) -> WidgetNode {
    let fraction = ((value - LOW) / STEPS).clamp(0.0, 1.0);

    // Dragging integrates the finger's travel into the value; the arena gives
    // a horizontal drag to this and a vertical one to the scroll the studio
    // lives in, so both work over the same widget.
    let held = Rc::new(Cell::new(value));
    let held_start = held.clone();
    let held_update = held.clone();
    let held_end = held.clone();
    let pick_drag = on_pick.clone();
    let pick_tap = on_pick.clone();
    let pick_end = on_pick;

    let slider = GestureDetector::new()
        .drag_axis(Axis::Horizontal)
        .on_drag_start(move |_| held_start.set(value))
        .on_drag_update(move |details| {
            let next = (held_update.get() + details.delta.dx / TRACK * STEPS)
                .clamp(LOW, HIGH);
            held_update.set(next);
            pick_drag(next);
        })
        .on_drag_end(move |_| {
            // At rest the thumb sits on a dot: whole sessions, as drawn.
            let snapped = held_end.get().round().clamp(LOW, HIGH);
            held_end.set(snapped);
            pick_end(snapped);
        })
        .on_tap(move |tap| {
            let next = LOW + (tap.local.dx / TRACK).clamp(0.0, 1.0) * STEPS;
            pick_tap(next.round().clamp(LOW, HIGH));
        })
        .child(paint_track(fraction, time));

    Semantics::new()
        .role(vieww_widget::SemanticRole::Slider)
        .value(format!("{value:.0}"))
        .label("Daily sessions")
        .child(slider)
        .into()
}

/// The track itself, drawn at the reference's own measures.
fn paint_track(fraction: f32, time: f32) -> Painting {
    Painting::sized(
        Size::new(TRACK, THUMB * 2.0 + 2.0),
        PaintWith::new(move |book, s| {
            let cy = s.height / 2.0;
            let fill = fraction * TRACK;

            // The track: `#29364a`, four points, round ends.
            book.rrect(
                vieww_foundation::Rect::new(0.0, cy - THICK / 2.0, TRACK, cy + THICK / 2.0),
                THICK / 2.0,
                Color::rgb(41, 54, 74),
            );

            // The filled part, mint, under the thumb.
            if fill > THICK {
                book.rrect(
                    vieww_foundation::Rect::new(0.0, cy - THICK / 2.0, fill, cy + THICK / 2.0),
                    THICK / 2.0,
                    crate::palette::mint_gradient(),
                );
            }

            // Eleven tick dots — one per division, none under the thumb.
            for step in 0..=STEPS as u32 {
                #[allow(clippy::cast_precision_loss)]
                let x = step as f32 / STEPS * TRACK;
                if (x - fill).abs() < THUMB {
                    continue;
                }
                book.circle(Offset::new(x, cy), DOT, Color::rgb(98, 115, 139));
            }

            // The thumb: deep teal, a mint rim, breathing as the reference's
            // `bloom` does.
            let breathe = 1.0 + 0.04 * (time * std::f32::consts::TAU / 3.2).sin();
            let c = Offset::new(fill, cy);
            book.circle(c, THUMB * breathe, Color::rgb(20, 46, 49));
            book.ring(c, THUMB * breathe, 2.0, SLIDER_MINT);
        }),
    )
}

// ---- the pill ---------------------------------------------------------------

/// The studio's switch: the reference's 40 × 22 pill, its 16-point knob
/// sliding between `#9aa8bc` on `#2b3649` and `#d8fff7` on `#28645c`, the
/// travel eased over the same 120 ms a framework control takes.
///
/// `on` is the current state; `on_flip` receives the next one. The travel
/// animates inside the press, so a flip mid-press continues from wherever
/// the knob is, as the framework's own switches do.
#[must_use]
pub fn pill(ctx: &BuildContext, on: bool, on_flip: Rc<dyn Fn(bool)>) -> WidgetNode {
    // Resolved now, while the build context is alive: the animation's
    // builder runs after the build has gone.
    let fade = vieww_widget::ThemeData::of(ctx).motion.duration_short;
    Animated::new(f32::from(on)).build(move |travel| {
        let on_flip = on_flip.clone();
        Pressable::new(move |press| pill_painting(travel, press).into())
            .fade(fade)
            .on_tap(move || on_flip(!on))
            .into()
    })
    .into()
}

/// The pill itself, at `travel` (`0` off, `1` on) and `press` (`0..1`).
fn pill_painting(travel: f32, press: f32) -> Painting {
    Painting::sized(
        Size::new(40.0, 22.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            // The reference's `toggleGlow`: the on rim breathes.
            let glow = 0.5 + 0.5 * (std::f32::consts::TAU / 3.0).sin();

            // The track: the off and on fills mixed by travel, its rim the
            // reference's two strokes (`#56657c` at half on `#2b3649`;
            // `#5fe2d0` at 45% and glowing on `#28645c`).
            let fill = mix(PILL_OFF, PILL_ON, travel);
            let rim = mix_rgba(
                Color::rgba(86, 101, 124, 128),
                Color::rgba(95, 226, 208, (115.0 + 40.0 * glow) as u8),
                travel,
            );

            book.rrect(
                vieww_foundation::Rect::new(0.0, 0.0, s.width, s.height),
                11.0,
                fill,
            );
            book.stroke_rrect(
                vieww_foundation::Rect::new(0.0, 0.0, s.width, s.height),
                11.0,
                rim,
                1.0,
            );

            // The knob: 16 points, sliding 11 → 29, easing in a touch under a
            // finger as the framework's own switches are.
            let knob = mix(PILL_KNOB_OFF, PILL_KNOB_ON, travel);
            book.circle(
                Offset::new(11.0 + travel * 18.0, c.dy),
                8.0 * (1.0 - press * 0.08),
                knob,
            );
        }),
    )
}

/// sRGB interpolation that carries the alpha too — the pill's two rims differ
/// in both hue and presence, and lerping one without the other crosses through
/// a colour the reference never drew.
fn mix_rgba(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color::rgba(
        lerp(a.r, b.r),
        lerp(a.g, b.g),
        lerp(a.b, b.b),
        lerp(a.a, b.a),
    )
}
