//! CELT (RFC 6716 §4.3; libopus `celt_decoder.c`): the MDCT layer, for the
//! standard 48 kHz / 20 ms mode only (`opus_custom_mode_create(48000, 960)`
//! — the one Opus uses), float build.

#![allow(
    clippy::needless_range_loop,
    clippy::too_many_lines,
    clippy::too_many_arguments
)]

mod alloc;
mod bands;
pub(crate) mod dsp;
pub(crate) mod tables;

use super::range::{RangeDecoder, BITRES};
use alloc::eb;

pub(crate) const NB_EBANDS: usize = 21;
const OVERLAP: usize = 120;
const DECODE_BUFFER_SIZE: usize = 2048;
const MAX_PERIOD: usize = 1024;
const LPC_ORDER: usize = 24;
const PLC_PITCH_LAG_MAX: usize = 720;
const PLC_PITCH_LAG_MIN: usize = 100;
const SHORT_MDCT_SIZE: usize = 120;
const MAX_LM: usize = 3;
const PREEMPH: f32 = 0.850_006_1;

/// One CELT decoder (one or two channels).
#[derive(Clone)]
pub(crate) struct Celt {
    channels: usize,
    pub(crate) stream_channels: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
    disable_inv: bool,
    pub(crate) rng: u32,
    pub(crate) error: bool,
    last_pitch_index: usize,
    loss_count: u32,
    skip_plc: bool,
    postfilter_period: i32,
    postfilter_period_old: i32,
    postfilter_gain: f32,
    postfilter_gain_old: f32,
    postfilter_tapset: usize,
    postfilter_tapset_old: usize,
    preemph_mem: [f32; 2],
    decode_mem: [Vec<f32>; 2],
    lpc: [[f32; LPC_ORDER]; 2],
    old_band_e: [f32; 2 * NB_EBANDS],
    old_log_e: [f32; 2 * NB_EBANDS],
    old_log_e2: [f32; 2 * NB_EBANDS],
    background_log_e: [f32; 2 * NB_EBANDS],
}

impl Celt {
    pub(crate) fn new(channels: usize) -> Self {
        let mut c = Self {
            channels,
            stream_channels: channels,
            start: 0,
            end: NB_EBANDS,
            disable_inv: channels == 1,
            rng: 0,
            error: false,
            last_pitch_index: 0,
            loss_count: 0,
            skip_plc: false,
            postfilter_period: 0,
            postfilter_period_old: 0,
            postfilter_gain: 0.0,
            postfilter_gain_old: 0.0,
            postfilter_tapset: 0,
            postfilter_tapset_old: 0,
            preemph_mem: [0.0; 2],
            decode_mem: [
                vec![0.0; DECODE_BUFFER_SIZE + OVERLAP],
                vec![0.0; DECODE_BUFFER_SIZE + OVERLAP],
            ],
            lpc: [[0.0; LPC_ORDER]; 2],
            old_band_e: [0.0; 2 * NB_EBANDS],
            old_log_e: [0.0; 2 * NB_EBANDS],
            old_log_e2: [0.0; 2 * NB_EBANDS],
            background_log_e: [0.0; 2 * NB_EBANDS],
        };
        c.reset();
        c
    }

    /// `OPUS_RESET_STATE`.
    pub(crate) fn reset(&mut self) {
        self.rng = 0;
        self.error = false;
        self.last_pitch_index = 0;
        self.loss_count = 0;
        self.postfilter_period = 0;
        self.postfilter_period_old = 0;
        self.postfilter_gain = 0.0;
        self.postfilter_gain_old = 0.0;
        self.postfilter_tapset = 0;
        self.postfilter_tapset_old = 0;
        self.preemph_mem = [0.0; 2];
        for m in &mut self.decode_mem {
            m.iter_mut().for_each(|v| *v = 0.0);
        }
        self.lpc = [[0.0; LPC_ORDER]; 2];
        self.old_band_e = [0.0; 2 * NB_EBANDS];
        self.old_log_e = [-28.0; 2 * NB_EBANDS];
        self.old_log_e2 = [-28.0; 2 * NB_EBANDS];
        self.background_log_e = [0.0; 2 * NB_EBANDS];
        self.skip_plc = true;
    }

