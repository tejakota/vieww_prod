//! MusicXML 4.0 (partwise) out and back in (AU-16).
//!
//! [`to_musicxml`] writes a [`Score`] that MuseScore, Dorico, Sibelius and
//! Finale open; [`from_musicxml`] reads partwise MusicXML back into a
//! [`Score`] (ours, and the common subset other programs write).
//!
//! # The conventions
//!
//! * **Time.** `divisions` = 480 per quarter note, one beat = one quarter.
//!   Measures are 4/4. A note that crosses a barline is split and tied.
//!   Overlapping notes in one part go to separate voices, separated by
//!   `<backup>`.
//! * **Pitch.** The written pitch is the *sounding* pitch in 12-TET
//!   (A4 = 440 Hz): step, octave, and a decimal `<alter>` for any
//!   microtonal remainder (MusicXML allows decimal alters; a just major
//!   third is E with alter −0.1369). Pitch is therefore exact for every
//!   tuning, raga scales and Scala files included, and the score's own
//!   tuning is recorded for information in
//!   `<miscellaneous-field name="vieww-tuning">`. Drum parts write their
//!   General MIDI key as is.
//! * **Gamakas.** MusicXML has no element for a shaped pitch curve. A note's
//!   gamaka is written as an `<other-ornament>` whose text starts with
//!   `vieww-gamaka:`. Other programs show it as a text ornament. The
//!   reader keeps the note and drops the curve (this conversion is lossy).
//! * **Loudness.** Velocity goes in the `dynamics` attribute, as a
//!   percentage of MusicXML's default forte (MIDI velocity 90).
//! * **Tempo.** `<sound tempo>` in the first measure.
//!
//! What reading drops: gamakas, part gain/pan/reverb, tags other than the
//! title, and anything outside notes, rests, chords, voices, ties, backups,
//! forwards, divisions and tempo.

use crate::score::{Event, Instrument, Part, Score, ScoreTuning};

/// Divisions per quarter note (= per beat).
pub const DIVISIONS: i64 = 480;
const MEASURE: i64 = 4 * DIVISIONS;
const STEPS: [(&str, i32); 12] = [
    ("C", 0),
    ("C", 1),
    ("D", 0),
    ("D", 1),
    ("E", 0),
    ("F", 0),
    ("F", 1),
    ("G", 0),
    ("G", 1),
    ("A", 0),
    ("A", 1),
    ("B", 0),
];

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

const fn instrument_name(i: Instrument) -> &'static str {
    match i {
        Instrument::Pluck => "Pluck",
        Instrument::Bowed => "Bowed",
        Instrument::Reed => "Reed",
        Instrument::Mallet => "Mallet",
        Instrument::Pad => "Pad",
        Instrument::Bass => "Bass",
        Instrument::Lead => "Lead",
        Instrument::Drums => "Drums",
        Instrument::Tambura => "Tambura",
        Instrument::Mridangam => "Mridangam",
    }
}

fn instrument_from(name: &str) -> Instrument {
    match name {
        "Pluck" => Instrument::Pluck,
        "Bowed" => Instrument::Bowed,
        "Reed" => Instrument::Reed,
        "Mallet" => Instrument::Mallet,
        "Bass" => Instrument::Bass,
        "Lead" => Instrument::Lead,
        "Drums" => Instrument::Drums,
        "Tambura" => Instrument::Tambura,
        "Mridangam" => Instrument::Mridangam,
        _ => Instrument::Pad,
    }
}

/// The 12-TET pitch number (fractional) a part's event sounds at.
fn sounding(score: &Score, part: &Part, e: &Event) -> f64 {
    if part.instrument == Instrument::Drums || part.instrument == Instrument::Mridangam {
        return f64::from(e.pitch);
    }
    let hz = f64::from(score.tuning.hz(e.pitch));
    if hz > 0.0 {
        69.0 + 12.0 * (hz / 440.0).log2()
    } else {
        f64::from(e.pitch)
    }
}

