//! SILK's output resampler (`resampler.c`, `resampler_private_IIR_FIR.c`,
//! `resampler_private_up2_HQ.c`), decoder side: 8/12/16 kHz up to the
//! 48 kHz API rate this decoder runs at, which is always the 2× all-pass
//! upsampler followed by the 12-phase fractional FIR.

use super::fix::{rshift_round, sat16, smlabb, smlawb, smulbb, smulwb, smulww};
use super::tables as t;

const ORDER_FIR_12: usize = 8;
const API_HZ: i32 = 48_000;

#[derive(Clone)]
pub(crate) struct Resampler {
    s_iir: [i32; 6],
    s_fir: [i16; ORDER_FIR_12],
    delay_buf: [i16; 48],
    batch_size: usize,
    inv_ratio_q16: i32,
    fs_in_khz: usize,
    fs_out_khz: usize,
    input_delay: usize,
}

impl Default for Resampler {
    fn default() -> Self {
        Self {
            s_iir: [0; 6],
            s_fir: [0; ORDER_FIR_12],
            delay_buf: [0; 48],
            batch_size: 0,
            inv_ratio_q16: 0,
            fs_in_khz: 0,
            fs_out_khz: 0,
            input_delay: 0,
        }
    }
}

impl Resampler {
    /// `silk_resampler_init(.., fs_in, 48000, forEnc = 0)`.
    pub(crate) fn new(fs_in: i32) -> Self {
        // delay_matrix_dec[rateID(in)][rateID(48000)]
        let input_delay = match fs_in {
            8000 => 0,
            12000 => 4,
            _ => 7,
        };
        let fs_in_khz = (fs_in / 1000) as usize;
        let mut inv = ((fs_in << 15) / API_HZ) << 2;
        while smulww(inv, API_HZ) < fs_in << 1 {
            inv += 1;
        }
        Self {
            fs_in_khz,
            fs_out_khz: 48,
            batch_size: fs_in_khz * 10,
            inv_ratio_q16: inv,
            input_delay,
            ..Self::default()
        }
    }

    /// `silk_resampler`: `input.len()` samples in, `len × 48 / fs_in` out.
    pub(crate) fn process(&mut self, out: &mut [i16], input: &[i16]) {
        let in_len = input.len();
        let n = self.fs_in_khz - self.input_delay;
        self.delay_buf[self.input_delay..self.fs_in_khz].copy_from_slice(&input[..n]);
        let db = self.delay_buf;
        let first = self.iir_fir(out, &db[..self.fs_in_khz]);
        debug_assert_eq!(first, self.fs_out_khz);
        self.iir_fir(
            &mut out[self.fs_out_khz..],
            &input[n..in_len - self.input_delay],
        );
        let d = self.input_delay;
        self.delay_buf[..d].copy_from_slice(&input[in_len - d..]);
    }

    fn up2_hq(s: &mut [i32; 6], out: &mut [i16], input: &[i16]) {
        let c0 = &t::RESAMPLER_UP2_HQ_0;
        let c1 = &t::RESAMPLER_UP2_HQ_1;
        for (k, &x) in input.iter().enumerate() {
            let in32 = i32::from(x) << 10;
            let mut y = in32.wrapping_sub(s[0]);
            let mut xx = smulwb(y, i32::from(c0[0]));
            let mut o1 = s[0].wrapping_add(xx);
            s[0] = in32.wrapping_add(xx);
            y = o1.wrapping_sub(s[1]);
            xx = smulwb(y, i32::from(c0[1]));
            let mut o2 = s[1].wrapping_add(xx);
            s[1] = o1.wrapping_add(xx);
            y = o2.wrapping_sub(s[2]);
            xx = smlawb(y, y, i32::from(c0[2]));
            o1 = s[2].wrapping_add(xx);
            s[2] = o2.wrapping_add(xx);
            out[2 * k] = sat16(rshift_round(o1, 10)) as i16;
            y = in32.wrapping_sub(s[3]);
            xx = smulwb(y, i32::from(c1[0]));
            o1 = s[3].wrapping_add(xx);
            s[3] = in32.wrapping_add(xx);
            y = o1.wrapping_sub(s[4]);
            xx = smulwb(y, i32::from(c1[1]));
            o2 = s[4].wrapping_add(xx);
            s[4] = o1.wrapping_add(xx);
            y = o2.wrapping_sub(s[5]);
            xx = smlawb(y, y, i32::from(c1[2]));
            o1 = s[5].wrapping_add(xx);
            s[5] = o2.wrapping_add(xx);
            out[2 * k + 1] = sat16(rshift_round(o1, 10)) as i16;
        }
    }

    /// `silk_resampler_private_IIR_FIR`; returns the samples written.
    fn iir_fir(&mut self, out: &mut [i16], mut input: &[i16]) -> usize {
        let mut buf = vec![0i16; 2 * self.batch_size + ORDER_FIR_12];
        buf[..ORDER_FIR_12].copy_from_slice(&self.s_fir);
        let inc = self.inv_ratio_q16;
        let mut o = 0usize;
        let mut n_in;
        loop {
            n_in = input.len().min(self.batch_size);
            Self::up2_hq(&mut self.s_iir, &mut buf[ORDER_FIR_12..], &input[..n_in]);
            let max_index_q16 = (n_in as i32) << 17;
            let mut idx = 0i32;
            while idx < max_index_q16 {
                let ti = smulwb(idx & 0xFFFF, 12) as usize;
                let b = &buf[(idx >> 16) as usize..];
                let f = |r: usize, c: usize| i32::from(t::RESAMPLER_FRAC_FIR_12[r * 4 + c]);
                let mut r = smulbb(i32::from(b[0]), f(ti, 0));
                r = smlabb(r, i32::from(b[1]), f(ti, 1));
                r = smlabb(r, i32::from(b[2]), f(ti, 2));
                r = smlabb(r, i32::from(b[3]), f(ti, 3));
                r = smlabb(r, i32::from(b[4]), f(11 - ti, 3));
                r = smlabb(r, i32::from(b[5]), f(11 - ti, 2));
                r = smlabb(r, i32::from(b[6]), f(11 - ti, 1));
                r = smlabb(r, i32::from(b[7]), f(11 - ti, 0));
                out[o] = sat16(rshift_round(r, 15)) as i16;
                o += 1;
                idx += inc;
            }
            input = &input[n_in..];
            if input.is_empty() {
                break;
            }
            buf.copy_within(n_in << 1..(n_in << 1) + ORDER_FIR_12, 0);
        }
        self.s_fir
            .copy_from_slice(&buf[n_in << 1..(n_in << 1) + ORDER_FIR_12]);
        o
    }
}
