//! SFZ text parsing (AU-20): opcodes, headers, ranges and includes from an
//! untrusted file. Samples resolve to one short synthetic buffer.

#![no_main]

use std::sync::Arc;

use libfuzzer_sys::fuzz_target;
use vieww_audio::synth::sampler::SampleData;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let sample = Arc::new(SampleData::mono(vec![0.0, 0.5, -0.5, 0.0], 48_000.0));
    let _ = vieww_audio::synth::sfz::parse(&text, |_| Some(Arc::clone(&sample)));
});
