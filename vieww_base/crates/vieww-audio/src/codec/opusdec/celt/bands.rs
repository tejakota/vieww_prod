//! CELT's band shapes: the recursive theta split, PVQ pulse decoding and
//! the stereo, folding and time–frequency machinery around it (libopus
//! `bands.c`, `vq.c`, `cwrs.c`) — the decode side only.

#![allow(
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]

use super::super::range::{ilog, RangeDecoder, BITRES};
use super::alloc::{bits2pulses, cache_max, eb, get_pulses, pulses2bits};
use super::tables as t;
use super::NB_EBANDS;

pub(crate) const SPREAD_NONE: i32 = 0;
pub(crate) const SPREAD_AGGRESSIVE: i32 = 3;

pub(crate) const fn lcg_rand(seed: u32) -> u32 {
    seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

fn frac_mul16(a: i32, b: i32) -> i32 {
    (16384 + a * b) >> 15
}

fn bitexact_cos(x: i16) -> i16 {
    let tmp = (4096 + i32::from(x) * i32::from(x)) >> 13;
    let x2 = tmp;
    let x2 = (32767 - x2) + frac_mul16(x2, -7651 + frac_mul16(x2, 8277 + frac_mul16(-626, x2)));
    (1 + x2) as i16
}

fn bitexact_log2tan(isin: i32, icos: i32) -> i32 {
    let lc = ilog(icos as u32);
    let ls = ilog(isin as u32);
    let icos = icos << (15 - lc);
    let isin = isin << (15 - ls);
    (ls - lc) * (1 << 11) + frac_mul16(isin, frac_mul16(isin, -2597) + 7932)
        - frac_mul16(icos, frac_mul16(icos, -2597) + 7932)
}

pub(crate) fn isqrt32(mut val: u32) -> u32 {
    let mut g = 0u32;
    let mut bshift = (ilog(val) - 1) >> 1;
    let mut b = 1u32 << bshift;
    loop {
        let t = ((g << 1) + b) << bshift;
        if t <= val {
            g += b;
            val -= t;
        }
        b >>= 1;
        bshift -= 1;
        if bshift < 0 {
            break;
        }
    }
    g
}

fn pvq_u(n: usize, k: usize) -> u32 {
    const ROW: [usize; 15] = [
        0, 176, 351, 525, 698, 870, 1041, 1131, 1178, 1207, 1226, 1240, 1248, 1254, 1257,
    ];
    let (a, b) = (n.min(k), n.max(k));
    t::PVQ_U_DATA.get(ROW[a] + b).copied().unwrap_or(0)
}

fn row(n: usize, k: usize) -> u32 {
    // CELT_PVQ_U_ROW[n][k]
    const ROW: [usize; 15] = [
        0, 176, 351, 525, 698, 870, 1041, 1131, 1178, 1207, 1226, 1240, 1248, 1254, 1257,
    ];
    t::PVQ_U_DATA.get(ROW[n.min(14)] + k).copied().unwrap_or(0)
}

/// `cwrsi`: the `i`th pulse vector of `n` dimensions and `k` pulses.
fn cwrsi(mut n: usize, mut k: usize, mut i: u32, y: &mut [i32]) -> f32 {
    let mut yy = 0f32;
    let mut at = 0usize;
    while n > 2 {
        let val;
        if k >= n {
            let p = row(n, k + 1);
            let s = -i32::from(i >= p);
            i -= p & (s as u32);
            let k0 = k as i32;
            let q = row(n, n);
            let mut p;
            if q > i {
                k = n;
                loop {
                    k -= 1;
                    p = row(k, n);
                    if p <= i {
                        break;
                    }
                }
            } else {
                p = row(n, k);
                while p > i {
                    k -= 1;
                    p = row(n, k);
                }
            }
            i -= p;
            val = (k0 - k as i32 + s) ^ s;
        } else {
            let p = row(k, n);
            let q = row(k + 1, n);
            if p <= i && i < q {
                i -= p;
                y[at] = 0;
                at += 1;
                n -= 1;
                continue;
            }
            let s = -i32::from(i >= q);
            i -= q & (s as u32);
            let k0 = k as i32;
            let mut p;
            loop {
                k -= 1;
                p = row(k, n);
                if p <= i {
                    break;
                }
            }
            i -= p;
            val = (k0 - k as i32 + s) ^ s;
        }
        y[at] = val;
        at += 1;
        yy += (val as f32) * (val as f32);
        n -= 1;
    }
    let p = 2 * k as u32 + 1;
    let s = -i32::from(i >= p);
    i -= p & (s as u32);
    let k0 = k as i32;
    k = ((i + 1) >> 1) as usize;
    if k != 0 {
        i -= 2 * k as u32 - 1;
    }
    let val = (k0 - k as i32 + s) ^ s;
    y[at] = val;
    yy += (val as f32) * (val as f32);
    let s = -(i as i32);
    let val = (k as i32 + s) ^ s;
    y[at + 1] = val;
    yy += (val as f32) * (val as f32);
    yy
}

fn decode_pulses(y: &mut [i32], n: usize, k: usize, dec: &mut RangeDecoder<'_>) -> f32 {
    let ft = pvq_u(n, k).wrapping_add(pvq_u(n, k + 1));
    let i = dec.uint(ft.max(2));
    cwrsi(n, k, i, y)
}

fn exp_rotation1(x: &mut [f32], len: usize, stride: usize, c: f32, s: f32) {
    let ms = -s;
    let mut p = 0usize;
    for _ in 0..len.saturating_sub(stride) {
        let x1 = x[p];
        let x2 = x[p + stride];
        x[p + stride] = c * x2 + s * x1;
        x[p] = c * x1 + ms * x2;
        p += 1;
    }
    if len >= 2 * stride + 1 {
        let mut p = len - 2 * stride - 1;
        loop {
            let x1 = x[p];
            let x2 = x[p + stride];
            x[p + stride] = c * x2 + s * x1;
            x[p] = c * x1 + ms * x2;
            if p == 0 {
                break;
            }
            p -= 1;
        }
    }
}

fn exp_rotation(x: &mut [f32], len: usize, dir: i32, stride: usize, k: i32, spread: i32) {
    const SPREAD_FACTOR: [i32; 3] = [15, 10, 5];
    if 2 * k >= len as i32 || spread == SPREAD_NONE {
        return;
    }
    let factor = SPREAD_FACTOR[(spread - 1) as usize];
    let gain = (1.0f32 * len as f32) / (len as i32 + factor * k) as f32;
    let theta = 0.5 * (gain * gain);
    let c = f64::from((0.5f32 * 3.141_592_7) * theta).cos() as f32;
    let s = f64::from((0.5f32 * 3.141_592_7) * (1.0 - theta)).cos() as f32;
    let mut stride2 = 0usize;
    if len >= 8 * stride {
        stride2 = 1;
        while (stride2 * stride2 + stride2) * stride + (stride >> 2) < len {
            stride2 += 1;
        }
    }
    let l = len / stride;
    for i in 0..stride {
        let seg = &mut x[i * l..i * l + l];
        if dir < 0 {
            if stride2 != 0 {
                exp_rotation1(seg, l, stride2, s, c);
            }
            exp_rotation1(seg, l, 1, c, s);
        } else {
            exp_rotation1(seg, l, 1, c, -s);
            if stride2 != 0 {
                exp_rotation1(seg, l, stride2, s, -c);
            }
        }
    }
}

fn alg_unquant(
    x: &mut [f32],
    n: usize,
    k: i32,
    spread: i32,
    b: usize,
    dec: &mut RangeDecoder<'_>,
    gain: f32,
) -> u32 {
    let mut iy = vec![0i32; n];
    let ryy = decode_pulses(&mut iy, n, k as usize, dec);
    let g = (1.0 / f64::from(ryy).sqrt() as f32) * gain;
    for i in 0..n {
        x[i] = g * iy[i] as f32;
    }
    exp_rotation(x, n, -1, b, k, spread);
    if b <= 1 {
        return 1;
    }
    let n0 = n / b;
    let mut mask = 0u32;
    for i in 0..b {
        let tmp = iy[i * n0..(i + 1) * n0].iter().fold(0, |a, v| a | v);
        mask |= u32::from(tmp != 0) << i;
    }
    mask
}

pub(crate) fn renormalise_vector(x: &mut [f32], n: usize, gain: f32) {
    let e = 1e-15f32 + super::dsp::inner_prod(x, x, n);
    let g = (1.0 / f64::from(e).sqrt() as f32) * gain;
    for v in x.iter_mut().take(n) {
        *v *= g;
    }
}

fn haar1(x: &mut [f32], n0: usize, stride: usize) {
    let n0 = n0 >> 1;
    for i in 0..stride {
        for j in 0..n0 {
            let tmp1 = 0.707_106_77 * x[stride * 2 * j + i];
            let tmp2 = 0.707_106_77 * x[stride * (2 * j + 1) + i];
            x[stride * 2 * j + i] = tmp1 + tmp2;
            x[stride * (2 * j + 1) + i] = tmp1 - tmp2;
        }
    }
}

fn deinterleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = vec![0f32; n];
    if hadamard {
        let ord = &t::ORDERY[stride - 2..];
        for i in 0..stride {
            for j in 0..n0 {
                tmp[ord[i] as usize * n0 + j] = x[j * stride + i];
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[i * n0 + j] = x[j * stride + i];
            }
        }
    }
    x[..n].copy_from_slice(&tmp);
}

