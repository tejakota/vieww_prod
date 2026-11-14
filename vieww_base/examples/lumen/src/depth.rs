//! Depth — the theme as a dimension.
//!
//! Every surface is given one number, its `z`, and everything that tells the
//! eye "this is closer" is derived from it, so the cues can never disagree:
//!
//! - **shadow** — longer, softer and darker the higher the surface sits, cast
//!   away from the one light (up and to the left);
//! - **rim** — the edge facing the light catches more of it the closer the
//!   surface is;
//! - **lift** — a closer surface is lit a little more, a farther one sinks
//!   toward the ground and cools: atmospheric perspective;
//! - **depth of field** — what lies under a raised surface is blurred by the
//!   distance between them, so a dialog lifts off the screen instead of
//!   dimming it;
//! - **parallax** — under tilt, layers move by their depth.
//!
//! # Light costs battery, shadow does not
//!
//! On an OLED panel every pixel emits its own light, so a theme's energy is
//! roughly how much light it puts out. Depth therefore comes from *darkness*:
//! a near-black ground, surfaces lifted by small steps of lightness, shadows
//! (which are free), and exactly one coloured light — the action that matters.
//! The blur and the parallax are compute, not light; [`Power::Saver`] drops
//! both and keeps every other cue, so the screen still reads in depth.

use vieww::prelude::*;
use vieww_foundation::{
    Border, BoxDecoration, Color, FontWeight, Gradient, Offset, Shadow, Size, Transform,
};

use super::{accent, accent_hi, p3a, text, HAIR, INK, LIGHT, MUTED};

/// How much the platform lets the theme spend.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Power {
    /// Blur, parallax and the one glow.
    Full,
    /// Battery saver: no blur (surfaces go opaque), no parallax, no glow.
    Saver,
}

/// The ground: near-black, so it costs almost nothing to show.
pub const GROUND: Color = Color::rgb(4, 5, 9);

/// What one frame of the depth screen shows.
#[derive(Clone, Copy)]
pub struct Frame {
    /// Device tilt, -1 (left) to 1 (right).
    pub tilt: f32,
    /// How far the dialog has opened, 0 to 1.
    pub dialog: f32,
    pub power: Power,
}

/// A surface's lightness at height `z`: each step up is lit a little more.
fn lift(z: f32) -> Color {
    let t = (z / 6.0).clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32| (a + (b - a) * t) as u8;
    // Low surfaces cool toward the ground's blue; high ones warm to neutral.
    Color::rgb(lerp(12.0, 38.0), lerp(15.0, 40.0), lerp(24.0, 48.0))
}

/// The shadow a surface at `z` casts, away from the light.
fn shadow(z: f32) -> Shadow {
    Shadow::new(
        Color::rgba(0, 0, 0, (90.0 + z * 22.0).min(230.0) as u8),
        Offset::new(-LIGHT.dx * z * 2.0, -LIGHT.dy * z * 3.0),
        2.0 + z * 7.0,
    )
}

/// The rim the light catches on the surface's top edge.
fn rim(z: f32) -> Shadow {
    Shadow::inset(
        Color::rgba(255, 255, 255, (10.0 + z * 9.0).min(70.0) as u8),
        Offset::new(0.0, HAIR * 2.0),
        0.0,
    )
}

/// How far a layer at `z` moves under `tilt`.
fn parallax(z: f32, frame: Frame) -> Offset {
    if frame.power == Power::Saver {
        return Offset::ZERO;
    }
    Offset::new(frame.tilt * z * 3.0, frame.tilt.abs() * z * -0.6)
}

/// A surface at height `z`. In full power it is glass over what lies below,
/// blurred by its height; in saver it is the same colour, opaque.
pub fn surface(z: f32, radius: f32, frame: Frame, child: impl Into<WidgetNode>) -> WidgetNode {
    let face = BoxDecoration::new()
        .radius(radius)
        .border(Border::new(
            Color::rgba(255, 255, 255, (8.0 + z * 4.0) as u8),
            HAIR,
        ))
        .shadow(rim(z));
    let body: WidgetNode = match frame.power {
        Power::Full => Clip::rounded(radius)
            .child(
                Filtered::blur(4.0 + z * 4.0).with_backdrop().child(
                    Container::new()
                        .decoration(face.color(lift(z).with_alpha(225)))
                        .child(child),
                ),
            )
            .into(),
        Power::Saver => Container::new()
            .decoration(face.color(lift(z)))
            .child(child)
            .into(),
    };
    Transformed::new(Transform::translate(parallax(z, frame)))
        .child(
            Container::new()
                .decoration(BoxDecoration::new().radius(radius).shadow(shadow(z)))
                .child(body),
        )
        .into()
}

