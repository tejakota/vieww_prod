//! Home — the screen the app opens on.
//!
//! The screen is the persona's room, and everything in it answers one of two
//! questions: *who lives here* — the companion at the centre, the greeting,
//! the caption — and *what do I do* — the one primary action, **New
//! decision**, the four domains it can be brought from, the last thing U
//! noted, and the little nudge that offers a question when one is due. The
//! question itself is not on this screen: it arrives as the ask sheet, pushed
//! by the action, a domain, the recent strip or the nudge.
//!
//! # The layout is the reference's own
//!
//! Every block is placed at the reference SVG's own coordinates, inside the
//! 420 × 920 canvas it was drawn on — the sigil's eye at (37, 44), the
//! companion at (210, 373), the action at (210, 605), the categories at
//! y = 720, the strip at y = 810, the nudge at y = 878. The [`crate::stage`]
//! puts that canvas on whatever screen the app wakes up on, so the spacing
//! the reference drew is the spacing the app keeps — nothing is reflowed,
//! nothing is inferred, and a window of the design's own size is the identity.
//!
//! # The opening
//!
//! The app opens on a splash that is this same screen assembling: the eye
//! appears, the six tendrils grow out of it, the atmosphere arrives, the
//! wordmark rises under it — and then the wordmark hands its place over to
//! the home content while **the companion never moves**. That is the whole
//! trick of the handoff: splash and home are one layout, the icon is painted
//! by the same painter at the same size in the same slot, so what changes
//! around it is everything except the thing you were looking at. A tap
//! anywhere while it plays finishes it at once.
//!
//! # The way out
//!
//! Settings opens from the ALIVE chip at the top right — the pulse is
//! tappable, because the one thing a status that glows invites is a look
//! inside — and from a right-to-left swipe anywhere on the screen, because a
//! gesture that opens a drawer from its edge is how a phone says "there is
//! more over here".

use std::cell::Cell;
use std::f32::consts::TAU;
use std::rc::Rc;

use vieww::prelude::*;
use vieww_foundation::{
    Axis, BlendMode, Color, FontWeight, Offset, Path, Rect, Size, Sketchbook, TextStyle, Transform,
};
use vieww_widget::prelude::{PaintWith, Painting};

use crate::neurons::{self, Reveal};
use crate::palette::{self, AXON, CHIP_DOT, CHIP_EDGE, EDGE, INK, INK_GREET, MUTED, SYNAPSE};
use crate::stage;
use crate::{ask, ask_route, settings_route, take, AppState, Shared};

// ---- the opening's timeline, in seconds since the app opened --------------

/// The eye scales in.
const EYE_AT: f32 = 0.12;
const EYE_DUR: f32 = 0.62;
/// The strands grow, one after another, out of the eye.
const STRAND_AT: f32 = 0.80;
const STRAND_STEP: f32 = 0.08;
const STRAND_DUR: f32 = 0.62;
/// The rings, the aura and the float arrive.
const ATMOS_AT: f32 = 1.70;
const ATMOS_DUR: f32 = 0.55;
/// The wordmark, the tagline, the footer and its dots.
const WORD_AT: f32 = 2.05;
const WORD_DUR: f32 = 0.45;
const TAG_AT: f32 = 2.35;
const TAG_DUR: f32 = 0.40;
const FOOT_AT: f32 = 2.65;
const FOOT_DUR: f32 = 0.40;
/// The splash's text leaves; the home content arrives.
const EXIT_AT: f32 = 3.35;
const EXIT_DUR: f32 = 0.40;
const HOME_AT: f32 = 3.60;
const HOME_DUR: f32 = 0.55;
const HOME_STEP: f32 = 0.07;
/// The last moment a tap still counts as skipping the opening.
const SKIP_END: f32 = EXIT_AT;

/// Where a glyph's baseline sits below its line box's top, as a fraction of
/// the font size: Inter's own ascent, which the framework top-aligns the
/// line box against. Measured against the rasterizer rather than assumed —
/// placing a box is only placing a baseline if this number is true.
const ASCENT: f32 = 0.969;

/// A left-aligned text box top for a baseline at `baseline`.
fn top_of(baseline: f32, size: f32) -> f32 {
    baseline - ASCENT * size
}

/// The four domains a decision can be brought from, as the tags the bank
/// files its questions under.
const CAREER: &[&str] = &["time", "standards", "persistence"];
const PERSONAL: &[&str] = &["energy", "conflict", "pride"];
const FINANCE: &[&str] = &["money"];
const LIFE: &[&str] = &["impulse", "control", "risk", "discipline"];

#[derive(Debug)]
pub struct HomeScreen {
    pub state: AppState,
    pub shared: Shared,
}

#[widget]
impl HomeScreen {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        let state = self.state.clone();
        let t = state.time.get();
        let burst = state.burst.get();
        let calm = state.calm.get();
        let intro = Intro::at(t, state.skip_at.get());

