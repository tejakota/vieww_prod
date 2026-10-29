//! Decoding a file a piece at a time (AU-05).
//!
//! [`decode`](super::decode) takes the whole file and returns every
//! sample: a three-hour audiobook at 64 kb/s is 90 MB in and 3.8 GB of
//! `f32` out, which no phone survives. [`open`] instead returns a
//! [`PullDecoder`] over any `Read + Seek`: it reads the file through a
//! bounded window ([`Input`], at most a few megabytes held) and decodes one
//! codec frame at a time into the caller's buffer, so memory is a frame or
//! two of audio whatever the file's length.
//!
//! | Format | Streaming | Seek |
//! |---|---|---|
//! | WAV | yes | exact |
//! | FLAC | yes, frame by frame | bisection on frame headers, then exact |
//! | MP3 | yes, frame by frame, gapless (LAME/Xing) | frame index, exact after a short pre-roll |
//! | AAC in MP4/M4A | yes, access unit by access unit | sample table, exact after one frame's pre-roll |
//! | AAC in ADTS | yes, frame by frame | frame index, as MP3 |
//! | Ogg Vorbis, AIFF | whole-file decode behind the same interface | exact |
//!
//! Each streaming decoder is built from the same per-frame functions the
//! whole-file decoder uses, and the tests hold them to the same output:
//! every fixture decodes sample-for-sample identically through both paths
//! (`tests/stream_decode.rs`), and a seek lands on the samples the whole
//! decode has at that position.
//!
//! [`crate::media_stream`] runs a `PullDecoder` on a reader thread ahead of
//! playback; [`crate::engine_player::MediaPlayer`] uses it for long files.

use std::io::{Read, Seek, SeekFrom};

use super::{CodecError, Format, CONCEAL_FRAMES, RUNAWAY};

/// Anything a stream can be read from: a file, a cache, a network buffer.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// A decoder that produces audio a piece at a time.
pub trait PullDecoder: Send {
    /// Channels in the output (interleaved).
    fn channels(&self) -> u8;
    /// Sample rate.
    fn rate(&self) -> u32;
    /// Frames in the stream, when the container says (after gapless
    /// trimming), else `None`.
    fn total_frames(&self) -> Option<u64>;
    /// Fill `out` with interleaved samples; returns how many were written
    /// (a multiple of [`Self::channels`]). `0` means the stream has ended.
    ///
    /// # Errors
    /// The stream is corrupt in a way the decoder cannot conceal.
    fn read(&mut self, out: &mut [f32]) -> Result<usize, CodecError>;
    /// Continue from frame `frame` (clamped to the end).
    ///
    /// # Errors
    /// As [`Self::read`].
    fn seek(&mut self, frame: u64) -> Result<(), CodecError>;
    /// The frame the next [`Self::read`] starts at.
    fn position(&self) -> u64;
}

// ── the byte window ──────────────────────────────────────────────────────

/// How far behind the read position the window keeps bytes.
const KEEP: usize = 1 << 20;
/// The smallest read from the underlying stream.
const READ: usize = 64 << 10;

/// A bounded window over a `Read + Seek`: [`Input::window`] returns the
/// bytes at a position, reading (and seeking) the source as needed, and
/// never holds much more than [`KEEP`] plus what was asked for.
pub(crate) struct Input {
    r: Box<dyn ReadSeek>,
    len: u64,
    buf: Vec<u8>,
    start: u64,
    rpos: u64,
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Input")
            .field("len", &self.len)
            .field("window", &(self.start..self.start + self.buf.len() as u64))
            .finish_non_exhaustive()
    }
}

fn io(e: &std::io::Error) -> CodecError {
    CodecError::Io(e.to_string())
}

