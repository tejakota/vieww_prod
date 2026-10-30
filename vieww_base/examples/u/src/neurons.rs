//! The companion — the mark that is U — and the quiet atmosphere every
//! screen floats in.
//!
//! # The companion
//!
//! [`companion`] is the organism itself: one eye, six tendrils, two
//! elliptical rings. The tendrils are the reference mark's own — **four
//! violet signal strands, each a stem that forks into branches, and two
//! lateral mint strands**, drawn from the reference SVGs path for path and
//! deliberately *not* symmetric: a tendril bows where it bows, forks where
//! the mark forks. The same geometry at small size makes the [`sigil`] in
//! the top bar, so the mark and its signature cannot disagree. The rings
//! rotate *in place*, about the mark's exact centre and about nothing else:
//! an ellipse tumbling about its own centre while its rider dot revolves is
//! the whole of their motion.
//!
//! `Reveal` is the splash's choreography — the eye scales in, each tendril
//! grows out of the eye (stem first, then its branches sprout), the
//! atmosphere (rings, aura, float) fades in last — and `Reveal::complete()`
//! is the steady state home keeps for the rest of the app's life.
//!
//! # One additive layer for all the light
//!
//! Every glow — the aura, the pulse halos, the luminous strokes — goes into
//! a single `BlendMode::Plus` layer. That is the pricing rule from
//! `Sketchbook::layer`'s own documentation: a blurred or blended layer costs
//! one offscreen buffer *per layer*, and the expensive version of this file
//! is the one that wraps each glow separately. Group early, blend once.
//!
//! # The atmosphere
//!
//! [`atmosphere`] is the background the reference screens share: a wide
//! ambient bloom where the companion sits, one mint wash low in a corner,
//! a few hand-set filaments bowing in from the frame's edges, and a handful
//! of twinkling sparks. Everything is a *function of the clock* — nothing
//! accumulates, nothing steps — so any frame is reproducible from `time`
//! alone, and the screenshot tool can photograph a frame without having
//! lived through the ones before it.

use std::f32::consts::{PI, TAU};

use vieww_foundation::{
    BlendMode, Color, Dash, Gradient, Offset, Path, Size, Sketchbook, StrokeStyle, Transform,
};
use vieww_widget::prelude::{PaintWith, Painting};

use crate::palette::{AXON, SYNAPSE};

// ---------------------------------------------------------------------------
// The companion: the mark that is U.
// ---------------------------------------------------------------------------

/// The eye's own colours — the one place the palette serves specific tints
/// rather than roles, because an iris is not a button.
const SCLERA: Color = Color::rgb(18, 26, 43);
const RIM: Color = Color::rgb(185, 165, 255);
const IRIS_RIM: Color = Color::rgb(85, 221, 204);
const IRIS_DEEP: Color = Color::rgb(39, 108, 136);
const PUPIL: Color = Color::rgb(7, 17, 31);
/// The tendril tips: lighter than their strands, as the reference draws them.
const VIOLET_TIP: Color = Color::rgb(196, 181, 253);
const MINT_TIP: Color = Color::rgb(141, 248, 231);

/// The mark's own space: the reference SVG's coordinates are kept verbatim
/// and divided by this span, so the numbers below can be read against the
/// design without translating them first.
const SPAN: f32 = 150.0;

/// One point of a tendril, in the mark's own units.
type Pt = (f32, f32);

/// One cubic of a tendril, in the mark's own units: two controls, an end,
/// and — when the cubic ends in open air — the tip dot that crowns it.
struct Seg {
    c1: Pt,
    c2: Pt,
    to: Pt,
    tip: Option<(Pt, f32)>,
}

const fn seg(c1: Pt, c2: Pt, to: Pt, tip: Option<(Pt, f32)>) -> Seg {
    Seg { c1, c2, to, tip }
}

/// One tendril: where it leaves the eye, whether it is one of the four
/// **violet signal strands** (the other two are mint), and its segments in
/// draw order — stem first, then its branches, so the splash can grow it
/// the way it was drawn: out along the stem, then forking.
struct Strand {
    from: Pt,
    violet: bool,
    segs: &'static [Seg],
}

