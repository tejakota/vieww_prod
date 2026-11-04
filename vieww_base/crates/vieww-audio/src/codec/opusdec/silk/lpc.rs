//! NLSF decoding and conversion to LPC coefficients, and the LPC tools
//! around it (`NLSF_decode.c`, `NLSF_unpack.c`, `NLSF_stabilize.c`,
//! `NLSF2A.c`, `LPC_fit.c`, `LPC_inv_pred_gain.c`, `bwexpander*.c`,
//! `LPC_analysis_filter.c`, `sum_sqr_shift.c`).

use super::fix::{
    clz32, inverse32_varq, rshift_round, rshift_round64, sat16, smlabb, smlawb, smmul, smulbb,
    smull, smulww,
};
use super::tables as t;

/// One NLSF codebook (`silk_NLSF_CB_struct`, decoder fields).
pub(crate) struct NlsfCb {
    pub(crate) order: usize,
    pub(crate) quant_step_q16: i32,
    pub(crate) cb1_nlsf_q8: &'static [u8],
    pub(crate) cb1_wght_q9: &'static [i16],
    pub(crate) cb1_icdf: &'static [u8],
    pub(crate) pred_q8: &'static [u8],
    pub(crate) ec_sel: &'static [u8],
    pub(crate) ec_icdf: &'static [u8],
    pub(crate) delta_min_q15: &'static [i16],
}

pub(crate) const NB_VECTORS: usize = t::NLSF_NVECTORS;

pub(crate) static CB_NB_MB: NlsfCb = NlsfCb {
    order: 10,
    quant_step_q16: t::NLSF_QSTEP_NB_MB_Q16,
    cb1_nlsf_q8: &t::NLSF_CB1_NB_MB_Q8,
    cb1_wght_q9: &t::NLSF_CB1_WGHT_NB_MB_Q9,
    cb1_icdf: &t::NLSF_CB1_ICDF_NB_MB,
    pred_q8: &t::NLSF_PRED_NB_MB_Q8,
    ec_sel: &t::NLSF_CB2_SELECT_NB_MB,
    ec_icdf: &t::NLSF_CB2_ICDF_NB_MB,
    delta_min_q15: &t::NLSF_DELTA_MIN_NB_MB_Q15,
};

pub(crate) static CB_WB: NlsfCb = NlsfCb {
    order: 16,
    quant_step_q16: t::NLSF_QSTEP_WB_Q16,
    cb1_nlsf_q8: &t::NLSF_CB1_WB_Q8,
    cb1_wght_q9: &t::NLSF_CB1_WGHT_WB_Q9,
    cb1_icdf: &t::NLSF_CB1_ICDF_WB,
    pred_q8: &t::NLSF_PRED_WB_Q8,
    ec_sel: &t::NLSF_CB2_SELECT_WB,
    ec_icdf: &t::NLSF_CB2_ICDF_WB,
    delta_min_q15: &t::NLSF_DELTA_MIN_WB_Q15,
};

/// `silk_NLSF_unpack`: the entropy-table offsets and predictor weights
/// codebook vector `cb1` selects.
pub(crate) fn nlsf_unpack(ec_ix: &mut [i16; 16], pred_q8: &mut [u8; 16], cb: &NlsfCb, cb1: usize) {
    let order = cb.order;
    let sel = &cb.ec_sel[cb1 * order / 2..];
    for i in (0..order).step_by(2) {
        let entry = i32::from(sel[i / 2]);
        ec_ix[i] = smulbb((entry >> 1) & 7, 9) as i16;
        pred_q8[i] = cb.pred_q8[i + (entry & 1) as usize * (order - 1)];
        ec_ix[i + 1] = smulbb((entry >> 5) & 7, 9) as i16;
        pred_q8[i + 1] = cb.pred_q8[i + ((entry >> 4) & 1) as usize * (order - 1) + 1];
    }
}

