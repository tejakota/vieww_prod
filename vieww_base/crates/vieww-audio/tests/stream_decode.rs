//! AU-05: streaming decode matches whole-file decode, and seeks land.
use vieww_audio::codec::{decode, stream};

const FIXTURES: &[&str] = &[
    "source16.wav",
    "source24m.wav",
    "src16.flac",
    "src24m.flac",
    "src.mp3",
    "src_mono_vbr.mp3",
    "src.m4a",
    "src_mono.aac",
    "src.ogg",
    "src24.aiff",
];

fn path(n: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(n)
}

fn drain(d: &mut dyn stream::PullDecoder) -> Vec<f32> {
    let mut out = Vec::new();
    let mut buf = vec![0f32; 1000 * usize::from(d.channels())];
    loop {
        let n = d.read(&mut buf).expect("read");
        if n == 0 {
            return out;
        }
        out.extend_from_slice(&buf[..n]);
    }
}

#[test]
fn streaming_matches_whole_file() {
    for f in FIXTURES {
        let whole = decode(&std::fs::read(path(f)).unwrap()).unwrap();
        let mut s = stream::open_file(path(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(s.channels(), whole.channels, "{f}");
        assert_eq!(s.rate(), whole.rate, "{f}");
        let got = drain(&mut *s);
        assert_eq!(got.len(), whole.data.len(), "{f}: length");
        let worst = got
            .iter()
            .zip(&whole.data)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst < 1e-4, "{f}: max diff {worst}");
    }
}

#[test]
fn seeks_land_on_the_same_samples() {
    for f in FIXTURES {
        let whole = decode(&std::fs::read(path(f)).unwrap()).unwrap();
        let ch = usize::from(whole.channels);
        let frames = (whole.data.len() / ch) as u64;
        let mut s = stream::open_file(path(f)).unwrap();
        for t in [frames / 2, frames / 5, frames * 4 / 5, 0] {
            s.seek(t).unwrap();
            assert_eq!(s.position(), t, "{f}");
            let mut buf = vec![0f32; 256 * ch];
            let n = s.read(&mut buf).unwrap();
            let at = t as usize * ch;
            let worst = buf[..n]
                .iter()
                .zip(&whole.data[at..])
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(worst < 2e-3, "{f} @ {t}: max diff {worst}");
        }
    }
}

/// Counts the bytes a decoder reads.
struct Counting {
    inner: std::io::Cursor<Vec<u8>>,
    read: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl std::io::Read for Counting {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read
            .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

impl std::io::Seek for Counting {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

#[test]
fn a_late_seek_does_not_read_the_whole_file() {
    use std::sync::atomic::Ordering;
    // Ogg pages are found by bisection, MP3/ADTS frames from an index or a
    // header walk, FLAC frames by bisection, MP4 units from the sample
    // table: none of them decodes from the start to reach the end.
    for f in ["long.ogg", "long.flac", "long.m4a"] {
        let bytes = std::fs::read(path(f)).unwrap();
        let len = bytes.len() as u64;
        let read = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut s = stream::open(Box::new(Counting {
            inner: std::io::Cursor::new(bytes),
            read: read.clone(),
        }))
        .unwrap();
        let total = s.total_frames().expect(f);
        let before = read.load(Ordering::Relaxed);
        s.seek(total * 9 / 10).unwrap();
        let mut buf = vec![0f32; 512 * usize::from(s.channels())];
        s.read(&mut buf).unwrap();
        let used = read.load(Ordering::Relaxed) - before;
        assert!(
            used < len / 2,
            "{f}: a seek to 90% read {used} of {len} bytes"
        );
    }
}

/// A 16-bit stereo WAV generated on the fly, never stored: sample `i` of
/// channel `c` is a function of `i` and `c`, so any read can be checked.
struct SyntheticWav {
    frames: u64,
    pos: u64,
}

const SYN_RATE: u32 = 48_000;

fn syn_sample(frame: u64, ch: u64) -> i16 {
    // A pattern that changes every frame and differs per channel.
    let v = (frame.wrapping_mul(2_654_435_761) >> 7) ^ (ch * 0x5555);
    (v & 0x7FFF) as i16 - 0x4000
}

impl SyntheticWav {
    fn len(&self) -> u64 {
        44 + self.frames * 4
    }

    fn byte(&self, at: u64) -> u8 {
        if at < 44 {
            let data = (self.frames * 4) as u32;
            let mut h = Vec::with_capacity(44);
            h.extend_from_slice(b"RIFF");
            h.extend_from_slice(&(36 + data).to_le_bytes());
            h.extend_from_slice(b"WAVEfmt ");
            h.extend_from_slice(&16u32.to_le_bytes());
            h.extend_from_slice(&1u16.to_le_bytes());
            h.extend_from_slice(&2u16.to_le_bytes());
            h.extend_from_slice(&SYN_RATE.to_le_bytes());
            h.extend_from_slice(&(SYN_RATE * 4).to_le_bytes());
            h.extend_from_slice(&4u16.to_le_bytes());
            h.extend_from_slice(&16u16.to_le_bytes());
            h.extend_from_slice(b"data");
            h.extend_from_slice(&data.to_le_bytes());
            return h[at as usize];
        }
        let off = at - 44;
        let (frame, rest) = (off / 4, off % 4);
        syn_sample(frame, rest / 2).to_le_bytes()[(rest % 2) as usize]
    }
}

impl std::io::Read for SyntheticWav {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = (self.len().saturating_sub(self.pos)).min(buf.len() as u64) as usize;
        for (i, b) in buf[..n].iter_mut().enumerate() {
            *b = self.byte(self.pos + i as u64);
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for SyntheticWav {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.pos = match to {
            std::io::SeekFrom::Start(p) => p,
            std::io::SeekFrom::End(d) => (self.len() as i64 + d) as u64,
            std::io::SeekFrom::Current(d) => (self.pos as i64 + d) as u64,
        };
        Ok(self.pos)
    }
}

/// Resident memory, bytes (Linux; `None` elsewhere).
fn rss() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// AU-05's large-media contract: a three-hour file (2 GB of PCM) streams in
/// bounded memory through a storm of seeks, every read landing on the
/// right samples.
#[test]
fn a_three_hour_stream_stays_small_through_a_seek_storm() {
    let frames = 3 * 3600 * u64::from(SYN_RATE);
    let mut s = stream::open(Box::new(SyntheticWav { frames, pos: 0 })).unwrap();
    assert_eq!(s.total_frames(), Some(frames));
    let before = rss();
    let mut buf = vec![0f32; 4096 * 2];
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..200 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let at = x % (frames - 10_000);
        s.seek(at).unwrap();
        let n = s.read(&mut buf).unwrap() / 2;
        assert!(n > 0);
        for k in [0, n / 2, n - 1] {
            for c in 0..2 {
                let want = f32::from(syn_sample(at + k as u64, c as u64)) / 32_768.0;
                let got = buf[k * 2 + c];
                assert!(
                    (got - want).abs() < 1e-3,
                    "frame {} ch {c}: {got} vs {want}",
                    at + k as u64
                );
            }
        }
    }
    // And a long straight read: a minute, start to end of the window.
    s.seek(frames / 2).unwrap();
    for _ in 0..(60 * SYN_RATE as usize / 4096) {
        assert!(s.read(&mut buf).unwrap() > 0);
    }
    if let (Some(a), Some(b)) = (before, rss()) {
        let grew = b.saturating_sub(a);
        eprintln!(
            "rss grew {} KB over 200 seeks and a minute of reading",
            grew / 1024
        );
        assert!(grew < 32 << 20, "resident memory grew {grew} bytes");
    }
}
