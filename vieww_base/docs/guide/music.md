# Music

How to write music with `vieww-music`: a score, a tuning, a tradition's
grammar, rendering it to a file or playing it live, and getting it out
to notation programs (AU-21). [`audio.md`](./audio.md) covers sound in
general; this page is about scores.

## One pitch space

Every tradition in the crate shares one idea of pitch: a **pitch number**
(MIDI-style, 60 = middle C, fractions allowed) played through a
**tuning** that says what frequency it sounds at. Equal temperament, just
intonation, a raga's swara ratios, 22 shrutis and 53-EDO are all values of
`vieww_audio::synth::tuning::Tuning`. Retuning a whole piece is one
assignment:

```rust
use vieww_music::score::{Event, Instrument, Score};

let mut score = Score::new("Two notes", 90.0); // title, beats per minute
let part = score.part("Flute", Instrument::Reed);
part.push(Event::new(0.0, 1.0, 62.0, 0.8)); // start beat, length, pitch, velocity
part.push(Event::new(1.0, 1.0, 66.0, 0.8)); // F#4: a major third above D
// score.tuning = /* just intonation above D */;
```

## Rendering

A score renders offline to a mastered file, or plays live through a
running stream (see [`audio.md`](./audio.md#a-score-live)):

```rust
use vieww_music::render::{render_flac, Settings};

std::fs::write("piece.flac", render_flac(&score, Settings::default())?)?;
std::fs::write("piece.mid", score.to_smf(480).to_bytes())?; // Standard MIDI File
```

`LiveScore` plays the same score live, sample for sample the offline mix,
and its render block is `#[realtime]`: it allocates and locks nothing on
the audio thread (checked at compile time and by
`crates/vieww-music/tests/realtime_no_alloc.rs`).

## Three pieces, walked through

The crate ships three demonstration pieces built from its layers. Each
has an example that renders it:

```sh
cargo run --release -p vieww-music --example carnatic -- out/
cargo run --release -p vieww-music --example chorale  -- out/
cargo run --release -p vieww-music --example rock     -- out/
```

### A Carnatic piece in Adi tala

`pieces::carnatic(seed)` builds a piece in raga **Mayamalavagowla**, the
15th melakarta, on tambura, veena and mridangam. What the example prints
for seed 15:

```text
Mayamalavagowla in Adi tala — raga Mayamalavagowla (melakarta 15), Chaturasra-jati Triputa (x 1 2 3 x o x o)
melody: S R1 G3 M1 G3 M1 D1 P D1 P D1 M1 G3 R1 N3. S R1 S R1 S N3. …
korvai: ta ka di mi ta ka di mi - - - - ta ka di mi … at beat 32, landing on sam at beat 40
wrote out/carnatic-seed15.flac (39.1 s) and out/carnatic-seed15.mid
```

What happened, layer by layer:

1. **The raga is generated, not looked up.** `Melakarta::new(15)` derives
   the scale from the melakarta formula (the chakra picks R and G, the
   number within it picks D and N): `[0, 1, 4, 5, 7, 8, 11]` semitones,
   S R1 G3 M1 P D1 N3.
2. **The melody is a grammar walk.** `generate` walks the raga's arohana
   and avarohana with its phrases, seeded, so the same seed gives the same
   piece. The validator in `carnatic` checks every phrase against the
   grammar.
3. **Gamakas are pitch curves.** Ornamented swaras carry a continuous
   pitch curve; the renderer plays the curve, not a step.
4. **The tala is arithmetic.** Adi tala is Chaturasra-jati Triputa,
   `x 1 2 3 x o x o`: eight beats. The **korvai** (three repetitions of a
   phrase with gaps) is computed so that its last syllable lands on
   **sam**, the first beat of the cycle — here it starts at beat 32 and
   lands at 40.

Change the seed for a different melody in the same raga and tala.

### A chorale, twice

`pieces::chorale(just)` is a four-voice chorale in D on an organ
(I vi ii⁷ V⁷ I IV ii⁷ V⁷ I, the voices led by minimal motion). The example renders
it in 12-tone equal temperament and in **just intonation**, and prints the
just ratios it used:

```text
wrote out/chorale-12tet.flac: Chorale in D, 12-TET (18.2 s)
wrote out/chorale-just.flac: Chorale in D, just (18.2 s)
just chromatic ratios above D: 16/15 9/8 6/5 5/4 4/3 45/32 3/2 8/5 5/3 9/5 15/8
```

Play the two files one after the other. The major thirds (5/4, 14 cents
flatter than equal temperament's) stop beating in the just version. The
score is the same; only `score.tuning` differs.

### A 12-bar rock song

`pieces::rock()` uses the `rock` layer: power chords, a riff, a backbeat
kit and the 12-bar form, on four parts.

## Out to notation programs

`vieww_music::notation` writes a score in three formats:

| Format | Function | Read by |
|---|---|---|
| MusicXML 4.0 (and read back) | `musicxml::to_musicxml`, `from_musicxml` | MuseScore, Dorico, Finale, Sibelius |
| ABC | `abc::to_abc` | abcjs, EasyABC, folk and session sites |
| LilyPond | `lilypond::to_lilypond` | `lilypond score.ly` → engraved PDF and MIDI |

```sh
cargo run -p vieww-music --example notation_export -- out/
lilypond -o out/chorale out/chorale-just.ly   # an engraved score
```

`ci/check/notation.sh` checks all three demo pieces against the tools
themselves: every MusicXML file validates against the official MusicXML
4.0 schema, and every LilyPond file engraves without a warning.

Microtones: MusicXML keeps exact pitch (alter in cents). ABC and LilyPond
round to the nearest semitone and write the offset above the note
(`+12c`). Gamakas are not written by any of the three.

## What the crate does not claim

That following the rules makes music sound idiomatic. The validators
check the grammar a tradition writes down; whether a phrase *sounds* like
the raga is for musicians to judge.