// The four violet signal strands. Numbers are the reference SVG's own path
// data, relative to the eye's centre; the `S` commands have been unfolded
// into explicit controls. Each stem forks: the first two segments are the
// stem, the rest spring from the fork.
static V_UL: [Seg; 4] = [
    seg((-31.0, -36.0), (-39.0, -52.0), (-61.0, -65.0), None),
    seg(
        (-83.0, -78.0),
        (-84.0, -79.0),
        (-96.0, -94.0),
        Some(((-96.0, -94.0), 3.2)),
    ),
    seg(
        (-58.0, -83.0),
        (-68.0, -96.0),
        (-82.0, -107.0),
        Some(((-82.0, -107.0), 2.5)),
    ),
    seg(
        (-83.0, -57.0),
        (-96.0, -58.0),
        (-110.0, -66.0),
        Some(((-110.0, -66.0), 2.5)),
    ),
];
static V_UR: [Seg; 4] = [
    seg((33.0, -36.0), (43.0, -47.0), (65.0, -49.0), None),
    seg(
        (87.0, -51.0),
        (91.0, -60.0),
        (101.0, -77.0),
        Some(((101.0, -77.0), 3.0)),
    ),
    seg((77.0, -36.0), (93.0, -33.0), (110.0, -36.0), Some(((110.0, -36.0), 2.5))),
    seg((64.0, -68.0), (74.0, -80.0), (88.0, -90.0), Some(((88.0, -90.0), 2.5))),
];
static V_LL: [Seg; 3] = [
    seg((-34.0, 33.0), (-50.0, 43.0), (-71.0, 43.0), None),
    seg(
        (-92.0, 43.0),
        (-92.0, 57.0),
        (-104.0, 73.0),
        Some(((-104.0, 73.0), 3.0)),
    ),
    seg((-69.0, 61.0), (-78.0, 76.0), (-91.0, 87.0), Some(((-91.0, 87.0), 2.5))),
];
static V_LR: [Seg; 3] = [
    seg((35.0, 32.0), (43.0, 49.0), (63.0, 62.0), None),
    seg((83.0, 75.0), (86.0, 81.0), (91.0, 100.0), Some(((91.0, 100.0), 3.0))),
    seg((48.0, 76.0), (48.0, 90.0), (53.0, 106.0), Some(((53.0, 106.0), 2.5))),
];
// The two lateral mint strands, left and right.
static M_L: [Seg; 3] = [
    seg((-42.0, -7.0), (-58.0, -2.0), (-76.0, 7.0), None),
    seg((-94.0, 16.0), (-99.0, 6.0), (-113.0, 0.0), Some(((-113.0, 0.0), 3.0))),
    seg(
        (-86.0, -7.0),
        (-100.0, -10.0),
        (-113.0, -9.0),
        Some(((-113.0, -9.0), 2.3)),
    ),
];
static M_R: [Seg; 3] = [
    seg((42.0, -2.0), (57.0, 5.0), (75.0, 16.0), None),
    seg((93.0, 27.0), (99.0, 19.0), (112.0, 14.0), Some(((112.0, 14.0), 3.0))),
    seg((86.0, 5.0), (101.0, 1.0), (113.0, 5.0), Some(((113.0, 5.0), 2.3))),
];

/// The six tendrils, in the order the splash grows them: upper-left,
/// upper-right, left, right, lower-left, lower-right — the reference
/// splash's own sequence.
static STRANDS: [Strand; 6] = [
    Strand { from: (-8.0, -13.0), violet: true, segs: &V_UL },
    Strand { from: (12.0, -12.0), violet: true, segs: &V_UR },
    Strand { from: (-18.0, -2.0), violet: false, segs: &M_L },
    Strand { from: (18.0, 3.0), violet: false, segs: &M_R },
    Strand { from: (-12.0, 12.0), violet: true, segs: &V_LL },
    Strand { from: (11.0, 13.0), violet: true, segs: &V_LR },
];

/// How assembled the companion is — the splash's whole choreography, handed
/// to the painter as numbers rather than replayed through it.
#[derive(Clone, Copy, Debug)]
pub struct Reveal {
    /// The eye's scale-in, `0..1`.
    pub eye: f32,
    /// How far each tendril has grown from the eye outward, `0..1`, in
    /// [`STRANDS`] order. Within a tendril the growth is sequential: stem
    /// first, branches after.
    pub strands: [f32; 6],
    /// The atmosphere — rings, aura pulse, sparks, the float — fading in,
    /// `0..1`.
    pub atmosphere: f32,
}

