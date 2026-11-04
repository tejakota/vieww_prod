//! AAC-LC decoding (ISO/IEC 14496-3, General Audio, Low Complexity).
//!
//! Containers: ADTS frames ([`decode_adts`]) and MP4/M4A
//! ([`decode_mp4`], through [`mp4`](super::mp4), honouring the edit list's
//! priming and duration so the output is sample-exact).
//!
//! Syntax: single-channel, channel-pair (with common window and M/S mask),
//! LFE, data-stream, fill and program-config elements; section data,
//! scalefactors (with intensity positions and PNS energies), pulse data,
//! TNS, and all eleven spectral codebooks with escape sequences.
//! Tools: requantisation (`|q|^{4/3}·2^{(sf−100)/4}`), M/S stereo,
//! intensity stereo, perceptual noise substitution, TNS synthesis
//! filtering, short-window grouping, and the filterbank — 2048- and
//! 256-point IMDCTs with sine and Kaiser–Bessel-derived windows and the
//! four window sequences. Channel layouts 1–7 are mapped to WAV order.
//!
//! HE-AAC: spectral band replication and parametric stereo
//! (`codec/sbr.rs`, `codec/ps.rs`) — object types 5 and 29, signalled
//! explicitly (the AudioSpecificConfig's hierarchical form or its 0x2b7 /
//! 0x548 sync extensions) or implicitly (ADTS, or an LC config whose first
//! access unit carries SBR fill elements: decided on that first unit, the
//! way the conformance decoders do). Output is at twice the core rate; a
//! mono core with PS decodes to stereo.
//!
//! Refused by name: Main-profile prediction, LTP, coupling channels, and
//! downsampled SBR (an SBR rate that is not twice the core's).

use std::sync::OnceLock;

use super::aac_tables::{BOOKS, SCF, SWB_LONG, SWB_SHORT};
use super::bits::MsbReader;
use super::{CodecError, Decoded};
use crate::fft::Imdct;
use crate::rng::Rng;

const RATES: [u32; 12] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
];

type Rd<'a> = MsbReader<'a>;

fn rd(r: &mut Rd<'_>, n: u32) -> Result<u32, CodecError> {
    r.read(n).ok_or(CodecError::Truncated)
}

/// Huffman tree for a codebook: leaf = !index.
struct Tree(Vec<[i32; 2]>);

fn build(codes: impl Iterator<Item = (u32, u8)>) -> Tree {
    let mut nodes = vec![[0i32; 2]];
    for (idx, (code, len)) in codes.enumerate() {
        let mut n = 0usize;
        for i in (0..len).rev() {
            let bit = ((code >> i) & 1) as usize;
            if i == 0 {
                nodes[n][bit] = !i32::try_from(idx).unwrap_or(0);
            } else {
                if nodes[n][bit] <= 0 {
                    nodes.push([0, 0]);
                    nodes[n][bit] = i32::try_from(nodes.len() - 1).unwrap_or(0);
                }
                n = usize::try_from(nodes[n][bit]).unwrap_or(0);
            }
        }
    }
    Tree(nodes)
}

fn trees() -> &'static (Vec<Tree>, Tree) {
    static T: OnceLock<(Vec<Tree>, Tree)> = OnceLock::new();
    T.get_or_init(|| {
        let books = BOOKS
            .iter()
            .map(|b| build(b.iter().map(|e| (e.0, e.1))))
            .collect();
        let scf = build(SCF.iter().copied());
        (books, scf)
    })
}