        let answers = state.answers.get();
        let noted = state.noted.get();
        let nudge_up = state.nudge.get();

        // The room's ambient life arrives with the splash's atmosphere stage
        // and stays; calm keeps a third of it.
        let amb = if calm { 0.3 } else { 1.0 } * intro.amb;

        let mut screen = Stack::new().fit(StackFit::Expand).push(Positioned::fill().child(
            neurons::atmosphere(t, amb),
        ));

        // -- the room, at the reference's own coordinates --------------------
        screen = screen
            .push(arrive(intro.blocks[0], -12.0, top_bar(&self.shared, ctx, t)))
            .push(place(24.0, top_of(132.0, 11.0)).child(arrive(
                intro.blocks[1],
                -8.0,
                daypart_text(),
            )))
            .push(place(25.0, top_of(166.0, 31.0)).child(arrive(
                intro.blocks[1],
                -8.0,
                name_text(&state),
            )))
            .push(place(86.0, 147.0).child(arrive(intro.blocks[1], -8.0, name_spark(t).into())))
            .push(place(25.0, top_of(190.0, 12.0)).child(arrive(
                intro.blocks[1],
                -8.0,
                Text::new("Ready when you are.").size(12.0).color(MUTED).into(),
            )))
            // The companion — the mark the splash assembles, at the exact
            // slot it never leaves: centre (210, 373), 276 across.
            .push(place(72.0, 235.0).child(neurons::companion(
                276.0,
                t,
                burst,
                if calm { 1.0 } else { 0.0 },
                intro.reveal,
            )))
            .push(below_slot(&intro, t))
            .push(arrive(intro.blocks[3], 14.0, action(&self.shared, ctx, t)))
            .push(arrive(intro.blocks[4], 16.0, categories(&state, &self.shared, ctx)));

        // -- the tail: whatever was noted last, and the nudge ---------------
        if let Some(noted) = noted {
            screen = screen.push(place(24.0, top_of(798.0, 9.0)).child(arrive(
                intro.blocks[4],
                10.0,
                recent_label(),
            )));
            screen = screen.push(place(24.0, 810.0).child(arrive(
                intro.blocks[4],
                10.0,
                recent(&self.shared, ctx, noted),
            )));
        }
        if nudge_up {
            screen = screen.push(place(24.0, 878.0).child(arrive(
                intro.blocks[4],
                10.0,
                nudge(&state, &self.shared, ctx, answers),
            )));
        }

        // While the opening plays, a tap anywhere finishes it: the layer sits
        // above the content, so the tap cannot fall through to a control that
        // is not there yet.
        if intro.splashing {
            let state_for_skip = state.clone();
            screen = screen.push(
                Positioned::fill().child(
                    GestureDetector::new()
                        .on_tap(move |_tap| {
                            let now = state_for_skip.time.peek();
                            state_for_skip.skip_at.set(now.min(SKIP_END));
                        })
                        .child(SizedBox::expand()),
                ),
            );
        }

        // The way out: a swipe from right to left opens settings — a fling,
        // or a firm pull. Anything gentler was a scroll that went sideways,
        // and home has nothing to scroll.
        let travel = Rc::new(Cell::new(0.0_f32));
        let travel_start = travel.clone();
        let travel_update = travel.clone();
        let shared_for_swipe = self.shared.clone();
        WidgetNode::new(
            GestureDetector::new()
                .drag_axis(Axis::Horizontal)
                .on_drag_start(move |_details| travel_start.set(0.0))
                .on_drag_update(move |details| {
                    travel_update.set(travel_update.get() + details.delta.dx);
                })
                .on_drag_end(move |details| {
                    let state = take(&shared_for_swipe);
                    let now = state.time.peek();
                    let opened = now >= EXIT_AT + EXIT_DUR || now >= state.skip_at.peek() + 0.35;
                    if opened && (travel.get() <= -48.0 || details.velocity.dx <= -420.0) {
                        state.nav.push(settings_route(shared_for_swipe.clone()));
                    }
                })
                .child(stage::screen(screen)),
        )
    }
}

/// A `Positioned` at the reference's own coordinates.
fn place(left: f32, top: f32) -> Positioned {
    Positioned::new().left(left).top(top)
}

// ---- the opening, as numbers ----------------------------------------------

/// Where in the opening this frame is. Everything is a pure function of
/// `(t, skip_at)`, so any frame of it is reproducible from the clock alone.
#[derive(Debug)]
struct Intro {
    /// The companion's assembly.
    reveal: Reveal,
    /// The splash text's departure, `0..1`.
    exit: f32,
    /// The wordmark, tagline and footer alphas — already including the exit.
    word: f32,
    tagline: f32,
    footer: f32,
    /// The home blocks, staggered: bar, greeting, caption, action, tail.
    blocks: [f32; 5],
    /// The background atmosphere's presence — it arrives with the splash's
    /// atmosphere stage, and a skip fades it in from the moment of the tap.
    amb: f32,
    /// Still in the opening: the skip layer is up.
    splashing: bool,
}

