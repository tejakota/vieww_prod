//! An opt-in linear-light compositing pipeline — "Renderer v2" pillar B
//! (`docs/RENDERER-V2-NOTES.md`): "linear light → working color space →
//! premultiplied compositing → effect processing → display transform →
//! surface encoding."
//!
//! # Why this renderer blends in gamma space by default, and always has
//!
//! `Color::rgba`'s channels are sRGB-*encoded* 8-bit values — what every
//! platform widget toolkit, design tool and CSS color actually hands a
//! renderer. `Premul::from_straight` has always taken those encoded values
//! and premultiplied them by alpha directly, with no transfer-function
//! decode first — every blend this renderer has ever computed has been
//! arithmetic on gamma-encoded numbers, treating "0.5" as if it meant
//! half the *light*, when the sRGB transfer function actually makes it
//! mean roughly a fifth of the light (`srgb_to_linear(0.5) ≈ 0.214`). This
//! is extremely common — it's what most 2D UI toolkits have always done —
//! and it is not colorimetrically correct: averaging two sRGB-encoded
//! values does not average the light those values represent, which is most
//! visible exactly where the spec review flagged it, on a soft gradient or
//! a translucent overlay between two contrasting colors, where gamma-space
//! blending produces a visibly darker, muddier midpoint than physical light
//! mixing would.
//!
//! # What "opt in" means, concretely
//!
//! [`ColorPipeline::GammaSpace`] is `NativeRenderer`'s default and is
//! **byte-for-byte** what this renderer has always produced — every
//! existing pixel test (`tests/native_parity.rs` and the rest) keeps
//! passing unchanged, because the default pipeline runs the exact same
//! `color::blend` call on the exact same numbers it always has.
//! [`ColorPipeline::LinearLight`], selected via
//! `NativeRenderer::with_color_pipeline`, decodes both operands from sRGB to
//! linear light before compositing and re-encodes the result back to sRGB
//! afterward — everything in between (the blend-mode arithmetic itself) is
//! the exact same `color::blend` this renderer already had, run on
//! differently-encoded numbers, not a second implementation of blending.
//! `blend_with_pipeline` is the one function that makes that choice; every
//! call site that used to call `color::blend` directly now goes through it.
//!
//! # What this does not do, and why
//!
//! Display P3, wide-gamut textures, 10-bit surfaces, HDR and scRGB-style
//! extended range are genuinely out of reach here, not merely undone: this
//! renderer's whole pixel format is 8-bit sRGB RGBA in and out
//! (`Pixels`/`Color`), there is no wider-gamut colour type anywhere in
//! `vieww-foundation` to carry a P3 or extended-range value between the
//! surface and this module, and there is no display in this build's test
//! environment (headless PNGs and a software Vulkan swapchain under Xvfb)
//! that could show a wide-gamut result even if one were computed. Building
//! a P3/HDR path with nothing able to consume or verify it would be
//! exactly the "unverified sketch that looks further along than it is"
//! `docs/RENDERER-MIGRATION.md` already declined to do for the vello
//! migration; this module goes as far as the 8-bit sRGB pipeline that
//! actually exists lets it go, and stops there honestly.

use super::color::{blend, Premul};
use vieww_foundation::BlendMode;

/// Which color space `blend_with_pipeline` runs its arithmetic in.
/// `GammaSpace` is `NativeRenderer`'s default — see the module docs for why
/// switching the default was never on the table for this pillar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorPipeline {
    #[default]
    GammaSpace,
    LinearLight,
}

/// [`super::color::blend`], run in whichever space `pipeline` selects.
/// `GammaSpace` is a direct passthrough — literally the same call, same
/// numbers, same result as before this pillar existed.
#[must_use]
pub(crate) fn blend_with_pipeline(
    pipeline: ColorPipeline,
    mode: BlendMode,
    src: Premul,
    dst: Premul,
) -> Premul {
    match pipeline {
        ColorPipeline::GammaSpace => blend(mode, src, dst),
        ColorPipeline::LinearLight => {
            let linear_result = blend(mode, to_linear(src), to_linear(dst));
            to_gamma(linear_result)
        }
    }
}

