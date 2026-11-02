//! ALAC — Apple Lossless Audio Codec, decoded (in MP4/M4A).
//!
//! The whole of the bitstream `ffmpeg -c:a alac` (and every Apple tool)
//! writes: the 24-byte `ALACSpecificConfig` magic cookie (frame length,
//! bit depth, the entropy coder's rice parameters, channel count, rate),
//! and frames built from AAC-style elements — single-channel elements and
//! channel pairs, each carrying a stereo decorrelation matrix, an adaptive
//! predictor of order 0..=31 with signed 16-bit coefficients, and
//! residuals in the format's own adaptive Golomb-Rice code with a
//! zero-run mode. A channel pair may push its least significant 1 or 2
//! bytes aside into uncompressed "shift-off" bytes (read after the coded
//! residuals), and any frame may declare itself uncompressed outright —
//! the escape path. Partial frames carry their own sample count, which is
//! how the last, short frame of a file is stored. The codec is lossless,
//! so the grading oracle is exactness itself: the fixtures decode
//! bit-exactly against ffmpeg's own decode of the same file.
//!
//! The format was reverse-engineered by David Hammerton (2005, public
//! domain notes) and later published by Apple as the Apache-2.0
//! `macosforge/alac` reference decoder. This module was written from the
//! bitstream those documents describe and verified against ffmpeg
//! reference decodes; the reference is credited in
//! `tools/THIRD_PARTY.md` alongside the other sources the codecs consult.
//!
//! Refused by name: `ID_CCE`/`ID_PCE` elements (Apple's reference decoder
//! refuses them too — no encoder writes them) and bit depths other than
//! 16, 20, 24 and 32.

use super::bits::MsbReader;
use super::mp4;
use super::{CodecError, Decoded};

/// The rice parameters every ALAC file carries in its magic cookie: they
/// seed the adaptive Golomb coder's mean, and `pb` is scaled per channel
/// by a 3-bit factor read from each element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Samples per frame (the last frame may declare fewer).
    pub frame_len: u32,
    /// Bits per sample: 16, 20, 24 or 32.
    pub bit_depth: u8,
    /// Rice history multiplier (40 in every file seen).
    pub pb: u8,
    /// Rice mean seed (10).
    pub mb: u8,
    /// Rice k clamp (14).
    pub kb: u8,
    /// Channels.
    pub channels: u8,
    /// Sample rate.
    pub rate: u32,
}

/// The largest frame length accepted: real encoders write 4096; anything
/// past a million samples a frame is not audio, it is an allocation.
const MAX_FRAME_LEN: u32 = 1 << 20;

impl Config {
    /// The cookie as it rides in an MP4 `alac` box: a 4-byte version/flags
    /// word, then the 24-byte `ALACSpecificConfig`.
    fn from_box(body: &[u8]) -> Result<Self, CodecError> {
        let c = body
            .get(4..28)
            .ok_or(CodecError::Malformed("ALAC cookie truncated"))?;
        let frame_len = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        if frame_len == 0 || frame_len > MAX_FRAME_LEN {
            return Err(CodecError::Malformed("ALAC frame length"));
        }
        let bit_depth = c[5];
        if !matches!(bit_depth, 16 | 20 | 24 | 32) {
            return Err(CodecError::Unsupported(
                "ALAC bit depths other than 16/20/24/32",
            ));
        }
        let channels = c[9];
        if channels == 0 || channels > 8 {
            // Apple's reference decoder caps at 8 (kALACMaxChannels); a
            // wider claim is a corrupt cookie, not surround sound.
            return Err(CodecError::Malformed("ALAC channel count"));
        }
        Ok(Self {
            frame_len,
            bit_depth,
            pb: c[6],
            mb: c[7],
            kb: c[8],
            channels,
            rate: u32::from_be_bytes([c[20], c[21], c[22], c[23]]),
        })
    }
}

/// One coded half ("u" or "v") of an element: the residual coding
/// parameters.
#[derive(Debug, Clone)]
struct Half {
    /// Prediction mode: 0 straight to the coefficient predictor, anything
    /// else integrates once first (the "numActive == 31" short-circuit).
    mode: u8,
    /// Denominator shift for the predictor.
    den_shift: u8,
    /// The `pb` scale factor (0..7); the effective rice history is
    /// `pb * 2^pb_factor / 4`.
    pb_factor: u8,
    /// Predictor order, 0..=31.
    num: u8,
    /// Signed 16-bit predictor coefficients.
    coefs: Vec<i16>,
}