fn pitch_xml(p: f64) -> String {
    let semis = p.round();
    #[expect(clippy::cast_possible_truncation, reason = "a MIDI pitch")]
    let n = semis as i64;
    let (step, sharp) = STEPS[usize::try_from(n.rem_euclid(12)).unwrap_or(0)];
    let octave = n.div_euclid(12) - 1;
    let alter = f64::from(sharp) + (p - semis);
    let mut s = format!("<pitch><step>{step}</step>");
    if alter.abs() > 1e-6 {
        s.push_str(&format!("<alter>{}</alter>", trim(alter)));
    }
    s.push_str(&format!("<octave>{octave}</octave></pitch>"));
    s
}

fn trim(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.to_owned()
    }
}

#[derive(Clone)]
struct Placed {
    start: i64,
    len: i64,
    pitch: f64,
    velocity: f32,
    gamaka: Option<String>,
}

/// Split one part's notes into voices, each with no overlaps.
fn voices(mut notes: Vec<Placed>) -> Vec<Vec<Placed>> {
    notes.sort_by(|a, b| a.start.cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
    let mut voices: Vec<Vec<Placed>> = Vec::new();
    for n in notes {
        // Same start and length as the last note of a voice: a chord tone
        // in that voice.
        if let Some(v) = voices.iter_mut().find(|v| {
            v.last().is_some_and(|l| {
                (l.start == n.start && l.len == n.len) || l.start + l.len <= n.start
            })
        }) {
            v.push(n);
        } else {
            voices.push(vec![n]);
        }
    }
    voices
}

/// The score as MusicXML 4.0 partwise.
#[must_use]
pub fn to_musicxml(score: &Score) -> String {
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
    x.push_str("<!DOCTYPE score-partwise PUBLIC \"-//Recordare//DTD MusicXML 4.0 Partwise//EN\" \"http://www.musicxml.org/dtds/partwise.dtd\">\n");
    x.push_str("<score-partwise version=\"4.0\">\n");
    x.push_str(&format!(
        "  <work><work-title>{}</work-title></work>\n",
        escape(&score.title)
    ));
    let tuning = match &score.tuning {
        ScoreTuning::Equal(t) => format!(
            "equal {} steps/octave, pitch {} = {} Hz",
            trim(f64::from(t.divisions)),
            trim(f64::from(t.reference_pitch)),
            trim(f64::from(t.reference_hz))
        ),
        ScoreTuning::Scale(t) => format!(
            "scale tonic pitch {} = {} Hz, period {} cents, degrees {}",
            t.tonic_pitch,
            trim(f64::from(t.tonic_hz)),
            trim(f64::from(t.period_cents)),
            t.cents
                .iter()
                .map(|c| trim(f64::from(*c)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    };
    x.push_str("  <identification>\n    <encoding><software>vieww-music</software></encoding>\n");
    x.push_str(&format!(
        "    <miscellaneous><miscellaneous-field name=\"vieww-tuning\">{}</miscellaneous-field></miscellaneous>\n  </identification>\n",
        escape(&tuning)
    ));
    x.push_str("  <part-list>\n");
    for (i, p) in score.parts.iter().enumerate() {
        let id = i + 1;
        x.push_str(&format!(
            "    <score-part id=\"P{id}\"><part-name>{}</part-name><score-instrument id=\"P{id}-I1\"><instrument-name>{}</instrument-name></score-instrument></score-part>\n",
            escape(&p.name),
            instrument_name(p.instrument)
        ));
    }
    x.push_str("  </part-list>\n");

    #[expect(clippy::cast_possible_truncation, reason = "a time in divisions")]
    let to_div = |beats: f64| (beats * DIVISIONS as f64).round() as i64;
    let total = score
        .parts
        .iter()
        .flat_map(|p| p.events.iter().map(|e| to_div(e.end())))
        .max()
        .unwrap_or(0);
    let measures = (total + MEASURE - 1).div_euclid(MEASURE).max(1);

    for (i, part) in score.parts.iter().enumerate() {
        let notes: Vec<Placed> = part
            .events
            .iter()
            .filter(|e| e.length > 0.0)
            .map(|e| Placed {
                start: to_div(e.start),
                len: to_div(e.length).max(1),
                pitch: sounding(score, part, e),
                velocity: e.velocity,
                gamaka: (e.gamaka != crate::carnatic::gamaka::Gamaka::Plain)
                    .then(|| format!("{:?}", e.gamaka)),
            })
            .collect();
        let vs = voices(notes);
        x.push_str(&format!("  <part id=\"P{}\">\n", i + 1));
        for m in 0..measures {
            let (m0, m1) = (m * MEASURE, (m + 1) * MEASURE);
            x.push_str(&format!("    <measure number=\"{}\">\n", m + 1));
            if m == 0 {
                x.push_str(&format!(
                    "      <attributes><divisions>{DIVISIONS}</divisions><time><beats>4</beats><beat-type>4</beat-type></time></attributes>\n"
                ));
                x.push_str(&format!(
                    "      <direction placement=\"above\"><direction-type><words>{} BPM</words></direction-type><sound tempo=\"{}\"/></direction>\n",
                    trim(score.bpm),
                    trim(score.bpm)
                ));
            }
            for (vi, voice) in vs.iter().enumerate() {
                if vi > 0 {
                    x.push_str(&format!(
                        "      <backup><duration>{MEASURE}</duration></backup>\n"
                    ));
                }
                let mut cursor = m0;
                let mut last_start = i64::MIN;
                for n in voice
                    .iter()
                    .filter(|n| n.start < m1 && n.start + n.len > m0)
                {
                    let s = n.start.max(m0);
                    let e = (n.start + n.len).min(m1);
                    let chord = s == last_start;
                    if !chord && s > cursor {
                        x.push_str(&format!(
                            "      <note><rest/><duration>{}</duration><voice>{}</voice></note>\n",
                            s - cursor,
                            vi + 1
                        ));
                    }
                    let tie_stop = n.start < m0;
                    let tie_start = n.start + n.len > m1;
                    let dynamics = f64::from(n.velocity) * 127.0 / 90.0 * 100.0;
                    x.push_str(&format!(
                        "      <note dynamics=\"{}\">",
                        trim(dynamics.round())
                    ));
                    if chord {
                        x.push_str("<chord/>");
                    }
                    x.push_str(&pitch_xml(n.pitch));
                    x.push_str(&format!("<duration>{}</duration>", e - s));
                    if tie_stop {
                        x.push_str("<tie type=\"stop\"/>");
                    }
                    if tie_start {
                        x.push_str("<tie type=\"start\"/>");
                    }
                    x.push_str(&format!("<voice>{}</voice>", vi + 1));
                    let mut notations = String::new();
                    if tie_stop {
                        notations.push_str("<tied type=\"stop\"/>");
                    }
                    if tie_start {
                        notations.push_str("<tied type=\"start\"/>");
                    }
                    if let (Some(g), false) = (&n.gamaka, tie_stop) {
                        notations.push_str(&format!(
                            "<ornaments><other-ornament>vieww-gamaka:{}</other-ornament></ornaments>",
                            escape(g)
                        ));
                    }
                    if !notations.is_empty() {
                        x.push_str(&format!("<notations>{notations}</notations>"));
                    }
                    x.push_str("</note>\n");
                    if !chord {
                        cursor = e;
                    }
                    last_start = s;
                }
                if cursor < m1 {
                    x.push_str(&format!(
                        "      <note><rest/><duration>{}</duration><voice>{}</voice></note>\n",
                        m1 - cursor,
                        vi + 1
                    ));
                }
            }
            if vs.is_empty() {
                x.push_str(&format!(
                    "      <note><rest measure=\"yes\"/><duration>{MEASURE}</duration><voice>1</voice></note>\n"
                ));
            }
            x.push_str("    </measure>\n");
        }
        x.push_str("  </part>\n");
    }
    x.push_str("</score-partwise>\n");
    x
}

// ── reading ───────────────────────────────────────────────────────────────

/// A parsed XML element: name, attributes, text, children.
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Self>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Self> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.trim())
    }
}

/// A small XML reader: elements, attributes, text, entities; skips the
/// prolog, doctype, comments and processing instructions.
fn parse_xml(src: &str) -> Result<Node, String> {
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->").ok_or("unterminated comment")?;
            rest = &after[end + 3..];
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            let end = rest.find('>').ok_or("unterminated declaration")?;
            rest = &rest[end + 1..];
        } else if let Some(after) = rest.strip_prefix("</") {
            let end = after.find('>').ok_or("unterminated end tag")?;
            let name = after[..end].trim();
            let node = stack.pop().ok_or("unbalanced end tag")?;
            if node.name != name {
                return Err(format!("</{name}> closes <{}>", node.name));
            }
            stack
                .last_mut()
                .ok_or("unbalanced end tag")?
                .children
                .push(node);
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix('<') {
            let end = after.find('>').ok_or("unterminated tag")?;
            let mut inner = &after[..end];
            let empty = inner.ends_with('/');
            if empty {
                inner = &inner[..inner.len() - 1];
            }
            let mut parts = inner.splitn(2, char::is_whitespace);
            let name = parts.next().unwrap_or("").to_owned();
            let mut attrs = Vec::new();
            let mut a = parts.next().unwrap_or("").trim();
            while let Some(eq) = a.find('=') {
                let key = a[..eq].trim().to_owned();
                let v = a[eq + 1..].trim_start();
                let q = v.chars().next().ok_or("attribute without value")?;
                let close = v[1..].find(q).ok_or("unterminated attribute")?;
                attrs.push((key, unescape(&v[1..=close])));
                a = v[close + 2..].trim_start();
            }
            let node = Node {
                name,
                attrs,
                ..Node::default()
            };
            if empty {
                stack.last_mut().ok_or("no parent")?.children.push(node);
            } else {
                stack.push(node);
            }
            rest = &after[end + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&unescape(&rest[..end]));
            }
            rest = &rest[end..];
        }
    }
    let mut root = stack.pop().ok_or("empty document")?;
    if !stack.is_empty() {
        return Err("unclosed elements".into());
    }
    root.children.pop().ok_or_else(|| "no root element".into())
}

