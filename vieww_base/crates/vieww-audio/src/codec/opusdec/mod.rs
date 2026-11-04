//! A pure-Rust Opus decoder (RFC 6716), ported from libopus 1.3.1 — the
//! RFC's own reference implementation — and graded against it: SILK (the LP layer,
//! fixed-point exactly as libopus runs it), CELT (the MDCT layer, the
//! float build) and the Opus layer that switches, overlaps and
//! cross-fades between them, plus the multistream layer for channel
//! mapping families 1, 2 and 255 (the ones `opus_multistream_decoder`
//! serves; family 3's projection matrix is refused at the header).
//!
//! Output is 48 kHz float, as `opus_decode_float` returns it (no soft
//! clipping). Packet loss is not a container-level event here, but the
//! concealment paths still run: libopus synthesises the 2.5 ms
//! cross-fades at SILK↔CELT mode switches by concealing the previous
//! mode's next frame, and the port does the same.
//!
//! Derived from libopus, Copyright 2001-2011 Xiph.Org, Skype Limited,
//! Octasic, Jean-Marc Valin, Timothy B. Terriberry, CSIRO, Gregory
//! Maxwell, Mark Borgerding, Erik de Castro Lopo; BSD-3-Clause — the full
//! notice is tools/LICENSE-libopus.txt (see tools/THIRD_PARTY.md).

// A port that keeps libopus's expression order and literal constants on
// purpose (the float output is graded against libopus to the rounding,
// so "simpler" rewrites of its arithmetic are not neutral); these style
// lints are allowed for the whole decoder, its submodules included.
#![allow(
    clippy::needless_range_loop,
    clippy::too_many_lines,
    clippy::precedence,
    clippy::approx_constant,
    clippy::excessive_precision,
    clippy::assign_op_pattern,
    clippy::explicit_counter_loop,
    clippy::int_plus_one,
    clippy::manual_is_multiple_of,
    clippy::needless_late_init,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::chunks_exact_to_as_chunks
)]

pub(crate) mod celt;
pub(crate) mod range;
pub(crate) mod silk;

use range::RangeDecoder;

pub(crate) const MODE_SILK: i32 = 1000;
pub(crate) const MODE_HYBRID: i32 = 1001;
pub(crate) const MODE_CELT: i32 = 1002;

pub(crate) const BW_NB: i32 = 1101;
pub(crate) const BW_MB: i32 = 1102;
pub(crate) const BW_WB: i32 = 1103;
pub(crate) const BW_SWB: i32 = 1104;
pub(crate) const BW_FB: i32 = 1105;

const FS: usize = 48_000;
const F20: usize = FS / 50;
const F10: usize = F20 >> 1;
const F5: usize = F10 >> 1;
const F2_5: usize = F5 >> 1;
/// The most samples one packet can hold (120 ms).
pub(crate) const MAX_FRAME: usize = 5760;

/// Why a packet could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpusError {
    BadArg,
    BufferTooSmall,
    InternalError,
    InvalidPacket,
}

/// One Opus stream's decoder (1 or 2 output channels).
pub(crate) struct OpusDecoder {
    channels: usize,
    celt: celt::Celt,
    silk: silk::SilkDecoder,
    stream_channels: usize,
    bandwidth: i32,
    mode: i32,
    prev_mode: i32,
    frame_size: usize,
    prev_redundancy: bool,
    pub(crate) last_packet_duration: usize,
    decode_gain: i32,
    pub(crate) range_final: u32,
}

fn smooth_fade(in1: &[f32], in2: &[f32], out: &mut [f32], overlap: usize, channels: usize) {
    let w = &celt::tables::WINDOW;
    for c in 0..channels {
        for i in 0..overlap {
            let wv = w[i] * w[i];
            out[i * channels + c] = wv * in2[i * channels + c] + (1.0 - wv) * in1[i * channels + c];
        }
    }
}

const fn packet_mode(toc: u8) -> i32 {
    if toc & 0x80 != 0 {
        MODE_CELT
    } else if toc & 0x60 == 0x60 {
        MODE_HYBRID
    } else {
        MODE_SILK
    }
}

const fn packet_bandwidth(toc: u8) -> i32 {
    if toc & 0x80 != 0 {
        let bw = BW_MB + (((toc >> 5) & 0x3) as i32);
        if bw == BW_MB {
            BW_NB
        } else {
            bw
        }
    } else if toc & 0x60 == 0x60 {
        if toc & 0x10 != 0 {
            BW_FB
        } else {
            BW_SWB
        }
    } else {
        BW_NB + (((toc >> 5) & 0x3) as i32)
    }
}

pub(crate) const fn samples_per_frame(toc: u8) -> usize {
    if toc & 0x80 != 0 {
        (FS << ((toc >> 3) & 0x3)) / 400
    } else if toc & 0x60 == 0x60 {
        if toc & 0x08 != 0 {
            FS / 50
        } else {
            FS / 100
        }
    } else {
        let a = (toc >> 3) & 0x3;
        if a == 3 {
            FS * 60 / 1000
        } else {
            (FS << a) / 100
        }
    }
}