impl Reveal {
    /// The steady state: everything grown, everything lit. What home keeps
    /// for the rest of the app's life, and what the settings portrait and
    /// the splash both resolve to.
    #[must_use]
    pub const fn complete() -> Self {
        Self {
            eye: 1.0,
            strands: [1.0; 6],
            atmosphere: 1.0,
        }
    }
}

/// One coordinate of a cubic Bézier.
fn bez(p0: f32, c1: f32, c2: f32, p3: f32, u: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    (1.0 - 3.0 * u + 3.0 * u2 - u3) * p0
        + (3.0 * u - 6.0 * u2 + 3.0 * u3) * c1
        + (3.0 * u2 - 3.0 * u3) * c2
        + u3 * p3
}

/// The point at `u` on a cubic, as offsets.
fn cubic_at(p0: Offset, c1: Offset, c2: Offset, p3: Offset, u: f32) -> Offset {
    Offset::new(
        bez(p0.dx, c1.dx, c2.dx, p3.dx, u),
        bez(p0.dy, c1.dy, c2.dy, p3.dy, u),
    )
}

/// Rotate a point about the origin by `phi` radians (screen convention:
/// positive turns clockwise on screen).
fn turned(p: Offset, phi: f32) -> Offset {
    let (sin, cos) = phi.sin_cos();
    Offset::new(p.dx * cos - p.dy * sin, p.dx * sin + p.dy * cos)
}

/// An ellipse centred on `c`, tilted by `tiltr` radians, as a path of four
/// Bézier arcs — the canvas approximation, exact enough at these sizes.
fn tilted_ellipse(c: Offset, rx: f32, ry: f32, tiltr: f32) -> Path {
    const K: f32 = 0.552_284_7;
    let at = |x: f32, y: f32| c + turned(Offset::new(x, y), tiltr);
    let mut path = Path::new();
    path.move_to(at(rx, 0.0));
    path.cubic_to(at(rx, -K * ry), at(K * rx, -ry), at(0.0, -ry));
    path.cubic_to(at(-K * rx, -ry), at(-rx, -K * ry), at(-rx, 0.0));
    path.cubic_to(at(-rx, K * ry), at(-K * rx, ry), at(0.0, ry));
    path.cubic_to(at(K * rx, ry), at(rx, K * ry), at(rx, 0.0));
    path
}

/// A flat (untilted) ellipse — the splash's landing rings.
#[must_use]
pub fn flat_ellipse(c: Offset, rx: f32, ry: f32) -> Path {
    tilted_ellipse(c, rx, ry, 0.0)
}

/// The eye's blink: `1` open, dipping toward `0` for a moment once every
/// `period` seconds. A sine dip rather than a snap, because an eyelid that
/// closes in one frame reads as a flicker.
fn blink(time: f32, period: f32) -> f32 {
    let x = (time / period).fract();
    if (0.88..0.95).contains(&x) {
        let u = (x - 0.88) / 0.07;
        1.0 - 0.94 * (u * PI).sin().min(1.0)
    } else {
        1.0
    }
}

/// A slow twinkle for the sparks: `0..1`, each dot on its own phase.
fn twinkle(time: f32, period: f32, phase: f32) -> f32 {
    0.5 + 0.5 * (time * TAU / period + phase).sin()
}

