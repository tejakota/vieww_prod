//! Key estimation from a chroma vector (AU-22).
//!
//! [`estimate_key`] scores the 24 major and minor keys by correlating the
//! chroma with the Krumhansl–Kessler probe-tone profiles, the standard
//! method behind most key finders. [`estimate_melakarta`] does the same for
//! the 72 melakartas on every tonic. It uses scale templates that weight Sa
//! and Pa, since a Carnatic performance sounds them in the drone. As far as
//! we know, no other key finder offers this.
//!
//! The chroma comes from
//! [`vieww_audio::analysis::chroma`]: C = 0 … B = 11.

use crate::carnatic::melakarta::Melakarta;

/// Krumhansl & Kessler (1982) major-key profile, tonic first.
const MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// Krumhansl & Kessler (1982) minor-key profile, tonic first.
const MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Major or minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Major.
    Major,
    /// Minor.
    Minor,
}

/// A key and how well it fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEstimate {
    /// Tonic pitch class, C = 0.
    pub tonic: u8,
    /// Major or minor.
    pub mode: Mode,
    /// Pearson correlation with the key's profile, −1..=1.
    pub score: f32,
}

/// A melakarta on a tonic, and how well it fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MelakartaEstimate {
    /// Sa's pitch class, C = 0.
    pub tonic: u8,
    /// The scale.
    pub melakarta: Melakarta,
    /// Pearson correlation with the scale's template, −1..=1.
    pub score: f32,
}

fn pearson(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let ma = a.iter().sum::<f32>() / 12.0;
    let mb = b.iter().sum::<f32>() / 12.0;
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let (x, y) = (a[i] - ma, b[i] - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da * db).sqrt()
    }
}

/// `profile` (tonic first) moved so that its tonic sits on `tonic`.
fn rotated(profile: &[f32; 12], tonic: usize) -> [f32; 12] {
    let mut out = [0.0; 12];
    for (i, v) in profile.iter().enumerate() {
        out[(i + tonic) % 12] = *v;
    }
    out
}

/// The best-fitting major or minor key for `chroma`.
#[must_use]
pub fn estimate_key(chroma: &[f32; 12]) -> KeyEstimate {
    let mut best = KeyEstimate {
        tonic: 0,
        mode: Mode::Major,
        score: f32::NEG_INFINITY,
    };
    for tonic in 0..12u8 {
        for (mode, profile) in [(Mode::Major, &MAJOR), (Mode::Minor, &MINOR)] {
            let score = pearson(chroma, &rotated(profile, usize::from(tonic)));
            if score > best.score {
                best = KeyEstimate { tonic, mode, score };
            }
        }
    }
    best
}

/// A melakarta's template, Sa first: its seven swaras, with Sa and Pa
/// weighted the way a drone sounds them.
fn template(m: Melakarta) -> [f32; 12] {
    let mut t = [0.0f32; 12];
    for s in m.semitones() {
        t[usize::from(s % 12)] = 1.0;
    }
    t[0] = 2.0;
    t[7] = t[7].max(1.5);
    t
}

/// The best-fitting melakarta and tonic for `chroma`.
#[must_use]
pub fn estimate_melakarta(chroma: &[f32; 12]) -> MelakartaEstimate {
    let mut best: Option<MelakartaEstimate> = None;
    for m in Melakarta::all() {
        let t = template(m);
        for tonic in 0..12u8 {
            let score = pearson(chroma, &rotated(&t, usize::from(tonic)));
            if best.is_none_or(|b| score > b.score) {
                best = Some(MelakartaEstimate {
                    tonic,
                    melakarta: m,
                    score,
                });
            }
        }
    }
    best.expect("72 melakartas")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_audio::analysis::{chroma, Spectrum, Window};
    use vieww_audio::Samples;

    /// Chroma of real audio: each pitch class in `notes` as a sine (octave
    /// 4), weighted.
    fn chroma_of(notes: &[(u8, f32)]) -> [f32; 12] {
        let rate = 44_100u32;
        let mut x = vec![0.0f32; 32_768];
        for &(pc, w) in notes {
            let hz = 261.63 * 2f32.powf(f32::from(pc) / 12.0);
            for (i, v) in x.iter_mut().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "a sample index")]
                let t = i as f32 / rate as f32;
                *v += w * (std::f32::consts::TAU * hz * t).sin();
            }
        }
        chroma(&Spectrum::analyze(&Samples::mono(x, rate), Window::Hann))
    }

    #[test]
    fn a_major_scale_with_its_tonic_stressed_is_its_major_key() {
        // G major: G A B C D E F#, tonic and dominant heavier.
        let c = chroma_of(&[
            (7, 1.0),
            (9, 0.5),
            (11, 0.6),
            (0, 0.5),
            (2, 0.8),
            (4, 0.5),
            (6, 0.4),
        ]);
        let k = estimate_key(&c);
        assert_eq!((k.tonic, k.mode), (7, Mode::Major), "{k:?}");
    }

    #[test]
    fn a_minor_scale_with_its_tonic_stressed_is_its_minor_key() {
        // A harmonic minor: A B C D E F G#.
        let c = chroma_of(&[
            (9, 1.0),
            (11, 0.4),
            (0, 0.7),
            (2, 0.5),
            (4, 0.8),
            (5, 0.4),
            (8, 0.4),
        ]);
        let k = estimate_key(&c);
        assert_eq!((k.tonic, k.mode), (9, Mode::Minor), "{k:?}");
    }

    #[test]
    fn mayamalavagowla_over_a_drone_on_d_is_found() {
        let m = Melakarta::by_name("Mayamalavagowla").expect("15");
        let tonic = 2u8; // D
        let mut notes: Vec<(u8, f32)> = m
            .semitones()
            .iter()
            .map(|s| ((tonic + s) % 12, 0.5))
            .collect();
        // The drone: Sa and Pa.
        notes.push((tonic, 1.2));
        notes.push(((tonic + 7) % 12, 0.8));
        let e = estimate_melakarta(&chroma_of(&notes));
        assert_eq!((e.tonic, e.melakarta), (tonic, m), "{e:?}");
    }
}
