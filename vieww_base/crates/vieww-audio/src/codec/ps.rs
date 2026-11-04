//! HE-AAC v2 parametric stereo (ISO/IEC 14496-3 §8.6.4): a mono SBR
//! channel plus a few inter-channel parameters per band and envelope —
//! intensity difference (IID), coherence (ICC) and, optionally, phase
//! differences (IPD/OPD) — become stereo.
//!
//! In the QMF domain, per frame: the low QMF bands are split further by the
//! hybrid analysis filters (10 or 32 sub-subbands for the 20- or 34-band
//! parameter layout); a decorrelated copy of the mono signal is made with
//! a fractional-delay all-pass chain and transient ducking; each band's
//! 2×2 mixing matrix (from IID/ICC, rotated by IPD/OPD) is interpolated
//! across each envelope and applied to (mono, decorrelated); the hybrid
//! bands are summed back.
//!
//! Tables: the hybrid prototypes come from [`sbr_tables`](super::sbr_tables);
//! the band maps are the standard's parameter-band groupings; the mixing
//! matrices, phase tables and fractional delays are computed from their
//! defining formulas.

// The filterbank, envelope and mixing loops walk several arrays by the same
// subband/slot index (the standard's own `k`, `l`, `n`); the iterator forms
// clippy suggests would hide which index addresses which table.
#![allow(clippy::needless_range_loop)]
use super::bits::MsbReader;
use super::sbr::{rd, Cx, SLOTS, X_SLOTS};
use super::sbr_tables as tab;

type Rd<'a> = MsbReader<'a>;

const MAX_ENV: usize = 5;
const NR_PAR_BANDS: [usize; 2] = [20, 34];
const NR_IPDOPD_BANDS: [usize; 2] = [11, 17];
const NR_BANDS: [usize; 2] = [71, 91];
const DECAY_CUTOFF: [usize; 2] = [10, 32];
const NR_ALLPASS_BANDS: [usize; 2] = [30, 50];
const SHORT_DELAY_BAND: [usize; 2] = [42, 62];
const MAX_DELAY: usize = 14;
const AP_LINKS: usize = 3;
const MAX_AP_DELAY: usize = 5;

/// Parameter band of each hybrid/QMF band (the standard's groupings).
fn k_to_i(is34: bool) -> &'static [u8] {
    static T: std::sync::OnceLock<[Vec<u8>; 2]> = std::sync::OnceLock::new();
    &T.get_or_init(|| {
        let build = |sub: &[u8], qmf_from: usize, borders: &[usize], first_bk: u8| {
            let mut v = sub.to_vec();
            for q in qmf_from..64 {
                let g = borders
                    .windows(2)
                    .position(|w| q >= w[0] && q < w[1])
                    .unwrap_or(0);
                v.push(first_bk + g as u8);
            }
            v
        };
        [
            build(
                &[1, 0, 0, 1, 2, 3, 4, 5, 6, 7],
                3,
                &[3, 4, 5, 6, 7, 8, 9, 11, 14, 18, 23, 35, 64],
                8,
            ),
            build(
                &[
                    0, 1, 2, 3, 4, 5, 6, 6, 7, 2, 1, 0, 10, 10, 4, 5, 6, 7, 8, 9, 10, 11, 12, 9,
                    14, 11, 12, 13, 14, 15, 16, 13,
                ],
                5,
                &[
                    5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 21, 24, 27, 30, 33, 37, 41, 64,
                ],
                16,
            ),
        ]
    })[usize::from(is34)]
}

struct Tables {
    /// Hybrid analysis filters, `[q][n]`, n = 0..7 (the symmetric half).
    f20_0_8: Vec<[Cx; 7]>,
    f34_0_12: Vec<[Cx; 7]>,
    f34_1_8: Vec<[Cx; 7]>,
    f34_2_4: Vec<[Cx; 7]>,
    /// `[iid][icc][h11, h12, h21, h22]`, mixing procedures A and B.
    ha: Vec<[[f32; 4]; 8]>,
    hb: Vec<[[f32; 4]; 8]>,
    pd_smooth: Vec<Cx>,
    phi_fract: [Vec<Cx>; 2],
    q_fract: [Vec<[Cx; 3]>; 2],
}