impl Input {
    pub(crate) fn new(mut r: Box<dyn ReadSeek>) -> Result<Self, CodecError> {
        let len = r.seek(SeekFrom::End(0)).map_err(|e| io(&e))?;
        r.seek(SeekFrom::Start(0)).map_err(|e| io(&e))?;
        Ok(Self {
            r,
            len,
            buf: Vec::new(),
            start: 0,
            rpos: 0,
        })
    }

    /// The stream's length in bytes.
    pub(crate) const fn len(&self) -> u64 {
        self.len
    }

    /// Up to `want` bytes from `at` (fewer at the end of the stream).
    pub(crate) fn window(&mut self, at: u64, want: usize) -> Result<&[u8], CodecError> {
        let at = at.min(self.len);
        let end_of_buf = self.start + self.buf.len() as u64;
        if at < self.start || at > end_of_buf {
            self.buf.clear();
            self.start = at;
        } else if (at - self.start) as usize > KEEP {
            let cut = (at - self.start) as usize;
            self.buf.drain(..cut);
            self.start = at;
        }
        let from = (at - self.start) as usize;
        let need = from + want;
        while self.buf.len() < need && self.start + (self.buf.len() as u64) < self.len {
            let pos = self.start + self.buf.len() as u64;
            if self.rpos != pos {
                self.r.seek(SeekFrom::Start(pos)).map_err(|e| io(&e))?;
                self.rpos = pos;
            }
            let chunk = (need - self.buf.len()).max(READ);
            let room = usize::try_from(self.len - pos).unwrap_or(usize::MAX);
            let old = self.buf.len();
            self.buf.resize(old + chunk.min(room), 0);
            let n = self.r.read(&mut self.buf[old..]).map_err(|e| io(&e))?;
            self.buf.truncate(old + n);
            self.rpos += n as u64;
            if n == 0 {
                break;
            }
        }
        let end = need.min(self.buf.len());
        Ok(&self.buf[from.min(end)..end])
    }

    /// Bytes held now: what the tests bound.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.buf.len()
    }
}

// ── one codec frame at a time ────────────────────────────────────────────

/// A run of decoded frames.
#[derive(Debug)]
pub(crate) struct Chunk {
    /// Interleaved samples.
    pub(crate) pcm: Vec<f32>,
}

/// What each format provides: the next codec frame's samples, and a seek
/// to a frame at or before a target.
pub(crate) trait FrameSource: Send {
    fn channels(&self) -> usize;
    fn rate(&self) -> u32;
    /// Frames to drop at the start (encoder priming) and how many to play
    /// after them, when known.
    fn trim(&self) -> (u64, Option<u64>);
    /// The next chunk; `None` at the end.
    fn next(&mut self, input: &mut Input) -> Result<Option<Chunk>, CodecError>;
    /// Reposition so the next chunk starts at a source frame at or before
    /// `frame`; returns that frame.
    fn seek(&mut self, input: &mut Input, frame: u64) -> Result<u64, CodecError>;
}

/// Every `CONCEAL_FRAMES`-frame block of a chunk holding a non-finite or
/// runaway sample is silenced, as [`super::Decoded::validated`] does for a
/// whole decode.
fn conceal(pcm: &mut [f32], channels: usize) {
    for block in pcm.chunks_mut(CONCEAL_FRAMES * channels.max(1)) {
        if block.iter().any(|v| !v.is_finite() || v.abs() > RUNAWAY) {
            block.fill(0.0);
        }
    }
}

/// A [`FrameSource`] as a [`PullDecoder`]: buffering, gapless trimming,
/// positions and the pre-roll a seek needs.
struct Pull<S: FrameSource> {
    input: Input,
    src: S,
    /// Decoded samples not yet handed out, from `pending_at`.
    pending: Vec<f32>,
    pending_at: usize,
    /// The source frame of `pending[pending_at]`.
    src_pos: u64,
    /// Drop source frames before this one (gapless start, or a seek target).
    discard_until: u64,
    skip: u64,
    total: Option<u64>,
    ended: bool,
}

