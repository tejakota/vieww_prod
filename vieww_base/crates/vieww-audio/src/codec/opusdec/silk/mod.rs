//! SILK, the linear-prediction layer of Opus (RFC 6716 §4.2), ported
//! from libopus 1.3's fixed-point decoder (`dec_API.c` and the files it
//! calls). SILK is integer arithmetic end to end, so the port is
//! bit-exact: the same packets give the same 16-bit samples libopus
//! gives, which the tests check sample for sample.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_range_loop,
    clippy::similar_names
)]

mod fix;
mod lpc;
mod resampler;
#[rustfmt::skip]
mod tables;

use fix::{
    div32_varq, inverse32_varq, log2lin, lshift_sat32, rand, rshift_round, sat16, smlabb, smlawb,
    smulbb, smultt, smulwb, smulww, sqrt_approx,
};
use lpc::{
    bwexpander, lpc_analysis_filter, lpc_inverse_pred_gain, nlsf2a, nlsf_decode, nlsf_unpack,
    sum_sqr_shift, NlsfCb,
};
use resampler::Resampler;
use tables as t;

use super::range::RangeDecoder;

const MAX_LPC_ORDER: usize = 16;
const MAX_FRAME_LENGTH: usize = 320;
const MAX_SUB_FRAME_LENGTH: usize = 80;
const LTP_ORDER: usize = 5;
const TYPE_NO_VOICE_ACTIVITY: i32 = 0;
const TYPE_VOICED: i32 = 2;
const CODE_INDEPENDENTLY: i32 = 0;
const CODE_INDEPENDENTLY_NO_LTP_SCALING: i32 = 1;
const CODE_CONDITIONALLY: i32 = 2;
const FLAG_DECODE_NORMAL: i32 = 0;
const FLAG_PACKET_LOST: i32 = 1;
const FLAG_DECODE_LBRR: i32 = 2;
const BWE_AFTER_LOSS_Q16: i32 = 63570;
const CNG_GAIN_SMTH_Q16: i32 = 4634;
const CNG_GAIN_SMTH_THRESHOLD_Q16: i32 = 46396;
const CNG_NLSF_SMTH_Q16: i32 = 16348;
const API_HZ: i32 = 48_000;

#[derive(Clone, Copy, Default)]
struct Indices {
    gains: [i8; 4],
    ltp: [i8; 4],
    nlsf: [i8; 17],
    lag: i16,
    contour: i8,
    signal_type: i8,
    quant_offset_type: i8,
    nlsf_interp_coef_q2: i8,
    per: i8,
    ltp_scale: i8,
    seed: i8,
}

#[derive(Clone, Copy, Default)]
struct Ctrl {
    pitch_l: [i32; 4],
    gains_q16: [i32; 4],
    pred_coef_q12: [[i16; MAX_LPC_ORDER]; 2],
    ltp_coef_q14: [i16; LTP_ORDER * 4],
    ltp_scale_q14: i32,
}

#[derive(Clone, Copy)]
struct Plc {
    pitch_l_q8: i32,
    ltp_coef_q14: [i16; LTP_ORDER],
    prev_lpc_q12: [i16; MAX_LPC_ORDER],
    last_frame_lost: bool,
    rand_seed: i32,
    rand_scale_q14: i16,
    conc_energy: i32,
    conc_energy_shift: i32,
    prev_ltp_scale_q14: i16,
    prev_gain_q16: [i32; 2],
    fs_khz: i32,
    nb_subfr: usize,
    subfr_length: usize,
}

#[derive(Clone, Copy)]
struct Cng {
    exc_buf_q14: [i32; MAX_FRAME_LENGTH],
    smth_nlsf_q15: [i16; MAX_LPC_ORDER],
    synth_state: [i32; MAX_LPC_ORDER],
    smth_gain_q16: i32,
    rand_seed: i32,
    fs_khz: i32,
}

/// `silk_decoder_state`: one coded channel.
#[derive(Clone)]
struct Channel {
    prev_gain_q16: i32,
    exc_q14: [i32; MAX_FRAME_LENGTH],
    slpc_q14_buf: [i32; MAX_LPC_ORDER],
    out_buf: [i16; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH],
    lag_prev: i32,
    last_gain_index: i8,
    fs_khz: i32,
    fs_api_hz: i32,
    nb_subfr: usize,
    frame_length: usize,
    subfr_length: usize,
    ltp_mem_length: usize,
    lpc_order: usize,
    prev_nlsf_q15: [i16; MAX_LPC_ORDER],
    first_frame_after_reset: bool,
    pitch_lag_low_bits_icdf: &'static [u8],
    pitch_contour_icdf: &'static [u8],
    n_frames_decoded: usize,
    n_frames_per_packet: usize,
    ec_prev_signal_type: i32,
    ec_prev_lag_index: i16,
    vad_flags: [bool; 3],
    lbrr_flag: bool,
    lbrr_flags: [bool; 3],
    resampler: Resampler,
    nlsf_cb: &'static NlsfCb,
    indices: Indices,
    cng: Cng,
    loss_cnt: i32,
    prev_signal_type: i32,
    plc: Plc,
}

impl Channel {
    /// `silk_init_decoder`.
    fn new() -> Self {
        let mut c = Self {
            prev_gain_q16: 65536,
            exc_q14: [0; MAX_FRAME_LENGTH],
            slpc_q14_buf: [0; MAX_LPC_ORDER],
            out_buf: [0; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH],
            lag_prev: 0,
            last_gain_index: 0,
            fs_khz: 0,
            fs_api_hz: 0,
            nb_subfr: 0,
            frame_length: 0,
            subfr_length: 0,
            ltp_mem_length: 0,
            lpc_order: 0,
            prev_nlsf_q15: [0; MAX_LPC_ORDER],
            first_frame_after_reset: true,
            pitch_lag_low_bits_icdf: &[],
            pitch_contour_icdf: &[],
            n_frames_decoded: 0,
            n_frames_per_packet: 0,
            ec_prev_signal_type: 0,
            ec_prev_lag_index: 0,
            vad_flags: [false; 3],
            lbrr_flag: false,
            lbrr_flags: [false; 3],
            resampler: Resampler::default(),
            nlsf_cb: &lpc::CB_NB_MB,
            indices: Indices::default(),
            cng: Cng {
                exc_buf_q14: [0; MAX_FRAME_LENGTH],
                smth_nlsf_q15: [0; 16],
                synth_state: [0; 16],
                smth_gain_q16: 0,
                rand_seed: 0,
                fs_khz: 0,
            },
            loss_cnt: 0,
            prev_signal_type: 0,
            plc: Plc {
                pitch_l_q8: 0,
                ltp_coef_q14: [0; LTP_ORDER],
                prev_lpc_q12: [0; 16],
                last_frame_lost: false,
                rand_seed: 0,
                rand_scale_q14: 0,
                conc_energy: 0,
                conc_energy_shift: 0,
                prev_ltp_scale_q14: 0,
                prev_gain_q16: [0; 2],
                fs_khz: 0,
                nb_subfr: 0,
                subfr_length: 0,
            },
        };
        c.cng_reset();
        c.plc_reset();
        c
    }