fn tables() -> &'static Tables {
    use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};
    static T: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let filters = |proto: &[f32; 7], bands: usize| -> Vec<[Cx; 7]> {
            (0..bands)
                .map(|q| {
                    let mut f = [Cx::ZERO; 7];
                    for (n, fv) in f.iter_mut().enumerate() {
                        let th = 2.0 * PI * (q as f64 + 0.5) * (n as f64 - 6.0) / bands as f64;
                        let p = f64::from(proto[n]);
                        *fv = Cx::new((p * th.cos()) as f32, (-p * th.sin()) as f32);
                    }
                    f
                })
                .collect()
        };
        // IID steps in dB (default, then fine), ICC values.
        let iid_db: Vec<f64> = [
            -25, -18, -14, -10, -7, -4, -2, 0, 2, 4, 7, 10, 14, 18, 25, -50, -45, -40, -35, -30,
            -25, -22, -19, -16, -13, -10, -8, -6, -4, -2, 0, 2, 4, 6, 8, 10, 13, 16, 19, 22, 25,
            30, 35, 40, 45, 50,
        ]
        .iter()
        .map(|&d| f64::from(d))
        .collect();
        let icc_invq = [1.0, 0.937, 0.84118, 0.60092, 0.36764, 0.0, -0.589, -1.0f64];
        let mut ha = Vec::new();
        let mut hb = Vec::new();
        for &db in &iid_db {
            let c = 10f64.powf(db / 20.0);
            let c1 = SQRT_2 / (1.0 + c * c).sqrt();
            let c2 = c * c1;
            let mut rowa = [[0f32; 4]; 8];
            let mut rowb = [[0f32; 4]; 8];
            for (icc, &rho0) in icc_invq.iter().enumerate() {
                let alpha = 0.5 * rho0.acos();
                let beta = alpha * (c1 - c2) * FRAC_1_SQRT_2;
                rowa[icc] = [
                    (c2 * (beta + alpha).cos()) as f32,
                    (c1 * (beta - alpha).cos()) as f32,
                    (c2 * (beta + alpha).sin()) as f32,
                    (c1 * (beta - alpha).sin()) as f32,
                ];
                let rho = rho0.max(0.05);
                let mut alpha = 0.5 * (2.0 * c * rho).atan2(c * c - 1.0);
                let mu = c + 1.0 / c;
                let mu = (1.0 + (4.0 * rho * rho - 4.0) / (mu * mu)).sqrt();
                let gamma = ((1.0 - mu) / (1.0 + mu)).sqrt().atan();
                if alpha < 0.0 {
                    alpha += PI / 2.0;
                }
                rowb[icc] = [
                    (SQRT_2 * alpha.cos() * gamma.cos()) as f32,
                    (SQRT_2 * alpha.sin() * gamma.cos()) as f32,
                    (-SQRT_2 * alpha.sin() * gamma.sin()) as f32,
                    (SQRT_2 * alpha.cos() * gamma.sin()) as f32,
                ];
            }
            ha.push(rowa);
            hb.push(rowb);
        }
        let pd = |i: usize| {
            let a = PI * i as f64 / 4.0;
            (a.cos(), a.sin())
        };
        let mut pd_smooth = Vec::with_capacity(512);
        for p0 in 0..8 {
            for p1 in 0..8 {
                for p2 in 0..8 {
                    let (a, b, c) = (pd(p0), pd(p1), pd(p2));
                    let re = 0.25 * a.0 + 0.5 * b.0 + c.0;
                    let im = 0.25 * a.1 + 0.5 * b.1 + c.1;
                    let mag = 1.0 / im.hypot(re);
                    pd_smooth.push(Cx::new((re * mag) as f32, (im * mag) as f32));
                }
            }
        }
        const LINKS: [f64; 3] = [0.43, 0.75, 0.347];
        const F20: [f64; 10] = [-3.0, -1.0, 1.0, 3.0, 5.0, 7.0, 10.0, 14.0, 18.0, 22.0];
        const F34: [f64; 32] = [
            2.0, 6.0, 10.0, 14.0, 18.0, 22.0, 26.0, 30.0, 34.0, -10.0, -6.0, -2.0, 51.0, 57.0,
            15.0, 21.0, 27.0, 33.0, 39.0, 45.0, 54.0, 66.0, 78.0, 42.0, 102.0, 66.0, 78.0, 90.0,
            102.0, 114.0, 126.0, 90.0,
        ];
        let mut phi_fract = [Vec::new(), Vec::new()];
        let mut q_fract = [Vec::new(), Vec::new()];
        for is34 in 0..2 {
            for k in 0..NR_ALLPASS_BANDS[is34] {
                let fc = if is34 == 0 {
                    if k < 10 {
                        F20[k] * 0.125
                    } else {
                        k as f64 - 6.5
                    }
                } else if k < 32 {
                    F34[k] / 24.0
                } else {
                    k as f64 - 26.5
                };
                let mut q = [Cx::ZERO; 3];
                for (m, qm) in q.iter_mut().enumerate() {
                    let th = -PI * LINKS[m] * fc;
                    *qm = Cx::new(th.cos() as f32, th.sin() as f32);
                }
                q_fract[is34].push(q);
                let th = -PI * 0.39 * fc;
                phi_fract[is34].push(Cx::new(th.cos() as f32, th.sin() as f32));
            }
        }
        Tables {
            f20_0_8: filters(&tab::P8_13_20, 8),
            f34_0_12: filters(&tab::P12_13_34, 12),
            f34_1_8: filters(&tab::P8_13_34, 8),
            f34_2_4: filters(&tab::P4_13_34, 4),
            ha,
            hb,
            pd_smooth,
            phi_fract,
            q_fract,
        }
    })
}

type Par = [[i8; 34]; MAX_ENV + 1];

/// Parametric-stereo state for one mono SBR channel.
pub(crate) struct Ps {
    start: bool,
    enable_iid: bool,
    iid_quant: bool,
    nr_iid_par: usize,
    nr_ipdopd_par: usize,
    enable_icc: bool,
    icc_mode: u32,
    nr_icc_par: usize,
    enable_ext: bool,
    enable_ipdopd: bool,
    num_env: usize,
    num_env_old: usize,
    border: [i32; MAX_ENV + 2],
    iid: Par,
    icc: Par,
    ipd: Par,
    opd: Par,
    is34: bool,
    is34_old: bool,
    in_buf: [[Cx; 44]; 5],
    delay: Vec<[Cx; SLOTS + MAX_DELAY]>,
    ap_delay: Vec<[[Cx; SLOTS + MAX_AP_DELAY]; AP_LINKS]>,
    peak_decay_nrg: [f32; 34],
    power_smooth: [f32; 34],
    peak_decay_diff_smooth: [f32; 34],
    /// `[re/im][env][band]` for h11, h12, h21, h22.
    h: [[[[f32; 34]; MAX_ENV + 1]; 2]; 4],
    ipd_hist: [u8; 17],
    opd_hist: [u8; 17],
}

