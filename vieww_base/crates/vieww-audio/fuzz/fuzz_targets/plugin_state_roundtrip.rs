//! Plugin state chunks (AU-20): hostile bytes into `Plugin::load_state`, and
//! a state saved by `Plugin::save_state` must reload into the same
//! parameters. This is the plugin-level counterpart of `load_preset` (which
//! fuzzes the raw `State` codec): here the default `Plugin::save_state` /
//! `Plugin::load_state` pair, built on `State::capture` / `State::apply`,
//! is what every wrapper's state extension funnels into, so it gets its
//! own target.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vieww_audio_plugin::{
    Buffers, Event, Kind, ParamInfo, Parametric, Plugin, PluginInfo, Status, Transport,
};

/// The smallest real plugin: one linear parameter.
struct Tiny {
    level: f64,
}

static TINY_INFO: PluginInfo = PluginInfo {
    id: "dev.vieww.fuzz.tiny",
    name: "Fuzz Tiny",
    vendor: "vieww",
    url: "https://vieww.dev",
    version: "1.0.0",
    description: "One-parameter plugin for the state round-trip fuzzer",
    kind: Kind::Effect,
    inputs: 1,
    outputs: 1,
    note_input: false,
    vst3_class_id: *b"vieww-fuzz-tiny!",
    features: &["utility"],
};

impl Parametric for Tiny {
    fn param_info(&self) -> Vec<ParamInfo> {
        vec![ParamInfo::new(1, "Level", 0.0, 1.0, 0.5)]
    }

    fn get_param(&self, id: u32) -> Option<f64> {
        (id == 1).then_some(self.level)
    }

    fn set_param(&mut self, id: u32, v: f64) {
        if id == 1 {
            self.level = v;
        }
    }
}

impl Plugin for Tiny {
    fn info() -> &'static PluginInfo {
        &TINY_INFO
    }

    fn create() -> Self {
        Self { level: 0.5 }
    }

    fn process(&mut self, buffers: &mut Buffers<'_, '_, '_>, _: &[Event], _: &Transport) -> Status {
        buffers.pass_through();
        Status::ContinueIfNotQuiet
    }
}

fuzz_target!(|data: &[u8]| {
    let mut p = Tiny::create();
    if p.load_state(data) {
        // A hostile chunk that parsed must now survive its own round trip:
        // save and reload, and every parameter must come back equal.
        let saved = p.save_state();
        let mut q = Tiny::create();
        assert!(q.load_state(&saved), "our own saved state reloads");
        for info in p.param_info() {
            assert_eq!(
                p.get_param(info.id),
                q.get_param(info.id),
                "parameter {} survives the round trip",
                info.name
            );
        }
    }
});