fn walk(r: &mut Rd<'_>, t: &Tree) -> Result<usize, CodecError> {
    let mut n = 0usize;
    loop {
        let b = usize::from(r.bit().ok_or(CodecError::Truncated)?);
        let v = t.0.get(n).ok_or(CodecError::Malformed("huffman"))?[b];
        if v < 0 {
            return usize::try_from(!v).map_err(|_| CodecError::Malformed("huffman"));
        }
        if v == 0 {
            return Err(CodecError::Malformed("invalid huffman code"));
        }
        n = usize::try_from(v).map_err(|_| CodecError::Malformed("huffman"))?;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Seq {
    #[default]
    OnlyLong,
    LongStart,
    EightShort,
    LongStop,
}

#[derive(Debug, Clone, Default)]
struct IcsInfo {
    seq: Seq,
    kbd: bool,
    max_sfb: usize,
    /// Group lengths in windows (sum 8 for short, `[1]` for long).
    groups: Vec<usize>,
}

impl IcsInfo {
    fn short(&self) -> bool {
        self.seq == Seq::EightShort
    }
}

fn ics_info(r: &mut Rd<'_>) -> Result<IcsInfo, CodecError> {
    rd(r, 1)?;
    let seq = match rd(r, 2)? {
        0 => Seq::OnlyLong,
        1 => Seq::LongStart,
        2 => Seq::EightShort,
        _ => Seq::LongStop,
    };
    let kbd = rd(r, 1)? == 1;
    if seq == Seq::EightShort {
        let max_sfb = rd(r, 4)? as usize;
        let grouping = rd(r, 7)?;
        let mut groups = vec![1usize];
        for i in (0..7).rev() {
            if grouping & (1 << i) != 0 {
                if let Some(last) = groups.last_mut() {
                    *last += 1;
                }
            } else {
                groups.push(1);
            }
        }
        Ok(IcsInfo {
            seq,
            kbd,
            max_sfb,
            groups,
        })
    } else {
        let max_sfb = rd(r, 6)? as usize;
        if rd(r, 1)? == 1 {
            return Err(CodecError::Unsupported(
                "AAC prediction (Main/LTP profiles)",
            ));
        }
        Ok(IcsInfo {
            seq,
            kbd,
            max_sfb,
            groups: vec![1],
        })
    }
}

/// One TNS filter: (start band, end band, runs upward, LPC coefficients).
type TnsFilter = (usize, usize, bool, Vec<f32>);

#[derive(Debug, Clone, Default)]
struct Tns {
    /// Per window: its filters.
    filters: Vec<Vec<TnsFilter>>,
}

/// One decoded channel's spectrum (1024 values; short windows as 8×128).
#[derive(Debug, Clone)]
struct Channel {
    info: IcsInfo,
    spec: Vec<f32>,
    /// Codebook per (group, band).
    cb: Vec<Vec<u8>>,
    /// Scalefactor / intensity position / noise energy per (group, band).
    sf: Vec<Vec<i32>>,
    tns: Tns,
}

const ZERO_HCB: u8 = 0;
const NOISE_HCB: u8 = 13;
const INTENSITY_HCB2: u8 = 14;
const INTENSITY_HCB: u8 = 15;

fn swb(rate_idx: usize, short: bool) -> &'static [u16] {
    if short {
        SWB_SHORT[rate_idx]
    } else {
        SWB_LONG[rate_idx]
    }
}

#[expect(clippy::too_many_lines, reason = "the ICS syntax, top to bottom")]
fn ics(
    r: &mut Rd<'_>,
    rate_idx: usize,
    common: Option<&IcsInfo>,
    rng: &mut Rng,
) -> Result<Channel, CodecError> {
    let global_gain = i32::try_from(rd(r, 8)?).unwrap_or(0);
    let info = match common {
        Some(i) => i.clone(),
        None => ics_info(r)?,
    };
    let short = info.short();
    let bands = swb(rate_idx, short);
    let nbands = bands.len() - 1;
    if info.max_sfb > nbands {
        return Err(CodecError::Malformed("max_sfb"));
    }
    let ngroups = info.groups.len();
    // Section data.
    let (sect_bits, esc) = if short { (3, 7) } else { (5, 31) };
    let mut cb = vec![vec![0u8; info.max_sfb]; ngroups];
    for g in cb.iter_mut() {
        let mut k = 0;
        while k < info.max_sfb {
            let book = u8::try_from(rd(r, 4)?).unwrap_or(0);
            if book == 12 {
                return Err(CodecError::Malformed("reserved codebook 12"));
            }
            let mut len = 0usize;
            loop {
                let inc = rd(r, sect_bits)? as usize;
                len += inc;
                if inc != esc {
                    break;
                }
            }
            if k + len > info.max_sfb {
                return Err(CodecError::Malformed("section length"));
            }
            for b in &mut g[k..k + len] {
                *b = book;
            }
            k += len;
        }
    }
    // Scalefactors.
    let (_, scf_tree) = trees();
    let mut sf = vec![vec![0i32; info.max_sfb]; ngroups];
    let mut gain = global_gain;
    let mut is_pos = 0i32;
    let mut noise = global_gain - 90;
    let mut first_noise = true;
    for (g, row) in sf.iter_mut().enumerate() {
        for (b, v) in row.iter_mut().enumerate() {
            match cb[g][b] {
                ZERO_HCB => *v = 0,
                INTENSITY_HCB | INTENSITY_HCB2 => {
                    is_pos += i32::try_from(walk(r, scf_tree)?).unwrap_or(60) - 60;
                    *v = is_pos;
                }
                NOISE_HCB => {
                    if first_noise {
                        first_noise = false;
                        noise += i32::try_from(rd(r, 9)?).unwrap_or(256) - 256;
                    } else {
                        noise += i32::try_from(walk(r, scf_tree)?).unwrap_or(60) - 60;
                    }
                    *v = noise;
                }
                _ => {
                    gain += i32::try_from(walk(r, scf_tree)?).unwrap_or(60) - 60;
                    *v = gain;
                }
            }
        }
    }
    // Pulse data.
    let mut pulses: Vec<(usize, i32)> = Vec::new();
    if rd(r, 1)? == 1 {
        if short {
            return Err(CodecError::Malformed("pulse data in a short window"));
        }
        let n = rd(r, 2)? as usize + 1;
        let start = rd(r, 6)? as usize;
        let mut k = usize::from(
            *bands
                .get(start)
                .ok_or(CodecError::Malformed("pulse start"))?,
        );
        for _ in 0..n {
            k += rd(r, 5)? as usize;
            let amp = i32::try_from(rd(r, 4)?).unwrap_or(0);
            pulses.push((k, amp));
        }
    }
    // TNS.
    let mut tns = Tns::default();
    if rd(r, 1)? == 1 {
        let windows = if short { 8 } else { 1 };
        let (nf_bits, len_bits, ord_bits) = if short { (1, 4, 3) } else { (2, 6, 5) };
        for _ in 0..windows {
            let nf = rd(r, nf_bits)?;
            let mut fl = Vec::new();
            if nf > 0 {
                let res = rd(r, 1)?;
                let mut top = nbands.min(info.max_sfb.max(nbands));
                top = top.min(nbands);
                for _ in 0..nf {
                    let len = rd(r, len_bits)? as usize;
                    let order = rd(r, ord_bits)? as usize;
                    let bottom = top.saturating_sub(len);
                    if order > 0 {
                        let up = rd(r, 1)? == 0;
                        let compress = rd(r, 1)?;
                        let bits = res + 3 - compress;
                        let mut coefs = Vec::with_capacity(order);
                        for _ in 0..order {
                            let raw = rd(r, bits)?;
                            // Sign-extend, then inverse-quantise (sin domain).
                            let shift = 32 - bits;
                            let q = (raw.cast_signed() << shift) >> shift;
                            let iqfac = if q >= 0 {
                                ((1 << (res + 3 - 1)) as f64 - 0.5) / (std::f64::consts::PI / 2.0)
                            } else {
                                ((1 << (res + 3 - 1)) as f64 + 0.5) / (std::f64::consts::PI / 2.0)
                            };
                            #[expect(
                                clippy::cast_possible_truncation,
                                reason = "f32 coefficients"
                            )]
                            coefs.push((f64::from(q) / iqfac).sin() as f32);
                        }
                        fl.push((bottom, top, up, parcor_to_lpc(&coefs)));
                    }
                    top = bottom;
                }
            }
            tns.filters.push(fl);
        }
    }
    if rd(r, 1)? == 1 {
        return Err(CodecError::Unsupported("AAC gain control (SSR profile)"));
    }
    // Spectral data.
    let (books, _) = trees();
    let mut q = vec![0i32; 1024];
    let mut win0 = 0usize; // first window of the current group
    for (g, &glen) in info.groups.iter().enumerate() {
        for b in 0..info.max_sfb {
            let book = cb[g][b];
            let (lo, hi) = (usize::from(bands[b]), usize::from(bands[b + 1]));
            if book == ZERO_HCB || book >= NOISE_HCB {
                continue;
            }
            let bi = usize::from(book);
            let entries = BOOKS[bi];
            let (dims, unsigned) = match book {
                1 | 2 => (4, false),
                3 | 4 => (4, true),
                5 | 6 => (2, false),
                _ => (2, true),
            };
            for w in 0..glen {
                let base = (win0 + w) * 128 * usize::from(short);
                let mut k = lo;
                while k < hi {
                    let idx = walk(r, &books[bi])?;
                    let e = entries[idx];
                    let mut vals = [
                        i32::from(e.2),
                        i32::from(e.3),
                        i32::from(e.4),
                        i32::from(e.5),
                    ];
                    if unsigned {
                        for v in vals.iter_mut().take(dims) {
                            if *v != 0 && rd(r, 1)? == 1 {
                                *v = -*v;
                            }
                        }
                    }
                    if book == 11 {
                        for v in vals.iter_mut().take(2) {
                            if v.abs() == 16 {
                                let mut n = 0;
                                while rd(r, 1)? == 1 {
                                    n += 1;
                                    if n > 8 {
                                        return Err(CodecError::Malformed("escape"));
                                    }
                                }
                                let word = i32::try_from(rd(r, n + 4)?).unwrap_or(0);
                                let mag = (1 << (n + 4)) + word;
                                *v = if *v < 0 { -mag } else { mag };
                            }
                        }
                    }
                    for (j, v) in vals.iter().take(dims).enumerate() {
                        if let Some(slot) = q.get_mut(base + k + j) {
                            *slot = *v;
                        }
                    }
                    k += dims;
                }
            }
        }
        win0 += glen;
    }
    for (k, amp) in pulses {
        if let Some(v) = q.get_mut(k) {
            *v += if *v < 0 { -amp } else { amp };
        }
    }
    // Requantise, and noise substitution.
    let mut spec = vec![0f32; 1024];
    let mut win0 = 0usize;
    for (g, &glen) in info.groups.iter().enumerate() {
        for b in 0..info.max_sfb {
            let (lo, hi) = (usize::from(bands[b]), usize::from(bands[b + 1]));
            let book = cb[g][b];
            for w in 0..glen {
                let base = (win0 + w) * 128 * usize::from(short);
                if book == NOISE_HCB {
                    let mut energy = 0.0f64;
                    let mut tmp = vec![0f64; hi - lo];
                    for t in &mut tmp {
                        *t = rng.range(-1.0, 1.0);
                        energy += *t * *t;
                    }
                    let scale = 2f64.powf(0.25 * f64::from(sf[g][b])) / energy.max(1e-30).sqrt();
                    for (j, t) in tmp.iter().enumerate() {
                        #[expect(clippy::cast_possible_truncation, reason = "f32 audio")]
                        {
                            spec[base + lo + j] = (t * scale) as f32;
                        }
                    }
                } else if book != ZERO_HCB && book < NOISE_HCB {
                    let scale = 2f64.powf(0.25 * f64::from(sf[g][b] - 100));
                    for k in lo..hi {
                        let v = q[base + k];
                        if v != 0 {
                            let m = f64::from(v.unsigned_abs()).powf(4.0 / 3.0) * scale;
                            #[expect(clippy::cast_possible_truncation, reason = "f32 audio")]
                            {
                                spec[base + k] = if v < 0 { -m as f32 } else { m as f32 };
                            }
                        }
                    }
                }
            }
        }
        win0 += glen;
    }
    Ok(Channel {
        info,
        spec,
        cb,
        sf,
        tns,
    })
}

