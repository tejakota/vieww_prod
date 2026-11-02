//! Ogg Opus — the container, durations, gains and seeking; the payload by
//! platform.
//!
//! What is here, complete and tested: the Ogg-Opus encapsulation of
//! RFC 7845 — the identification header ([`Head`]: version, channel
//! count, pre-skip, input sample rate, output gain in Q7.8, channel
//! mapping families 0/1/2 with their stream/coupled counts and mapping
//! tables), the comment header (read by [`tags`](super::tags)), packet
//! enumeration with the TOC byte's mode/bandwidth/frame-size decode
//! (RFC 6716 §3.1's Table 2), the four frame-packing codes of §3.2
//! including code 3's VBR/CBR/padding layout, per-packet duration, and
//! the granule-position → PCM-sample-position arithmetic (`granule −
//! pre-skip` at the Ogg Opus clock of 48 kHz) that sample-accurate
//! seeking rests on. Output-gain application and pre-skip trimming are
//! pure functions over a decoded stream ([`Head::apply_to`]) so a
//! platform decode can be finished off exactly as the spec demands.
//!
//! What is not here, named plainly: the SILK and CELT payload decoders
//! of RFC 6716 §4–§6 (LP-layer LTP/LPC, shape coding, the MDCT layer's
//! band algebra, mode coupling). `decode` refuses them by name rather
//! than guessing, exactly as `aac` refuses SBR/PS. The player paths
//! consult [`platform`](super::platform) when this refusal comes back:
//! Android's MediaCodec and Apple's Core Audio decode Opus natively, and
//! the parsed head above is what finishes their output.
//!
//! Opus itself is royalty-free: the contributing patent holders
//! (Xiph, Microsoft, Broadcom and others) made IETF royalty-free
//! licensing declarations for RFC 6716 — see PATENTS.md.

use super::ogg;
use super::{CodecError, Decoded};

/// The refusal every Opus payload decode returns — a stable prefix the
/// platform fallback can match on (see [`super::platform`]).
pub const REFUSED_PAYLOAD: &str =
    "Opus: container, durations, gain and seeking decoded; SILK/CELT payload (RFC 6716 §4-6) is not — hand the bytes to a PlatformCodec";

/// The Ogg Opus clock: granule positions and pre-skip are always in
/// 48 kHz samples, whatever rate the audio was encoded from.
pub const CLOCK: u32 = 48_000;

/// The identification header (RFC 7845 §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// Encapsulation version (1; ≤15 is backwards compatible).
    pub version: u8,
    /// Output channel count.
    pub channels: u8,
    /// Samples (at 48 kHz) to discard from the start of the decode.
    pub pre_skip: u16,
    /// The rate of the audio the encoder was fed — informational, not
    /// the playback rate.
    pub input_rate: u32,
    /// Output gain in Q7.8 dB (i.e. `gain / 256` dB), applied after
    /// decoding.
    pub output_gain: i16,
    /// Channel mapping family (0, 1 or 2 here).
    pub mapping_family: u8,
    /// Total streams packed into each packet.
    pub streams: u8,
    /// Streams decoded as stereo pairs.
    pub coupled: u8,
    /// The mapping table: which decoded channel feeds each output.
    pub mapping: Vec<u8>,
}