fn interleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = vec![0f32; n];
    if hadamard {
        let ord = &t::ORDERY[stride - 2..];
        for i in 0..stride {
            for j in 0..n0 {
                tmp[j * stride + i] = x[ord[i] as usize * n0 + j];
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[j * stride + i] = x[i * n0 + j];
            }
        }
    }
    x[..n].copy_from_slice(&tmp);
}

fn compute_qn(n: i32, b: i32, offset: i32, pulse_cap: i32, stereo: bool) -> i32 {
    const EXP2_TABLE8: [i32; 8] = [16384, 17866, 19483, 21247, 23170, 25267, 27554, 30048];
    let mut n2 = 2 * n - 1;
    if stereo && n == 2 {
        n2 -= 1;
    }
    let mut qb = (b + n2 * offset) / n2; // celt_sudiv: truncating
    qb = (b - pulse_cap - (4 << BITRES)).min(qb);
    qb = (8 << BITRES).min(qb);
    if qb < (1 << BITRES >> 1) {
        1
    } else {
        let qn = EXP2_TABLE8[(qb & 7) as usize] >> (14 - (qb >> BITRES));
        (qn + 1) >> 1 << 1
    }
}

struct Ctx<'a, 'b> {
    i: usize,
    intensity: usize,
    spread: i32,
    tf_change: i32,
    ec: &'a mut RangeDecoder<'b>,
    remaining_bits: i32,
    seed: u32,
    disable_inv: bool,
}