/// Step-up recursion: reflection coefficients to direct-form LPC.
fn parcor_to_lpc(k: &[f32]) -> Vec<f32> {
    let mut a: Vec<f32> = Vec::with_capacity(k.len());
    for (m, &km) in k.iter().enumerate() {
        let prev = a.clone();
        a.push(km);
        for i in 0..m {
            a[i] = prev[i] + km * prev[m - 1 - i];
        }
    }
    a
}

/// TNS_MAX_BANDS for AAC-LC (long, short) per sampling-frequency index.
const TNS_MAX_BANDS: [(usize, usize); 12] = [
    (31, 9),
    (31, 9),
    (34, 10),
    (40, 14),
    (42, 14),
    (51, 14),
    (46, 14),
    (46, 14),
    (42, 14),
    (42, 14),
    (42, 14),
    (39, 14),
];

fn apply_tns(ch: &mut Channel, rate_idx: usize) {
    let short = ch.info.short();
    let bands = swb(rate_idx, short);
    let nbands = bands.len() - 1;
    let limit = if short {
        TNS_MAX_BANDS[rate_idx].1
    } else {
        TNS_MAX_BANDS[rate_idx].0
    };
    let max = ch.info.max_sfb.min(nbands).min(limit);
    for (w, filters) in ch.tns.filters.iter().enumerate() {
        let base = if short { w * 128 } else { 0 };
        for (bottom, top, up, lpc) in filters {
            let start = usize::from(bands[(*bottom).min(max)]);
            let end = usize::from(bands[(*top).min(max)]);
            if end <= start || lpc.is_empty() {
                continue;
            }
            let order = lpc.len();
            let mut hist = vec![0f32; order];
            let n = end - start;
            for i in 0..n {
                let k = if *up { start + i } else { end - 1 - i };
                let x = ch.spec[base + k];
                let mut y = x;
                for j in 0..order {
                    y -= lpc[j] * hist[j];
                }
                hist.rotate_right(1);
                hist[0] = y;
                ch.spec[base + k] = y;
            }
        }
    }
}

fn ms_and_intensity(l: &mut Channel, r: &mut Channel, ms: &[Vec<bool>], rate_idx: usize) {
    let short = l.info.short();
    let bands = swb(rate_idx, short);
    let mut win0 = 0usize;
    for (g, &glen) in l.info.groups.iter().enumerate() {
        // The spec requires both channels of a pair to share one window
        // sequence, but the bitstream grammar lets a hostile file say
        // otherwise when `common_window` is false — and the right channel's
        // codebook and scalefactor rows exist only for the group count it
        // declared. Indexing them with the left's group count is the panic;
        // stopping at the first group the right channel does not have
        // degrades the rest of the element to independent stereo, which is
        // what the element claimed to be anyway.
        if r.cb.get(g).is_none() || r.sf.get(g).is_none() {
            break;
        }
        for b in 0..l.info.max_sfb.min(r.info.max_sfb) {
            let (lo, hi) = (usize::from(bands[b]), usize::from(bands[b + 1]));
            let rb = r.cb[g][b];
            let msb = ms.get(g).and_then(|m| m.get(b)).copied().unwrap_or(false);
            for w in 0..glen {
                let base = (win0 + w) * 128 * usize::from(short);
                if rb == INTENSITY_HCB || rb == INTENSITY_HCB2 {
                    let mut sign = if rb == INTENSITY_HCB { 1.0 } else { -1.0 };
                    if msb {
                        sign = -sign;
                    }
                    #[expect(clippy::cast_possible_truncation, reason = "f32 audio")]
                    let scale = (sign * 0.5f64.powf(0.25 * f64::from(r.sf[g][b]))) as f32;
                    for k in lo..hi {
                        r.spec[base + k] = l.spec[base + k] * scale;
                    }
                } else if msb && l.cb[g][b] < NOISE_HCB && rb < NOISE_HCB {
                    for k in lo..hi {
                        let (m, s) = (l.spec[base + k], r.spec[base + k]);
                        l.spec[base + k] = m + s;
                        r.spec[base + k] = m - s;
                    }
                }
            }
        }
        win0 += glen;
    }
}

struct Windows {
    long: [Vec<f32>; 2],  // [sine, kbd], 1024 rising halves
    short: [Vec<f32>; 2], // 128 rising halves
}

fn kbd(n: usize, alpha: f64) -> Vec<f32> {
    let half = n / 2;
    let beta = std::f64::consts::PI * alpha;
    let i0b = crate::fx::fir::bessel_i0(beta);
    #[expect(clippy::cast_precision_loss, reason = "window sizes")]
    let k: Vec<f64> = (0..=half)
        .map(|j| {
            let r = 2.0 * j as f64 / half as f64 - 1.0;
            crate::fx::fir::bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0b
        })
        .collect();
    let total: f64 = k.iter().sum();
    let mut acc = 0.0;
    (0..half)
        .map(|j| {
            acc += k[j];
            #[expect(clippy::cast_possible_truncation, reason = "f32 window")]
            let v = (acc / total).sqrt() as f32;
            v
        })
        .collect()
}

fn sine(n: usize) -> Vec<f32> {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "f32 window"
    )]
    (0..n / 2)
        .map(|i| (std::f64::consts::PI / n as f64 * (i as f64 + 0.5)).sin() as f32)
        .collect()
}

fn windows() -> &'static Windows {
    static W: OnceLock<Windows> = OnceLock::new();
    W.get_or_init(|| Windows {
        long: [sine(2048), kbd(2048, 4.0)],
        short: [sine(256), kbd(256, 6.0)],
    })
}

/// Filterbank state for one channel.
#[derive(Debug, Clone)]
struct Fb {
    overlap: Vec<f32>,
    prev_kbd: bool,
}

impl Fb {
    fn new() -> Self {
        Self {
            overlap: vec![0.0; 1024],
            prev_kbd: false,
        }
    }

    fn run(&mut self, ch: &Channel, long: &Imdct, short: &Imdct) -> Vec<f32> {
        let w = windows();
        let cur = usize::from(ch.info.kbd);
        let prev = usize::from(self.prev_kbd);
        let mut buf = vec![0f32; 2048];
        match ch.info.seq {
            Seq::EightShort => {
                let mut y = vec![0f32; 256];
                for win in 0..8 {
                    short.run(&ch.spec[win * 128..win * 128 + 128], &mut y);
                    let left = if win == 0 {
                        &w.short[prev]
                    } else {
                        &w.short[cur]
                    };
                    let right = &w.short[cur];
                    for i in 0..256 {
                        let wv = if i < 128 { left[i] } else { right[255 - i] };
                        buf[448 + win * 128 + i] += y[i] * wv * (2.0 / 256.0);
                    }
                }
            }
            seq => {
                long.run(&ch.spec, &mut buf);
                for (i, v) in buf.iter_mut().enumerate() {
                    let left = match seq {
                        Seq::LongStop => {
                            if i < 448 {
                                0.0
                            } else if i < 576 {
                                w.short[prev][i - 448]
                            } else if i < 1024 {
                                1.0
                            } else {
                                w.long[cur][2047 - i]
                            }
                        }
                        _ => {
                            if i < 1024 {
                                w.long[prev][i]
                            } else {
                                w.long[cur][2047 - i]
                            }
                        }
                    };
                    let wv = if seq == Seq::LongStart && i >= 1024 {
                        if i < 1472 {
                            1.0
                        } else if i < 1600 {
                            w.short[cur][1599 - i]
                        } else {
                            0.0
                        }
                    } else {
                        left
                    };
                    *v *= wv * (2.0 / 2048.0);
                }
            }
        }
        let out: Vec<f32> = (0..1024).map(|i| buf[i] + self.overlap[i]).collect();
        self.overlap.copy_from_slice(&buf[1024..]);
        self.prev_kbd = ch.info.kbd;
        out
    }
}