/// The IEC 61966-2-1 sRGB electro-optical transfer function: the piecewise
/// curve (a near-linear segment below `0.0031308`, a power curve above it)
/// that actually defines "sRGB", not the `x^2.2` approximation some
/// pipelines substitute for it. Getting the toe segment right matters
/// disproportionately here — it is where shadow detail lives, and the pure
/// power-law approximation is visibly wrong specifically in the dark
/// values a translucent shadow or a dim overlay spends most of its range
/// in.
#[must_use]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The inverse of [`srgb_to_linear`] — the sRGB opto-electronic transfer
/// function, encoding a linear-light value back to sRGB for display/output.
#[must_use]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// How many levels the [`coverage_alpha`] table quantises coverage into.
/// 1024: the rounding error at the darkest fringe stays below half an
/// 8-bit alpha step while the table keeps one cache line per pixel row.
pub(crate) const COVERAGE_LEVELS: usize = 1024;

/// The perceptual transfer applied to rasterised coverage before it becomes
/// alpha — the one change that separates thin, washed-out AA from the
/// Skia-style AA the reference renderers ship.
///
/// Straight from geometry, coverage is a fraction of a pixel's area. But a
/// display pixel is light, not area, and a half-lit pixel reads *darker*
/// than half to the eye — so a stem composited from linear coverage looks
/// spindly next to the same stem through a gamma ramp, most visibly as
/// thin, grey text and hairline icons on dark backgrounds.
///
/// Skia — and therefore VS Code, Chrome and every Chromium surface —
/// applies roughly this curve to **all** antialiased coverage, text and
/// vector shapes alike, as its A8 coverage gamma. This is the same policy,
/// one transfer for every caller; subpixel (RGB-stripe) coverage is the
/// separate, opt-in LCD path and is deliberately not put through it.
///
/// The curve is the square root — the standard coverage gamma, midway
/// between FreeType's gamma-1.8 ramp (c^0.556) and Skia's steeper dark-mode
/// contrast (~c^0.45). Endpoints stay endpoints: a full pixel stays full
/// and an empty one stays empty, so every opaque fast path survives, and
/// coverage above one (accumulated overshoot) is passed through untouched —
/// it is already "more than covered" and squashing it would silently
/// change the blend.
///
/// A table rather than a per-pixel `powf`: the fringe of every glyph and
/// every shape in the frame pays this, and one cache-resident lookup costs
/// about what the linear scaling it feeds costs.
#[must_use]
pub(crate) fn coverage_alpha(coverage: f32) -> f32 {
    coverage_alpha_in(coverage_table(), coverage)
}

/// The table behind [`coverage_alpha`], for a loop that maps a whole row:
/// fetched once per draw rather than once per pixel. The per-pixel
/// `OnceLock` check was 3.4% of a layer-heavy frame on its own (callgrind,
/// `00-layers-nested`, FW-07).
#[must_use]
pub(crate) fn coverage_table() -> &'static [f32; COVERAGE_LEVELS] {
    use std::sync::OnceLock;

    static TABLE: OnceLock<[f32; COVERAGE_LEVELS]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|level| (level as f32 / (COVERAGE_LEVELS - 1) as f32).sqrt())
    })
}

/// [`coverage_alpha`] against a table already in hand — the same arithmetic,
/// so the two are byte-identical by construction.
#[inline]
#[must_use]
pub(crate) fn coverage_alpha_in(table: &[f32; COVERAGE_LEVELS], coverage: f32) -> f32 {
    if coverage <= 0.0 {
        return 0.0;
    }
    if coverage >= 1.0 {
        return coverage;
    }
    table[((coverage * (COVERAGE_LEVELS - 1) as f32).round() as usize).min(COVERAGE_LEVELS - 1)]
}

/// `color`'s straight RGB, sRGB-decoded to linear light and re-premultiplied
/// by its own (untouched — alpha is not gamma-encoded) alpha.
fn to_linear(color: Premul) -> Premul {
    map_straight_rgb(color, srgb_to_linear)
}

/// The inverse of [`to_linear`]: linear-light premultiplied color, encoded
/// back to sRGB-gamma premultiplied color.
fn to_gamma(color: Premul) -> Premul {
    map_straight_rgb(color, linear_to_srgb)
}

