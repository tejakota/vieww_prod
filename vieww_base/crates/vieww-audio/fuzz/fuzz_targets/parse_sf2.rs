//! SoundFont 2/3 parsing (AU-20): a `.sf2` is a hostile-input-shaped RIFF
//! tree, and asking for an instrument walks its preset/instrument zones.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(mut sf) = vieww_audio::synth::sf2::SoundFont::parse(data) {
        for program in 0..4 {
            let _ = sf.instrument(0, program);
        }
    }
});