/// A clamp-to-zero ramp.
fn ramp(t: f32, at: f32, dur: f32) -> f32 {
    ((t - at) / dur).clamp(0.0, 1.0)
}

/// The ease of things that arrive: quick, then settling.
fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

impl Intro {
    #[allow(clippy::cast_precision_loss)]
    fn at(t: f32, skip_at: f32) -> Self {
        let skipped = t >= skip_at;
        let (reveal, exit, word, tagline, footer, blocks, amb) = if skipped {
            // A skip resolves the assembly at once and fades the room in
            // over a third of a second from the moment of the tap.
            let fade = ease_out(ramp(t, skip_at, 0.30));
            (Reveal::complete(), 1.0, 0.0, 0.0, 0.0, [fade; 5], fade)
        } else {
            let eye = ease_out(ramp(t, EYE_AT, EYE_DUR));
            let mut strands = [0.0_f32; 6];
            for (i, grown) in strands.iter_mut().enumerate() {
                let at = STRAND_AT + i as f32 * STRAND_STEP;
                *grown = ramp(t, at, STRAND_DUR);
            }
            let exit = ramp(t, EXIT_AT, EXIT_DUR);
            let blocks = [
                ease_out(ramp(t, HOME_AT, HOME_DUR)),
                ease_out(ramp(t, HOME_AT + HOME_STEP, HOME_DUR)),
                ease_out(ramp(t, HOME_AT + 2.0 * HOME_STEP, HOME_DUR)),
                ease_out(ramp(t, HOME_AT + 3.0 * HOME_STEP, HOME_DUR)),
                ease_out(ramp(t, HOME_AT + 4.0 * HOME_STEP, HOME_DUR)),
            ];
            let atmosphere = ramp(t, ATMOS_AT, ATMOS_DUR);
            (
                Reveal { eye, strands, atmosphere },
                exit,
                ramp(t, WORD_AT, WORD_DUR) * (1.0 - exit),
                ramp(t, TAG_AT, TAG_DUR) * (1.0 - exit),
                ramp(t, FOOT_AT, FOOT_DUR) * (1.0 - exit),
                blocks,
                atmosphere,
            )
        };
        Self {
            reveal,
            exit,
            word,
            tagline,
            footer,
            blocks,
            amb,
            splashing: !skipped && t < SKIP_END,
        }
    }
}

/// One block of the room arriving: fading in, and rising from `rise` pixels
/// below where it lives. The movement is paint-time — the flight costs no
/// layout.
fn arrive(alpha: f32, rise: f32, child: WidgetNode) -> WidgetNode {
    Opacity::new(alpha)
        .child(
            Transformed::new(Transform::translate(Offset::new(0.0, rise * (1.0 - alpha))))
                .child(child),
        )
        .into()
}

// ---- the room --------------------------------------------------------------

/// The mark's signature, the name it stands beside, and the ALIVE chip —
/// the room's one exit besides the swipe. All at the reference's own marks:
/// the sigil's eye at (37, 44), the wordmark's baseline at y = 51, the chip
/// at (320, 36).
fn top_bar(shared: &Shared, ctx: &BuildContext, t: f32) -> WidgetNode {
    let shared_for_chip = shared.clone();
    let chip = Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(20, 35, 56, 255))
                    .radius(15.0)
                    .border(Border::new(
                        CHIP_EDGE.with_alpha((71.0 + press * 120.0) as u8),
                        1.0,
                    )),
            )
            .height(30.0)
            .padding(EdgeInsets::symmetric(13.0, 0.0))
            .child(
                Flex::row()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .spacing(8.0)
                    .children(children![
                        alive_dot(t),
                        Text::new("ALIVE").style(
                            TextStyle::new(9.5)
                                .weight(FontWeight::Medium)
                                .letter_spacing(0.4)
                                .color(Color::rgb(156, 233, 220)),
                        ),
                    ]),
            )
            .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_chip);
        state.nav.push(settings_route(shared_for_chip.clone()));
    });

    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .push(place(20.0, 27.0).child(neurons::sigil(34.0, t, 0.0)))
            .push(place(64.0, top_of(51.0, 24.0)).child(Text::new("U").style(
                TextStyle::new(24.0)
                    .weight(FontWeight::Bold)
                    .letter_spacing(-1.0)
                    .color(INK),
            )))
            .push(place(65.0, top_of(65.0, 7.5)).child(Text::new("PERSONA").style(
                TextStyle::new(7.5).letter_spacing(1.6).color(Color::rgb(115, 130, 156)),
            )))
            .push(place(320.0, 36.0).child(chip)),
    )
    .into()
}