    fn synthesis(
        &mut self,
        x: &[f32],
        out_at: usize,
        c_stream: usize,
        is_transient: bool,
        lm: usize,
        silence: bool,
        start: usize,
        eff_end: usize,
    ) {
        let cc = self.channels;
        let n = SHORT_MDCT_SIZE << lm;
        let m = 1usize << lm;
        let (b, nb, shift) = if is_transient {
            (m, SHORT_MDCT_SIZE, MAX_LM)
        } else {
            (1, SHORT_MDCT_SIZE << lm, MAX_LM - lm)
        };
        let mut freq = vec![0f32; n];
        let run = |mem: &mut Vec<f32>, freq: &[f32]| {
            for bi in 0..b {
                dsp::mdct_backward(&freq[bi..], &mut mem[out_at + nb * bi..], OVERLAP, shift, b);
            }
        };
        if cc == 2 && c_stream == 1 {
            bands::denormalise_bands(
                x,
                &mut freq,
                &self.old_band_e,
                start,
                eff_end,
                m,
                1,
                silence,
            );
            let freq2 = freq.clone();
            run(&mut self.decode_mem[0], &freq2);
            run(&mut self.decode_mem[1], &freq);
        } else if cc == 1 && c_stream == 2 {
            let mut freq2 = vec![0f32; n];
            bands::denormalise_bands(
                x,
                &mut freq,
                &self.old_band_e,
                start,
                eff_end,
                m,
                1,
                silence,
            );
            bands::denormalise_bands(
                &x[n..],
                &mut freq2,
                &self.old_band_e[NB_EBANDS..],
                start,
                eff_end,
                m,
                1,
                silence,
            );
            for i in 0..n {
                freq[i] = 0.5 * freq[i] + 0.5 * freq2[i];
            }
            run(&mut self.decode_mem[0], &freq);
        } else {
            for c in 0..cc {
                bands::denormalise_bands(
                    &x[c * n..],
                    &mut freq,
                    &self.old_band_e[c * NB_EBANDS..],
                    start,
                    eff_end,
                    m,
                    1,
                    silence,
                );
                run(&mut self.decode_mem[c], &freq);
            }
        }
    }

    fn deemphasis(&mut self, out_at: usize, pcm: &mut [f32], n: usize) {
        let cc = self.channels;
        for c in 0..cc {
            let mut m = self.preemph_mem[c];
            let x = &self.decode_mem[c][out_at..];
            for j in 0..n {
                let tmp = x[j] + 1e-30 + m;
                m = PREEMPH * tmp;
                pcm[j * cc + c] = tmp * (1.0 / 32768.0);
            }
            self.preemph_mem[c] = m;
        }
    }

