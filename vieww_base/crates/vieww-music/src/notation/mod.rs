//! Notation interchange (AU-16): scores out of, and back into, vieww.
//!
//! * [`musicxml`]: MusicXML 4.0 partwise, the format every notation
//!   program reads, written and read;
//! * [`abc`]: ABC notation, written (sessions, folk, abcjs);
//! * [`lilypond`]: LilyPond, written (engraving: `lilypond score.ly`).

pub mod abc;
pub mod lilypond;
pub mod musicxml;