impl Head {
    /// Parse the identification-header packet.
    ///
    /// # Errors
    /// Not an OpusHead, an incompatible version, or a mapping this
    /// module refuses by name.
    pub fn parse(p: &[u8]) -> Result<Self, CodecError> {
        if p.len() < 8 || &p[..8] != b"OpusHead" {
            return Err(CodecError::NotThisFormat);
        }
        if p.len() < 19 {
            return Err(CodecError::Truncated);
        }
        let version = p[8];
        if version > 15 {
            return Err(CodecError::Unsupported(
                "Opus encapsulation versions above 15",
            ));
        }
        let channels = p[9];
        if channels == 0 {
            return Err(CodecError::Malformed("Opus channel count is zero"));
        }
        let pre_skip = u16::from_le_bytes([p[10], p[11]]);
        let input_rate = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
        let output_gain = i16::from_le_bytes([p[16], p[17]]);
        let mapping_family = p[18];
        let mut head = Self {
            version,
            channels,
            pre_skip,
            input_rate,
            output_gain,
            mapping_family,
            streams: 1,
            coupled: if channels > 1 { 1 } else { 0 },
            mapping: Vec::new(),
        };
        match mapping_family {
            0 => {
                if channels > 2 {
                    return Err(CodecError::Malformed(
                        "Opus mapping family 0 with more than 2 channels",
                    ));
                }
                // The table is omitted; the defaults above are the spec's.
            }
            1 | 2 => {
                if mapping_family == 1 && channels > 8 {
                    return Err(CodecError::Malformed(
                        "Opus mapping family 1 with more than 8 channels",
                    ));
                }
                if channels > 64 {
                    // Family 2 permits wide ambisonic orders; this crate's
                    // channel ceiling (MAX_CHANNELS) is the honest bound.
                    return Err(CodecError::Unsupported(
                        "Opus streams of more than 64 channels",
                    ));
                }
                if p.len() < 21 {
                    return Err(CodecError::Truncated);
                }
                head.streams = p[19];
                head.coupled = p[20];
                if head.streams == 0 {
                    return Err(CodecError::Malformed("Opus stream count is zero"));
                }
                if usize::from(head.coupled) > usize::from(head.streams) {
                    return Err(CodecError::Malformed(
                        "Opus coupled count above the stream count",
                    ));
                }
                let want = usize::from(channels);
                let table = p.get(21..21 + want).ok_or(CodecError::Truncated)?;
                head.mapping = table.to_vec();
            }
            3..=191 | 200..=253 => {
                return Err(CodecError::Unsupported(
                    "Opus channel mapping families other than 0/1/2",
                ));
            }
            _ => {
                // 192..199 and 254..255: reserved/undefined in the
                // registry; a stream using one is not decodable here.
                return Err(CodecError::Unsupported(
                    "Opus channel mapping families other than 0/1/2",
                ));
            }
        }
        Ok(head)
    }

    /// The gain as a linear factor.
    #[must_use]
    pub fn gain_factor(&self) -> f32 {
        // Q7.8: 1/256 dB per unit, and the dB→linear scale is 10^(dB/20)
        // — together a divisor of 20·256 = 5120. f32::from(i16) then powf
        // keeps the conversion exact enough for a ±128 dB range.
        10f32.powf(f32::from(self.output_gain) / 5_120.0)
    }

    /// Apply the header's post-processing to a decoded stream: trim the
    /// pre-skip from the start, then scale by the output gain. Both are
    /// the spec's own "SHOULD"s, and both are what makes a decode
    /// byte-compatible with a spec-conformant one.
    ///
    /// The pre-skip counts at the 48 kHz clock whatever rate the decode
    /// is at (a platform decoder's choice), so it is scaled before the
    /// frames are dropped — dropped, not silenced: the stream's own
    /// length arithmetic (granule − pre-skip) is only honest if the
    /// trimmed samples are actually gone.
    ///
    /// # Errors
    /// [`CodecError::Truncated`] when the pre-skip would scale past the
    /// decode's whole length — the head and the decode disagree, and the
    /// caller should hear about it rather than receive an empty stream.
    pub fn apply_to(&self, d: &mut Decoded) -> Result<(), CodecError> {
        let ch = usize::from(d.channels.max(1));
        // Pre-skip is on the 48 kHz clock; the decode may be at another
        // rate (a platform decoder's choice), so scale it.
        let skip = if d.rate == 0 {
            usize::from(self.pre_skip)
        } else {
            ((u64::from(self.pre_skip) * u64::from(d.rate)) / u64::from(CLOCK)) as usize
        };
        if skip * ch > d.data.len() {
            return Err(CodecError::Truncated);
        }
        d.data.drain(..skip * ch);
        if self.output_gain != 0 {
            let g = self.gain_factor();
            for v in &mut d.data {
                *v *= g;
            }
        }
        Ok(())
    }
}

/// What the TOC byte says about a packet's payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toc {
    /// Configuration (5 bits): mode, bandwidth and frame size.
    pub config: u8,
    /// Stereo flag.
    pub stereo: bool,
    /// Frame count code (2 bits).
    pub code: u8,
}

/// The coding mode a configuration selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The LP layer, for low bitrates.
    Silk,
    /// Both layers, for full-band speech.
    Hybrid,
    /// The MDCT layer, for music and low delay.
    Celt,
}

impl Toc {
    /// The TOC byte of the first frame of a packet.
    #[must_use]
    pub const fn parse(b: u8) -> Self {
        Self {
            config: b >> 3,
            stereo: b & 0x4 != 0,
            code: b & 0x3,
        }
    }