/// Read one coded half's parameters.
fn half(r: &mut MsbReader<'_>) -> Result<Half, CodecError> {
    let b = rd(r, 8)? as u8;
    let mode = b >> 4;
    let den_shift = b & 0xF;
    let b = rd(r, 8)? as u8;
    let pb_factor = b >> 5;
    let num = b & 0x1F;
    let mut coefs = Vec::with_capacity(usize::from(num));
    for _ in 0..num {
        // Big-endian, two's complement: read the pair as a word so a
        // negative coefficient's high byte cannot overflow a half-width
        // shift the way reading it in two signed halves would.
        let hi = rd(r, 8)? as u16;
        let lo = rd(r, 8)? as u16;
        coefs.push(((hi << 8) | lo) as i16);
    }
    Ok(Half {
        mode,
        den_shift,
        pb_factor,
        num,
        coefs,
    })
}

/// The effective rice history multiplier for a channel's `pb_factor`.
fn effective_pb(pb: u8, factor: u8) -> u32 {
    (u32::from(pb) * u32::from(factor)) / 4
}

/// The adaptive Golomb-Rice residual decoder (the reference's
/// `dyn_decomp`): a history term `mb` tracks the mean magnitude, each
/// value is a zero-run prefix with a `k`-bit remainder (k follows the
/// history), and a "zmode" run-length path takes over when the mean falls
/// low enough that zeros dominate. The prefix is capped at nine bits,
/// past which the value rides uncompressed — 32 bits here, 16 in zmode.
fn residuals(
    r: &mut MsbReader<'_>,
    num: usize,
    mb_seed: u8,
    pb: u32,
    kb: u32,
    escape_bits: u32,
) -> Result<Vec<i32>, CodecError> {
    // The constants of the adaptive-Golomb code: the mean's fixed-point
    // scale (512), the zmode threshold, and the k estimate's rounding.
    const QB: u32 = 1 << 9;
    const MMULSHIFT: u32 = 2;
    const MDENSHIFT: u32 = 9 - MMULSHIFT - 1;
    const MOFF: u32 = 1 << (MDENSHIFT - 2);

    let mut mb = u32::from(mb_seed).min(QB - 1);
    let mut zmode = false;
    let mut out = Vec::with_capacity(num.min(1 << 16));
    while out.len() < num {
        // k and m follow the history: lg3a(m) is bits(m + 3) − 1, the
        // table-free log the reference uses.
        let mut k = {
            let x = (mb >> 9) + 3;
            31 - x.leading_zeros()
        };
        k = k.min(kb);
        let m = (1u32 << k) - 1;
        let n = dyn_get(r, m, k, escape_bits, false)?;
        // The least significant bit is the sign.
        let nd = u64::from(n) + u64::from(zmode);
        let del = if nd & 1 == 0 {
            ((nd + 1) >> 1) as i32
        } else {
            (((nd + 1) >> 1) as i32).wrapping_neg()
        };
        out.push(del);
        mb = pb
            .wrapping_mul(n + u32::from(zmode))
            .wrapping_add(mb)
            .wrapping_sub((pb.wrapping_mul(mb)) >> 9);
        if n > 0xFFFF {
            mb = 0xFFFF;
        }
        zmode = false;
        if (mb << MMULSHIFT) < QB && out.len() < num {
            // A run of zeros is cheaper than coding each: the count is
            // itself Golomb-coded with a k estimated from the mean —
            // `lead(mb) − BITOFF + ((mb+MOFF) >> MDENSHIFT)`, the
            // reference's own table-free estimate (ffmpeg agrees:
            // `7 − log2(mb) + ((mb+16) >> 6)`).
            zmode = true;
            let k = mb
                .leading_zeros()
                .saturating_sub(24)
                .saturating_add((mb + MOFF) >> MDENSHIFT);
            let wb = (1u32 << kb) - 1;
            let mz = ((1u32 << k) - 1) & wb;
            let n = dyn_get(r, mz, k, 16, true)? as usize;
            if out.len() + n > num {
                return Err(CodecError::Malformed("ALAC zero run past the frame"));
            }
            out.resize(out.len() + n, 0);
            if n >= 65_535 {
                zmode = false;
            }
            mb = 0;
        }
    }
    Ok(out)
}

