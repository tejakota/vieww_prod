//! The SSE2 half of the crossing interpolation - see
//! the containing module's `interpolate_active` for the contract and the
//! argument. SSE2 is part of the x86-64 baseline (`native::simd`'s module
//! doc holds the reasoning); the `target_feature` attribute is Rust's
//! asking price for the intrinsics, not a negotiation with the hardware.
//!
//! # Why it stayed four lanes when the AVX2 dispatch arrived
//!
//! Eight edges per group needs two of these 4x4 transposes plus the
//! lane-crossing combines that stack their outputs into 256-bit registers:
//! about two and a half lane operations per edge against the two the
//! four-edge group spends, to share an arithmetic chain that costs under
//! two ops per edge at four wide. Half an op saved per edge out of five,
//! and only when eight or more edges are active in one subsample — a
//! rectangle never gets there (four edges), and the shapes that do are the
//! minority of any real frame, so the eight-group would mostly run as a
//! four-group with dead lanes. Not a win; left at four until a profile
//! says otherwise.

use core::arch::x86_64::{
    _mm_add_ps, _mm_div_ps, _mm_loadu_ps, _mm_movehl_ps, _mm_movelh_ps, _mm_mul_ps, _mm_set1_ps,
    _mm_storeu_ps, _mm_sub_ps, _mm_unpackhi_ps, _mm_unpacklo_ps,
};

use super::Edge;

/// The four edges' crossing `x` values at `sy`, one register set for all
/// four: `(sy - y0) / (y1 - y0)` then `x0 + t * (x1 - x0)`, lane for lane
/// the scalar walk's pair, the divide shared by four edges.
///
/// # Safety
///
/// SSE2 is part of the x86-64 baseline; the loads read each edge's first
/// 16 bytes (`Edge` is `repr(C)`, `x0, y0, x1, y1` first), and the store
/// writes four `f32`s into a `[f32; 4]` on the stack.
#[target_feature(enable = "sse2")]
pub(super) unsafe fn interpolate4(e: [&Edge; 4], sy: f32) -> [f32; 4] {
    unsafe {
        let word = |edge: &Edge| core::ptr::from_ref(edge).cast::<f32>();
        let r0 = _mm_loadu_ps(word(e[0]));
        let r1 = _mm_loadu_ps(word(e[1]));
        let r2 = _mm_loadu_ps(word(e[2]));
        let r3 = _mm_loadu_ps(word(e[3]));
        // 4x4 transpose: edge i's word is [x0, y0, x1, y1]; the unpack/move
        // pairs gather the four words' like fields into one register each.
        // `movehl(a, b)` is [b2, b3, a2, a3]; `movelh(a, b)` is [a0, a1,
        // b0, b1] - eight shuffles, the standard SSE2 transpose.
        let lo01 = _mm_unpacklo_ps(r0, r1);
        let hi01 = _mm_unpackhi_ps(r0, r1);
        let lo23 = _mm_unpacklo_ps(r2, r3);
        let hi23 = _mm_unpackhi_ps(r2, r3);
        let x0 = _mm_movelh_ps(lo01, lo23);
        let y0 = _mm_movehl_ps(lo23, lo01);
        let x1 = _mm_movelh_ps(hi01, hi23);
        let y1 = _mm_movehl_ps(hi23, hi01);
        let t = _mm_div_ps(_mm_sub_ps(_mm_set1_ps(sy), y0), _mm_sub_ps(y1, y0));
        let x = _mm_add_ps(x0, _mm_mul_ps(t, _mm_sub_ps(x1, x0)));
        let mut out = [0.0f32; 4];
        _mm_storeu_ps(out.as_mut_ptr(), x);
        out
    }
}