impl<S: FrameSource> Pull<S> {
    fn new(input: Input, src: S) -> Self {
        let (skip, total) = src.trim();
        Self {
            input,
            src,
            pending: Vec::new(),
            pending_at: 0,
            src_pos: 0,
            discard_until: skip,
            skip,
            total,
            ended: false,
        }
    }

    /// One past the last source frame that plays.
    fn src_end(&self) -> u64 {
        self.total.map_or(u64::MAX, |t| self.skip + t)
    }
}

impl<S: FrameSource> PullDecoder for Pull<S> {
    fn channels(&self) -> u8 {
        u8::try_from(self.src.channels()).unwrap_or(u8::MAX)
    }

    fn rate(&self) -> u32 {
        self.src.rate()
    }

    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    fn read(&mut self, out: &mut [f32]) -> Result<usize, CodecError> {
        let ch = self.src.channels().max(1);
        let want_frames = out.len() / ch;
        let mut written = 0usize; // frames
        while written < want_frames {
            if self.pending_at >= self.pending.len() {
                if self.ended {
                    break;
                }
                match self.src.next(&mut self.input)? {
                    Some(mut c) => {
                        conceal(&mut c.pcm, ch);
                        self.pending = c.pcm;
                        self.pending_at = 0;
                    }
                    None => {
                        self.ended = true;
                        break;
                    }
                }
                continue;
            }
            let avail = (self.pending.len() - self.pending_at) / ch;
            // Before the first frame that plays: drop.
            if self.src_pos < self.discard_until {
                let drop_n = avail
                    .min(usize::try_from(self.discard_until - self.src_pos).unwrap_or(usize::MAX));
                self.pending_at += drop_n * ch;
                self.src_pos += drop_n as u64;
                continue;
            }
            let end = self.src_end();
            if self.src_pos >= end {
                self.ended = true;
                self.pending.clear();
                self.pending_at = 0;
                break;
            }
            let n = avail
                .min(want_frames - written)
                .min(usize::try_from(end - self.src_pos).unwrap_or(usize::MAX));
            let from = self.pending_at;
            out[written * ch..(written + n) * ch]
                .copy_from_slice(&self.pending[from..from + n * ch]);
            self.pending_at += n * ch;
            self.src_pos += n as u64;
            written += n;
        }
        Ok(written * ch)
    }

    fn seek(&mut self, frame: u64) -> Result<(), CodecError> {
        let target = self.skip + self.total.map_or(frame, |t| frame.min(t));
        let landed = self.src.seek(&mut self.input, target)?;
        self.pending.clear();
        self.pending_at = 0;
        self.src_pos = landed;
        self.discard_until = target;
        self.ended = false;
        Ok(())
    }

    fn position(&self) -> u64 {
        self.src_pos
            .max(self.discard_until)
            .saturating_sub(self.skip)
    }
}

// ── WAV ──────────────────────────────────────────────────────────────────

struct WavSource {
    info: crate::wav::WavInfo,
    data_off: u64,
    frames: u64,
    at: u64,
}

/// Frames a WAV chunk carries.
const WAV_CHUNK: u64 = 4096;

impl FrameSource for WavSource {
    fn channels(&self) -> usize {
        usize::from(self.info.channels)
    }

    fn rate(&self) -> u32 {
        self.info.rate
    }

    fn trim(&self) -> (u64, Option<u64>) {
        (0, Some(self.frames))
    }

    fn next(&mut self, input: &mut Input) -> Result<Option<Chunk>, CodecError> {
        if self.at >= self.frames {
            return Ok(None);
        }
        let ch = usize::from(self.info.channels);
        let width = self.info.format.bytes();
        let n = (self.frames - self.at).min(WAV_CHUNK);
        let bytes = usize::try_from(n).unwrap_or(0) * ch * width;
        let off = self.data_off + self.at * (ch * width) as u64;
        let raw = input.window(off, bytes)?;
        let whole = raw.len() / (ch * width) * (ch * width);
        let pcm = raw[..whole]
            .chunks_exact(width)
            .map(|b| self.info.format.decode_le(b))
            .collect::<Vec<_>>();
        if pcm.is_empty() {
            return Ok(None);
        }
        self.at += (pcm.len() / ch) as u64;
        Ok(Some(Chunk { pcm }))
    }

