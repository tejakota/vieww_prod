//! LilyPond out (AU-16): the engraver's input language, for publication
//! quality scores (`lilypond score.ly` → PDF and MIDI).
//!
//! Conventions match [`super::abc`]: time in sixteenths, `\time 4/4` with
//! notes that cross a barline split and tied (`~`), one voice per
//! non-overlapping line of each part (`<< { } \\ { } >>` inside the
//! part's staff), chords as `< >`. Pitches are the sounding pitch
//! rounded to the nearest semitone with sharps (`cis`), absolute octaves
//! (`c'` is middle C); a note more than 10 cents off carries its offset as
//! a markup above it (`^"+12c"`). Durations LilyPond cannot write as one
//! note (5, 7, 9… sixteenths) are tied sums. Each staff gets a clef from
//! its range. Gamakas are not written; for exact pitch, use
//! [`super::musicxml`].

use crate::score::{Instrument, Score};

const UNIT: f64 = 4.0; // sixteenths per beat
const BAR: i64 = 16;

fn pitch_name(pitch: i64) -> String {
    const NAMES: [&str; 12] = [
        "c", "cis", "d", "dis", "e", "f", "fis", "g", "gis", "a", "ais", "b",
    ];
    let mut s = NAMES[usize::try_from(pitch.rem_euclid(12)).unwrap_or(0)].to_owned();
    // LilyPond's absolute octaves: `c` is C3, `c'` C4, `c,` C2.
    let octave = pitch.div_euclid(12) - 1;
    match octave - 3 {
        n if n > 0 => s.push_str(&"'".repeat(usize::try_from(n).unwrap_or(0))),
        n if n < 0 => s.push_str(&",".repeat(usize::try_from(-n).unwrap_or(0))),
        _ => {}
    }
    s
}

/// One duration in sixteenths as LilyPond durations, longest first; more
/// than one means tie them.
fn durations(units: i64) -> Vec<&'static str> {
    const ONE: [(i64, &str); 8] = [
        (16, "1"),
        (12, "2."),
        (8, "2"),
        (6, "4."),
        (4, "4"),
        (3, "8."),
        (2, "8"),
        (1, "16"),
    ];
    let mut left = units;
    let mut out = Vec::new();
    while left > 0 {
        let (n, d) = ONE
            .iter()
            .find(|(n, _)| *n <= left)
            .copied()
            .unwrap_or((1, "16"));
        out.push(d);
        left -= n;
    }
    out
}