struct Split {
    inv: bool,
    imid: i32,
    iside: i32,
    delta: i32,
    itheta: i32,
    qalloc: i32,
}

fn compute_theta(
    ctx: &mut Ctx<'_, '_>,
    n: i32,
    b: &mut i32,
    bb: i32,
    b0: i32,
    lm: i32,
    stereo: bool,
    fill: &mut i32,
) -> Split {
    let i = ctx.i;
    let pulse_cap = i32::from(t::LOGN[i]) + lm * (1 << BITRES);
    let offset = (pulse_cap >> 1) - if stereo && n == 2 { 16 } else { 4 };
    let mut qn = compute_qn(n, *b, offset, pulse_cap, stereo);
    if stereo && i >= ctx.intensity {
        qn = 1;
    }
    let tell = ctx.ec.tell_frac() as i32;
    let mut itheta = 0i32;
    let mut inv = false;
    let ec = &mut *ctx.ec;
    if qn != 1 {
        if stereo && n > 2 {
            let p0 = 3;
            let x0 = qn / 2;
            let ft = p0 * (x0 + 1) + x0;
            let fs = ec.decode(ft as u32) as i32;
            let x = if fs < (x0 + 1) * p0 {
                fs / p0
            } else {
                x0 + 1 + (fs - (x0 + 1) * p0)
            };
            let (fl, fh) = if x <= x0 {
                (p0 * x, p0 * (x + 1))
            } else {
                ((x - 1 - x0) + (x0 + 1) * p0, (x - x0) + (x0 + 1) * p0)
            };
            ec.update(fl as u32, fh as u32, ft as u32);
            itheta = x;
        } else if b0 > 1 || stereo {
            itheta = ec.uint((qn + 1) as u32) as i32;
        } else {
            let ft = ((qn >> 1) + 1) * ((qn >> 1) + 1);
            let fm = ec.decode(ft as u32) as i32;
            let (fs, fl);
            if fm < ((qn >> 1) * ((qn >> 1) + 1) >> 1) {
                itheta = (isqrt32(8 * fm as u32 + 1) as i32 - 1) >> 1;
                fs = itheta + 1;
                fl = itheta * (itheta + 1) >> 1;
            } else {
                itheta = (2 * (qn + 1) - isqrt32(8 * (ft - fm - 1) as u32 + 1) as i32) >> 1;
                fs = qn + 1 - itheta;
                fl = ft - ((qn + 1 - itheta) * (qn + 2 - itheta) >> 1);
            }
            ec.update(fl as u32, (fl + fs) as u32, ft as u32);
        }
        itheta = ((itheta as u32 * 16384) / qn as u32) as i32;
    } else if stereo {
        if *b > 2 << BITRES && ctx.remaining_bits > 2 << BITRES {
            inv = ec.bit_logp(2);
        }
        if ctx.disable_inv {
            inv = false;
        }
        itheta = 0;
    }
    let qalloc = ctx.ec.tell_frac() as i32 - tell;
    *b -= qalloc;
    let (imid, iside, delta);
    if itheta == 0 {
        imid = 32767;
        iside = 0;
        *fill &= (1 << bb) - 1;
        delta = -16384;
    } else if itheta == 16384 {
        imid = 0;
        iside = 32767;
        *fill &= ((1 << bb) - 1) << bb;
        delta = 16384;
    } else {
        imid = i32::from(bitexact_cos(itheta as i16));
        iside = i32::from(bitexact_cos((16384 - itheta) as i16));
        delta = (16384
            + i32::from(((n - 1) << 7) as i16) * i32::from(bitexact_log2tan(iside, imid) as i16))
            >> 15;
    }
    Split {
        inv,
        imid,
        iside,
        delta,
        itheta,
        qalloc,
    }
}