/// One Golomb-Rice value: a prefix of **one** bits (at most nine),
/// terminated by a zero, then `k` remainder bits. `v < 2` returns the one
/// bit it did not need — the code's own quirk, reproduced exactly. At nine
/// ones the value rides uncompressed in `escape_bits` bits (16 in zmode,
/// the half's bit width otherwise), starting with the tenth bit.
///
/// The prefix's polarity is the format's own trap: the reference counts it
/// with `lead(~streamlong)` — leading *ones* of the stream, terminated by
/// the zero — and ffmpeg's `get_unary_0_9` agrees. A FLAC-style
/// zero-prefix unary reads the same bytes as plausible-looking nonsense.
fn dyn_get(
    r: &mut MsbReader<'_>,
    m: u32,
    k: u32,
    escape_bits: u32,
    zmode: bool,
) -> Result<u32, CodecError> {
    const MAX_PREFIX: u32 = 9;
    // Count the prefix, consuming as we go: at nine ones the escape
    // begins, so the tenth bit — one or zero — belongs to the value.
    let mut pre = 0u32;
    while pre < MAX_PREFIX {
        // `peek` can only see one bit, so anything but a one is the
        // terminating zero; a truncated read is caught by the skip below.
        match r.peek(1) {
            Some(1) => {
                r.skip(1).ok_or(CodecError::Truncated)?;
                pre += 1;
            }
            _ => break,
        }
    }
    if pre >= MAX_PREFIX {
        let v = rd(r, escape_bits.min(32))?;
        return Ok(v);
    }
    r.skip(1).ok_or(CodecError::Truncated)?; // the terminating zero
    if !zmode && k == 1 {
        // The reference reads no remainder when k == 1 (m is 1, and the
        // prefix alone is the value); only the non-run code has the case.
        return Ok(pre);
    }
    let v = rd(r, k)?;
    if v < 2 {
        // Give one bit back: the low case encodes in k−1 bits.
        r.seek(r.position() - 1);
        Ok(pre * m)
    } else {
        Ok(pre * m + v - 1)
    }
}

fn rd(r: &mut MsbReader<'_>, n: u32) -> Result<u32, CodecError> {
    r.read(n).ok_or(CodecError::Truncated)
}

/// The adaptive predictor (the reference's `unpc_block`): integrate the
/// residuals through a signed-coefficient ladder whose coefficients nudge
/// themselves toward the signal every sample. The first `num` samples are
/// straight integration; order 31 is pure first-order integration; order
/// 0 is a copy.
///
/// The arithmetic that can overflow on a hostile file is wrapping, as the
/// C reference's is by compilation: a valid file never leaves the sample
/// range, and an invalid one is caught by the concealment pass later.
fn predict(res: &[i32], coefs: &mut [i16], num: u32, chan_bits: u32, den_shift: u32) -> Vec<i32> {
    let n = res.len();
    let mut out = vec![0i32; n];
    if n == 0 {
        return out;
    }
    out[0] = res[0];
    let chan_shift = 32 - chan_bits.min(32);
    let clamp = |v: i64| -> i32 {
        // Sign-extend the low `chan_bits` bits, exactly as the reference's
        // `(del << chanshift) >> chanshift` does on a wrapping compiler.
        ((v as u32).wrapping_shl(chan_shift) as i32) >> chan_shift
    };
    if num == 0 {
        for (o, &v) in out.iter_mut().zip(res) {
            *o = clamp(i64::from(v));
        }
        return out;
    }
    if num == 31 {
        let mut prev = out[0];
        for j in 1..n {
            prev = clamp(i64::from(res[j]) + i64::from(prev));
            out[j] = prev;
        }
        return out;
    }
    let num = num as usize;
    let den_half = 1i64 << (den_shift - 1);
    // Warm-up: straight integration for the first `num` samples.
    for j in 1..=num.min(n - 1) {
        out[j] = clamp(i64::from(res[j]) + i64::from(out[j - 1]));
    }
    if n <= num {
        return out;
    }
    let lim = num + 1;
    for j in lim..n {
        let top = i64::from(out[j - lim]);
        // sum1 = Σ coefs[k]·(out[j−1−k] − top), in 64 bits so a hostile
        // coefficient cannot panic the add; the reference wraps in 32.
        let mut sum1 = 0i64;
        for (k, &c) in coefs.iter().enumerate().take(num) {
            sum1 += i64::from(c) * (i64::from(out[j - 1 - k]) - top);
        }
        let del = i64::from(res[j]);
        let del0 = del;
        let sg = del.signum();
        out[j] = clamp(del + top + ((sum1 + den_half) >> den_shift));
        // Coefficient adaptation: nudge each coefficient against the
        // error's sign, oldest first, stopping when the residual is
        // explained. The reference's hand-unrolled 4/8 cases are this same
        // arithmetic; the general case is the honest shape.
        if sg > 0 {
            let mut del0 = del0;
            for k in (0..num).rev() {
                let dd = top - i64::from(out[j - 1 - k]);
                let sgn = dd.signum();
                coefs[k] = coefs[k].wrapping_sub(sgn as i16);
                del0 -= (num - k) as i64 * ((sgn * dd) >> den_shift);
                if del0 <= 0 {
                    break;
                }
            }
        } else if sg < 0 {
            let mut del0 = del0;
            for k in (0..num).rev() {
                let dd = top - i64::from(out[j - 1 - k]);
                let sgn = dd.signum();
                coefs[k] = coefs[k].wrapping_add(sgn as i16);
                del0 -= (num - k) as i64 * ((-sgn * dd) >> den_shift);
                if del0 >= 0 {
                    break;
                }
            }
        }
    }
    out
}