/// The chip's pulse: a small synapse dot, slowly twinkling — the reference's
/// own (334, 51) r = 3.
fn alive_dot(t: f32) -> Painting {
    Painting::sized(
        Size::square(9.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let twinkle = 0.5 + 0.5 * (t * TAU / 3.0).sin();
            book.circle(
                c,
                2.1 + twinkle * 0.6,
                CHIP_DOT.with_alpha((150.0 + 90.0 * twinkle) as u8),
            );
        }),
    )
}

/// The greeting's first line — 11 pt caps at the reference's y = 132.
fn daypart_text() -> WidgetNode {
    Text::new(daypart())
        .style(
            TextStyle::new(11.0)
                .weight(FontWeight::Medium)
                .letter_spacing(1.9)
                .color(Color::rgb(131, 146, 170)),
        )
        .into()
}

/// The name — 31 pt at the reference's y = 166.
fn name_text(state: &AppState) -> WidgetNode {
    Text::new(state.display_name())
        .style(
            TextStyle::new(31.0)
                .weight(FontWeight::Bold)
                .letter_spacing(-1.3)
                .color(INK_GREET),
        )
        .into()
}

/// The small spark that rides the name — a charm, not a status, at the
/// reference's (93, 154).
fn name_spark(t: f32) -> Painting {
    Painting::sized(
        Size::square(8.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let twinkle = 0.5 + 0.5 * (t * TAU / 3.0 + 0.9).sin();
            book.circle(
                c,
                1.9 + twinkle * 0.7,
                AXON.with_alpha((90.0 + 140.0 * twinkle) as u8),
            );
        }),
    )
}

/// The part of the day for the greeting.
///
/// UTC + 5:30: the prototype greets in the one timezone its one user is in,
/// the same compromise the day counter already owns — `std` can produce the
/// instant but not the zone, and the store that replaces the log owns time
/// properly. In London this greets five and a half hours early; it greets
/// honestly.
fn daypart() -> &'static str {
    let hours = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(12.0, |since| since.as_secs_f32() / 3600.0);
    let local = (hours + 5.5).rem_euclid(24.0);
    if local < 5.0 {
        "GOOD NIGHT"
    } else if local < 12.0 {
        "GOOD MORNING"
    } else if local < 17.0 {
        "GOOD AFTERNOON"
    } else if local < 22.0 {
        "GOOD EVENING"
    } else {
        "GOOD NIGHT"
    }
}

/// The slot under the companion: the splash's wordmark lives here until the
/// room arrives, then the caption does — a crossfade between two absolutely
/// placed stacks, so nothing around either ever shifts.
///
/// The caption is the reference's own — `YOUR U` at y = 525, its line at
/// y = 544, both centred on the mark. The splash's text is the opening's own:
/// the landing rings the mark settles onto, the wordmark, its subtitle, the
/// tagline, the loader and the privacy line.
fn below_slot(intro: &Intro, t: f32) -> WidgetNode {
    Positioned::fill().child(
    Stack::new()
        .fit(StackFit::Expand)
        .push(
            Positioned::fill().child(
                Opacity::new(1.0 - intro.exit).child(
                    Stack::new()
                        .fit(StackFit::Expand)
                        .push(place(130.0, 495.0).child(Opacity::new(intro.word).child(
                            Align::new(Alignment::CENTER).child(landing_rings()),
                        )))
                        .push(place(0.0, top_of(536.0, 40.0)).width(420.0).child(Opacity::new(intro.word).child(
                            Text::new("U").style(
                                TextStyle::new(40.0)
                                    .weight(FontWeight::Bold)
                                    .letter_spacing(-2.5)
                                    .color(Color::rgb(241, 239, 255)),
                            )
                            .align(TextAlign::Center),
                        )))
                        .push(place(0.0, top_of(551.0, 8.5)).width(420.0).child(Opacity::new(intro.word).child(
                            Text::new("PERSONA COMPANION").style(
                                TextStyle::new(8.5)
                                    .weight(FontWeight::Medium)
                                    .letter_spacing(2.6)
                                    .color(Color::rgb(155, 139, 220)),
                            )
                            .align(TextAlign::Center),
                        )))
                        .push(place(0.0, top_of(572.0, 11.0)).width(420.0).child(Opacity::new(intro.tagline).child(
                            Text::new("A little more you, every day.")
                                .size(11.0)
                                .color(MUTED)
                                .align(TextAlign::Center),
                        )))
                        .push(place(187.0, 585.0).child(Opacity::new(intro.footer).child(
                            Align::new(Alignment::CENTER).child(loading_dots(t)),
                        )))
                        .push(place(0.0, top_of(612.0, 7.5)).width(420.0).child(Opacity::new(intro.footer).child(
                            Text::new("PRIVATE BY DESIGN").style(
                                TextStyle::new(7.5)
                                    .weight(FontWeight::Medium)
                                    .letter_spacing(2.0)
                                    .color(Color::rgb(83, 98, 123)),
                            )
                            .align(TextAlign::Center),
                        ))),
                ),
            ),
        )
        .push(
            Positioned::fill().child(Opacity::new(intro.blocks[2]).child(
                Stack::new()
                    .fit(StackFit::Expand)
                    .push(place(0.0, top_of(525.0, 9.0)).width(420.0).child(Text::new("YOUR U").style(
                        TextStyle::new(9.0)
                            .weight(FontWeight::Bold)
                            .letter_spacing(2.3)
                            .color(Color::rgb(189, 176, 245)),
                    )
                    .align(TextAlign::Center)))
                    .push(place(0.0, top_of(544.0, 9.5)).width(420.0).child(
                        Text::new("A little more you, every day")
                            .style(
                                TextStyle::new(9.5)
                                    .letter_spacing(0.4)
                                    .color(Color::rgb(111, 126, 152)),
                            )
                            .align(TextAlign::Center),
                    )),
            )),
        )
    )
    .into()
}

