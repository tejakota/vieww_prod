//! Every demo piece as MusicXML, ABC and LilyPond (AU-16), for checking
//! with the tools that read them:
//!
//!     cargo run -p vieww-music --example notation_export -- OUT_DIR
//!
//! `ci/check/notation.sh` validates the MusicXML against the official
//! schema (xmllint) and engraves the LilyPond (`lilypond`).

use vieww_music::notation::{abc, lilypond, musicxml};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "notation-out".into());
    std::fs::create_dir_all(&out).expect("output folder");
    let pieces = [
        ("rock", vieww_music::pieces::rock()),
        ("chorale-just", vieww_music::pieces::chorale(true)),
        ("chorale-equal", vieww_music::pieces::chorale(false)),
        ("carnatic", vieww_music::pieces::carnatic(1).0),
    ];
    for (name, score) in &pieces {
        let path = |ext: &str| format!("{out}/{name}.{ext}");
        std::fs::write(path("musicxml"), musicxml::to_musicxml(score)).expect("write");
        std::fs::write(path("abc"), abc::to_abc(score)).expect("write");
        std::fs::write(path("ly"), lilypond::to_lilypond(score)).expect("write");
        println!("{name}: {} parts", score.parts.len());
    }
}
