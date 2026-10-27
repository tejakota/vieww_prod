//! Binary preset / plugin state chunks (AU-20): decode, and a decoded
//! state must survive its own round trip.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vieww_audio::preset::State;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = State::from_bytes(data) {
        let bytes = s.to_bytes();
        let again = State::from_bytes(&bytes).expect("a state re-reads its own bytes");
        // Bytes, not PartialEq: a NaN parameter is equal to itself only
        // bit for bit.
        assert_eq!(again.to_bytes(), bytes);
    }
});
