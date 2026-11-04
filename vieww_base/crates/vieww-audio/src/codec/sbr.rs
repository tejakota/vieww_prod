//! HE-AAC: spectral band replication (ISO/IEC 14496-3 §4.6.18) and, through
//! [`ps`](super::ps), parametric stereo (§8.6.4) — AAC object types 5 and 29.
//!
//! The AAC-LC core decodes at half the output rate; SBR rebuilds the upper
//! band from the lower one. Per channel and per 1024-sample core frame:
//!
//! 1. a 32-band complex QMF analysis of the core's output (§4.6.18.4.1);
//! 2. HF generation — the low band's subbands copied up through the patch
//!    table, whitened by a per-band second-order LPC inverse filter whose
//!    chirp the bitstream sets (§4.6.18.6);
//! 3. envelope adjustment — the patched band's energy measured per
//!    time/frequency envelope and replaced by the transmitted one, with
//!    noise floor and sinusoids added and the limiter bounding the gains
//!    (§4.6.18.7);
//! 4. a 64-band complex QMF synthesis at twice the core rate (§4.6.18.4.2).
//!
//! The time-slot bookkeeping (the 8 slots of low band carried over from the
//! previous frame, the 2-slot envelope adjustment offset, the previous
//! frame's high band for the slots its last envelope reaches into) follows
//! the structure the conformance decoders use, so the output is
//! sample-aligned with ffmpeg's — which is what `tests/fixtures`' references
//! are, graded by SNR like the LC decoder.
//!
//! A malformed SBR payload switches SBR off until the next header (the
//! output keeps playing, upsampled, low band only) — it never fails the
//! decode; the core's own errors still do.
//!
//! Tables: [`sbr_tables`](super::sbr_tables) carries only what the standard
//! tabulates (the QMF prototype, the noise table, the Huffman books, the
//! hybrid filter taps); everything with a closed form is computed here.

// The filterbank, envelope and mixing loops walk several arrays by the same
// subband/slot index (the standard's own `k`, `l`, `n`); the iterator forms
// clippy suggests would hide which index addresses which table.
#![allow(clippy::needless_range_loop)]
use super::bits::MsbReader;
use super::ps::Ps;
use super::sbr_tables as tab;

type Rd<'a> = MsbReader<'a>;

/// A complex sample.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Cx {
    pub(crate) re: f32,
    pub(crate) im: f32,
}