impl Ps {
    pub(crate) fn new() -> Self {
        Self {
            start: false,
            enable_iid: false,
            iid_quant: false,
            nr_iid_par: 0,
            nr_ipdopd_par: 0,
            enable_icc: false,
            icc_mode: 0,
            nr_icc_par: 0,
            enable_ext: false,
            enable_ipdopd: false,
            num_env: 0,
            num_env_old: 0,
            border: [0; MAX_ENV + 2],
            iid: [[0; 34]; MAX_ENV + 1],
            icc: [[0; 34]; MAX_ENV + 1],
            ipd: [[0; 34]; MAX_ENV + 1],
            opd: [[0; 34]; MAX_ENV + 1],
            is34: false,
            is34_old: false,
            in_buf: [[Cx::ZERO; 44]; 5],
            delay: vec![[Cx::ZERO; SLOTS + MAX_DELAY]; 91],
            ap_delay: vec![[[Cx::ZERO; SLOTS + MAX_AP_DELAY]; AP_LINKS]; 50],
            peak_decay_nrg: [0.0; 34],
            power_smooth: [0.0; 34],
            peak_decay_diff_smooth: [0.0; 34],
            h: [[[[0.0; 34]; MAX_ENV + 1]; 2]; 4],
            ipd_hist: [0; 17],
            opd_hist: [0; 17],
        }
    }

    pub(crate) const fn started(&self) -> bool {
        self.start
    }

    /// Parse `ps_data` from at most `bits_left` bits; on any error the
    /// payload is skipped whole and PS waits for its next header.
    pub(crate) fn read(&mut self, r: &mut Rd<'_>, bits_left: usize) {
        let at = r.position();
        let ok = self.read_inner(r).is_some() && r.position() - at <= bits_left;
        if !ok {
            self.start = false;
            self.iid = [[0; 34]; MAX_ENV + 1];
            self.icc = [[0; 34]; MAX_ENV + 1];
            self.ipd = [[0; 34]; MAX_ENV + 1];
            self.opd = [[0; 34]; MAX_ENV + 1];
            r.seek(at);
            let _ = r.skip(bits_left);
        }
    }

