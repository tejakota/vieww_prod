//! Standard MIDI File parsing (AU-20), then writing back what was read.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(smf) = vieww_audio::midi::Smf::parse(data) {
        let _ = smf.to_bytes();
    }
});