impl Cx {
    pub(crate) const ZERO: Self = Self { re: 0.0, im: 0.0 };
    pub(crate) const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }
    pub(crate) fn mul(self, o: Self) -> Self {
        Self::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
    pub(crate) fn conj_mul(self, o: Self) -> Self {
        // self * conj(o)
        Self::new(
            self.re * o.re + self.im * o.im,
            self.im * o.re - self.re * o.im,
        )
    }
    pub(crate) fn scale(self, s: f32) -> Self {
        Self::new(self.re * s, self.im * s)
    }
    pub(crate) fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
    pub(crate) fn norm2(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

/// QMF time slots in one 1024-sample core frame (`numTimeSlots · RATE`).
pub(crate) const SLOTS: usize = 32;
/// Low-band slots carried over from the previous frame (`t_HFGen`).
const T_HFGEN: usize = 8;
/// The envelope adjustment's time offset (`t_HFAdj`).
const T_HFADJ: usize = 2;
/// Slots in the synthesis input: 32 plus the 6 the PS hybrid filter reads
/// ahead.
pub(crate) const X_SLOTS: usize = 38;
const MAX_ENV: usize = 5;
const MAX_BANDS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spectrum {
    start_freq: u8,
    stop_freq: u8,
    xover_band: u8,
    freq_scale: u8,
    alter_scale: u8,
    noise_bands: u8,
}

/// One channel's SBR state: what was parsed this frame and what carries.
#[derive(Clone)]
struct Channel {
    // ---- parsed ----
    frame_class: u8,
    amp_res: bool,
    num_env: usize,
    num_noise: usize,
    /// `freq_res[0]` is the previous frame's last envelope's.
    freq_res: [u8; MAX_ENV + 1],
    pointer: usize,
    t_env: [u8; MAX_ENV + 1],
    t_env_num_env_old: u8,
    t_q: [u8; 3],
    df_env: [bool; MAX_ENV],
    df_noise: [bool; 2],
    /// `[0]` this frame's, `[1]` the previous frame's.
    invf_mode: [[u8; 5]; 2],
    /// Envelope scalefactors; row 0 is the previous frame's last.
    env_q: [[i32; MAX_BANDS]; MAX_ENV + 1],
    noise_q: [[i32; 5]; 3],
    env: [[f32; MAX_BANDS]; MAX_ENV + 1],
    noise: [[f32; 5]; 3],
    add_harmonic_flag: bool,
    add_harmonic: [bool; MAX_BANDS],
    /// Transient envelope indices: `[1]` this frame's, `[0]` from the last.
    e_a: [i32; 2],
    // ---- carried across frames ----
    bw: [f32; 5],
    s_indexmapped: [[bool; MAX_BANDS]; MAX_ENV + 1],
    g_temp: [[f32; MAX_BANDS]; 42],
    q_temp: [[f32; MAX_BANDS]; 42],
    f_indexnoise: usize,
    f_indexsine: usize,
    /// QMF analysis outputs, this frame's and the previous frame's.
    w: [[[Cx; 32]; SLOTS]; 2],
    /// HF-adjusted high band, this frame's and the previous frame's.
    y: [Box<[[Cx; 64]; X_SLOTS]>; 2],
    /// Which of `w`/`y` is this frame's.
    pos: usize,
    analysis: [f32; 320],
}

impl Channel {
    fn new() -> Self {
        Self {
            frame_class: 0,
            amp_res: false,
            num_env: 0,
            num_noise: 0,
            freq_res: [0; MAX_ENV + 1],
            pointer: 0,
            t_env: [0; MAX_ENV + 1],
            t_env_num_env_old: 0,
            t_q: [0; 3],
            df_env: [false; MAX_ENV],
            df_noise: [false; 2],
            invf_mode: [[0; 5]; 2],
            env_q: [[0; MAX_BANDS]; MAX_ENV + 1],
            noise_q: [[0; 5]; 3],
            env: [[0.0; MAX_BANDS]; MAX_ENV + 1],
            noise: [[0.0; 5]; 3],
            add_harmonic_flag: false,
            add_harmonic: [false; MAX_BANDS],
            e_a: [-1, -1],
            bw: [0.0; 5],
            s_indexmapped: [[false; MAX_BANDS]; MAX_ENV + 1],
            g_temp: [[0.0; MAX_BANDS]; 42],
            q_temp: [[0.0; MAX_BANDS]; 42],
            f_indexnoise: 0,
            f_indexsine: 0,
            w: [[[Cx::ZERO; 32]; SLOTS]; 2],
            y: [
                Box::new([[Cx::ZERO; 64]; X_SLOTS]),
                Box::new([[Cx::ZERO; 64]; X_SLOTS]),
            ],
            pos: 0,
            analysis: [0.0; 320],
        }
    }
}

/// The QMF banks' twiddles, built once.
struct Qmf {
    /// Analysis: `2·exp(iπ/64·(k+½)·(2n−½))`, `[k][n]`, 32 × 64.
    ana: Vec<Cx>,
    /// Synthesis: `exp(iπ/128·(k+½)·(2n−255))/64`, `[n][k]`, 128 × 64.
    syn: Vec<Cx>,
}

fn qmf() -> &'static Qmf {
    use std::f64::consts::PI;
    static Q: std::sync::OnceLock<Qmf> = std::sync::OnceLock::new();
    Q.get_or_init(|| {
        let mut ana = Vec::with_capacity(32 * 64);
        for k in 0..32 {
            for n in 0..64 {
                let a = PI / 64.0 * (k as f64 + 0.5) * (2.0 * n as f64 - 0.5);
                ana.push(Cx::new((2.0 * a.cos()) as f32, (2.0 * a.sin()) as f32));
            }
        }
        let mut syn = Vec::with_capacity(128 * 64);
        for n in 0..128 {
            for k in 0..64 {
                let a = PI / 128.0 * (k as f64 + 0.5) * (2.0 * n as f64 - 255.0);
                syn.push(Cx::new((a.cos() / 64.0) as f32, (a.sin() / 64.0) as f32));
            }
        }
        Qmf { ana, syn }
    })
}

/// §4.6.18.4.1, one frame: 1024 samples in, 32 slots × 32 bands out.
fn analysis(state: &mut [f32; 320], input: &[f32], out: &mut [[Cx; 32]; SLOTS]) {
    let q = qmf();
    let c = &tab::QMF_C;
    for (l, slot) in out.iter_mut().enumerate() {
        // Shift by 32, newest sample first.
        state.copy_within(0..288, 32);
        for n in 0..32 {
            state[31 - n] = input[l * 32 + n];
        }
        let mut u = [0f32; 64];
        for (n, un) in u.iter_mut().enumerate() {
            let mut s = 0.0;
            for j in 0..5 {
                s += state[n + 64 * j] * c[2 * (n + 64 * j)];
            }
            *un = s;
        }
        for (k, x) in slot.iter_mut().enumerate() {
            let row = &q.ana[k * 64..k * 64 + 64];
            let (mut re, mut im) = (0.0, 0.0);
            for (un, w) in u.iter().zip(row) {
                re += un * w.re;
                im += un * w.im;
            }
            *x = Cx::new(re, im);
        }
    }
}

/// §4.6.18.4.2, one frame: 32 slots × 64 bands in, 2048 samples out.
fn synthesis(v: &mut [f32; 1280], x: &[[Cx; 64]; X_SLOTS], out: &mut [f32]) {
    let q = qmf();
    let c = &tab::QMF_C;
    for l in 0..SLOTS {
        v.copy_within(0..1152, 128);
        let xs = &x[l];
        for n in 0..128 {
            let row = &q.syn[n * 64..n * 64 + 64];
            let mut s = 0.0;
            for (xk, w) in xs.iter().zip(row) {
                s += xk.re * w.re - xk.im * w.im;
            }
            v[n] = s;
        }
        let o = &mut out[l * 64..l * 64 + 64];
        for (n, on) in o.iter_mut().enumerate() {
            let mut s = 0.0;
            for i in 0..5 {
                s += v[256 * i + n] * c[128 * i + n];
                s += v[256 * i + 192 + n] * c[128 * i + 64 + n];
            }
            *on = s;
        }
    }
}

/// The frequency tables a header implies (§4.6.18.3.2).
#[derive(Clone, Default)]
struct Bands {
    k0: usize,
    kx: usize,
    m: usize,
    /// `n[0]` low-resolution bands, `n[1]` high.
    n: [usize; 2],
    n_q: usize,
    n_lim: usize,
    f_master: Vec<usize>,
    f_high: Vec<usize>,
    f_low: Vec<usize>,
    f_noise: Vec<usize>,
    f_lim: Vec<usize>,
    patch_num: Vec<usize>,
    patch_start: Vec<usize>,
}

/// `num_bands` band widths rising geometrically from `start` to `stop`.
fn make_bands(start: usize, stop: usize, num_bands: usize) -> Vec<i32> {
    let base = (stop as f32 / start as f32).powf(1.0 / num_bands as f32);
    let mut prod = start as f32;
    let mut previous = start as i32;
    let mut out = Vec::with_capacity(num_bands);
    for _ in 0..num_bands - 1 {
        prod *= base;
        let present = prod.round_ties_even() as i32;
        out.push(present - previous);
        previous = present;
    }
    out.push(stop as i32 - previous);
    out
}

fn bands(sp: &Spectrum, rate: u32) -> Option<Bands> {
    // Table 4.82 (start), 4.83 (stop): indexed by the SBR (output) rate.
    const START_OFFSET: [[i8; 16]; 7] = [
        [-8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7],
        [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 9, 11, 13],
        [-5, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 9, 11, 13, 16],
        [-6, -4, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 9, 11, 13, 16],
        [-4, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 9, 11, 13, 16, 20],
        [-2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 9, 11, 13, 16, 20, 24],
        [0, 1, 2, 3, 4, 5, 6, 7, 9, 11, 13, 16, 20, 24, 28, 33],
    ];
    const RATES: [u32; 12] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000,
    ];
    const START_MIN: [u8; 12] = [7, 7, 10, 11, 12, 16, 16, 17, 24, 32, 35, 48];
    const OFFSET_INDEX: [usize; 12] = [5, 5, 4, 4, 4, 3, 2, 1, 0, 6, 6, 6];
    const STOP_MIN: [u8; 12] = [13, 15, 20, 21, 23, 32, 32, 35, 48, 64, 70, 96];
    const STOP_OFFSET: [[i8; 14]; 12] = [
        [0, 2, 4, 6, 8, 11, 14, 18, 22, 26, 31, 37, 44, 51],
        [0, 2, 4, 6, 8, 11, 14, 18, 22, 26, 31, 36, 42, 49],
        [0, 2, 4, 6, 8, 11, 14, 17, 21, 25, 29, 34, 39, 44],
        [0, 2, 4, 6, 8, 11, 14, 17, 20, 24, 28, 33, 38, 43],
        [0, 2, 4, 6, 8, 11, 14, 17, 20, 24, 28, 32, 36, 41],
        [0, 2, 4, 6, 8, 10, 12, 14, 17, 20, 23, 26, 29, 32],
        [0, 2, 4, 6, 8, 10, 12, 14, 17, 20, 23, 26, 29, 32],
        [0, 1, 3, 5, 7, 9, 11, 13, 15, 17, 20, 23, 26, 29],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 16],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, -1, -2, -3, -4, -5, -6, -6, -6, -6, -6, -6, -6, -6],
        [
            0, -3, -6, -9, -12, -15, -18, -20, -22, -24, -26, -28, -30, -32,
        ],
    ];
    // The table's row for the nearest rate at or below this one.
    let ri = RATES.iter().position(|&r| rate >= r).unwrap_or(11);
    let k0 = (i32::from(START_MIN[ri])
        + i32::from(START_OFFSET[OFFSET_INDEX[ri]][usize::from(sp.start_freq)]))
    .max(1) as usize;
    let k2 = match sp.stop_freq {
        14 => 2 * k0,
        15 => 3 * k0,
        s => (i32::from(STOP_MIN[ri]) + i32::from(STOP_OFFSET[ri][usize::from(s).min(13)])).max(0)
            as usize,
    }
    .min(64);
    if k2 <= k0 {
        return None;
    }
    let max_span = if rate <= 32_000 {
        48
    } else if rate == 44_100 {
        35
    } else {
        32
    };
    if k2 - k0 > max_span {
        return None;
    }

    // Master table.
    let mut f_master: Vec<usize>;
    if sp.freq_scale == 0 {
        let (dk, num) = if sp.alter_scale == 0 {
            (1, 2 * ((k2 - k0) / 2))
        } else {
            (2, 2 * ((k2 - k0 + 2) / 4))
        };
        if num == 0 {
            return None;
        }
        let mut vdk = vec![dk as i32; num];
        let mut diff = (k2 - k0) as i32 - (num * dk) as i32;
        if diff != 0 {
            let incr: i32 = if diff > 0 { -1 } else { 1 };
            let mut k: i32 = if diff > 0 { num as i32 - 1 } else { 0 };
            while diff != 0 {
                if !(0..num as i32).contains(&k) {
                    return None;
                }
                vdk[k as usize] -= incr;
                k += incr;
                diff += incr;
            }
        }
        f_master = vec![k0];
        for d in vdk {
            if d <= 0 {
                return None;
            }
            f_master.push(*f_master.last()? + d as usize);
        }
    } else {
        let half = [6.0f32, 5.0, 4.0][usize::from(sp.freq_scale - 1)];
        let warp = if sp.alter_scale == 1 { 1.3f32 } else { 1.0 };
        let two = k2 as f32 / k0 as f32 > 2.2449;
        let k1 = if two { 2 * k0 } else { k2 };
        let num0 = (half * (k1 as f32 / k0 as f32).log2()).round_ties_even() as usize * 2;
        if num0 == 0 || num0 > 64 {
            return None;
        }
        let mut vdk0 = make_bands(k0, k1, num0);
        vdk0.sort_unstable();
        if vdk0[0] <= 0 {
            return None;
        }
        f_master = vec![k0];
        for &d in &vdk0 {
            f_master.push(*f_master.last()? + d as usize);
        }
        if two {
            let num1 =
                (half * (k2 as f32 / k1 as f32).log2() / warp).round_ties_even() as usize * 2;
            if num1 == 0 || num1 > 64 {
                return None;
            }
            let mut vdk1 = make_bands(k1, k2, num1);
            vdk1.sort_unstable();
            if vdk1[0] < vdk0[num0 - 1] {
                let change = vdk0[num0 - 1] - vdk1[0];
                vdk1[0] += change;
                vdk1[num1 - 1] -= change;
                vdk1.sort_unstable();
            }
            if vdk1[0] <= 0 {
                return None;
            }
            for d in vdk1 {
                f_master.push(*f_master.last()? + d as usize);
            }
        }
    }
    let n_master = f_master.len() - 1;
    let xover = usize::from(sp.xover_band);
    if xover >= n_master {
        return None;
    }
    let kx = f_master[xover];
    let m = f_master[n_master].checked_sub(kx)?;
    if kx > 32 || kx + m > 64 || m == 0 {
        return None;
    }
    let n_high = n_master - xover;
    let n_low = n_high / 2 + (n_high & 1);
    let f_high: Vec<usize> = f_master[xover..].to_vec();
    let mut f_low = vec![f_high[0]];
    for i in 1..=n_low {
        f_low.push(f_high[2 * i - (n_high & 1)]);
    }
    let n_q = if sp.noise_bands == 0 {
        1
    } else {
        ((f32::from(sp.noise_bands) * (k2 as f32 / kx as f32).log2()).round_ties_even() as usize)
            .max(1)
    };
    if n_q > 5 {
        return None;
    }
    let mut f_noise = vec![f_low[0]];
    let mut t = 0usize;
    for k in 1..=n_q {
        t += (n_low - t) / (n_q + 1 - k);
        f_noise.push(f_low[t]);
    }
    let mut b = Bands {
        k0,
        kx,
        m,
        n: [n_low, n_high],
        n_q,
        n_lim: 0,
        f_master,
        f_high,
        f_low,
        f_noise,
        f_lim: Vec::new(),
        patch_num: Vec::new(),
        patch_start: Vec::new(),
    };
    patches(&mut b, rate)?;
    Some(b)
}

