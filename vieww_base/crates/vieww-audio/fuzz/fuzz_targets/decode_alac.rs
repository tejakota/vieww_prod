//! The ALAC decoder: the MP4 sample tables, the `alac` magic cookie, and
//! the adaptive-Golomb frame bodies a corrupt file smears into one another.

#![no_main]

mod common;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(d) = vieww_audio::codec::alac::decode_mp4(data) {
        common::check(&d);
    }
});