    /// `silk_decoder_set_fs`.
    fn set_fs(&mut self, fs_khz: i32, fs_api_hz: i32) {
        self.subfr_length = 5 * fs_khz as usize;
        let frame_length = self.nb_subfr * self.subfr_length;
        if self.fs_khz != fs_khz || self.fs_api_hz != fs_api_hz {
            self.resampler = Resampler::new(fs_khz * 1000);
            self.fs_api_hz = fs_api_hz;
        }
        if self.fs_khz != fs_khz || frame_length != self.frame_length {
            self.pitch_contour_icdf = match (fs_khz == 8, self.nb_subfr == 4) {
                (true, true) => &t::PITCH_CONTOUR_NB_ICDF,
                (true, false) => &t::PITCH_CONTOUR_10_MS_NB_ICDF,
                (false, true) => &t::PITCH_CONTOUR_ICDF,
                (false, false) => &t::PITCH_CONTOUR_10_MS_ICDF,
            };
            if self.fs_khz != fs_khz {
                self.ltp_mem_length = 20 * fs_khz as usize;
                if fs_khz == 16 {
                    self.lpc_order = 16;
                    self.nlsf_cb = &lpc::CB_WB;
                } else {
                    self.lpc_order = 10;
                    self.nlsf_cb = &lpc::CB_NB_MB;
                }
                self.pitch_lag_low_bits_icdf = match fs_khz {
                    16 => &t::UNIFORM8_ICDF,
                    12 => &t::UNIFORM6_ICDF,
                    _ => &t::UNIFORM4_ICDF,
                };
                self.first_frame_after_reset = true;
                self.lag_prev = 100;
                self.last_gain_index = 10;
                self.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
                self.out_buf = [0; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH];
                self.slpc_q14_buf = [0; MAX_LPC_ORDER];
            }
            self.fs_khz = fs_khz;
            self.frame_length = frame_length;
        }
    }

    /// `silk_decode_indices`.
    fn decode_indices(
        &mut self,
        dec: &mut RangeDecoder<'_>,
        frame_index: usize,
        decode_lbrr: bool,
        cond_coding: i32,
    ) {
        let ix = if decode_lbrr || self.vad_flags[frame_index] {
            dec.icdf(&t::TYPE_OFFSET_VAD_ICDF, 8) as i32 + 2
        } else {
            dec.icdf(&t::TYPE_OFFSET_NO_VAD_ICDF, 8) as i32
        };
        let ind = &mut self.indices;
        ind.signal_type = (ix >> 1) as i8;
        ind.quant_offset_type = (ix & 1) as i8;
        let st = ind.signal_type as usize;
        if cond_coding == CODE_CONDITIONALLY {
            ind.gains[0] = dec.icdf(&t::DELTA_GAIN_ICDF, 8) as i8;
        } else {
            ind.gains[0] = ((dec.icdf(&t::GAIN_ICDF[st * 8..st * 8 + 8], 8) as i32) << 3) as i8;
            ind.gains[0] = ind.gains[0].wrapping_add(dec.icdf(&t::UNIFORM8_ICDF, 8) as i8);
        }
        for i in 1..self.nb_subfr {
            ind.gains[i] = dec.icdf(&t::DELTA_GAIN_ICDF, 8) as i8;
        }
        let cb = self.nlsf_cb;
        let nv = lpc::NB_VECTORS;
        ind.nlsf[0] = dec.icdf(&cb.cb1_icdf[(st >> 1) * nv..], 8) as i8;
        let mut ec_ix = [0i16; 16];
        let mut pred_q8 = [0u8; 16];
        nlsf_unpack(&mut ec_ix, &mut pred_q8, cb, ind.nlsf[0] as usize);
        for i in 0..cb.order {
            let mut v = dec.icdf(&cb.ec_icdf[ec_ix[i] as usize..], 8) as i32;
            if v == 0 {
                v -= dec.icdf(&t::NLSF_EXT_ICDF, 8) as i32;
            } else if v == 8 {
                v += dec.icdf(&t::NLSF_EXT_ICDF, 8) as i32;
            }
            ind.nlsf[i + 1] = (v - 4) as i8;
        }
        ind.nlsf_interp_coef_q2 = if self.nb_subfr == 4 {
            dec.icdf(&t::NLSF_INTERPOLATION_FACTOR_ICDF, 8) as i8
        } else {
            4
        };
        if i32::from(ind.signal_type) == TYPE_VOICED {
            let mut absolute = true;
            if cond_coding == CODE_CONDITIONALLY && self.ec_prev_signal_type == TYPE_VOICED {
                let delta = dec.icdf(&t::PITCH_DELTA_ICDF, 8) as i32;
                if delta > 0 {
                    ind.lag = (i32::from(self.ec_prev_lag_index) + delta - 9) as i16;
                    absolute = false;
                }
            }
            if absolute {
                ind.lag = (dec.icdf(&t::PITCH_LAG_ICDF, 8) as i32 * (self.fs_khz >> 1)) as i16;
                ind.lag = ind
                    .lag
                    .wrapping_add(dec.icdf(self.pitch_lag_low_bits_icdf, 8) as i16);
            }
            self.ec_prev_lag_index = ind.lag;
            ind.contour = dec.icdf(self.pitch_contour_icdf, 8) as i8;
            ind.per = dec.icdf(&t::LTP_PER_INDEX_ICDF, 8) as i8;
            let tab: &[u8] = match ind.per {
                0 => &t::LTP_GAIN_ICDF_0,
                1 => &t::LTP_GAIN_ICDF_1,
                _ => &t::LTP_GAIN_ICDF_2,
            };
            for k in 0..self.nb_subfr {
                ind.ltp[k] = dec.icdf(tab, 8) as i8;
            }
            ind.ltp_scale = if cond_coding == CODE_INDEPENDENTLY {
                dec.icdf(&t::LTPSCALE_ICDF, 8) as i8
            } else {
                0
            };
        }
        self.ec_prev_signal_type = i32::from(ind.signal_type);
        ind.seed = dec.icdf(&t::UNIFORM4_ICDF, 8) as i8;
    }

    /// `silk_decode_parameters`.
    fn decode_parameters(&mut self, ctrl: &mut Ctrl, cond_coding: i32) {
        // silk_gains_dequant
        let conditional = cond_coding == CODE_CONDITIONALLY;
        let mut prev = i32::from(self.last_gain_index);
        for k in 0..self.nb_subfr {
            let ind = i32::from(self.indices.gains[k]);
            if k == 0 && !conditional {
                prev = ind.max(prev - 16);
            } else {
                let ind_tmp = ind - 4;
                let thr = 2 * 36 - 64 + prev;
                if ind_tmp > thr {
                    prev += (ind_tmp << 1) - thr;
                } else {
                    prev += ind_tmp;
                }
            }
            prev = prev.clamp(0, 63);
            ctrl.gains_q16[k] = log2lin((smulwb(1907825, prev) + 2090).min(3967));
        }
        self.last_gain_index = prev as i8;

        let order = self.lpc_order;
        let mut nlsf = [0i16; 16];
        nlsf_decode(&mut nlsf, &self.indices.nlsf, self.nlsf_cb);
        nlsf2a(&mut ctrl.pred_coef_q12[1][..order], &nlsf[..order]);
        if self.first_frame_after_reset {
            self.indices.nlsf_interp_coef_q2 = 4;
        }
        if self.indices.nlsf_interp_coef_q2 < 4 {
            let mut nlsf0 = [0i16; 16];
            let c = i32::from(self.indices.nlsf_interp_coef_q2);
            for i in 0..order {
                nlsf0[i] = (i32::from(self.prev_nlsf_q15[i])
                    + ((c * (i32::from(nlsf[i]) - i32::from(self.prev_nlsf_q15[i]))) >> 2))
                    as i16;
            }
            nlsf2a(&mut ctrl.pred_coef_q12[0][..order], &nlsf0[..order]);
        } else {
            ctrl.pred_coef_q12[0] = ctrl.pred_coef_q12[1];
        }
        self.prev_nlsf_q15[..order].copy_from_slice(&nlsf[..order]);
        if self.loss_cnt != 0 {
            bwexpander(&mut ctrl.pred_coef_q12[0][..order], BWE_AFTER_LOSS_Q16);
            bwexpander(&mut ctrl.pred_coef_q12[1][..order], BWE_AFTER_LOSS_Q16);
        }
        if i32::from(self.indices.signal_type) == TYPE_VOICED {
            decode_pitch(
                self.indices.lag,
                self.indices.contour,
                &mut ctrl.pitch_l,
                self.fs_khz,
                self.nb_subfr,
            );
            let cbk: &[i8] = match self.indices.per {
                0 => &t::LTP_GAIN_VQ_0,
                1 => &t::LTP_GAIN_VQ_1,
                _ => &t::LTP_GAIN_VQ_2,
            };
            for k in 0..self.nb_subfr {
                let ix = self.indices.ltp[k] as usize;
                for i in 0..LTP_ORDER {
                    ctrl.ltp_coef_q14[k * LTP_ORDER + i] =
                        (i32::from(cbk[ix * LTP_ORDER + i]) << 7) as i16;
                }
            }
            ctrl.ltp_scale_q14 =
                i32::from(t::LTP_SCALES_TABLE_Q14[self.indices.ltp_scale as usize]);
        } else {
            ctrl.pitch_l = [0; 4];
            ctrl.ltp_coef_q14 = [0; LTP_ORDER * 4];
            self.indices.per = 0;
            ctrl.ltp_scale_q14 = 0;
        }
    }