    fn read_inner(&mut self, r: &mut Rd<'_>) -> Option<()> {
        const NR_IIDICC: [usize; 6] = [10, 20, 34, 10, 20, 34];
        const NR_IPDOPD: [usize; 6] = [5, 11, 17, 5, 11, 17];
        const NUM_ENV: [[usize; 4]; 2] = [[0, 1, 2, 4], [1, 2, 3, 4]];
        let header = rd(r, 1)? == 1;
        if header {
            self.enable_iid = rd(r, 1)? == 1;
            if self.enable_iid {
                let mode = rd(r, 3)? as usize;
                if mode > 5 {
                    return None;
                }
                self.nr_iid_par = NR_IIDICC[mode];
                self.iid_quant = mode > 2;
                self.nr_ipdopd_par = NR_IPDOPD[mode];
            }
            self.enable_icc = rd(r, 1)? == 1;
            if self.enable_icc {
                self.icc_mode = rd(r, 3)?;
                if self.icc_mode > 5 {
                    return None;
                }
                self.nr_icc_par = NR_IIDICC[self.icc_mode as usize];
            }
            self.enable_ext = rd(r, 1)? == 1;
        } else if !self.start {
            return None;
        }
        let frame_class = rd(r, 1)? as usize;
        self.num_env_old = self.num_env;
        self.num_env = NUM_ENV[frame_class][rd(r, 2)? as usize];
        self.border[0] = -1;
        if frame_class == 1 {
            for e in 1..=self.num_env {
                self.border[e] = rd(r, 5)? as i32;
                if self.border[e] < self.border[e - 1] {
                    return None;
                }
            }
        } else {
            let log2 = [0u32, 0, 1, 2, 2][self.num_env];
            for e in 1..=self.num_env {
                self.border[e] = ((e as i32 * SLOTS as i32) >> log2) - 1;
            }
        }
        if self.enable_iid {
            let fine = self.iid_quant;
            for e in 0..self.num_env {
                let dt = rd(r, 1)? == 1;
                let t: &[[i8; 2]] = match (dt, fine) {
                    (false, false) => &tab::PS_F_IID_DEF,
                    (true, false) => &tab::PS_T_IID_DEF,
                    (false, true) => &tab::PS_F_IID_FINE,
                    (true, true) => &tab::PS_T_IID_FINE,
                };
                let lim = if fine { 15 } else { 7 };
                read_par(
                    r,
                    &mut self.iid,
                    self.nr_iid_par,
                    t,
                    e,
                    dt,
                    self.num_env_old,
                    0,
                )?;
                if self.iid[e][..self.nr_iid_par]
                    .iter()
                    .any(|&v| i32::from(v).abs() > lim)
                {
                    return None;
                }
            }
        } else {
            self.iid = [[0; 34]; MAX_ENV + 1];
        }
        if self.enable_icc {
            for e in 0..self.num_env {
                let dt = rd(r, 1)? == 1;
                let t: &[[i8; 2]] = if dt { &tab::PS_T_ICC } else { &tab::PS_F_ICC };
                read_par(
                    r,
                    &mut self.icc,
                    self.nr_icc_par,
                    t,
                    e,
                    dt,
                    self.num_env_old,
                    0,
                )?;
                if self.icc[e][..self.nr_icc_par]
                    .iter()
                    .any(|&v| !(0..=7).contains(&v))
                {
                    return None;
                }
            }
        } else {
            self.icc = [[0; 34]; MAX_ENV + 1];
        }
        if self.enable_ext {
            let mut cnt = rd(r, 4)? as usize;
            if cnt == 15 {
                cnt += rd(r, 8)? as usize;
            }
            let mut cnt = cnt as isize * 8;
            while cnt > 7 {
                let id = rd(r, 2)?;
                let at = r.position();
                if id == 0 {
                    self.enable_ipdopd = rd(r, 1)? == 1;
                    if self.enable_ipdopd {
                        for e in 0..self.num_env {
                            let dt = rd(r, 1)? == 1;
                            let t: &[[i8; 2]] = if dt { &tab::PS_T_IPD } else { &tab::PS_F_IPD };
                            read_par(
                                r,
                                &mut self.ipd,
                                self.nr_ipdopd_par,
                                t,
                                e,
                                dt,
                                self.num_env_old,
                                7,
                            )?;
                            let dt = rd(r, 1)? == 1;
                            let t: &[[i8; 2]] = if dt { &tab::PS_T_OPD } else { &tab::PS_F_OPD };
                            read_par(
                                r,
                                &mut self.opd,
                                self.nr_ipdopd_par,
                                t,
                                e,
                                dt,
                                self.num_env_old,
                                7,
                            )?;
                        }
                    }
                    rd(r, 1)?;
                }
                cnt -= 2 + (r.position() - at) as isize;
            }
            if cnt < 0 {
                return None;
            }
            r.skip(cnt as usize)?;
        }
        // The last envelope must end at the frame's end: add one that
        // repeats the last (or the previous frame's) parameters.
        if self.num_env == 0 || self.border[self.num_env] < SLOTS as i32 - 1 {
            let source = if self.num_env > 0 {
                self.num_env as isize - 1
            } else {
                self.num_env_old as isize - 1
            };
            if source >= 0 && source as usize != self.num_env {
                let (s, d) = (source as usize, self.num_env);
                if self.enable_iid {
                    self.iid[d] = self.iid[s];
                }
                if self.enable_icc {
                    self.icc[d] = self.icc[s];
                }
                if self.enable_ipdopd {
                    self.ipd[d] = self.ipd[s];
                    self.opd[d] = self.opd[s];
                }
            }
            let lim = if self.iid_quant { 15 } else { 7 };
            if self.enable_iid
                && self.iid[self.num_env][..self.nr_iid_par]
                    .iter()
                    .any(|&v| i32::from(v).abs() > lim)
            {
                return None;
            }
            if self.enable_icc
                && self.icc[self.num_env][..self.nr_icc_par]
                    .iter()
                    .any(|&v| !(0..=7).contains(&v))
            {
                return None;
            }
            self.num_env += 1;
            self.border[self.num_env] = SLOTS as i32 - 1;
        }
        self.is34_old = self.is34;
        if self.enable_iid || self.enable_icc {
            self.is34 = (self.enable_iid && self.nr_iid_par == 34)
                || (!self.enable_iid && self.nr_icc_par == 34);
        }
        if !self.enable_ipdopd {
            self.ipd = [[0; 34]; MAX_ENV + 1];
            self.opd = [[0; 34]; MAX_ENV + 1];
        }
        if header {
            self.start = true;
        }
        Some(())
    }

    /// Turn `l` (the mono QMF signal, `top` bands wide) into the stereo
    /// pair, in place in `l` and `r`.
    pub(crate) fn apply(
        &mut self,
        l: &mut [[Cx; 64]; X_SLOTS],
        r: &mut [[Cx; 64]; X_SLOTS],
        top: usize,
    ) {
        let is34 = usize::from(self.is34);
        let top = (top + NR_BANDS[is34])
            .saturating_sub(64)
            .min(NR_BANDS[is34]);
        for d in &mut self.delay[top..NR_BANDS[is34]] {
            *d = [Cx::ZERO; SLOTS + MAX_DELAY];
        }
        if top < NR_ALLPASS_BANDS[is34] {
            for d in &mut self.ap_delay[top..NR_ALLPASS_BANDS[is34]] {
                *d = [[Cx::ZERO; SLOTS + MAX_AP_DELAY]; AP_LINKS];
            }
        }
        let mut lbuf = vec![[Cx::ZERO; SLOTS]; 91];
        let mut rbuf = vec![[Cx::ZERO; SLOTS]; 91];
        self.hybrid_analysis(&mut lbuf, l);
        self.decorrelation(&mut rbuf, &lbuf);
        self.stereo(&mut lbuf, &mut rbuf);
        hybrid_synthesis(l, &lbuf, self.is34);
        hybrid_synthesis(r, &rbuf, self.is34);
    }