/// The companion: the organism that is U, at whatever `size` box it is given.
///
/// `reveal` is the splash's staging ([`Reveal::complete`] for the steady
/// state); `burst` is the training afterglow — the tendrils flare and the
/// aura brightens when a choice lands; `calm` is the app's own reduced-motion
/// switch, dimming the additive light to a third and stilling the travelling
/// sparks.
///
/// **The rings only rotate.** Each is drawn centred on the mark's exact
/// centre and turned about that centre and nothing else, so an ellipse tumbles
/// in place while its rider dot revolves — no offset drift, no wobble.
#[must_use]
pub fn companion(size: f32, time: f32, burst: f32, calm: f32, reveal: Reveal) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            paint_companion(book, s, time, burst, calm, reveal);
        }),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn paint_companion(
    book: &mut Sketchbook,
    s: Size,
    time: f32,
    burst: f32,
    calm: f32,
    reveal: Reveal,
) {
    let r = s.width * 0.5;
    let c = Offset::new(s.width / 2.0, s.height / 2.0);
    let dim = if calm > 0.5 { 0.34 } else { 1.0 };
    let breathe = (time * TAU / 4.4).sin();
    // The float belongs to the organism, not the rings: the eye and its
    // tendrils ride it, the rings stay put.
    let bob = (time * TAU / 5.8).sin() * 0.047 * r * reveal.atmosphere;
    // A choice landing: the organism leans out a sixteenth, and settles.
    let flare = 1.0 + burst * 0.06;

    // ---- the rings: rotation, and only rotation ---------------------------

    if reveal.atmosphere > 0.01 {
        let a = reveal.atmosphere * dim;
        // Ring A: violet, one turn every 26 s, its dot riding near the
        // ellipse's long end.
        let turn = time * TAU / 26.0;
        book.transformed(Transform::rotate_around(c, turn), |g| {
            g.stroke(
                tilted_ellipse(c, 0.84 * r, 0.327 * r, (-26.0_f32).to_radians()),
                RIM.with_alpha((97.0 * a) as u8),
                0.008 * r,
            );
            let dot = c + turned(
                Offset::new(0.84 * r * 0.32_f32.cos(), 0.327 * r * 0.32_f32.sin()),
                (-26.0_f32).to_radians(),
            );
            let twinkle = 0.8 + 0.2 * (time * TAU / 3.0).sin();
            g.circle(dot, 0.021 * r, RIM.with_alpha((215.0 * a * twinkle) as u8));
        });
        // Ring B: mint, dashed, the other way and slower — 34 s.
        let turn = -time * TAU / 34.0;
        book.transformed(Transform::rotate_around(c, turn), |g| {
            let mut dashed = StrokeStyle::rounded();
            dashed.dash = Some(Dash {
                pattern: vec![0.02 * r, 0.06 * r],
                offset: 0.0,
            });
            g.stroke_styled(
                tilted_ellipse(c, 0.627 * r, 0.867 * r, 33.0_f32.to_radians()),
                SYNAPSE.with_alpha((64.0 * a) as u8),
                0.007 * r,
                dashed,
            );
            let dot = c + turned(
                Offset::new(0.627 * r * 4.08_f32.cos(), 0.867 * r * 4.08_f32.sin()),
                33.0_f32.to_radians(),
            );
            let twinkle = 0.8 + 0.2 * (time * TAU / 3.7).sin();
            g.circle(
                dot,
                0.019 * r,
                SYNAPSE.with_alpha((200.0 * a * twinkle) as u8),
            );
        });
    }

    // ---- all the light, in one additive layer ----------------------------

    book.blended_layer(1.0, 0.0, BlendMode::Plus, None, |light| {
        // The aura: a breathing violet bloom, the eye's own halo arriving
        // with the eye and the wider atmosphere behind it.
        let halo = (0.5 + 0.5 * reveal.eye) * (0.30 + 0.05 * breathe + burst * 0.45);
        light.circle(
            c,
            0.67 * r * (1.0 + 0.02 * breathe),
            Gradient::radial_fill().with_stops(&[
                (0.0, AXON.with_alpha((115.0 * halo * dim) as u8)),
                (0.55, AXON.with_alpha((36.0 * halo * dim) as u8)),
                (1.0, Color::TRANSPARENT),
            ]),
        );
        if reveal.atmosphere > 0.01 {
            light.circle(
                c,
                r * (1.0 + 0.02 * breathe),
                Gradient::radial_fill().with_stops(&[
                    (0.0, AXON.with_alpha((38.0 * reveal.atmosphere * dim) as u8)),
                    (
                        0.55,
                        SYNAPSE.with_alpha((14.0 * reveal.atmosphere * dim) as u8),
                    ),
                    (1.0, Color::TRANSPARENT),
                ]),
            );
            // Two pulse rings, expanding out through the rings and fading.
            for k in 0..2 {
                let phase = ((time + 1.8 * k as f32) / 3.6).fract();
                let alpha = (1.0 - phase) * 26.0 * reveal.atmosphere * dim;
                if alpha > 0.5 {
                    light.ring(
                        c,
                        0.62 * r + phase * 0.52 * r,
                        0.008 * r,
                        RIM.with_alpha(alpha as u8),
                    );
                }
            }
        }

        // The organism floats; the light it gives off floats with it.
        light.transformed(Transform::translate(Offset::new(0.0, bob)), |g| {
            let eye_scale = 0.25 + 0.75 * reveal.eye;

            // The core glow behind the eye.
            let core = (0.34 + burst * 0.5) * (0.5 + 0.5 * reveal.eye);
            g.transformed(Transform::scale_around(c, eye_scale, eye_scale), |eye| {
                eye.circle(
                    c,
                    0.32 * r,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            Color::rgb(201, 250, 255).with_alpha((120.0 * core * dim) as u8),
                        ),
                        (0.5, AXON.with_alpha((60.0 * core * dim) as u8)),
                        (1.0, Color::TRANSPARENT),
                    ]),
                );
            });

            // The six tendrils, drawn at the flare scale — each grows
            // sequentially out of the eye: stem, then its branches.
            g.transformed(Transform::scale_around(c, flare, flare), |g| {
                for (i, strand) in STRANDS.iter().enumerate() {
                    paint_strand(g, c, r, strand, reveal.strands[i], time, dim, calm);
                }
            });
        });
    });

    // ---- the organism itself: eye and tendrils' root ----------------------

    book.transformed(Transform::translate(Offset::new(0.0, bob)), |g| {
        let eye_scale = 0.25 + 0.75 * reveal.eye;
        let eye_alpha = (reveal.eye * 1.6).min(1.0);
        g.transformed(Transform::scale_around(c, eye_scale, eye_scale), |eye| {
            // The dark sclera and its lit rim — the one part that does not
            // take part in the blink.
            eye.circle(c, 0.193 * r, SCLERA.with_alpha((255.0 * eye_alpha) as u8));
            eye.ring(
                c,
                0.193 * r,
                0.010 * r,
                RIM.with_alpha((184.0 * eye_alpha) as u8),
            );
            // The eyeball, blinking: squashed about its own centre.
            let lid = blink(time, 5.1);
            eye.transformed(Transform::scale_around(c, 1.0, lid), |ball| {
                ball.circle(
                    c,
                    0.143 * r,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            Color::rgb(255, 255, 255).with_alpha((255.0 * eye_alpha) as u8),
                        ),
                        (
                            0.62,
                            Color::rgb(220, 232, 255).with_alpha((245.0 * eye_alpha) as u8),
                        ),
                        (
                            1.0,
                            Color::rgb(139, 164, 210).with_alpha((235.0 * eye_alpha) as u8),
                        ),
                    ]),
                );
                // The iris: violet into teal, deep water at the rim — a
                // hair off-centre, as the reference draws it.
                let iris_at = c + Offset::new(0.010 * r, 0.007 * r);
                ball.circle(
                    iris_at,
                    0.081 * r,
                    Gradient::radial_fill().with_stops(&[
                        (
                            0.0,
                            Color::rgb(228, 215, 255).with_alpha((255.0 * eye_alpha) as u8),
                        ),
                        (0.35, AXON.with_alpha((250.0 * eye_alpha) as u8)),
                        (0.72, IRIS_RIM.with_alpha((240.0 * eye_alpha) as u8)),
                        (1.0, IRIS_DEEP.with_alpha((230.0 * eye_alpha) as u8)),
                    ]),
                );
                ball.circle(iris_at, 0.045 * r, PUPIL.with_alpha((255.0 * eye_alpha) as u8));
                // The catchlights.
                ball.circle(
                    c + Offset::new(-0.034 * r, -0.043 * r),
                    0.026 * r,
                    Color::rgba(255, 255, 255, (244.0 * eye_alpha) as u8),
                );
                ball.circle(
                    c + Offset::new(0.046 * r, 0.046 * r),
                    0.012 * r,
                    Color::rgb(217, 255, 248).with_alpha((200.0 * eye_alpha) as u8),
                );
            });
        });

        // The flare: a ring racing out when a choice is recorded.
        if burst > 0.02 {
            g.ring(
                c,
                0.22 * r + burst * 0.34 * r,
                0.010 * r,
                SYNAPSE.with_alpha(((1.0 - burst) * 190.0) as u8),
            );
        }
    });
}