/// §4.6.18.6.3: which low subbands are copied where.
fn patches(b: &mut Bands, rate: u32) -> Option<()> {
    let k0 = b.k0 as isize;
    let mut msb = b.k0 as isize;
    let mut usb = b.kx as isize;
    let goal = ((2_048_000 + rate / 2) / rate) as usize;
    let n_master = b.f_master.len() - 1;
    let mut k = if goal < b.kx + b.m {
        (0..=n_master).find(|&k| b.f_master[k] >= goal)?
    } else {
        n_master
    };
    let (mut last_k, mut last_msb) = (usize::MAX, isize::MIN);
    let mut sb: isize = 0;
    loop {
        if k == last_k && msb == last_msb {
            return None;
        }
        last_k = k;
        last_msb = msb;
        let mut odd: isize = 0;
        let mut i = k as isize;
        // for (i = k; i == k || sb > k0 − 1 + msb − odd; i−−)
        while i == k as isize || sb > k0 - 1 + msb - odd {
            if i < 0 {
                return None;
            }
            sb = b.f_master[i as usize] as isize;
            odd = (sb + k0) & 1;
            i -= 1;
        }
        if b.patch_num.len() > 5 {
            return None;
        }
        let num = (sb - usb).max(0);
        let start = k0 - odd - num;
        if start < 0 {
            return None;
        }
        if num > 0 {
            b.patch_num.push(num as usize);
            b.patch_start.push(start as usize);
            usb = sb;
            msb = sb;
        } else {
            msb = b.kx as isize;
        }
        if (b.f_master[k] as isize) - sb < 3 {
            k = n_master;
        }
        if sb == (b.kx + b.m) as isize {
            break;
        }
    }
    if b.patch_num.len() > 1 && *b.patch_num.last()? < 3 {
        b.patch_num.pop();
        b.patch_start.pop();
    }
    if b.patch_num.is_empty() || b.patch_num.len() > 5 {
        return None;
    }
    Some(())
}

/// §4.6.18.3.2.3: the limiter's bands.
fn limiter(b: &mut Bands, limiter_bands: u8) {
    if limiter_bands == 0 {
        b.f_lim = vec![b.f_low[0], b.f_low[b.n[0]]];
        b.n_lim = 1;
        return;
    }
    // 2^(0.49 / bands-per-octave), bands per octave 1.2, 2, 3.
    let warped = [1.327_151_7_f32, 1.185_092_8, 1.119_871_6][usize::from(limiter_bands - 1)];
    let mut borders = vec![b.kx];
    for &n in &b.patch_num {
        borders.push(borders.last().copied().unwrap_or(0) + n);
    }
    let mut lim: Vec<usize> = b.f_low.clone();
    lim.extend_from_slice(&borders[1..borders.len() - 1]);
    lim.sort_unstable();
    let in_borders = |v: usize| borders.contains(&v);
    let mut n_lim = lim.len() - 1;
    let mut out = 0;
    let mut i = 1;
    while out < n_lim {
        if i >= lim.len() {
            break;
        }
        let (vi, vo) = (lim[i], lim[out]);
        if vi as f32 >= vo as f32 * warped {
            out += 1;
            lim[out] = vi;
            i += 1;
        } else if vi == vo || !in_borders(vi) {
            i += 1;
            n_lim -= 1;
        } else if !in_borders(vo) {
            lim[out] = vi;
            i += 1;
            n_lim -= 1;
        } else {
            out += 1;
            lim[out] = vi;
            i += 1;
        }
    }
    lim.truncate(n_lim + 1);
    b.f_lim = lim;
    b.n_lim = n_lim;
}

/// Walk a Huffman tree (leaf = `value − bias`).
fn huff(r: &mut Rd<'_>, t: &[[i8; 2]], bias: i32) -> Option<i32> {
    let mut i = 0usize;
    loop {
        let bit = usize::from(r.bit()?);
        let n = *t.get(i)?.get(bit)?;
        if n < 0 {
            return Some(i32::from(n) + bias);
        }
        i = n as usize;
    }
}

pub(crate) fn rd(r: &mut Rd<'_>, n: u32) -> Option<u32> {
    r.read(n)
}

/// SBR for one SCE/CPE (and LFE: no data ever arrives, so it is only
/// upsampled).
pub(crate) struct Sbr {
    rate: u32,
    /// SBR has a header and valid tables.
    start: bool,
    /// The tables were rebuilt this frame.
    reset: bool,
    spectrum: Option<Spectrum>,
    amp_res: bool,
    limiter_bands: u8,
    limiter_gains: u8,
    interpol_freq: bool,
    smoothing_mode: bool,
    bands: Bands,
    coupling: bool,
    /// `kx`/`m` this frame and the previous one (`[1]` current).
    kx: [usize; 2],
    m: [usize; 2],
    /// New data arrived for this frame (dequantise before applying).
    fresh: bool,
    ch: Vec<Channel>,
    pub(crate) ps: Option<Box<Ps>>,
    /// One synthesis filterbank per output channel.
    synth: Vec<Box<[f32; 1280]>>,
}