    /// The mode a configuration selects (RFC 6716 Table 2's grouping).
    #[must_use]
    pub const fn mode(&self) -> Mode {
        match self.config {
            0..=11 => Mode::Silk,
            12..=15 => Mode::Hybrid,
            _ => Mode::Celt,
        }
    }

    /// One frame's duration in 48 kHz samples — the arithmetic of the
    /// reference decoder's `opus_packet_get_samples_per_frame`:
    /// SILK 10/20/40/60 ms, Hybrid 10/20 ms, CELT 2.5/5/10/20 ms.
    #[must_use]
    pub const fn frame_samples(&self) -> u32 {
        match self.config {
            0..=11 => match self.config & 3 {
                0 => CLOCK / 100,
                1 => CLOCK / 50,
                2 => CLOCK / 25,
                _ => CLOCK * 60 / 1000,
            },
            12..=15 => {
                if self.config & 1 == 0 {
                    CLOCK / 100
                } else {
                    CLOCK / 50
                }
            }
            _ => match self.config & 3 {
                0 => CLOCK * 25 / 10_000,
                1 => CLOCK / 200,
                2 => CLOCK / 100,
                _ => CLOCK / 50,
            },
        }
    }

    /// The audio bandwidth a configuration selects, by the published
    /// table's grouping (narrow through full).
    #[must_use]
    pub const fn bandwidth_hz(&self) -> u32 {
        match self.config {
            0..=3 | 16..=19 => 4_000,
            4..=7 => 6_000,
            8..=11 | 20..=23 => 8_000,
            12..=13 | 24..=27 => 12_000,
            _ => 20_000,
        }
    }
}

/// What one packet holds, without touching its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketInfo {
    /// Frames in the packet.
    pub frames: usize,
    /// The packet's duration in 48 kHz samples.
    pub samples: u64,
}

/// Parse one Ogg packet's TOC and framing (RFC 6716 §3): the number of
/// frames and their total duration. Frame lengths are validated against
/// the packet's own size — a hostile count refuses instead of running
/// off the end.
///
/// # Errors
/// Truncated or inconsistent framing.
pub fn packet_info(data: &[u8]) -> Result<PacketInfo, CodecError> {
    let Some(&toc_b) = data.first() else {
        return Err(CodecError::Truncated);
    };
    let toc = Toc::parse(toc_b);
    let frame = u64::from(toc.frame_samples());
    let n = data.len();
    let frames = match toc.code {
        0 => 1,
        1 => {
            // Two frames of equal size; the payload must divide evenly.
            if !(n - 1).is_multiple_of(2) {
                return Err(CodecError::Malformed("Opus code-1 packet of odd size"));
            }
            2
        }
        2 => {
            // Two frames; the first's length is coded in one or two bytes.
            let (len1, hdr) = frame_len(data, 1)?;
            if hdr + len1 > n {
                return Err(CodecError::Malformed("Opus code-2 length past the packet"));
            }
            2
        }
        _ => {
            // An arbitrary number of frames, CBR or VBR, with optional
            // padding.
            if n < 2 {
                return Err(CodecError::Truncated);
            }
            let b = data[1];
            let count = usize::from(b >> 2);
            let padding = b & 0x2 != 0;
            let vbr = b & 0x1 != 0;
            if count == 0 {
                return Err(CodecError::Malformed("Opus code-3 frame count is zero"));
            }
            let mut at = 2usize;
            if padding {
                // Padding bytes: 0..254 directly, or 255-escapes each
                // standing for 254 plus the rest of the chain.
                let mut pad = 0usize;
                loop {
                    let Some(&v) = data.get(at) else {
                        return Err(CodecError::Truncated);
                    };
                    at += 1;
                    if v < 255 {
                        pad += usize::from(v);
                        break;
                    }
                    pad += 254;
                }
                if at + pad > n {
                    return Err(CodecError::Malformed("Opus padding past the packet"));
                }
            }
            if vbr {
                // Every frame but the last is length-prefixed.
                for _ in 0..count.saturating_sub(1) {
                    let (len, hdr) = frame_len(data, at)?;
                    at = hdr + len;
                    if at > n {
                        return Err(CodecError::Malformed("Opus frame past the packet"));
                    }
                }
            }
            count
        }
    };
    // The 120 ms packet-duration ceiling the spec sets.
    let samples = frame * frames as u64;
    if samples > u64::from(CLOCK) * 120 / 1000 {
        return Err(CodecError::Malformed("Opus packet longer than 120 ms"));
    }
    Ok(PacketInfo { frames, samples })
}