/// Whether SBR (and PS) run on this stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SbrMode {
    /// AAC-LC: no SBR (none signalled, none found on the first unit).
    Off,
    /// Not signalled either way: decided by the first access unit.
    Implicit,
    /// HE-AAC; `ps` when a mono core decodes to stereo.
    On { ps: bool },
}

/// The decoder for a stream of raw data blocks.
struct Decoder {
    rate_idx: usize,
    channels: usize,
    fbs: Vec<Fb>,
    long: Imdct,
    short: Imdct,
    rng: Rng,
    sbr: SbrMode,
    /// One SBR instance per syntax element, in element order.
    sbrs: Vec<super::sbr::Sbr>,
    blocks: u64,
}

impl Decoder {
    fn new(rate_idx: usize, channels: usize) -> Self {
        Self::with_sbr(rate_idx, channels, SbrMode::Implicit)
    }

    fn with_sbr(rate_idx: usize, channels: usize, sbr: SbrMode) -> Self {
        // SBR runs at twice the core rate, which must exist (≤ 96 kHz).
        let sbr = if rate_idx < 3 { SbrMode::Off } else { sbr };
        Self {
            rate_idx,
            channels,
            fbs: (0..channels.max(1)).map(|_| Fb::new()).collect(),
            long: Imdct::new(2048),
            short: Imdct::new(256),
            rng: Rng::new(0x5EED),
            sbr,
            sbrs: Vec::new(),
            blocks: 0,
        }
    }

    /// Output channels (a PS stream's mono core is two).
    fn out_channels(&self) -> usize {
        match self.sbr {
            SbrMode::On { ps: true } => 2,
            _ => self.channels.max(1),
        }
    }

    /// Output rate.
    fn out_rate(&self) -> u32 {
        let core = RATES[self.rate_idx];
        if matches!(self.sbr, SbrMode::On { .. }) {
            core * 2
        } else {
            core
        }
    }

    /// Samples per channel per access unit.
    fn frame_len(&self) -> u64 {
        if matches!(self.sbr, SbrMode::On { .. }) {
            2 * AAC_FRAME
        } else {
            AAC_FRAME
        }
    }

    /// Decode one raw_data_block into interleaved PCM (1024 frames).
    fn block(&mut self, data: &[u8]) -> Result<Vec<f32>, CodecError> {
        let mut r = Rd::new(data);
        let mut chans: Vec<Channel> = Vec::new();
        // (element id, first channel, channel count), in bitstream order.
        let mut elems: Vec<(u32, usize, usize)> = Vec::new();
        loop {
            let id = rd(&mut r, 3)?;
            match id {
                0 | 3 => {
                    rd(&mut r, 4)?;
                    let mut c = ics(&mut r, self.rate_idx, None, &mut self.rng)?;
                    apply_tns(&mut c, self.rate_idx);
                    elems.push((id, chans.len(), 1));
                    chans.push(c);
                }
                1 => {
                    rd(&mut r, 4)?;
                    let common = rd(&mut r, 1)? == 1;
                    let mut info = None;
                    let mut ms = Vec::new();
                    if common {
                        let i = ics_info(&mut r)?;
                        let mask = rd(&mut r, 2)?;
                        ms = match mask {
                            1 => {
                                let mut m = Vec::new();
                                for _ in 0..i.groups.len() {
                                    let row: Vec<bool> = (0..i.max_sfb)
                                        .map(|_| rd(&mut r, 1).map(|v| v == 1))
                                        .collect::<Result<_, _>>()?;
                                    m.push(row);
                                }
                                m
                            }
                            2 => vec![vec![true; i.max_sfb]; i.groups.len()],
                            _ => Vec::new(),
                        };
                        info = Some(i);
                    }
                    let mut l = ics(&mut r, self.rate_idx, info.as_ref(), &mut self.rng)?;
                    let mut rr = ics(&mut r, self.rate_idx, info.as_ref(), &mut self.rng)?;
                    if common {
                        ms_and_intensity(&mut l, &mut rr, &ms, self.rate_idx);
                    } else {
                        ms_and_intensity(&mut l, &mut rr, &[], self.rate_idx);
                    }
                    apply_tns(&mut l, self.rate_idx);
                    apply_tns(&mut rr, self.rate_idx);
                    elems.push((id, chans.len(), 2));
                    chans.push(l);
                    chans.push(rr);
                }
                2 => return Err(CodecError::Unsupported("AAC coupling channel elements")),
                4 => {
                    rd(&mut r, 4)?;
                    let align = rd(&mut r, 1)? == 1;
                    let mut n = rd(&mut r, 8)? as usize;
                    if n == 255 {
                        n += rd(&mut r, 8)? as usize;
                    }
                    if align {
                        r.align();
                    }
                    r.skip(n * 8).ok_or(CodecError::Truncated)?;
                }
                5 => skip_pce(&mut r)?,
                6 => {
                    let mut n = rd(&mut r, 4)? as usize;
                    if n == 15 {
                        // cnt = 15 + esc_count - 1 (ISO 14496-3 fill_element);
                        // add before subtracting, or esc_count 0 underflows.
                        n = n + rd(&mut r, 8)? as usize - 1;
                    }
                    if n * 8 > r.remaining() {
                        return Err(CodecError::Truncated);
                    }
                    // extension_payload: EXT_SBR_DATA (13) / _CRC (14) belongs
                    // to the element just before it.
                    let kind = if n > 0 { r.peek(4) } else { None };
                    let target = elems.len().checked_sub(1).filter(|&e| elems[e].0 != 3);
                    match (kind, target, self.sbr) {
                        (Some(k @ (13 | 14)), Some(e), SbrMode::Implicit | SbrMode::On { .. }) => {
                            rd(&mut r, 4)?;
                            let rate = RATES[self.rate_idx] * 2;
                            while self.sbrs.len() <= e {
                                let ch = elems[self.sbrs.len()].2;
                                self.sbrs.push(super::sbr::Sbr::new(rate, ch));
                            }
                            let allow_ps = elems[e].2 == 1 && self.channels <= 1;
                            self.sbrs[e].read(&mut r, n * 8 - 4, k == 14, allow_ps);
                        }
                        _ => r.skip(n * 8).ok_or(CodecError::Truncated)?,
                    }
                }
                _ => break,
            }
        }
        let nch = self.channels.max(chans.len()).max(1);
        while self.fbs.len() < chans.len() {
            self.fbs.push(Fb::new());
        }
        let core: Vec<Vec<f32>> = chans
            .iter()
            .enumerate()
            .take(nch)
            .map(|(c, ch)| self.fbs[c].run(ch, &self.long, &self.short))
            .collect();
        if self.sbr == SbrMode::Implicit {
            // Decided on the first unit: SBR if it carried SBR data, PS if
            // that data (on a mono stream) carried a PS header.
            self.sbr = if self.sbrs.is_empty() {
                SbrMode::Off
            } else {
                SbrMode::On {
                    ps: nch == 1 && self.sbrs.first().is_some_and(super::sbr::Sbr::has_ps),
                }
            };
        }
        self.blocks += 1;
        let out: Vec<Vec<f32>> = match self.sbr {
            SbrMode::On { ps } => {
                let rate = RATES[self.rate_idx] * 2;
                while self.sbrs.len() < elems.len() {
                    let ch = elems[self.sbrs.len()].2;
                    self.sbrs.push(super::sbr::Sbr::new(rate, ch));
                }
                let mut out = Vec::with_capacity(nch + 1);
                for (e, &(_, first, count)) in elems.iter().enumerate() {
                    let ins: Vec<&[f32]> = core
                        .get(first..(first + count).min(core.len()))
                        .unwrap_or(&[])
                        .iter()
                        .map(Vec::as_slice)
                        .collect();
                    if ins.len() != count {
                        continue;
                    }
                    let n_out = if ps && count == 1 && e == 0 { 2 } else { count };
                    let mut o = vec![Vec::new(); n_out];
                    self.sbrs[e].apply(&ins, &mut o);
                    out.extend(o);
                }
                out
            }
            _ => core,
        };
        let nout = self.out_channels().max(out.len()).max(1);
        let len = out.first().map_or(1024, Vec::len);
        let mut pcm = vec![0f32; len * nout];
        for (c, ch) in out.iter().enumerate().take(nout) {
            for (i, v) in ch.iter().enumerate() {
                pcm[i * nout + c] = v / 32_768.0;
            }
        }
        Ok(pcm)
    }
}