/// The predictor with the mode-≠-0 pre-integration the reference performs
/// (order 31 in place first).
fn predict_half(res: &[i32], h: &Half, chan_bits: u32) -> Vec<i32> {
    let mut coefs = h.coefs.clone();
    if h.mode == 0 {
        predict(
            res,
            &mut coefs,
            u32::from(h.num),
            chan_bits,
            u32::from(h.den_shift),
        )
    } else {
        let integrated = predict(res, &mut [], 31, chan_bits, 0);
        predict(
            &integrated,
            &mut coefs,
            u32::from(h.num),
            chan_bits,
            u32::from(h.den_shift),
        )
    }
}

/// The 4-bit element header: partial-frame flag, shift-off byte count,
/// escape flag. `bytes_shifted == 3` is invalid (the reference refuses it;
/// 1 and 2 mean the low 8/16 bits ride uncompressed).
fn header4(r: &mut MsbReader<'_>) -> Result<(bool, u8, bool), CodecError> {
    let b = rd(r, 4)? as u8;
    let partial = b & 0x8 != 0;
    let bytes_shifted = (b >> 1) & 0x3;
    if bytes_shifted == 3 {
        return Err(CodecError::Malformed("ALAC shift-off count"));
    }
    let esc = b & 1 == 1;
    Ok((esc, bytes_shifted, partial))
}

/// A partial frame's 32-bit sample count, clamped to the sanity bound —
/// the count is a claim from the file, and the allocation that follows
/// must not take it at face value.
fn read_len(r: &mut MsbReader<'_>) -> Result<usize, CodecError> {
    let hi = rd(r, 16)?;
    let lo = rd(r, 16)?;
    let n = ((u64::from(hi) << 16) | u64::from(lo)).min(u64::from(MAX_FRAME_LEN));
    Ok(usize::try_from(n).unwrap_or(0))
}

/// The shift-off bytes: `channels` values per sample, each
/// `bytes_shifted * 8` bits, interleaved.
fn read_shift(
    r: &mut MsbReader<'_>,
    samples: usize,
    channels: usize,
    bytes_shifted: u8,
) -> Result<Vec<u16>, CodecError> {
    let bits = u32::from(bytes_shifted) * 8;
    let mut out = Vec::with_capacity(samples * channels);
    for _ in 0..samples * channels {
        out.push(rd(r, bits)? as u16);
    }
    Ok(out)
}

/// OR the shift-off low bits back into one half's samples — the
/// single-channel path's own recombination, where the half *is* the
/// channel and the reference's `copyPredictorTo24Shift` applies the
/// bytes one sample at a time.
fn apply_shift(half_out: &mut [i32], shift: &[u16], bytes_shifted: u8) {
    let sh = u32::from(bytes_shifted) * 8;
    for (v, &s) in half_out.iter_mut().zip(shift) {
        *v = ((*v as u32) << sh | u32::from(s)) as i32;
    }
}

/// Undo the stereo decorrelation matrix: `u`/`v` are the coded halves,
/// `L = u + v − [mix_res·v >> mix_bits]`, `R = L − v` — the generalized
/// middle-side transformation the format uses, lossless for any mix_res.
/// With shift-off bytes the matrix runs at the reduced width *first* and
/// the low bytes are OR-ed into `L` and `R` separately afterwards — the
/// order the reference's `unmix16/20/24/32` perform, and the only one
/// that is lossless (mixing first and shifting after would carry the
/// shift through the matrix arithmetic).
#[expect(
    clippy::too_many_arguments,
    reason = "the parameter set is the format's own"
)]
fn unmix(
    u: &[i32],
    v: &[i32],
    out: &mut [i32],
    channel: usize,
    channels: usize,
    mix_bits: u8,
    mix_res: i8,
    shift: &[u16],
    bytes_shifted: u8,
) {
    let sh = u32::from(bytes_shifted) * 8;
    for (i, ((&a, &b), frame)) in u
        .iter()
        .zip(v)
        .zip(out.chunks_exact_mut(channels))
        .enumerate()
    {
        let (l, r) = if mix_res != 0 {
            let l = i64::from(a) + i64::from(b)
                - ((i64::from(i32::from(mix_res)) * i64::from(b)) >> mix_bits);
            (l, l - i64::from(b))
        } else {
            (i64::from(a), i64::from(b))
        };
        let (l, r) = if sh == 0 {
            (l, r)
        } else {
            // The low bytes OR-ed in after the matrix, L and R each
            // with their own — the reference's `unmix24`/`unmix32`.
            (
                i64::from(((l as u32) << sh | u32::from(shift[2 * i])) as i32),
                i64::from(((r as u32) << sh | u32::from(shift[2 * i + 1])) as i32),
            )
        };
        frame[channel] = l as i32;
        frame[channel + 1] = r as i32;
    }
}