    /// `silk_decode_core`.
    fn decode_core(&mut self, ctrl: &mut Ctrl, xq: &mut [i16], pulses: &[i16]) {
        let ltp_mem = self.ltp_mem_length;
        let order = self.lpc_order;
        let subfr = self.subfr_length;
        let mut s_ltp = vec![0i16; ltp_mem];
        let mut s_ltp_q15 = vec![0i32; ltp_mem + self.frame_length];
        let mut res_q14 = [0i32; MAX_SUB_FRAME_LENGTH];
        let mut s_lpc = [0i32; MAX_SUB_FRAME_LENGTH + MAX_LPC_ORDER];
        let st = i32::from(self.indices.signal_type);
        let offset_q10 = i32::from(
            t::QUANTIZATION_OFFSETS_Q10
                [(st >> 1) as usize * 2 + self.indices.quant_offset_type as usize],
        );
        let nlsf_interp = self.indices.nlsf_interp_coef_q2 < 4;
        let mut seed = i32::from(self.indices.seed);
        for i in 0..self.frame_length {
            seed = rand(seed);
            let mut e = i32::from(pulses[i]) << 14;
            if e > 0 {
                e -= 80 << 4;
            } else if e < 0 {
                e += 80 << 4;
            }
            e += offset_q10 << 4;
            if seed < 0 {
                e = -e;
            }
            self.exc_q14[i] = e;
            seed = seed.wrapping_add(i32::from(pulses[i]));
        }
        s_lpc[..MAX_LPC_ORDER].copy_from_slice(&self.slpc_q14_buf);
        let mut sltp_buf_idx = ltp_mem;
        let mut lag = 0i32;
        for k in 0..self.nb_subfr {
            let exc_at = k * subfr;
            let a_q12: [i16; MAX_LPC_ORDER] = ctrl.pred_coef_q12[k >> 1];
            let mut signal_type = st;
            let gain_q10 = ctrl.gains_q16[k] >> 6;
            let mut inv_gain_q31 = inverse32_varq(ctrl.gains_q16[k], 47);
            let gain_adj_q16 = if ctrl.gains_q16[k] != self.prev_gain_q16 {
                let g = div32_varq(self.prev_gain_q16, ctrl.gains_q16[k], 16);
                for v in s_lpc.iter_mut().take(MAX_LPC_ORDER) {
                    *v = smulww(g, *v);
                }
                g
            } else {
                1 << 16
            };
            self.prev_gain_q16 = ctrl.gains_q16[k];
            if self.loss_cnt != 0
                && self.prev_signal_type == TYPE_VOICED
                && st != TYPE_VOICED
                && k < 2
            {
                let b = &mut ctrl.ltp_coef_q14[k * LTP_ORDER..(k + 1) * LTP_ORDER];
                b.fill(0);
                b[LTP_ORDER / 2] = 4096;
                signal_type = TYPE_VOICED;
                ctrl.pitch_l[k] = self.lag_prev;
            }
            let b_q14: [i16; LTP_ORDER] = ctrl.ltp_coef_q14[k * LTP_ORDER..(k + 1) * LTP_ORDER]
                .try_into()
                .unwrap_or([0; LTP_ORDER]);
            if signal_type == TYPE_VOICED {
                lag = ctrl.pitch_l[k];
                let lag_u = lag.max(0) as usize;
                if k == 0 || (k == 2 && nlsf_interp) {
                    let start_idx = ltp_mem.saturating_sub(lag_u + order + LTP_ORDER / 2).max(1);
                    if k == 2 {
                        self.out_buf[ltp_mem..ltp_mem + 2 * subfr]
                            .copy_from_slice(&xq[..2 * subfr]);
                    }
                    lpc_analysis_filter(
                        &mut s_ltp[start_idx..],
                        &self.out_buf[start_idx + k * subfr..],
                        &a_q12[..order],
                        ltp_mem - start_idx,
                    );
                    if k == 0 {
                        inv_gain_q31 = smulwb(inv_gain_q31, ctrl.ltp_scale_q14) << 2;
                    }
                    for i in 0..(lag_u + LTP_ORDER / 2).min(ltp_mem) {
                        s_ltp_q15[sltp_buf_idx - i - 1] =
                            smulwb(inv_gain_q31, i32::from(s_ltp[ltp_mem - i - 1]));
                    }
                } else if gain_adj_q16 != 1 << 16 {
                    for i in 0..(lag_u + LTP_ORDER / 2).min(sltp_buf_idx) {
                        let j = sltp_buf_idx - i - 1;
                        s_ltp_q15[j] = smulww(gain_adj_q16, s_ltp_q15[j]);
                    }
                }
            }
            let pres: &[i32] = if signal_type == TYPE_VOICED {
                let lag_u = lag.max(0) as usize;
                for i in 0..subfr {
                    let p = (sltp_buf_idx + LTP_ORDER / 2).saturating_sub(lag_u);
                    let g = |o: usize| s_ltp_q15.get(p.wrapping_sub(o)).copied().unwrap_or(0);
                    let mut pred = 2i32;
                    pred = smlawb(pred, g(0), i32::from(b_q14[0]));
                    pred = smlawb(pred, g(1), i32::from(b_q14[1]));
                    pred = smlawb(pred, g(2), i32::from(b_q14[2]));
                    pred = smlawb(pred, g(3), i32::from(b_q14[3]));
                    pred = smlawb(pred, g(4), i32::from(b_q14[4]));
                    res_q14[i] = self.exc_q14[exc_at + i].wrapping_add(pred.wrapping_shl(1));
                    s_ltp_q15[sltp_buf_idx] = res_q14[i].wrapping_shl(1);
                    sltp_buf_idx += 1;
                }
                &res_q14[..subfr]
            } else {
                &self.exc_q14[exc_at..exc_at + subfr]
            };
            for i in 0..subfr {
                let mut pred = (order >> 1) as i32;
                for j in 0..order {
                    pred = smlawb(pred, s_lpc[MAX_LPC_ORDER + i - j - 1], i32::from(a_q12[j]));
                }
                s_lpc[MAX_LPC_ORDER + i] = pres[i].saturating_add(lshift_sat32(pred, 4));
                xq[k * subfr + i] =
                    sat16(rshift_round(smulww(s_lpc[MAX_LPC_ORDER + i], gain_q10), 8)) as i16;
            }
            s_lpc.copy_within(subfr..subfr + MAX_LPC_ORDER, 0);
        }
        self.slpc_q14_buf.copy_from_slice(&s_lpc[..MAX_LPC_ORDER]);
    }