fn skip_pce(r: &mut Rd<'_>) -> Result<(), CodecError> {
    rd(r, 4)?;
    rd(r, 2)?;
    rd(r, 4)?;
    let front = rd(r, 4)?;
    let side = rd(r, 4)?;
    let back = rd(r, 4)?;
    let lfe = rd(r, 2)?;
    let assoc = rd(r, 3)?;
    let cc = rd(r, 4)?;
    for _ in 0..3 {
        if rd(r, 1)? == 1 {
            rd(r, 4)?;
        }
    }
    if rd(r, 1)? == 1 {
        rd(r, 3)?;
    }
    for _ in 0..(front + side + back) {
        rd(r, 5)?;
    }
    for _ in 0..lfe {
        rd(r, 4)?;
    }
    for _ in 0..assoc {
        rd(r, 4)?;
    }
    for _ in 0..cc {
        rd(r, 5)?;
    }
    r.align();
    let n = rd(r, 8)? as usize;
    r.skip(n * 8).ok_or(CodecError::Truncated)
}

/// AAC's channel order for configuration `c` mapped to WAV order.
fn reorder(pcm: &mut [f32], channels: usize, config: u8) {
    // AAC: C, L, R, Ls, Rs, LFE (config 6). WAV: L, R, C, LFE, Ls, Rs.
    let map: &[usize] = match config {
        3 => &[1, 2, 0],
        4 => &[1, 2, 0, 3],
        5 => &[1, 2, 0, 3, 4],
        6 => &[1, 2, 0, 5, 3, 4],
        7 => &[3, 4, 0, 7, 5, 6, 1, 2],
        _ => return,
    };
    if map.len() != channels {
        return;
    }
    for f in pcm.chunks_exact_mut(channels) {
        let src = f.to_vec();
        for (dst, &s) in f.iter_mut().zip(map) {
            *dst = src[s];
        }
    }
}

/// Decode an ADTS AAC stream.
///
/// # Errors
/// Malformed frames or unsupported profiles.
/// An ADTS frame header.
#[derive(Debug, Clone, Copy)]
struct Adts {
    /// Header bytes (7, or 9 with a CRC).
    hdr: usize,
    /// The whole frame's length.
    len: usize,
    rate_idx: usize,
    channel_cfg: u8,
}

/// The ADTS header at `b` (the stream from a sync word on), checked as
/// [`decode_adts`] checks it; `remaining` is how many bytes the stream has
/// from here, which a frame may not run past.
fn adts_header(b: &[u8], remaining: usize) -> Result<Adts, CodecError> {
    let mut r = Rd::new(b);
    rd(&mut r, 12)?;
    rd(&mut r, 1)?;
    rd(&mut r, 2)?;
    let no_crc = rd(&mut r, 1)? == 1;
    let profile = rd(&mut r, 2)?;
    let rate_idx = rd(&mut r, 4)? as usize;
    rd(&mut r, 1)?;
    let channel_cfg = u8::try_from(rd(&mut r, 3)?).unwrap_or(0);
    rd(&mut r, 4)?;
    let len = rd(&mut r, 13)? as usize;
    rd(&mut r, 11)?;
    let blocks = rd(&mut r, 2)? as usize + 1;
    if profile != 1 {
        return Err(CodecError::Unsupported("AAC profiles other than LC"));
    }
    if rate_idx >= 12 || len < 7 || len > remaining {
        return Err(CodecError::Malformed("ADTS header"));
    }
    let hdr = if no_crc { 7 } else { 9 };
    // A frame with a CRC has a 9-byte header: a declared length of 7 or
    // 8 passed the check above and sliced `at+9..at+len` backwards
    // (fuzz finding: "slice index starts at 86 but ends at 85").
    if len < hdr {
        return Err(CodecError::Malformed("ADTS frame shorter than its header"));
    }
    if blocks > 1 {
        return Err(CodecError::Unsupported(
            "multiple raw blocks per ADTS frame",
        ));
    }
    Ok(Adts {
        hdr,
        len,
        rate_idx,
        channel_cfg,
    })
}

const fn is_adts_sync(b: &[u8]) -> bool {
    b.len() >= 2 && b[0] == 0xFF && b[1] & 0xF6 == 0xF0
}

pub fn decode_adts(bytes: &[u8]) -> Result<Decoded, CodecError> {
    let mut at = 0;
    let mut dec: Option<Decoder> = None;
    let mut cfg = 0u8;
    let mut out = Vec::new();
    let mut rate = 0;
    while at + 7 <= bytes.len() {
        if !is_adts_sync(&bytes[at..]) {
            at += 1;
            continue;
        }
        let h = adts_header(&bytes[at..], bytes.len() - at)?;
        let d =
            dec.get_or_insert_with(|| Decoder::new(h.rate_idx, usize::from(h.channel_cfg.max(1))));
        cfg = h.channel_cfg;
        rate = RATES[h.rate_idx];
        out.extend(d.block(&bytes[at + h.hdr..at + h.len])?);
        at += h.len;
    }
    let d = dec.ok_or(CodecError::NotThisFormat)?;
    let _ = rate;
    let ch = d.out_channels();
    reorder(&mut out, ch, cfg);
    Decoded {
        data: out,
        channels: u8::try_from(ch).unwrap_or(2),
        rate: d.out_rate(),
    }
    .validated()
}

/// What an AudioSpecificConfig says: the core's rate index, the channel configuration and whether SBR/PS run — refusing what
/// this decoder does not implement (cores other than AAC-LC, 960-sample
/// frames, downsampled SBR).
struct Asc {
    ri: usize,
    channel_cfg: u8,
    sbr: SbrMode,
}