/// `silk_NLSF_decode`: indices to stabilised NLSFs (Q15).
pub(crate) fn nlsf_decode(nlsf_q15: &mut [i16; 16], indices: &[i8; 17], cb: &NlsfCb) {
    let mut ec_ix = [0i16; 16];
    let mut pred_q8 = [0u8; 16];
    let cb1 = indices[0] as usize;
    nlsf_unpack(&mut ec_ix, &mut pred_q8, cb, cb1);
    let order = cb.order;
    // silk_NLSF_residual_dequant
    let mut res_q10 = [0i16; 16];
    let mut out_q10: i32 = 0;
    for i in (0..order).rev() {
        let pred_q10 = smulbb(out_q10, i32::from(pred_q8[i])) >> 8;
        out_q10 = i32::from(indices[i + 1]) << 10;
        if out_q10 > 0 {
            out_q10 -= 102;
        } else if out_q10 < 0 {
            out_q10 += 102;
        }
        out_q10 = smlawb(pred_q10, out_q10, cb.quant_step_q16);
        res_q10[i] = out_q10 as i16;
    }
    let el = &cb.cb1_nlsf_q8[cb1 * order..];
    let w = &cb.cb1_wght_q9[cb1 * order..];
    for i in 0..order {
        let tmp =
            ((i32::from(res_q10[i]) << 14) / i32::from(w[i])).wrapping_add(i32::from(el[i]) << 7);
        nlsf_q15[i] = tmp.clamp(0, 32767) as i16;
    }
    nlsf_stabilize(&mut nlsf_q15[..order], cb.delta_min_q15);
}

/// `silk_NLSF_stabilize`: enforce the codebook's minimum spacing.
pub(crate) fn nlsf_stabilize(nlsf: &mut [i16], dmin: &[i16]) {
    let l = nlsf.len();
    let d = |k: usize| i32::from(dmin[k]);
    let mut loops = 0;
    while loops < 20 {
        let mut min_diff = i32::from(nlsf[0]) - d(0);
        let mut ii = 0usize;
        for i in 1..l {
            let diff = i32::from(nlsf[i]) - (i32::from(nlsf[i - 1]) + d(i));
            if diff < min_diff {
                min_diff = diff;
                ii = i;
            }
        }
        let diff = (1 << 15) - (i32::from(nlsf[l - 1]) + d(l));
        if diff < min_diff {
            min_diff = diff;
            ii = l;
        }
        if min_diff >= 0 {
            return;
        }
        if ii == 0 {
            nlsf[0] = dmin[0];
        } else if ii == l {
            nlsf[l - 1] = ((1 << 15) - d(l)) as i16;
        } else {
            let mut min_center = 0i32;
            for k in 0..ii {
                min_center += d(k);
            }
            min_center += d(ii) >> 1;
            let mut max_center = 1i32 << 15;
            for k in (ii + 1..=l).rev() {
                max_center -= d(k);
            }
            max_center -= d(ii) >> 1;
            let center = super::fix::limit(
                rshift_round(i32::from(nlsf[ii - 1]) + i32::from(nlsf[ii]), 1),
                min_center,
                max_center,
            ) as i16;
            nlsf[ii - 1] = (i32::from(center) - (d(ii) >> 1)) as i16;
            nlsf[ii] = (i32::from(nlsf[ii - 1]) + d(ii)) as i16;
        }
        loops += 1;
    }
    // Fall back to the safe method.
    for i in 1..l {
        let v = nlsf[i];
        let mut j = i;
        while j > 0 && v < nlsf[j - 1] {
            nlsf[j] = nlsf[j - 1];
            j -= 1;
        }
        nlsf[j] = v;
    }
    nlsf[0] = nlsf[0].max(dmin[0]);
    for i in 1..l {
        nlsf[i] = i32::from(nlsf[i]).max(sat16(i32::from(nlsf[i - 1]) + d(i))) as i16;
    }
    nlsf[l - 1] = i32::from(nlsf[l - 1]).min((1 << 15) - d(l)) as i16;
    for i in (0..l - 1).rev() {
        nlsf[i] = i32::from(nlsf[i]).min(i32::from(nlsf[i + 1]) - d(i + 1)) as i16;
    }
}

const QA: u32 = 16;

fn find_poly(out: &mut [i32], c: &[i32], at: usize, dd: usize) {
    out[0] = 1 << QA;
    out[1] = -c[at];
    for k in 1..dd {
        let ftmp = c[at + 2 * k];
        out[k + 1] = (out[k - 1] << 1).wrapping_sub(rshift_round64(smull(ftmp, out[k]), QA) as i32);
        for n in (2..=k).rev() {
            out[n] = out[n].wrapping_add(
                out[n - 2].wrapping_sub(rshift_round64(smull(ftmp, out[n - 1]), QA) as i32),
            );
        }
        out[1] = out[1].wrapping_sub(ftmp);
    }
}

/// `silk_bwexpander_32`.
pub(crate) fn bwexpander_32(ar: &mut [i32], mut chirp_q16: i32) {
    let d = ar.len();
    let cm1 = chirp_q16 - 65536;
    for v in ar.iter_mut().take(d - 1) {
        *v = smulww(chirp_q16, *v);
        chirp_q16 += rshift_round(chirp_q16.wrapping_mul(cm1), 16);
    }
    ar[d - 1] = smulww(chirp_q16, ar[d - 1]);
}