/// The splash's landing rings: two flat ellipses the icon settles onto, the
/// reference's own closing chord.
fn landing_rings() -> Painting {
    Painting::sized(
        Size::new(160.0, 20.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            book.stroke(neurons::flat_ellipse(c, 70.0, 8.0), AXON.with_alpha(64), 1.1);
            book.stroke(neurons::flat_ellipse(c, 48.0, 5.0), SYNAPSE.with_alpha(56), 1.0);
        }),
    )
}

/// The splash's three dots, breathing in sequence.
#[allow(clippy::cast_precision_loss)]
fn loading_dots(time: f32) -> Painting {
    Painting::sized(
        Size::new(46.0, 10.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let colors = [AXON, palette::mix(AXON, SYNAPSE, 0.5), SYNAPSE];
            for (i, color) in colors.iter().enumerate() {
                let breathe = (time * TAU / 1.7 + i as f32 * 0.8).sin();
                let (radius, alpha) = (2.6 + breathe * 0.7, 130.0 + breathe * 90.0);
                book.circle(
                    Offset::new(c.dx + (i as f32 - 1.0) * 16.0, c.dy),
                    (radius).max(1.0),
                    color.with_alpha(alpha.clamp(40.0, 235.0) as u8),
                );
            }
        }),
    )
}

/// The one primary action: bring U a decision — the reference's disc at
/// (210, 605), r = 39, under its breathing halo.
fn action(shared: &Shared, ctx: &BuildContext, t: f32) -> WidgetNode {
    let shared_for_ask = shared.clone();
    let button = Pressable::themed(ctx, move |press| {
        let sink = 1.0 - press * 0.05;
        SizedBox::square(98.0)
            .child(
                Stack::new()
                    .fit(StackFit::Expand)
                    .push(Positioned::fill().child(action_halo(t)))
                    .push(
                        Positioned::fill().child(
                            Align::new(Alignment::CENTER).child(
                                Transformed::new(Transform::scale_around(
                                    Offset::new(49.0, 49.0),
                                    sink,
                                    sink,
                                ))
                                .child(
                                    Container::new()
                                        .decoration(
                                            BoxDecoration::new()
                                                .gradient(palette::action_gradient())
                                                .radius(39.0)
                                                .shadow(Shadow::new(
                                                    Color::rgba(0, 0, 0, 122),
                                                    Offset::new(0.0, 6.0),
                                                    9.0,
                                                )),
                                        )
                                        .size(78.0, 78.0)
                                        .alignment(Alignment::CENTER)
                                        .child(
                                            Stack::new()
                                                .fit(StackFit::Expand)
                                                .push(Positioned::fill().child(action_ring()))
                                                .push(
                                                    Align::new(Alignment::CENTER).child(
                                                        plus_glyph(12.0, 2.8, Color::rgb(255, 255, 255)),
                                                    ),
                                                ),
                                        ),
                                ),
                            ),
                        ),
                    ),
            )
            .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_ask);
        state.nav.push(ask_route(shared_for_ask.clone()));
    });

    Stack::new()
        .fit(StackFit::Expand)
        .push(place(161.0, 556.0).child(button))
        .push(place(0.0, top_of(661.0, 15.0)).width(420.0).child(
            Text::new("New decision").style(
                TextStyle::new(15.0)
                    .weight(FontWeight::Medium)
                    .color(Color::rgb(238, 240, 251)),
            )
            .align(TextAlign::Center),
        ))
        .into()
}

/// The halo behind the action: the reference's wide pulse disc, breathing,
/// and one slow ring racing outward.
fn action_halo(t: f32) -> Painting {
    Painting::sized(
        Size::square(98.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let phase = (t / 3.2).fract();
            let breathe = 0.5 + 0.5 * (t * TAU / 3.6).sin();
            book.blended_layer(1.0, 0.0, BlendMode::Plus, None, |light| {
                light.circle(
                    c,
                    49.0,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            Color::rgb(146, 121, 235).with_alpha((26.0 + 10.0 * breathe) as u8),
                        ),
                        (0.72, Color::rgb(146, 121, 235).with_alpha(14)),
                        (1.0, Color::TRANSPARENT),
                    ]),
                );
                light.ring(
                    c,
                    39.0 + phase * 10.0,
                    1.3,
                    AXON.with_alpha(((1.0 - phase) * 60.0) as u8),
                );
            });
        }),
    )
}