fn quant_band_n1(
    ctx: &mut Ctx<'_, '_>,
    x: &mut [f32],
    y: Option<&mut [f32]>,
    lowband_out: Option<&mut [f32]>,
) -> u32 {
    let decode_one = |ctx: &mut Ctx<'_, '_>, v: &mut [f32]| {
        let mut sign = false;
        if ctx.remaining_bits >= 1 << BITRES {
            sign = ctx.ec.bits(1) == 1;
            ctx.remaining_bits -= 1 << BITRES;
        }
        v[0] = if sign { -1.0 } else { 1.0 };
    };
    decode_one(ctx, x);
    if let Some(y) = y {
        decode_one(ctx, y);
    }
    if let Some(o) = lowband_out {
        o[0] = x[0];
    }
    1
}

fn quant_partition(
    ctx: &mut Ctx<'_, '_>,
    x: &mut [f32],
    mut n: usize,
    mut b: i32,
    mut bb: i32,
    lowband: Option<&[f32]>,
    mut lm: i32,
    gain: f32,
    mut fill: i32,
) -> u32 {
    let i = ctx.i;
    let b0 = bb;
    if lm != -1 && b > cache_max(i, lm) + 12 && n > 2 {
        n >>= 1;
        lm -= 1;
        if bb == 1 {
            fill = (fill & 1) | (fill << 1);
        }
        bb = (bb + 1) >> 1;
        let sc = compute_theta(ctx, n as i32, &mut b, bb, b0, lm, false, &mut fill);
        let mid = (1.0f32 / 32768.0) * sc.imid as f32;
        let side = (1.0f32 / 32768.0) * sc.iside as f32;
        let mut delta = sc.delta;
        if b0 > 1 && (sc.itheta & 0x3fff) != 0 {
            if sc.itheta > 8192 {
                delta -= delta >> (4 - lm);
            } else {
                delta = 0.min(delta + ((n as i32) << BITRES >> (5 - lm)));
            }
        }
        let mut mbits = 0.max(b.min((b - delta) / 2));
        let mut sbits = b - mbits;
        ctx.remaining_bits -= sc.qalloc;
        let (xl, yl) = x.split_at_mut(n);
        let lb1 = lowband;
        let lb2 = lowband.map(|l| &l[n..]);
        let mut rebalance = ctx.remaining_bits;
        let cm;
        if mbits >= sbits {
            let mut c = quant_partition(ctx, xl, n, mbits, bb, lb1, lm, gain * mid, fill);
            rebalance = mbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && sc.itheta != 0 {
                sbits += rebalance - (3 << BITRES);
            }
            c |= quant_partition(ctx, yl, n, sbits, bb, lb2, lm, gain * side, fill >> bb)
                << (b0 >> 1);
            cm = c;
        } else {
            let mut c = quant_partition(ctx, yl, n, sbits, bb, lb2, lm, gain * side, fill >> bb)
                << (b0 >> 1);
            rebalance = sbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && sc.itheta != 16384 {
                mbits += rebalance - (3 << BITRES);
            }
            c |= quant_partition(ctx, xl, n, mbits, bb, lb1, lm, gain * mid, fill);
            cm = c;
        }
        return cm;
    }
    let mut q = bits2pulses(i, lm, b);
    let mut curr_bits = pulses2bits(i, lm, q);
    ctx.remaining_bits -= curr_bits;
    while ctx.remaining_bits < 0 && q > 0 {
        ctx.remaining_bits += curr_bits;
        q -= 1;
        curr_bits = pulses2bits(i, lm, q);
        ctx.remaining_bits -= curr_bits;
    }
    if q != 0 {
        let k = get_pulses(q);
        return alg_unquant(&mut x[..n], n, k, ctx.spread, bb as usize, ctx.ec, gain);
    }
    let cm_mask = ((1u64 << bb) - 1) as u32;
    let fill = (fill as u32) & cm_mask;
    if fill == 0 {
        for v in x.iter_mut().take(n) {
            *v = 0.0;
        }
        return 0;
    }
    let cm;
    match lowband {
        None => {
            for v in x.iter_mut().take(n) {
                ctx.seed = lcg_rand(ctx.seed);
                *v = ((ctx.seed as i32) >> 20) as f32;
            }
            cm = cm_mask;
        }
        Some(lb) => {
            for j in 0..n {
                ctx.seed = lcg_rand(ctx.seed);
                let tmp = 1.0f32 / 256.0;
                let tmp = if ctx.seed & 0x8000 != 0 { tmp } else { -tmp };
                x[j] = lb[j] + tmp;
            }
            cm = fill;
        }
    }
    renormalise_vector(x, n, gain);
    cm
}

