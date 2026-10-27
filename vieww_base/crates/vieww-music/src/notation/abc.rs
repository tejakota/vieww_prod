//! ABC notation out (AU-16): the plain-text format folk and session
//! musicians trade tunes in, read by abcjs, EasyABC and MuseScore's
//! importer.
//!
//! Conventions: `L:1/16` (the unit is a sixteenth note), `M:4/4` with a
//! barline every bar, notes that cross a barline split and tied (`-`),
//! one `V:` voice per non-overlapping line of each part (chords as
//! `[…]`). Pitches are the sounding pitch rounded to the nearest
//! semitone, written with sharps. ABC has no microtones, so a note more
//! than 10 cents off carries a `"+12c"` style annotation above it. Gamakas
//! are not written. For exact pitch, use [`super::musicxml`].

use crate::score::{Instrument, Score};

const UNIT: f64 = 4.0; // sixteenths per beat
const BAR: i64 = 16;

fn note_name(pitch: i64) -> String {
    const NAMES: [(&str, bool); 12] = [
        ("C", false),
        ("C", true),
        ("D", false),
        ("D", true),
        ("E", false),
        ("F", false),
        ("F", true),
        ("G", false),
        ("G", true),
        ("A", false),
        ("A", true),
        ("B", false),
    ];
    let (name, sharp) = NAMES[usize::try_from(pitch.rem_euclid(12)).unwrap_or(0)];
    let octave = pitch.div_euclid(12) - 1; // MIDI 60 = C4
    let mut s = String::new();
    if sharp {
        s.push('^');
    }
    if octave >= 5 {
        s.push_str(&name.to_ascii_lowercase());
        for _ in 5..octave {
            s.push('\'');
        }
    } else {
        s.push_str(name);
        for _ in octave..4 {
            s.push(',');
        }
    }
    s
}

fn length(units: i64) -> String {
    match units {
        1 => String::new(),
        n => n.to_string(),
    }
}

#[derive(Clone)]
struct N {
    start: i64,
    len: i64,
    pitch: f64,
}

/// The score as an ABC tune.
#[must_use]
pub fn to_abc(score: &Score) -> String {
    let mut out = format!(
        "X:1\nT:{}\nM:4/4\nL:1/16\nQ:1/4={}\nK:C\n",
        score.title.replace('\n', " "),
        score.bpm.round()
    );
    #[expect(clippy::cast_possible_truncation, reason = "a time in sixteenths")]
    let units = |beats: f64| (beats * UNIT).round() as i64;
    let mut voice_no = 0;
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
        notes.sort_by(|a, b| a.start.cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
        // Voices: chords stay together, overlaps move to a new voice.
        let mut voices: Vec<Vec<Vec<N>>> = Vec::new(); // voice → chords
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
        for (k, chords) in voices.iter().enumerate() {
            voice_no += 1;
            let name = if voices.len() > 1 {
                format!("{} {}", part.name, k + 1)
            } else {
                part.name.clone()
            };
            out.push_str(&format!(
                "V:{voice_no} name=\"{}\"\n",
                name.replace('"', "'")
            ));
            let mut body = String::new();
            let mut cursor = 0i64;
            let emit = |body: &mut String,
                        cursor: &mut i64,
                        len: i64,
                        token: &dyn Fn(i64, bool) -> String| {
                // Split at barlines, tying the pieces.
                let mut left = len;
                while left > 0 {
                    let room = BAR - cursor.rem_euclid(BAR);
                    let piece = left.min(room);
                    left -= piece;
                    body.push_str(&token(piece, left > 0));
                    *cursor += piece;
                    if cursor.rem_euclid(BAR) == 0 {
                        body.push_str(" | ");
                    } else {
                        body.push(' ');
                    }
                }
            };
            for chord in chords {
                let start = chord[0].start;
                if start > cursor {
                    let gap = start - cursor;
                    emit(&mut body, &mut cursor, gap, &|p, _| {
                        format!("z{}", length(p))
                    });
                }
                let pitches: Vec<(String, f64)> = chord
                    .iter()
                    .map(|n| {
                        #[expect(clippy::cast_possible_truncation, reason = "a MIDI pitch")]
                        let rounded = n.pitch.round() as i64;
                        (note_name(rounded), (n.pitch - n.pitch.round()) * 100.0)
                    })
                    .collect();
                let cents = pitches
                    .iter()
                    .map(|(_, c)| *c)
                    .find(|c| c.abs() > 10.0)
                    .map(|c| format!("\"^{c:+.0}c\""))
                    .unwrap_or_default();
                let names: Vec<String> = pitches.into_iter().map(|(n, _)| n).collect();
                let len = chord[0].len;
                emit(&mut body, &mut cursor, len, &|p, tied| {
                    let notes = if names.len() == 1 {
                        format!("{}{}", names[0], length(p))
                    } else {
                        format!("[{}]{}", names.join(""), length(p))
                    };
                    format!("{}{}{}", cents, notes, if tied { "-" } else { "" })
                });
            }
            let body = body.trim_end().trim_end_matches('|').trim_end();
            out.push_str(body);
            out.push_str(" |]\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Event;

    #[test]
    fn a_small_tune_is_written_as_abc() {
        let mut s = Score::new("Little Tune", 100.0);
        let p = s.part("Flute", Instrument::Reed);
        p.push(Event::new(0.0, 1.0, 60.0, 0.8)); // C4 quarter
        p.push(Event::new(1.0, 0.5, 62.0, 0.8)); // D4 eighth
        p.push(Event::new(2.0, 3.0, 73.0, 0.8)); // C#5, across the barline
        let abc = to_abc(&s);
        assert!(
            abc.starts_with("X:1\nT:Little Tune\nM:4/4\nL:1/16\nQ:1/4=100\nK:C\n"),
            "{abc}"
        );
        assert!(abc.contains("V:1 name=\"Flute\""), "{abc}");
        assert!(abc.contains("C4 D2 z2 ^c8- | ^c4"), "{abc}");
        assert!(abc.trim_end().ends_with("|]"), "{abc}");
    }

    #[test]
    fn chords_overlaps_octaves_and_microtones() {
        let mut s = Score::new("Chords", 60.0);
        let p = s.part("Pad", Instrument::Pad);
        for k in [48.0, 52.0, 55.0] {
            p.push(Event::new(0.0, 4.0, k, 0.5)); // a C major triad, C3
        }
        p.push(Event::new(1.0, 1.0, 84.0, 0.5)); // overlapping C6: a second voice
        p.push(Event::new(2.0, 1.0, 64.0 - 0.1369, 0.5)); // a just third
        let abc = to_abc(&s);
        assert!(abc.contains("[C,E,G,]16"), "{abc}");
        assert!(abc.contains("V:2 name=\"Pad 2\""), "{abc}");
        assert!(abc.contains("c'4"), "C6 is c' in ABC: {abc}");
        assert!(abc.contains("\"^-14c\"E4"), "{abc}");
    }

    #[test]
    fn every_demo_piece_exports() {
        for score in [
            crate::pieces::rock(),
            crate::pieces::chorale(true),
            crate::pieces::carnatic(1).0,
        ] {
            let abc = to_abc(&score);
            assert!(abc.contains("V:1"), "{}", score.title);
            assert_eq!(abc.matches("|]").count(), abc.matches("\nV:").count());
        }
    }
}