    /// `silk_decode_frame`: one 10/20 ms frame into `out[..frame_length]`.
    fn decode_frame(
        &mut self,
        dec: Option<&mut RangeDecoder<'_>>,
        out: &mut [i16],
        lost_flag: i32,
        cond_coding: i32,
    ) -> usize {
        let l = self.frame_length;
        let mut ctrl = Ctrl::default();
        let coded = lost_flag == FLAG_DECODE_NORMAL
            || (lost_flag == FLAG_DECODE_LBRR && self.lbrr_flags[self.n_frames_decoded.min(2)]);
        match dec {
            Some(dec) if coded => {
                let mut pulses = [0i16; MAX_FRAME_LENGTH];
                self.decode_indices(
                    dec,
                    self.n_frames_decoded.min(2),
                    lost_flag != 0,
                    cond_coding,
                );
                decode_pulses(
                    dec,
                    &mut pulses,
                    i32::from(self.indices.signal_type),
                    i32::from(self.indices.quant_offset_type),
                    l,
                );
                self.decode_parameters(&mut ctrl, cond_coding);
                self.decode_core(&mut ctrl, out, &pulses);
                self.plc(&mut ctrl, out, false);
                self.loss_cnt = 0;
                self.prev_signal_type = i32::from(self.indices.signal_type);
                self.first_frame_after_reset = false;
            }
            _ => self.plc(&mut ctrl, out, true),
        }
        let mv = self.ltp_mem_length - l;
        self.out_buf.copy_within(l..l + mv, 0);
        self.out_buf[mv..mv + l].copy_from_slice(&out[..l]);
        self.cng(&ctrl, out, l);
        self.plc_glue_frames(out, l);
        self.lag_prev = ctrl.pitch_l[self.nb_subfr - 1];
        l
    }

    // ── PLC (PLC.c) ─────────────────────────────────────────────────────

    fn plc_reset(&mut self) {
        self.plc.pitch_l_q8 = (self.frame_length as i32) << 7;
        self.plc.prev_gain_q16 = [65536, 65536];
        self.plc.subfr_length = 20;
        self.plc.nb_subfr = 2;
    }

    fn plc(&mut self, ctrl: &mut Ctrl, frame: &mut [i16], lost: bool) {
        if self.fs_khz != self.plc.fs_khz {
            self.plc_reset();
            self.plc.fs_khz = self.fs_khz;
        }
        if lost {
            self.plc_conceal(ctrl, frame);
            self.loss_cnt += 1;
        } else {
            self.plc_update(ctrl);
        }
    }

    fn plc_update(&mut self, ctrl: &Ctrl) {
        let nb = self.nb_subfr;
        self.prev_signal_type = i32::from(self.indices.signal_type);
        let mut ltp_gain_q14 = 0i32;
        let p = &mut self.plc;
        if i32::from(self.indices.signal_type) == TYPE_VOICED {
            let mut j = 0usize;
            while ((j * self.subfr_length) as i32) < ctrl.pitch_l[nb - 1] {
                if j == nb {
                    break;
                }
                let at = (nb - 1 - j) * LTP_ORDER;
                let tmp: i32 = ctrl.ltp_coef_q14[at..at + LTP_ORDER]
                    .iter()
                    .map(|&v| i32::from(v))
                    .sum();
                if tmp > ltp_gain_q14 {
                    ltp_gain_q14 = tmp;
                    p.ltp_coef_q14
                        .copy_from_slice(&ctrl.ltp_coef_q14[at..at + LTP_ORDER]);
                    p.pitch_l_q8 = ctrl.pitch_l[nb - 1 - j] << 8;
                }
                j += 1;
            }
            p.ltp_coef_q14 = [0; LTP_ORDER];
            p.ltp_coef_q14[LTP_ORDER / 2] = ltp_gain_q14 as i16;
            if ltp_gain_q14 < 11469 {
                let scale_q10 = (11469 << 10) / ltp_gain_q14.max(1);
                for v in &mut p.ltp_coef_q14 {
                    *v = (smulbb(i32::from(*v), scale_q10) >> 10) as i16;
                }
            } else if ltp_gain_q14 > 15565 {
                let scale_q14 = (15565 << 14) / ltp_gain_q14.max(1);
                for v in &mut p.ltp_coef_q14 {
                    *v = (smulbb(i32::from(*v), scale_q14) >> 14) as i16;
                }
            }
        } else {
            p.pitch_l_q8 = smulbb(self.fs_khz, 18) << 8;
            p.ltp_coef_q14 = [0; LTP_ORDER];
        }
        let order = self.lpc_order;
        p.prev_lpc_q12[..order].copy_from_slice(&ctrl.pred_coef_q12[1][..order]);
        p.prev_ltp_scale_q14 = ctrl.ltp_scale_q14 as i16;
        p.prev_gain_q16 = [ctrl.gains_q16[nb - 2], ctrl.gains_q16[nb - 1]];
        p.subfr_length = self.subfr_length;
        p.nb_subfr = nb;
    }