/// The button's inner ring — the reference's quiet second border.
fn action_ring() -> Painting {
    Painting::sized(
        Size::square(78.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            book.ring(c, 31.0, 1.2, Color::rgba(238, 232, 255, 107));
        }),
    )
}

/// A plus, as the reference draws both of its own: two round-capped strokes
/// `arm` long, `width` thick, crossing at the centre of a box that holds
/// them. The action's is twelve and 2.8; the nudge's is five and 1.25.
fn plus_glyph(arm: f32, width: f32, color: Color) -> Painting {
    let span = arm * 2.0 + width + 2.0;
    Painting::sized(
        Size::square(span),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let mut path = Path::new();
            path.move_to(Offset::new(c.dx, c.dy - arm));
            path.line_to(Offset::new(c.dx, c.dy + arm));
            path.move_to(Offset::new(c.dx - arm, c.dy));
            path.line_to(Offset::new(c.dx + arm, c.dy));
            book.stroke_styled(path, color, width, StrokeStyle::rounded());
        }),
    )
}

/// The four domains a decision can arrive from — the reference's own row:
/// circles at x = 78, 166, 254, 342, forty points of air between their
/// centres, labels nineteen points below each rim.
fn categories(state: &AppState, shared: &Shared, ctx: &BuildContext) -> WidgetNode {
    Positioned::fill().child(
        Stack::new()
            .fit(StackFit::Expand)
            .children(children![
                category(state, shared, ctx, 78.0, "CAREER", CAREER, AXON, Glyph::Career),
                category(
                    state,
                    shared,
                    ctx,
                    166.0,
                    "PERSONAL",
                    PERSONAL,
                    Color::rgb(239, 159, 192),
                    Glyph::Personal,
                ),
                category(
                    state,
                    shared,
                    ctx,
                    254.0,
                    "FINANCE",
                    FINANCE,
                    Color::rgb(240, 199, 127),
                    Glyph::Finance,
                ),
                category(state, shared, ctx, 342.0, "LIFE", LIFE, crate::palette::LIFE_TINT, Glyph::Life),
            ]),
    )
    .into()
}

/// One domain: its circle at `(x, 720)`, its glyph, its name at y = 763 —
/// both placed directly, as the reference places them.
fn category(
    state: &AppState,
    shared: &Shared,
    ctx: &BuildContext,
    x: f32,
    label: &'static str,
    tags: &'static [&'static str],
    tint: Color,
    glyph: Glyph,
) -> WidgetNode {
    let state_for_pick = state.clone();
    let shared_for_ask = shared.clone();
    let pressable = Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(21, 31, 49, (235.0 + press * 20.0) as u8))
                    .radius(24.0)
                    .border(Border::new(
                        tint.with_alpha((51.0 + press * 120.0) as u8),
                        1.0,
                    )),
            )
            .size(48.0, 48.0)
            .alignment(Alignment::CENTER)
            .child(glyph.paint(tint))
            .into()
    })
    .on_tap(move || {
        // The next question from this domain, then the sheet.
        state_for_pick.ask_in(tags);
        state_for_pick.nav.push(ask_route(shared_for_ask.clone()));
    });

    Stack::new()
        .fit(StackFit::Expand)
        .push(place(x - 24.0, 696.0).child(pressable))
        .push(place(x - 40.0, top_of(763.0, 9.0)).child(
            SizedBox::width(80.0).child(
                Text::new(label)
                    .style(
                        TextStyle::new(9.0)
                            .letter_spacing(0.4)
                            .color(Color::rgb(147, 160, 182)),
                    )
                    .align(TextAlign::Center),
            ),
        ))
        .into()
}

/// A domain's glyph, drawn rather than stocked — the reference's own paths,
/// at their own measures.
#[derive(Debug, Clone, Copy)]
enum Glyph {
    Career,
    Personal,
    Finance,
    Life,
}