/// The one-or-two-byte frame length of §3.2.1: `v` alone for 0..=251,
/// `v*1 + second*4` when `v` is 252..=255.
fn frame_len(data: &[u8], at: usize) -> Result<(usize, usize), CodecError> {
    let Some(&v) = data.get(at) else {
        return Err(CodecError::Truncated);
    };
    if v < 252 {
        return Ok((usize::from(v), at + 1));
    }
    let Some(&w) = data.get(at + 1) else {
        return Err(CodecError::Truncated);
    };
    Ok((usize::from(v) + usize::from(w) * 4, at + 2))
}

/// A parsed Ogg Opus stream: everything about it except the payload.
#[derive(Debug, Clone)]
pub struct Stream {
    /// The identification header.
    pub head: Head,
    /// Every audio packet, in order, with the granule position of the
    /// page it completed on (−1 when a later packet on the same page
    /// carried it) — the 48 kHz sample count the stream had produced by
    /// its end.
    pub packets: Vec<(Vec<u8>, i64)>,
    /// The stream's total PCM samples: the final granule less the
    /// pre-skip, when the stream ended cleanly.
    pub total_pcm: Option<u64>,
}

impl Stream {
    /// The position (in 48 kHz samples) the packet at `index` starts at
    /// — the arithmetic sample-accurate seeking rests on: trust the page
    /// granules, sum the durations in between.
    #[must_use]
    pub fn packet_start(&self, index: usize) -> u64 {
        // The last packet before `index` whose page granule is known.
        let anchor = (0..index).rev().find(|&i| self.packets[i].1 >= 0);
        match anchor {
            Some(j) => {
                u64::try_from(self.packets[j].1).unwrap_or(0)
                    + (j + 1..index).fold(0u64, |at, i| {
                        at + packet_info(&self.packets[i].0).map_or(0, |p| p.samples)
                    })
            }
            None => (0..index).fold(0u64, |at, i| {
                at + packet_info(&self.packets[i].0).map_or(0, |p| p.samples)
            }),
        }
    }

    /// The total duration by summing packet durations — the cross-check
    /// the tests hold against the final granule.
    #[must_use]
    pub fn total_by_durations(&self) -> u64 {
        self.packets
            .iter()
            .map(|(p, _)| packet_info(p).map_or(0, |i| i.samples))
            .sum()
    }
}

/// Whether an Ogg byte stream is Opus (its first packet is an OpusHead).
///
/// The ID header completes on the first page for every real file; a
/// first segment of 255 bytes (a header split across pages, which no
/// encoder produces but a hostile one may) falls back to the full
/// packet reassembly.
#[must_use]
pub fn is_opus(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"OggS") || bytes.len() < 28 {
        return false;
    }
    let nseg = usize::from(bytes[26]);
    let segs = bytes.get(27..27 + nseg).unwrap_or(&[]);
    let Some(&first) = segs.first() else {
        return false;
    };
    let body = 27 + nseg;
    if first < 255 {
        bytes
            .get(body..body + usize::from(first))
            .is_some_and(|p| p.starts_with(b"OpusHead"))
    } else {
        ogg::packets(bytes)
            .ok()
            .and_then(|p| p.first().map(|p| p.data.starts_with(b"OpusHead")))
            .unwrap_or(false)
    }
}

/// Parse an Ogg Opus stream's structure end to end.
///
/// # Errors
/// Not Ogg Opus, a page CRC failure, a malformed header or packet
/// framing.
pub fn parse(bytes: &[u8]) -> Result<Stream, CodecError> {
    let packets = ogg::packets(bytes)?;
    if packets.is_empty() || !packets[0].data.starts_with(b"OpusHead") {
        return Err(CodecError::NotThisFormat);
    }
    let head = Head::parse(&packets[0].data)?;
    if packets.len() < 2 || !packets[1].data.starts_with(b"OpusTags") {
        return Err(CodecError::Malformed("no OpusTags header"));
    }
    let mut out = Vec::with_capacity(packets.len());
    for p in &packets[2..] {
        // Validate every packet's framing now, so a caller that trusts
        // `Stream` never meets a malformed one half-way through playback.
        packet_info(&p.data)?;
        out.push((p.data.clone(), p.granule));
    }
    let total_pcm = packets
        .last()
        .filter(|p| p.last_page)
        .and_then(|p| u64::try_from(p.granule.max(0)).ok())
        .map(|g| g.saturating_sub(u64::from(head.pre_skip)));
    Ok(Stream {
        head,
        packets: out,
        total_pcm,
    })
}