    fn plc_conceal(&mut self, ctrl: &mut Ctrl, frame: &mut [i16]) {
        const HARM_ATT_Q15: [i32; 2] = [32440, 31130];
        const RAND_ATT_V_Q15: [i32; 2] = [31130, 26214];
        const RAND_ATT_UV_Q15: [i32; 2] = [32440, 29491];
        let ltp_mem = self.ltp_mem_length;
        let order = self.lpc_order;
        let mut s_ltp_q14 = vec![0i32; ltp_mem + self.frame_length];
        let mut s_ltp = vec![0i16; ltp_mem];
        let prev_gain_q10 = [
            self.plc.prev_gain_q16[0] >> 6,
            self.plc.prev_gain_q16[1] >> 6,
        ];
        if self.first_frame_after_reset {
            self.plc.prev_lpc_q12 = [0; 16];
        }
        // silk_PLC_energy
        let sl = self.subfr_length;
        let nb = self.nb_subfr;
        let mut exc_buf = vec![0i16; 2 * sl];
        for k in 0..2 {
            for i in 0..sl {
                let src = i + (k + nb - 2) * sl;
                exc_buf[k * sl + i] =
                    sat16(smulww(self.exc_q14[src], prev_gain_q10[k]) >> 8) as i16;
            }
        }
        let (energy1, shift1) = sum_sqr_shift(&exc_buf[..sl]);
        let (energy2, shift2) = sum_sqr_shift(&exc_buf[sl..]);
        let rand_at = if (energy1 >> shift2) < (energy2 >> shift1) {
            ((self.plc.nb_subfr - 1) * self.plc.subfr_length).saturating_sub(128)
        } else {
            (self.plc.nb_subfr * self.plc.subfr_length).saturating_sub(128)
        };
        let mut b_q14 = self.plc.ltp_coef_q14;
        let mut rand_scale_q14 = self.plc.rand_scale_q14;
        let lc = (self.loss_cnt.min(1)) as usize;
        let harm_gain_q15 = HARM_ATT_Q15[lc];
        let mut rand_gain_q15 = if self.prev_signal_type == TYPE_VOICED {
            RAND_ATT_V_Q15[lc]
        } else {
            RAND_ATT_UV_Q15[lc]
        };
        bwexpander(&mut self.plc.prev_lpc_q12[..order], 64881); // SILK_FIX_CONST(0.99, 16)
        let a_q12 = self.plc.prev_lpc_q12;
        if self.loss_cnt == 0 {
            rand_scale_q14 = 1 << 14;
            if self.prev_signal_type == TYPE_VOICED {
                for &b in &b_q14 {
                    rand_scale_q14 = rand_scale_q14.wrapping_sub(b);
                }
                rand_scale_q14 = rand_scale_q14.max(3277);
                rand_scale_q14 = (smulbb(
                    i32::from(rand_scale_q14),
                    i32::from(self.plc.prev_ltp_scale_q14),
                ) >> 14) as i16;
            } else {
                let inv_gain_q30 = lpc_inverse_pred_gain(&self.plc.prev_lpc_q12[..order]);
                let mut down = ((1i32 << 30) >> 3).min(inv_gain_q30);
                down = ((1i32 << 30) >> 8).max(down);
                down <<= 3;
                rand_gain_q15 = smulwb(down, rand_gain_q15) >> 14;
            }
        }
        let mut rand_seed = self.plc.rand_seed;
        let mut lag = rshift_round(self.plc.pitch_l_q8, 8);
        let mut sltp_buf_idx = ltp_mem;
        let idx = ltp_mem
            .saturating_sub(lag.max(0) as usize + order + LTP_ORDER / 2)
            .max(1);
        lpc_analysis_filter(
            &mut s_ltp[idx..],
            &self.out_buf[idx..],
            &a_q12[..order],
            ltp_mem - idx,
        );
        let inv_gain_q30 = inverse32_varq(self.plc.prev_gain_q16[1], 46).min(i32::MAX >> 1);
        for i in idx + order..ltp_mem {
            s_ltp_q14[i] = smulwb(inv_gain_q30, i32::from(s_ltp[i]));
        }
        for _ in 0..nb {
            for _ in 0..sl {
                // pred_lag_ptr walks s_ltp_q14 from buf_idx - lag + 2.
                let p = sltp_buf_idx as isize + (LTP_ORDER / 2) as isize - lag as isize;
                let g = |o: isize| {
                    if p - o >= 0 {
                        s_ltp_q14.get((p - o) as usize).copied().unwrap_or(0)
                    } else {
                        0
                    }
                };
                let mut pred = 2i32;
                pred = smlawb(pred, g(0), i32::from(b_q14[0]));
                pred = smlawb(pred, g(1), i32::from(b_q14[1]));
                pred = smlawb(pred, g(2), i32::from(b_q14[2]));
                pred = smlawb(pred, g(3), i32::from(b_q14[3]));
                pred = smlawb(pred, g(4), i32::from(b_q14[4]));
                rand_seed = rand(rand_seed);
                let ri = ((rand_seed >> 25) & 127) as usize;
                s_ltp_q14[sltp_buf_idx] =
                    smlawb(pred, self.exc_q14[rand_at + ri], i32::from(rand_scale_q14))
                        .wrapping_shl(2);
                sltp_buf_idx += 1;
            }
            for b in &mut b_q14 {
                *b = (smulbb(harm_gain_q15, i32::from(*b)) >> 15) as i16;
            }
            rand_scale_q14 = (smulbb(i32::from(rand_scale_q14), rand_gain_q15) >> 15) as i16;
            self.plc.pitch_l_q8 = smlawb(self.plc.pitch_l_q8, self.plc.pitch_l_q8, 655);
            self.plc.pitch_l_q8 = self.plc.pitch_l_q8.min(smulbb(18, self.fs_khz) << 8);
            lag = rshift_round(self.plc.pitch_l_q8, 8);
        }
        let s0 = ltp_mem - MAX_LPC_ORDER;
        s_ltp_q14[s0..ltp_mem].copy_from_slice(&self.slpc_q14_buf);
        for i in 0..self.frame_length {
            let mut pred = (order >> 1) as i32;
            for j in 0..order {
                pred = smlawb(
                    pred,
                    s_ltp_q14[s0 + MAX_LPC_ORDER + i - j - 1],
                    i32::from(a_q12[j]),
                );
            }
            let at = s0 + MAX_LPC_ORDER + i;
            s_ltp_q14[at] = s_ltp_q14[at].saturating_add(lshift_sat32(pred, 4));
            frame[i] = sat16(rshift_round(smulww(s_ltp_q14[at], prev_gain_q10[1]), 8)) as i16;
        }
        let e = s0 + self.frame_length;
        self.slpc_q14_buf
            .copy_from_slice(&s_ltp_q14[e..e + MAX_LPC_ORDER]);
        self.plc.rand_seed = rand_seed;
        self.plc.rand_scale_q14 = rand_scale_q14;
        ctrl.pitch_l = [lag; 4];
    }

    fn plc_glue_frames(&mut self, frame: &mut [i16], length: usize) {
        let p = &mut self.plc;
        if self.loss_cnt != 0 {
            let (e, s) = sum_sqr_shift(&frame[..length]);
            p.conc_energy = e;
            p.conc_energy_shift = s;
            p.last_frame_lost = true;
        } else {
            if p.last_frame_lost {
                let (mut energy, energy_shift) = sum_sqr_shift(&frame[..length]);
                if energy_shift > p.conc_energy_shift {
                    p.conc_energy >>= energy_shift - p.conc_energy_shift;
                } else if energy_shift < p.conc_energy_shift {
                    energy >>= p.conc_energy_shift - energy_shift;
                }
                if energy > p.conc_energy {
                    let lz = fix::clz32(p.conc_energy) - 1;
                    p.conc_energy = p.conc_energy.wrapping_shl(lz as u32);
                    energy >>= (24 - lz).max(0);
                    let frac_q24 = p.conc_energy / energy.max(1);
                    let mut gain_q16 = sqrt_approx(frac_q24) << 4;
                    let mut slope_q16 = ((1i32 << 16) - gain_q16) / length as i32;
                    slope_q16 <<= 2;
                    for v in frame.iter_mut().take(length) {
                        *v = smulwb(gain_q16, i32::from(*v)) as i16;
                        gain_q16 += slope_q16;
                        if gain_q16 > 1 << 16 {
                            break;
                        }
                    }
                }
            }
            p.last_frame_lost = false;
        }
    }

    // ── CNG (CNG.c) ─────────────────────────────────────────────────────

    fn cng_reset(&mut self) {
        let step = i32::from(i16::MAX) / (self.lpc_order as i32 + 1);
        let mut acc = 0i32;
        for i in 0..self.lpc_order {
            acc += step;
            self.cng.smth_nlsf_q15[i] = acc as i16;
        }
        self.cng.smth_gain_q16 = 0;
        self.cng.rand_seed = 3_176_576;
    }