impl Glyph {
    fn paint(self, color: Color) -> Painting {
        Painting::sized(
            Size::square(32.0),
            PaintWith::new(move |book, s| {
                use vieww_foundation::StrokeStyle;
                let c = Offset::new(s.width / 2.0, s.height / 2.0);
                let style = StrokeStyle::rounded();
                // One straight segment, as its own stroke — a helper rather
                // than a closure so the case below can keep borrowing the
                // book.
                fn seg(
                    book: &mut Sketchbook,
                    from: Offset,
                    to: Offset,
                    color: Color,
                    style: &StrokeStyle,
                ) {
                    let mut path = Path::new();
                    path.move_to(from).line_to(to);
                    book.stroke_styled(path, color, 1.65, style.clone());
                }
                match self {
                    Glyph::Career => {
                        // The reference's briefcase: a 20 × 14 case, a handle
                        // five above it, a clasp line through the lid.
                        book.stroke_rrect(
                            Rect::new(c.dx - 10.0, c.dy - 5.0, c.dx + 10.0, c.dy + 9.0),
                            2.0,
                            color,
                            1.65,
                        );
                        let mut handle = Path::new();
                        handle.move_to(Offset::new(c.dx - 6.0, c.dy - 5.0));
                        handle.line_to(Offset::new(c.dx - 6.0, c.dy - 10.0));
                        handle.line_to(Offset::new(c.dx + 6.0, c.dy - 10.0));
                        handle.line_to(Offset::new(c.dx + 6.0, c.dy - 5.0));
                        book.stroke_styled(handle, color, 1.65, style.clone());
                        seg(
                            book,
                            Offset::new(c.dx - 4.0, c.dy + 1.0),
                            Offset::new(c.dx + 4.0, c.dy + 1.0),
                            color,
                            &style,
                        );
                    }
                    Glyph::Personal => {
                        // The reference's heart, in its four bows.
                        let mut path = Path::new();
                        path.move_to(Offset::new(c.dx, c.dy + 10.0));
                        path.cubic_to(
                            Offset::new(c.dx, c.dy + 10.0),
                            Offset::new(c.dx - 12.0, c.dy + 3.0),
                            Offset::new(c.dx - 12.0, c.dy - 5.0),
                        );
                        path.cubic_to(
                            Offset::new(c.dx - 12.0, c.dy - 13.0),
                            Offset::new(c.dx - 3.0, c.dy - 14.0),
                            Offset::new(c.dx, c.dy - 7.0),
                        );
                        path.cubic_to(
                            Offset::new(c.dx + 3.0, c.dy - 14.0),
                            Offset::new(c.dx + 12.0, c.dy - 13.0),
                            Offset::new(c.dx + 12.0, c.dy - 5.0),
                        );
                        path.cubic_to(
                            Offset::new(c.dx + 12.0, c.dy + 3.0),
                            Offset::new(c.dx, c.dy + 10.0),
                            Offset::new(c.dx, c.dy + 10.0),
                        );
                        book.stroke_styled(path, color, 1.65, style.clone());
                    }
                    Glyph::Finance => {
                        // The reference's card: a 22 × 16 face, its stripe,
                        // its five-point line.
                        book.stroke_rrect(
                            Rect::new(c.dx - 11.0, c.dy - 8.0, c.dx + 11.0, c.dy + 8.0),
                            3.0,
                            color,
                            1.6,
                        );
                        seg(
                            book,
                            Offset::new(c.dx - 11.0, c.dy - 2.0),
                            Offset::new(c.dx + 11.0, c.dy - 2.0),
                            color,
                            &style,
                        );
                        seg(
                            book,
                            Offset::new(c.dx - 5.0, c.dy + 4.0),
                            Offset::new(c.dx, c.dy + 4.0),
                            color,
                            &style,
                        );
                    }
                    Glyph::Life => {
                        // The reference's compass: a ten-point ring and four
                        // long rays, one of them the diagonal.
                        book.ring(c, 10.0, 1.65, color);
                        seg(book, Offset::new(c.dx, c.dy - 15.0), Offset::new(c.dx, c.dy - 10.0), color, &style);
                        seg(book, Offset::new(c.dx, c.dy + 10.0), Offset::new(c.dx, c.dy + 15.0), color, &style);
                        seg(book, Offset::new(c.dx - 15.0, c.dy), Offset::new(c.dx - 10.0, c.dy), color, &style);
                        seg(book, Offset::new(c.dx + 10.0, c.dy), Offset::new(c.dx + 15.0, c.dy), color, &style);
                        seg(book, Offset::new(c.dx + 8.0, c.dy - 8.0), Offset::new(c.dx + 4.0, c.dy - 4.0), color, &style);
                    }
                }
            }),
        )
    }
}

/// The strip's label — the reference's y = 798.
fn recent_label() -> WidgetNode {
    Text::new("PICK UP WHERE YOU LEFT OFF").style(
        TextStyle::new(9.0)
            .weight(FontWeight::Bold)
            .letter_spacing(1.8)
            .color(Color::rgb(119, 136, 165)),
    )
    .into()
}