fn audio_config(config: &[u8]) -> Result<Asc, CodecError> {
    let mut r = Rd::new(config);
    let object_type = |r: &mut Rd<'_>| -> Result<u32, CodecError> {
        let aot = rd(r, 5)?;
        Ok(if aot == 31 { 32 + rd(r, 6)? } else { aot })
    };
    let rate_of = |r: &mut Rd<'_>| -> Result<u32, CodecError> {
        let idx = rd(r, 4)? as usize;
        if idx == 15 {
            rd(r, 24)
        } else {
            RATES
                .get(idx)
                .copied()
                .ok_or(CodecError::Malformed("rate index"))
        }
    };
    let mut aot = object_type(&mut r)?;
    let rate = rate_of(&mut r)?;
    let channel_cfg = u8::try_from(rd(&mut r, 4)?).unwrap_or(0);
    // Explicit hierarchical signalling: SBR (5) or PS (29) wrapping a core.
    let mut sbr: Option<bool> = None;
    let mut ps: Option<bool> = None;
    let mut ext_rate = None;
    if aot == 5 || aot == 29 {
        sbr = Some(true);
        if aot == 29 {
            ps = Some(true);
        }
        ext_rate = Some(rate_of(&mut r)?);
        aot = object_type(&mut r)?;
    }
    if aot != 2 {
        return Err(CodecError::Unsupported("AAC object types other than LC"));
    }
    if rd(&mut r, 1)? == 1 {
        return Err(CodecError::Unsupported("960-sample AAC frames"));
    }
    if rd(&mut r, 1)? == 1 {
        rd(&mut r, 14)?; // coreCoderDelay
    }
    let _extension_flag = rd(&mut r, 1)?;
    // Backward-compatible signalling: a sync extension after the LC config.
    if sbr.is_none() && r.remaining() >= 16 && r.peek(11) == Some(0x2b7) {
        rd(&mut r, 11)?;
        if object_type(&mut r)? == 5 {
            let present = rd(&mut r, 1)? == 1;
            sbr = Some(present);
            if present {
                ext_rate = Some(rate_of(&mut r)?);
                if r.remaining() >= 12 && r.peek(11) == Some(0x548) {
                    rd(&mut r, 11)?;
                    ps = Some(rd(&mut r, 1)? == 1);
                }
            }
        }
    }
    let ri = RATES.iter().position(|v| *v == rate).unwrap_or(4);
    let mode = match sbr {
        Some(true) => {
            if ext_rate.is_some_and(|e| e != rate * 2) {
                return Err(CodecError::Unsupported("downsampled SBR"));
            }
            SbrMode::On {
                ps: ps == Some(true) && channel_cfg == 1,
            }
        }
        Some(false) => SbrMode::Off,
        None => SbrMode::Implicit,
    };
    Ok(Asc {
        ri,
        channel_cfg,
        sbr: mode,
    })
}

/// Settle an implicitly signalled stream's mode from its first access unit
/// (decoded once and thrown away), so a streaming source can state its rate
/// and channel count before the first chunk.
fn probe_mode(ri: usize, channels: usize, first: Option<&[u8]>) -> SbrMode {
    let Some(unit) = first else {
        return SbrMode::Off;
    };
    let mut d = Decoder::new(ri, channels);
    match d.block(unit) {
        Ok(_) => d.sbr,
        Err(_) => SbrMode::Off,
    }
}

/// Decode AAC in an MP4/M4A container.
///
/// # Errors
/// No audio track, unsupported object types, malformed frames.
pub fn decode_mp4(bytes: &[u8]) -> Result<Decoded, CodecError> {
    let t = super::mp4::audio_track(bytes)?;
    if t.codec != *b"mp4a" {
        return Err(CodecError::NotThisFormat);
    }
    let asc = audio_config(&t.config)?;
    let channel_cfg = asc.channel_cfg;
    let mut d = Decoder::with_sbr(asc.ri, usize::from(channel_cfg.max(1)), asc.sbr);
    let mut out = Vec::new();
    for (off, len) in &t.samples {
        let unit = bytes
            .get(*off..off.saturating_add(*len))
            .ok_or(CodecError::Truncated)?;
        out.extend(d.block(unit)?);
    }
    let ch = d.out_channels();
    let rate = d.out_rate();
    reorder(&mut out, ch, channel_cfg);
    let (skip, play) = media_to_output(t.skip, t.play, t.timescale, rate);
    let skip = usize::try_from(skip).unwrap_or(0) * ch;
    let mut data = out.get(skip.min(out.len())..).unwrap_or(&[]).to_vec();
    if let Some(p) = play {
        data.truncate(usize::try_from(p).unwrap_or(usize::MAX) * ch);
    }
    Decoded {
        data,
        channels: u8::try_from(ch).unwrap_or(2),
        rate,
    }
    .validated()
}

/// An edit list's skip/play, given in the media timescale, in output
/// samples. Equal for LC and for HE-AAC muxed at its output rate (the
/// usual case); an HE-AAC track timed at its core rate is scaled.
fn media_to_output(skip: u64, play: Option<u64>, timescale: u32, rate: u32) -> (u64, Option<u64>) {
    if timescale == 0 || timescale == rate {
        return (skip, play);
    }
    let conv = |v: u64| v.saturating_mul(u64::from(rate)) / u64::from(timescale);
    (conv(skip), play.map(conv))
}

// ── streaming (AU-05) ────────────────────────────────────────────────────

/// Frames decoded before a seek target: the filterbank's overlap is one
/// frame deep.
const AAC_PREROLL: u64 = 2;

/// Units decoded before a seek target. SBR adds its QMF banks' delay and,
/// worse, time-differential envelopes that chain back to the last
/// frequency-coded one; the encoders this was graded against restart that
/// chain (and send a header) at least every 16 units.
const fn preroll(mode: SbrMode) -> u64 {
    match mode {
        SbrMode::On { .. } => 16,
        _ => AAC_PREROLL,
    }
}
/// Samples per AAC-LC frame.
const AAC_FRAME: u64 = 1024;

/// AAC in MP4/M4A an access unit at a time, for [`super::stream`]: the
/// `moov` box is read (its sample table lists every unit's bytes), and
/// each unit is read from the file as it is decoded.
pub(crate) struct Mp4Source {
    track: super::mp4::AudioTrack,
    rate: u32,
    ri: usize,
    channel_cfg: u8,
    mode: SbrMode,
    dec: Decoder,
    next: usize,
}

impl Mp4Source {
    /// A source over a track already demuxed — the codec was picked from
    /// its `stsd` entry type by the caller ([`super::stream`], which owns
    /// the one `moov` read every streaming MP4 codec starts from).
    pub(crate) fn from_track(
        track: super::mp4::AudioTrack,
        input: &mut super::stream::Input,
    ) -> Result<Self, CodecError> {
        let asc = audio_config(&track.config)?;
        let channels = usize::from(asc.channel_cfg.max(1));
        let mut mode = asc.sbr;
        if mode == SbrMode::Implicit {
            let first = match track.samples.first() {
                Some(&(off, len)) => Some(input.window(off as u64, len)?.to_vec()),
                None => None,
            };
            mode = probe_mode(asc.ri, channels, first.as_deref());
        }
        let dec = Decoder::with_sbr(asc.ri, channels, mode);
        Ok(Self {
            rate: dec.out_rate(),
            dec,
            track,
            ri: asc.ri,
            channel_cfg: asc.channel_cfg,
            mode,
            next: 0,
        })
    }
}