    fn seek(&mut self, _input: &mut Input, frame: u64) -> Result<u64, CodecError> {
        self.at = frame.min(self.frames);
        Ok(self.at)
    }
}

// ── AIFF ─────────────────────────────────────────────────────────────────

struct AiffSource {
    channels: usize,
    rate: u32,
    width: usize,
    decode_one: fn(&[u8]) -> f32,
    data_off: u64,
    frames: u64,
    at: u64,
}

impl AiffSource {
    fn open(input: &mut Input) -> Result<Self, CodecError> {
        let head = input.window(0, 12)?.to_vec();
        if head.len() < 12 || &head[..4] != b"FORM" {
            return Err(CodecError::NotThisFormat);
        }
        let aifc = match &head[8..12] {
            b"AIFF" => false,
            b"AIFC" => true,
            _ => return Err(CodecError::NotThisFormat),
        };
        let len = input.len();
        let mut comm = None;
        let mut ssnd = None;
        let mut at = 12u64;
        while at + 8 <= len {
            let h = input.window(at, 8)?.to_vec();
            if h.len() < 8 {
                break;
            }
            let size = u64::from(u32::from_be_bytes([h[4], h[5], h[6], h[7]]));
            match &h[..4] {
                b"COMM" => {
                    let n = usize::try_from(size.min(64)).unwrap_or(64);
                    comm = Some(input.window(at + 8, n)?.to_vec());
                }
                b"SSND" => {
                    let o = input.window(at + 8, 8)?.to_vec();
                    if o.len() < 8 {
                        return Err(CodecError::Truncated);
                    }
                    let offset = u64::from(u32::from_be_bytes([o[0], o[1], o[2], o[3]]));
                    let start = at + 16 + offset;
                    let end = (at + 8 + size).min(len);
                    ssnd = Some((start, end.saturating_sub(start)));
                }
                _ => {}
            }
            at += 8 + size + (size & 1);
        }
        let c = comm.ok_or(CodecError::Malformed("no COMM chunk"))?;
        let (channels, frames, bits, rate, kind) = super::aiff::parse_comm(&c, aifc)?;
        let (data_off, data_len) = ssnd.ok_or(CodecError::Malformed("no SSND chunk"))?;
        let (width, decode_one) = super::aiff::sample_decoder(kind, bits)?;
        let ch = usize::from(channels.max(1));
        Ok(Self {
            channels: ch,
            rate,
            width,
            decode_one,
            data_off,
            frames: u64::from(frames).min(data_len / (ch * width) as u64),
            at: 0,
        })
    }
}

