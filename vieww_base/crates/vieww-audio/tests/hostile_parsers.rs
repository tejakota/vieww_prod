//! AU-20 on the stable toolchain: every parser that reads an untrusted
//! file (SMF, binary presets, SFZ text, SoundFonts) is fed thousands of
//! mutations of a valid seed. A panic fails the test. The cargo-fuzz
//! targets in `fuzz/` go deeper; this runs in every `cargo test`.

use std::collections::BTreeMap;
use std::sync::Arc;

use vieww_audio::midi::{Message, Smf, TrackEvent};
use vieww_audio::preset::State;
use vieww_audio::synth::sampler::SampleData;

/// Deterministic mutations: flip, overwrite, insert, delete, truncate.
fn mutations(seed: &[u8], count: usize) -> impl Iterator<Item = Vec<u8>> + '_ {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    (0..count).map(move |_| {
        let mut v = seed.to_vec();
        for _ in 0..1 + next() % 4 {
            let len = v.len().max(1);
            #[expect(clippy::cast_possible_truncation, reason = "an index")]
            let at = (next() % len as u64) as usize;
            #[expect(clippy::cast_possible_truncation, reason = "a byte")]
            let byte = next() as u8;
            let last = v.len().saturating_sub(1);
            match next() % 5 {
                0 if !v.is_empty() => v[at.min(last)] ^= 1 << (byte % 8),
                1 if !v.is_empty() => v[at.min(last)] = byte,
                2 => v.insert(at.min(v.len()), byte),
                3 if !v.is_empty() => {
                    v.remove(at.min(last));
                }
                _ => v.truncate(at),
            }
        }
        v
    })
}

#[test]
fn midi_files_survive_mutation() {
    let mut smf = Smf::new(480);
    smf.tracks.push(vec![
        (0, TrackEvent::Tempo(500_000)),
        (0, TrackEvent::TrackName("seed".into())),
        (
            0,
            TrackEvent::Midi(Message::NoteOn {
                channel: 0,
                key: 60,
                velocity: 100,
            }),
        ),
        (
            480,
            TrackEvent::Midi(Message::NoteOff {
                channel: 0,
                key: 60,
                velocity: 64,
            }),
        ),
    ]);
    let seed = smf.to_bytes();
    assert!(Smf::parse(&seed).is_ok(), "the seed is valid");
    for m in mutations(&seed, 20_000) {
        if let Ok(s) = Smf::parse(&m) {
            let _ = Smf::parse(&s.to_bytes());
        }
    }
}

#[test]
fn preset_states_survive_mutation() {
    let mut params = BTreeMap::new();
    params.insert(1, 0.5);
    params.insert(7, -12.0);
    let mut blobs = BTreeMap::new();
    blobs.insert("wavetable".into(), vec![1, 2, 3, 4]);
    let seed = State {
        name: "Seed".into(),
        version: 3,
        params,
        blobs,
    }
    .to_bytes();
    assert!(State::from_bytes(&seed).is_ok());
    for m in mutations(&seed, 20_000) {
        if let Ok(s) = State::from_bytes(&m) {
            let b = s.to_bytes();
            assert_eq!(State::from_bytes(&b).map(|s| s.to_bytes()), Ok(b));
        }
    }
}

#[test]
fn sfz_text_survives_mutation() {
    let seed = b"<control> default_path=samples/\n<global> ampeg_release=0.3\n<group> lokey=36 hikey=60 lovel=1 hivel=127\n<region> sample=a.wav pitch_keycenter=48 loop_mode=loop_continuous loop_start=0 loop_end=3\n<region> sample=b.wav key=61 tune=-12 volume=-6 pan=20\n";
    let sample = Arc::new(SampleData::mono(vec![0.0, 0.5, -0.5, 0.0], 48_000.0));
    let text = String::from_utf8_lossy(seed);
    assert!(vieww_audio::synth::sfz::parse(&text, |_| Some(Arc::clone(&sample))).is_ok());
    for m in mutations(seed, 20_000) {
        let text = String::from_utf8_lossy(&m);
        let _ = vieww_audio::synth::sfz::parse(&text, |_| Some(Arc::clone(&sample)));
    }
}

#[test]
fn soundfont_headers_survive_mutation() {
    // A RIFF/sfbk skeleton with empty INFO, sdta and pdta lists: enough to
    // reach the chunk walker on every mutation.
    let list = |kind: &[u8], body: &[u8]| {
        let mut v = b"LIST".to_vec();
        v.extend(u32::try_from(body.len() + 4).unwrap().to_le_bytes());
        v.extend(kind);
        v.extend(body);
        v
    };
    let mut body = b"sfbk".to_vec();
    body.extend(list(b"INFO", b"ifil\x04\x00\x00\x00\x02\x00\x01\x00"));
    body.extend(list(
        b"sdta",
        b"smpl\x08\x00\x00\x00\x00\x00\x10\x00\xf0\xff\x00\x00",
    ));
    body.extend(list(b"pdta", b""));
    let mut seed = b"RIFF".to_vec();
    seed.extend(u32::try_from(body.len()).unwrap().to_le_bytes());
    seed.extend(body);
    for m in mutations(&seed, 20_000) {
        if let Ok(mut sf) = vieww_audio::synth::sf2::SoundFont::parse(&m) {
            let _ = sf.instrument(0, 0);
        }
    }
}