impl super::stream::FrameSource for Mp4Source {
    fn channels(&self) -> usize {
        self.dec.out_channels()
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn trim(&self) -> (u64, Option<u64>) {
        media_to_output(
            self.track.skip,
            self.track.play,
            self.track.timescale,
            self.rate,
        )
    }

    fn next(
        &mut self,
        input: &mut super::stream::Input,
    ) -> Result<Option<super::stream::Chunk>, CodecError> {
        let Some(&(off, len)) = self.track.samples.get(self.next) else {
            return Ok(None);
        };
        self.next += 1;
        let unit = input.window(off as u64, len)?;
        if unit.len() < len {
            return Err(CodecError::Truncated);
        }
        let mut pcm = self.dec.block(unit)?;
        reorder(&mut pcm, self.dec.out_channels(), self.channel_cfg);
        Ok(Some(super::stream::Chunk { pcm }))
    }

    fn seek(&mut self, _input: &mut super::stream::Input, frame: u64) -> Result<u64, CodecError> {
        let fl = self.dec.frame_len();
        let unit = (frame / fl).saturating_sub(preroll(self.mode));
        self.next = usize::try_from(unit)
            .unwrap_or(usize::MAX)
            .min(self.track.samples.len());
        self.dec = Decoder::with_sbr(self.ri, usize::from(self.channel_cfg.max(1)), self.mode);
        Ok(self.next as u64 * fl)
    }
}

/// AAC in ADTS frames, a frame at a time, for [`super::stream`].
pub(crate) struct AdtsSource {
    dec: Decoder,
    rate_idx: usize,
    /// Output channels / rate / samples per frame (SBR doubles the last two).
    channels: usize,
    rate: u32,
    frame_len: u64,
    core_channels: usize,
    mode: SbrMode,
    at: u64,
    first: u64,
    frames: u64,
    /// `(offset, frame number)` every 16 frames, for seeking.
    index: Vec<(u64, u64)>,
}

impl AdtsSource {
    pub(crate) fn open(input: &mut super::stream::Input) -> Result<Self, CodecError> {
        let len = input.len();
        let head = input.window(0, 64 << 10)?.to_vec();
        let first = (0..head.len().saturating_sub(7))
            .find(|&i| is_adts_sync(&head[i..]))
            .ok_or(CodecError::NotThisFormat)?;
        let h = adts_header(
            &head[first..],
            usize::try_from(len - first as u64).unwrap_or(usize::MAX),
        )?;
        let channels = usize::from(h.channel_cfg.max(1));
        let unit = head.get(first + h.hdr..first + h.len);
        let mode = probe_mode(h.rate_idx, channels, unit);
        let dec = Decoder::with_sbr(h.rate_idx, channels, mode);
        Ok(Self {
            channels: dec.out_channels(),
            rate: dec.out_rate(),
            frame_len: dec.frame_len(),
            core_channels: channels,
            mode,
            dec,
            rate_idx: h.rate_idx,
            at: first as u64,
            first: first as u64,
            frames: 0,
            index: vec![(first as u64, 0)],
        })
    }
}

impl super::stream::FrameSource for AdtsSource {
    fn channels(&self) -> usize {
        self.channels
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn trim(&self) -> (u64, Option<u64>) {
        (0, None)
    }

    fn next(
        &mut self,
        input: &mut super::stream::Input,
    ) -> Result<Option<super::stream::Chunk>, CodecError> {
        let len = input.len();
        loop {
            if self.at + 7 > len {
                return Ok(None);
            }
            let w = input.window(self.at, 8 << 10)?;
            if !is_adts_sync(w) {
                self.at += 1;
                continue;
            }
            let h = adts_header(w, usize::try_from(len - self.at).unwrap_or(usize::MAX))?;
            let frame = input.window(self.at, h.len)?;
            if frame.len() < h.len {
                return Err(CodecError::Truncated);
            }
            let mut pcm = self.dec.block(&frame[h.hdr..h.len])?;
            reorder(&mut pcm, self.channels, h.channel_cfg);
            if self.frames.is_multiple_of(16) && self.index.last().is_none_or(|l| l.1 < self.frames)
            {
                self.index.push((self.at, self.frames));
            }
            self.at += h.len as u64;
            self.frames += 1;
            return Ok(Some(super::stream::Chunk { pcm }));
        }
    }

    fn seek(&mut self, input: &mut super::stream::Input, frame: u64) -> Result<u64, CodecError> {
        let len = input.len();
        let want = (frame / self.frame_len).saturating_sub(preroll(self.mode));
        let &(mut pos, mut n) = self
            .index
            .iter()
            .rev()
            .find(|(_, f)| *f <= want)
            .unwrap_or(&(self.first, 0));
        while n < want && pos + 7 <= len {
            let w = input.window(pos, 16)?;
            if !is_adts_sync(w) {
                pos += 1;
                continue;
            }
            let Ok(h) = adts_header(w, usize::try_from(len - pos).unwrap_or(usize::MAX)) else {
                break;
            };
            pos += h.len as u64;
            n += 1;
            if n.is_multiple_of(16) && self.index.last().is_none_or(|l| l.1 < n) {
                self.index.push((pos, n));
            }
        }
        self.at = pos;
        self.frames = n;
        self.dec = Decoder::with_sbr(self.rate_idx, self.core_channels, self.mode);
        Ok(n * self.frame_len)
    }
}

#[cfg(test)]
mod tests {

    /// Fuzz regression (`decode_aac_adts`, CI 2026-10-25): a CRC-protected
    /// frame (9-byte header) declaring a length of 8 panicked slicing
    /// `at+9..at+8`. It must be refused as malformed.
    #[test]
    fn a_frame_shorter_than_its_crc_header_is_malformed_not_a_panic() {
        // sync FFF, MPEG-4, layer 0, protection_absent 0 (CRC present);
        // LC, 44.1 kHz (index 4), stereo; frame_length 8; fullness 0x7FF;
        // one raw block.
        let mut bytes = vec![0xFF, 0xF0, 0x50, 0x80, 0x01, 0x1F, 0xFC];
        bytes.resize(32, 0);
        for len in [7u16, 8] {
            bytes[3] = 0x80 | ((len >> 11) & 0x3) as u8;
            bytes[4] = ((len >> 3) & 0xFF) as u8;
            bytes[5] = (((len & 0x7) as u8) << 5) | 0x1F;
            assert!(
                matches!(decode_adts(&bytes), Err(CodecError::Malformed(_))),
                "length {len}"
            );
        }
    }

    use super::*;
    use crate::codec::{s16le, snr_db};

    #[test]
    fn parcor_conversion_matches_a_hand_case() {
        // Order 2: a = [k1 + k2 k1, k2].
        let a = parcor_to_lpc(&[0.5, -0.25]);
        assert!((a[0] - (0.5 + -0.25 * 0.5)).abs() < 1e-6 && (a[1] + 0.25).abs() < 1e-6);
    }

    #[test]
    fn kbd_window_is_power_complementary() {
        let w = kbd(2048, 4.0);
        for i in 0..1024 {
            let a = w[i];
            let b = w[1023 - i];
            assert!((a * a + b * b - 1.0).abs() < 1e-5, "{i}");
        }
    }