/// One tendril, grown to `grown` (`0..1`) of its whole length. The growth is
/// sequential over the tendril's segments — stem first, branches sprouting
/// after — and each branch's tip pops as its own cubic completes.
#[allow(
    clippy::cast_precision_loss,
    clippy::too_many_arguments
)]
fn paint_strand(
    book: &mut Sketchbook,
    c: Offset,
    r: f32,
    strand: &Strand,
    grown: f32,
    time: f32,
    dim: f32,
    calm: f32,
) {
    if grown <= 0.01 {
        return;
    }
    let k = SPAN.recip() * r;
    let count = strand.segs.len() as f32;
    let lit = 0.85 + 0.15 * (time * TAU / 3.4).sin();

    let mut start = c + Offset::new(strand.from.0 * k, strand.from.1 * k);
    for (i, piece) in strand.segs.iter().enumerate() {
        // This cubic's own slice of the tendril's growth.
        let local = (grown * count - i as f32).clamp(0.0, 1.0);
        if local <= 0.0 {
            break;
        }
        let c1 = c + Offset::new(piece.c1.0 * k, piece.c1.1 * k);
        let c2 = c + Offset::new(piece.c2.0 * k, piece.c2.1 * k);
        let end = c + Offset::new(piece.to.0 * k, piece.to.1 * k);

        let mut path = Path::new();
        path.move_to(start);
        if local >= 1.0 {
            path.cubic_to(c1, c2, end);
        } else {
            let tip_now = cubic_at(start, c1, c2, end, local);
            // Split the cubic at `local` by drawing the full control hull
            // constrained to the sub-curve (de Casteljau, first order is
            // enough at these widths).
            let mid1 = start + (c1 - start).scale(local);
            let mid2 = c1 + (c2 - c1).scale(local);
            let mid3 = c2 + (end - c2).scale(local);
            let q1 = mid1 + (mid2 - mid1).scale(local);
            let q2 = mid2 + (mid3 - mid2).scale(local);
            path.cubic_to(q1, q2, tip_now);
        }

        if strand.violet {
            // The signal strands: a faint solid base, a travelling dashed
            // spark over it.
            book.stroke(path.clone(), AXON.with_alpha((70.0 * reveal_of(grown)) as u8), 0.018 * r);
            let mut dashes = StrokeStyle::rounded();
            dashes.dash = Some(Dash {
                pattern: vec![0.047 * r, 0.087 * r],
                offset: if calm > 0.5 {
                    0.0
                } else {
                    -(time * 0.30 * r) % (0.134 * r)
                },
            });
            book.stroke_styled(
                path,
                AXON.with_alpha((205.0 * lit * dim) as u8),
                0.018 * r,
                dashes,
            );
        } else {
            // The mint strands: solid, fully there.
            book.stroke(
                path,
                SYNAPSE.with_alpha((190.0 * reveal_of(grown) * lit) as u8),
                0.015 * r,
            );
        }

        // The growing end carries a small bright bead.
        if local < 1.0 {
            let bead = cubic_at(start, c1, c2, end, local);
            book.circle(bead, 0.016 * r, Color::rgb(214, 202, 255).with_alpha(220));
        } else if let Some(((tx, ty), tr)) = piece.tip {
            // The tip: a dot that pops as its cubic completes.
            let pop = ((grown * count - i as f32 - 1.0).clamp(0.0, 1.0) * PI).sin();
            book.circle(
                c + Offset::new(tx * k, ty * k),
                tr * k * (0.8 + 0.35 * pop),
                (if strand.violet { VIOLET_TIP } else { MINT_TIP })
                    .with_alpha((235.0 * reveal_of(grown)) as u8),
            );
        }
        start = end;
    }
}