/// `silk_bwexpander`.
pub(crate) fn bwexpander(ar: &mut [i16], mut chirp_q16: i32) {
    let d = ar.len();
    let cm1 = chirp_q16 - 65536;
    for v in ar.iter_mut().take(d - 1) {
        *v = rshift_round(chirp_q16.wrapping_mul(i32::from(*v)), 16) as i16;
        chirp_q16 += rshift_round(chirp_q16.wrapping_mul(cm1), 16);
    }
    ar[d - 1] = rshift_round(chirp_q16.wrapping_mul(i32::from(ar[d - 1])), 16) as i16;
}

/// `silk_LPC_fit`: Q`qin` coefficients into Q`qout` int16, chirping
/// until they fit.
fn lpc_fit(a_qout: &mut [i16], a_qin: &mut [i32], qout: u32, qin: u32) {
    let d = a_qin.len();
    let mut idx = 0usize;
    let mut i = 0;
    while i < 10 {
        let mut maxabs = 0i32;
        for (k, &v) in a_qin.iter().enumerate() {
            let a = v.wrapping_abs();
            if a > maxabs {
                maxabs = a;
                idx = k;
            }
        }
        maxabs = rshift_round(maxabs, qin - qout);
        if maxabs > i32::from(i16::MAX) {
            maxabs = maxabs.min(163_838);
            let chirp = 65470 - ((maxabs - 32767) << 14) / ((maxabs * (idx as i32 + 1)) >> 2);
            bwexpander_32(a_qin, chirp);
        } else {
            break;
        }
        i += 1;
    }
    if i == 10 {
        for k in 0..d {
            a_qout[k] = sat16(rshift_round(a_qin[k], qin - qout)) as i16;
            a_qin[k] = i32::from(a_qout[k]) << (qin - qout);
        }
    } else {
        for k in 0..d {
            a_qout[k] = rshift_round(a_qin[k], qin - qout) as i16;
        }
    }
}

const A_LIMIT: i32 = 16_773_022; // SILK_FIX_CONST(0.99975, 24)
const INV_GAIN_MIN_Q30: i32 = 107_374; // SILK_FIX_CONST(1/MAX_PREDICTION_POWER_GAIN, 30)

fn inv_pred_gain_qa(a: &mut [i32; 16], order: usize) -> i32 {
    let mut inv_gain_q30: i32 = 1 << 30;
    let mut k = order - 1;
    while k > 0 {
        if a[k] > A_LIMIT || a[k] < -A_LIMIT {
            return 0;
        }
        let rc_q31 = -(a[k] << 7);
        let rc_mult1_q30 = (1i32 << 30).wrapping_sub(smmul(rc_q31, rc_q31));
        inv_gain_q30 = smmul(inv_gain_q30, rc_mult1_q30) << 2;
        if inv_gain_q30 < INV_GAIN_MIN_Q30 {
            return 0;
        }
        let mult2q = 32 - clz32(rc_mult1_q30.wrapping_abs());
        let rc_mult2 = inverse32_varq(rc_mult1_q30, mult2q + 30);
        for n in 0..(k + 1) >> 1 {
            let tmp1 = a[n];
            let tmp2 = a[k - n - 1];
            let frac = |x: i32| rshift_round64(smull(x, rc_q31), 31) as i32;
            let t64 = rshift_round64(
                smull(tmp1.saturating_sub(frac(tmp2)), rc_mult2),
                mult2q as u32,
            );
            if t64 > i64::from(i32::MAX) || t64 < i64::from(i32::MIN) {
                return 0;
            }
            a[n] = t64 as i32;
            let t64 = rshift_round64(
                smull(tmp2.saturating_sub(frac(tmp1)), rc_mult2),
                mult2q as u32,
            );
            if t64 > i64::from(i32::MAX) || t64 < i64::from(i32::MIN) {
                return 0;
            }
            a[k - n - 1] = t64 as i32;
        }
        k -= 1;
    }
    if a[0] > A_LIMIT || a[0] < -A_LIMIT {
        return 0;
    }
    let rc_q31 = -(a[0] << 7);
    let rc_mult1_q30 = (1i32 << 30).wrapping_sub(smmul(rc_q31, rc_q31));
    inv_gain_q30 = smmul(inv_gain_q30, rc_mult1_q30) << 2;
    if inv_gain_q30 < INV_GAIN_MIN_Q30 {
        return 0;
    }
    inv_gain_q30
}