/// Decode an Ogg Opus stream. The container is parsed and validated;
/// the SILK/CELT payload is refused by name (see [`REFUSED_PAYLOAD`]) —
/// the player paths hand the bytes to [`platform`](super::platform),
/// which Android and Apple implementations finish with
/// [`Head::apply_to`].
///
/// # Errors
/// [`CodecError::Unsupported`] naming the missing payload decoder, or
/// [`parse`]'s errors.
pub fn decode(bytes: &[u8]) -> Result<Decoded, CodecError> {
    let _ = parse(bytes)?;
    Err(CodecError::Unsupported(REFUSED_PAYLOAD))
}

// ── streaming (AU-05) ────────────────────────────────────────────────────

/// Ogg Opus behind [`super::stream`]'s interface: the container is parsed
/// purely — every packet's framing validated, the trim and the seek
/// arithmetic exact — and the payload is handed whole to the platform
/// bridge, which serves it back a chunk at a time.
///
/// Without a [`PlatformCodec`](super::platform::PlatformCodec) installed,
/// opening refuses by name, so a caller always learns precisely what is
/// missing. With one, the whole decode is held in memory: the platform
/// bridge decodes whole streams (that is its trait), so this source is
/// the "whole-file decode behind the same interface" row of AU-05's
/// table, not the bounded-window path the pure decoders have. A pure
/// payload decoder, when one lands, replaces the bridge here and nothing
/// else moves.
pub(crate) struct OpusSource {
    pcm: Vec<f32>,
    channels: usize,
    rate: u32,
    at: usize,
}

impl OpusSource {
    /// Read the whole stream through the bounded window (the bytes are
    /// the file's own size; nothing more is held) and hand it over.
    pub(crate) fn open(input: &mut super::stream::Input) -> Result<Self, CodecError> {
        let len = usize::try_from(input.len()).unwrap_or(usize::MAX);
        let bytes = input.window(0, len)?.to_vec();
        if bytes.len() < len {
            return Err(CodecError::Truncated);
        }
        Self::from_bytes(&bytes)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        // Parse first: the refusal names what is missing only after the
        // container itself has been proven sound.
        let _ = parse(bytes)?;
        let d = super::platform::decode_chunked(bytes)?.0;
        let channels = usize::from(d.channels.max(1));
        Ok(Self {
            pcm: d.data,
            channels,
            rate: d.rate,
            at: 0,
        })
    }
}

/// Frames served per chunk: one 20 ms Opus frame's neighbourhood, so the
/// pull side's buffering stays fine-grained.
const OPUS_CHUNK: usize = 960;