/// A tendril's overall presence as it grows: faint at first, fully there
/// once grown.
fn reveal_of(grown: f32) -> f32 {
    (grown * 1.6).min(1.0)
}

/// The sigil: the companion's signature, small — the mark that sits in the
/// top bar of home.
///
/// The reference's own six curved strokes, all violet, with the four end
/// dots — the upper two violet, the lower two mint — and the light-eyed
/// centre that blinks on the same clock as its big sibling, so the two read
/// as one organism at two scales.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn sigil(size: f32, time: f32, burst: f32) -> Painting {
    Painting::sized(
        Size::square(size),
        PaintWith::new(move |book, s| {
            use vieww_foundation::StrokeStyle;

            // The sigil's own space: the reference draws it about 32 units
            // across, centred on the eye.
            let k = s.width / 32.0;
            let c = Offset::new(s.width / 2.0, s.height / 2.0);
            let style = StrokeStyle::rounded();
            let lift = 1.0 + burst * 0.15;

            // Six curved strokes from the centre: up, down, and the four
            // diagonals — the reference's own paths.
            let arms: [(Pt, Pt, Pt); 6] = [
                ((-2.0, -5.0), (-4.0, -9.0), (-3.0, -15.0)),
                ((3.0, 6.0), (5.0, 10.0), (4.0, 16.0)),
                ((-7.0, -8.0), (-12.0, -8.0), (-15.0, -14.0)),
                ((8.0, -8.0), (13.0, -9.0), (16.0, -15.0)),
                ((-9.0, 5.0), (-13.0, 10.0), (-15.0, 16.0)),
                ((9.0, 5.0), (14.0, 10.0), (17.0, 15.0)),
            ];
            for (c1, c2, to) in arms {
                let mut path = Path::new();
                path.move_to(c);
                path.cubic_to(
                    c + Offset::new(c1.0 * k, c1.1 * k),
                    c + Offset::new(c2.0 * k, c2.1 * k),
                    c + Offset::new(to.0 * k, to.1 * k),
                );
                book.stroke_styled(path, AXON.with_alpha((215.0 * lift).min(255.0) as u8), 1.55 * k, style.clone());
            }

            // The four end dots: violet above, mint below.
            let dots: [(Pt, Color); 4] = [
                ((-15.0, -14.0), AXON),
                ((16.0, -15.0), AXON),
                ((-15.0, 16.0), SYNAPSE),
                ((17.0, 15.0), SYNAPSE),
            ];
            for ((dx, dy), color) in dots {
                book.circle(
                    c + Offset::new(dx * k, dy * k),
                    2.2 * k,
                    color.with_alpha((230.0 * lift).min(255.0) as u8),
                );
            }

            // The light-eyed centre, blinking with its sibling.
            book.circle(c, 5.2 * k, Color::rgb(220, 232, 255));
            book.ring(c, 5.2 * k, 0.7 * k, AXON.with_alpha(210));
            let lid = blink(time, 5.1);
            book.transformed(Transform::scale_around(c, 1.0, lid), |ball| {
                ball.circle(
                    c,
                    2.8 * k,
                    Gradient::radial_fill().with_stops(&[
                        (0.0, Color::rgb(228, 215, 255)),
                        (0.35, AXON),
                        (0.72, IRIS_RIM),
                        (1.0, IRIS_DEEP),
                    ]),
                );
                ball.circle(c, 1.5 * k, PUPIL);
                ball.circle(
                    c + Offset::new(-0.9 * k, -1.1 * k),
                    0.8 * k,
                    Color::rgba(255, 255, 255, 246),
                );
            });
        }),
    )
}

