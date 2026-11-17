//! The two compositing inner loops that dominate a full repaint, as explicit
//! SIMD — **byte-identical** to the scalar loops they replace, which stay
//! here as the reference and the fallback. x86-64 gets SSE2 (the `sse2`
//! module) and, when the CPU reports it, AVX2 (the `avx2` module — the same
//! kernels twice as wide, behind a once-per-process check); aarch64 gets
//! NEON (the `neon` module, the same loops lane for lane — written and
//! cross-type-checked, and labelled there as not yet run on ARM). The
//! shadow blur's vertical strip steps live here too, as strip-level kernels
//! for the same reason.
//!
//! # Why these two
//!
//! `Target::composite_flat` (a solid colour through a coverage mask — every
//! fill, stroke and glyph) and `Target::composite_layer` (a resolved layer
//! onto its parent) are the two loops a callgrind profile of
//! `examples/fixtures`' over-budget screens puts at the top: `00-layers-nested`
//! spends about half its frame in them, `23-editor-glass` about a fifth. Both
//! run the normal-blend, gamma-space path almost exclusively, and that path is
//! what is vectorised here; every other blend mode and pipeline still goes
//! through `blend_with_pipeline`, unchanged.
//!
//! # Which kernels grew an AVX2 half, and why the ops did not
//!
//! The compositing row kernels — `over_layer_row`, its zeroing twin
//! `over_layer_row_take` (the path `composite_layer_and_clear` resolves a
//! nested layer through, which is where `00-layers-nested` spends its
//! compositing) and `over_flat_row` — and the shadow blur's vertical strip
//! steps (`strip_add`, `strip_store`, `strip_shift`, below). Those are the
//! places a whole row or strip is in hand at once: two adjacent `Premul`s
//! are exactly one 256-bit register, and the blur strip's sixteen columns
//! are two of them, so the AVX2 halves issue the same instruction count as
//! the SSE2 ones at twice the width.
//!
//! The lane and premul ops below stay four wide on purpose. Their callers
//! hold one 4-lane group per call — `native/effects.rs`' blur passes call
//! `premul_add` one pixel at a time — so an eight-lane dispatch *of the op*
//! would still be handed four lanes and pair nothing. The widening has to
//! happen where the groups can be paired, which is the kernel, not the op.
//! The fill walk's crossing interpolation (`crossing_lanes_x86`) was
//! measured out on op count — its doc records the count that keeps it
//! SSE2. `native/effects.rs`' own `vertical` strip is the same idea in
//! `Premul`s and keeps the four-lane ops: this change widens the kernels
//! the budget chase named and leaves the next one for the profile that
//! asks for it.
//!
//! The scalar loops spent most of their instructions *around* the
//! arithmetic — iterator bounds checks, four scalar channels, and a chain of
//! per-pixel branches — not in it. A `Premul` is exactly one 128-bit
//! register, so here each pixel is one load, one multiply, the `over`
//! arithmetic, and the branches become lane selects.
//!
//! # Why it is the same bytes
//!
//! Every branch of the scalar loop becomes a mask, evaluated under the same
//! comparison (`<=`, `>=`, `==`, each false for `NaN` exactly as in scalar
//! code), and the nested selects apply them in the scalar loop's order of
//! precedence. Every arithmetic step is the scalar one in the same order and
//! never fused: `scaled` is a per-channel multiply; `over` is
//! `porter_duff(src, dst, 1.0, 1.0 - src.a)`, i.e. `src * 1.0 + dst * fb`,
//! and `src * 1.0` is an IEEE identity, so it is `src + dst * fb`. A skipped
//! pixel is written back with the value it was loaded with, which is the same
//! bits as not writing it.
//!
//! The one thing that can differ is which `NaN` a `NaN` comes out as. x86
//! propagates the first operand's `NaN`, and which operand is first is the
//! compiler's choice in the scalar loop as much as here (`src * k` and
//! `k * src` are the same multiply to LLVM), so a `NaN` channel may come out
//! with the other sign bit. Nothing can observe it: a `NaN` stays `NaN`
//! through every later blend and converts to byte 0 on the way out either
//! way. A `NaN` only reaches these loops from an already-invalid input in the
//! first place (infinite alpha, say).
//!
//! `simd_rows_match_scalar_bit_for_bit` below holds the row kernels to
//! their scalar reference — and, on x86-64, the AVX2 and SSE2 halves to each
//! other — over hostile and random inputs, compared as raw bits with every
//! `NaN` treated as one value.

use super::color::{over, Premul};

/// Whether this process may run the `avx2` halves: asked once per process
/// and held, like the repo's other once-per-process reads — the answer is a
/// property of the machine and cannot change while the process lives (a
/// CPU does not grow AVX2, and a VM masking CPUID masks it for the whole
/// run). The dispatcher fns below branch on it at the kernel boundary —
/// once per row for the compositing kernels, once per strip step for the
/// blur — never per pixel or per lane group, the granularity
/// `linear.rs`' `coverage_table` measured at 3.4% of a layer-heavy frame.
#[cfg(target_arch = "x86_64")]
fn has_avx2() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::arch::is_x86_feature_detected!("avx2"))
}

/// `composite_layer`'s normal-blend, gamma-space row: `src` (scaled by the
/// layer's group `alpha` unless it is exactly 1) over `dst`, pixel for pixel.
pub(crate) fn over_layer_row(dst: &mut [Premul], src: &[Premul], alpha: f32, opaque_copy: bool) {
    debug_assert_eq!(dst.len(), src.len());
    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY: `has_avx2()` asked the CPU once for this process; see
            // `avx2`'s doc for what that check promises.
            unsafe { avx2::over_layer_row(dst, src, alpha, opaque_copy) }
        } else {
            // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
            unsafe { sse2::over_layer_row(dst, src, alpha, opaque_copy) }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::over_layer_row(dst, src, alpha, opaque_copy) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    over_layer_row_scalar(dst, src, alpha, opaque_copy);
}

/// [`over_layer_row`], leaving `src` fully transparent: each source pixel is
/// zeroed in the same pass that reads it. See
/// `Target::composite_layer_and_clear`.
pub(crate) fn over_layer_row_take(
    dst: &mut [Premul],
    src: &mut [Premul],
    alpha: f32,
    opaque_copy: bool,
) {
    debug_assert_eq!(dst.len(), src.len());
    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY: `has_avx2()` asked the CPU once for this process; see
            // `avx2`'s doc for what that check promises.
            unsafe { avx2::over_layer_row_take(dst, src, alpha, opaque_copy) }
        } else {
            // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
            unsafe { sse2::over_layer_row_take(dst, src, alpha, opaque_copy) }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::over_layer_row_take(dst, src, alpha, opaque_copy) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        over_layer_row_scalar(dst, src, alpha, opaque_copy);
        src.fill(Premul::TRANSPARENT);
    }
}