fn quant_band(
    ctx: &mut Ctx<'_, '_>,
    x: &mut [f32],
    n: usize,
    b: i32,
    bb: i32,
    lowband: Option<&[f32]>,
    lm: i32,
    lowband_out: Option<&mut [f32]>,
    gain: f32,
    mut fill: i32,
) -> u32 {
    const BIT_INTERLEAVE: [i32; 16] = [0, 1, 1, 1, 2, 3, 3, 3, 2, 3, 3, 3, 2, 3, 3, 3];
    const BIT_DEINTERLEAVE: [u32; 16] = [
        0x00, 0x03, 0x0C, 0x0F, 0x30, 0x33, 0x3C, 0x3F, 0xC0, 0xC3, 0xCC, 0xCF, 0xF0, 0xF3, 0xFC,
        0xFF,
    ];
    let n0 = n;
    let mut n_b = n;
    let mut b_ = bb;
    let b0_orig = bb;
    let long_blocks = b0_orig == 1;
    let mut time_divide = 0;
    let mut recombine = 0;
    let mut tf_change = ctx.tf_change;
    n_b /= b_ as usize;
    if n == 1 {
        return quant_band_n1(ctx, x, None, lowband_out);
    }
    if tf_change > 0 {
        recombine = tf_change;
    }
    // A private copy of the folding source: libopus copies it into scratch
    // exactly when it is about to be transformed, and otherwise only reads
    // it, so always copying is the same arithmetic.
    let mut lb: Option<Vec<f32>> = lowband.map(|l| l[..n].to_vec());
    for k in 0..recombine {
        if let Some(l) = lb.as_mut() {
            haar1(l, n >> k, 1 << k);
        }
        fill = BIT_INTERLEAVE[(fill & 0xF) as usize]
            | BIT_INTERLEAVE[((fill >> 4) & 0xF) as usize] << 2;
    }
    b_ >>= recombine;
    n_b <<= recombine;
    while (n_b & 1) == 0 && tf_change < 0 {
        if let Some(l) = lb.as_mut() {
            haar1(l, n_b, b_ as usize);
        }
        fill |= fill << b_;
        b_ <<= 1;
        n_b >>= 1;
        time_divide += 1;
        tf_change += 1;
    }
    let b0 = b_;
    let n_b0 = n_b;
    if b0 > 1 {
        if let Some(l) = lb.as_mut() {
            deinterleave_hadamard(l, n_b >> recombine, (b0 << recombine) as usize, long_blocks);
        }
    }
    let mut cm = quant_partition(ctx, x, n, b, b_, lb.as_deref(), lm, gain, fill);
    if b0 > 1 {
        interleave_hadamard(x, n_b >> recombine, (b0 << recombine) as usize, long_blocks);
    }
    n_b = n_b0;
    b_ = b0;
    for _ in 0..time_divide {
        b_ >>= 1;
        n_b <<= 1;
        cm |= cm >> b_;
        haar1(x, n_b, b_ as usize);
    }
    for k in 0..recombine {
        cm = BIT_DEINTERLEAVE[(cm & 0xF) as usize];
        haar1(x, n0 >> k, 1 << k);
    }
    b_ <<= recombine;
    if let Some(o) = lowband_out {
        let nn = f64::from(n0 as f32).sqrt() as f32;
        for j in 0..n0 {
            o[j] = nn * x[j];
        }
    }
    cm & ((1u32 << b_) - 1)
}

fn stereo_merge(x: &mut [f32], y: &mut [f32], mid: f32, n: usize) {
    let mut xp = 0f32;
    let mut side = 0f32;
    for i in 0..n {
        xp += y[i] * x[i];
        side += y[i] * y[i];
    }
    let xp = mid * xp;
    let mid2 = mid;
    let el = mid2 * mid2 + side - 2.0 * xp;
    let er = mid2 * mid2 + side + 2.0 * xp;
    if er < 6e-4 || el < 6e-4 {
        y[..n].copy_from_slice(&x[..n]);
        return;
    }
    let lgain = 1.0 / f64::from(el).sqrt() as f32;
    let rgain = 1.0 / f64::from(er).sqrt() as f32;
    for j in 0..n {
        let l = mid * x[j];
        let r = y[j];
        x[j] = lgain * (l - r);
        y[j] = rgain * (l + r);
    }
}

