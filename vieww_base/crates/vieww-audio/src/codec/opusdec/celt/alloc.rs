//! CELT's bit allocation (libopus `rate.c`) and band-energy decoding
//! (`quant_bands.c`, `laplace.c`).

#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use super::super::range::{RangeDecoder, BITRES};
use super::tables as t;
use super::NB_EBANDS;

const ALLOC_STEPS: i32 = 6;
const MAX_FINE_BITS: i32 = 8;
const FINE_OFFSET: i32 = 21;
const LOG_MAX_PSEUDO: usize = 6;
const NB_ALLOC_VECTORS: usize = 11;

pub(crate) fn eb(i: usize) -> i32 {
    i32::from(t::EBANDS[i])
}

fn cache_row(band: usize, lm: i32) -> &'static [u8] {
    let idx = t::CACHE_INDEX[(lm + 1).max(0) as usize * NB_EBANDS + band];
    &t::CACHE_BITS[idx.max(0) as usize..]
}

pub(crate) const fn get_pulses(i: i32) -> i32 {
    if i < 8 {
        i
    } else {
        (8 + (i & 7)) << ((i >> 3) - 1)
    }
}

pub(crate) fn bits2pulses(band: usize, lm: i32, bits: i32) -> i32 {
    let cache = cache_row(band, lm);
    let mut lo = 0i32;
    let mut hi = i32::from(cache[0]);
    let bits = bits - 1;
    for _ in 0..LOG_MAX_PSEUDO {
        let mid = (lo + hi + 1) >> 1;
        if i32::from(cache[mid as usize]) >= bits {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let low = if lo == 0 {
        -1
    } else {
        i32::from(cache[lo as usize])
    };
    if bits - low <= i32::from(cache[hi as usize]) - bits {
        lo
    } else {
        hi
    }
}

pub(crate) fn pulses2bits(band: usize, lm: i32, pulses: i32) -> i32 {
    let cache = cache_row(band, lm);
    if pulses == 0 {
        0
    } else {
        i32::from(cache[pulses as usize]) + 1
    }
}

/// The cache's first entry for a band: the most bits a split can take.
pub(crate) fn cache_max(band: usize, lm: i32) -> i32 {
    let cache = cache_row(band, lm);
    i32::from(cache[cache[0] as usize])
}

pub(crate) fn init_caps(cap: &mut [i32; NB_EBANDS], lm: i32, c: i32) {
    for i in 0..NB_EBANDS {
        let n = (eb(i + 1) - eb(i)) << lm;
        cap[i] =
            (i32::from(t::CACHE_CAPS[NB_EBANDS * (2 * lm + c - 1) as usize + i]) + 64) * c * n >> 2;
    }
}

pub(crate) struct Allocation {
    pub(crate) coded_bands: usize,
    pub(crate) intensity: usize,
    pub(crate) dual_stereo: bool,
    pub(crate) balance: i32,
    pub(crate) pulses: [i32; NB_EBANDS],
    pub(crate) ebits: [i32; NB_EBANDS],
    pub(crate) fine_priority: [i32; NB_EBANDS],
}

#[allow(clippy::cognitive_complexity)]
pub(crate) fn compute_allocation(
    start: usize,
    end: usize,
    offsets: &[i32; NB_EBANDS],
    cap: &[i32; NB_EBANDS],
    alloc_trim: i32,
    total: i32,
    c: i32,
    lm: i32,
    ec: &mut RangeDecoder<'_>,
) -> Allocation {
    let mut total = total.max(0);
    let len = NB_EBANDS;
    let mut skip_start = start;
    let skip_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
    total -= skip_rsv;
    let (mut intensity_rsv, mut dual_stereo_rsv) = (0i32, 0i32);
    if c == 2 {
        intensity_rsv = i32::from(t::LOG2_FRAC[end - start]);
        if intensity_rsv > total {
            intensity_rsv = 0;
        } else {
            total -= intensity_rsv;
            dual_stereo_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
            total -= dual_stereo_rsv;
        }
    }
    let mut bits1 = [0i32; NB_EBANDS];
    let mut bits2 = [0i32; NB_EBANDS];
    let mut thresh = [0i32; NB_EBANDS];
    let mut trim_offset = [0i32; NB_EBANDS];
    for j in start..end {
        let w = eb(j + 1) - eb(j);
        thresh[j] = (c << BITRES).max((3 * w << lm << BITRES) >> 4);
        trim_offset[j] = c
            * w
            * (alloc_trim - 5 - lm)
            * (end as i32 - j as i32 - 1)
            * (1 << (lm + BITRES as i32))
            >> 6;
        if w << lm == 1 {
            trim_offset[j] -= c << BITRES;
        }
    }
    let av = |v: usize, j: usize| i32::from(t::BAND_ALLOCATION[v * len + j]);
    let mut lo = 1i32;
    let mut hi = NB_ALLOC_VECTORS as i32 - 1;
    loop {
        let mut done = false;
        let mut psum = 0i32;
        let mid = (lo + hi) >> 1;
        for j in (start..end).rev() {
            let n = eb(j + 1) - eb(j);
            let mut bitsj = c * n * av(mid as usize, j) << lm >> 2;
            if bitsj > 0 {
                bitsj = (bitsj + trim_offset[j]).max(0);
            }
            bitsj += offsets[j];
            if bitsj >= thresh[j] || done {
                done = true;
                psum += bitsj.min(cap[j]);
            } else if bitsj >= c << BITRES {
                psum += c << BITRES;
            }
        }
        if psum > total {
            hi = mid - 1;
        } else {
            lo = mid + 1;
        }
        if lo > hi {
            break;
        }
    }
    hi = lo;
    lo -= 1;
    for j in start..end {
        let n = eb(j + 1) - eb(j);
        let mut b1 = c * n * av(lo as usize, j) << lm >> 2;
        let mut b2 = if hi as usize >= NB_ALLOC_VECTORS {
            cap[j]
        } else {
            c * n * av(hi as usize, j) << lm >> 2
        };
        if b1 > 0 {
            b1 = (b1 + trim_offset[j]).max(0);
        }
        if b2 > 0 {
            b2 = (b2 + trim_offset[j]).max(0);
        }
        if lo > 0 {
            b1 += offsets[j];
        }
        b2 += offsets[j];
        if offsets[j] > 0 {
            skip_start = j;
        }
        b2 = (b2 - b1).max(0);
        bits1[j] = b1;
        bits2[j] = b2;
    }
    interp_bits2pulses(
        start,
        end,
        skip_start,
        &bits1,
        &bits2,
        &thresh,
        cap,
        total,
        skip_rsv,
        intensity_rsv,
        dual_stereo_rsv,
        c,
        lm,
        ec,
    )
}

fn interp_bits2pulses(
    start: usize,
    end: usize,
    skip_start: usize,
    bits1: &[i32; NB_EBANDS],
    bits2: &[i32; NB_EBANDS],
    thresh: &[i32; NB_EBANDS],
    cap: &[i32; NB_EBANDS],
    mut total: i32,
    skip_rsv: i32,
    mut intensity_rsv: i32,
    mut dual_stereo_rsv: i32,
    c: i32,
    lm: i32,
    ec: &mut RangeDecoder<'_>,
) -> Allocation {
    let alloc_floor = c << BITRES;
    let stereo = i32::from(c > 1);
    let log_m = lm << BITRES;
    let mut bits = [0i32; NB_EBANDS];
    let mut ebits = [0i32; NB_EBANDS];
    let mut fine_priority = [0i32; NB_EBANDS];
    let (mut lo, mut hi) = (0i32, 1i32 << ALLOC_STEPS);
    for _ in 0..ALLOC_STEPS {
        let mid = (lo + hi) >> 1;
        let mut psum = 0i32;
        let mut done = false;
        for j in (start..end).rev() {
            let tmp = bits1[j] + (mid * bits2[j] >> ALLOC_STEPS);
            if tmp >= thresh[j] || done {
                done = true;
                psum += tmp.min(cap[j]);
            } else if tmp >= alloc_floor {
                psum += alloc_floor;
            }
        }
        if psum > total {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let mut psum = 0i32;
    let mut done = false;
    for j in (start..end).rev() {
        let mut tmp = bits1[j] + (lo * bits2[j] >> ALLOC_STEPS);
        if tmp < thresh[j] && !done {
            tmp = if tmp >= alloc_floor { alloc_floor } else { 0 };
        } else {
            done = true;
        }
        tmp = tmp.min(cap[j]);
        bits[j] = tmp;
        psum += tmp;
    }
    let ebw = |a: usize, b: usize| eb(a) - eb(b);
    let mut coded_bands = end;
    loop {
        let j = coded_bands - 1;
        if j <= skip_start {
            total += skip_rsv;
            break;
        }
        let mut left = total - psum;
        let percoeff = (left as u32 / ebw(coded_bands, start) as u32) as i32;
        left -= ebw(coded_bands, start) * percoeff;
        let rem = (left - ebw(j, start)).max(0);
        let band_width = ebw(coded_bands, j);
        let mut band_bits = bits[j] + percoeff * band_width + rem;
        if band_bits >= thresh[j].max(alloc_floor + (1 << BITRES)) {
            if ec.bit_logp(1) {
                break;
            }
            psum += 1 << BITRES;
            band_bits -= 1 << BITRES;
        }
        psum -= bits[j] + intensity_rsv;
        if intensity_rsv > 0 {
            intensity_rsv = i32::from(t::LOG2_FRAC[j - start]);
        }
        psum += intensity_rsv;
        if band_bits >= alloc_floor {
            psum += alloc_floor;
            bits[j] = alloc_floor;
        } else {
            bits[j] = 0;
        }
        coded_bands -= 1;
    }
    let intensity = if intensity_rsv > 0 {
        start + ec.uint((coded_bands + 1 - start) as u32) as usize
    } else {
        0
    };
    if intensity <= start {
        total += dual_stereo_rsv;
        dual_stereo_rsv = 0;
    }
    let dual_stereo = if dual_stereo_rsv > 0 {
        ec.bit_logp(1)
    } else {
        false
    };
    let mut left = total - psum;
    let percoeff = (left as u32 / ebw(coded_bands, start) as u32) as i32;
    left -= ebw(coded_bands, start) * percoeff;
    for j in start..coded_bands {
        bits[j] += percoeff * ebw(j + 1, j);
    }
    for j in start..coded_bands {
        let tmp = left.min(ebw(j + 1, j));
        bits[j] += tmp;
        left -= tmp;
    }
    let mut balance = 0i32;
    let mut j = start;
    while j < coded_bands {
        let n0 = ebw(j + 1, j);
        let n = n0 << lm;
        let bit = bits[j] + balance;
        let mut excess;
        if n > 1 {
            excess = (bit - cap[j]).max(0);
            bits[j] = bit - excess;
            let den = c * n + i32::from(c == 2 && n > 2 && !dual_stereo && j < intensity);
            let nclogn = den * (i32::from(t::LOGN[j]) + log_m);
            let mut offset = (nclogn >> 1) - den * FINE_OFFSET;
            if n == 2 {
                offset += den << BITRES >> 2;
            }
            if bits[j] + offset < den * 2 << BITRES {
                offset += nclogn >> 2;
            } else if bits[j] + offset < den * 3 << BITRES {
                offset += nclogn >> 3;
            }
            ebits[j] = (bits[j] + offset + (den << (BITRES - 1))).max(0);
            ebits[j] = ((ebits[j] as u32 / den as u32) >> BITRES) as i32;
            if c * ebits[j] > bits[j] >> BITRES {
                ebits[j] = bits[j] >> stereo >> BITRES;
            }
            ebits[j] = ebits[j].min(MAX_FINE_BITS);
            fine_priority[j] = i32::from(ebits[j] * (den << BITRES) >= bits[j] + offset);
            bits[j] -= c * ebits[j] << BITRES;
        } else {
            excess = (bit - (c << BITRES)).max(0);
            bits[j] = bit - excess;
            ebits[j] = 0;
            fine_priority[j] = 1;
        }
        if excess > 0 {
            let extra_fine = (excess >> (stereo + BITRES as i32)).min(MAX_FINE_BITS - ebits[j]);
            ebits[j] += extra_fine;
            let extra_bits = extra_fine * c << BITRES;
            fine_priority[j] = i32::from(extra_bits >= excess - balance);
            excess -= extra_bits;
        }
        balance = excess;
        j += 1;
    }
    while j < end {
        ebits[j] = bits[j] >> stereo >> BITRES;
        bits[j] = 0;
        fine_priority[j] = i32::from(ebits[j] < 1);
        j += 1;
    }
    Allocation {
        coded_bands,
        intensity,
        dual_stereo,
        balance,
        pulses: bits,
        ebits,
        fine_priority,
    }
}

// ---------------------------------------------------------------- energy

const LAPLACE_MINP: u32 = 1;
const LAPLACE_NMIN: u32 = 16;

fn laplace_freq1(fs0: u32, decay: i32) -> u32 {
    let ft = 32768 - LAPLACE_MINP * (2 * LAPLACE_NMIN) - fs0;
    ((ft as i32 * (16384 - decay)) >> 15) as u32
}

fn laplace_decode(dec: &mut RangeDecoder<'_>, mut fs: u32, decay: i32) -> i32 {
    let mut val = 0i32;
    let fm = dec.decode_bin(15);
    let mut fl = 0u32;
    if fm >= fs {
        val += 1;
        fl = fs;
        fs = laplace_freq1(fs, decay) + LAPLACE_MINP;
        while fs > LAPLACE_MINP && fm >= fl + 2 * fs {
            fs *= 2;
            fl += fs;
            fs = (((fs - 2 * LAPLACE_MINP) as i32 * decay) >> 15) as u32;
            fs += LAPLACE_MINP;
            val += 1;
        }
        if fs <= LAPLACE_MINP {
            let di = (fm - fl) >> 1;
            val += di as i32;
            fl += 2 * di * LAPLACE_MINP;
        }
        if fm < fl + fs {
            val = -val;
        } else {
            fl += fs;
        }
    }
    dec.update(fl, (fl + fs).min(32768), 32768);
    val
}

pub(crate) fn unquant_coarse_energy(
    start: usize,
    end: usize,
    old: &mut [f32],
    intra: bool,
    dec: &mut RangeDecoder<'_>,
    c: usize,
    lm: usize,
) {
    const SMALL_ENERGY_ICDF: [u8; 3] = [2, 1, 0];
    let prob = &t::E_PROB_MODEL[(lm * 2 + usize::from(intra)) * 42..][..42];
    let mut prev = [0f32; 2];
    let (coef, beta) = if intra {
        (0.0f32, 4915.0f64 / 32768.0)
    } else {
        (t::PRED_COEF[lm], f64::from(t::BETA_COEF[lm]))
    };
    let beta = beta as f32;
    let budget = dec.storage() as i32 * 8;
    for i in start..end {
        for ch in 0..c {
            let tell = dec.tell();
            let qi = if budget - tell >= 15 {
                let pi = 2 * i.min(20);
                laplace_decode(dec, u32::from(prob[pi]) << 7, i32::from(prob[pi + 1]) << 6)
            } else if budget - tell >= 2 {
                let q = dec.icdf(&SMALL_ENERGY_ICDF, 2) as i32;
                (q >> 1) ^ -(q & 1)
            } else if budget - tell >= 1 {
                -i32::from(dec.bit_logp(1))
            } else {
                -1
            };
            let q = qi as f32;
            let idx = i + ch * NB_EBANDS;
            old[idx] = old[idx].max(-9.0);
            let tmp = coef * old[idx] + prev[ch] + q;
            old[idx] = tmp;
            prev[ch] = prev[ch] + q - beta * q;
        }
    }
}

pub(crate) fn unquant_fine_energy(
    start: usize,
    end: usize,
    old: &mut [f32],
    fine_quant: &[i32; NB_EBANDS],
    dec: &mut RangeDecoder<'_>,
    c: usize,
) {
    for i in start..end {
        if fine_quant[i] <= 0 {
            continue;
        }
        for ch in 0..c {
            let q2 = dec.bits(fine_quant[i] as u32);
            let offset =
                (q2 as f32 + 0.5) * (1 << (14 - fine_quant[i])) as f32 * (1.0 / 16384.0) - 0.5;
            old[i + ch * NB_EBANDS] += offset;
        }
    }
}

pub(crate) fn unquant_energy_finalise(
    start: usize,
    end: usize,
    old: &mut [f32],
    fine_quant: &[i32; NB_EBANDS],
    fine_priority: &[i32; NB_EBANDS],
    mut bits_left: i32,
    dec: &mut RangeDecoder<'_>,
    c: usize,
) {
    for prio in 0..2 {
        let mut i = start;
        while i < end && bits_left >= c as i32 {
            if fine_quant[i] >= MAX_FINE_BITS || fine_priority[i] != prio {
                i += 1;
                continue;
            }
            for ch in 0..c {
                let q2 = dec.bits(1);
                let offset =
                    (q2 as f32 - 0.5) * (1 << (14 - fine_quant[i] - 1)) as f32 * (1.0 / 16384.0);
                old[i + ch * NB_EBANDS] += offset;
                bits_left -= 1;
            }
            i += 1;
        }
    }
}

pub(crate) fn tf_decode(
    start: usize,
    end: usize,
    is_transient: bool,
    tf_res: &mut [i32; NB_EBANDS],
    lm: usize,
    dec: &mut RangeDecoder<'_>,
) {
    const TF_SELECT: [[i8; 8]; 4] = [
        [0, -1, 0, -1, 0, -1, 0, -1],
        [0, -1, 0, -2, 1, 0, 1, -1],
        [0, -2, 0, -3, 2, 0, 1, -1],
        [0, -2, 0, -3, 3, 0, 1, -1],
    ];
    let mut budget = dec.storage() * 8;
    let mut tell = dec.tell() as u32;
    let mut logp: u32 = if is_transient { 2 } else { 4 };
    let tf_select_rsv = lm > 0 && tell + logp + 1 <= budget;
    budget -= u32::from(tf_select_rsv);
    let mut tf_changed = 0i32;
    let mut curr = 0i32;
    for i in start..end {
        if tell + logp <= budget {
            curr ^= i32::from(dec.bit_logp(logp));
            tell = dec.tell() as u32;
            tf_changed |= curr;
        }
        tf_res[i] = curr;
        logp = if is_transient { 4 } else { 5 };
    }
    let tr = 4 * usize::from(is_transient);
    let mut tf_select = 0usize;
    if tf_select_rsv
        && TF_SELECT[lm][tr + tf_changed as usize] != TF_SELECT[lm][tr + 2 + tf_changed as usize]
    {
        tf_select = usize::from(dec.bit_logp(1));
    }
    for i in start..end {
        tf_res[i] = i32::from(TF_SELECT[lm][tr + 2 * tf_select + tf_res[i] as usize]);
    }
}