/// The one light on the screen: the primary action. In saver it keeps its
/// colour and loses its glow.
fn action(label: &str, frame: Frame) -> WidgetNode {
    let mut deco = BoxDecoration::new().stadium().gradient(
        Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0))
            .with_dither()
            .with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
    );
    deco = deco.shadow(Shadow::inset(
        Color::rgba(255, 255, 255, 110),
        Offset::new(0.0, HAIR * 2.0),
        0.0,
    ));
    let glow = if frame.power == Power::Full {
        Shadow::new(p3a(124, 92, 255, 90), Offset::new(0.0, 6.0), 18.0)
    } else {
        shadow(2.0)
    };
    Transformed::new(Transform::translate(parallax(3.0, frame)))
        .child(
            Container::new()
                .decoration(BoxDecoration::new().stadium().shadow(glow))
                .child(
                    Container::new()
                        .decoration(deco)
                        .height(52.0)
                        .alignment(Alignment::CENTER)
                        .child(text(label, 16.0, FontWeight::Bold, Color::WHITE)),
                ),
        )
        .into()
}

fn toggle(on: bool) -> WidgetNode {
    let track = if on {
        BoxDecoration::new()
            .stadium()
            .gradient(Gradient::horizontal().with_stops(&[(0.0, accent()), (1.0, accent_hi())]))
    } else {
        BoxDecoration::new()
            .stadium()
            .color(Color::rgb(10, 12, 18))
            .shadow(Shadow::inset(
                Color::rgba(0, 0, 0, 200),
                Offset::new(0.0, 2.0),
                4.0,
            ))
    };
    Container::new()
        .decoration(track)
        .size(52.0, 32.0)
        .padding(EdgeInsets::all(3.0))
        .alignment(if on {
            Alignment::CENTER_RIGHT
        } else {
            Alignment::CENTER_LEFT
        })
        .child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .stadium()
                        .gradient(
                            Gradient::linear(Offset::new(0.0, 0.0), Offset::new(0.6, 1.0))
                                .with_stops(&[
                                    (0.0, Color::rgb(236, 239, 246)),
                                    (1.0, Color::rgb(170, 178, 196)),
                                ]),
                        )
                        .shadow(shadow(1.5)),
                )
                .size(26.0, 26.0),
        )
        .into()
}

fn row(title: &str, detail: &str, on: bool) -> WidgetNode {
    Container::new()
        .height(56.0)
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(children![
                    Flexible::expanded(1).child(
                        Flex::column()
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(3.0)
                            .children(children![
                                text(title, 15.0, FontWeight::Medium, INK),
                                text(detail, 12.0, FontWeight::Regular, MUTED),
                            ]),
                    ),
                    toggle(on),
                ]),
        )
        .into()
}

/// A recessed well: the opposite of a surface — below the ground, lit from
/// inside its top edge by nothing, shadowed by its own rim.
fn well(label: &str, value: &str) -> WidgetNode {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(6.0)
        .children(children![
            text(label, 12.0, FontWeight::Medium, MUTED),
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .radius(14.0)
                        .color(Color::rgb(2, 3, 6))
                        .border(Border::new(Color::rgba(255, 255, 255, 14), HAIR))
                        .shadow(Shadow::inset(
                            Color::rgba(0, 0, 0, 255),
                            Offset::new(1.0, 3.0),
                            6.0
                        )),
                )
                .height(46.0)
                .padding(EdgeInsets::symmetric(16.0, 0.0))
                .alignment(Alignment::CENTER_LEFT)
                .child(text(value, 15.0, FontWeight::Regular, INK)),
        ])
        .into()
}

/// Far layers for the parallax to move against: dim, cool, low-contrast
/// shapes deep below the content — the screen's horizon.
fn far(frame: Frame) -> WidgetNode {
    let slab = |left: f32, top: f32, w: f32, h: f32, z: f32| -> WidgetNode {
        Positioned::new()
            .left(left)
            .top(top)
            .child(
                Transformed::new(Transform::translate(parallax(z, frame))).child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .radius(28.0)
                                .color(Color::rgb(9, 11, 18)),
                        )
                        .size(w, h),
                ),
            )
            .into()
    };
    Stack::new()
        .fit(StackFit::Expand)
        .push(Positioned::fill().child(Container::new().color(GROUND).child(SizedBox::expand())))
        .push(slab(-60.0, 90.0, 260.0, 200.0, -3.0))
        .push(slab(210.0, 300.0, 240.0, 260.0, -2.0))
        .push(slab(-40.0, 600.0, 300.0, 220.0, -3.0))
        .into()
}