    fn hybrid_analysis(&mut self, out: &mut [[Cx; SLOTS]], l: &[[Cx; 64]; X_SLOTS]) {
        let t = tables();
        for i in 0..5 {
            for j in 0..X_SLOTS {
                self.in_buf[i][j + 6] = l[j][i];
            }
        }
        let filt = |input: &[Cx; 44], f: &[[Cx; 7]], n: usize| -> Vec<[Cx; SLOTS]> {
            let mut o = vec![[Cx::ZERO; SLOTS]; n];
            for s in 0..SLOTS {
                let x = &input[s..s + 13];
                for (q, fq) in f.iter().enumerate().take(n) {
                    let mut re = fq[6].re * x[6].re;
                    let mut im = fq[6].re * x[6].im;
                    for j in 0..6 {
                        let (a, b) = (x[j], x[12 - j]);
                        re += fq[j].re * (a.re + b.re) - fq[j].im * (a.im - b.im);
                        im += fq[j].re * (a.im + b.im) + fq[j].im * (a.re - b.re);
                    }
                    o[q][s] = Cx::new(re, im);
                }
            }
            o
        };
        if self.is34 {
            let mut at = 0;
            for (band, f, n) in [
                (0usize, &t.f34_0_12, 12usize),
                (1, &t.f34_1_8, 8),
                (2, &t.f34_2_4, 4),
                (3, &t.f34_2_4, 4),
                (4, &t.f34_2_4, 4),
            ] {
                for (q, row) in filt(&self.in_buf[band], f, n).into_iter().enumerate() {
                    out[at + q] = row;
                }
                at += n;
            }
            for q in 5..64 {
                for s in 0..SLOTS {
                    out[27 + q][s] = l[s][q];
                }
            }
        } else {
            let t8 = filt(&self.in_buf[0], &t.f20_0_8, 8);
            // The eight sub-subbands of QMF band 0 become six: the four
            // nearest DC stay single, the two pairs mirrored about DC are
            // summed (they carry one real band's two halves).
            for s in 0..SLOTS {
                out[0][s] = t8[6][s];
                out[1][s] = t8[7][s];
                out[2][s] = t8[0][s];
                out[3][s] = t8[1][s];
                out[4][s] = t8[2][s].add(t8[5][s]);
                out[5][s] = t8[3][s].add(t8[4][s]);
            }
            hybrid2(&self.in_buf[1], out, 6, true);
            hybrid2(&self.in_buf[2], out, 8, false);
            for q in 3..64 {
                for s in 0..SLOTS {
                    out[7 + q][s] = l[s][q];
                }
            }
        }
        for b in &mut self.in_buf {
            b.copy_within(32..38, 0);
        }
    }

    fn decorrelation(&mut self, out: &mut [[Cx; SLOTS]], s: &[[Cx; SLOTS]]) {
        const PEAK_DECAY: f32 = 0.765_928_3;
        const A_SMOOTH: f32 = 0.25;
        const TRANSIENT_IMPACT: f32 = 1.5;
        const A: [f32; 3] = [0.651_439_1, 0.564_718_1, 0.489_541_66];
        let t = tables();
        let is34 = usize::from(self.is34);
        let k2i = k_to_i(self.is34);
        if self.is34 != self.is34_old {
            self.peak_decay_nrg = [0.0; 34];
            self.power_smooth = [0.0; 34];
            self.peak_decay_diff_smooth = [0.0; 34];
            self.delay.fill([Cx::ZERO; SLOTS + MAX_DELAY]);
            self.ap_delay
                .fill([[Cx::ZERO; SLOTS + MAX_AP_DELAY]; AP_LINKS]);
        }
        let mut power = [[0f32; SLOTS]; 34];
        for k in 0..NR_BANDS[is34] {
            let i = usize::from(k2i[k]);
            for n in 0..SLOTS {
                power[i][n] += s[k][n].norm2();
            }
        }
        let mut gain = [[0f32; SLOTS]; 34];
        for i in 0..NR_PAR_BANDS[is34] {
            for n in 0..SLOTS {
                let decayed = PEAK_DECAY * self.peak_decay_nrg[i];
                self.peak_decay_nrg[i] = decayed.max(power[i][n]);
                self.power_smooth[i] += A_SMOOTH * (power[i][n] - self.power_smooth[i]);
                self.peak_decay_diff_smooth[i] += A_SMOOTH
                    * (self.peak_decay_nrg[i] - power[i][n] - self.peak_decay_diff_smooth[i]);
                let denom = TRANSIENT_IMPACT * self.peak_decay_diff_smooth[i];
                gain[i][n] = if denom > self.power_smooth[i] {
                    self.power_smooth[i] / denom
                } else {
                    1.0
                };
            }
        }
        let shift = |d: &mut [Cx; SLOTS + MAX_DELAY], src: &[Cx; SLOTS]| {
            d.copy_within(SLOTS..SLOTS + MAX_DELAY, 0);
            d[MAX_DELAY..].copy_from_slice(src);
        };
        let mut k = 0;
        while k < NR_ALLPASS_BANDS[is34] {
            let b = usize::from(k2i[k]);
            let slope = (1.0 - 0.05 * (k as f32 - DECAY_CUTOFF[is34] as f32)).clamp(0.0, 1.0);
            shift(&mut self.delay[k], &s[k]);
            for m in 0..AP_LINKS {
                self.ap_delay[k][m].copy_within(SLOTS..SLOTS + MAX_AP_DELAY, 0);
            }
            let phi = t.phi_fract[is34][k];
            let q = t.q_fract[is34][k];
            let ag = [A[0] * slope, A[1] * slope, A[2] * slope];
            for n in 0..SLOTS {
                let mut v = self.delay[k][n + MAX_DELAY - 2].mul(phi);
                for m in 0..AP_LINKS {
                    let a = v.scale(ag[m]);
                    let link = self.ap_delay[k][m][n + 2 - m];
                    let apd = v;
                    v = link.mul(q[m]);
                    v.re -= a.re;
                    v.im -= a.im;
                    self.ap_delay[k][m][n + 5] = apd.add(v.scale(ag[m]));
                }
                out[k][n] = v.scale(gain[b][n]);
            }
            k += 1;
        }
        while k < SHORT_DELAY_BAND[is34] {
            let i = usize::from(k2i[k]);
            shift(&mut self.delay[k], &s[k]);
            for n in 0..SLOTS {
                out[k][n] = self.delay[k][n + MAX_DELAY - 14].scale(gain[i][n]);
            }
            k += 1;
        }
        while k < NR_BANDS[is34] {
            let i = usize::from(k2i[k]);
            shift(&mut self.delay[k], &s[k]);
            for n in 0..SLOTS {
                out[k][n] = self.delay[k][n + MAX_DELAY - 1].scale(gain[i][n]);
            }
            k += 1;
        }
    }