impl Sbr {
    /// `rate` is the output (SBR) rate; `channels` 1 for SCE/LFE, 2 for CPE.
    pub(crate) fn new(rate: u32, channels: usize) -> Self {
        Self {
            rate,
            start: false,
            reset: false,
            spectrum: None,
            amp_res: false,
            limiter_bands: 2,
            limiter_gains: 2,
            interpol_freq: true,
            smoothing_mode: true,
            bands: Bands::default(),
            coupling: false,
            kx: [32, 32],
            m: [0, 0],
            fresh: false,
            ch: (0..channels).map(|_| Channel::new()).collect(),
            ps: None,
            synth: Vec::new(),
        }
    }

    /// Whether a PS extension has been seen and set up.
    pub(crate) fn has_ps(&self) -> bool {
        self.ps.as_ref().is_some_and(|p| p.started())
    }

    fn turn_off(&mut self) {
        self.start = false;
        self.fresh = false;
        self.kx[1] = 32;
        self.m[1] = 0;
        self.spectrum = None;
        for c in &mut self.ch {
            c.e_a[1] = -1;
        }
    }

    /// Parse one `sbr_extension_data` occupying `bits` bits from the reader's
    /// position (the fill element's payload after its 4-bit type), and leave
    /// the reader exactly past it, whatever happens inside.
    pub(crate) fn read(&mut self, r: &mut Rd<'_>, bits: usize, crc: bool, allow_ps: bool) {
        let begin = r.position();
        let total = begin + r.remaining();
        let end = (begin + bits).min(total);
        let ok = self.read_inner(r, end, crc, allow_ps).is_some() && r.position() <= end;
        if !ok {
            self.turn_off();
        }
        r.seek(end);
    }

    fn read_inner(&mut self, r: &mut Rd<'_>, end: usize, crc: bool, allow_ps: bool) -> Option<()> {
        self.kx[0] = self.kx[1];
        self.m[0] = self.m[1];
        if crc {
            rd(r, 10)?;
        }
        self.reset = false;
        if rd(r, 1)? == 1 {
            self.header(r)?;
        }
        if !self.start {
            return Some(());
        }
        // A reset (or the first header) rebuilt the tables: kx/m move now.
        self.kx[1] = self.bands.kx;
        self.m[1] = self.bands.m;
        match self.ch.len() {
            1 => self.single(r, end, allow_ps)?,
            _ => self.pair(r, end)?,
        }
        self.fresh = true;
        Some(())
    }

    fn header(&mut self, r: &mut Rd<'_>) -> Option<()> {
        let amp_res = rd(r, 1)? == 1;
        let start_freq = rd(r, 4)? as u8;
        let stop_freq = rd(r, 4)? as u8;
        let xover_band = rd(r, 3)? as u8;
        rd(r, 2)?;
        let extra1 = rd(r, 1)? == 1;
        let extra2 = rd(r, 1)? == 1;
        let (mut freq_scale, mut alter_scale, mut noise_bands) = (2, 1, 2);
        if extra1 {
            freq_scale = rd(r, 2)? as u8;
            alter_scale = rd(r, 1)? as u8;
            noise_bands = rd(r, 2)? as u8;
        }
        let (mut lb, mut lg, mut ifq, mut sm) = (2, 2, true, true);
        if extra2 {
            lb = rd(r, 2)? as u8;
            lg = rd(r, 2)? as u8;
            ifq = rd(r, 1)? == 1;
            sm = rd(r, 1)? == 1;
        }
        self.amp_res = amp_res;
        let sp = Spectrum {
            start_freq,
            stop_freq,
            xover_band,
            freq_scale,
            alter_scale,
            noise_bands,
        };
        let limiter_changed = lb != self.limiter_bands;
        self.limiter_bands = lb;
        self.limiter_gains = lg;
        self.interpol_freq = ifq;
        self.smoothing_mode = sm;
        if self.spectrum != Some(sp) || !self.start {
            let mut b = bands(&sp, self.rate)?;
            limiter(&mut b, lb);
            self.bands = b;
            self.spectrum = Some(sp);
            self.reset = true;
            self.start = true;
        } else if limiter_changed {
            limiter(&mut self.bands, lb);
        }
        Some(())
    }

    fn single(&mut self, r: &mut Rd<'_>, end: usize, allow_ps: bool) -> Option<()> {
        if rd(r, 1)? == 1 {
            rd(r, 4)?;
        }
        let b = self.bands.clone();
        let amp = self.amp_res;
        let c = &mut self.ch[0];
        grid(r, c, amp)?;
        dtdf(r, c)?;
        invf(r, c, b.n_q)?;
        envelope(r, c, &b, false)?;
        noise(r, c, &b, false)?;
        harmonics(r, c, &b)?;
        self.extended(r, end, allow_ps)?;
        Some(())
    }

    fn pair(&mut self, r: &mut Rd<'_>, end: usize) -> Option<()> {
        if rd(r, 1)? == 1 {
            rd(r, 8)?;
        }
        self.coupling = rd(r, 1)? == 1;
        let b = self.bands.clone();
        let amp = self.amp_res;
        let (c0, c1) = self.ch.split_at_mut(1);
        let (c0, c1) = (&mut c0[0], &mut c1[0]);
        if self.coupling {
            grid(r, c0, amp)?;
            copy_grid(c0, c1);
            dtdf(r, c0)?;
            dtdf(r, c1)?;
            invf(r, c0, b.n_q)?;
            c1.invf_mode[1] = c1.invf_mode[0];
            c1.invf_mode[0] = c0.invf_mode[0];
            envelope(r, c0, &b, false)?;
            noise(r, c0, &b, false)?;
            envelope(r, c1, &b, true)?;
            noise(r, c1, &b, true)?;
        } else {
            grid(r, c0, amp)?;
            grid(r, c1, amp)?;
            dtdf(r, c0)?;
            dtdf(r, c1)?;
            invf(r, c0, b.n_q)?;
            invf(r, c1, b.n_q)?;
            envelope(r, c0, &b, false)?;
            envelope(r, c1, &b, false)?;
            noise(r, c0, &b, false)?;
            noise(r, c1, &b, false)?;
        }
        harmonics(r, c0, &b)?;
        harmonics(r, c1, &b)?;
        self.extended(r, end, false)?;
        Some(())
    }

    fn extended(&mut self, r: &mut Rd<'_>, end: usize, allow_ps: bool) -> Option<()> {
        if rd(r, 1)? == 0 {
            return Some(());
        }
        let mut cnt = rd(r, 4)? as usize;
        if cnt == 15 {
            cnt += rd(r, 8)? as usize;
        }
        let mut left = cnt * 8;
        if r.position() + left > end {
            return None;
        }
        while left > 7 {
            let id = rd(r, 2)?;
            left -= 2;
            let at = r.position();
            if id == 2 && allow_ps {
                let ps = self.ps.get_or_insert_with(|| Box::new(Ps::new()));
                ps.read(r, left);
            }
            let used = r.position() - at;
            if used > left {
                return None;
            }
            if id != 2 || !allow_ps {
                // Unknown extension: the rest of the count is its payload.
                r.skip(left)?;
                left = 0;
            } else {
                left -= used;
            }
        }
        r.skip(left)?;
        Some(())
    }