/// `composite_flat`'s normal-blend, gamma-space row: `color` through the
/// coverage `mask` over `dst`.
pub(crate) fn over_flat_row(dst: &mut [Premul], mask: &[f32], color: Premul, replaces: bool) {
    debug_assert_eq!(dst.len(), mask.len());
    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY: `has_avx2()` asked the CPU once for this process; see
            // `avx2`'s doc for what that check promises.
            unsafe { avx2::over_flat_row(dst, mask, color, replaces) }
        } else {
            // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
            unsafe { sse2::over_flat_row(dst, mask, color, replaces) }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::over_flat_row(dst, mask, color, replaces) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    over_flat_row_scalar(dst, mask, color, replaces);
}

// ---------------------------------------------------------------------------
// Lane-wise float ops: add, subtract, divide.
//
// The blur passes (`native/effects.rs`) and the fill walk's edge
// interpolation (`native/geometry/fill.rs`) are running sums and
// closed-form expressions over one, four or sixteen independent `f32` values
// at a time. The kernels there already keep each value's operation sequence
// in a fixed order — a running sum is sequential by construction and each
// sum is independent of every other — so widening each *group* of four
// values into one register lane-for-lane changes no operation, no order and
// no rounding: lane `i` of `lanes_add(a, b)` is exactly `a[i] + b[i]`, the
// op the scalar loop ran for that value. These entry points are the
// whole contract; the callers decide the grouping.
//
// The grouping is also why these ops have no AVX2 halves: a caller that
// hands over one four-lane group per call cannot pair it with anything, so
// an eight-lane dispatch of the op itself would widen nothing — the halves
// went to the kernels that hold whole rows and strips (the row kernels
// above, the strip steps below), where two groups are in hand. The module
// doc holds the full argument.
//
// The same "no runtime detection" argument as the row kernels above applies:
// SSE2 and NEON are baselines of their architectures, and the scalar
// fallback below is the reference any third architecture gets — and the one
// the bit tests compare against on this one.
// ---------------------------------------------------------------------------

/// Four independent `f32` lanes, added lane for lane.
#[must_use]
pub(crate) fn lanes_add(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
        unsafe { sse2::lanes_add(a, b) }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::lanes_add(a, b) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    [
        a[0] + b[0],
        a[1] + b[1],
        a[2] + b[2],
        a[3] + b[3],
    ]
}

/// Four independent `f32` lanes, subtracted lane for lane.
#[must_use]
pub(crate) fn lanes_sub(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
        unsafe { sse2::lanes_sub(a, b) }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::lanes_sub(a, b) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    [
        a[0] - b[0],
        a[1] - b[1],
        a[2] - b[2],
        a[3] - b[3],
    ]
}

/// Four `f32` lanes divided by one scalar, lane for lane. Divided, never
/// multiplied by a precomputed reciprocal: `x / w` and `x * (1/w)` differ in
/// the last bit, and the blur kernels are correctness oracles before they
/// are fast paths (`native/effects.rs`'s `horizontal` records the six-pixel
/// panel the fixture gallery caught on exactly that difference).
#[must_use]
pub(crate) fn lanes_div_scalar(a: [f32; 4], k: f32) -> [f32; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
        unsafe { sse2::lanes_div_scalar(a, k) }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::lanes_div_scalar(a, k) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    [
        a[0] / k,
        a[1] / k,
        a[2] / k,
        a[3] / k,
    ]
}

/// A `Premul`'s four channels, added as one register — `a.r + b.r` and
/// friends, lane for lane, the operation the scalar caller ran.
#[must_use]
pub(crate) fn premul_add(a: Premul, b: Premul) -> Premul {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
        unsafe { sse2::premul_add(a, b) }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::premul_add(a, b) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    Premul {
        r: a.r + b.r,
        g: a.g + b.g,
        b: a.b + b.b,
        a: a.a + b.a,
    }
}

/// A `Premul`'s four channels, subtracted as one register.
#[must_use]
pub(crate) fn premul_sub(a: Premul, b: Premul) -> Premul {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
        unsafe { sse2::premul_sub(a, b) }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::premul_sub(a, b) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    Premul {
        r: a.r - b.r,
        g: a.g - b.g,
        b: a.b - b.b,
        a: a.a - b.a,
    }
}

/// A `Premul`'s four channels divided by one scalar, as one register.
/// Divided, never reciprocal-multiplied — `native/effects.rs`'s `horizontal`
/// documents the fixture-gallery panel that caught the difference.
#[must_use]
pub(crate) fn premul_div(p: Premul, k: f32) -> Premul {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86-64 baseline; see `sse2`'s doc.
        unsafe { sse2::premul_div(p, k) }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; see `neon`'s doc.
        unsafe { neon::premul_div(p, k) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    Premul {
        r: p.r / k,
        g: p.g / k,
        b: p.b / k,
        a: p.a / k,
    }
}

// ---------------------------------------------------------------------------
// The shadow blur's vertical strip, one step per call.
//
// `native/shadow.rs`'s `box_blur_vertical` carries sixteen columns down the
// mask at once; the three steps it makes per strip — the seed add, the
// divided store, the window shift — are the `lanes_*` ops above applied to
// the whole strip, so they live here at the kernel boundary rather than one
// four-lane group at a time in the caller. That is what makes an AVX2 half
// of them possible: sixteen columns is two 256-bit registers, and a machine
// that reports AVX2 runs the strip eight columns per op while every other
// machine keeps the four-lane grouping the caller used before. Same bytes
// either way — each column keeps its own running sum and its own divide, so
// the grouping widens and the per-column arithmetic does not (`avx2`'s doc
// holds the full argument). `shadow_box_blurs_match_scalar` pins the whole
// kernel against the scalar walk, and the strip sweeps in
// `lane_ops_match_scalar_bit_for_bit` pin the steps against the scalar ops
// and the four-lane fallback.
// ---------------------------------------------------------------------------

/// The strip's seed step: `sums[c] += row[c]`, the whole strip at once.
pub(crate) fn strip_add(sums: &mut [f32], row: &[f32]) {
    debug_assert_eq!(sums.len(), row.len());
    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY: `has_avx2()` asked the CPU once for this process; see
            // `avx2`'s doc for what that check promises.
            unsafe { avx2::strip_add(sums, row) }
        } else {
            strip_add_fallback(sums, row);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        strip_add_fallback(sums, row);
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        for (s, &r) in sums.iter_mut().zip(row) {
            *s += r;
        }
    }
}

/// The strip's store step: `out[c] = sums[c] / window` — divided, never
/// reciprocal-multiplied, `lanes_div_scalar`'s rule and its reason.
pub(crate) fn strip_store(sums: &[f32], out: &mut [f32], window: f32) {
    debug_assert_eq!(sums.len(), out.len());
    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY: `has_avx2()` asked the CPU once for this process; see
            // `avx2`'s doc for what that check promises.
            unsafe { avx2::strip_store(sums, out, window) }
        } else {
            strip_store_fallback(sums, out, window);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        strip_store_fallback(sums, out, window);
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        for (o, &s) in out.iter_mut().zip(sums) {
            *o = s / window;
        }
    }
}