fn read_pitch(p: &Node) -> Option<f64> {
    let step = p.text_of("step")?;
    let base = match step {
        "C" => 0,
        "D" => 2,
        "E" => 4,
        "F" => 5,
        "G" => 7,
        "A" => 9,
        "B" => 11,
        _ => return None,
    };
    let alter: f64 = p
        .text_of("alter")
        .and_then(|a| a.parse().ok())
        .unwrap_or(0.0);
    let octave: f64 = p.text_of("octave")?.parse().ok()?;
    Some((octave + 1.0) * 12.0 + f64::from(base) + alter)
}

/// Read partwise MusicXML into a 12-TET [`Score`].
///
/// # Errors
/// Not well-formed XML, or not `score-partwise`.
pub fn from_musicxml(xml: &str) -> Result<Score, String> {
    let root = parse_xml(xml)?;
    if root.name != "score-partwise" {
        return Err(format!(
            "<{}> is not score-partwise (timewise MusicXML is not read)",
            root.name
        ));
    }
    let title = root
        .child("work")
        .and_then(|w| w.text_of("work-title"))
        .or_else(|| root.text_of("movement-title"))
        .unwrap_or("Untitled");
    let mut bpm = 120.0;
    let list = root.child("part-list");
    let mut score = Score::new(title, bpm);
    for part in root.all("part") {
        let id = part.attr("id").unwrap_or("");
        let sp = list.and_then(|l| l.all("score-part").find(|s| s.attr("id") == Some(id)));
        let name = sp.and_then(|s| s.text_of("part-name")).unwrap_or(id);
        let instrument = sp
            .and_then(|s| s.child("score-instrument"))
            .and_then(|i| i.text_of("instrument-name"))
            .map_or(Instrument::Pad, instrument_from);
        let mut out = Part::new(name, instrument);
        let mut divisions = 1.0f64;
        let mut measure_start = 0.0f64; // beats
        let mut open_ties: Vec<usize> = Vec::new();
        for m in part.all("measure") {
            let mut cursor = 0.0f64; // divisions within the measure
            let mut longest = 0.0f64;
            let mut last_onset = 0.0f64;
            for el in &m.children {
                match el.name.as_str() {
                    "attributes" => {
                        if let Some(d) = el.text_of("divisions").and_then(|d| d.parse().ok()) {
                            divisions = d;
                        }
                    }
                    "direction" | "sound" => {
                        let sound = if el.name == "sound" {
                            Some(el)
                        } else {
                            el.child("sound")
                        };
                        if let Some(t) = sound
                            .and_then(|s| s.attr("tempo"))
                            .and_then(|t| t.parse().ok())
                        {
                            bpm = t;
                        }
                    }
                    "backup" => {
                        let d: f64 = el
                            .text_of("duration")
                            .and_then(|d| d.parse().ok())
                            .unwrap_or(0.0);
                        cursor = (cursor - d).max(0.0);
                    }
                    "forward" => {
                        let d: f64 = el
                            .text_of("duration")
                            .and_then(|d| d.parse().ok())
                            .unwrap_or(0.0);
                        cursor += d;
                        longest = longest.max(cursor);
                    }
                    "note" => {
                        let dur: f64 = el
                            .text_of("duration")
                            .and_then(|d| d.parse().ok())
                            .unwrap_or(0.0);
                        let chord = el.child("chord").is_some();
                        let onset = if chord { last_onset } else { cursor };
                        if el.child("rest").is_none() {
                            if let Some(pitch) = el.child("pitch").and_then(read_pitch) {
                                let start = measure_start + onset / divisions;
                                let length = dur / divisions;
                                let stop = el.all("tie").any(|t| t.attr("type") == Some("stop"));
                                let starts = el.all("tie").any(|t| t.attr("type") == Some("start"));
                                // Continue a tied note of the same pitch that ends here.
                                let joined = stop.then(|| {
                                    open_ties.iter().position(|&i| {
                                        let e: &Event = &out.events[i];
                                        (f64::from(e.pitch) - pitch).abs() < 1e-3
                                            && (e.end() - start).abs() < 1e-6
                                    })
                                });
                                if let Some(Some(k)) = joined {
                                    let i = open_ties[k];
                                    out.events[i].length += length;
                                    if !starts {
                                        open_ties.remove(k);
                                    }
                                } else {
                                    let velocity = el
                                        .attr("dynamics")
                                        .and_then(|d| d.parse::<f64>().ok())
                                        .map_or(0.7, |d| d / 100.0 * 90.0 / 127.0);
                                    #[expect(
                                        clippy::cast_possible_truncation,
                                        reason = "a pitch and a velocity"
                                    )]
                                    out.events.push(Event::new(
                                        start,
                                        length,
                                        pitch as f32,
                                        velocity.clamp(0.0, 1.0) as f32,
                                    ));
                                    if starts {
                                        open_ties.push(out.events.len() - 1);
                                    }
                                }
                            }
                        }
                        if !chord {
                            last_onset = cursor;
                            cursor += dur;
                            longest = longest.max(cursor);
                        }
                    }
                    _ => {}
                }
            }
            measure_start += longest / divisions;
        }
        score.parts.push(out);
    }
    score.bpm = bpm;
    Ok(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every demo piece survives the round trip at note level: start and
    /// length to one division, sounding pitch to a cent.
    #[test]
    fn the_demo_pieces_round_trip_note_for_note() {
        let pieces = [
            crate::pieces::carnatic(7).0,
            crate::pieces::chorale(false),
            crate::pieces::chorale(true),
            crate::pieces::rock(),
        ];
        for original in pieces {
            let xml = to_musicxml(&original);
            let back = from_musicxml(&xml).unwrap_or_else(|e| panic!("{}: {e}", original.title));
            assert_eq!(back.title, original.title);
            assert!((back.bpm - original.bpm).abs() < 1e-6);
            assert_eq!(back.parts.len(), original.parts.len());
            let tick = 1.0 / DIVISIONS as f64 + 1e-9;
            for (p, q) in original.parts.iter().zip(&back.parts) {
                assert_eq!((&p.name, p.instrument), (&q.name, q.instrument));
                let mut want: Vec<(f64, f64, f64)> = p
                    .events
                    .iter()
                    .filter(|e| e.length > 0.0)
                    .map(|e| (e.start, e.length, sounding(&original, p, e)))
                    .collect();
                let mut got: Vec<(f64, f64, f64)> = q
                    .events
                    .iter()
                    .map(|e| (e.start, e.length, f64::from(e.pitch)))
                    .collect();
                let order = |a: &(f64, f64, f64), b: &(f64, f64, f64)| {
                    a.0.total_cmp(&b.0).then(a.2.total_cmp(&b.2))
                };
                want.sort_by(order);
                got.sort_by(order);
                assert_eq!(
                    want.len(),
                    got.len(),
                    "{} / {}: note count",
                    original.title,
                    p.name
                );
                for (w, g) in want.iter().zip(&got) {
                    assert!(
                        (w.0 - g.0).abs() <= tick,
                        "{}: start {} vs {}",
                        p.name,
                        w.0,
                        g.0
                    );
                    assert!(
                        (w.1 - g.1).abs() <= 2.0 * tick,
                        "{}: length {} vs {}",
                        p.name,
                        w.1,
                        g.1
                    );
                    assert!(
                        (w.2 - g.2).abs() < 0.01,
                        "{}: pitch {} vs {}",
                        p.name,
                        w.2,
                        g.2
                    );
                }
            }
        }
    }

    #[test]
    fn microtones_are_decimal_alters_and_ties_cross_barlines() {
        let mut s = Score::new("Ties & tones", 90.0);
        let p = s.part("Melody", Instrument::Reed);
        p.push(Event::new(3.0, 2.0, 64.0 - 0.1369, 0.7)); // a just third, over the barline
        let xml = to_musicxml(&s);
        assert!(xml.contains("<alter>-0.1369"), "{xml}");
        assert_eq!(xml.matches("<tie type=\"start\"/>").count(), 1);
        assert!(xml.contains("Ties &amp; tones"));
        let back = from_musicxml(&xml).unwrap();
        let e = &back.parts[0].events;
        assert_eq!(e.len(), 1, "the tied halves join");
        assert!((e[0].start - 3.0).abs() < 1e-9 && (e[0].length - 2.0).abs() < 1e-9);
        assert!((f64::from(e[0].pitch) - (64.0 - 0.1369)).abs() < 1e-3);
    }

    #[test]
    fn gamakas_are_written_as_ornaments() {
        use crate::carnatic::gamaka::Gamaka;
        let mut s = Score::new("Kampita", 60.0);
        s.part("Veena", Instrument::Pluck)
            .push(
                Event::new(0.0, 1.0, 62.0, 0.8).with_gamaka(Gamaka::Kampita {
                    depth_cents: 100.0,
                    rate_hz: 5.0,
                }),
            );
        let xml = to_musicxml(&s);
        assert!(
            xml.contains("<other-ornament>vieww-gamaka:Kampita"),
            "{xml}"
        );
        assert_eq!(from_musicxml(&xml).unwrap().parts[0].events.len(), 1);
    }

    #[test]
    fn garbage_is_refused_with_a_reason() {
        assert!(from_musicxml("<score-timewise></score-timewise>").is_err());
        assert!(from_musicxml("<score-partwise><part></score-partwise>").is_err());
    }
}
