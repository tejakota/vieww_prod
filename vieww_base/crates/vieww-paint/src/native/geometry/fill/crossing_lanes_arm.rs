//! The NEON half of the crossing interpolation - see the containing module's
//! `interpolate_active` for the contract, and `native::simd::neon`'s doc for
//! the written-not-run status this half shares with every other aarch64
//! kernel in this crate: `cross-check.sh` keeps it compiling for
//! aarch64-apple-ios and aarch64-linux-android, and the first ARM run of
//! the bit test below is its verdict. NEON is part of the aarch64 baseline.

use core::arch::aarch64::{
    vaddq_f32, vcombine_f32, vdivq_f32, vdupq_n_f32, vget_high_f32, vget_low_f32, vld1q_f32,
    vmulq_f32, vst1q_f32, vsubq_f32, vtrnq_f32,
};

use super::Edge;

/// The four edges' crossing `x` values at `sy`, one register set for all
/// four: `(sy - y0) / (y1 - y0)` then `x0 + t * (x1 - x0)`, lane for lane
/// the scalar walk's pair, the divide shared by four edges.
///
/// # Safety
///
/// NEON is part of the aarch64 baseline; the loads read each edge's first
/// 16 bytes (`Edge` is `repr(C)`, `x0, y0, x1, y1` first), and the store
/// writes four `f32`s into a `[f32; 4]` on the stack.
#[target_feature(enable = "neon")]
pub(super) unsafe fn interpolate4(e: [&Edge; 4], sy: f32) -> [f32; 4] {
    unsafe {
        let word = |edge: &Edge| core::ptr::from_ref(edge).cast::<f32>();
        let r0 = vld1q_f32(word(e[0]));
        let r1 = vld1q_f32(word(e[1]));
        let r2 = vld1q_f32(word(e[2]));
        let r3 = vld1q_f32(word(e[3]));
        // 4x4 transpose: `vtrnq_f32(a, b)` interleaves the even lanes into
        // `.0` and the odd lanes into `.1`; combine low/high halves to pick
        // each field out of the four words.
        let lo01 = vtrnq_f32(r0, r1);
        let lo23 = vtrnq_f32(r2, r3);
        let x0 = vcombine_f32(vget_low_f32(lo01.0), vget_low_f32(lo23.0));
        let y0 = vcombine_f32(vget_low_f32(lo01.1), vget_low_f32(lo23.1));
        let x1 = vcombine_f32(vget_high_f32(lo01.0), vget_high_f32(lo23.0));
        let y1 = vcombine_f32(vget_high_f32(lo01.1), vget_high_f32(lo23.1));
        let t = vdivq_f32(vsubq_f32(vdupq_n_f32(sy), y0), vsubq_f32(y1, y0));
        let x = vaddq_f32(x0, vmulq_f32(t, vsubq_f32(x1, x0)));
        let mut out = [0.0f32; 4];
        vst1q_f32(out.as_mut_ptr(), x);
        out
    }
}