fn quant_band_stereo(
    ctx: &mut Ctx<'_, '_>,
    x: &mut [f32],
    y: &mut [f32],
    n: usize,
    mut b: i32,
    bb: i32,
    lowband: Option<&[f32]>,
    lm: i32,
    lowband_out: Option<&mut [f32]>,
    mut fill: i32,
) -> u32 {
    if n == 1 {
        return quant_band_n1(ctx, x, Some(y), lowband_out);
    }
    let orig_fill = fill;
    let sc = compute_theta(ctx, n as i32, &mut b, bb, bb, lm, true, &mut fill);
    let mid = (1.0f32 / 32768.0) * sc.imid as f32;
    let side = (1.0f32 / 32768.0) * sc.iside as f32;
    let mut cm;
    if n == 2 {
        let mut mbits = b;
        let mut sbits = 0;
        if sc.itheta != 0 && sc.itheta != 16384 {
            sbits = 1 << BITRES;
        }
        mbits -= sbits;
        let c = sc.itheta > 8192;
        ctx.remaining_bits -= sc.qalloc + sbits;
        let mut sign = 0;
        if sbits != 0 {
            sign = ctx.ec.bits(1) as i32;
        }
        let sign = 1 - 2 * sign;
        {
            let (x2, y2): (&mut [f32], &mut [f32]) = if c {
                (&mut *y, &mut *x)
            } else {
                (&mut *x, &mut *y)
            };
            cm = quant_band(
                ctx,
                x2,
                n,
                mbits,
                bb,
                lowband,
                lm,
                lowband_out,
                1.0,
                orig_fill,
            );
            y2[0] = -(sign as f32) * x2[1];
            y2[1] = sign as f32 * x2[0];
        }
        x[0] *= mid;
        x[1] *= mid;
        y[0] *= side;
        y[1] *= side;
        let tmp = x[0];
        x[0] = tmp - y[0];
        y[0] = tmp + y[0];
        let tmp = x[1];
        x[1] = tmp - y[1];
        y[1] = tmp + y[1];
    } else {
        let mut mbits = 0.max(b.min((b - sc.delta) / 2));
        let mut sbits = b - mbits;
        ctx.remaining_bits -= sc.qalloc;
        let mut rebalance = ctx.remaining_bits;
        if mbits >= sbits {
            cm = quant_band(ctx, x, n, mbits, bb, lowband, lm, lowband_out, 1.0, fill);
            rebalance = mbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && sc.itheta != 0 {
                sbits += rebalance - (3 << BITRES);
            }
            cm |= quant_band(ctx, y, n, sbits, bb, None, lm, None, side, fill >> bb);
        } else {
            cm = quant_band(ctx, y, n, sbits, bb, None, lm, None, side, fill >> bb);
            rebalance = sbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && sc.itheta != 16384 {
                mbits += rebalance - (3 << BITRES);
            }
            cm |= quant_band(ctx, x, n, mbits, bb, lowband, lm, lowband_out, 1.0, fill);
        }
    }
    if n != 2 {
        stereo_merge(x, y, mid, n);
    }
    if sc.inv {
        for v in y.iter_mut().take(n) {
            *v = -*v;
        }
    }
    cm
}