/// Unpremultiply, apply `transfer` to each of the three color channels
/// (never to alpha, which carries the *coverage* transfer instead — see
/// [`coverage_alpha`], applied where coverage becomes alpha, not here),
/// re-premultiply by the original alpha.
fn map_straight_rgb(color: Premul, transfer: impl Fn(f32) -> f32) -> Premul {
    if color.a <= 0.0 {
        return color;
    }
    let inv = 1.0 / color.a;
    let straight = [color.r * inv, color.g * inv, color.b * inv];
    let mapped = straight.map(|c| transfer(c.clamp(0.0, 1.0)));
    Premul {
        r: mapped[0] * color.a,
        g: mapped[1] * color.a,
        b: mapped[2] * color.a,
        a: color.a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_foundation::Color;

    fn opaque(r: u8, g: u8, b: u8) -> Premul {
        Premul::from_straight(Color::rgba(r, g, b, 255))
    }

    #[test]
    fn the_transfer_function_round_trips() {
        for i in 0..=255u32 {
            let c = i as f32 / 255.0;
            let round_tripped = linear_to_srgb(srgb_to_linear(c));
            assert!(
                (round_tripped - c).abs() < 1e-5,
                "{c} round-tripped to {round_tripped}"
            );
        }
    }

    #[test]
    fn zero_and_one_are_fixed_points() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert!((linear_to_srgb(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mid_gray_decodes_to_roughly_a_fifth_of_the_light_not_half() {
        // The textbook illustration of why gamma-space blending is wrong:
        // sRGB 0.5 is nowhere near half the physical light.
        let linear = srgb_to_linear(0.5);
        assert!(
            (0.20..0.23).contains(&linear),
            "srgb_to_linear(0.5) = {linear}, expected roughly 0.214"
        );
    }

    #[test]
    fn gamma_space_pipeline_is_byte_identical_to_calling_blend_directly() {
        let src = opaque(220, 30, 30);
        let dst = opaque(30, 30, 220);
        for mode in BlendMode::ALL {
            let direct = blend(mode, src, dst);
            let piped = blend_with_pipeline(ColorPipeline::GammaSpace, mode, src, dst);
            assert_eq!(
                direct, piped,
                "{mode:?}: GammaSpace pipeline must be a pure passthrough"
            );
        }
    }

    /// The actual pillar-B deliverable: linear-space compositing produces a
    /// measurably different, and colorimetrically correct, midpoint from
    /// naive gamma-space blending on a known case — 50% `Normal` opacity
    /// halfway between black and white, the simplest possible averaging
    /// blend. Gamma space "averages" the encoded numbers (0 and 1) to get
    /// encoded 0.5 — visually a fairly dark gray, because sRGB 0.5 is only
    /// ~21% of the light. Linear space averages the *light* (0 and 1) to
    /// get linear 0.5 (a true half-brightness gray), then encodes *that* —
    /// which comes out to sRGB ≈0.735, a visibly lighter gray. This is the
    /// well-known, textbook-documented difference (see e.g. the "dark
    /// gradient" artifact any gamma-space image resize or blend produces).
    #[test]
    fn linear_light_normal_half_alpha_over_produces_a_lighter_midpoint_than_gamma_space() {
        // 50%-alpha white, correctly premultiplied: straight (1,1,1) at
        // alpha 0.5 stores as (0.5, 0.5, 0.5, 0.5), not (1,1,1,0.5).
        let src = Premul {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 0.5,
        };
        let dst = opaque(0, 0, 0); // opaque black backdrop

        let gamma_result =
            blend_with_pipeline(ColorPipeline::GammaSpace, BlendMode::Normal, src, dst);
        let linear_result =
            blend_with_pipeline(ColorPipeline::LinearLight, BlendMode::Normal, src, dst);

        let gamma_gray = gamma_result.to_straight_u8()[0];
        let linear_gray = linear_result.to_straight_u8()[0];

        assert_eq!(
            gamma_gray, 128,
            "gamma-space 50% white over black is the textbook encoded-0.5 midpoint"
        );
        assert!(
            linear_gray > gamma_gray + 50,
            "linear-light compositing must be measurably lighter: gamma={gamma_gray}, linear={linear_gray}"
        );
        // The colorimetrically correct answer: linear 0.5 gray, re-encoded.
        let expected_linear_encoded = (linear_to_srgb(0.5) * 255.0).round() as u8;
        assert!(
            (i16::from(linear_gray) - i16::from(expected_linear_encoded)).abs() <= 1,
            "linear result {linear_gray} should match the closed-form answer {expected_linear_encoded}"
        );
    }
}