fn parse_size(data: &[u8]) -> Option<(usize, usize)> {
    match data {
        [] => None,
        [a, ..] if *a < 252 => Some((usize::from(*a), 1)),
        [_] => None,
        [a, b, ..] => Some((4 * usize::from(*b) + usize::from(*a), 2)),
    }
}

/// A parsed packet: each frame's byte range, and the whole packet's
/// length including padding (what a self-delimited packet occupies).
pub(crate) struct Parsed {
    pub(crate) frames: Vec<(usize, usize)>,
    pub(crate) packet_len: usize,
}

/// `opus_packet_parse_impl`, both framings (RFC 6716 §3.2, Appendix B).
pub(crate) fn parse_packet(data: &[u8], self_delimited: bool) -> Result<Parsed, OpusError> {
    let bad = Err(OpusError::InvalidPacket);
    if data.is_empty() {
        return bad;
    }
    let framesize = samples_per_frame(data[0]);
    let toc = data[0];
    let mut at = 1usize;
    let mut len = data.len() as isize - 1;
    let mut last_size = len;
    let mut pad = 0usize;
    let mut cbr = false;
    let mut size = [0isize; 48];
    let count: usize;
    let sz = |at: usize, len: isize| parse_size(&data[at..at + len.max(0) as usize]);
    match toc & 0x3 {
        0 => count = 1,
        1 => {
            count = 2;
            cbr = true;
            if !self_delimited {
                if len & 1 != 0 {
                    return bad;
                }
                last_size = len / 2;
                size[0] = last_size;
            }
        }
        2 => {
            count = 2;
            let Some((s0, bytes)) = sz(at, len) else {
                return bad;
            };
            len -= bytes as isize;
            if s0 as isize > len {
                return bad;
            }
            size[0] = s0 as isize;
            at += bytes;
            last_size = len - size[0];
        }
        _ => {
            if len < 1 {
                return bad;
            }
            let ch = data[at];
            at += 1;
            count = usize::from(ch & 0x3F);
            if count == 0 || framesize * count > MAX_FRAME {
                return bad;
            }
            len -= 1;
            if ch & 0x40 != 0 {
                loop {
                    if len <= 0 {
                        return bad;
                    }
                    let p = data[at];
                    at += 1;
                    len -= 1;
                    let tmp = if p == 255 { 254 } else { usize::from(p) };
                    len -= tmp as isize;
                    pad += tmp;
                    if p != 255 {
                        break;
                    }
                }
            }
            if len < 0 {
                return bad;
            }
            cbr = ch & 0x80 == 0;
            if !cbr {
                last_size = len;
                for i in 0..count - 1 {
                    let Some((si, bytes)) = sz(at, len) else {
                        return bad;
                    };
                    len -= bytes as isize;
                    if si as isize > len {
                        return bad;
                    }
                    size[i] = si as isize;
                    at += bytes;
                    last_size -= (bytes + si) as isize;
                }
                if last_size < 0 {
                    return bad;
                }
            } else if !self_delimited {
                last_size = len / count as isize;
                if last_size * count as isize != len {
                    return bad;
                }
                for v in size.iter_mut().take(count - 1) {
                    *v = last_size;
                }
            }
        }
    }
    if self_delimited {
        let Some((sl, bytes)) = sz(at, len) else {
            return bad;
        };
        len -= bytes as isize;
        if sl as isize > len {
            return bad;
        }
        size[count - 1] = sl as isize;
        at += bytes;
        if cbr {
            if (sl * count) as isize > len {
                return bad;
            }
            for v in size.iter_mut().take(count - 1) {
                *v = sl as isize;
            }
        } else if (bytes + sl) as isize > last_size {
            return bad;
        }
    } else {
        if last_size > 1275 {
            return bad;
        }
        size[count - 1] = last_size;
    }
    let mut frames = Vec::with_capacity(count);
    for &s in &size[..count] {
        let s = s as usize;
        if at + s > data.len() {
            return bad;
        }
        frames.push((at, s));
        at += s;
    }
    Ok(Parsed {
        frames,
        packet_len: pad + at,
    })
}

impl OpusDecoder {
    pub(crate) fn new(channels: usize) -> Self {
        let mut celt = celt::Celt::new(channels);
        celt.reset();
        Self {
            channels,
            celt,
            silk: silk::SilkDecoder::new(channels),
            stream_channels: channels,
            bandwidth: 0,
            mode: 0,
            prev_mode: 0,
            frame_size: FS / 400,
            prev_redundancy: false,
            last_packet_duration: 0,
            decode_gain: 0,
            range_final: 0,
        }
    }

    /// `OPUS_SET_GAIN` (Q8 dB) — the Ogg header's output gain.
    pub(crate) fn set_gain(&mut self, q8: i32) {
        self.decode_gain = q8;
    }

