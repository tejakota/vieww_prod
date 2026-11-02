//! The Ogg-Opus container layer: page CRCs, the OpusHead/OpusTags pair,
//! TOC framing of every packet — everything the pure side decodes before
//! it refuses the payload by name (which this target also pins: the
//! refusal is the contract the platform bridge matches on).

#![no_main]

use libfuzzer_sys::fuzz_target;
use vieww_audio::codec::opus;

fuzz_target!(|data: &[u8]| {
    // The packet-level framing, hostile bytes and all.
    let _ = opus::packet_info(data);
    if let Ok(s) = opus::parse(data) {
        // A parsed stream is internally consistent by construction; the
        // position arithmetic must still stay sane under hostile granules.
        let _ = s.total_by_durations();
        for i in 0..s.packets.len() {
            let _ = s.packet_start(i);
        }
        if let Some(total) = s.total_pcm {
            assert!(total < 1 << 32, "granule arithmetic ran away: {total}");
        }
        // A sound container is refused by name — the exact string the
        // platform fallback keys on.
        assert_eq!(
            opus::decode(data).err(),
            Some(vieww_audio::codec::CodecError::Unsupported(opus::REFUSED_PAYLOAD))
        );
    }
});