    #[test]
    fn decodes_ffmpegs_adts_mono_like_ffmpeg() {
        let d = decode_adts(include_bytes!("../../tests/fixtures/src_mono.aac")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_mono.aac.ref"));
        assert_eq!((d.channels, d.rate), (1, 44_100));
        assert_eq!(d.data.len(), reference.len());
        let (snr, lag) = snr_db(&reference, &d.data, 1, 2_048);
        eprintln!("aac adts mono: {snr:.1} dB at lag {lag}");
        assert_eq!(lag, 0);
        assert!(snr > 60.0, "SNR {snr} dB");
    }

    /// Energy in dB of `x` between `lo` and `hi` Hz, averaged over frames.
    fn band_db(x: &[f32], lo: f64, hi: f64) -> f64 {
        use crate::fft::{Complex, Plan};
        let n = 2048;
        let plan = Plan::new(n);
        let mut e = 0.0;
        let mut frames = 0;
        for start in (0..x.len().saturating_sub(n)).step_by(n / 2) {
            let mut buf: Vec<Complex> = (0..n)
                .map(|i| Complex::new(f64::from(x[start + i]), 0.0))
                .collect();
            plan.forward(&mut buf);
            for (k, z) in buf.iter().enumerate().take(n / 2) {
                let hz = k as f64 * 44_100.0 / n as f64;
                if hz >= lo && hz < hi {
                    e += z.norm().powi(2);
                }
            }
            frames += 1;
        }
        10.0 * (e / f64::from(frames.max(1))).log10()
    }

    #[test]
    fn noise_substitution_matches_the_reference_by_band_energy() {
        // PNS bands are decoder-generated noise: samples cannot match, the
        // energy per band must.
        let d = decode_adts(include_bytes!("../../tests/fixtures/src_mono_pns.aac")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_mono_pns.aac.ref"));
        for (lo, hi) in [
            (4_000.0, 8_000.0),
            (8_000.0, 12_000.0),
            (12_000.0, 16_000.0),
        ] {
            let (a, b) = (band_db(&d.data, lo, hi), band_db(&reference, lo, hi));
            eprintln!("pns {lo}-{hi} Hz: ours {a:.2} dB, reference {b:.2} dB");
            assert!((a - b).abs() < 1.5, "{lo}-{hi}: {a} vs {b}");
        }
        // Below the PNS bands the decode is still sample-exact.
        let (lo_ours, lo_ref) = (
            band_db(&d.data, 50.0, 2_000.0),
            band_db(&reference, 50.0, 2_000.0),
        );
        assert!((lo_ours - lo_ref).abs() < 0.05);
    }

    #[test]
    fn decodes_ffmpegs_m4a_stereo_with_its_edit_list() {
        let d = decode_mp4(include_bytes!("../../tests/fixtures/src.m4a")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src.m4a.ref"));
        assert_eq!((d.channels, d.rate), (2, 44_100));
        assert_eq!(d.frames(), 17_640, "priming skipped, padding trimmed");
        let (snr, lag) = snr_db(&reference, &d.data, 2, 2_048);
        eprintln!("aac m4a stereo: {snr:.1} dB at lag {lag}");
        assert_eq!(lag, 0);
        assert!(snr > 60.0, "SNR {snr} dB");
    }

    #[test]
    fn a_channel_pair_with_mismatched_window_shapes_degrades_not_panics() {
        // The spec requires both channels of a pair to share one window
        // sequence, but the bitstream grammar lets a hostile file disagree
        // when `common_window` is false. Left declares eight short-window
        // groups; right declares a single long one — indexing the right
        // channel's per-group rows with the left's group count used to
        // panic at the first missing group. The element now degrades to
        // independent stereo from the first shape mismatch onward.
        let l = Channel {
            info: IcsInfo {
                seq: Seq::EightShort,
                kbd: false,
                max_sfb: 2,
                groups: vec![1, 1, 1, 1, 1, 1, 1, 1],
            },
            spec: vec![0.0; 1024],
            cb: vec![vec![0u8; 2]; 8],
            sf: vec![vec![0i32; 2]; 8],
            tns: Tns::default(),
        };
        let r = Channel {
            info: IcsInfo {
                seq: Seq::OnlyLong,
                kbd: false,
                max_sfb: 2,
                groups: vec![1],
            },
            spec: vec![0.0; 1024],
            cb: vec![vec![0u8; 2]],
            sf: vec![vec![0i32; 2]],
            tns: Tns::default(),
        };
        let mut l = l;
        let mut r = r;
        ms_and_intensity(&mut l, &mut r, &[], 0);
        // Group 0 (the shapes' common prefix) still went through M/S with
        // an all-false matrix, so both channels stay zeroed here; the
        // assertion is that the call returned at all.
        assert!(l.spec.iter().all(|&s| s == 0.0));
        assert!(r.spec.iter().all(|&s| s == 0.0));
    }

    // ── HE-AAC (AU-06): fixtures encoded by the full FDK encoder
    // (tests/fixtures/he-aac-enc), references decoded by ffmpeg. ──

    fn graded(d: &super::Decoded, reference: &[f32], ch: usize, what: &str) -> f64 {
        assert_eq!(usize::from(d.channels), ch, "{what}: channels");
        assert_eq!(d.rate, 44_100, "{what}: SBR doubles the 22.05 kHz core");
        assert_eq!(d.data.len(), reference.len(), "{what}: length");
        // The reference is ffmpeg's 16-bit decode, so it is clipped where
        // the signal overshoots full scale (the PS fixture's last frames
        // reach -1.35); grade our float output through the same clip.
        let clipped: Vec<f32> = d
            .data
            .iter()
            .map(|v| v.clamp(-1.0, 32_767.0 / 32_768.0))
            .collect();
        let (snr, lag) = snr_db(reference, &clipped, ch, 2_048);
        eprintln!("{what}: {snr:.1} dB at lag {lag}");
        assert_eq!(lag, 0, "{what}: sample-aligned with the reference");
        snr
    }

    #[test]
    fn decodes_explicit_he_aac_m4a_like_ffmpeg() {
        let d = decode_mp4(include_bytes!("../../tests/fixtures/src_he.m4a")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_he.m4a.ref"));
        let snr = graded(&d, &reference, 2, "he-aac m4a (explicit SBR)");
        assert!(snr > 70.0, "SNR {snr} dB");
    }

    #[test]
    fn decodes_implicit_he_aac_adts_like_ffmpeg() {
        let d = decode_adts(include_bytes!("../../tests/fixtures/src_he.aac")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_he.aac.ref"));
        let snr = graded(&d, &reference, 2, "he-aac adts (implicit SBR)");
        assert!(snr > 70.0, "SNR {snr} dB");
    }

    #[test]
    fn decodes_mono_he_aac_adts_like_ffmpeg() {
        let d = decode_adts(include_bytes!("../../tests/fixtures/src_he_mono.aac")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_he_mono.aac.ref"));
        let snr = graded(&d, &reference, 1, "he-aac adts mono");
        assert!(snr > 70.0, "SNR {snr} dB");
    }

    #[test]
    fn decodes_explicit_he_aac_v2_m4a_like_ffmpeg() {
        let d = decode_mp4(include_bytes!("../../tests/fixtures/src_hev2.m4a")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_hev2.m4a.ref"));
        let snr = graded(&d, &reference, 2, "he-aac v2 m4a (explicit PS)");
        assert!(snr > 70.0, "SNR {snr} dB");
    }

    #[test]
    fn decodes_implicit_he_aac_v2_adts_like_ffmpeg() {
        let d = decode_adts(include_bytes!("../../tests/fixtures/src_hev2.aac")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_hev2.aac.ref"));
        let snr = graded(&d, &reference, 2, "he-aac v2 adts (implicit PS)");
        assert!(snr > 70.0, "SNR {snr} dB");
    }

    #[test]
    fn hostile_he_aac_payloads_degrade_never_panic() {
        // Seeded byte flips across the SBR/PS fixtures: every outcome must
        // be a decode or an error — a malformed SBR payload switches SBR off
        // until the next header, a malformed PS payload switches PS off.
        let mut rng = crate::rng::Rng::new(0xAE06);
        for (bytes, mp4) in [
            (
                &include_bytes!("../../tests/fixtures/src_he.aac")[..],
                false,
            ),
            (
                &include_bytes!("../../tests/fixtures/src_hev2.aac")[..],
                false,
            ),
            (&include_bytes!("../../tests/fixtures/src_he.m4a")[..], true),
            (
                &include_bytes!("../../tests/fixtures/src_hev2.m4a")[..],
                true,
            ),
        ] {
            for _ in 0..120 {
                let mut b = bytes.to_vec();
                for _ in 0..1 + (rng.below(8)) {
                    let i = rng.below(b.len());
                    b[i] ^= 1 << (rng.below(8));
                }
                let out = if mp4 { decode_mp4(&b) } else { decode_adts(&b) };
                if let Ok(d) = out {
                    assert!(d.data.iter().all(|v| v.is_finite()));
                }
            }
        }
    }
}