    fn decode_frame(
        &mut self,
        data: Option<&[u8]>,
        pcm: &mut [f32],
        frame_size: usize,
        decode_fec: bool,
    ) -> Result<usize, OpusError> {
        let ch = self.channels;
        if frame_size < F2_5 {
            return Err(OpusError::BufferTooSmall);
        }
        let mut frame_size = frame_size.min(FS / 25 * 3);
        let data = match data {
            Some(d) if d.len() > 1 => Some(d),
            _ => {
                frame_size = frame_size.min(self.frame_size);
                None
            }
        };
        let mut len = data.map_or(0, <[u8]>::len);
        let (mut audiosize, mode, bandwidth);
        let mut own_dec;
        if let Some(d) = data {
            audiosize = self.frame_size;
            mode = self.mode;
            bandwidth = self.bandwidth;
            own_dec = Some(RangeDecoder::new(d));
        } else {
            own_dec = None;
            audiosize = frame_size;
            mode = self.prev_mode;
            bandwidth = 0;
            if mode == 0 {
                for v in pcm.iter_mut().take(audiosize * ch) {
                    *v = 0.0;
                }
                return Ok(audiosize);
            }
            if audiosize > F20 {
                let mut done = 0;
                while audiosize > 0 {
                    let r =
                        self.decode_frame(None, &mut pcm[done * ch..], audiosize.min(F20), false)?;
                    done += r;
                    audiosize -= r;
                }
                return Ok(frame_size);
            } else if audiosize < F20 {
                if audiosize > F10 {
                    audiosize = F10;
                } else if mode != MODE_SILK && audiosize > F5 && audiosize < F10 {
                    audiosize = F5;
                }
            }
        }
        let mut transition = false;
        let mut pcm_transition: Vec<f32> = Vec::new();
        if data.is_some()
            && self.prev_mode > 0
            && ((mode == MODE_CELT && self.prev_mode != MODE_CELT && !self.prev_redundancy)
                || (mode != MODE_CELT && self.prev_mode == MODE_CELT))
        {
            transition = true;
        }
        if transition && mode == MODE_CELT {
            pcm_transition = vec![0f32; F5 * ch];
            self.decode_frame(None, &mut pcm_transition, F5.min(audiosize), false)?;
        }
        if audiosize > frame_size {
            return Err(OpusError::BadArg);
        }
        frame_size = audiosize;
        let mut pcm_silk = vec![
            0i16;
            if mode != MODE_CELT {
                F10.max(frame_size) * ch
            } else {
                0
            }
        ];
        if mode != MODE_CELT {
            if self.prev_mode == MODE_CELT {
                self.silk.reset();
            }
            let payload_ms = 10.max(1000 * audiosize / FS);
            let mut internal_rate = None;
            if data.is_some() {
                internal_rate = Some(if mode == MODE_SILK {
                    match bandwidth {
                        BW_NB => 8000,
                        BW_MB => 12000,
                        _ => 16000,
                    }
                } else {
                    16000
                });
            }
            let lost_flag = if data.is_none() {
                1
            } else {
                2 * i32::from(decode_fec)
            };
            let mut decoded = 0usize;
            while decoded < frame_size {
                let first = decoded == 0;
                let out = &mut pcm_silk[decoded * ch..];
                let r = self.silk.decode(
                    self.stream_channels,
                    internal_rate,
                    payload_ms,
                    lost_flag,
                    first,
                    own_dec.as_mut(),
                    out,
                );
                let n = match r {
                    Ok(n) => n,
                    Err(()) => {
                        if lost_flag != 0 {
                            for v in out.iter_mut().take(frame_size * ch) {
                                *v = 0;
                            }
                            frame_size
                        } else {
                            return Err(OpusError::InternalError);
                        }
                    }
                };
                decoded += n;
            }
        }
        let mut start_band = 0;
        let mut redundancy = false;
        let mut redundancy_bytes = 0usize;
        let mut celt_to_silk = false;
        if !decode_fec && mode != MODE_CELT {
            if let Some(dec) = own_dec.as_mut() {
                if dec.tell() + 17 + 20 * i32::from(self.mode == MODE_HYBRID) <= 8 * len as i32 {
                    redundancy = if mode == MODE_HYBRID {
                        dec.bit_logp(12)
                    } else {
                        true
                    };
                    if redundancy {
                        celt_to_silk = dec.bit_logp(1);
                        redundancy_bytes = if mode == MODE_HYBRID {
                            dec.uint(256) as usize + 2
                        } else {
                            len.saturating_sub(((dec.tell() + 7) >> 3) as usize)
                        };
                        if redundancy_bytes > len {
                            len = 0;
                            redundancy_bytes = 0;
                            redundancy = false;
                        } else {
                            len -= redundancy_bytes;
                            if (len as i32) * 8 < dec.tell() {
                                len = 0;
                                redundancy_bytes = 0;
                                redundancy = false;
                            }
                        }
                        dec.shrink(dec.storage() - redundancy_bytes as u32);
                    }
                }
            }
        }
        if mode != MODE_CELT {
            start_band = 17;
        }
        if redundancy {
            transition = false;
        }
        if transition && mode != MODE_CELT {
            pcm_transition = vec![0f32; F5 * ch];
            self.decode_frame(None, &mut pcm_transition, F5.min(audiosize), false)?;
        }
        if bandwidth != 0 {
            self.celt.end = match bandwidth {
                BW_NB => 13,
                BW_MB | BW_WB => 17,
                BW_SWB => 19,
                _ => 21,
            };
        }
        self.celt.stream_channels = self.stream_channels;
        let mut redundant_audio = vec![0f32; if redundancy { F5 * ch } else { 0 }];
        let mut redundant_rng = 0u32;
        let red_data: Option<&[u8]> = data.map(|d| &d[len..len + redundancy_bytes]);
        if redundancy && celt_to_silk {
            self.celt.start = 0;
            let _ = self.celt.decode(red_data, &mut redundant_audio, F5, None);
            redundant_rng = self.celt.rng;
        }
        self.celt.start = start_band;
        let mut celt_ret = Ok(0usize);
        if mode != MODE_SILK {
            let celt_frame = F20.min(frame_size);
            if mode != self.prev_mode && self.prev_mode > 0 && !self.prev_redundancy {
                let (s, e, sc) = (self.celt.start, self.celt.end, self.celt.stream_channels);
                self.celt.reset();
                self.celt.start = s;
                self.celt.end = e;
                self.celt.stream_channels = sc;
            }
            let cdata = if decode_fec {
                None
            } else {
                data.map(|d| &d[..len])
            };
            celt_ret = self
                .celt
                .decode(cdata, pcm, celt_frame, own_dec.as_mut())
                .map_err(|()| OpusError::InternalError);
        } else {
            for v in pcm.iter_mut().take(frame_size * ch) {
                *v = 0.0;
            }
            if self.prev_mode == MODE_HYBRID
                && !(redundancy && celt_to_silk && self.prev_redundancy)
            {
                self.celt.start = 0;
                let _ = self.celt.decode(Some(&[0xFF, 0xFF]), pcm, F2_5, None);
            }
        }
        if mode != MODE_CELT {
            for i in 0..frame_size * ch {
                pcm[i] += (1.0f32 / 32768.0) * f32::from(pcm_silk[i]);
            }
        }
        if redundancy && !celt_to_silk {
            let (s, e, sc) = (0, self.celt.end, self.celt.stream_channels);
            self.celt.reset();
            self.celt.start = s;
            self.celt.end = e;
            self.celt.stream_channels = sc;
            let _ = self.celt.decode(red_data, &mut redundant_audio, F5, None);
            redundant_rng = self.celt.rng;
            let at = ch * (frame_size - F2_5);
            let in1: Vec<f32> = pcm[at..at + F2_5 * ch].to_vec();
            smooth_fade(
                &in1,
                &redundant_audio[ch * F2_5..],
                &mut pcm[at..],
                F2_5,
                ch,
            );
        }
        if redundancy && celt_to_silk {
            for c in 0..ch {
                for i in 0..F2_5 {
                    pcm[ch * i + c] = redundant_audio[ch * i + c];
                }
            }
            let in2: Vec<f32> = pcm[ch * F2_5..ch * F5].to_vec();
            smooth_fade(
                &redundant_audio[ch * F2_5..],
                &in2,
                &mut pcm[ch * F2_5..],
                F2_5,
                ch,
            );
        }
        if transition {
            if audiosize >= F5 {
                pcm[..ch * F2_5].copy_from_slice(&pcm_transition[..ch * F2_5]);
                let in2: Vec<f32> = pcm[ch * F2_5..ch * F5].to_vec();
                smooth_fade(
                    &pcm_transition[ch * F2_5..],
                    &in2,
                    &mut pcm[ch * F2_5..],
                    F2_5,
                    ch,
                );
            } else {
                let in2: Vec<f32> = pcm[..ch * F2_5].to_vec();
                smooth_fade(&pcm_transition, &in2, pcm, F2_5, ch);
            }
        }
        if self.decode_gain != 0 {
            let gain = (0.693_147_180_559_945_3_f64
                * f64::from(6.488_140_8e-4_f32 * self.decode_gain as f32))
            .exp() as f32;
            for v in pcm.iter_mut().take(frame_size * ch) {
                *v *= gain;
            }
        }
        self.range_final = if len <= 1 {
            0
        } else {
            own_dec.as_ref().map_or(0, RangeDecoder::rng) ^ redundant_rng
        };
        self.prev_mode = mode;
        self.prev_redundancy = redundancy && !celt_to_silk;
        celt_ret.map(|_| audiosize)
    }