#[derive(Clone)]
struct N {
    start: i64,
    len: i64,
    pitch: f64,
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The score as a LilyPond file.
#[must_use]
pub fn to_lilypond(score: &Score) -> String {
    let mut out = format!(
        "\\version \"2.24.0\"\n\\header {{\n  title = \"{}\"\n  tagline = ##f\n}}\n\n\\score {{\n  <<\n",
        escape(&score.title.replace('\n', " "))
    );
    #[expect(clippy::cast_possible_truncation, reason = "a time in sixteenths")]
    let units = |beats: f64| (beats * UNIT).round() as i64;
    let mut first = true;
    for part in &score.parts {
        let drums = matches!(part.instrument, Instrument::Drums | Instrument::Mridangam);
        let mut notes: Vec<N> = part
            .events
            .iter()
            .filter(|e| e.length > 0.0)
            .map(|e| {
                let pitch = if drums {
                    f64::from(e.pitch)
                } else {
                    let hz = f64::from(score.tuning.hz(e.pitch));
                    69.0 + 12.0 * (hz / 440.0).log2()
                };
                N {
                    start: units(e.start),
                    len: units(e.length).max(1),
                    pitch,
                }
            })
            .collect();
        if notes.is_empty() {
            continue;
        }
        notes.sort_by(|a, b| a.start.cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
        let mean = notes.iter().map(|n| n.pitch).sum::<f64>() / notes.len() as f64;
        let clef = if mean < 55.0 { "bass" } else { "treble" };
        let mut voices: Vec<Vec<Vec<N>>> = Vec::new();
        for n in notes {
            let slot = voices.iter_mut().find(|v| {
                v.last().is_some_and(|c: &Vec<N>| {
                    let l = &c[0];
                    (l.start == n.start && l.len == n.len) || l.start + l.len <= n.start
                })
            });
            match slot {
                Some(v) => {
                    let same = v
                        .last()
                        .is_some_and(|c| c[0].start == n.start && c[0].len == n.len);
                    if same {
                        v.last_mut().expect("checked").push(n);
                    } else {
                        v.push(vec![n]);
                    }
                }
                None => voices.push(vec![vec![n]]),
            }
        }
        out.push_str(&format!(
            "    \\new Staff \\with {{ instrumentName = \"{}\" }} {{\n      \\clef {clef}\n      \\time 4/4\n",
            escape(&part.name)
        ));
        if first {
            out.push_str(&format!("      \\tempo 4 = {}\n", score.bpm.round()));
            first = false;
        }
        let bodies: Vec<String> = voices.iter().map(|chords| voice_body(chords)).collect();
        if bodies.len() == 1 {
            out.push_str(&format!("      {{ {} }}\n", bodies[0]));
        } else {
            out.push_str("      <<\n");
            for (k, b) in bodies.iter().enumerate() {
                let sep = if k + 1 < bodies.len() { " \\\\" } else { "" };
                out.push_str(&format!("        {{ {b} }}{sep}\n"));
            }
            out.push_str("      >>\n");
        }
        out.push_str("    }\n");
    }
    out.push_str("  >>\n  \\layout { }\n  \\midi { }\n}\n");
    out
}

fn voice_body(chords: &[Vec<N>]) -> String {
    let mut body = String::new();
    let mut cursor = 0i64;
    // Write `len` sixteenths of `token(duration)`, split at barlines and
    // into writable durations, tying the pieces when `tie`.
    let emit = |body: &mut String,
                cursor: &mut i64,
                len: i64,
                token: &dyn Fn(&str) -> String,
                tie: bool| {
        let mut left = len;
        while left > 0 {
            let room = BAR - cursor.rem_euclid(BAR);
            let piece = left.min(room);
            left -= piece;
            let ds = durations(piece);
            for (i, d) in ds.iter().enumerate() {
                body.push_str(&token(d));
                let more = i + 1 < ds.len() || left > 0;
                if tie && more {
                    body.push('~');
                }
                body.push(' ');
            }
            *cursor += piece;
            if cursor.rem_euclid(BAR) == 0 {
                body.push_str("| ");
            }
        }
    };
    for chord in chords {
        let start = chord[0].start;
        if start > cursor {
            let gap = start - cursor;
            emit(&mut body, &mut cursor, gap, &|d| format!("r{d}"), false);
        }
        let mut cents = String::new();
        let names: Vec<String> = chord
            .iter()
            .map(|n| {
                #[expect(clippy::cast_possible_truncation, reason = "a MIDI pitch")]
                let rounded = n.pitch.round() as i64;
                let c = (n.pitch - n.pitch.round()) * 100.0;
                if cents.is_empty() && c.abs() > 10.0 {
                    cents = format!("^\"{c:+.0}c\"");
                }
                pitch_name(rounded)
            })
            .collect();
        let pitch = if names.len() == 1 {
            names[0].clone()
        } else {
            format!("<{}>", names.join(" "))
        };
        // The cents markup goes on the chord's first written note only.
        let first = std::cell::Cell::new(true);
        let len = chord[0].len;
        emit(
            &mut body,
            &mut cursor,
            len,
            &|d| {
                let mark = if first.replace(false) {
                    cents.as_str()
                } else {
                    ""
                };
                format!("{pitch}{d}{mark}")
            },
            true,
        );
    }
    body.trim_end().trim_end_matches('|').trim_end().to_owned() + " \\bar \"|.\""
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Event;

    #[test]
    fn pitches_and_durations() {
        assert_eq!(pitch_name(60), "c'");
        assert_eq!(pitch_name(48), "c");
        assert_eq!(pitch_name(36), "c,");
        assert_eq!(pitch_name(73), "cis''");
        assert_eq!(durations(16), ["1"]);
        assert_eq!(durations(6), ["4."]);
        assert_eq!(durations(5), ["4", "16"]);
        assert_eq!(durations(7), ["4.", "16"]);
    }

    #[test]
    fn a_small_tune_is_written_as_lilypond() {
        let mut s = Score::new("Little Tune", 100.0);
        let p = s.part("Flute", Instrument::Reed);
        p.push(Event::new(0.0, 1.0, 60.0, 0.8)); // C4 quarter
        p.push(Event::new(1.0, 0.5, 62.0, 0.8)); // D4 eighth
        p.push(Event::new(2.0, 3.0, 73.0, 0.8)); // C#5, across the barline
        let ly = to_lilypond(&s);
        assert!(ly.starts_with("\\version \"2.24.0\""), "{ly}");
        assert!(ly.contains("title = \"Little Tune\""), "{ly}");
        assert!(ly.contains("instrumentName = \"Flute\""), "{ly}");
        assert!(ly.contains("\\tempo 4 = 100"), "{ly}");
        assert!(ly.contains("c'4 d'8 r8 cis''2~ | cis''4"), "{ly}");
        assert!(ly.contains("\\bar \"|.\""), "{ly}");
    }

    #[test]
    fn chords_voices_clefs_and_microtones() {
        let mut s = Score::new("Chords", 60.0);
        let p = s.part("Pad", Instrument::Pad);
        for k in [48.0, 52.0, 55.0] {
            p.push(Event::new(0.0, 4.0, k, 0.5));
        }
        p.push(Event::new(1.0, 1.0, 84.0, 0.5)); // overlapping: a second voice
        p.push(Event::new(2.0, 1.0, 64.0 - 0.1369, 0.5)); // a just third
        let ly = to_lilypond(&s);
        assert!(ly.contains("<c e g>1"), "{ly}");
        assert!(ly.contains(" \\\\\n"), "two voices: {ly}");
        assert!(ly.contains("c'''4"), "C6 is c''': {ly}");
        assert!(ly.contains("e'4^\"-14c\""), "{ly}");
    }

    #[test]
    fn every_demo_piece_exports() {
        for score in [
            crate::pieces::rock(),
            crate::pieces::chorale(true),
            crate::pieces::carnatic(1).0,
        ] {
            let ly = to_lilypond(&score);
            assert_eq!(
                ly.matches('{').count(),
                ly.matches('}').count(),
                "{}",
                score.title
            );
            assert!(ly.contains("\\new Staff"), "{}", score.title);
        }
    }
}
