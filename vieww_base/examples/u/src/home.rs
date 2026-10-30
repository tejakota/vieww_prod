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
use crate::palette::{self, AXON, EDGE, INK, MUTED, SYNAPSE};
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

        // The tail: whatever was noted last, and the nudge offering the next
        // question. Both belong to the bottom of the screen.
        let mut tail = Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(6.0);
        if let Some(noted) = noted {
            tail = tail.push(recent(&self.shared, ctx, noted, t));
        }
        if nudge_up {
            tail = tail.push(nudge(&state, &self.shared, ctx, answers));
        }

        let mut screen = Stack::new()
            .fit(StackFit::Expand)
            .push(
                Positioned::fill().child(
                    Container::new()
                        .gradient(palette::ground_gradient())
                        .child(neurons::atmosphere(t, amb, false)),
                ),
            )
            .push(
                Positioned::fill().child(
                    SafeArea::new().child(
                        Padding::new(EdgeInsets::symmetric(24.0, 4.0)).child(
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .children(children![
                                    arrive(intro.blocks[0], -12.0, top_bar(&self.shared, ctx, t)),
                                    SizedBox::height(6.0),
                                    arrive(intro.blocks[1], -8.0, greeting(&state)),
                                    SizedBox::height(2.0),
                                    Align::new(Alignment::CENTER).child(neurons::companion(
                                        252.0,
                                        t,
                                        burst,
                                        if calm { 1.0 } else { 0.0 },
                                        intro.reveal,
                                    )),
                                    below_slot(&intro, t),
                                    arrive(intro.blocks[3], 14.0, action(&self.shared, ctx, t),),
                                    SizedBox::height(6.0),
                                    arrive(
                                        intro.blocks[4],
                                        16.0,
                                        categories(&state, &self.shared, ctx),
                                    ),
                                    Flexible::expanded(1).child(SizedBox::shrink()),
                                    arrive(intro.blocks[4], 10.0, tail.into()),
                                ]),
                        ),
                    ),
                ),
            );

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
                .child(screen),
        )
    }
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
/// the room's one exit besides the swipe.
fn top_bar(shared: &Shared, ctx: &BuildContext, t: f32) -> WidgetNode {
    let shared_for_chip = shared.clone();
    let chip = Pressable::themed(ctx, move |press| {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .color(Color::rgba(20, 35, 56, (200.0 + press * 55.0) as u8))
                    .radius(15.0)
                    .border(Border::new(
                        SYNAPSE.with_alpha((71.0 + press * 120.0) as u8),
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
                                .weight(FontWeight::Bold)
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

    Flex::row()
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(children![
            // The mark's signature: the same six tendrils, the same eye.
            neurons::sigil(34.0, t, 0.0),
            SizedBox::width(11.0),
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(children![
                    Text::new("U").style(
                        TextStyle::new(24.0)
                            .weight(FontWeight::Bold)
                            .letter_spacing(-1.0)
                            .color(INK),
                    ),
                    Text::new("PERSONA").style(
                        TextStyle::new(7.5)
                            .letter_spacing(1.6)
                            .color(Color::rgb(115, 130, 156)),
                    ),
                ]),
            Flexible::expanded(1).child(SizedBox::shrink()),
            chip,
        ])
        .into()
}

/// The chip's pulse: a small synapse dot, slowly twinkling.
fn alive_dot(t: f32) -> Painting {
    Painting::sized(
        Size::square(9.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let twinkle = 0.5 + 0.5 * (t * std::f32::consts::TAU / 3.0).sin();
            book.circle(
                c,
                2.1 + twinkle * 0.6,
                SYNAPSE.with_alpha((150.0 + 90.0 * twinkle) as u8),
            );
        }),
    )
}

/// Who this room is for, and that it is ready.
fn greeting(state: &AppState) -> WidgetNode {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(3.0)
        .children(children![
            Text::new(daypart()).style(
                TextStyle::new(11.0)
                    .weight(FontWeight::Medium)
                    .letter_spacing(1.9)
                    .color(Color::rgb(131, 146, 170)),
            ),
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(9.0)
                .children(children![
                    Text::new(state.display_name()).style(
                        TextStyle::new(30.0)
                            .weight(FontWeight::Bold)
                            .letter_spacing(-1.3)
                            .color(Color::rgb(242, 243, 250)),
                    ),
                    name_spark(state.time.get()),
                ]),
            Text::new("Ready when you are.").size(12.0).color(MUTED),
        ])
        .into()
}

/// The small spark that rides the name — a charm, not a status.
fn name_spark(t: f32) -> Painting {
    Painting::sized(
        Size::square(8.0),
        PaintWith::new(move |book, s| {
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let twinkle = 0.5 + 0.5 * (t * std::f32::consts::TAU / 3.0 + 0.9).sin();
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
/// room arrives, then the caption does — a crossfade in a fixed box, so
/// nothing above or below it ever shifts. The splash's stack carries the
/// reference's own descent: the landing rings, the wordmark, its subtitle,
/// the tagline, the loader and the privacy line.
fn below_slot(intro: &Intro, t: f32) -> WidgetNode {
    let splash = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(children![
            Opacity::new(intro.word).child(Align::new(Alignment::CENTER).child(landing_rings())),
            SizedBox::height(2.0),
            Opacity::new(intro.word).child(
                Text::new("U").style(
                    TextStyle::new(40.0)
                        .weight(FontWeight::Bold)
                        .letter_spacing(-2.5)
                        .color(Color::rgb(241, 239, 255)),
                )
                .align(TextAlign::Center),
            ),
            SizedBox::height(1.0),
            Opacity::new(intro.word).child(
                Text::new("PERSONA COMPANION").style(
                    TextStyle::new(8.5)
                        .weight(FontWeight::Medium)
                        .letter_spacing(2.6)
                        .color(Color::rgb(155, 139, 220)),
                )
                .align(TextAlign::Center),
            ),
            SizedBox::height(4.0),
            Opacity::new(intro.tagline).child(
                Text::new("A little more you, every day.")
                    .size(11.0)
                    .color(MUTED)
                    .align(TextAlign::Center),
            ),
            SizedBox::height(4.0),
            Opacity::new(intro.footer).child(Align::new(Alignment::CENTER).child(loading_dots(t)),),
            SizedBox::height(3.0),
            Opacity::new(intro.footer).child(
                Text::new("PRIVATE BY DESIGN").style(
                    TextStyle::new(7.5)
                        .weight(FontWeight::Medium)
                        .letter_spacing(2.0)
                        .color(Color::rgb(83, 98, 123)),
                )
                .align(TextAlign::Center),
            ),
        ]);

    let caption = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(5.0)
        .children(children![
            Text::new("YOUR U").style(
                TextStyle::new(9.0)
                    .weight(FontWeight::Bold)
                    .letter_spacing(2.3)
                    .color(Color::rgb(189, 176, 245)),
            )
            .align(TextAlign::Center),
            Text::new("A little more you, every day")
                .size(9.5)
                .color(Color::rgb(111, 126, 152))
                .align(TextAlign::Center),
        ]);

    SizedBox::height(130.0)
        .child(
            Stack::new()
                .fit(StackFit::Expand)
                .push(
                    Positioned::fill().child(
                        Opacity::new(1.0 - intro.exit)
                            .child(Align::new(Alignment::CENTER).child(splash)),
                    ),
                )
                .push(
                    Positioned::fill().child(
                        Opacity::new(intro.blocks[2])
                            .child(Align::new(Alignment::CENTER).child(caption)),
                    ),
                ),
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

/// The one primary action: bring U a decision.
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
                                                    Color::rgba(0, 0, 0, 130),
                                                    Offset::new(0.0, 12.0),
                                                    24.0,
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
                                                        Icon::new(icons::add())
                                                            .size(28.0)
                                                            .color(Color::rgb(255, 255, 255)),
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

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(4.0)
        .children(children![
            button,
            Text::new("New decision").style(
                TextStyle::new(15.0)
                    .weight(FontWeight::Medium)
                    .color(Color::rgb(238, 240, 251)),
            )
            .align(TextAlign::Center),
        ])
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
            let breathe = 0.5 + 0.5 * (t * std::f32::consts::TAU / 3.6).sin();
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

/// The four domains a decision can arrive from.
fn categories(state: &AppState, shared: &Shared, ctx: &BuildContext) -> WidgetNode {
    Flex::row()
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(children![
            category(state, shared, ctx, "CAREER", CAREER, AXON, Glyph::Career,),
            category(
                state,
                shared,
                ctx,
                "PERSONAL",
                PERSONAL,
                Color::rgb(239, 159, 192),
                Glyph::Personal,
            ),
            category(
                state,
                shared,
                ctx,
                "FINANCE",
                FINANCE,
                Color::rgb(240, 199, 127),
                Glyph::Finance,
            ),
            category(state, shared, ctx, "LIFE", LIFE, SYNAPSE, Glyph::Life,),
        ])
        .into()
}

/// One domain: its circle, its glyph, its name.
fn category(
    state: &AppState,
    shared: &Shared,
    ctx: &BuildContext,
    label: &'static str,
    tags: &'static [&'static str],
    tint: Color,
    glyph: Glyph,
) -> WidgetNode {
    let state_for_pick = state.clone();
    let shared_for_ask = shared.clone();
    Pressable::themed(ctx, move |press| {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .spacing(5.0)
            .children(children![
                Container::new()
                    .decoration(
                        BoxDecoration::new()
                            .color(Color::rgba(21, 31, 49, (235.0 + press * 20.0) as u8,))
                            .radius(24.0)
                            .border(Border::new(
                                tint.with_alpha((51.0 + press * 120.0) as u8),
                                1.0,
                            )),
                    )
                    .size(48.0, 48.0)
                    .alignment(Alignment::CENTER)
                    .child(glyph.paint(tint)),
                Text::new(label).size(9.0).color(MUTED),
            ])
            .into()
    })
    .on_tap(move || {
        // The next question from this domain, then the sheet.
        state_for_pick.ask_in(tags);
        state_for_pick.nav.push(ask_route(shared_for_ask.clone()));
    })
    .into()
}

/// A domain's glyph, drawn rather than stocked — the icon set is ten shapes
/// and none of them is a briefcase.
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
            Size::square(26.0),
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
                    book.stroke_styled(path, color, 1.6, style.clone());
                }
                match self {
                    Glyph::Career => {
                        // A briefcase: the case, the handle, the clasp line.
                        book.stroke_rrect(
                            Rect::new(c.dx - 9.0, c.dy - 5.0, c.dx + 9.0, c.dy + 7.0),
                            2.5,
                            color,
                            1.6,
                        );
                        seg(
                            book,
                            Offset::new(c.dx - 3.0, c.dy - 5.0),
                            Offset::new(c.dx - 3.0, c.dy - 8.0),
                            color,
                            &style,
                        );
                        seg(
                            book,
                            Offset::new(c.dx + 3.0, c.dy - 5.0),
                            Offset::new(c.dx + 3.0, c.dy - 8.0),
                            color,
                            &style,
                        );
                        seg(
                            book,
                            Offset::new(c.dx - 3.0, c.dy - 8.0),
                            Offset::new(c.dx + 3.0, c.dy - 8.0),
                            color,
                            &style,
                        );
                        seg(
                            book,
                            Offset::new(c.dx - 9.0, c.dy + 0.5),
                            Offset::new(c.dx + 9.0, c.dy + 0.5),
                            color,
                            &style,
                        );
                    }
                    Glyph::Personal => {
                        // A heart, in two bows.
                        let mut path = Path::new();
                        path.move_to(Offset::new(c.dx, c.dy + 7.0));
                        path.cubic_to(
                            Offset::new(c.dx - 10.0, c.dy + 1.0),
                            Offset::new(c.dx - 8.0, c.dy - 8.0),
                            Offset::new(c.dx, c.dy - 3.0),
                        );
                        path.cubic_to(
                            Offset::new(c.dx + 8.0, c.dy - 8.0),
                            Offset::new(c.dx + 10.0, c.dy + 1.0),
                            Offset::new(c.dx, c.dy + 7.0),
                        );
                        book.stroke_styled(path, color, 1.6, style);
                    }
                    Glyph::Finance => {
                        // A card: the face, the stripe, the chip.
                        book.stroke_rrect(
                            Rect::new(c.dx - 10.0, c.dy - 6.5, c.dx + 10.0, c.dy + 6.5),
                            2.5,
                            color,
                            1.6,
                        );
                        seg(
                            book,
                            Offset::new(c.dx - 10.0, c.dy - 1.5),
                            Offset::new(c.dx + 10.0, c.dy - 1.5),
                            color,
                            &style,
                        );
                        book.stroke_rrect(
                            Rect::new(c.dx + 2.0, c.dy + 1.5, c.dx + 7.0, c.dy + 4.0),
                            1.0,
                            color,
                            1.3,
                        );
                    }
                    Glyph::Life => {
                        // A compass rose: the ring and four points.
                        book.ring(c, 6.0, 1.6, color);
                        seg(
                            book,
                            Offset::new(c.dx, c.dy - 9.5),
                            Offset::new(c.dx, c.dy - 6.5),
                            color,
                            &style,
                        );
                        seg(
                            book,
                            Offset::new(c.dx, c.dy + 6.5),
                            Offset::new(c.dx, c.dy + 9.5),
                            color,
                            &style,
                        );
                        seg(
                            book,
                            Offset::new(c.dx - 9.5, c.dy),
                            Offset::new(c.dx - 6.5, c.dy),
                            color,
                            &style,
                        );
                        seg(
                            book,
                            Offset::new(c.dx + 6.5, c.dy),
                            Offset::new(c.dx + 9.5, c.dy),
                            color,
                            &style,
                        );
                    }
                }
            }),
        )
    }
}

/// The last thing U noted, and the way back into the asking.
fn recent(shared: &Shared, ctx: &BuildContext, noted: &'static str, _t: f32) -> WidgetNode {
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
                            .child(Glyph::Career.paint(Color::rgb(185, 167, 250))),
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

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(6.0)
        .children(children![
            Text::new("PICK UP WHERE YOU LEFT OFF").style(
                TextStyle::new(9.0)
                    .weight(FontWeight::Bold)
                    .letter_spacing(1.8)
                    .color(Color::rgb(119, 136, 165)),
            ),
            card,
        ])
        .into()
}

/// The nudge: U's offer of a question, dismissible for this session. What it
/// says is where the two of you stand — the sketch and its progress, or the
/// portrait it has become.
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
                            .child(Icon::new(icons::add()).size(10.0).color(Color::rgb(187, 168, 255))),
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