    /// `opus_decode_native` (no FEC): also returns the bytes the packet
    /// occupied, which is what a self-delimited one needs.
    fn decode_native(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [f32],
        frame_size: usize,
        self_delimited: bool,
    ) -> Result<(usize, usize), OpusError> {
        let ch = self.channels;
        let data = match packet {
            Some(p) if !p.is_empty() => p,
            _ => {
                if frame_size % (FS / 400) != 0 {
                    return Err(OpusError::BadArg);
                }
                let mut count = 0;
                while count < frame_size {
                    let r =
                        self.decode_frame(None, &mut pcm[count * ch..], frame_size - count, false)?;
                    count += r;
                }
                self.last_packet_duration = count;
                return Ok((count, 0));
            }
        };
        let toc = data[0];
        let pmode = packet_mode(toc);
        let pbw = packet_bandwidth(toc);
        let pfs = samples_per_frame(toc);
        let psc = if toc & 0x4 != 0 { 2 } else { 1 };
        let parsed = parse_packet(data, self_delimited)?;
        if parsed.frames.len() * pfs > frame_size {
            return Err(OpusError::BufferTooSmall);
        }
        self.mode = pmode;
        self.bandwidth = pbw;
        self.frame_size = pfs;
        self.stream_channels = psc;
        let mut nb = 0usize;
        for &(off, size) in &parsed.frames {
            let r = self.decode_frame(
                Some(&data[off..off + size]),
                &mut pcm[nb * ch..],
                frame_size - nb,
                false,
            )?;
            nb += r;
        }
        self.last_packet_duration = nb;
        Ok((nb, parsed.packet_len))
    }
}