/// `silk_LPC_inverse_pred_gain` (Q30), 0 when the filter is unstable.
pub(crate) fn lpc_inverse_pred_gain(a_q12: &[i16]) -> i32 {
    let mut a = [0i32; 16];
    let mut dc = 0i32;
    for (k, &v) in a_q12.iter().enumerate() {
        dc += i32::from(v);
        a[k] = i32::from(v) << 12;
    }
    if dc >= 4096 {
        return 0;
    }
    inv_pred_gain_qa(&mut a, a_q12.len())
}

/// `silk_NLSF2A`: NLSFs (Q15) to stable LPC coefficients (Q12).
pub(crate) fn nlsf2a(a_q12: &mut [i16], nlsf: &[i16]) {
    const ORD16: [usize; 16] = [0, 15, 8, 7, 4, 11, 12, 3, 2, 13, 10, 5, 6, 9, 14, 1];
    const ORD10: [usize; 10] = [0, 9, 6, 3, 4, 5, 8, 1, 2, 7];
    let d = a_q12.len();
    let ordering: &[usize] = if d == 16 { &ORD16 } else { &ORD10 };
    let mut cos_lsf = [0i32; 16];
    for k in 0..d {
        let n = i32::from(nlsf[k]);
        let f_int = n >> 8;
        let f_frac = n - (f_int << 8);
        let cos_val = i32::from(t::LSF_COS_TAB_FIX_Q12[f_int as usize]);
        let delta = i32::from(t::LSF_COS_TAB_FIX_Q12[f_int as usize + 1]) - cos_val;
        cos_lsf[ordering[k]] = rshift_round((cos_val << 8) + delta * f_frac, 20 - QA);
    }
    let dd = d >> 1;
    let mut p = [0i32; 9];
    let mut q = [0i32; 9];
    find_poly(&mut p, &cos_lsf, 0, dd);
    find_poly(&mut q, &cos_lsf, 1, dd);
    let mut a32 = [0i32; 16];
    for k in 0..dd {
        let ptmp = p[k + 1].wrapping_add(p[k]);
        let qtmp = q[k + 1].wrapping_sub(q[k]);
        a32[k] = (-qtmp).wrapping_sub(ptmp);
        a32[d - k - 1] = qtmp.wrapping_sub(ptmp);
    }
    lpc_fit(a_q12, &mut a32[..d], 12, QA + 1);
    let mut i = 0;
    while lpc_inverse_pred_gain(a_q12) == 0 && i < 16 {
        bwexpander_32(&mut a32[..d], 65536 - (2 << i));
        for k in 0..d {
            a_q12[k] = rshift_round(a32[k], QA + 1 - 12) as i16;
        }
        i += 1;
    }
}

/// `silk_LPC_analysis_filter`: `out[d..len]` the residual of `input`
/// through `b`; `out[..d]` zero.
pub(crate) fn lpc_analysis_filter(out: &mut [i16], input: &[i16], b: &[i16], len: usize) {
    let d = b.len();
    for ix in d..len {
        let p = ix - 1;
        let mut acc = smulbb(i32::from(input[p]), i32::from(b[0]));
        for j in 1..d {
            acc = smlabb(acc, i32::from(input[p - j]), i32::from(b[j]));
        }
        acc = (i32::from(input[p + 1]) << 12).wrapping_sub(acc);
        out[ix] = sat16(rshift_round(acc, 12)) as i16;
    }
    for v in out.iter_mut().take(d) {
        *v = 0;
    }
}

/// `silk_sum_sqr_shift`: energy of `x` and the right shift that keeps it
/// in 32 bits.
pub(crate) fn sum_sqr_shift(x: &[i16]) -> (i32, i32) {
    let len = x.len();
    let pass = |shft: i32, init: i32| -> i32 {
        let mut nrg = init;
        let mut i = 0;
        while i + 1 < len {
            let mut t = smulbb(i32::from(x[i]), i32::from(x[i])) as u32;
            t = t.wrapping_add(smulbb(i32::from(x[i + 1]), i32::from(x[i + 1])) as u32);
            nrg = (nrg as u32).wrapping_add(t >> shft) as i32;
            i += 2;
        }
        if i < len {
            let t = smulbb(i32::from(x[i]), i32::from(x[i])) as u32;
            nrg = (nrg as u32).wrapping_add(t >> shft) as i32;
        }
        nrg
    };
    let mut shft = 31 - clz32(len as i32);
    let nrg = pass(shft, len as i32);
    shft = (shft + 3 - clz32(nrg)).max(0);
    (pass(shft, 0), shft)
}