fn content(frame: Frame) -> WidgetNode {
    let hero = surface(
        2.0,
        24.0,
        frame,
        Padding::new(EdgeInsets::all(18.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(5.0)
                .children(children![
                    text("TONIGHT", 11.0, FontWeight::Bold, accent_hi()),
                    text("Your evening, in focus", 22.0, FontWeight::Bold, INK),
                    text(
                        "Three things left, one call at 9. Everything else can wait.",
                        14.0,
                        FontWeight::Regular,
                        MUTED
                    ),
                    SizedBox::height(8.0),
                    progress(0.62),
                    text("62% of today done", 12.0, FontWeight::Medium, MUTED),
                ]),
        ),
    );
    let settings = surface(
        1.0,
        20.0,
        frame,
        Padding::new(EdgeInsets::symmetric(18.0, 4.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(children![
                    row("Focus mode", "Silence everything but calls", true),
                    Container::new()
                        .color(Color::rgba(255, 255, 255, 16))
                        .height(HAIR),
                    row("Ambient light", "Follow the time of day", false),
                ]),
        ),
    );
    Padding::new(EdgeInsets::only(20.0, 52.0, 20.0, 0.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(14.0)
                .children(children![
                    text("Good evening, Teja", 30.0, FontWeight::Bold, INK),
                    hero,
                    well("Search", "Find anything…"),
                    settings,
                    SizedBox::height(4.0),
                    action("Start focus session", frame),
                ]),
        )
        .into()
}

fn progress(value: f32) -> WidgetNode {
    let width = 314.0;
    Stack::new()
        .push(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .stadium()
                        .color(Color::rgb(4, 5, 9))
                        .shadow(Shadow::inset(
                            Color::rgba(0, 0, 0, 255),
                            Offset::new(0.0, 1.0),
                            2.0,
                        )),
                )
                .size(width, 8.0),
        )
        .push(
            Container::new()
                .decoration(BoxDecoration::new().stadium().gradient(
                    Gradient::horizontal().with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
                ))
                .size(width * value, 8.0),
        )
        .into()
}

fn tab_bar(frame: Frame) -> WidgetNode {
    let tabs = ["Home", "Explore", "Library", "You"];
    let mut row = Flex::row();
    for (i, label) in tabs.iter().enumerate() {
        let on = i == 0;
        row = row.push(
            Flexible::expanded(1).child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .spacing(8.0)
                    .children(children![
                        Container::new()
                            .decoration(if on {
                                BoxDecoration::new().stadium().gradient(
                                    Gradient::horizontal()
                                        .with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
                                )
                            } else {
                                BoxDecoration::new().stadium().color(Color::rgb(70, 78, 96))
                            })
                            .size(if on { 22.0 } else { 6.0 }, 6.0),
                        text(
                            label,
                            12.0,
                            FontWeight::Medium,
                            if on { INK } else { MUTED }
                        ),
                    ]),
            ),
        );
    }
    surface(
        3.0,
        26.0,
        frame,
        Container::new()
            .height(68.0)
            .alignment(Alignment::CENTER)
            .child(row),
    )
}

/// The dialog, at z = 5. Opening it moves it *forward* — up from smaller and
/// lower — while everything under it steps back and goes out of focus by the
/// distance between them.
fn dialog(frame: Frame) -> WidgetNode {
    let t = frame.dialog;
    let scale = 0.92 + 0.08 * t;
    let card = surface(
        5.0,
        26.0,
        Frame {
            tilt: frame.tilt,
            ..frame
        },
        Padding::new(EdgeInsets::all(22.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(10.0)
                .children(children![
                    text("Start a focus session?", 20.0, FontWeight::Bold, INK),
                    text(
                        "Calls from favourites still ring. Everything else waits for 50 minutes.",
                        14.0,
                        FontWeight::Regular,
                        MUTED,
                    ),
                    SizedBox::height(6.0),
                    action("Start", frame),
                    Container::new()
                        .height(44.0)
                        .alignment(Alignment::CENTER)
                        .child(text("Not now", 15.0, FontWeight::Medium, MUTED)),
                ]),
        ),
    );
    Opacity::new(t.clamp(0.0, 1.0))
        .child(
            Transformed::new(
                Transform::scale_around(Offset::new(175.0, 120.0), scale, scale)
                    .then(Transform::translate(Offset::new(0.0, (1.0 - t) * 36.0))),
            )
            .child(card),
        )
        .into()
}

/// The whole screen for one frame.
pub fn screen(frame: Frame) -> WidgetNode {
    let t = frame.dialog;
    // Depth of field: the content is 3 to 5 levels below the dialog, so it
    // blurs by that distance and steps back. Saver keeps the step back and
    // the darkening — both free — and skips the blur.
    let back = 1.0 - 0.05 * t;
    let base = Stack::new()
        .fit(StackFit::Expand)
        .push(Positioned::fill().child(content(frame)))
        .push(
            Positioned::new()
                .left(16.0)
                .right(16.0)
                .bottom(24.0)
                .child(tab_bar(frame)),
        );
    let base: WidgetNode = Transformed::new(Transform::scale_around(
        Offset::new(195.0, 422.0),
        back,
        back,
    ))
    .child(base)
    .into();
    let base: WidgetNode = if frame.power == Power::Full && t > 0.0 {
        Filtered::blur(6.0 * t).child(base).into()
    } else {
        base
    };

    let mut stack = Stack::new()
        .fit(StackFit::Expand)
        .push(Positioned::fill().child(far(frame)))
        .push(Positioned::fill().child(base));
    if t > 0.0 {
        stack = stack
            .push(
                Positioned::fill().child(
                    Container::new()
                        .color(Color::rgba(0, 0, 0, (150.0 * t) as u8))
                        .child(SizedBox::expand()),
                ),
            )
            .push(
                Positioned::new()
                    .left(20.0)
                    .right(20.0)
                    .top(250.0)
                    .child(dialog(frame)),
            );
    }
    SizedBox::from_size(Size::new(390.0, 844.0))
        .child(stack)
        .into()
}