/// The multistream decoder (RFC 7845 §5.1.1, libopus
/// `opus_multistream_decoder.c`): `streams` Opus streams, the first
/// `coupled` of them stereo, routed to output channels by `mapping`
/// (255 = silence). Channel mapping family 0 is the one-stream case.
pub(crate) struct MultistreamDecoder {
    decoders: Vec<OpusDecoder>,
    coupled: usize,
    mapping: Vec<u8>,
    channels: usize,
    buf: Vec<f32>,
}

impl MultistreamDecoder {
    /// `None` for a layout `opus_multistream_decoder_init` rejects.
    pub(crate) fn new(
        channels: usize,
        streams: usize,
        coupled: usize,
        mapping: &[u8],
    ) -> Option<Self> {
        if channels == 0
            || channels > 255
            || coupled > streams
            || streams == 0
            || streams + coupled > 255
            || mapping.len() != channels
        {
            return None;
        }
        if mapping
            .iter()
            .any(|&m| m != 255 && usize::from(m) >= streams + coupled)
        {
            return None;
        }
        let decoders = (0..streams)
            .map(|s| OpusDecoder::new(if s < coupled { 2 } else { 1 }))
            .collect();
        Some(Self {
            decoders,
            coupled,
            mapping: mapping.to_vec(),
            channels,
            buf: vec![0.0; 2 * MAX_FRAME],
        })
    }

    pub(crate) fn set_gain(&mut self, q8: i32) {
        for d in &mut self.decoders {
            d.set_gain(q8);
        }
    }

    /// The streams' final range coder states XORed (`OPUS_GET_FINAL_RANGE`),
    /// the bit-exactness fingerprint the tests compare with libopus's.
    #[cfg(test)]
    pub(crate) fn range_final(&self) -> u32 {
        self.decoders.iter().fold(0, |a, d| a ^ d.range_final)
    }

    fn validate(&self, mut data: &[u8]) -> Result<usize, OpusError> {
        let n = self.decoders.len();
        let mut samples = 0;
        for s in 0..n {
            if data.is_empty() {
                return Err(OpusError::InvalidPacket);
            }
            let p = parse_packet(data, s != n - 1)?;
            let tmp = p.frames.len() * samples_per_frame(data[0]);
            if s != 0 && samples != tmp {
                return Err(OpusError::InvalidPacket);
            }
            samples = tmp;
            data = &data[p.packet_len.min(data.len())..];
        }
        Ok(samples)
    }