    fn decode_lost(&mut self, n: usize, lm: usize) {
        let cc = self.channels;
        let out_at = DECODE_BUFFER_SIZE - n;
        let noise_based = self.loss_count >= 5 || self.start != 0 || self.skip_plc;
        if noise_based {
            let end = self.end;
            let start = self.start;
            let eff_end = start.max(end.min(NB_EBANDS));
            let mut x = vec![0f32; cc * n];
            let decay = if self.loss_count == 0 { 1.5f32 } else { 0.5 };
            for c in 0..cc {
                for i in start..end {
                    let k = c * NB_EBANDS + i;
                    self.old_band_e[k] = self.background_log_e[k].max(self.old_band_e[k] - decay);
                }
            }
            let mut seed = self.rng;
            for c in 0..cc {
                for i in start..eff_end {
                    let boffs = n * c + ((eb(i) as usize) << lm);
                    let blen = ((eb(i + 1) - eb(i)) as usize) << lm;
                    for j in 0..blen {
                        seed = bands::lcg_rand(seed);
                        x[boffs + j] = ((seed as i32) >> 20) as f32;
                    }
                    bands::renormalise_vector(&mut x[boffs..], blen, 1.0);
                }
            }
            self.rng = seed;
            for c in 0..cc {
                self.decode_mem[c].copy_within(n..DECODE_BUFFER_SIZE + (OVERLAP >> 1), 0);
            }
            self.synthesis(&x, out_at, cc, false, lm, false, start, eff_end);
        } else {
            let mut fade = 1.0f32;
            let pitch_index = if self.loss_count == 0 {
                let mut lp = vec![0f32; DECODE_BUFFER_SIZE >> 1];
                {
                    let mem: [&[f32]; 2] =
                        [&self.decode_mem[0], &self.decode_mem[usize::from(cc == 2)]];
                    dsp::pitch_downsample(mem, &mut lp, DECODE_BUFFER_SIZE, cc);
                }
                let p = dsp::pitch_search(
                    &lp[PLC_PITCH_LAG_MAX >> 1..],
                    &lp,
                    DECODE_BUFFER_SIZE - PLC_PITCH_LAG_MAX,
                    PLC_PITCH_LAG_MAX - PLC_PITCH_LAG_MIN,
                );
                let p = PLC_PITCH_LAG_MAX - p;
                self.last_pitch_index = p;
                p
            } else {
                fade = 0.8;
                self.last_pitch_index
            };
            let exc_length = (2 * pitch_index).min(MAX_PERIOD);
            let window = &tables::WINDOW;
            for c in 0..cc {
                let buf = &mut self.decode_mem[c];
                let mut exc_full = [0f32; MAX_PERIOD + LPC_ORDER];
                for i in 0..MAX_PERIOD + LPC_ORDER {
                    exc_full[i] = buf[DECODE_BUFFER_SIZE - MAX_PERIOD - LPC_ORDER + i];
                }
                if self.loss_count == 0 {
                    let mut ac = [0f32; LPC_ORDER + 1];
                    dsp::autocorr(
                        &exc_full[LPC_ORDER..],
                        &mut ac,
                        Some(window),
                        OVERLAP,
                        LPC_ORDER,
                        MAX_PERIOD,
                    );
                    ac[0] *= 1.0001;
                    for i in 1..=LPC_ORDER {
                        ac[i] -= ac[i] * (0.008 * 0.008) * i as f32 * i as f32;
                    }
                    dsp::lpc(&mut self.lpc[c], &ac, LPC_ORDER);
                }
                {
                    let mut fir_tmp = vec![0f32; exc_length];
                    let off = LPC_ORDER + MAX_PERIOD - exc_length;
                    dsp::fir(
                        &exc_full,
                        off,
                        &self.lpc[c],
                        &mut fir_tmp,
                        exc_length,
                        LPC_ORDER,
                    );
                    exc_full[off..off + exc_length].copy_from_slice(&fir_tmp);
                }
                let exc = &exc_full[LPC_ORDER..];
                let decay;
                {
                    let (mut e1, mut e2) = (1f32, 1f32);
                    let decay_length = exc_length >> 1;
                    for i in 0..decay_length {
                        let e = exc[MAX_PERIOD - decay_length + i];
                        e1 += e * e;
                        let e = exc[MAX_PERIOD - 2 * decay_length + i];
                        e2 += e * e;
                    }
                    e1 = e1.min(e2);
                    // SHR32 is the identity in the float build.
                    decay = f64::from(e1 / e2).sqrt() as f32;
                }
                buf.copy_within(n..DECODE_BUFFER_SIZE, 0);
                let extrapolation_offset = MAX_PERIOD - pitch_index;
                let extrapolation_len = n + OVERLAP;
                let mut attenuation = fade * decay;
                let mut s1 = 0f32;
                let mut j = 0usize;
                for i in 0..extrapolation_len {
                    if j >= pitch_index {
                        j -= pitch_index;
                        attenuation *= decay;
                    }
                    buf[DECODE_BUFFER_SIZE - n + i] = attenuation * exc[extrapolation_offset + j];
                    let tmp = buf[DECODE_BUFFER_SIZE - MAX_PERIOD - n + extrapolation_offset + j];
                    s1 += tmp * tmp;
                    j += 1;
                }
                {
                    let mut lpc_mem = [0f32; LPC_ORDER];
                    for i in 0..LPC_ORDER {
                        lpc_mem[i] = buf[DECODE_BUFFER_SIZE - n - 1 - i];
                    }
                    let src: Vec<f32> = buf
                        [DECODE_BUFFER_SIZE - n..DECODE_BUFFER_SIZE - n + extrapolation_len]
                        .to_vec();
                    let mut out = vec![0f32; extrapolation_len];
                    dsp::iir(
                        &src,
                        &self.lpc[c],
                        &mut out,
                        extrapolation_len,
                        LPC_ORDER,
                        &mut lpc_mem,
                    );
                    buf[DECODE_BUFFER_SIZE - n..DECODE_BUFFER_SIZE - n + extrapolation_len]
                        .copy_from_slice(&out);
                }
                {
                    let mut s2 = 0f32;
                    for i in 0..extrapolation_len {
                        let tmp = buf[DECODE_BUFFER_SIZE - n + i];
                        s2 += tmp * tmp;
                    }
                    if !(s1 > 0.2 * s2) {
                        for i in 0..extrapolation_len {
                            buf[DECODE_BUFFER_SIZE - n + i] = 0.0;
                        }
                    } else if s1 < s2 {
                        let ratio = f64::from((s1 + 1.0) / (s2 + 1.0)).sqrt() as f32;
                        for i in 0..OVERLAP {
                            let tmp_g = 1.0 - window[i] * (1.0 - ratio);
                            buf[DECODE_BUFFER_SIZE - n + i] *= tmp_g;
                        }
                        for i in OVERLAP..extrapolation_len {
                            buf[DECODE_BUFFER_SIZE - n + i] *= ratio;
                        }
                    }
                }
                let mut etmp = vec![0f32; OVERLAP];
                {
                    let src: Vec<f32> = buf.clone();
                    dsp::comb_filter(
                        &mut etmp,
                        0,
                        Some(&src),
                        DECODE_BUFFER_SIZE,
                        self.postfilter_period,
                        self.postfilter_period,
                        OVERLAP,
                        -self.postfilter_gain,
                        -self.postfilter_gain,
                        self.postfilter_tapset,
                        self.postfilter_tapset,
                        None,
                        0,
                    );
                }
                for i in 0..OVERLAP / 2 {
                    buf[DECODE_BUFFER_SIZE + i] =
                        window[i] * etmp[OVERLAP - 1 - i] + window[OVERLAP - i - 1] * etmp[i];
                }
            }
        }
        self.loss_count += 1;
    }