    fn cng(&mut self, ctrl: &Ctrl, frame: &mut [i16], length: usize) {
        if self.fs_khz != self.cng.fs_khz {
            self.cng_reset();
            self.cng.fs_khz = self.fs_khz;
        }
        let order = self.lpc_order;
        let sl = self.subfr_length;
        if self.loss_cnt == 0 && self.prev_signal_type == TYPE_NO_VOICE_ACTIVITY {
            let c = &mut self.cng;
            for i in 0..order {
                let d = i32::from(self.prev_nlsf_q15[i]) - i32::from(c.smth_nlsf_q15[i]);
                c.smth_nlsf_q15[i] =
                    (i32::from(c.smth_nlsf_q15[i]) + smulwb(d, CNG_NLSF_SMTH_Q16)) as i16;
            }
            let mut max_gain = 0i32;
            let mut subfr = 0usize;
            for i in 0..self.nb_subfr {
                if ctrl.gains_q16[i] > max_gain {
                    max_gain = ctrl.gains_q16[i];
                    subfr = i;
                }
            }
            c.exc_buf_q14.copy_within(0..(self.nb_subfr - 1) * sl, sl);
            c.exc_buf_q14[..sl].copy_from_slice(&self.exc_q14[subfr * sl..subfr * sl + sl]);
            for i in 0..self.nb_subfr {
                c.smth_gain_q16 = c.smth_gain_q16.wrapping_add(smulwb(
                    ctrl.gains_q16[i].wrapping_sub(c.smth_gain_q16),
                    CNG_GAIN_SMTH_Q16,
                ));
                if smulww(c.smth_gain_q16, CNG_GAIN_SMTH_THRESHOLD_Q16) > ctrl.gains_q16[i] {
                    c.smth_gain_q16 = ctrl.gains_q16[i];
                }
            }
        }
        if self.loss_cnt != 0 {
            let mut sig = vec![0i32; length + MAX_LPC_ORDER];
            let c = &mut self.cng;
            let mut gain_q16 = smulww(
                i32::from(self.plc.rand_scale_q14),
                self.plc.prev_gain_q16[1],
            );
            if gain_q16 >= 1 << 21 || c.smth_gain_q16 > 1 << 23 {
                gain_q16 = smultt(gain_q16, gain_q16);
                gain_q16 =
                    smultt(c.smth_gain_q16, c.smth_gain_q16).wrapping_sub(gain_q16.wrapping_shl(5));
                gain_q16 = sqrt_approx(gain_q16).wrapping_shl(16);
            } else {
                gain_q16 = smulww(gain_q16, gain_q16);
                gain_q16 =
                    smulww(c.smth_gain_q16, c.smth_gain_q16).wrapping_sub(gain_q16.wrapping_shl(5));
                gain_q16 = sqrt_approx(gain_q16).wrapping_shl(8);
            }
            let gain_q10 = gain_q16 >> 6;
            // silk_CNG_exc
            let mut mask = 255usize;
            while mask > length {
                mask >>= 1;
            }
            let mut seed = c.rand_seed;
            for i in 0..length {
                seed = rand(seed);
                let idx = ((seed >> 24) as usize) & mask;
                sig[MAX_LPC_ORDER + i] = c.exc_buf_q14[idx];
            }
            c.rand_seed = seed;
            let mut a_q12 = [0i16; 16];
            nlsf2a(&mut a_q12[..order], &c.smth_nlsf_q15[..order]);
            sig[..MAX_LPC_ORDER].copy_from_slice(&c.synth_state);
            for i in 0..length {
                let mut pred = (order >> 1) as i32;
                for j in 0..order {
                    pred = smlawb(pred, sig[MAX_LPC_ORDER + i - j - 1], i32::from(a_q12[j]));
                }
                let at = MAX_LPC_ORDER + i;
                sig[at] = sig[at].saturating_add(lshift_sat32(pred, 4));
                frame[i] =
                    sat16(i32::from(frame[i]) + sat16(rshift_round(smulww(sig[at], gain_q10), 8)))
                        as i16;
            }
            c.synth_state
                .copy_from_slice(&sig[length..length + MAX_LPC_ORDER]);
        } else {
            self.cng.synth_state[..order].fill(0);
        }
    }
}

/// `silk_decode_pitch`.
fn decode_pitch(lag_index: i16, contour: i8, lags: &mut [i32; 4], fs_khz: i32, nb_subfr: usize) {
    let (cb, size): (&[i8], usize) = match (fs_khz == 8, nb_subfr == 4) {
        (true, true) => (&t::CB_LAGS_STAGE2, 11),
        (true, false) => (&t::CB_LAGS_STAGE2_10_MS, 3),
        (false, true) => (&t::CB_LAGS_STAGE3, 34),
        (false, false) => (&t::CB_LAGS_STAGE3_10_MS, 12),
    };
    let min_lag = 2 * fs_khz;
    let max_lag = 18 * fs_khz;
    let lag = min_lag + i32::from(lag_index);
    let c = (contour.max(0) as usize).min(size - 1);
    for k in 0..nb_subfr {
        lags[k] = (lag + i32::from(cb[k * size + c])).clamp(min_lag, max_lag);
    }
}

/// `silk_shell_decoder`: 16 pulse magnitudes summing to `pulses4`.
fn shell_decoder(out: &mut [i16], dec: &mut RangeDecoder<'_>, pulses4: i32) {
    let split = |dec: &mut RangeDecoder<'_>, p: i32, table: &[u8]| -> (i16, i16) {
        if p > 0 {
            let off = t::SHELL_CODE_TABLE_OFFSETS[p.min(16) as usize] as usize;
            let a = dec.icdf(&table[off..], 8) as i32;
            (a as i16, (p - a) as i16)
        } else {
            (0, 0)
        }
    };
    let (t0, t1, t2, t3) = (
        &t::SHELL_CODE_TABLE0[..],
        &t::SHELL_CODE_TABLE1[..],
        &t::SHELL_CODE_TABLE2[..],
        &t::SHELL_CODE_TABLE3[..],
    );
    let mut p3 = [0i16; 2];
    let mut p2 = [0i16; 4];
    let mut p1 = [0i16; 8];
    (p3[0], p3[1]) = split(dec, pulses4, t3);
    (p2[0], p2[1]) = split(dec, i32::from(p3[0]), t2);
    (p1[0], p1[1]) = split(dec, i32::from(p2[0]), t1);
    (out[0], out[1]) = split(dec, i32::from(p1[0]), t0);
    (out[2], out[3]) = split(dec, i32::from(p1[1]), t0);
    (p1[2], p1[3]) = split(dec, i32::from(p2[1]), t1);
    (out[4], out[5]) = split(dec, i32::from(p1[2]), t0);
    (out[6], out[7]) = split(dec, i32::from(p1[3]), t0);
    (p2[2], p2[3]) = split(dec, i32::from(p3[1]), t2);
    (p1[4], p1[5]) = split(dec, i32::from(p2[2]), t1);
    (out[8], out[9]) = split(dec, i32::from(p1[4]), t0);
    (out[10], out[11]) = split(dec, i32::from(p1[5]), t0);
    (p1[6], p1[7]) = split(dec, i32::from(p2[3]), t1);
    (out[12], out[13]) = split(dec, i32::from(p1[6]), t0);
    (out[14], out[15]) = split(dec, i32::from(p1[7]), t0);
}