    /// `opus_multistream_decode_float`: one multistream packet (`None`
    /// conceals `frame_size` samples) into `pcm` (interleaved, all
    /// channels); returns samples per channel.
    pub(crate) fn decode(
        &mut self,
        packet: Option<&[u8]>,
        pcm: &mut [f32],
        frame_size: usize,
    ) -> Result<usize, OpusError> {
        let mut frame_size = frame_size.min(FS / 25 * 3);
        let n = self.decoders.len();
        let do_plc = packet.is_none_or(<[u8]>::is_empty);
        let mut data = packet.unwrap_or(&[]);
        if !do_plc {
            if data.len() < 2 * n - 1 {
                return Err(OpusError::InvalidPacket);
            }
            let s = self.validate(data)?;
            if s > frame_size {
                return Err(OpusError::BufferTooSmall);
            }
        }
        let cc = self.channels;
        for s in 0..n {
            if !do_plc && data.is_empty() {
                return Err(OpusError::InternalError);
            }
            let pkt = if do_plc { None } else { Some(data) };
            let (ret, used) =
                self.decoders[s].decode_native(pkt, &mut self.buf, frame_size, s != n - 1)?;
            if !do_plc {
                data = &data[used.min(data.len())..];
            }
            if ret == 0 {
                return Ok(0);
            }
            frame_size = ret;
            let route = |m: u8| -> Option<(usize, usize)> {
                let m = usize::from(m);
                if s < self.coupled {
                    (m == 2 * s)
                        .then_some((0, 2))
                        .or((m == 2 * s + 1).then_some((1, 2)))
                } else {
                    (m == s + self.coupled).then_some((0, 1))
                }
            };
            for c in 0..cc {
                if self.mapping[c] == 255 {
                    continue;
                }
                if let Some((off, stride)) = route(self.mapping[c]) {
                    for i in 0..frame_size {
                        pcm[i * cc + c] = self.buf[i * stride + off];
                    }
                }
            }
        }
        for c in 0..cc {
            if self.mapping[c] == 255 {
                for i in 0..frame_size {
                    pcm[i * cc + c] = 0.0;
                }
            }
        }
        Ok(frame_size)
    }
}

#[cfg(test)]
mod conformance {
    //! Graded against libopus 1.3 itself (tools/opus-ref encodes each
    //! fixture and decodes it with `opus_multistream_decode_float`): every
    //! packet's final range-coder state must equal libopus's — the
    //! RFC 6716 §6 conformance fingerprint, which proves every symbol was
    //! decoded identically — and every sample must round to libopus's
    //! own 16-bit rounding of its float output.

    use super::MultistreamDecoder;
    use crate::codec::opus;

    /// Decode `bytes` the way `opus-ref decf` does, collecting the
    /// per-packet final ranges.
    fn decode(bytes: &[u8]) -> (Vec<f32>, Vec<u32>, usize) {
        let s = opus::parse(bytes).unwrap();
        let ch = usize::from(s.head.channels);
        let (streams, coupled, mapping) = if s.head.mapping_family == 0 {
            (1, usize::from(ch == 2), (0..ch as u8).collect())
        } else {
            (
                usize::from(s.head.streams),
                usize::from(s.head.coupled),
                s.head.mapping.clone(),
            )
        };
        let mut d = MultistreamDecoder::new(ch, streams, coupled, &mapping).unwrap();
        d.set_gain(i32::from(s.head.output_gain));
        let mut pcm = Vec::new();
        let mut rngs = Vec::new();
        let mut buf = vec![0f32; super::MAX_FRAME * ch];
        for (p, _) in &s.packets {
            let n = d.decode(Some(p), &mut buf, super::MAX_FRAME).unwrap();
            pcm.extend_from_slice(&buf[..n * ch]);
            rngs.push(d.range_final());
        }
        let skip = usize::from(s.head.pre_skip) * ch;
        let end = s
            .total_pcm
            .map_or(pcm.len(), |t| skip + t as usize * ch)
            .min(pcm.len());
        (pcm[skip..end].to_vec(), rngs, ch)
    }

    fn check(name: &str, bytes: &[u8], reference: &[u8], rng: &[u8], exact: bool) {
        let (pcm, rngs, _) = decode(bytes);
        let want: Vec<u32> = rng
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(rngs.len(), want.len(), "{name}: packet count");
        for (i, (a, b)) in rngs.iter().zip(&want).enumerate() {
            assert_eq!(a, b, "{name}: final range of packet {i}");
        }
        let r: Vec<i16> = reference
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(pcm.len(), r.len(), "{name}: sample count");
        let mut worst = 0f32;
        for (i, (&v, &w)) in pcm.iter().zip(&r).enumerate() {
            let x = v * 32768.0;
            if (w == i16::MAX && x >= 32767.0) || (w == i16::MIN && x <= -32768.0) {
                continue; // clamped in the reference
            }
            let e = (x - f32::from(w)).abs();
            worst = worst.max(e);
            if exact {
                // SILK-only: integer output, so the float is exactly the
                // 16-bit sample libopus produced.
                assert!(e == 0.0, "{name}: sample {i}: {x} vs {w}");
            } else {
                // Libopus's own rounding, give or take float slack at a
                // .5 boundary (2e-2 LSB is 6e-7 of full scale).
                assert!(e <= 0.52, "{name}: sample {i}: {x} vs {w}");
            }
        }
        eprintln!(
            "{name}: {} packets, {} samples, worst {worst:.4} LSB",
            rngs.len(),
            pcm.len()
        );
    }

    macro_rules! fixture {
        ($f:literal, $exact:expr) => {
            check(
                $f,
                include_bytes!(concat!("../../../tests/fixtures/", $f, ".opus")),
                include_bytes!(concat!("../../../tests/fixtures/", $f, ".opus.ref")),
                include_bytes!(concat!("../../../tests/fixtures/", $f, ".opus.rng")),
                $exact,
            )
        };
    }