/// `quant_all_bands` (decode): every band's shape into `x` (and `y` for a
/// stereo frame), `n` coefficients per channel.
pub(crate) fn decode_all_bands(
    start: usize,
    end: usize,
    x_: &mut [f32],
    mut y_: Option<&mut [f32]>,
    collapse_masks: &mut [u8],
    pulses: &[i32; NB_EBANDS],
    short_blocks: bool,
    spread: i32,
    mut dual_stereo: bool,
    intensity: usize,
    tf_res: &[i32; NB_EBANDS],
    total_bits: i32,
    mut balance: i32,
    ec: &mut RangeDecoder<'_>,
    lm: i32,
    coded_bands: usize,
    seed: &mut u32,
    disable_inv: bool,
) {
    let m = 1i32 << lm;
    let bb = if short_blocks { m } else { 1 };
    let c = if y_.is_some() { 2 } else { 1 };
    let norm_offset = (m * eb(start)) as usize;
    let nlen = (m * eb(NB_EBANDS - 1)) as usize - norm_offset;
    let mut norm = vec![0f32; nlen];
    let mut norm2 = vec![0f32; nlen];
    let mut lowband_offset = 0usize;
    let mut update_lowband = true;
    let mut ctx = Ctx {
        i: start,
        intensity,
        spread,
        tf_change: 0,
        ec,
        remaining_bits: 0,
        seed: *seed,
        disable_inv,
    };
    for i in start..end {
        ctx.i = i;
        let last = i == end - 1;
        let xo = (m * eb(i)) as usize;
        let n = (m * eb(i + 1) - m * eb(i)) as usize;
        let tell = ctx.ec.tell_frac() as i32;
        if i != start {
            balance -= tell;
        }
        let remaining_bits = total_bits - tell - 1;
        ctx.remaining_bits = remaining_bits;
        let b = if i < coded_bands {
            let curr_balance = balance / 3.min(coded_bands as i32 - i as i32);
            0.max(16383.min((remaining_bits + 1).min(pulses[i] + curr_balance)))
        } else {
            0
        };
        if ((m * eb(i)) - n as i32 >= m * eb(start) || i == start + 1)
            && (update_lowband || lowband_offset == 0)
        {
            lowband_offset = i;
        }
        if i == start + 1 {
            // special_hybrid_folding
            let n1 = (m * (eb(start + 1) - eb(start))) as usize;
            let n2 = (m * (eb(start + 2) - eb(start + 1))) as usize;
            if n2 > n1 {
                let src: Vec<f32> = norm[2 * n1 - n2..n1].to_vec();
                norm[n1..n2].copy_from_slice(&src);
                if dual_stereo {
                    let src2: Vec<f32> = norm2[2 * n1 - n2..n1].to_vec();
                    norm2[n1..n2].copy_from_slice(&src2);
                }
            }
        }
        let tf_change = tf_res[i];
        ctx.tf_change = tf_change;
        let mut effective_lowband: i32 = -1;
        let (mut x_cm, mut y_cm);
        if lowband_offset != 0 && (spread != SPREAD_AGGRESSIVE || bb > 1 || tf_change < 0) {
            effective_lowband = 0.max(m * eb(lowband_offset) - norm_offset as i32 - n as i32);
            let mut fold_start = lowband_offset;
            loop {
                fold_start -= 1;
                if m * eb(fold_start) <= effective_lowband + norm_offset as i32 {
                    break;
                }
            }
            let mut fold_end = lowband_offset - 1;
            loop {
                fold_end += 1;
                if !(fold_end < i
                    && m * eb(fold_end) < effective_lowband + norm_offset as i32 + n as i32)
                {
                    break;
                }
            }
            x_cm = 0u32;
            y_cm = 0u32;
            let mut fi = fold_start;
            loop {
                x_cm |= u32::from(collapse_masks[fi * c]);
                y_cm |= u32::from(collapse_masks[fi * c + c - 1]);
                fi += 1;
                if fi >= fold_end {
                    break;
                }
            }
        } else {
            x_cm = (1u32 << bb) - 1;
            y_cm = x_cm;
        }
        if dual_stereo && i == intensity {
            dual_stereo = false;
            for j in 0..(m * eb(i)) as usize - norm_offset {
                norm[j] = 0.5 * (norm[j] + norm2[j]);
            }
        }
        let out_at = (m * eb(i)) as usize - norm_offset;
        let lb_range = if effective_lowband >= 0 {
            Some(effective_lowband as usize)
        } else {
            None
        };
        if dual_stereo {
            let lb: Option<Vec<f32>> = lb_range.map(|o| norm[o..o + n].to_vec());
            let (lo, _) = if last { (None, ()) } else { (Some(()), ()) };
            x_cm = {
                let mut out_buf = vec![0f32; n];
                let r = quant_band(
                    &mut ctx,
                    &mut x_[xo..xo + n],
                    n,
                    b / 2,
                    bb,
                    lb.as_deref(),
                    lm,
                    lo.map(|_| &mut out_buf[..]),
                    1.0,
                    x_cm as i32,
                );
                if !last {
                    norm[out_at..out_at + n].copy_from_slice(&out_buf);
                }
                r
            };
            let lb2: Option<Vec<f32>> = lb_range.map(|o| norm2[o..o + n].to_vec());
            let yb = y_.as_deref_mut().expect("dual stereo has two channels");
            y_cm = {
                let mut out_buf = vec![0f32; n];
                let r = quant_band(
                    &mut ctx,
                    &mut yb[xo..xo + n],
                    n,
                    b / 2,
                    bb,
                    lb2.as_deref(),
                    lm,
                    lo.map(|_| &mut out_buf[..]),
                    1.0,
                    y_cm as i32,
                );
                if !last {
                    norm2[out_at..out_at + n].copy_from_slice(&out_buf);
                }
                r
            };
        } else {
            let lb: Option<Vec<f32>> = lb_range.map(|o| norm[o..o + n].to_vec());
            let mut out_buf = vec![0f32; n];
            let out = if last { None } else { Some(&mut out_buf[..]) };
            if let Some(yb) = y_.as_deref_mut() {
                x_cm = quant_band_stereo(
                    &mut ctx,
                    &mut x_[xo..xo + n],
                    &mut yb[xo..xo + n],
                    n,
                    b,
                    bb,
                    lb.as_deref(),
                    lm,
                    out,
                    (x_cm | y_cm) as i32,
                );
            } else {
                x_cm = quant_band(
                    &mut ctx,
                    &mut x_[xo..xo + n],
                    n,
                    b,
                    bb,
                    lb.as_deref(),
                    lm,
                    out,
                    1.0,
                    (x_cm | y_cm) as i32,
                );
            }
            if !last {
                norm[out_at..out_at + n].copy_from_slice(&out_buf);
            }
            y_cm = x_cm;
        }
        collapse_masks[i * c] = x_cm as u8;
        collapse_masks[i * c + c - 1] = y_cm as u8;
        balance += pulses[i] + tell;
        update_lowband = b > (n as i32) << BITRES;
    }
    *seed = ctx.seed;
}