    fn stereo(&mut self, l: &mut [[Cx; SLOTS]], r: &mut [[Cx; SLOTS]]) {
        let t = tables();
        let is34 = self.is34;
        let i34 = usize::from(is34);
        let k2i = k_to_i(is34);
        let lut = if self.icc_mode < 3 { &t.ha } else { &t.hb };
        if self.num_env_old > 0 {
            for hh in &mut self.h {
                for ri in hh.iter_mut() {
                    ri[0] = ri[self.num_env_old.min(MAX_ENV)];
                }
            }
        }
        let map = |par: &Par, num: usize, full: bool| -> Par {
            let mut out = [[0i8; 34]; MAX_ENV + 1];
            for e in 0..MAX_ENV + 1 {
                out[e] = if is34 {
                    match num {
                        20 | 11 => map_20_to_34(&par[e], full),
                        10 | 5 => map_10_to_34(&par[e], full),
                        _ => par[e],
                    }
                } else {
                    match num {
                        34 | 17 => map_34_to_20(&par[e], full),
                        10 | 5 => map_10_to_20(&par[e], full),
                        _ => par[e],
                    }
                };
            }
            out
        };
        let iid = map(&self.iid, self.nr_iid_par, true);
        let icc = map(&self.icc, self.nr_icc_par, true);
        let (ipd, opd) = if self.enable_ipdopd {
            (
                map(&self.ipd, self.nr_ipdopd_par, false),
                map(&self.opd, self.nr_ipdopd_par, false),
            )
        } else {
            ([[0; 34]; MAX_ENV + 1], [[0; 34]; MAX_ENV + 1])
        };
        if is34 != self.is34_old {
            for hh in &mut self.h {
                for ri in hh.iter_mut() {
                    let v = &mut ri[0];
                    if is34 {
                        val_20_to_34(v);
                    } else {
                        val_34_to_20(v);
                    }
                }
            }
            self.ipd_hist = [0; 17];
            self.opd_hist = [0; 17];
        }
        let quant = if self.iid_quant { 23 } else { 0 };
        for e in 0..self.num_env {
            for b in 0..NR_PAR_BANDS[i34] {
                let idx = (i32::from(iid[e][b]) + 7 + quant).clamp(0, 45) as usize;
                let ic = (icc[e][b].clamp(0, 7)) as usize;
                let mut hv = lut[idx][ic];
                if self.enable_ipdopd && b < NR_IPDOPD_BANDS[i34] {
                    let opd_idx = usize::from(self.opd_hist[b]) * 8 + (opd[e][b] & 7) as usize;
                    let ipd_idx = usize::from(self.ipd_hist[b]) * 8 + (ipd[e][b] & 7) as usize;
                    let o = t.pd_smooth[opd_idx];
                    let p = t.pd_smooth[ipd_idx];
                    self.opd_hist[b] = (opd_idx & 0x3f) as u8;
                    self.ipd_hist[b] = (ipd_idx & 0x3f) as u8;
                    let adj = Cx::new(o.re * p.re + o.im * p.im, o.im * p.re - o.re * p.im);
                    let im = [hv[0] * o.im, hv[1] * adj.im, hv[2] * o.im, hv[3] * adj.im];
                    hv = [hv[0] * o.re, hv[1] * adj.re, hv[2] * o.re, hv[3] * adj.re];
                    for j in 0..4 {
                        self.h[j][1][e + 1][b] = im[j];
                    }
                }
                for j in 0..4 {
                    self.h[j][0][e + 1][b] = hv[j];
                }
            }
            let start = self.border[e];
            let stop = self.border[e + 1];
            let len = (stop - start).max(0) as usize;
            if len == 0 {
                continue;
            }
            let width = 1.0 / len as f32;
            for k in 0..NR_BANDS[i34] {
                let b = usize::from(k2i[k]);
                let mut h0 = [0f32; 4];
                let mut h1 = [0f32; 4];
                let mut s0 = [0f32; 4];
                let mut s1 = [0f32; 4];
                let negate = (is34 && (9..=11).contains(&k)) || (!is34 && k <= 1);
                for j in 0..4 {
                    h0[j] = self.h[j][0][e][b];
                    s0[j] = (self.h[j][0][e + 1][b] - h0[j]) * width;
                    if self.enable_ipdopd {
                        let sign = if negate { -1.0 } else { 1.0 };
                        h1[j] = sign * self.h[j][1][e][b];
                        s1[j] = (sign * self.h[j][1][e + 1][b] - h1[j]) * width;
                    }
                }
                let first = (start + 1) as usize;
                for n in first..(first + len).min(SLOTS) {
                    for j in 0..4 {
                        h0[j] += s0[j];
                        h1[j] += s1[j];
                    }
                    let (lv, rv) = (l[k][n], r[k][n]);
                    l[k][n] = Cx::new(
                        h0[0] * lv.re + h0[2] * rv.re - h1[0] * lv.im - h1[2] * rv.im,
                        h0[0] * lv.im + h0[2] * rv.im + h1[0] * lv.re + h1[2] * rv.re,
                    );
                    r[k][n] = Cx::new(
                        h0[1] * lv.re + h0[3] * rv.re - h1[1] * lv.im - h1[3] * rv.im,
                        h0[1] * lv.im + h0[3] * rv.im + h1[1] * lv.re + h1[3] * rv.re,
                    );
                }
            }
        }
    }
}