    /// SILK NB/MB/WB at 10/20/40/60 ms, stereo, a stereo→mono-coded
    /// switch and LBRR (in-band FEC) data to skip: sample-exact.
    #[test]
    fn silk_is_bit_exact() {
        fixture!("opus_silk", true);
    }

    /// CELT at 2.5/5/10/20 ms, NB to FB, 16–256 kb/s, mono-coded stereo
    /// and CBR.
    #[test]
    fn celt_matches_libopus() {
        fixture!("opus_celt", false);
    }

    /// Hybrid SWB/FB and every switch between SILK, hybrid and CELT
    /// (redundant CELT frames, the 2.5 ms cross-fades, concealment of the
    /// previous mode), with a +3.125 dB header gain.
    #[test]
    fn hybrid_and_mode_switches_match_libopus() {
        fixture!("opus_switch", false);
    }

    /// DTX: 1-byte packets over silence drive SILK's concealment and
    /// comfort-noise generator.
    #[test]
    fn dtx_concealment_and_comfort_noise_match_libopus() {
        fixture!("opus_dtx", false);
    }

    /// 5.1 surround: mapping family 1, four streams (two coupled),
    /// self-delimited framing.
    #[test]
    fn surround_multistream_matches_libopus() {
        fixture!("opus_51", false);
    }

    /// Concealment on demand: a lost packet (`None`) and an empty one
    /// conceal for the asked duration, in every mode, without panicking,
    /// and the stream recovers on the next real packet.
    #[test]
    fn loss_conceals_in_every_mode() {
        for bytes in [
            &include_bytes!("../../../tests/fixtures/opus_silk.opus")[..],
            &include_bytes!("../../../tests/fixtures/opus_celt.opus")[..],
            &include_bytes!("../../../tests/fixtures/opus_switch.opus")[..],
        ] {
            let s = opus::parse(bytes).unwrap();
            let mut d = MultistreamDecoder::new(2, 1, 1, &[0, 1]).unwrap();
            let mut buf = vec![0f32; super::MAX_FRAME * 2];
            for (i, (p, _)) in s.packets.iter().enumerate() {
                let n = if i % 4 == 3 {
                    let dur = opus::packet_info(p).unwrap().samples as usize;
                    d.decode(if i % 8 == 3 { None } else { Some(&[]) }, &mut buf, dur)
                        .unwrap()
                } else {
                    d.decode(Some(p), &mut buf, super::MAX_FRAME).unwrap()
                };
                assert!(n > 0);
                assert!(buf[..n * 2].iter().all(|v| v.is_finite() && v.abs() < 4.0));
            }
        }
    }

    /// Hostile payloads: every fixture's packets bit-flipped, truncated,
    /// re-TOC'd and extended with noise, fed through one long-lived
    /// decoder per stream (so corrupted state carries over, as it would in
    /// a real stream). Nothing may panic — this runs with overflow checks
    /// on — and whatever decodes must be finite.
    #[test]
    fn hostile_packets_never_panic() {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut rnd = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let fixtures: [&[u8]; 6] = [
            include_bytes!("../../../tests/fixtures/opus_silk.opus"),
            include_bytes!("../../../tests/fixtures/opus_celt.opus"),
            include_bytes!("../../../tests/fixtures/opus_switch.opus"),
            include_bytes!("../../../tests/fixtures/opus_dtx.opus"),
            include_bytes!("../../../tests/fixtures/opus_51.opus"),
            include_bytes!("../../../tests/fixtures/src.opus"),
        ];
        let mut decoded = 0usize;
        for bytes in fixtures {
            let s = opus::parse(bytes).unwrap();
            let ch = usize::from(s.head.channels);
            let (streams, coupled, mapping) = if s.head.mapping_family == 0 {
                (1, usize::from(ch == 2), (0..ch as u8).collect())
            } else {
                (
                    usize::from(s.head.streams),
                    usize::from(s.head.coupled),
                    s.head.mapping.clone(),
                )
            };
            let mut d = MultistreamDecoder::new(ch, streams, coupled, &mapping).unwrap();
            let mut buf = vec![0f32; super::MAX_FRAME * ch];
            for round in 0..12 {
                for (p, _) in &s.packets {
                    let mut p = p.clone();
                    match (rnd() % 6, round) {
                        (_, 0) => {} // a clean pass first: real state
                        (0, _) => {
                            for _ in 0..1 + rnd() % 8 {
                                let i = (rnd() as usize) % p.len();
                                p[i] ^= 1 << (rnd() % 8);
                            }
                        }
                        (1, _) => p.truncate(1 + (rnd() as usize) % p.len()),
                        (2, _) => p[0] = rnd() as u8,
                        (3, _) => p.extend((0..rnd() % 64).map(|_| rnd() as u8)),
                        (4, _) => {
                            for v in p.iter_mut().skip(1) {
                                *v = rnd() as u8;
                            }
                        }
                        _ => {
                            let i = (rnd() as usize) % p.len();
                            p[i] = if rnd() & 1 == 0 { 0 } else { 0xFF };
                        }
                    }
                    if let Ok(n) = d.decode(Some(&p), &mut buf, super::MAX_FRAME) {
                        assert!(
                            buf[..n * ch].iter().all(|v| v.is_finite()),
                            "non-finite output"
                        );
                        decoded += 1;
                    }
                }
            }
        }
        assert!(decoded > 1000, "only {decoded} packets decoded");
    }