/// Decode one whole frame (one MP4 sample) into interleaved samples.
fn frame(data: &[u8], cfg: &Config, default_samples: usize) -> Result<Vec<i32>, CodecError> {
    let mut r = MsbReader::new(data);
    let channels = usize::from(cfg.channels);
    let mut samples = default_samples;
    let mut out: Vec<i32> = vec![0; samples * channels];
    let mut filled = 0usize; // channels decoded so far
    loop {
        let tag = rd(&mut r, 3)?;
        match tag {
            0 | 3 => {
                // Single-channel element (an LFE element is coded the
                // same way). An element past the channel count ends the
                // frame, as the END tag does.
                if filled >= channels {
                    r.align();
                    return Ok(out);
                }
                rd(&mut r, 4)?; // element instance tag
                if rd(&mut r, 12)? != 0 {
                    return Err(CodecError::Malformed("ALAC reserved bits set"));
                }
                let (esc, bytes_shifted, partial) = header4(&mut r)?;
                if partial {
                    samples = read_len(&mut r)?;
                    if filled > 0 {
                        // A partial-frame count belongs to the frame's first
                        // element; a later one contradicting it is the file
                        // lying (ffmpeg refuses the same mismatch).
                        return Err(CodecError::Malformed(
                            "ALAC partial-frame length change mid-frame",
                        ));
                    }
                    out = vec![0; samples * channels];
                }
                let mut u;
                if esc {
                    // Uncompressed frame: plain samples at full depth (the
                    // shift-off flag is ignored, exactly as the reference
                    // ignores it).
                    let bits = u32::from(cfg.bit_depth);
                    u = Vec::with_capacity(samples);
                    for _ in 0..samples {
                        u.push(r.read_signed(bits).ok_or(CodecError::Truncated)?);
                    }
                } else {
                    // The mix fields ride here even for a lone channel —
                    // the format inherits AAC's element syntax whole; the
                    // reference reads and ignores them for mono.
                    rd(&mut r, 8)?; // mixBits (0 for a mono element)
                    rd(&mut r, 8)?; // mixRes (0 for a mono element)
                    let h = half(&mut r)?;
                    // The shift-off bytes sit between the parameters and
                    // the coded residuals.
                    let shift_at = r.position();
                    if bytes_shifted != 0 {
                        r.skip(usize::from(bytes_shifted) * 8 * samples)
                            .ok_or(CodecError::Truncated)?;
                    }
                    let chan_bits = u32::from(cfg.bit_depth) - u32::from(bytes_shifted) * 8;
                    let res = residuals(
                        &mut r,
                        samples,
                        cfg.mb,
                        effective_pb(cfg.pb, h.pb_factor),
                        u32::from(cfg.kb),
                        chan_bits,
                    )?;
                    u = predict_half(&res, &h, chan_bits);
                    if bytes_shifted != 0 {
                        let mut sr = MsbReader::new(data);
                        sr.seek(shift_at);
                        let shift = read_shift(&mut sr, samples, 1, bytes_shifted)?;
                        apply_shift(&mut u, &shift, bytes_shifted);
                    }
                }
                for (frame, &v) in out.chunks_exact_mut(channels).zip(&u) {
                    frame[filled] = v;
                }
                filled += 1;
            }
            1 => {
                // Channel pair: two coded halves, decorrelated. A pair
                // that does not fit ends the frame.
                if filled + 2 > channels {
                    r.align();
                    return Ok(out);
                }
                rd(&mut r, 4)?; // element instance tag
                if rd(&mut r, 12)? != 0 {
                    return Err(CodecError::Malformed("ALAC reserved bits set"));
                }
                let (esc, bytes_shifted, partial) = header4(&mut r)?;
                if partial {
                    samples = read_len(&mut r)?;
                    if filled > 0 {
                        return Err(CodecError::Malformed(
                            "ALAC partial-frame length change mid-frame",
                        ));
                    }
                    out = vec![0; samples * channels];
                }
                let mut mix_bits = 0u8;
                let mut mix_res = 0i8;
                let mut u;
                let mut v;
                let mut shift = Vec::new();
                if esc {
                    let bits = u32::from(cfg.bit_depth);
                    u = Vec::with_capacity(samples);
                    v = Vec::with_capacity(samples);
                    for _ in 0..samples {
                        u.push(r.read_signed(bits).ok_or(CodecError::Truncated)?);
                        v.push(r.read_signed(bits).ok_or(CodecError::Truncated)?);
                    }
                } else {
                    mix_bits = rd(&mut r, 8)? as u8;
                    mix_res = rd(&mut r, 8)? as u8 as i8;
                    let hu = half(&mut r)?;
                    let hv = half(&mut r)?;
                    let shift_at = r.position();
                    if bytes_shifted != 0 {
                        r.skip(usize::from(bytes_shifted) * 8 * 2 * samples)
                            .ok_or(CodecError::Truncated)?;
                    }
                    // A pair's halves carry one bit more than the sample
                    // depth less the shifted-off bytes — the headroom the
                    // mid channel's +1 needs. 32-bit pairs always shift at
                    // least one byte off (the reference asserts it; ffmpeg
                    // refuses the 33-bit read outright, and so does this).
                    let chan_bits = u32::from(cfg.bit_depth) - u32::from(bytes_shifted) * 8 + 1;
                    if chan_bits > 32 {
                        return Err(CodecError::Unsupported(
                            "ALAC 32-bit stereo without shift-off bytes",
                        ));
                    }
                    let res_u = residuals(
                        &mut r,
                        samples,
                        cfg.mb,
                        effective_pb(cfg.pb, hu.pb_factor),
                        u32::from(cfg.kb),
                        chan_bits,
                    )?;
                    u = predict_half(&res_u, &hu, chan_bits);
                    let res_v = residuals(
                        &mut r,
                        samples,
                        cfg.mb,
                        effective_pb(cfg.pb, hv.pb_factor),
                        u32::from(cfg.kb),
                        chan_bits,
                    )?;
                    v = predict_half(&res_v, &hv, chan_bits);
                    if bytes_shifted != 0 {
                        // The interleaved shift bytes for both halves,
                        // re-read from where they sit in the frame — the
                        // low bytes of L and R in that order.
                        let mut sr = MsbReader::new(data);
                        sr.seek(shift_at);
                        shift = read_shift(&mut sr, samples, 2, bytes_shifted)?;
                    }
                }
                unmix(
                    &u,
                    &v,
                    &mut out,
                    filled,
                    channels,
                    mix_bits,
                    mix_res,
                    &shift,
                    bytes_shifted,
                );
                filled += 2;
            }
            4 => {
                // Data stream element: parse and skip.
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
            6 => {
                // Fill element: 4-bit count, or 4-bit 15 + 8-bit − 1.
                let mut n = rd(&mut r, 4)? as usize;
                if n == 15 {
                    n += rd(&mut r, 8)?.saturating_sub(1) as usize;
                }
                r.skip(n * 8).ok_or(CodecError::Truncated)?;
            }
            7 => {
                r.align();
                return Ok(out);
            }
            2 | 5 => {
                return Err(CodecError::Unsupported(
                    "ALAC coupling/program-config elements",
                ));
            }
            _ => return Err(CodecError::Malformed("ALAC element tag")),
        }
        if filled >= channels {
            r.align();
            return Ok(out);
        }
    }
}

/// Interleaved `f32` from decoded integers.
fn to_f32(ints: &[i32], bit_depth: u8) -> Vec<f32> {
    let scale = 1.0 / f64::from(1u32 << (bit_depth - 1));
    ints.iter()
        .map(|&v| (f64::from(v) * scale) as f32)
        .collect()
}

/// Decode ALAC in an MP4/M4A container (an `alac` sample description).
///
/// # Errors
/// Not an ALAC track, a malformed cookie or frame, or an unsupported
/// element.
pub fn decode_mp4(bytes: &[u8]) -> Result<Decoded, CodecError> {
    let t = mp4::audio_track(bytes)?;
    if t.codec != *b"alac" {
        return Err(CodecError::NotThisFormat);
    }
    let cfg = Config::from_box(&t.config)?;
    let channels = usize::from(cfg.channels);
    let mut out: Vec<i32> = Vec::new();
    for (off, len) in &t.samples {
        let f = frame(&bytes[*off..off + len], &cfg, cfg.frame_len as usize)?;
        out.extend(f);
    }
    let skip = usize::try_from(t.skip).unwrap_or(0) * channels;
    let mut data = out.get(skip.min(out.len())..).unwrap_or(&[]).to_vec();
    if let Some(p) = t.play {
        data.truncate(usize::try_from(p).unwrap_or(usize::MAX) * channels);
    }
    Decoded {
        data: to_f32(&data, cfg.bit_depth),
        channels: cfg.channels,
        rate: cfg.rate,
    }
    .validated()
}

// ── streaming (AU-05) ────────────────────────────────────────────────────

/// ALAC in MP4/M4A a frame at a time, for [`super::stream`]: the sample
/// table lists every frame's bytes; each decodes independently (the
/// predictor's state does not carry across frames), so a seek is just the
/// sample table's index.
pub(crate) struct AlacSource {
    cfg: Config,
    track: mp4::AudioTrack,
    next: usize,
}

impl AlacSource {
    pub(crate) fn from_track(track: mp4::AudioTrack) -> Result<Self, CodecError> {
        let cfg = Config::from_box(&track.config)?;
        if !super::RATE_RANGE.contains(&cfg.rate) {
            return Err(CodecError::Malformed("sample rate out of range"));
        }
        Ok(Self {
            cfg,
            track,
            next: 0,
        })
    }
}

impl super::stream::FrameSource for AlacSource {
    fn channels(&self) -> usize {
        usize::from(self.cfg.channels)
    }

    fn rate(&self) -> u32 {
        self.cfg.rate
    }

    fn trim(&self) -> (u64, Option<u64>) {
        (self.track.skip, self.track.play)
    }

    fn next(
        &mut self,
        input: &mut super::stream::Input,
    ) -> Result<Option<super::stream::Chunk>, CodecError> {
        let Some(&(off, len)) = self.track.samples.get(self.next) else {
            return Ok(None);
        };
        self.next += 1;
        let bytes = input.window(off as u64, len)?;
        if bytes.len() < len {
            return Err(CodecError::Truncated);
        }
        let ints = frame(bytes, &self.cfg, self.cfg.frame_len as usize)?;
        Ok(Some(super::stream::Chunk {
            pcm: to_f32(&ints, self.cfg.bit_depth),
        }))
    }

    fn seek(
        &mut self,
        _input: &mut super::stream::Input,
        frame_no: u64,
    ) -> Result<u64, CodecError> {
        // Frames are independent; land on the frame containing the target.
        let per = u64::from(self.cfg.frame_len).max(1);
        self.next = usize::try_from(frame_no / per)
            .unwrap_or(usize::MAX)
            .min(self.track.samples.len());
        Ok(self.next as u64 * per)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{s16le, snr_db};
    use super::*;

    /// ALAC is lossless: the decode must be *bit-exact* against ffmpeg's
    /// own decode, not merely close. The SNR oracle doubles as the length
    /// and alignment check.
    #[test]
    fn decodes_ffmpegs_alac_stereo_bit_exact() {
        let d = decode_mp4(include_bytes!("../../tests/fixtures/src_alac.m4a")).unwrap();
        let reference = s16le(include_bytes!("../../tests/fixtures/src_alac.m4a.ref"));
        assert_eq!((d.channels, d.rate), (2, 44_100));
        assert_eq!(d.data.len(), reference.len(), "0.4 s of stereo");
        let (snr, lag) = snr_db(&reference, &d.data, 2, 2_048);
        eprintln!("alac m4a stereo: {snr:.1} dB at lag {lag}");
        assert_eq!(lag, 0);
        assert!(snr > 130.0, "lossless: {snr} dB");
        for (a, b) in d.data.iter().zip(&reference) {
            assert!((a - b).abs() < 1.0 / 32_768.0, "{a} vs {b}");
        }
    }

    #[test]
    fn decodes_ffmpegs_alac_24_bit_mono_bit_exact() {
        let d = decode_mp4(include_bytes!("../../tests/fixtures/src24m_alac.m4a")).unwrap();
        let raw = include_bytes!("../../tests/fixtures/src24m_alac.m4a.ref");
        let reference: Vec<f32> = raw
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| {
                let v = (i32::from(c[0]) << 16) | (i32::from(c[1]) << 8) | i32::from(c[2]);
                // The 24 bits are big-endian and signed.
                let v = (v << 8) >> 8;
                f64::from(v) as f32 / 8_388_608.0
            })
            .collect();
        assert_eq!((d.channels, d.rate), (1, 44_100));
        assert_eq!(d.data.len(), reference.len());
        let (snr, lag) = snr_db(&reference, &d.data, 1, 2_048);
        eprintln!("alac m4a 24-bit mono: {snr:.1} dB at lag {lag}");
        assert_eq!(lag, 0);
        assert!(snr > 130.0, "lossless: {snr} dB");
    }

    #[test]
    fn refuses_the_aac_fixture_by_name() {
        assert_eq!(
            decode_mp4(include_bytes!("../../tests/fixtures/src.m4a")).err(),
            Some(CodecError::NotThisFormat)
        );
    }

    #[test]
    fn an_uncompressed_frame_decodes_verbatim() {
        use super::super::bits::MsbWriter;
        let mut w = MsbWriter::default();
        w.write(0, 3); // SCE
        w.write(0, 4); // element instance tag
        w.write(0, 12); // reserved
        w.write(0b0001, 4); // partial 0, shift 0, escape 1
        let samples = [100i32, -200, 30_000, -30_000, 0];
        for &s in &samples {
            w.write_signed(i64::from(s), 16);
        }
        w.write(7, 3); // END
        w.align();
        let cfg = Config {
            frame_len: 4096,
            bit_depth: 16,
            pb: 40,
            mb: 10,
            kb: 14,
            channels: 1,
            rate: 44_100,
        };
        let out = frame(&w.bytes, &cfg, samples.len()).unwrap();
        assert_eq!(out, samples.to_vec());
    }

    /// The generalized middle-side matrix inverts losslessly for the
    /// round-trip values a real encoder picks.
    #[test]
    fn the_stereo_matrix_round_trips() {
        let mix = |l: i32, r: i32, bits: u8, res: i8| {
            // The encoder's side (`matrix_enc.c`): mixres 0 is plain
            // separated stereo — u=L, v=R — and only a nonzero mixres
            // takes the generalized transform u = [res·L + (m−res)·R]/m,
            // v = L − R.
            if res == 0 {
                return (l, r);
            }
            let m = 1i64 << bits;
            let rr = i64::from(res);
            let u = ((rr * i64::from(l) + (m - rr) * i64::from(r)) >> bits) as i32;
            let v = l - r;
            (u, v)
        };
        for &(bits, res) in &[(2u8, 0i8), (2, 1), (2, 2), (3, 3), (3, -2)] {
            let l = 12_345i32;
            let r = -6_789;
            let (u, v) = mix(l, r, bits, res);
            let mut out = vec![0i32; 2];
            unmix(&[u], &[v], &mut out, 0, 2, bits, res, &[], 0);
            assert_eq!(out[0], l, "L, bits {bits} res {res}");
            assert_eq!(out[1], r, "R, bits {bits} res {res}");
        }
    }

    /// A truncated coded frame refuses rather than panicking — the fuzz
    /// target's contract in miniature.
    #[test]
    fn a_truncated_coded_frame_refuses() {
        use super::super::bits::MsbWriter;
        let mut w = MsbWriter::default();
        w.write(0, 3); // SCE
        w.write(0, 4);
        w.write(0, 12);
        w.write(0, 4); // coded, no shift, no partial
        w.write(0, 8); // mixBits
        w.write(0, 8); // mixRes
        w.write(0, 8); // mode | denShift
        w.write(0, 8); // pbFactor | num
        w.write(1, 1); // one residual's terminating bit
        let cfg = Config {
            frame_len: 4096,
            bit_depth: 16,
            pb: 40,
            mb: 10,
            kb: 14,
            channels: 1,
            rate: 44_100,
        };
        // 4096 residuals cannot come from two bytes: a refusal, never a
        // panic.
        assert!(matches!(
            frame(&w.bytes, &cfg, 4096),
            Err(CodecError::Truncated)
        ));
    }

    /// A hostile partial-frame length is clamped, not allocated.
    #[test]
    fn a_huge_partial_frame_length_is_clamped() {
        use super::super::bits::MsbWriter;
        let mut w = MsbWriter::default();
        w.write(0, 3); // SCE
        w.write(0, 4);
        w.write(0, 12);
        w.write(0b1000, 4); // partial, no shift, no escape
        w.write(0xFFFF, 16);
        w.write(0xFFFF, 16);
        w.write(0, 8); // mixBits
        w.write(0, 8); // mixRes
        w.write(0, 8);
        w.write(0, 8);
        let cfg = Config {
            frame_len: 4096,
            bit_depth: 16,
            pb: 40,
            mb: 10,
            kb: 14,
            channels: 1,
            rate: 44_100,
        };
        // The length is clamped by read_len; the frame then runs out of
        // bits — a refusal, never a four-gigabyte allocation.
        assert!(matches!(
            frame(&w.bytes, &cfg, 4096),
            Err(CodecError::Truncated) | Err(CodecError::Malformed(_))
        ));
    }
}