impl super::stream::FrameSource for OpusSource {
    fn channels(&self) -> usize {
        self.channels
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn trim(&self) -> (u64, Option<u64>) {
        // The platform contract already trimmed the pre-skip; the length
        // is what the decode holds, and the granule cross-check lives in
        // the tests.
        (0, Some((self.pcm.len() / self.channels.max(1)) as u64))
    }

    fn next(
        &mut self,
        _input: &mut super::stream::Input,
    ) -> Result<Option<super::stream::Chunk>, CodecError> {
        let ch = self.channels.max(1);
        if self.at >= self.pcm.len() {
            return Ok(None);
        }
        let end = (self.at + OPUS_CHUNK * ch).min(self.pcm.len());
        let pcm = self.pcm[self.at..end].to_vec();
        self.at = end;
        Ok(Some(super::stream::Chunk { pcm }))
    }

    fn seek(&mut self, _input: &mut super::stream::Input, frame: u64) -> Result<u64, CodecError> {
        // The decode is in memory, so seeking is exact by construction —
        // the packet-start arithmetic this module publishes is what a
        // pure payload decoder will use to get here without one.
        let ch = self.channels.max(1) as u64;
        self.at = usize::try_from(frame.saturating_mul(ch))
            .unwrap_or(usize::MAX)
            .min(self.pcm.len());
        Ok((self.at / self.channels.max(1)) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Building an Ogg page the way the tests need it: a header, a
    /// lacing table, a body, and the CRC the container checks.
    fn page(granule: i64, packets: &[&[u8]], last: bool) -> Vec<u8> {
        let mut lacing = Vec::new();
        let mut body = Vec::new();
        for p in packets {
            body.extend_from_slice(p);
            let mut left = p.len();
            while left >= 255 {
                lacing.push(255);
                left -= 255;
            }
            lacing.push(u8::try_from(left).unwrap());
        }
        let mut b = Vec::with_capacity(27 + lacing.len() + body.len());
        b.extend_from_slice(b"OggS");
        b.push(0);
        b.push(u8::from(last) << 2 | 2); // BOS first page, EOS when asked
        b.extend_from_slice(&granule.to_le_bytes());
        b.extend_from_slice(&0x1234_5678u32.to_le_bytes()); // serial
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes()); // CRC, fixed below
        b.push(u8::try_from(lacing.len()).unwrap());
        b.extend_from_slice(&lacing);
        b.extend_from_slice(&body);
        let crc = ogg::crc32(&b);
        b[22..26].copy_from_slice(&crc.to_le_bytes());
        b
    }

    fn opus_head(channels: u8, pre_skip: u16, gain: i16) -> Vec<u8> {
        let mut h = b"OpusHead".to_vec();
        h.push(1);
        h.push(channels);
        h.extend_from_slice(&pre_skip.to_le_bytes());
        h.extend_from_slice(&44_100u32.to_le_bytes());
        h.extend_from_slice(&gain.to_le_bytes());
        h.push(0);
        h
    }

    fn stream(head: &[u8], audio: &[&[u8]], final_granule: i64) -> Vec<u8> {
        let mut s = page(0, &[head], false);
        let tags = {
            let mut t = b"OpusTags".to_vec();
            t.extend_from_slice(&0u32.to_le_bytes());
            t.extend_from_slice(&0u32.to_le_bytes());
            t
        };
        s.extend(page(0, &[&tags], false));
        s.extend(page(final_granule, audio, true));
        s
    }

    #[test]
    fn the_ffmpeg_fixture_parses_and_its_durations_sum_to_the_granule() {
        let bytes = include_bytes!("../../tests/fixtures/src.opus");
        assert!(is_opus(bytes));
        let s = parse(bytes).unwrap();
        assert_eq!((s.head.channels, s.head.version), (2, 1));
        // ffmpeg's default pre-skip for libopus at 48 kHz is 312 samples
        // (a 6.5 ms lookahead); the ref decode trimmed it.
        eprintln!(
            "opus: {} packets, pre-skip {}, gain {} Q7.8, total {:?}",
            s.packets.len(),
            s.head.pre_skip,
            s.head.output_gain,
            s.total_pcm
        );
        assert!(s.head.pre_skip > 0, "the encoder's lookahead is trimmed");
        // The durations must account for every sample the stream claims,
        // pre-skip included — with one allowance the spec itself makes:
        // an encoder whose audio does not fill a whole final frame pads
        // it, and the final granule position (not the frame durations)
        // names the true end. ffmpeg does exactly that here: 20 full
        // 20 ms frames plus one padded to 960 with only 312 valid.
        let sum = s.total_by_durations();
        let final_granule = s.total_pcm.unwrap() + u64::from(s.head.pre_skip);
        let last = packet_info(&s.packets.last().unwrap().0).unwrap().samples;
        assert!(
            sum >= final_granule && sum - final_granule < last,
            "durations {sum} vs granule {final_granule} (last frame {last})"
        );
        // The reference decode ran at the Opus clock (48 kHz) and trimmed
        // the pre-skip: its frame count is the stream's PCM total.
        let reference_len = include_bytes!("../../tests/fixtures/src.opus.ref").len() / 4;
        assert_eq!(s.total_pcm.unwrap(), reference_len as u64);
    }

    #[test]
    fn decode_refuses_the_payload_by_name() {
        let e = decode(include_bytes!("../../tests/fixtures/src.opus")).unwrap_err();
        assert_eq!(e, CodecError::Unsupported(REFUSED_PAYLOAD));
        assert!(e.to_string().contains("SILK"));
    }

    #[test]
    fn the_vorbis_fixture_is_not_opus() {
        assert!(!is_opus(include_bytes!("../../tests/fixtures/src.ogg")));
    }

    #[test]
    fn toc_durations_cover_every_configuration() {
        let cases = [
            (0u8, 480u32),
            (1, 960),
            (2, 1_920),
            (3, 2_880), // SILK 10-60 ms
            (8, 480),
            (11, 2_880),
            (12, 480),
            (13, 960), // Hybrid 10/20 ms
            (14, 480),
            (15, 960),
            (16, 120),
            (17, 240),
            (18, 480),
            (19, 960), // CELT 2.5-20 ms
            (28, 120),
            (31, 960),
        ];
        for &(config, want) in &cases {
            let toc = Toc::parse(config << 3);
            assert_eq!(toc.frame_samples(), want, "config {config}");
            assert!(!toc.stereo && toc.code == 0);
        }
        assert!(Toc::parse(0x04).stereo);
        assert_eq!(Toc::parse(0x03).code, 3);
        assert_eq!(Toc::parse(0).mode(), Mode::Silk);
        assert_eq!(Toc::parse(12 << 3).mode(), Mode::Hybrid);
        assert_eq!(Toc::parse(31 << 3).mode(), Mode::Celt);
        assert_eq!(Toc::parse(31 << 3).bandwidth_hz(), 20_000);
    }

    #[test]
    fn packet_framing_codes_parse_and_validate() {
        // Code 0: one frame.
        assert_eq!(
            packet_info(&[0x00, 1, 2, 3]).unwrap(),
            PacketInfo {
                frames: 1,
                samples: 480
            }
        );
        // Code 1: two equal frames; odd payload is malformed.
        assert_eq!(
            packet_info(&[0x01, 1, 2, 3, 4]).unwrap(),
            PacketInfo {
                frames: 2,
                samples: 960
            }
        );
        assert!(packet_info(&[0x01, 1, 2, 3]).is_err());
        // Code 2: length byte then two frames.
        assert_eq!(
            packet_info(&[0x02, 2, 9, 9, 8, 8]).unwrap(),
            PacketInfo {
                frames: 2,
                samples: 960
            }
        );
        // A length that runs past the packet refuses.
        assert!(packet_info(&[0x02, 9, 1]).is_err());
        // Code 3 CBR: three frames, no padding, no VBR.
        assert_eq!(
            packet_info(&[0x03, 3 << 2, 0, 0, 0, 0, 0, 0]).unwrap(),
            PacketInfo {
                frames: 3,
                samples: 1_440
            }
        );
        // Code 3 VBR: two length-prefixed frames then the last — the
        // length bytes name the frames that follow them.
        assert_eq!(
            packet_info(&[0x03, 3 << 2 | 1, 1, 9, 1, 8, 8]).unwrap(),
            PacketInfo {
                frames: 3,
                samples: 1_440
            }
        );
        // Code 3 with padding: three padding bytes at the end.
        assert_eq!(
            packet_info(&[0x03, 3 << 2 | 2, 3, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
            PacketInfo {
                frames: 3,
                samples: 1_440
            }
        );
        // The 255 padding escape chain: 254 bytes of padding, then frames.
        {
            let mut p = vec![0x03, 3 << 2 | 2, 255, 0];
            p.resize(4 + 254 + 3, 0);
            assert_eq!(
                packet_info(&p).unwrap(),
                PacketInfo {
                    frames: 3,
                    samples: 1_440
                }
            );
            // One byte short of the padding it claims: refuse.
            p.truncate(4 + 254 - 1);
            assert!(packet_info(&p).is_err());
        }
        // Frame count zero refuses; a one-byte packet truncates.
        assert!(packet_info(&[0x03, 0]).is_err());
        assert!(packet_info(&[0x03]).is_err());
        assert!(packet_info(&[]).is_err());
        // The 120 ms ceiling: 48 × 60 ms frames is 2.88 s — refuse.
        assert!(packet_info(&[0x03, 49 << 2, 0]).is_err());
        // A two-byte length (252 + second*4).
        assert_eq!(frame_len(&[252, 3], 0).unwrap(), (252usize + 12, 2));
    }

    #[test]
    fn a_hand_built_stream_parses_with_gain_and_pre_skip() {
        // 20 ms stereo CELT frames (config 19: WB 20 ms), gain −3.125 dB.
        let head = opus_head(2, 312, -800);
        let audio: [&[u8]; 3] = [&[0x9C, 1, 1, 1], &[0x9C, 1, 1, 1], &[0x9C, 1, 1, 1]];
        let bytes = stream(&head, &audio, 3 * 960);
        assert!(is_opus(&bytes));
        let s = parse(&bytes).unwrap();
        assert_eq!(s.packets.len(), 3);
        assert_eq!(s.total_by_durations(), 2_880);
        assert_eq!(s.total_pcm, Some(2_880 - 312));
        // The gain: −800 Q7.8 is −3.125 dB.
        assert!((s.head.gain_factor() - 10f32.powf(-3.125 / 20.0)).abs() < 1e-6);
        // Packet starts walk the granule clock.
        assert_eq!(s.packet_start(0), 0);
        assert_eq!(s.packet_start(1), 960);
        assert_eq!(s.packet_start(2), 1_920);
    }

    #[test]
    fn pre_skip_trims_and_gain_scales_a_platform_decode() {
        let head = Head::parse(&opus_head(2, 480, 800)).unwrap(); // +3.125 dB
        let mut d = Decoded {
            // 960 stereo frames at 48 kHz: 480 trimmed, 480 kept.
            data: vec![0.5; 960 * 2],
            channels: 2,
            rate: 48_000,
        };
        head.apply_to(&mut d).unwrap();
        let g = 10f32.powf(3.125 / 20.0);
        assert_eq!(d.data.len(), 480 * 2, "the pre-skip is dropped, not muted");
        assert!(d.data.iter().all(|v| (*v - 0.5 * g).abs() < 1e-6));
        // At another rate the pre-skip scales with it.
        let mut d = Decoded {
            data: vec![0.5; 441 * 2],
            channels: 2,
            rate: 44_100,
        };
        head.apply_to(&mut d).unwrap();
        let skip = (480u64 * 44_100 / 48_000) as usize;
        assert_eq!(d.data.len(), (441 - skip) * 2);
        // A pre-skip that would eat the whole decode refuses rather than
        // handing back empty audio.
        let mut d = Decoded {
            data: vec![0.5; 4],
            channels: 2,
            rate: 48_000,
        };
        assert_eq!(head.apply_to(&mut d).err(), Some(CodecError::Truncated));
    }

    #[test]
    fn head_validation_refuses_by_name() {
        // Wrong magic.
        assert_eq!(
            Head::parse(b"OpusTag\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00").err(),
            Some(CodecError::NotThisFormat)
        );
        // Truncated.
        assert_eq!(
            Head::parse(b"OpusHead\x01").err(),
            Some(CodecError::Truncated)
        );
        // Future version.
        let mut future = opus_head(2, 0, 0);
        future[8] = 16;
        assert_eq!(
            Head::parse(&future).err(),
            Some(CodecError::Unsupported(
                "Opus encapsulation versions above 15"
            ))
        );
        // Zero channels.
        assert!(Head::parse(&opus_head(0, 0, 0)).is_err());
        // Family 0 with 3 channels.
        let mut h3 = opus_head(3, 0, 0);
        assert!(Head::parse(&h3).is_err());
        h3[9] = 2;
        h3[18] = 9;
        assert_eq!(
            Head::parse(&h3).err(),
            Some(CodecError::Unsupported(
                "Opus channel mapping families other than 0/1/2"
            ))
        );
        // Family 1 with the mapping table: streams, coupled, then one
        // table entry per channel.
        let mut h8 = opus_head(6, 0, 0);
        h8[18] = 1;
        h8.extend_from_slice(&[2, 1, 0, 1, 2, 3, 4, 0]);
        let parsed = Head::parse(&h8).unwrap();
        assert_eq!(
            (parsed.mapping_family, parsed.streams, parsed.coupled),
            (1, 2, 1)
        );
        assert_eq!(parsed.mapping, vec![0, 1, 2, 3, 4, 0]);
        // Family 1 with a truncated table.
        let mut short = h8.clone();
        short.truncate(24);
        assert_eq!(Head::parse(&short).err(), Some(CodecError::Truncated));
    }

    #[test]
    fn a_corrupt_page_fails_the_crc() {
        let head = opus_head(1, 0, 0);
        let audio: [&[u8]; 1] = [&[0x00, 1, 2, 3]];
        let mut bytes = stream(&head, &audio, 480);
        let n = bytes.len();
        bytes[n - 1] ^= 0xFF;
        assert!(matches!(parse(&bytes), Err(CodecError::Checksum(_))));
    }
}