// ---------------------------------------------------------------------------
// The atmosphere: the quiet room every screen floats in.
// ---------------------------------------------------------------------------

/// The screen's background life: a wide ambient bloom where the companion
/// sits, a mint wash low in a corner, hand-set filaments bowing in from the
/// frame, and a few twinkling sparks — the reference screens' own
/// atmosphere, nothing more. `intensity` is how present it is (the splash
/// fades it in, calm dims it); `quiet` is the settings variant, one bloom in
/// the top right and two filaments, because the form is the subject there.
#[must_use]
pub fn atmosphere(time: f32, intensity: f32, quiet: bool) -> Painting {
    Painting::new(PaintWith::new(move |book, size| {
        paint_atmosphere(book, size, time, intensity, quiet);
    }))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn paint_atmosphere(book: &mut Sketchbook, s: Size, time: f32, intensity: f32, quiet: bool) {
    if intensity <= 0.005 {
        return;
    }
    let sx = s.width / 420.0;
    let sy = s.height / 880.0;
    let at = |x: f32, y: f32| Offset::new(x * sx, y * sy);

    // One bowing filament, from a to b through its two controls.
    let filament =
        |book: &mut Sketchbook, a: Pt, c1: Pt, c2: Pt, b: Pt, color: Color, width: f32| {
            let mut path = Path::new();
            path.move_to(at(a.0, a.1));
            path.cubic_to(at(c1.0, c1.1), at(c2.0, c2.1), at(b.0, b.1));
            book.stroke(path, color, width * sx);
        };

    if quiet {
        // The studio: one bloom over the form's shoulder, two filaments.
        book.circle(
            at(330.0, 155.0),
            160.0 * sx,
            Gradient::radial_fill().with_stops(&[
                (0.0, AXON.with_alpha((46.0 * intensity) as u8)),
                (0.55, SYNAPSE.with_alpha((12.0 * intensity) as u8)),
                (1.0, Color::TRANSPARENT),
            ]),
        );
        let a = (31.0 * intensity) as u8;
        filament(
            book,
            (-30.0, 190.0),
            (22.0, 165.0),
            (42.0, 184.0),
            (68.0, 213.0),
            AXON.with_alpha(a),
            1.2,
        );
        filament(
            book,
            (68.0, 213.0),
            (94.0, 242.0),
            (111.0, 246.0),
            (138.0, 238.0),
            AXON.with_alpha(a),
            1.2,
        );
        filament(
            book,
            (433.0, 408.0),
            (389.0, 383.0),
            (360.0, 401.0),
            (339.0, 431.0),
            SYNAPSE.with_alpha(a),
            1.2,
        );
        filament(
            book,
            (339.0, 431.0),
            (318.0, 461.0),
            (300.0, 470.0),
            (266.0, 472.0),
            SYNAPSE.with_alpha(a),
            1.2,
        );
        return;
    }

    // The room: the wide ambient bloom behind the companion, the low mint
    // wash, four filaments, four sparks.
    book.circle(
        at(210.0, 350.0),
        228.0 * sx,
        Gradient::radial_fill().with_stops(&[
            (0.0, Color::rgb(156, 123, 247).with_alpha((70.0 * intensity) as u8)),
            (0.55, Color::rgb(101, 223, 208).with_alpha((18.0 * intensity) as u8)),
            (1.0, Color::TRANSPARENT),
        ]),
    );
    book.circle(
        at(40.0, 765.0),
        120.0 * sx,
        Gradient::radial_fill().with_stops(&[
            (0.0, SYNAPSE.with_alpha((14.0 * intensity) as u8)),
            (1.0, Color::TRANSPARENT),
        ]),
    );

    let a = (41.0 * intensity) as u8;
    filament(
        book,
        (-20.0, 230.0),
        (42.0, 207.0),
        (47.0, 153.0),
        (99.0, 138.0),
        AXON.with_alpha(a),
        1.3,
    );
    filament(
        book,
        (99.0, 138.0),
        (151.0, 123.0),
        (164.0, 113.0),
        (185.0, 65.0),
        AXON.with_alpha(a),
        1.3,
    );
    filament(
        book,
        (439.0, 186.0),
        (394.0, 204.0),
        (373.0, 235.0),
        (376.0, 275.0),
        SYNAPSE.with_alpha(a),
        1.3,
    );
    filament(
        book,
        (376.0, 275.0),
        (379.0, 315.0),
        (364.0, 329.0),
        (344.0, 348.0),
        SYNAPSE.with_alpha(a),
        1.3,
    );
    filament(
        book,
        (-16.0, 735.0),
        (32.0, 710.0),
        (58.0, 723.0),
        (89.0, 759.0),
        AXON.with_alpha((35.0 * intensity) as u8),
        1.1,
    );
    filament(
        book,
        (89.0, 759.0),
        (120.0, 795.0),
        (141.0, 799.0),
        (181.0, 801.0),
        AXON.with_alpha((35.0 * intensity) as u8),
        1.1,
    );
    filament(
        book,
        (438.0, 694.0),
        (401.0, 672.0),
        (368.0, 693.0),
        (342.0, 725.0),
        SYNAPSE.with_alpha((35.0 * intensity) as u8),
        1.1,
    );
    filament(
        book,
        (342.0, 725.0),
        (316.0, 757.0),
        (294.0, 759.0),
        (262.0, 764.0),
        SYNAPSE.with_alpha((35.0 * intensity) as u8),
        1.1,
    );

    // The sparks, each on its own slow twinkle.
    let sparks: [(Pt, f32, f32); 4] = [
        ((48.0, 230.0), 2.0, 0.0),
        ((365.0, 264.0), 2.2, 1.4),
        ((72.0, 700.0), 1.8, 2.2),
        ((348.0, 740.0), 2.0, 0.7),
    ];
    for ((x, y), radius, phase) in sparks {
        let t = twinkle(time, 3.0 + phase * 0.4, phase);
        book.circle(
            at(x, y),
            radius * sx * (0.7 + 0.35 * t),
            Color::rgb(143, 123, 228).with_alpha(((26.0 + 66.0 * t) * intensity) as u8),
        );
    }
}