/// The strip's window shift: `sums[c] += entering[c] - leaving[c]`, one
/// expression per column — the spelling the scalar reference keeps, because
/// splitting the add from the subtract moves the last bit.
pub(crate) fn strip_shift(sums: &mut [f32], entering: &[f32], leaving: &[f32]) {
    debug_assert_eq!(sums.len(), entering.len());
    debug_assert_eq!(sums.len(), leaving.len());
    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY: `has_avx2()` asked the CPU once for this process; see
            // `avx2`'s doc for what that check promises.
            unsafe { avx2::strip_shift(sums, entering, leaving) }
        } else {
            strip_shift_fallback(sums, entering, leaving);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        strip_shift_fallback(sums, entering, leaving);
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        for i in 0..sums.len() {
            sums[i] += entering[i] - leaving[i];
        }
    }
}

/// The seed step's four-lane grouping — what every baseline has (SSE2, NEON)
/// and what the strip ran before it grew an AVX2 half — plus the scalar tail
/// a ragged strip edge leaves. The AVX2 half hands its own remainder here.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn strip_add_fallback(sums: &mut [f32], row: &[f32]) {
    let whole = sums.len() - sums.len() % 4;
    let mut i = 0;
    while i < whole {
        let next = lanes_add(
            <[f32; 4]>::try_from(&sums[i..i + 4]).unwrap(),
            <[f32; 4]>::try_from(&row[i..i + 4]).unwrap(),
        );
        sums[i..i + 4].copy_from_slice(&next);
        i += 4;
    }
    while i < sums.len() {
        sums[i] += row[i];
        i += 1;
    }
}

/// As `strip_add_fallback`, for the divided store.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn strip_store_fallback(sums: &[f32], out: &mut [f32], window: f32) {
    let whole = sums.len() - sums.len() % 4;
    let mut i = 0;
    while i < whole {
        let o = lanes_div_scalar(<[f32; 4]>::try_from(&sums[i..i + 4]).unwrap(), window);
        out[i..i + 4].copy_from_slice(&o);
        i += 4;
    }
    while i < sums.len() {
        out[i] = sums[i] / window;
        i += 1;
    }
}

/// As `strip_add_fallback`, for the window shift.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn strip_shift_fallback(sums: &mut [f32], entering: &[f32], leaving: &[f32]) {
    let whole = sums.len() - sums.len() % 4;
    let mut i = 0;
    while i < whole {
        let a = <[f32; 4]>::try_from(&entering[i..i + 4]).unwrap();
        let b = <[f32; 4]>::try_from(&leaving[i..i + 4]).unwrap();
        let next = lanes_add(
            <[f32; 4]>::try_from(&sums[i..i + 4]).unwrap(),
            lanes_sub(a, b),
        );
        sums[i..i + 4].copy_from_slice(&next);
        i += 4;
    }
    while i < sums.len() {
        sums[i] += entering[i] - leaving[i];
        i += 1;
    }
}

/// The scalar loop `composite_layer` ran for the normal blend, verbatim.
#[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), allow(dead_code))]
pub(crate) fn over_layer_row_scalar(
    dst: &mut [Premul],
    src: &[Premul],
    alpha: f32,
    opaque_copy: bool,
) {
    for (px, &src) in dst.iter_mut().zip(src) {
        let src = if alpha == 1.0 { src } else { src.scaled(alpha) };
        if src.a <= 0.0 {
            continue;
        }
        if opaque_copy && src.a == 1.0 {
            *px = src;
            continue;
        }
        *px = over(src, *px);
    }
}

/// The scalar loop `composite_flat` ran for the normal blend, verbatim.
#[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), allow(dead_code))]
pub(crate) fn over_flat_row_scalar(
    dst: &mut [Premul],
    mask: &[f32],
    color: Premul,
    replaces: bool,
) {
    for (px, &c) in dst.iter_mut().zip(mask) {
        if c <= 0.0 {
            continue;
        }
        if replaces && c >= 1.0 {
            *px = color;
            continue;
        }
        let src = color.scaled(c);
        if src.a == 1.0 {
            *px = src;
            continue;
        }
        *px = over(src, *px);
    }
}

/// The aarch64 sibling of the module below — see `neon`'s own doc for its
/// written-not-run status.
#[cfg(target_arch = "aarch64")]
mod neon;

/// # Why no runtime detection
///
/// SSE2 is part of the x86-64 baseline: every x86-64 CPU has it and rustc
/// enables it for every x86-64 target. The `target_feature` attribute is
/// there because Rust asks for it before treating the intrinsics as safe, not
/// because any machine could lack it.
#[cfg(target_arch = "x86_64")]
mod sse2 {
    use core::arch::x86_64::{
        __m128, _mm_add_ps, _mm_and_ps, _mm_andnot_ps, _mm_castsi128_ps, _mm_cmpeq_ps,
        _mm_cmple_ps, _mm_div_ps, _mm_loadu_ps, _mm_mul_ps, _mm_or_ps, _mm_set1_epi32,
        _mm_set1_ps, _mm_setzero_ps, _mm_shuffle_ps, _mm_storeu_ps, _mm_sub_ps,
    };

    use super::Premul;