/// The real two-band hybrid filter (QMF bands 1 and 2 of the 20-band
/// layout); `reverse` swaps which output is the upper half.
fn hybrid2(input: &[Cx; 44], out: &mut [[Cx; SLOTS]], at: usize, reverse: bool) {
    let f = &tab::P2_13_20;
    for s in 0..SLOTS {
        let x = &input[s..s + 13];
        let re_in = f[6] * x[6].re;
        let im_in = f[6] * x[6].im;
        let (mut re_op, mut im_op) = (0.0, 0.0);
        for j in (0..6).step_by(2) {
            re_op += f[j + 1] * (x[j + 1].re + x[12 - j - 1].re);
            im_op += f[j + 1] * (x[j + 1].im + x[12 - j - 1].im);
        }
        let (a, b) = if reverse { (at + 1, at) } else { (at, at + 1) };
        out[a][s] = Cx::new(re_in + re_op, im_in + im_op);
        out[b][s] = Cx::new(re_in - re_op, im_in - im_op);
    }
}

fn hybrid_synthesis(out: &mut [[Cx; 64]; X_SLOTS], input: &[[Cx; SLOTS]], is34: bool) {
    let sum = |range: std::ops::Range<usize>, n: usize| {
        range.fold(Cx::ZERO, |acc, i| acc.add(input[i][n]))
    };
    for n in 0..SLOTS {
        if is34 {
            out[n][0] = sum(0..12, n);
            out[n][1] = sum(12..20, n);
            out[n][2] = sum(20..24, n);
            out[n][3] = sum(24..28, n);
            out[n][4] = sum(28..32, n);
            for q in 5..64 {
                out[n][q] = input[27 + q][n];
            }
        } else {
            out[n][0] = sum(0..6, n);
            out[n][1] = sum(6..8, n);
            out[n][2] = sum(8..10, n);
            for q in 3..64 {
                out[n][q] = input[7 + q][n];
            }
        }
    }
}

/// `read_par`: one envelope of a parameter, delta-coded in frequency or
/// in time (against the previous envelope, or the previous frame's last).
#[allow(clippy::too_many_arguments)]
fn read_par(
    r: &mut Rd<'_>,
    par: &mut Par,
    num: usize,
    t: &[[i8; 2]],
    e: usize,
    dt: bool,
    num_env_old: usize,
    mask: i32,
) -> Option<()> {
    let huff = |r: &mut Rd<'_>| -> Option<i32> {
        let mut i = 0usize;
        loop {
            let bit = usize::from(r.bit()?);
            let n = *t.get(i)?.get(bit)?;
            if n < 0 {
                return Some(i32::from(n) + 31);
            }
            i = n as usize;
        }
    };
    if dt {
        let prev = if e > 0 {
            e - 1
        } else {
            num_env_old.saturating_sub(1)
        };
        for b in 0..num {
            let mut v = i32::from(par[prev][b]) + huff(r)?;
            if mask != 0 {
                v &= mask;
            }
            par[e][b] = v.clamp(-128, 127) as i8;
        }
    } else {
        let mut v = 0i32;
        for b in 0..num {
            v += huff(r)?;
            if mask != 0 {
                v &= mask;
            }
            if !(-128..=127).contains(&v) {
                return None;
            }
            par[e][b] = v as i8;
        }
    }
    Some(())
}

/// Table 8.46's parameter remaps.
fn map_10_to_20(p: &[i8; 34], full: bool) -> [i8; 34] {
    let mut o = [0i8; 34];
    let top = if full { 9 } else { 4 };
    for b in (0..=top).rev() {
        o[2 * b] = p[b];
        o[2 * b + 1] = p[b];
    }
    o
}

fn map_34_to_20(p: &[i8; 34], full: bool) -> [i8; 34] {
    let v = |i: usize| i32::from(p[i]);
    let mut o = [0i8; 34];
    let f = |x: i32| x as i8;
    o[0] = f((2 * v(0) + v(1)) / 3);
    o[1] = f((v(1) + 2 * v(2)) / 3);
    o[2] = f((2 * v(3) + v(4)) / 3);
    o[3] = f((v(4) + 2 * v(5)) / 3);
    o[4] = f((v(6) + v(7)) / 2);
    o[5] = f((v(8) + v(9)) / 2);
    o[6] = p[10];
    o[7] = p[11];
    o[8] = f((v(12) + v(13)) / 2);
    o[9] = f((v(14) + v(15)) / 2);
    o[10] = p[16];
    if full {
        o[11] = p[17];
        o[12] = p[18];
        o[13] = p[19];
        o[14] = f((v(20) + v(21)) / 2);
        o[15] = f((v(22) + v(23)) / 2);
        o[16] = f((v(24) + v(25)) / 2);
        o[17] = f((v(26) + v(27)) / 2);
        o[18] = f((v(28) + v(29) + v(30) + v(31)) / 4);
        o[19] = f((v(32) + v(33)) / 2);
    }
    o
}