    /// `celt_decode_with_ec`. `data == None` is a lost frame (concealed).
    /// `pcm` receives `frame_size × channels` interleaved samples.
    pub(crate) fn decode<'a>(
        &mut self,
        data: Option<&'a [u8]>,
        pcm: &mut [f32],
        frame_size: usize,
        dec: Option<&mut RangeDecoder<'a>>,
    ) -> Result<usize, ()> {
        let cc = self.channels;
        let c = self.stream_channels;
        let start = self.start;
        let end = self.end;
        let mut lm = 0usize;
        while lm <= MAX_LM && SHORT_MDCT_SIZE << lm != frame_size {
            lm += 1;
        }
        if lm > MAX_LM {
            return Err(());
        }
        let m = 1usize << lm;
        let n = m * SHORT_MDCT_SIZE;
        let len = data.map_or(0, <[u8]>::len);
        if len > 1275 {
            return Err(());
        }
        let out_at = DECODE_BUFFER_SIZE - n;
        let eff_end = end.min(NB_EBANDS);
        let data = match data {
            Some(d) if len > 1 => d,
            _ => {
                self.decode_lost(n, lm);
                self.deemphasis(out_at, pcm, n);
                return Ok(frame_size);
            }
        };
        self.skip_plc = self.loss_count != 0;
        let mut own;
        let dec: &mut RangeDecoder<'a> = match dec {
            Some(d) => d,
            None => {
                own = RangeDecoder::new(data);
                &mut own
            }
        };
        if c == 1 {
            for i in 0..NB_EBANDS {
                self.old_band_e[i] = self.old_band_e[i].max(self.old_band_e[NB_EBANDS + i]);
            }
        }
        let mut total_bits = len as i32 * 8;
        let mut tell = dec.tell();
        let silence = if tell >= total_bits {
            true
        } else if tell == 1 {
            dec.bit_logp(15)
        } else {
            false
        };
        if silence {
            tell = len as i32 * 8;
            dec.add_bits(tell - dec.tell());
        }
        let mut postfilter_gain = 0f32;
        let mut postfilter_pitch = 0i32;
        let mut postfilter_tapset = 0usize;
        if start == 0 && tell + 16 <= total_bits {
            if dec.bit_logp(1) {
                let octave = dec.uint(6) as i32;
                postfilter_pitch = (16 << octave) + dec.bits(4 + octave as u32) as i32 - 1;
                let qg = dec.bits(3) as i32;
                if dec.tell() + 2 <= total_bits {
                    postfilter_tapset = dec.icdf(&[2, 1, 0], 2);
                }
                postfilter_gain = 0.093_75 * (qg + 1) as f32;
            }
            tell = dec.tell();
        }
        let is_transient = if lm > 0 && tell + 3 <= total_bits {
            let t = dec.bit_logp(3);
            tell = dec.tell();
            t
        } else {
            false
        };
        let intra_ener = if tell + 3 <= total_bits {
            dec.bit_logp(3)
        } else {
            false
        };
        alloc::unquant_coarse_energy(start, end, &mut self.old_band_e, intra_ener, dec, c, lm);
        let mut tf_res = [0i32; NB_EBANDS];
        alloc::tf_decode(start, end, is_transient, &mut tf_res, lm, dec);
        tell = dec.tell();
        let mut spread_decision = 2i32;
        if tell + 4 <= total_bits {
            spread_decision = dec.icdf(&[25, 23, 2, 0], 5) as i32;
        }
        let mut cap = [0i32; NB_EBANDS];
        alloc::init_caps(&mut cap, lm as i32, c as i32);
        let mut offsets = [0i32; NB_EBANDS];
        let mut dynalloc_logp = 6i32;
        total_bits <<= BITRES;
        let mut tellf = dec.tell_frac() as i32;
        for i in start..end {
            let width = (c as i32 * (eb(i + 1) - eb(i))) << lm;
            let quanta = (width << BITRES).min((6 << BITRES).max(width));
            let mut loop_logp = dynalloc_logp;
            let mut boost = 0;
            while tellf + (loop_logp << BITRES) < total_bits && boost < cap[i] {
                let flag = dec.bit_logp(loop_logp as u32);
                tellf = dec.tell_frac() as i32;
                if !flag {
                    break;
                }
                boost += quanta;
                total_bits -= quanta;
                loop_logp = 1;
            }
            offsets[i] = boost;
            if boost > 0 {
                dynalloc_logp = 2.max(dynalloc_logp - 1);
            }
        }
        let alloc_trim = if tellf + (6 << BITRES) <= total_bits {
            dec.icdf(&[126, 124, 119, 109, 87, 41, 19, 9, 4, 2, 0], 7) as i32
        } else {
            5
        };
        let mut bits = ((len as i32 * 8) << BITRES) - dec.tell_frac() as i32 - 1;
        let anti_collapse_rsv = if is_transient && lm >= 2 && bits >= ((lm as i32 + 2) << BITRES) {
            1 << BITRES
        } else {
            0
        };
        bits -= anti_collapse_rsv;
        let al = alloc::compute_allocation(
            start, end, &offsets, &cap, alloc_trim, bits, c as i32, lm as i32, dec,
        );
        alloc::unquant_fine_energy(start, end, &mut self.old_band_e, &al.ebits, dec, c);
        for ch in 0..cc {
            self.decode_mem[ch].copy_within(n..DECODE_BUFFER_SIZE + OVERLAP / 2, 0);
        }
        let mut collapse_masks = vec![0u8; c * NB_EBANDS];
        let mut x = vec![0f32; c * n];
        {
            let (xa, xb) = x.split_at_mut(n);
            bands::decode_all_bands(
                start,
                end,
                xa,
                if c == 2 { Some(xb) } else { None },
                &mut collapse_masks,
                &al.pulses,
                is_transient,
                spread_decision,
                al.dual_stereo,
                al.intensity,
                &tf_res,
                len as i32 * (8 << BITRES) - anti_collapse_rsv,
                al.balance,
                dec,
                lm as i32,
                al.coded_bands,
                &mut self.rng,
                self.disable_inv,
            );
        }
        let mut anti_collapse_on = false;
        if anti_collapse_rsv > 0 {
            anti_collapse_on = dec.bits(1) == 1;
        }
        alloc::unquant_energy_finalise(
            start,
            end,
            &mut self.old_band_e,
            &al.ebits,
            &al.fine_priority,
            len as i32 * 8 - dec.tell(),
            dec,
            c,
        );
        if anti_collapse_on {
            bands::anti_collapse(
                &mut x,
                &collapse_masks,
                lm as i32,
                c,
                n,
                start,
                end,
                &self.old_band_e,
                &self.old_log_e,
                &self.old_log_e2,
                &al.pulses,
                self.rng,
            );
        }
        if silence {
            for v in self.old_band_e.iter_mut().take(c * NB_EBANDS) {
                *v = -28.0;
            }
        }
        self.synthesis(&x, out_at, c, is_transient, lm, silence, start, eff_end);
        for ch in 0..cc {
            self.postfilter_period = self.postfilter_period.max(dsp::COMBFILTER_MINPERIOD);
            self.postfilter_period_old = self.postfilter_period_old.max(dsp::COMBFILTER_MINPERIOD);
            let w = &tables::WINDOW[..];
            let mem = &mut self.decode_mem[ch];
            dsp::comb_filter(
                mem,
                out_at,
                None,
                out_at,
                self.postfilter_period_old,
                self.postfilter_period,
                SHORT_MDCT_SIZE,
                self.postfilter_gain_old,
                self.postfilter_gain,
                self.postfilter_tapset_old,
                self.postfilter_tapset,
                Some(w),
                OVERLAP,
            );
            if lm != 0 {
                dsp::comb_filter(
                    mem,
                    out_at + SHORT_MDCT_SIZE,
                    None,
                    out_at + SHORT_MDCT_SIZE,
                    self.postfilter_period,
                    postfilter_pitch,
                    n - SHORT_MDCT_SIZE,
                    self.postfilter_gain,
                    postfilter_gain,
                    self.postfilter_tapset,
                    postfilter_tapset,
                    Some(w),
                    OVERLAP,
                );
            }
        }
        self.postfilter_period_old = self.postfilter_period;
        self.postfilter_gain_old = self.postfilter_gain;
        self.postfilter_tapset_old = self.postfilter_tapset;
        self.postfilter_period = postfilter_pitch;
        self.postfilter_gain = postfilter_gain;
        self.postfilter_tapset = postfilter_tapset;
        if lm != 0 {
            self.postfilter_period_old = self.postfilter_period;
            self.postfilter_gain_old = self.postfilter_gain;
            self.postfilter_tapset_old = self.postfilter_tapset;
        }
        if c == 1 {
            let (a, b) = self.old_band_e.split_at_mut(NB_EBANDS);
            b.copy_from_slice(a);
        }
        if is_transient {
            for i in 0..2 * NB_EBANDS {
                self.old_log_e[i] = self.old_log_e[i].min(self.old_band_e[i]);
            }
        } else {
            self.old_log_e2 = self.old_log_e;
            self.old_log_e = self.old_band_e;
            let max_inc = if self.loss_count < 10 {
                m as f32 * 0.001
            } else {
                1.0
            };
            for i in 0..2 * NB_EBANDS {
                self.background_log_e[i] =
                    (self.background_log_e[i] + max_inc).min(self.old_band_e[i]);
            }
        }
        for ch in 0..2 {
            for i in (0..start).chain(end..NB_EBANDS) {
                let k = ch * NB_EBANDS + i;
                self.old_band_e[k] = 0.0;
                self.old_log_e[k] = -28.0;
                self.old_log_e2[k] = -28.0;
            }
        }
        self.rng = dec.rng();
        self.deemphasis(out_at, pcm, n);
        self.loss_count = 0;
        if dec.tell() > 8 * len as i32 {
            return Err(());
        }
        if dec.error {
            self.error = true;
        }
        Ok(frame_size)
    }
}
