//! AU-07: built with `default-features = false`, AAC input is refused with
//! a message that names the feature and PATENTS.md — and nothing else
//! changes. (Run with `--no-default-features`; with the feature on this
//! file is empty.)
#![cfg(not(feature = "aac"))]

use vieww_audio::codec::{self, CodecError};

#[test]
fn aac_is_refused_by_name_when_the_feature_is_off() {
    for bytes in [
        &include_bytes!("fixtures/src_mono.aac")[..],
        &include_bytes!("fixtures/src.m4a")[..],
    ] {
        match codec::decode(bytes) {
            Err(CodecError::Unsupported(why)) => {
                assert!(why.contains("`aac` feature") && why.contains("PATENTS.md"), "{why}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
    assert!(codec::decode(include_bytes!("fixtures/src.mp3")).is_ok(), "MP3 still decodes");
}