fn map_10_to_34(p: &[i8; 34], full: bool) -> [i8; 34] {
    let mut o = [0i8; 34];
    if full {
        for i in 28..34 {
            o[i] = p[9];
        }
        for i in 24..28 {
            o[i] = p[8];
        }
        for i in 20..24 {
            o[i] = p[7];
        }
        o[18] = p[6];
        o[19] = p[6];
        o[16] = p[5];
        o[17] = p[5];
    }
    for i in 12..16 {
        o[i] = p[4];
    }
    o[10] = p[3];
    o[11] = p[3];
    for i in 6..10 {
        o[i] = p[2];
    }
    for i in 3..6 {
        o[i] = p[1];
    }
    for i in 0..3 {
        o[i] = p[0];
    }
    o
}

fn map_20_to_34(p: &[i8; 34], full: bool) -> [i8; 34] {
    let mut o = [0i8; 34];
    if full {
        o[33] = p[19];
        o[32] = p[19];
        o[31] = p[18];
        o[30] = p[18];
        o[29] = p[18];
        o[28] = p[18];
        o[27] = p[17];
        o[26] = p[17];
        o[25] = p[16];
        o[24] = p[16];
        o[23] = p[15];
        o[22] = p[15];
        o[21] = p[14];
        o[20] = p[14];
        o[19] = p[13];
        o[18] = p[12];
        o[17] = p[11];
    }
    o[16] = p[10];
    o[15] = p[9];
    o[14] = p[9];
    o[13] = p[8];
    o[12] = p[8];
    o[11] = p[7];
    o[10] = p[6];
    o[9] = p[5];
    o[8] = p[5];
    o[7] = p[4];
    o[6] = p[4];
    o[5] = p[3];
    o[4] = ((i32::from(p[2]) + i32::from(p[3])) / 2) as i8;
    o[3] = p[2];
    o[2] = p[1];
    o[1] = ((i32::from(p[0]) + i32::from(p[1])) / 2) as i8;
    o[0] = p[0];
    o
}

fn val_34_to_20(p: &mut [f32; 34]) {
    let q = *p;
    p[0] = (2.0 * q[0] + q[1]) * 0.333_333_33;
    p[1] = (q[1] + 2.0 * q[2]) * 0.333_333_33;
    p[2] = (2.0 * q[3] + q[4]) * 0.333_333_33;
    p[3] = (q[4] + 2.0 * q[5]) * 0.333_333_33;
    p[4] = (q[6] + q[7]) * 0.5;
    p[5] = (q[8] + q[9]) * 0.5;
    p[6] = q[10];
    p[7] = q[11];
    p[8] = (q[12] + q[13]) * 0.5;
    p[9] = (q[14] + q[15]) * 0.5;
    p[10] = q[16];
    p[11] = q[17];
    p[12] = q[18];
    p[13] = q[19];
    p[14] = (q[20] + q[21]) * 0.5;
    p[15] = (q[22] + q[23]) * 0.5;
    p[16] = (q[24] + q[25]) * 0.5;
    p[17] = (q[26] + q[27]) * 0.5;
    p[18] = (q[28] + q[29] + q[30] + q[31]) * 0.25;
    p[19] = (q[32] + q[33]) * 0.5;
}

fn val_20_to_34(p: &mut [f32; 34]) {
    let q = *p;
    p[33] = q[19];
    p[32] = q[19];
    p[31] = q[18];
    p[30] = q[18];
    p[29] = q[18];
    p[28] = q[18];
    p[27] = q[17];
    p[26] = q[17];
    p[25] = q[16];
    p[24] = q[16];
    p[23] = q[15];
    p[22] = q[15];
    p[21] = q[14];
    p[20] = q[14];
    p[19] = q[13];
    p[18] = q[12];
    p[17] = q[11];
    p[16] = q[10];
    p[15] = q[9];
    p[14] = q[9];
    p[13] = q[8];
    p[12] = q[8];
    p[11] = q[7];
    p[10] = q[6];
    p[9] = q[5];
    p[8] = q[5];
    p[7] = q[4];
    p[6] = q[4];
    p[5] = q[3];
    p[4] = (q[2] + q[3]) * 0.5;
    p[3] = q[2];
    p[2] = q[1];
    p[1] = (q[0] + q[1]) * 0.5;
    p[0] = q[0];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_maps_cover_every_hybrid_and_qmf_band() {
        assert_eq!(k_to_i(false).len(), 71);
        assert_eq!(k_to_i(true).len(), 91);
        assert_eq!(k_to_i(false)[70], 19);
        assert_eq!(k_to_i(true)[90], 33);
        assert!(k_to_i(false).iter().all(|&b| b < 20));
        assert!(k_to_i(true).iter().all(|&b| b < 34));
    }

    #[test]
    fn mixing_matrix_a_is_energy_preserving_at_full_coherence() {
        // ICC index 0 (coherence 1): h11² + h21² = c2², h12² + h22² = c1²,
        // and c1² + c2² = 2 — the mono energy shared between the channels.
        for row in &tables().ha {
            let h = row[0];
            let e = h[0] * h[0] + h[2] * h[2] + h[1] * h[1] + h[3] * h[3];
            assert!((e - 2.0).abs() < 1e-4, "{e}");
        }
    }

    #[test]
    fn a_hostile_header_is_refused_not_panicked() {
        let mut ps = Ps::new();
        let bytes = [0xff; 16];
        let mut r = Rd::new(&bytes);
        ps.read(&mut r, 100);
        assert!(!ps.started());
        assert_eq!(r.position(), 100);
    }
}
