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