pub(crate) fn anti_collapse(
    x_: &mut [f32],
    collapse_masks: &[u8],
    lm: i32,
    c: usize,
    size: usize,
    start: usize,
    end: usize,
    log_e: &[f32],
    prev1: &[f32],
    prev2: &[f32],
    pulses: &[i32; NB_EBANDS],
    mut seed: u32,
) {
    for i in start..end {
        let n0 = (eb(i + 1) - eb(i)) as usize;
        let depth = (((1 + pulses[i]) as u32 / (eb(i + 1) - eb(i)) as u32) >> lm) as i32;
        let thresh = 0.5
            * ((0.693_147_180_559_945_3_f64 * f64::from(-0.125f32 * depth as f32)).exp() as f32);
        let sqrt_1 = 1.0 / f64::from((n0 << lm) as f32).sqrt() as f32;
        for ch in 0..c {
            let mut p1 = prev1[ch * NB_EBANDS + i];
            let mut p2 = prev2[ch * NB_EBANDS + i];
            if c == 1 {
                p1 = p1.max(prev1[NB_EBANDS + i]);
                p2 = p2.max(prev2[NB_EBANDS + i]);
            }
            let ediff = (log_e[ch * NB_EBANDS + i] - p1.min(p2)).max(0.0);
            let mut r = 2.0 * ((0.693_147_180_559_945_3_f64 * f64::from(-ediff)).exp() as f32);
            if lm == 3 {
                r *= 1.414_213_5;
            }
            r = thresh.min(r);
            r *= sqrt_1;
            let xo = ch * size + ((eb(i) as usize) << lm);
            let mut renormalize = false;
            for k in 0..1usize << lm {
                if collapse_masks[i * c + ch] & (1 << k) == 0 {
                    for j in 0..n0 {
                        seed = lcg_rand(seed);
                        x_[xo + (j << lm) + k] = if seed & 0x8000 != 0 { r } else { -r };
                    }
                    renormalize = true;
                }
            }
            if renormalize {
                renormalise_vector(&mut x_[xo..], n0 << lm, 1.0);
            }
        }
    }
}

/// `denormalise_bands`: shape × energy into MDCT coefficients.
pub(crate) fn denormalise_bands(
    x: &[f32],
    freq: &mut [f32],
    band_log_e: &[f32],
    mut start: usize,
    mut end: usize,
    m: usize,
    downsample: usize,
    silence: bool,
) {
    let n = m * 120;
    let mut bound = m * eb(end) as usize;
    if downsample != 1 {
        bound = bound.min(n / downsample);
    }
    if silence {
        bound = 0;
        start = 0;
        end = 0;
    }
    let first = m * eb(start) as usize;
    for v in freq.iter_mut().take(first) {
        *v = 0.0;
    }
    let mut f = first;
    let mut xi = first;
    for i in start..end {
        let band_end = m * eb(i + 1) as usize;
        let lg = band_log_e[i] + t::E_MEANS[i];
        let g = (0.693_147_180_559_945_3_f64 * f64::from(32.0f32.min(lg))).exp() as f32;
        while f < band_end {
            freq[f] = x[xi] * g;
            f += 1;
            xi += 1;
        }
    }
    for v in freq.iter_mut().take(n).skip(bound) {
        *v = 0.0;
    }
}