    #[inline]
    #[target_feature(enable = "sse2")]
    fn load(p: &Premul) -> __m128 {
        // SAFETY: `Premul` is `repr(C)`, four `f32`s: 16 readable bytes.
        unsafe { _mm_loadu_ps(std::ptr::from_ref(p).cast::<f32>()) }
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    fn store(p: &mut Premul, v: __m128) {
        // SAFETY: as `load`, 16 writable bytes.
        unsafe { _mm_storeu_ps(std::ptr::from_mut(p).cast::<f32>(), v) }
    }

    /// `if mask { a } else { b }`, per lane.
    #[inline]
    #[target_feature(enable = "sse2")]
    fn select(mask: __m128, a: __m128, b: __m128) -> __m128 {
        _mm_or_ps(_mm_and_ps(mask, a), _mm_andnot_ps(mask, b))
    }

    /// A mask of all ones when `flag`, else all zeros.
    #[inline]
    #[target_feature(enable = "sse2")]
    fn flag(flag: bool) -> __m128 {
        _mm_castsi128_ps(_mm_set1_epi32(-i32::from(flag)))
    }

    /// `over(src, dst)`: `src * 1.0 + dst * (1.0 - src.a)`, where the
    /// multiply by one is an identity and is left out.
    #[inline]
    #[target_feature(enable = "sse2")]
    fn over(s: __m128, a: __m128, d: __m128) -> __m128 {
        _mm_add_ps(s, _mm_mul_ps(d, _mm_sub_ps(_mm_set1_ps(1.0), a)))
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn over_layer_row(
        dst: &mut [Premul],
        src: &[Premul],
        alpha: f32,
        opaque_copy: bool,
    ) {
        // Two loops rather than a per-pixel test of `alpha == 1.0`: the scalar
        // path skips the multiply at exactly one, and `x * 1.0` is an identity
        // anyway, so either loop gives the same bits — this one just does not
        // spend the multiply.
        if alpha == 1.0 {
            layer_loop::<false>(dst, src, _mm_set1_ps(1.0), opaque_copy);
        } else {
            layer_loop::<true>(dst, src, _mm_set1_ps(alpha), opaque_copy);
        }
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn over_layer_row_take(
        dst: &mut [Premul],
        src: &mut [Premul],
        alpha: f32,
        opaque_copy: bool,
    ) {
        let zero = _mm_setzero_ps();
        let one = _mm_set1_ps(1.0);
        let copy_ok = flag(opaque_copy);
        let k = _mm_set1_ps(alpha);
        let scale = alpha != 1.0;
        for (px, sp) in dst.iter_mut().zip(src) {
            let s = load(sp);
            store(sp, zero);
            let s = if scale { _mm_mul_ps(s, k) } else { s };
            let a = _mm_shuffle_ps::<0xFF>(s, s);
            let d = load(px);
            let skip = _mm_cmple_ps(a, zero);
            let copy = _mm_and_ps(_mm_cmpeq_ps(a, one), copy_ok);
            store(px, select(skip, d, select(copy, s, over(s, a, d))));
        }
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    fn layer_loop<const SCALE: bool>(
        dst: &mut [Premul],
        src: &[Premul],
        k: __m128,
        opaque_copy: bool,
    ) {
        let zero = _mm_setzero_ps();
        let one = _mm_set1_ps(1.0);
        let copy_ok = flag(opaque_copy);
        for (px, s) in dst.iter_mut().zip(src) {
            let s = if SCALE {
                _mm_mul_ps(load(s), k)
            } else {
                load(s)
            };
            let a = _mm_shuffle_ps::<0xFF>(s, s);
            let d = load(px);
            // Precedence as in the scalar loop: skip, then copy, then over.
            let skip = _mm_cmple_ps(a, zero);
            let copy = _mm_and_ps(_mm_cmpeq_ps(a, one), copy_ok);
            let out = select(skip, d, select(copy, s, over(s, a, d)));
            store(px, out);
        }
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    fn load4(a: &[f32; 4]) -> __m128 {
        // SAFETY: `[f32; 4]` is four readable `f32`s: 16 readable bytes.
        unsafe { _mm_loadu_ps(a.as_ptr()) }
    }

    #[inline]
    #[target_feature(enable = "sse2")]
    fn store4(a: &mut [f32; 4], v: __m128) {
        // SAFETY: as `load4`, 16 writable bytes.
        unsafe { _mm_storeu_ps(a.as_mut_ptr(), v) }
    }

    /// A `__m128` back into a `Premul` by value — the mirror of `store` for
    /// the kernels that *return* pixels rather than writing rows of them.
    #[inline]
    #[target_feature(enable = "sse2")]
    fn put(v: __m128) -> Premul {
        // SAFETY: `__m128` is four `f32`s; `Premul` is `repr(C)`, four
        // `f32`s: 16 writable bytes in a value being returned.
        let mut out = Premul {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        store(&mut out, v);
        out
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn lanes_add(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0; 4];
        store4(&mut out, _mm_add_ps(load4(&a), load4(&b)));
        out
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn lanes_sub(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0; 4];
        store4(&mut out, _mm_sub_ps(load4(&a), load4(&b)));
        out
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn lanes_div_scalar(a: [f32; 4], k: f32) -> [f32; 4] {
        let mut out = [0.0; 4];
        store4(&mut out, _mm_div_ps(load4(&a), _mm_set1_ps(k)));
        out
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn premul_add(a: Premul, b: Premul) -> Premul {
        put(_mm_add_ps(load(&a), load(&b)))
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn premul_sub(a: Premul, b: Premul) -> Premul {
        put(_mm_sub_ps(load(&a), load(&b)))
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn premul_div(p: Premul, k: f32) -> Premul {
        put(_mm_div_ps(load(&p), _mm_set1_ps(k)))
    }

    #[target_feature(enable = "sse2")]
    pub(super) fn over_flat_row(dst: &mut [Premul], mask: &[f32], color: Premul, replaces: bool) {
        let one = _mm_set1_ps(1.0);
        let col = load(&color);
        let replace_all = replaces;
        for (px, &c) in dst.iter_mut().zip(mask) {
            // **The two cheap cases stay branches.** No coverage, and an
            // opaque colour at full coverage, are long runs — outside a shape,
            // and the interior of every solid fill — so the branches predict
            // almost perfectly and each costs a compare and at most a store.
            // Folding them into the selects below, as `layer_loop` does,
            // measured worse: `23-editor-glass` (mostly opaque panels) went
            // from 1,301 M to 1,431 M instructions, because every interior
            // pixel then paid the full blend to compute a value it discarded.
            if c <= 0.0 {
                continue;
            }
            if replace_all && c >= 1.0 {
                *px = color;
                continue;
            }
            let cv = _mm_set1_ps(c);
            let s = _mm_mul_ps(col, cv);
            let a = _mm_shuffle_ps::<0xFF>(s, s);
            let d = load(px);
            // An opaque scaled source replaces; anything else is `over`.
            let copy = _mm_cmpeq_ps(a, one);
            store(px, select(copy, s, over(s, a, d)));
        }
    }
}

/// # Why runtime detection
///
/// AVX2 is not part of the x86-64 baseline the way SSE2 is: it arrived with
/// Haswell in 2013, and the machine this renderer's budget is measured on
/// (the reference laptop, a Kaby Lake i5-7200U) has it, but the crate
/// compiles once for every x86-64 machine and the baseline stays SSE2 — a
/// pre-Haswell CPU, or a VM masking CPUID, still runs, on the `sse2` half,
/// same bytes. There is no build-time negotiation (`-C target-feature` opts
/// the whole binary out of exactly those machines); `has_avx2` above asks
/// the CPU once per process and the dispatcher fns branch on the answer.
///
/// What the check promises is only that the instructions will not fault —
/// `is_x86_feature_detected!` reads CPUID and the xsave state the OS has
/// enabled. It does not promise the wider ops are *faster*, though each
/// kernel here issues the same instruction count as its SSE2 half at twice
/// the width, so there is nothing to lose but the branch, and the branch is
/// paid once per row or per strip, never per pixel.
///
/// # Why it is the same bytes
///
/// Each kernel is its SSE2 half with two of that half's 128-bit groups in
/// one 256-bit register: two adjacent pixels for the row kernels (a `Premul`
/// is 16 bytes, so a pair is exactly one `__m256`), eight adjacent columns
/// for the strip steps. That changes the *grouping*, never the arithmetic:
/// an 8-lane add, subtract, multiply or divide is two of the 4-lane ones
/// side by side — IEEE 754 rounds every lane on its own, no carry crosses a
/// lane, nothing is reassociated, nothing fused — so lane `i` of the
/// 256-bit op is the same operation on the same inputs the SSE2 half ran on
/// its own lane `i`. The compares and selects are per lane the same way
/// (`vcmpps` answers the SSE2 comparison's question, `NaN` false for the
/// ordered ones exactly as the scalar `<=` and `==`; `vblendvps` is the
/// bitwise select the SSE2 `select` composes from and/andnot/or), and
/// `_mm256_shuffle_ps::<0xFF>` broadcasts each pixel's own alpha across its
/// own half — the 128-bit shuffle's trick twice in one instruction. A row's
/// pixels are independent of each other and a strip's columns keep their
/// own sums, so which of them share a register cannot reach the result.
///
/// The two ragged edges — a row of odd length, and the strip columns past
/// the last eight-group — run through the SSE2 half and the four-lane
/// fallback: one pixel or a handful of columns, the same arithmetic one
/// width narrower. The `NaN` caveat is the `sse2` half's, unchanged: which
/// `NaN` a `NaN` comes out as may differ, nothing can observe it, and
/// `simd_rows_match_scalar_bit_for_bit` compares all three halves — this
/// one, `sse2`, and the scalar reference — with every `NaN` one value.
#[cfg(target_arch = "x86_64")]
mod avx2 {
    use core::arch::x86_64::{
        __m128, __m256, _mm256_add_ps, _mm256_and_ps, _mm256_blendv_ps, _mm256_castps128_ps256,
        _mm256_castsi256_ps, _mm256_cmp_ps, _mm256_div_ps, _mm256_insertf128_ps, _mm256_loadu_ps,
        _mm256_mul_ps, _mm256_set1_epi32, _mm256_set1_ps, _mm256_setzero_ps, _mm256_shuffle_ps,
        _mm256_storeu_ps, _mm256_sub_ps, _mm_loadu_ps, _mm_set1_ps, _CMP_EQ_OQ, _CMP_LE_OQ,
    };

    use super::Premul;

    /// Two adjacent `Premul`s — the 32 bytes one `__m256` holds. The array
    /// type is the bound: two `Premul`s are 32 readable bytes by
    /// construction, not by call-site discipline.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn load2(p: &[Premul; 2]) -> __m256 {
        // SAFETY: `[Premul; 2]` is 32 readable bytes.
        unsafe { _mm256_loadu_ps(p.as_ptr().cast::<f32>()) }
    }

    /// As `load2`, writing.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn store2(p: &mut [Premul; 2], v: __m256) {
        // SAFETY: as `load2`, 32 writable bytes.
        unsafe { _mm256_storeu_ps(p.as_mut_ptr().cast::<f32>(), v) }
    }

    /// One `Premul`, for the constants hoisted out of the loops.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn load1(p: &Premul) -> __m128 {
        // SAFETY: `Premul` is `repr(C)`, four `f32`s: 16 readable bytes.
        unsafe { _mm_loadu_ps(std::ptr::from_ref(p).cast::<f32>()) }
    }

    /// `if mask { a } else { b }`, per lane — one `vblendvps`, the single
    /// instruction the SSE2 half's three-op and/andnot/or `select` was
    /// always composed of; the bits it picks are the bits those three pick.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn select(mask: __m256, a: __m256, b: __m256) -> __m256 {
        _mm256_blendv_ps(b, a, mask)
    }

    /// A mask of all ones when `flag`, else all zeros.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn flag(flag: bool) -> __m256 {
        _mm256_castsi256_ps(_mm256_set1_epi32(-i32::from(flag)))
    }

    /// `a <= b` per lane, `NaN` false exactly as the scalar `<=` — the SSE2
    /// half's `_mm_cmple_ps`, via the one 256-bit compare intrinsic rustc
    /// exposes.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn cmple(a: __m256, b: __m256) -> __m256 {
        // SAFETY: a register compare; this context already enables the
        // feature the intrinsic asks for.
        _mm256_cmp_ps::<_CMP_LE_OQ>(a, b)
    }

    /// `a == b` per lane, `NaN` false exactly as the scalar `==`.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn cmpeq(a: __m256, b: __m256) -> __m256 {
        // SAFETY: as `cmple`.
        _mm256_cmp_ps::<_CMP_EQ_OQ>(a, b)
    }

    /// Two scalars, each broadcast across its own 128-bit half — the per-pixel
    /// coverage a pair shares one register with: `[lo; 4]` below, `[hi; 4]`
    /// above.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn each_half(lo: f32, hi: f32) -> __m256 {
        _mm256_insertf128_ps(_mm256_castps128_ps256(_mm_set1_ps(lo)), _mm_set1_ps(hi), 1)
    }

    /// `over(src, dst)`: `src * 1.0 + dst * (1.0 - src.a)`, where the
    /// multiply by one is an identity and is left out.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn over(s: __m256, a: __m256, d: __m256) -> __m256 {
        _mm256_add_ps(s, _mm256_mul_ps(d, _mm256_sub_ps(_mm256_set1_ps(1.0), a)))
    }

    /// The longest even length that fits both slices: everything before it
    /// pairs, everything after it is a one-pixel tail.
    #[inline]
    fn even_prefix(a: usize, b: usize) -> usize {
        let n = a.min(b);
        n - n % 2
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn over_layer_row(
        dst: &mut [Premul],
        src: &[Premul],
        alpha: f32,
        opaque_copy: bool,
    ) {
        // The odd tail pixel — most rows have none — cannot pair and runs
        // through the SSE2 half, same arithmetic one pixel wide.
        let whole = even_prefix(dst.len(), src.len());
        let (body, tail) = dst.split_at_mut(whole);
        // Two loops rather than a per-pair test of `alpha == 1.0`, as the
        // SSE2 half: the scalar path skips the multiply at exactly one, and
        // `x * 1.0` is an identity anyway, so either loop gives the same
        // bits — this one just does not spend the multiply.
        if alpha == 1.0 {
            layer_loop::<false>(body, &src[..whole], _mm256_set1_ps(1.0), opaque_copy);
        } else {
            layer_loop::<true>(body, &src[..whole], _mm256_set1_ps(alpha), opaque_copy);
        }
        if !tail.is_empty() {
            // The SSE2 half for the unpaired pixel — no `unsafe` needed:
            // AVX2 implies SSE2, so this context already guarantees it.
            super::sse2::over_layer_row(tail, &src[whole..], alpha, opaque_copy);
        }
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn over_layer_row_take(
        dst: &mut [Premul],
        src: &mut [Premul],
        alpha: f32,
        opaque_copy: bool,
    ) {
        let whole = even_prefix(dst.len(), src.len());
        let (body, tail) = dst.split_at_mut(whole);
        let (src_body, src_tail) = src.split_at_mut(whole);
        let zero = _mm256_setzero_ps();
        let one = _mm256_set1_ps(1.0);
        let copy_ok = flag(opaque_copy);
        let k = _mm256_set1_ps(alpha);
        let scale = alpha != 1.0;
        for (px, sp) in body
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(src_body.as_chunks_mut::<2>().0.iter_mut())
        {
            let s = load2(sp);
            store2(sp, zero);
            let s = if scale { _mm256_mul_ps(s, k) } else { s };
            let a = _mm256_shuffle_ps::<0xFF>(s, s);
            let d = load2(px);
            let skip = cmple(a, zero);
            let copy = _mm256_and_ps(cmpeq(a, one), copy_ok);
            store2(px, select(skip, d, select(copy, s, over(s, a, d))));
        }
        if !tail.is_empty() {
            // The SSE2 half for the unpaired pixel — no `unsafe` needed:
            // AVX2 implies SSE2, so this context already guarantees it.
            super::sse2::over_layer_row_take(tail, src_tail, alpha, opaque_copy);
        }
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn layer_loop<const SCALE: bool>(
        dst: &mut [Premul],
        src: &[Premul],
        k: __m256,
        opaque_copy: bool,
    ) {
        let zero = _mm256_setzero_ps();
        let one = _mm256_set1_ps(1.0);
        let copy_ok = flag(opaque_copy);
        for (px, s) in dst
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(src.as_chunks::<2>().0.iter())
        {
            let s = if SCALE {
                _mm256_mul_ps(load2(s), k)
            } else {
                load2(s)
            };
            // Each half's own alpha: the shuffle duplicates lane 3 of each
            // 128-bit half across that half, pixel 0's alpha below, pixel
            // 1's above.
            let a = _mm256_shuffle_ps::<0xFF>(s, s);
            let d = load2(px);
            // Precedence as in the scalar loop: skip, then copy, then over.
            let skip = cmple(a, zero);
            let copy = _mm256_and_ps(cmpeq(a, one), copy_ok);
            store2(px, select(skip, d, select(copy, s, over(s, a, d))));
        }
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn over_flat_row(dst: &mut [Premul], mask: &[f32], color: Premul, replaces: bool) {
        let whole = even_prefix(dst.len(), mask.len());
        let (body, tail) = dst.split_at_mut(whole);
        let one = _mm256_set1_ps(1.0);
        let zero = _mm256_setzero_ps();
        let c = load1(&color);
        let col = _mm256_insertf128_ps(_mm256_castps128_ps256(c), c, 1);
        let replace_ok = flag(replaces);
        for (px, m) in body
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(mask[..whole].as_chunks::<2>().0.iter())
        {
            // **The two cheap cases stay branches, now a pair at a time.**
            // No coverage and a full-coverage opaque colour are long runs —
            // outside a shape, and the interior of every solid fill — so
            // whole *pairs* take the same two branches the SSE2 half gives
            // single pixels, for the same measured reason (see there). Only
            // a mixed pair falls through, and a mixed pair is two edge
            // pixels: the fringe, where the blend is the work anyway.
            let (c0, c1) = (m[0], m[1]);
            if c0 <= 0.0 && c1 <= 0.0 {
                continue;
            }
            if replaces && c0 >= 1.0 && c1 >= 1.0 {
                store2(px, col);
                continue;
            }
            let cv = each_half(c0, c1);
            let s = _mm256_mul_ps(col, cv);
            let a = _mm256_shuffle_ps::<0xFF>(s, s);
            let d = load2(px);
            // Precedence as in the scalar loop: skip, then replace, then
            // copy, then over. A skipped lane is written back with the value
            // it was loaded with — the same bits as not writing it.
            let skip = cmple(cv, zero);
            let replace = _mm256_and_ps(cmple(one, cv), replace_ok);
            let copy = cmpeq(a, one);
            store2(
                px,
                select(
                    skip,
                    d,
                    select(replace, col, select(copy, s, over(s, a, d))),
                ),
            );
        }
        if !tail.is_empty() {
            // The SSE2 half for the unpaired pixel — no `unsafe` needed:
            // AVX2 implies SSE2, so this context already guarantees it.
            super::sse2::over_flat_row(tail, &mask[whole..], color, replaces);
        }
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn strip_add(sums: &mut [f32], row: &[f32]) {
        let whole = sums.len().min(row.len());
        let whole = whole - whole % 8;
        for (s, r) in sums[..whole]
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .zip(row[..whole].as_chunks::<8>().0.iter())
        {
            // SAFETY: eight readable `f32`s on each side, `s` writable —
            // the chunk is exactly that.
            unsafe {
                let v = _mm256_add_ps(_mm256_loadu_ps(s.as_ptr()), _mm256_loadu_ps(r.as_ptr()));
                _mm256_storeu_ps(s.as_mut_ptr(), v);
            }
        }
        super::strip_add_fallback(&mut sums[whole..], &row[whole..]);
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn strip_store(sums: &[f32], out: &mut [f32], window: f32) {
        let k = _mm256_set1_ps(window);
        let whole = sums.len().min(out.len());
        let whole = whole - whole % 8;
        for (s, o) in sums[..whole]
            .as_chunks::<8>()
            .0
            .iter()
            .zip(out[..whole].as_chunks_mut::<8>().0.iter_mut())
        {
            // SAFETY: as `strip_add`, with the store on `o`.
            unsafe {
                _mm256_storeu_ps(
                    o.as_mut_ptr(),
                    _mm256_div_ps(_mm256_loadu_ps(s.as_ptr()), k),
                );
            }
        }
        super::strip_store_fallback(&sums[whole..], &mut out[whole..], window);
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn strip_shift(sums: &mut [f32], entering: &[f32], leaving: &[f32]) {
        let whole = sums.len().min(entering.len()).min(leaving.len());
        let whole = whole - whole % 8;
        for ((s, e), l) in sums[..whole]
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .zip(entering[..whole].as_chunks::<8>().0.iter())
            .zip(leaving[..whole].as_chunks::<8>().0.iter())
        {
            // SAFETY: as `strip_add`, over three slices.
            unsafe {
                let step = _mm256_sub_ps(_mm256_loadu_ps(e.as_ptr()), _mm256_loadu_ps(l.as_ptr()));
                let v = _mm256_add_ps(_mm256_loadu_ps(s.as_ptr()), step);
                _mm256_storeu_ps(s.as_mut_ptr(), v);
            }
        }
        super::strip_shift_fallback(&mut sums[whole..], &entering[whole..], &leaving[whole..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Raw bits, except that every `NaN` is one `NaN` — see the module doc's
    /// note on `NaN` sign and payload.
    fn bits(row: &[Premul]) -> Vec<[u32; 4]> {
        let canon = |v: f32| {
            if v.is_nan() {
                f32::NAN.to_bits()
            } else {
                v.to_bits()
            }
        };
        row.iter()
            .map(|p| [canon(p.r), canon(p.g), canon(p.b), canon(p.a)])
            .collect()
    }

    const EDGES: [f32; 16] = [
        0.0,
        -0.0,
        1.0,
        0.5,
        0.97,
        1.0 / 255.0,
        0.999_999,
        1.000_001,
        1.5,
        -0.25,
        f32::MIN_POSITIVE / 8.0,
        f32::EPSILON,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];

    /// xorshift, so the sweep is deterministic and needs no dependency.
    struct Rng(u64);
    impl Rng {
        fn f(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            // Biased toward the values the branches test: 0, 1, and between.
            match self.0 % 8 {
                0 => 0.0,
                1 => 1.0,
                _ => (self.0 >> 40) as f32 / (1u64 << 24) as f32 * 1.3 - 0.15,
            }
        }
        fn premul(&mut self) -> Premul {
            let a = self.f();
            Premul {
                r: self.f() * a.abs(),
                g: self.f() * a.abs(),
                b: self.f() * a.abs(),
                a,
            }
        }
    }

    fn pixels(rng: &mut Rng, n: usize) -> Vec<Premul> {
        let mut out: Vec<Premul> = EDGES
            .iter()
            .flat_map(|&a| {
                EDGES.iter().map(move |&c| Premul {
                    r: c,
                    g: c * 0.5,
                    b: 1.0 - c,
                    a,
                })
            })
            .collect();
        out.extend((0..n).map(|_| rng.premul()));
        out
    }

    #[test]
    fn simd_rows_match_scalar_bit_for_bit() {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        let dst = pixels(&mut rng, 200_000);
        let src = pixels(&mut rng, 200_000);
        let mask: Vec<f32> = EDGES
            .iter()
            .copied()
            .cycle()
            .take(EDGES.len() * EDGES.len())
            .chain((0..200_000).map(|_| rng.f()))
            .collect();
        assert_eq!(mask.len(), dst.len());

        for alpha in [1.0, 0.97, 0.5, 0.0, 1.5, f32::NAN] {
            for opaque_copy in [true, false] {
                let (mut a, mut b) = (dst.clone(), dst.clone());
                over_layer_row(&mut a, &src, alpha, opaque_copy);
                over_layer_row_scalar(&mut b, &src, alpha, opaque_copy);
                assert!(
                    bits(&a) == bits(&b),
                    "layer row: alpha {alpha}, opaque_copy {opaque_copy}"
                );
                // The SSE2 half beside the dispatched row (the AVX2 half on a
                // machine that reports it) and against the scalar reference:
                // whichever width ran, its bytes are the SSE2 bytes are the
                // scalar bytes.
                #[cfg(target_arch = "x86_64")]
                {
                    let mut c = dst.clone();
                    // SAFETY: SSE2 is part of the x86-64 baseline.
                    unsafe { sse2::over_layer_row(&mut c, &src, alpha, opaque_copy) };
                    assert!(
                        bits(&a) == bits(&c),
                        "layer row, dispatched vs sse2: alpha {alpha}, opaque_copy {opaque_copy}"
                    );
                    assert!(
                        bits(&c) == bits(&b),
                        "layer row, sse2 vs scalar: alpha {alpha}, opaque_copy {opaque_copy}"
                    );
                }
                // `over_layer_row_take` is the same blend plus the zeroing
                // store: held to the same comparisons, and to leaving `src`
                // fully transparent.
                let (mut a, mut b, mut src_a) = (dst.clone(), dst.clone(), src.clone());
                over_layer_row_take(&mut a, &mut src_a, alpha, opaque_copy);
                over_layer_row_scalar(&mut b, &src, alpha, opaque_copy);
                assert!(
                    bits(&a) == bits(&b),
                    "layer take row: alpha {alpha}, opaque_copy {opaque_copy}"
                );
                assert!(
                    bits(&src_a).iter().all(|&c| c == [0, 0, 0, 0]),
                    "layer take row leaves src zeroed: alpha {alpha}, opaque_copy {opaque_copy}"
                );
                #[cfg(target_arch = "x86_64")]
                {
                    let (mut c, mut src_c) = (dst.clone(), src.clone());
                    // SAFETY: SSE2 is part of the x86-64 baseline.
                    unsafe { sse2::over_layer_row_take(&mut c, &mut src_c, alpha, opaque_copy) };
                    assert!(
                        bits(&a) == bits(&c),
                        "layer take row, dispatched vs sse2: alpha {alpha}, opaque_copy {opaque_copy}"
                    );
                    assert!(
                        bits(&c) == bits(&b),
                        "layer take row, sse2 vs scalar: alpha {alpha}, opaque_copy {opaque_copy}"
                    );
                }
            }
        }
        let colors = [
            Premul {
                r: 0.2,
                g: 0.4,
                b: 0.6,
                a: 1.0,
            },
            Premul {
                r: 0.1,
                g: 0.05,
                b: 0.0,
                a: 0.25,
            },
            Premul {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            Premul {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.078,
            },
        ];
        for color in colors {
            for replaces in [true, false] {
                let (mut a, mut b) = (dst.clone(), dst.clone());
                over_flat_row(&mut a, &mask, color, replaces);
                over_flat_row_scalar(&mut b, &mask, color, replaces);
                assert!(
                    bits(&a) == bits(&b),
                    "flat row: {color:?}, replaces {replaces}"
                );
                #[cfg(target_arch = "x86_64")]
                {
                    let mut c = dst.clone();
                    // SAFETY: SSE2 is part of the x86-64 baseline.
                    unsafe { sse2::over_flat_row(&mut c, &mask, color, replaces) };
                    assert!(
                        bits(&a) == bits(&c),
                        "flat row, dispatched vs sse2: {color:?}, replaces {replaces}"
                    );
                    assert!(
                        bits(&c) == bits(&b),
                        "flat row, sse2 vs scalar: {color:?}, replaces {replaces}"
                    );
                }
            }
        }

        // Odd and short rows: the AVX2 halves pair pixels and hand the
        // unpaired tail to the SSE2 half, so every length that leaves a tail
        // is compared the same three ways — the tail is a path, not an
        // afterthought, and this is where its own arithmetic is checked.
        for len in [0, 1, 2, 3, 5, 9, dst.len() - 1] {
            let (d, s, m) = (&dst[..len], &src[..len], &mask[..len]);
            for alpha in [1.0, 0.5, f32::NAN] {
                let (mut a, mut b) = (d.to_vec(), d.to_vec());
                over_layer_row(&mut a, s, alpha, true);
                over_layer_row_scalar(&mut b, s, alpha, true);
                assert!(bits(&a) == bits(&b), "layer tail: len {len}, alpha {alpha}");
                let (mut a, mut b, mut sa) = (d.to_vec(), d.to_vec(), s.to_vec());
                over_layer_row_take(&mut a, &mut sa, alpha, true);
                over_layer_row_scalar(&mut b, s, alpha, true);
                assert!(
                    bits(&a) == bits(&b),
                    "layer take tail: len {len}, alpha {alpha}"
                );
                assert!(
                    bits(&sa).iter().all(|&c| c == [0, 0, 0, 0]),
                    "layer take tail leaves src zeroed: len {len}, alpha {alpha}"
                );
                #[cfg(target_arch = "x86_64")]
                {
                    let mut c = d.to_vec();
                    // SAFETY: SSE2 is part of the x86-64 baseline.
                    unsafe { sse2::over_layer_row(&mut c, s, alpha, true) };
                    assert!(
                        bits(&a) == bits(&c),
                        "layer tail, dispatched vs sse2: len {len}, alpha {alpha}"
                    );
                    let (mut c, mut sc) = (d.to_vec(), s.to_vec());
                    // SAFETY: SSE2 is part of the x86-64 baseline.
                    unsafe { sse2::over_layer_row_take(&mut c, &mut sc, alpha, true) };
                    assert!(
                        bits(&a) == bits(&c),
                        "layer take tail, dispatched vs sse2: len {len}, alpha {alpha}"
                    );
                }
            }
            for color in colors {
                for replaces in [true, false] {
                    let (mut a, mut b) = (d.to_vec(), d.to_vec());
                    over_flat_row(&mut a, m, color, replaces);
                    over_flat_row_scalar(&mut b, m, color, replaces);
                    assert!(
                        bits(&a) == bits(&b),
                        "flat tail: len {len}, replaces {replaces}"
                    );
                    #[cfg(target_arch = "x86_64")]
                    {
                        let mut c = d.to_vec();
                        // SAFETY: SSE2 is part of the x86-64 baseline.
                        unsafe { sse2::over_flat_row(&mut c, m, color, replaces) };
                        assert!(
                            bits(&a) == bits(&c),
                            "flat tail, dispatched vs sse2: len {len}, replaces {replaces}"
                        );
                    }
                }
            }
        }
    }

    /// The lane ops are each the scalar op, lane for lane — the whole
    /// byte-identity argument for the blur and fill callers rests on it, so
    /// they get their own sweep over the same hostile edges the row test
    /// uses, plus the div-by-scalar that must stay a divide. The strip steps
    /// carry the same argument one level up, for the shadow blur's caller,
    /// so they sweep here too: dispatched (the AVX2 half on a machine that
    /// reports it) against the scalar op per column, and — where the
    /// architecture has the four-lane fallback — that against both.
    #[test]
    fn lane_ops_match_scalar_bit_for_bit() {
        let canon = |v: f32| {
            if v.is_nan() {
                f32::NAN.to_bits()
            } else {
                v.to_bits()
            }
        };
        let lanes = |v: [f32; 4]| v.map(canon);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..100_000 {
            let a: [f32; 4] = core::array::from_fn(|_| rng.f());
            let b: [f32; 4] = core::array::from_fn(|_| rng.f());
            let k = rng.f();
            assert_eq!(
                lanes(lanes_add(a, b)),
                lanes([a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]),
                "lanes_add {a:?} {b:?}"
            );
            assert_eq!(
                lanes(lanes_sub(a, b)),
                lanes([a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]),
                "lanes_sub {a:?} {b:?}"
            );
            assert_eq!(
                lanes(lanes_div_scalar(a, k)),
                lanes([a[0] / k, a[1] / k, a[2] / k, a[3] / k]),
                "lanes_div_scalar {a:?} {k:?}"
            );
            let (pa, pb) = (rng.premul(), rng.premul());
            let scalar_add = Premul {
                r: pa.r + pb.r,
                g: pa.g + pb.g,
                b: pa.b + pb.b,
                a: pa.a + pb.a,
            };
            let scalar_sub = Premul {
                r: pa.r - pb.r,
                g: pa.g - pb.g,
                b: pa.b - pb.b,
                a: pa.a - pb.a,
            };
            let scalar_div = Premul {
                r: pa.r / k,
                g: pa.g / k,
                b: pa.b / k,
                a: pa.a / k,
            };
            assert_eq!(
                bits(&[premul_add(pa, pb)]),
                bits(&[scalar_add]),
                "premul_add"
            );
            assert_eq!(
                bits(&[premul_sub(pa, pb)]),
                bits(&[scalar_sub]),
                "premul_sub"
            );
            assert_eq!(
                bits(&[premul_div(pa, k)]),
                bits(&[scalar_div]),
                "premul_div"
            );
        }

        // The strip steps, over every length a strip's edge can leave so each
        // grouping's own boundary is compared, not just reached: dispatched
        // against the scalar op per column, and the four-lane fallback
        // against both.
        let fbits = |v: &[f32]| v.iter().map(|&x| canon(x)).collect::<Vec<_>>();
        for len in 0..=17usize {
            // Each field is the row test's mix: hostile edges first, the
            // biased random filling the rest.
            let field = |rng: &mut Rng| -> Vec<f32> {
                let n_edges = len / 2;
                EDGES
                    .iter()
                    .copied()
                    .cycle()
                    .take(n_edges)
                    .chain((n_edges..len).map(|_| rng.f()))
                    .collect()
            };
            let (sums, row, entering, leaving) = (
                field(&mut rng),
                field(&mut rng),
                field(&mut rng),
                field(&mut rng),
            );
            let window = rng.f();
            let scalar_add: Vec<f32> = sums.iter().zip(&row).map(|(&s, &r)| s + r).collect();
            let scalar_store: Vec<f32> = sums.iter().map(|&s| s / window).collect();
            let scalar_shift: Vec<f32> = sums
                .iter()
                .zip(&entering)
                .zip(&leaving)
                .map(|((&s, &e), &l)| s + (e - l))
                .collect();

            let (mut a, mut b) = (sums.clone(), sums.clone());
            strip_add(&mut a, &row);
            strip_shift(&mut b, &entering, &leaving);
            let mut stored = vec![0.0; len];
            strip_store(&sums, &mut stored, window);
            assert_eq!(fbits(&a), fbits(&scalar_add), "strip_add len {len}");
            assert_eq!(fbits(&b), fbits(&scalar_shift), "strip_shift len {len}");
            assert_eq!(
                fbits(&stored),
                fbits(&scalar_store),
                "strip_store len {len}"
            );

            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            {
                // The four-lane fallback beside the dispatched path and the
                // scalar: dispatched-vs-fallback is the AVX2-vs-SSE2
                // comparison on a machine that reports AVX2, and
                // fallback-vs-scalar is the SSE2-vs-scalar one.
                let (mut c, mut d) = (sums.clone(), sums.clone());
                strip_add_fallback(&mut c, &row);
                strip_shift_fallback(&mut d, &entering, &leaving);
                let mut stored4 = vec![0.0; len];
                strip_store_fallback(&sums, &mut stored4, window);
                assert_eq!(
                    fbits(&c),
                    fbits(&scalar_add),
                    "strip_add fallback len {len}"
                );
                assert_eq!(
                    fbits(&d),
                    fbits(&scalar_shift),
                    "strip_shift fallback len {len}"
                );
                assert_eq!(
                    fbits(&stored4),
                    fbits(&scalar_store),
                    "strip_store fallback len {len}"
                );
                assert_eq!(
                    fbits(&a),
                    fbits(&c),
                    "strip_add, dispatched vs fallback len {len}"
                );
                assert_eq!(
                    fbits(&b),
                    fbits(&d),
                    "strip_shift, dispatched vs fallback len {len}"
                );
                assert_eq!(
                    fbits(&stored),
                    fbits(&stored4),
                    "strip_store, dispatched vs fallback len {len}"
                );
            }
        }
    }
}