/// The last thing U noted, and the way back into the asking — the
/// reference's 372 × 48 card at y = 810.
fn recent(shared: &Shared, ctx: &BuildContext, noted: &'static str) -> WidgetNode {
    let shared_for_ask = shared.clone();
    let card = Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(17, 27, 45, (228.0 + press * 27.0) as u8))
                    .radius(15.0)
                    .border(Border::new(
                        EDGE.with_alpha((19.0 + press * 110.0) as u8),
                        1.0,
                    )),
            )
            .height(48.0)
            .width(372.0)
            .padding(EdgeInsets::symmetric(12.0, 0.0))
            .child(
                Flex::row()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .children(children![
                        Container::new()
                            .decoration(
                                BoxDecoration::new()
                                    .color(Color::rgb(48, 38, 74))
                                    .radius(13.0),
                            )
                            .size(26.0, 26.0)
                            .alignment(Alignment::CENTER)
                            .child(strip_briefcase()),
                        SizedBox::width(11.0),
                        Text::new(noted).style(
                            TextStyle::new(12.0)
                                .weight(FontWeight::Medium)
                                .color(Color::rgb(220, 227, 241)),
                        ),
                        Flexible::expanded(1).child(SizedBox::shrink()),
                        Icon::new(icons::chevron_right()).size(16.0).color(MUTED),
                    ]),
            )
            .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_ask);
        state.nav.push(ask_route(shared_for_ask.clone()));
    });

    card
    .into()
}

/// The strip's briefcase — the reference's smaller one: a 14 × 9 case, a
/// stubby handle, a four-point clasp, at (49, 834).
fn strip_briefcase() -> Painting {
    Painting::sized(
        Size::square(26.0),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let color = Color::rgb(185, 167, 250);
            let style = StrokeStyle::rounded();
            // The case sits two below the circle's centre, as drawn.
            book.stroke_rrect(
                Rect::new(c.dx - 7.0, c.dy - 2.0, c.dx + 7.0, c.dy + 7.0),
                2.0,
                color,
                1.3,
            );
            let mut handle = Path::new();
            handle.move_to(Offset::new(c.dx - 4.0, c.dy - 2.0));
            handle.line_to(Offset::new(c.dx - 4.0, c.dy - 5.0));
            handle.line_to(Offset::new(c.dx + 4.0, c.dy - 5.0));
            handle.line_to(Offset::new(c.dx + 4.0, c.dy - 2.0));
            book.stroke_styled(handle, color, 1.3, style.clone());
            let mut clasp = Path::new();
            clasp.move_to(Offset::new(c.dx - 2.0, c.dy + 2.0));
            clasp.line_to(Offset::new(c.dx + 2.0, c.dy + 2.0));
            book.stroke_styled(clasp, color, 1.3, style.clone());
        }),
    )
}

/// The nudge: U's offer of a question, dismissible for this session — the
/// reference's 372 × 34 bar at y = 878. What it says is where the two of you
/// stand — the sketch and its progress, or the portrait it has become.
fn nudge(state: &AppState, shared: &Shared, ctx: &BuildContext, answers: u32) -> WidgetNode {
    let text = if answers == 0 {
        "A first sketch — twelve little questions".to_owned()
    } else if answers < ask::SKETCH {
        format!("The sketch is {answers} of {}", ask::SKETCH)
    } else if !state.seen_sketch.get() {
        "Twelve answers — the portrait begins".to_owned()
    } else {
        "A little question?".to_owned()
    };

    let state_for_later = state.clone();
    let later = Pressable::themed(ctx, move |press| {
        Text::new("LATER").style(
            TextStyle::new(10.0)
                .weight(FontWeight::Medium)
                .letter_spacing(0.5)
                .color(palette::mix(Color::rgb(147, 137, 176), INK, press)),
        )
        .into()
    })
    .on_tap(move || state_for_later.dismiss_nudge());

    let state_for_close = state.clone();
    let close = Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(41, 35, 61, (220.0 + press * 35.0) as u8))
                    .radius(9.0),
            )
            .size(18.0, 18.0)
            .alignment(Alignment::CENTER)
            .child(Icon::new(icons::close()).size(8.0).color(Color::rgb(184, 175, 202)))
            .into()
    })
    .on_tap(move || state_for_close.dismiss_nudge());

    let shared_for_ask = shared.clone();
    Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(23, 22, 42, (226.0 + press * 29.0) as u8))
                    .radius(12.0)
                    .border(Border::new(AXON.with_alpha(56), 1.0)),
            )
            .height(34.0)
            .width(372.0)
            .padding(EdgeInsets::symmetric(8.0, 0.0))
            .child(
                Flex::row()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .children(children![
                        Container::new()
                            .decoration(
                                BoxDecoration::new()
                                    .color(Color::rgb(48, 36, 73))
                                    .radius(9.0),
                            )
                            .size(18.0, 18.0)
                            .alignment(Alignment::CENTER)
                            .child(plus_glyph(5.0, 1.25, Color::rgb(187, 168, 255))),
                        SizedBox::width(10.0),
                        Text::new(text.clone())
                            .size(10.5)
                            .color(Color::rgb(200, 194, 220)),
                        Flexible::expanded(1).child(SizedBox::shrink()),
                        later.clone(),
                        SizedBox::width(10.0),
                        close.clone(),
                    ]),
            )
            .into()
    })
    .on_tap(move || {
        let state = take(&shared_for_ask);
        state.nav.push(ask_route(shared_for_ask.clone()));
    })
    .into()
}