    /// Run one frame. `core` holds 1024 time samples per channel of this
    /// element (16-bit scale); `out` receives 2048 per output channel — one
    /// for an SCE (two when PS is on), two for a CPE.
    pub(crate) fn apply(&mut self, core: &[&[f32]], out: &mut [Vec<f32>]) {
        if self.fresh {
            self.dequant();
        } else {
            // No payload this frame: the band split stays where it is.
            self.kx[0] = self.kx[1];
            self.m[0] = self.m[1];
        }
        self.fresh = false;
        let kx = self.kx;
        let m = self.m;
        let start = self.start;
        let ps_on = self.ch.len() == 1 && out.len() == 2;
        let mut xs: Vec<Box<[[Cx; 64]; X_SLOTS]>> = Vec::with_capacity(2);
        for (ci, input) in core.iter().enumerate().take(self.ch.len()) {
            let bands = &self.bands;
            let c = &mut self.ch[ci];
            let cur = c.pos;
            analysis(&mut c.analysis, input, &mut c.w[cur]);
            let mut x_low = Box::new([[Cx::ZERO; 40]; 32]);
            for (k, row) in x_low.iter_mut().enumerate() {
                if k < kx[1] {
                    for i in T_HFGEN..SLOTS + T_HFGEN {
                        row[i] = c.w[cur][i - T_HFGEN][k];
                    }
                }
                if k < kx[0] {
                    for i in 0..T_HFGEN {
                        row[i] = c.w[1 - cur][i + SLOTS - T_HFGEN][k];
                    }
                }
            }
            c.pos ^= 1;
            let ypos = c.pos;
            if start && c.num_env > 0 {
                let mut x_high = Box::new([[Cx::ZERO; 40]; 64]);
                let (alpha0, alpha1) = inverse_filter(&x_low, bands.k0);
                chirp(c, bands.n_q);
                hf_gen(&mut x_high, &x_low, &alpha0, &alpha1, c, bands, kx[1]);
                let mapped = mapping(c, bands, kx[1]);
                let e_curr = estimate(&x_high, c, bands, kx[1], m[1], self.interpol_freq);
                let g = gains(c, bands, kx[1], &mapped, &e_curr, self.limiter_gains);
                // Borrowed out for the call (the channel's other state is
                // mutated alongside it), put back after.
                let mut y = std::mem::replace(&mut c.y[ypos], Box::new([[Cx::ZERO; 64]; X_SLOTS]));
                assemble(
                    &mut y,
                    &x_high,
                    c,
                    kx[1],
                    m[1],
                    &g,
                    self.reset,
                    self.smoothing_mode,
                );
                c.y[ypos] = y;
            }
            xs.push(x_gen(c, &x_low, kx, m, ypos));
        }
        if ps_on {
            let mut right = xs[0].clone();
            match self.ps.as_mut() {
                Some(ps) if ps.started() => {
                    ps.apply(&mut xs[0], &mut right, kx[1] + m[1]);
                }
                _ => {}
            }
            xs.push(right);
        }
        while self.synth.len() < xs.len() {
            self.synth.push(Box::new([0.0; 1280]));
        }
        for ((o, x), v) in out.iter_mut().zip(&xs).zip(self.synth.iter_mut()) {
            o.clear();
            o.resize(2048, 0.0);
            synthesis(v, x, o);
        }
        self.reset = false;
    }