impl FrameSource for AiffSource {
    fn channels(&self) -> usize {
        self.channels
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn trim(&self) -> (u64, Option<u64>) {
        (0, Some(self.frames))
    }

    fn next(&mut self, input: &mut Input) -> Result<Option<Chunk>, CodecError> {
        if self.at >= self.frames {
            return Ok(None);
        }
        let fb = self.channels * self.width;
        let n = (self.frames - self.at).min(WAV_CHUNK);
        let off = self.data_off + self.at * fb as u64;
        let raw = input.window(off, usize::try_from(n).unwrap_or(0) * fb)?;
        let whole = raw.len() / fb * fb;
        let pcm: Vec<f32> = raw[..whole]
            .chunks_exact(self.width)
            .map(self.decode_one)
            .collect();
        if pcm.is_empty() {
            return Ok(None);
        }
        self.at += (pcm.len() / self.channels) as u64;
        Ok(Some(Chunk { pcm }))
    }

    fn seek(&mut self, _input: &mut Input, frame: u64) -> Result<u64, CodecError> {
        self.at = frame.min(self.frames);
        Ok(self.at)
    }
}

/// Open `r` for decoding a piece at a time.
///
/// # Errors
/// Not a format this crate decodes, or a header it refuses.
pub fn open(r: Box<dyn ReadSeek>) -> Result<Box<dyn PullDecoder>, CodecError> {
    let mut input = Input::new(r)?;
    let head = input.window(0, 64 << 10)?.to_vec();
    let format = super::sniff(&head).ok_or(CodecError::NotThisFormat)?;
    Ok(match format {
        Format::Wav => {
            let (info, data_off, data_len) = crate::wav::read_wav_header(&head, input.len())
                .map_err(|_| CodecError::Malformed("WAV header"))?;
            if !super::RATE_RANGE.contains(&info.rate)
                || info.channels == 0
                || info.channels > u16::from(super::MAX_CHANNELS)
            {
                return Err(CodecError::Malformed("WAV shape"));
            }
            let frame = (info.format.bytes() * usize::from(info.channels)) as u64;
            Box::new(Pull::new(
                input,
                WavSource {
                    info,
                    data_off,
                    frames: data_len / frame.max(1),
                    at: 0,
                },
            ))
        }
        Format::Flac => Box::new(Pull::new_with(input, super::flac::FlacSource::open)?),
        Format::Mp3 => Box::new(Pull::new_with(input, super::mp3::Mp3Source::open)?),
        #[cfg(feature = "aac")]
        Format::Mp4 => Box::new(Pull::new_with(input, super::aac::Mp4Source::open)?),
        #[cfg(feature = "aac")]
        Format::Adts => Box::new(Pull::new_with(input, super::aac::AdtsSource::open)?),
        Format::Aiff => Box::new(Pull::new_with(input, AiffSource::open)?),
        Format::Ogg => Box::new(Pull::new_with(input, super::vorbis::VorbisSource::open)?),
        #[allow(unreachable_patterns, reason = "every format streams when aac is on")]
        _ => return Err(CodecError::Unsupported("streaming this format")),
    })
}

impl<S: FrameSource> Pull<S> {
    fn new_with(
        mut input: Input,
        open: impl FnOnce(&mut Input) -> Result<S, CodecError>,
    ) -> Result<Self, CodecError> {
        let src = open(&mut input)?;
        if !super::RATE_RANGE.contains(&src.rate())
            || src.channels() == 0
            || src.channels() > usize::from(super::MAX_CHANNELS)
        {
            return Err(CodecError::Malformed("stream shape"));
        }
        Ok(Self::new(input, src))
    }
}

/// Open a file for decoding a piece at a time.
///
/// # Errors
/// The file cannot be opened, or [`open`]'s.
pub fn open_file(path: impl AsRef<std::path::Path>) -> Result<Box<dyn PullDecoder>, CodecError> {
    let f = std::fs::File::open(path.as_ref()).map_err(|e| io(&e))?;
    open(Box::new(std::io::BufReader::with_capacity(64 << 10, f)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_bounded_and_correct() {
        let data: Vec<u8> = (0..4_000_000u32).map(|i| (i % 251) as u8).collect();
        let mut input = Input::new(Box::new(std::io::Cursor::new(data.clone()))).unwrap();
        let mut at = 0u64;
        while at < data.len() as u64 {
            let w = input.window(at, 3000).unwrap().to_vec();
            assert_eq!(w, data[at as usize..(at as usize + 3000).min(data.len())]);
            assert!(input.held() <= KEEP + READ + 3000, "held {}", input.held());
            at += 2500;
        }
        // Backwards: a fresh read.
        assert_eq!(input.window(10, 4).unwrap(), &data[10..14]);
        // Past the end: empty.
        assert!(input.window(data.len() as u64 + 5, 4).unwrap().is_empty());
    }
}