    /// Self-delimited framing (RFC 6716 Appendix B) round-trips every
    /// frame-packing code, and refuses a size that runs off the end.
    #[test]
    fn self_delimited_framing_parses() {
        use super::parse_packet;
        // Code 0, one 3-byte frame, then trailing data from the next stream.
        let p = parse_packet(&[0x08, 3, 1, 2, 3, 9, 9], true).unwrap();
        assert_eq!((p.frames.clone(), p.packet_len), (vec![(2, 3)], 5));
        // Code 1 (CBR pair): one explicit size covers both frames.
        let p = parse_packet(&[0x09, 2, 1, 1, 2, 2, 7], true).unwrap();
        assert_eq!((p.frames.clone(), p.packet_len), (vec![(2, 2), (4, 2)], 6));
        // Code 2: first size, then the self-delimiting last size.
        let p = parse_packet(&[0x0A, 1, 2, 7, 8, 8], true).unwrap();
        assert_eq!(p.frames, vec![(3, 1), (4, 2)]);
        // Code 3 CBR, 3 frames of 1 byte, with 1 byte of padding.
        let p = parse_packet(&[0x0B, 0x43, 1, 1, 5, 6, 7, 0, 9], true).unwrap();
        assert_eq!(
            (p.frames.clone(), p.packet_len),
            (vec![(4, 1), (5, 1), (6, 1)], 8)
        );
        // A self-delimited size past the end refuses.
        assert!(parse_packet(&[0x08, 9, 1, 2], true).is_err());
        // The non-delimited framing of the same code-0 bytes is one frame
        // holding everything.
        assert_eq!(
            parse_packet(&[0x08, 3, 1, 2], false).unwrap().frames,
            vec![(1, 3)]
        );
    }

    /// Layouts the multistream decoder cannot route are refused, not
    /// half-built.
    #[test]
    fn impossible_layouts_refuse() {
        assert!(
            MultistreamDecoder::new(2, 1, 2, &[0, 1]).is_none(),
            "coupled > streams"
        );
        assert!(
            MultistreamDecoder::new(2, 1, 0, &[0, 3]).is_none(),
            "mapping past the streams"
        );
        assert!(MultistreamDecoder::new(0, 1, 0, &[]).is_none());
        assert!(
            MultistreamDecoder::new(3, 2, 1, &[0, 1, 255]).is_some(),
            "255 is silence"
        );
    }
}

#[cfg(test)]
mod debug {
    /// Developer harness: `OPUS_IN=x.opus OPUS_OUT=y.f32 cargo test -p
    /// vieww-audio opusdec::debug -- --ignored` writes this decoder's
    /// output (pre-skip applied) and per-packet final ranges, the same
    /// layout `opus-ref decf` writes, for side-by-side comparison.
    #[test]
    #[ignore = "developer harness; needs OPUS_IN / OPUS_OUT"]
    fn dump() {
        let (Ok(inp), Ok(out)) = (std::env::var("OPUS_IN"), std::env::var("OPUS_OUT")) else {
            return;
        };
        let bytes = std::fs::read(inp).unwrap();
        let s = crate::codec::opus::parse(&bytes).unwrap();
        let ch = usize::from(s.head.channels);
        let (streams, coupled, mapping) = if s.head.mapping_family == 0 {
            (
                1,
                usize::from(ch == 2),
                if ch == 2 { vec![0, 1] } else { vec![0] },
            )
        } else {
            (
                usize::from(s.head.streams),
                usize::from(s.head.coupled),
                s.head.mapping.clone(),
            )
        };
        let mut d = super::MultistreamDecoder::new(ch, streams, coupled, &mapping).unwrap();
        d.set_gain(i32::from(s.head.output_gain));
        let mut pcm = Vec::new();
        let mut rngs = Vec::new();
        let mut buf = vec![0f32; super::MAX_FRAME * ch];
        for (p, _) in &s.packets {
            let n = d.decode(Some(p), &mut buf, super::MAX_FRAME).unwrap();
            pcm.extend_from_slice(&buf[..n * ch]);
            rngs.extend(d.range_final().to_le_bytes());
        }
        let skip = usize::from(s.head.pre_skip) * ch;
        let end = s
            .total_pcm
            .map_or(pcm.len(), |t| skip + t as usize * ch)
            .min(pcm.len());
        let b: Vec<u8> = pcm[skip..end]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        std::fs::write(&out, b).unwrap();
        std::fs::write(format!("{out}.rng"), rngs).unwrap();
    }
}
