//! Ogg Opus end to end: page CRCs, the OpusHead/OpusTags pair, TOC and
//! multistream framing of every packet, then the SILK/CELT/hybrid payload
//! decoder on whatever survives — hostile bytes must neither panic nor
//! produce a decode the oracle rejects.

#![no_main]

mod common;

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
        // A sound container decodes; whatever comes out passes the
        // oracle (finite, bounded, a sane shape).
        if let Ok(d) = opus::decode(data) {
            common::check(&d);
        }
    }
});