    /// §4.6.18.3.5.
    fn dequant(&mut self) {
        let b = &self.bands;
        if self.ch.len() == 2 && self.coupling {
            let alpha = if self.ch[0].amp_res { 1.0f32 } else { 0.5 };
            let pan = if self.ch[0].amp_res { 12.0f32 } else { 24.0 };
            let (c0, c1) = self.ch.split_at_mut(1);
            let (c0, c1) = (&mut c0[0], &mut c1[0]);
            for e in 1..=c0.num_env {
                for k in 0..b.n[usize::from(c0.freq_res[e])] {
                    let t1 = (c0.env_q[e][k] as f32 * alpha + 7.0).exp2();
                    let t2 = ((pan - c1.env_q[e][k] as f32) * alpha).exp2();
                    c0.env[e][k] = t1 / (1.0 + t2);
                    c1.env[e][k] = c0.env[e][k] * t2;
                }
            }
            for e in 1..=c0.num_noise {
                for k in 0..b.n_q {
                    let t1 = (6.0 - c0.noise_q[e][k] as f32 + 1.0).exp2();
                    let t2 = (12.0 - c1.noise_q[e][k] as f32).exp2();
                    c0.noise[e][k] = t1 / (1.0 + t2);
                    c1.noise[e][k] = c0.noise[e][k] * t2;
                }
            }
        } else {
            for c in &mut self.ch {
                let alpha = if c.amp_res { 1.0f32 } else { 0.5 };
                for e in 1..=c.num_env {
                    for k in 0..b.n[usize::from(c.freq_res[e])] {
                        c.env[e][k] = (alpha * c.env_q[e][k] as f32 + 6.0).exp2();
                    }
                }
                for e in 1..=c.num_noise {
                    for k in 0..b.n_q {
                        c.noise[e][k] = (6.0 - c.noise_q[e][k] as f32).exp2();
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- syntax

const FIXFIX: u8 = 0;
const FIXVAR: u8 = 1;
const VARFIX: u8 = 2;

fn grid(r: &mut Rd<'_>, c: &mut Channel, amp_res: bool) -> Option<()> {
    const CEIL_LOG2: [u32; 6] = [0, 1, 2, 2, 3, 3];
    let num_time = SLOTS as u32 / 2; // 16 envelope time units
    c.t_env_num_env_old = c.t_env[c.num_env];
    c.amp_res = amp_res;
    let class = rd(r, 2)? as u8;
    let mut t_env = [0u8; MAX_ENV + 1];
    let mut freq_res = [0u8; MAX_ENV + 1];
    let num_env;
    let mut pointer = 0usize;
    match class {
        FIXFIX => {
            num_env = 1usize << rd(r, 2)?;
            if num_env > 4 {
                return None;
            }
            if num_env == 1 {
                c.amp_res = false;
            }
            let res = rd(r, 1)? as u8;
            for f in freq_res.iter_mut().skip(1).take(num_env) {
                *f = res;
            }
            t_env[num_env] = num_time as u8;
            let step = (num_time + (num_env as u32 >> 1)) / num_env as u32;
            for i in 0..num_env - 1 {
                t_env[i + 1] = t_env[i] + step as u8;
            }
        }
        FIXVAR => {
            let trail = num_time + rd(r, 2)?;
            let rel = rd(r, 2)? as usize;
            num_env = rel + 1;
            t_env[num_env] = trail as u8;
            for i in 0..rel {
                let d = 2 * rd(r, 2)? + 2;
                let v = u32::from(t_env[num_env - i]).checked_sub(d)?;
                t_env[num_env - 1 - i] = v as u8;
            }
            pointer = rd(r, CEIL_LOG2[num_env])? as usize;
            for i in 0..num_env {
                freq_res[num_env - i] = rd(r, 1)? as u8;
            }
        }
        VARFIX => {
            let lead = rd(r, 2)?;
            let rel = rd(r, 2)? as usize;
            num_env = rel + 1;
            t_env[0] = lead as u8;
            t_env[num_env] = num_time as u8;
            for i in 0..rel {
                let d = 2 * rd(r, 2)? + 2;
                t_env[i + 1] = (u32::from(t_env[i]) + d).min(255) as u8;
            }
            pointer = rd(r, CEIL_LOG2[num_env])? as usize;
            for f in freq_res.iter_mut().skip(1).take(num_env) {
                *f = rd(r, 1)? as u8;
            }
        }
        _ => {
            let lead = rd(r, 2)?;
            let trail = num_time + rd(r, 2)?;
            let rel_lead = rd(r, 2)? as usize;
            let rel_trail = rd(r, 2)? as usize;
            num_env = rel_lead + rel_trail + 1;
            if num_env > 5 {
                return None;
            }
            t_env[0] = lead as u8;
            t_env[num_env] = trail as u8;
            for i in 0..rel_lead {
                let d = 2 * rd(r, 2)? + 2;
                t_env[i + 1] = (u32::from(t_env[i]) + d).min(255) as u8;
            }
            for i in 0..rel_trail {
                let d = 2 * rd(r, 2)? + 2;
                let v = u32::from(t_env[num_env - i]).checked_sub(d)?;
                t_env[num_env - 1 - i] = v as u8;
            }
            pointer = rd(r, CEIL_LOG2[num_env])? as usize;
            for f in freq_res.iter_mut().skip(1).take(num_env) {
                *f = rd(r, 1)? as u8;
            }
        }
    }
    if pointer > num_env + 1 {
        return None;
    }
    for i in 0..num_env {
        if t_env[i] >= t_env[i + 1] {
            return None;
        }
    }
    if usize::from(t_env[num_env]) > SLOTS / 2 + 3 {
        return None;
    }
    let num_noise = if num_env > 1 { 2 } else { 1 };
    let mut t_q = [0u8; 3];
    t_q[0] = t_env[0];
    t_q[num_noise] = t_env[num_env];
    if num_noise > 1 {
        let idx = if class == FIXFIX {
            num_env >> 1
        } else if class & 1 == 1 {
            num_env.checked_sub(pointer.saturating_sub(1).max(1))?
        } else if pointer == 0 {
            1
        } else if pointer == 1 {
            num_env - 1
        } else {
            pointer - 1
        };
        t_q[1] = *t_env.get(idx)?;
    }
    // The transient envelope (the one the pointer marks), and whether the
    // last frame's was its final envelope.
    c.e_a[0] = -i32::from(c.e_a[1] != c.num_env as i32);
    c.e_a[1] = -1;
    if class & 1 == 1 && pointer > 0 {
        c.e_a[1] = (num_env + 1 - pointer) as i32;
    } else if class == VARFIX && pointer > 1 {
        c.e_a[1] = pointer as i32 - 1;
    }
    freq_res[0] = c.freq_res[c.num_env];
    c.frame_class = class;
    c.num_env = num_env;
    c.num_noise = num_noise;
    c.t_env = t_env;
    c.t_q = t_q;
    c.freq_res = freq_res;
    c.pointer = pointer;
    Some(())
}

fn copy_grid(src: &Channel, dst: &mut Channel) {
    dst.t_env_num_env_old = dst.t_env[dst.num_env];
    dst.e_a[0] = -i32::from(dst.e_a[1] != dst.num_env as i32);
    dst.e_a[1] = src.e_a[1];
    let prev_res = dst.freq_res[dst.num_env];
    dst.frame_class = src.frame_class;
    dst.amp_res = src.amp_res;
    dst.num_env = src.num_env;
    dst.num_noise = src.num_noise;
    dst.t_env = src.t_env;
    dst.t_q = src.t_q;
    dst.freq_res = src.freq_res;
    dst.freq_res[0] = prev_res;
    dst.pointer = src.pointer;
}

fn dtdf(r: &mut Rd<'_>, c: &mut Channel) -> Option<()> {
    for i in 0..c.num_env {
        c.df_env[i] = rd(r, 1)? == 1;
    }
    for i in 0..c.num_noise {
        c.df_noise[i] = rd(r, 1)? == 1;
    }
    Some(())
}

fn invf(r: &mut Rd<'_>, c: &mut Channel, n_q: usize) -> Option<()> {
    c.invf_mode[1] = c.invf_mode[0];
    for i in 0..n_q {
        c.invf_mode[0][i] = rd(r, 2)? as u8;
    }
    Some(())
}

fn envelope(r: &mut Rd<'_>, c: &mut Channel, b: &Bands, balance: bool) -> Option<()> {
    let delta = if balance { 2 } else { 1 };
    let (bits, t_huff, f_huff): (u32, &[[i8; 2]], &[[i8; 2]]) = match (balance, c.amp_res) {
        (true, true) => (5, &tab::SBR_T_ENV_BAL_30, &tab::SBR_F_ENV_BAL_30),
        (true, false) => (6, &tab::SBR_T_ENV_BAL_15, &tab::SBR_F_ENV_BAL_15),
        (false, true) => (6, &tab::SBR_T_ENV_30, &tab::SBR_F_ENV_30),
        (false, false) => (7, &tab::SBR_T_ENV_15, &tab::SBR_F_ENV_15),
    };
    let odd = b.n[1] & 1;
    for i in 0..c.num_env {
        let res = usize::from(c.freq_res[i + 1]);
        let n = b.n[res];
        if c.df_env[i] {
            let prev_res = usize::from(c.freq_res[i]);
            for j in 0..n {
                let k = if res == prev_res {
                    j
                } else if res == 1 {
                    (j + odd) >> 1
                } else if j == 0 {
                    0
                } else {
                    2 * j - odd
                };
                let v = c.env_q[i][k.min(MAX_BANDS - 1)] + delta * huff(r, t_huff, 64)?;
                c.env_q[i + 1][j] = v;
            }
        } else {
            c.env_q[i + 1][0] = delta * rd(r, bits)? as i32;
            for j in 1..n {
                c.env_q[i + 1][j] = c.env_q[i + 1][j - 1] + delta * huff(r, f_huff, 64)?;
            }
        }
        for j in 0..n {
            // Hostile values would overflow exp2 into inf; the standard's
            // range is a few dozen steps.
            if !(-40..=127).contains(&c.env_q[i + 1][j]) {
                return None;
            }
        }
    }
    c.env_q[0] = c.env_q[c.num_env];
    Some(())
}

fn noise(r: &mut Rd<'_>, c: &mut Channel, b: &Bands, balance: bool) -> Option<()> {
    let delta = if balance { 2 } else { 1 };
    let (t_huff, f_huff): (&[[i8; 2]], &[[i8; 2]]) = if balance {
        (&tab::SBR_T_NOISE_BAL_30, &tab::SBR_F_ENV_BAL_30)
    } else {
        (&tab::SBR_T_NOISE_30, &tab::SBR_F_ENV_30)
    };
    for i in 0..c.num_noise {
        if c.df_noise[i] {
            for j in 0..b.n_q {
                c.noise_q[i + 1][j] = c.noise_q[i][j] + delta * huff(r, t_huff, 64)?;
            }
        } else {
            c.noise_q[i + 1][0] = delta * rd(r, 5)? as i32;
            for j in 1..b.n_q {
                c.noise_q[i + 1][j] = c.noise_q[i + 1][j - 1] + delta * huff(r, f_huff, 64)?;
            }
        }
        for j in 0..b.n_q {
            if !(-40..=60).contains(&c.noise_q[i + 1][j]) {
                return None;
            }
        }
    }
    c.noise_q[0] = c.noise_q[c.num_noise];
    Some(())
}

fn harmonics(r: &mut Rd<'_>, c: &mut Channel, b: &Bands) -> Option<()> {
    c.add_harmonic_flag = rd(r, 1)? == 1;
    if c.add_harmonic_flag {
        for i in 0..b.n[1] {
            c.add_harmonic[i] = rd(r, 1)? == 1;
        }
    }
    Some(())
}

// ------------------------------------------------------------- processing

/// §4.6.18.6.2: the low band's per-subband LPC coefficients.
fn inverse_filter(x_low: &[[Cx; 40]; 32], k0: usize) -> (Vec<Cx>, Vec<Cx>) {
    let mut a0 = vec![Cx::ZERO; 32];
    let mut a1 = vec![Cx::ZERO; 32];
    for k in 0..k0.min(32) {
        let x = &x_low[k];
        // φ(i,j) = Σ_{n=0}^{37} x[n+2−i]·conj(x[n+2−j]), accumulated in f64
        // (the sums are long and the determinant subtracts near-equals).
        let phi = |i: usize, j: usize| {
            let (mut re, mut im) = (0f64, 0f64);
            for n in 0..38 {
                let p = x[n + 2 - i].conj_mul(x[n + 2 - j]);
                re += f64::from(p.re);
                im += f64::from(p.im);
            }
            (re, im)
        };
        let p01 = phi(0, 1);
        let p02 = phi(0, 2);
        let p11 = phi(1, 1).0;
        let p12 = phi(1, 2);
        let p22 = phi(2, 2).0;
        let d = p22 * p11 - (p12.0 * p12.0 + p12.1 * p12.1) / 1.000_001;
        let (mut r1, mut i1) = (0.0, 0.0);
        if d != 0.0 {
            r1 = (p01.0 * p12.0 - p01.1 * p12.1 - p02.0 * p11) / d;
            i1 = (p01.0 * p12.1 + p01.1 * p12.0 - p02.1 * p11) / d;
        }
        let (mut r0, mut i0) = (0.0, 0.0);
        if p11 != 0.0 {
            r0 = -(p01.0 + r1 * p12.0 + i1 * p12.1) / p11;
            i0 = -(p01.1 + i1 * p12.0 - r1 * p12.1) / p11;
        }
        if r1 * r1 + i1 * i1 >= 16.0 || r0 * r0 + i0 * i0 >= 16.0 {
            continue;
        }
        a0[k] = Cx::new(r0 as f32, i0 as f32);
        a1[k] = Cx::new(r1 as f32, i1 as f32);
    }
    (a0, a1)
}

/// The chirp (bandwidth) factors per noise band.
fn chirp(c: &mut Channel, n_q: usize) {
    const BW: [f32; 4] = [0.0, 0.75, 0.9, 0.98];
    for i in 0..n_q {
        let (cur, prev) = (c.invf_mode[0][i], c.invf_mode[1][i]);
        let mut bw = if cur + prev == 1 {
            0.6
        } else {
            BW[usize::from(cur)]
        };
        bw = if bw < c.bw[i] {
            0.75 * bw + 0.25 * c.bw[i]
        } else {
            0.906_25 * bw + 0.093_75 * c.bw[i]
        };
        c.bw[i] = if bw < 0.015_625 { 0.0 } else { bw };
    }
}

fn hf_gen(
    x_high: &mut [[Cx; 40]; 64],
    x_low: &[[Cx; 40]; 32],
    a0: &[Cx],
    a1: &[Cx],
    c: &Channel,
    b: &Bands,
    kx: usize,
) {
    let mut k = kx;
    let start = 2 * usize::from(c.t_env[0]);
    let end = (2 * usize::from(c.t_env[c.num_env])).min(38);
    for (&num, &pstart) in b.patch_num.iter().zip(&b.patch_start) {
        for x in 0..num {
            if k >= 64 {
                return;
            }
            let p = (pstart + x).min(31);
            // The noise band this subband falls in.
            let gi = b.f_noise[..=b.n_q]
                .iter()
                .filter(|&&f| k >= f)
                .count()
                .saturating_sub(1)
                .min(4);
            let bw = c.bw[gi];
            let alpha1 = a1[p].scale(bw * bw);
            let alpha0 = a0[p].scale(bw);
            let lo = &x_low[p];
            for i in start..end {
                let o = i + T_HFADJ;
                let v = lo[o].add(lo[o - 2].mul(alpha1)).add(lo[o - 1].mul(alpha0));
                x_high[k][o] = v;
            }
            k += 1;
        }
    }
}

/// Per-envelope maps from bands to QMF subbands (§4.6.18.7.2).
struct Mapped {
    e_orig: [[f32; MAX_BANDS]; MAX_ENV],
    q: [[f32; MAX_BANDS]; MAX_ENV],
    s: [[bool; MAX_BANDS]; MAX_ENV],
}

fn mapping(c: &mut Channel, b: &Bands, kx: usize) -> Mapped {
    let mut m = Mapped {
        e_orig: [[0.0; MAX_BANDS]; MAX_ENV],
        q: [[0.0; MAX_BANDS]; MAX_ENV],
        s: [[false; MAX_BANDS]; MAX_ENV],
    };
    for row in c.s_indexmapped.iter_mut().skip(1) {
        *row = [false; MAX_BANDS];
    }
    for e in 0..c.num_env {
        let res = usize::from(c.freq_res[e + 1]);
        let table = if res == 1 { &b.f_high } else { &b.f_low };
        for i in 0..b.n[res] {
            for sb in table[i]..table[i + 1] {
                m.e_orig[e][sb - kx] = c.env[e + 1][i];
            }
        }
        let k = usize::from(c.num_noise > 1 && c.t_env[e] >= c.t_q[1]);
        for i in 0..b.n_q {
            for sb in b.f_noise[i]..b.f_noise[i + 1] {
                m.q[e][sb - kx] = c.noise[k + 1][i];
            }
        }
        if c.add_harmonic_flag {
            for i in 0..b.n[1] {
                let mid = (b.f_high[i] + b.f_high[i + 1]) >> 1;
                let prev = c.s_indexmapped[0][mid - kx];
                c.s_indexmapped[e + 1][mid - kx] =
                    c.add_harmonic[i] && (e as i32 >= c.e_a[1] || prev);
            }
        }
        for i in 0..b.n[res] {
            let present = (table[i]..table[i + 1]).any(|sb| c.s_indexmapped[e + 1][sb - kx]);
            for sb in table[i]..table[i + 1] {
                m.s[e][sb - kx] = present;
            }
        }
    }
    c.s_indexmapped[0] = c.s_indexmapped[c.num_env];
    m
}

fn estimate(
    x_high: &[[Cx; 40]; 64],
    c: &Channel,
    b: &Bands,
    kx: usize,
    mm: usize,
    interpol_freq: bool,
) -> [[f32; MAX_BANDS]; MAX_ENV] {
    let mut e_curr = [[0f32; MAX_BANDS]; MAX_ENV];
    for e in 0..c.num_env {
        let ilb = usize::from(c.t_env[e]) * 2 + T_HFADJ;
        let iub = (usize::from(c.t_env[e + 1]) * 2 + T_HFADJ).min(40);
        let sum_sq = |k: usize| -> f32 { x_high[k][ilb..iub].iter().map(|v| v.norm2()).sum() };
        if interpol_freq {
            let recip = 0.5 / f32::from(c.t_env[e + 1] - c.t_env[e]);
            for m in 0..mm {
                e_curr[e][m] = sum_sq(m + kx) * recip;
            }
        } else {
            let env_size = 2 * usize::from(c.t_env[e + 1] - c.t_env[e]);
            let res = usize::from(c.freq_res[e + 1]);
            let table = if res == 1 { &b.f_high } else { &b.f_low };
            for p in 0..b.n[res] {
                let den = (env_size * (table[p + 1] - table[p])) as f32;
                let mut s = 0.0;
                for k in table[p]..table[p + 1] {
                    s += sum_sq(k);
                }
                s /= den;
                for k in table[p]..table[p + 1] {
                    e_curr[e][k - kx] = s;
                }
            }
        }
    }
    e_curr
}

struct Gains {
    gain: [[f32; MAX_BANDS]; MAX_ENV],
    q_m: [[f32; MAX_BANDS]; MAX_ENV],
    s_m: [[f32; MAX_BANDS]; MAX_ENV],
}

fn gains(
    c: &Channel,
    b: &Bands,
    kx: usize,
    mp: &Mapped,
    e_curr: &[[f32; MAX_BANDS]; MAX_ENV],
    limiter_gains: u8,
) -> Gains {
    const LIMGAIN: [f32; 4] = [0.707_95, 1.0, 1.412_54, 1e10];
    let mut g = Gains {
        gain: [[0.0; MAX_BANDS]; MAX_ENV],
        q_m: [[0.0; MAX_BANDS]; MAX_ENV],
        s_m: [[0.0; MAX_BANDS]; MAX_ENV],
    };
    for e in 0..c.num_env {
        let delta = !(e as i32 == c.e_a[1] || e as i32 == c.e_a[0]);
        for k in 0..b.n_lim {
            let lo = b.f_lim[k] - kx;
            let hi = b.f_lim[k + 1] - kx;
            for m in lo..hi {
                let orig = mp.e_orig[e][m];
                let q = mp.q[e][m];
                let temp = orig / (1.0 + q);
                g.q_m[e][m] = (temp * q).sqrt();
                g.s_m[e][m] = (temp * f32::from(u8::from(c.s_indexmapped[e + 1][m]))).sqrt();
                g.gain[e][m] = if mp.s[e][m] {
                    (orig * q / ((1.0 + e_curr[e][m]) * (1.0 + q))).sqrt()
                } else {
                    (orig / ((1.0 + e_curr[e][m]) * (1.0 + q * f32::from(u8::from(delta))))).sqrt()
                };
                g.gain[e][m] += f32::MIN_POSITIVE;
            }
            let (mut s0, mut s1) = (0.0f32, 0.0f32);
            for m in lo..hi {
                s0 += mp.e_orig[e][m];
                s1 += e_curr[e][m];
            }
            let gain_max = (LIMGAIN[usize::from(limiter_gains)]
                * ((f32::EPSILON + s0) / (f32::EPSILON + s1)).sqrt())
            .min(100_000.0);
            for m in lo..hi {
                let q_m_max = g.q_m[e][m] * gain_max / g.gain[e][m];
                g.q_m[e][m] = g.q_m[e][m].min(q_m_max);
                g.gain[e][m] = g.gain[e][m].min(gain_max);
            }
            let (mut s0, mut s1) = (0.0f32, 0.0f32);
            for m in lo..hi {
                s0 += mp.e_orig[e][m];
                s1 += e_curr[e][m] * g.gain[e][m] * g.gain[e][m]
                    + g.s_m[e][m] * g.s_m[e][m]
                    + if delta && g.s_m[e][m] == 0.0 {
                        g.q_m[e][m] * g.q_m[e][m]
                    } else {
                        0.0
                    };
            }
            let boost = ((f32::EPSILON + s0) / (f32::EPSILON + s1))
                .sqrt()
                .min(1.584_893_2);
            for m in lo..hi {
                g.gain[e][m] *= boost;
                g.q_m[e][m] *= boost;
                g.s_m[e][m] *= boost;
            }
        }
    }
    g
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    y: &mut [[Cx; 64]; X_SLOTS],
    x_high: &[[Cx; 40]; 64],
    c: &mut Channel,
    kx: usize,
    m_max: usize,
    g: &Gains,
    reset: bool,
    smoothing_mode: bool,
) {
    const H_SMOOTH: [f32; 5] = [
        0.333_333_33,
        0.301_502_83,
        0.218_169_5,
        0.115_163_83,
        0.031_830_5,
    ];
    let h_sl = if smoothing_mode { 0 } else { 4 };
    let t0 = 2 * usize::from(c.t_env[0]);
    if reset {
        for i in 0..h_sl {
            c.g_temp[i + t0] = g.gain[0];
            c.q_temp[i + t0] = g.q_m[0];
        }
    } else if h_sl > 0 {
        let old = 2 * usize::from(c.t_env_num_env_old);
        for i in 0..4 {
            if i + old < 42 && i + t0 < 42 {
                c.g_temp[i + t0] = c.g_temp[i + old];
                c.q_temp[i + t0] = c.q_temp[i + old];
            }
        }
    }
    for e in 0..c.num_env {
        for i in 2 * usize::from(c.t_env[e])..2 * usize::from(c.t_env[e + 1]) {
            if h_sl + i < 42 {
                c.g_temp[h_sl + i] = g.gain[e];
                c.q_temp[h_sl + i] = g.q_m[e];
            }
        }
    }
    let mut indexnoise = c.f_indexnoise;
    let mut indexsine = c.f_indexsine;
    for e in 0..c.num_env {
        let transient = e as i32 == c.e_a[0] || e as i32 == c.e_a[1];
        for i in 2 * usize::from(c.t_env[e])..2 * usize::from(c.t_env[e + 1]) {
            if i >= X_SLOTS || i + h_sl >= 42 {
                break;
            }
            let mut g_filt = [0f32; MAX_BANDS];
            let mut q_filt = [0f32; MAX_BANDS];
            if h_sl > 0 && !transient {
                for m in 0..m_max {
                    let idx = i + h_sl;
                    for (j, h) in H_SMOOTH.iter().enumerate() {
                        g_filt[m] += c.g_temp[idx - j][m] * h;
                        q_filt[m] += c.q_temp[idx - j][m] * h;
                    }
                }
            } else {
                g_filt = c.g_temp[i + h_sl];
                q_filt = c.q_temp[i];
            }
            let row = &mut y[i];
            for m in 0..m_max {
                row[kx + m] = x_high[kx + m][i + T_HFADJ].scale(g_filt[m]);
            }
            if transient {
                // Sinusoids only, no noise, in the sine's quarter-phase.
                let idx = indexsine & 1;
                let a: f32 = if (indexsine + (kx & 1)) & 2 == 0 {
                    1.0
                } else {
                    -1.0
                };
                let bsign = if idx == 1 { -a } else { a };
                for m in 0..m_max {
                    let s = g.s_m[e][m] * if m & 1 == 0 { a } else { bsign };
                    if idx == 0 {
                        row[kx + m].re += s;
                    } else {
                        row[kx + m].im += s;
                    }
                }
            } else {
                let (sign0, mut sign1) = match indexsine {
                    0 => (1.0f32, 0.0f32),
                    1 => (0.0, 1.0 - 2.0 * (kx & 1) as f32),
                    2 => (-1.0, 0.0),
                    _ => (0.0, -(1.0 - 2.0 * (kx & 1) as f32)),
                };
                let mut noise = indexnoise;
                for m in 0..m_max {
                    noise = (noise + 1) & 0x1ff;
                    let s = g.s_m[e][m];
                    if s != 0.0 {
                        row[kx + m].re += s * sign0;
                        row[kx + m].im += s * sign1;
                    } else {
                        let v = tab::NOISE[noise];
                        row[kx + m].re += q_filt[m] * v[0];
                        row[kx + m].im += q_filt[m] * v[1];
                    }
                    sign1 = -sign1;
                }
            }
            indexnoise = (indexnoise + m_max) & 0x1ff;
            indexsine = (indexsine + 1) & 3;
        }
    }
    c.f_indexnoise = indexnoise;
    c.f_indexsine = indexsine;
}

/// The synthesis input: low band from `x_low`, high band from this frame's
/// and the previous frame's adjusted `y`.
fn x_gen(
    c: &Channel,
    x_low: &[[Cx; 40]; 32],
    kx: [usize; 2],
    m: [usize; 2],
    ypos: usize,
) -> Box<[[Cx; 64]; X_SLOTS]> {
    let mut x = Box::new([[Cx::ZERO; 64]; X_SLOTS]);
    let y_prev = &c.y[1 - ypos];
    let y_cur = &c.y[ypos];
    let i_temp = (2 * usize::from(c.t_env_num_env_old))
        .saturating_sub(SLOTS)
        .min(X_SLOTS);
    for i in 0..i_temp {
        for k in 0..kx[0].min(32) {
            x[i][k] = x_low[k][i + T_HFADJ];
        }
        for k in kx[0]..(kx[0] + m[0]).min(64) {
            if i + SLOTS < X_SLOTS {
                x[i][k] = y_prev[i + SLOTS][k];
            }
        }
    }
    for i in i_temp..X_SLOTS {
        for k in 0..kx[1].min(32) {
            x[i][k] = x_low[k][i + T_HFADJ];
        }
    }
    for i in i_temp..SLOTS {
        for k in kx[1]..(kx[1] + m[1]).min(64) {
            x[i][k] = y_cur[i][k];
        }
    }
    x
}