/// `silk_decode_pulses` with `silk_decode_signs`.
fn decode_pulses(
    dec: &mut RangeDecoder<'_>,
    pulses: &mut [i16; MAX_FRAME_LENGTH],
    signal_type: i32,
    quant_offset_type: i32,
    frame_length: usize,
) {
    let rate_level = dec.icdf(&t::RATE_LEVELS_ICDF[(signal_type >> 1) as usize * 9..], 8);
    let mut iter = frame_length >> 4;
    if iter * 16 < frame_length {
        iter += 1;
    }
    let mut sum = [0i32; MAX_FRAME_LENGTH / 16];
    let mut nls = [0i32; MAX_FRAME_LENGTH / 16];
    let row = |r: usize| &t::PULSES_PER_BLOCK_ICDF[r * 18..r * 18 + 18];
    for i in 0..iter {
        sum[i] = dec.icdf(row(rate_level.min(9)), 8) as i32;
        while sum[i] == 17 {
            nls[i] += 1;
            let r = row(9);
            sum[i] = dec.icdf(&r[usize::from(nls[i] == 10)..], 8) as i32;
            if nls[i] > 10 {
                // A hostile stream; libopus would loop on — stop the walk.
                sum[i] = 0;
                break;
            }
        }
    }
    for i in 0..iter {
        let blk = &mut pulses[i * 16..i * 16 + 16];
        if sum[i] > 0 {
            shell_decoder(blk, dec, sum[i]);
        } else {
            blk.fill(0);
        }
    }
    for i in 0..iter {
        if nls[i] > 0 {
            for k in 0..16 {
                let mut abs_q = i32::from(pulses[i * 16 + k]);
                for _ in 0..nls[i] {
                    abs_q = (abs_q << 1) + dec.icdf(&t::LSB_ICDF, 8) as i32;
                }
                pulses[i * 16 + k] = abs_q as i16;
            }
            sum[i] |= nls[i] << 5;
        }
    }
    // silk_decode_signs
    let sel = (7 * (quant_offset_type + (signal_type << 1))) as usize;
    let len = (frame_length + 8) >> 4;
    for i in 0..len {
        let p = sum[i];
        if p > 0 {
            let icdf = [t::SIGN_ICDF[sel + (p & 0x1f).min(6) as usize], 0];
            for j in 0..16 {
                let q = &mut pulses[i * 16 + j];
                if *q > 0 {
                    let s = dec.icdf(&icdf, 8) as i32;
                    *q = (i32::from(*q) * ((s << 1) - 1)) as i16;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Stereo {
    pred_prev_q13: [i16; 2],
    s_mid: [i16; 2],
    s_side: [i16; 2],
}

/// `silk_stereo_decode_pred`.
fn stereo_decode_pred(dec: &mut RangeDecoder<'_>, pred_q13: &mut [i32; 2]) {
    let n = dec.icdf(&t::STEREO_PRED_JOINT_ICDF, 8) as i32;
    let mut ix = [[0i32; 3]; 2];
    ix[0][2] = n / 5;
    ix[1][2] = n - 5 * ix[0][2];
    for v in &mut ix {
        v[0] = dec.icdf(&t::UNIFORM3_ICDF, 8) as i32;
        v[1] = dec.icdf(&t::UNIFORM5_ICDF, 8) as i32;
    }
    for n in 0..2 {
        ix[n][0] += 3 * ix[n][2];
        let i0 = (ix[n][0] as usize).min(14);
        let low = i32::from(t::STEREO_PRED_QUANT_Q13[i0]);
        let step = smulwb(i32::from(t::STEREO_PRED_QUANT_Q13[i0 + 1]) - low, 6554); // SILK_FIX_CONST(0.5/5, 16)
        pred_q13[n] = smlabb(low, step, 2 * ix[n][1] + 1);
    }
    pred_q13[0] -= pred_q13[1];
}

/// `silk_stereo_MS_to_LR`: `x1`/`x2` hold 2 history samples then
/// `frame_length` mid/side samples, and leave holding left/right.
fn stereo_ms_to_lr(
    st: &mut Stereo,
    x1: &mut [i16],
    x2: &mut [i16],
    pred_q13: &[i32; 2],
    fs_khz: i32,
    frame_length: usize,
) {
    x1[..2].copy_from_slice(&st.s_mid);
    x2[..2].copy_from_slice(&st.s_side);
    st.s_mid
        .copy_from_slice(&x1[frame_length..frame_length + 2]);
    st.s_side
        .copy_from_slice(&x2[frame_length..frame_length + 2]);
    let mut pred0 = i32::from(st.pred_prev_q13[0]);
    let mut pred1 = i32::from(st.pred_prev_q13[1]);
    let interp = (8 * fs_khz) as usize;
    let denom_q16 = (1i32 << 16) / (8 * fs_khz);
    let delta0 = rshift_round(
        smulbb(pred_q13[0] - i32::from(st.pred_prev_q13[0]), denom_q16),
        16,
    );
    let delta1 = rshift_round(
        smulbb(pred_q13[1] - i32::from(st.pred_prev_q13[1]), denom_q16),
        16,
    );
    let mix = |x1: &[i16], x2: &mut [i16], n: usize, p0: i32, p1: i32| {
        let mut sum =
            ((i32::from(x1[n]) + i32::from(x1[n + 2])) + (i32::from(x1[n + 1]) << 1)) << 9;
        sum = smlawb(i32::from(x2[n + 1]) << 8, sum, p0);
        sum = smlawb(sum, i32::from(x1[n + 1]) << 11, p1);
        x2[n + 1] = sat16(rshift_round(sum, 8)) as i16;
    };
    for n in 0..interp.min(frame_length) {
        pred0 += delta0;
        pred1 += delta1;
        mix(x1, x2, n, pred0, pred1);
    }
    for n in interp..frame_length {
        mix(x1, x2, n, pred_q13[0], pred_q13[1]);
    }
    st.pred_prev_q13 = [pred_q13[0] as i16, pred_q13[1] as i16];
    for n in 0..frame_length {
        let a = i32::from(x1[n + 1]);
        let b = i32::from(x2[n + 1]);
        x1[n + 1] = sat16(a + b) as i16;
        x2[n + 1] = sat16(a - b) as i16;
    }
}

/// `silk_decoder` with the `silk_Decode` entry point, fixed at a 48 kHz
/// API rate.
pub(crate) struct SilkDecoder {
    ch: [Channel; 2],
    stereo: Stereo,
    n_channels_api: usize,
    n_channels_internal: usize,
    prev_decode_only_middle: bool,
    api_channels: usize,
    internal_rate: i32,
}

impl SilkDecoder {
    pub(crate) fn new(api_channels: usize) -> Self {
        Self {
            ch: [Channel::new(), Channel::new()],
            stereo: Stereo::default(),
            n_channels_api: 0,
            n_channels_internal: 0,
            prev_decode_only_middle: false,
            api_channels,
            internal_rate: 0,
        }
    }

    /// `silk_InitDecoder` (what `OPUS_RESET_STATE` and a CELT→SILK switch
    /// do): the channel states and the stereo state, not the channel
    /// counts.
    pub(crate) fn reset(&mut self) {
        self.ch = [Channel::new(), Channel::new()];
        self.stereo = Stereo::default();
        self.prev_decode_only_middle = false;
    }

    /// `silk_Decode`: one 10/20 ms frame (of the packet's 10–60 ms) at
    /// 48 kHz into `out` (interleaved, `api_channels`); returns samples
    /// per channel. `internal_rate` is `None` for a lost packet (the last
    /// rate stands).
    pub(crate) fn decode(
        &mut self,
        stream_channels: usize,
        internal_rate: Option<i32>,
        payload_ms: usize,
        lost_flag: i32,
        new_packet: bool,
        mut dec: Option<&mut RangeDecoder<'_>>,
        out: &mut [i16],
    ) -> Result<usize, ()> {
        if let Some(r) = internal_rate {
            self.internal_rate = r;
        }
        let nci = stream_channels;
        let api = self.api_channels;
        if new_packet {
            for n in 0..nci {
                self.ch[n].n_frames_decoded = 0;
            }
        }
        if nci > self.n_channels_internal {
            self.ch[1] = Channel::new();
        }
        let stereo_to_mono = nci == 1
            && self.n_channels_internal == 2
            && self.internal_rate == 1000 * self.ch[0].fs_khz;
        if self.ch[0].n_frames_decoded == 0 {
            for n in 0..nci {
                let (fpp, nb) = match payload_ms {
                    0 | 10 => (1, 2),
                    20 => (1, 4),
                    40 => (2, 4),
                    60 => (3, 4),
                    _ => return Err(()),
                };
                self.ch[n].n_frames_per_packet = fpp;
                self.ch[n].nb_subfr = nb;
                let fs_khz_dec = (self.internal_rate >> 10) + 1;
                if fs_khz_dec != 8 && fs_khz_dec != 12 && fs_khz_dec != 16 {
                    return Err(());
                }
                self.ch[n].set_fs(fs_khz_dec, API_HZ);
            }
        }
        if api == 2 && nci == 2 && (self.n_channels_api == 1 || self.n_channels_internal == 1) {
            self.stereo.pred_prev_q13 = [0; 2];
            self.stereo.s_side = [0; 2];
            self.ch[1].resampler = self.ch[0].resampler.clone();
        }
        self.n_channels_api = api;
        self.n_channels_internal = nci;

        let mut decode_only_middle = false;
        let mut ms_pred_q13 = [0i32; 2];
        if lost_flag != FLAG_PACKET_LOST && self.ch[0].n_frames_decoded == 0 {
            let Some(d) = dec.as_deref_mut() else {
                return Err(());
            };
            for n in 0..nci {
                for i in 0..self.ch[n].n_frames_per_packet {
                    self.ch[n].vad_flags[i] = d.bit_logp(1);
                }
                self.ch[n].lbrr_flag = d.bit_logp(1);
            }
            for n in 0..nci {
                let c = &mut self.ch[n];
                c.lbrr_flags = [false; 3];
                if c.lbrr_flag {
                    if c.n_frames_per_packet == 1 {
                        c.lbrr_flags[0] = true;
                    } else {
                        let tab: &[u8] = if c.n_frames_per_packet == 2 {
                            &t::LBRR_FLAGS_2_ICDF
                        } else {
                            &t::LBRR_FLAGS_3_ICDF
                        };
                        let sym = d.icdf(tab, 8) + 1;
                        for i in 0..c.n_frames_per_packet {
                            c.lbrr_flags[i] = (sym >> i) & 1 != 0;
                        }
                    }
                }
            }
            if lost_flag == FLAG_DECODE_NORMAL {
                // Skip the LBRR data (parsed, side effects on the
                // channel's index history kept, pulses dropped), as
                // libopus does when not asked for FEC.
                for i in 0..self.ch[0].n_frames_per_packet {
                    for n in 0..nci {
                        if self.ch[n].lbrr_flags[i] {
                            if nci == 2 && n == 0 {
                                stereo_decode_pred(d, &mut ms_pred_q13);
                                if !self.ch[1].lbrr_flags[i] {
                                    decode_only_middle =
                                        d.icdf(&t::STEREO_ONLY_CODE_MID_ICDF, 8) != 0;
                                }
                            }
                            let cond = if i > 0 && self.ch[n].lbrr_flags[i - 1] {
                                CODE_CONDITIONALLY
                            } else {
                                CODE_INDEPENDENTLY
                            };
                            self.ch[n].decode_indices(d, i, true, cond);
                            let mut pulses = [0i16; MAX_FRAME_LENGTH];
                            let c = &self.ch[n];
                            decode_pulses(
                                d,
                                &mut pulses,
                                i32::from(c.indices.signal_type),
                                i32::from(c.indices.quant_offset_type),
                                c.frame_length,
                            );
                        }
                    }
                }
            }
        }
        if nci == 2 {
            let fd = self.ch[0].n_frames_decoded.min(2);
            if lost_flag == FLAG_DECODE_NORMAL
                || (lost_flag == FLAG_DECODE_LBRR && self.ch[0].lbrr_flags[fd])
            {
                let Some(d) = dec.as_deref_mut() else {
                    return Err(());
                };
                stereo_decode_pred(d, &mut ms_pred_q13);
                if (lost_flag == FLAG_DECODE_NORMAL && !self.ch[1].vad_flags[fd])
                    || (lost_flag == FLAG_DECODE_LBRR && !self.ch[1].lbrr_flags[fd])
                {
                    decode_only_middle = d.icdf(&t::STEREO_ONLY_CODE_MID_ICDF, 8) != 0;
                } else {
                    decode_only_middle = false;
                }
            } else {
                ms_pred_q13 = [
                    i32::from(self.stereo.pred_prev_q13[0]),
                    i32::from(self.stereo.pred_prev_q13[1]),
                ];
            }
        }
        if nci == 2 && !decode_only_middle && self.prev_decode_only_middle {
            let c = &mut self.ch[1];
            c.out_buf = [0; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH];
            c.slpc_q14_buf = [0; MAX_LPC_ORDER];
            c.lag_prev = 100;
            c.last_gain_index = 10;
            c.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
            c.first_frame_after_reset = true;
        }
        let fl = self.ch[0].frame_length;
        let mut tmp = [vec![0i16; fl + 2], vec![0i16; fl + 2]];
        let has_side = if lost_flag == FLAG_DECODE_NORMAL {
            !decode_only_middle
        } else {
            !self.prev_decode_only_middle
                || (nci == 2
                    && lost_flag == FLAG_DECODE_LBRR
                    && self.ch[1].lbrr_flags[self.ch[1].n_frames_decoded.min(2)])
        };
        let mut n_out_dec = 0usize;
        for n in 0..nci {
            if n == 0 || has_side {
                let frame_index = self.ch[0].n_frames_decoded as isize - n as isize;
                let cond = if frame_index <= 0 {
                    CODE_INDEPENDENTLY
                } else if lost_flag == FLAG_DECODE_LBRR {
                    if self.ch[n].lbrr_flags[(frame_index - 1) as usize] {
                        CODE_CONDITIONALLY
                    } else {
                        CODE_INDEPENDENTLY
                    }
                } else if n > 0 && self.prev_decode_only_middle {
                    CODE_INDEPENDENTLY_NO_LTP_SCALING
                } else {
                    CODE_CONDITIONALLY
                };
                if self.ch[n].frame_length != fl {
                    return Err(());
                }
                n_out_dec =
                    self.ch[n].decode_frame(dec.as_deref_mut(), &mut tmp[n][2..], lost_flag, cond);
            } else {
                tmp[n][2..2 + n_out_dec].fill(0);
            }
            self.ch[n].n_frames_decoded += 1;
        }
        let fs_khz = self.ch[0].fs_khz;
        {
            let [t0, t1] = &mut tmp;
            if api == 2 && nci == 2 {
                stereo_ms_to_lr(&mut self.stereo, t0, t1, &ms_pred_q13, fs_khz, n_out_dec);
            } else {
                t0[..2].copy_from_slice(&self.stereo.s_mid);
                self.stereo
                    .s_mid
                    .copy_from_slice(&t0[n_out_dec..n_out_dec + 2]);
            }
        }
        let n_out = n_out_dec * API_HZ as usize / (fs_khz as usize * 1000);
        let mut res = vec![0i16; n_out];
        for n in 0..api.min(nci) {
            self.ch[n]
                .resampler
                .process(&mut res, &tmp[n][1..1 + n_out_dec]);
            for i in 0..n_out {
                out[n + api * i] = res[i];
            }
        }
        if api == 2 && nci == 1 {
            if stereo_to_mono {
                self.ch[1]
                    .resampler
                    .process(&mut res, &tmp[0][1..1 + n_out_dec]);
                for i in 0..n_out {
                    out[1 + 2 * i] = res[i];
                }
            } else {
                for i in 0..n_out {
                    out[1 + 2 * i] = out[2 * i];
                }
            }
        }
        if lost_flag == FLAG_PACKET_LOST {
            for c in self.ch.iter_mut().take(self.n_channels_internal) {
                c.last_gain_index = 10;
            }
        } else {
            self.prev_decode_only_middle = decode_only_middle;
        }
        Ok(n_out)
    }
}
