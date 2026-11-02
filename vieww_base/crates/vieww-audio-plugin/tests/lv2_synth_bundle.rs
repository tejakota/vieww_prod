//! The instrument's LV2 bundle: the same end-to-end proof as
//! `lv2_bundle.rs`, for the plugin whose LV2 life is all MIDI atoms on the
//! control input — an `atom:Sequence` of `midi:MidiEvent` notes the wrapper
//! must fold into typed note events, with no audio input ports at all.
//!
//! Its own test binary for the same reason as `lv2_bundle.rs`: the example
//! source defines `clap_entry`/`lv2_descriptor`, so two examples cannot
//! share one binary.

// The example is `pub` for its own export macros; as a test module
// that pub is unreachable, which is fine here.
#[allow(unreachable_pub)]
#[path = "../examples/vieww_synth.rs"]
mod vieww_synth;

use std::path::PathBuf;

use vieww_audio_plugin::lv2::host::{HostEvent, Library, World};
use vieww_audio_plugin::lv2::ttl;

fn bundle_binary(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("{name}.dylib")
    } else {
        format!("{name}.so")
    }
}

fn built(name: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("VIEWW_PLUGIN_DIR")?);
    let file = if cfg!(windows) {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    };
    let p = dir.join(file);
    assert!(
        p.exists(),
        "VIEWW_PLUGIN_DIR is set but {} is missing",
        p.display()
    );
    Some(p)
}

fn bundle_root() -> PathBuf {
    std::env::var_os("VIEWW_LV2_BUNDLE_DIR").map_or_else(
        || std::env::temp_dir().join(format!("vieww-lv2-synth-{}", std::process::id())),
        PathBuf::from,
    )
}

const URI: &str = "urn:dev.vieww.synth";

#[test]
fn the_instrument_bundle_is_complete_and_speaks_midi() {
    let keep = std::env::var_os("VIEWW_LV2_BUNDLE_DIR").is_some();
    let dir = bundle_root().join("vieww_synth.lv2");
    let binary = bundle_binary("vieww_synth");
    ttl::write_bundle::<vieww_synth::Synth>(&dir, &binary, "vieww_synth")
        .expect("write the bundle");

    let plugin = std::fs::read_to_string(dir.join("vieww_synth.ttl")).unwrap();
    assert!(
        plugin.contains("a lv2:Plugin , lv2:InstrumentPlugin"),
        "{plugin}"
    );
    assert!(plugin.contains("atom:supports midi:MidiEvent , patch:Message"));
    assert!(!plugin.contains("lv2:symbol \"in_0\""), "no audio inputs");
    // The parameter list in the wrapper's order, joined as one predicate.
    assert!(
        plugin.contains(&format!(
            "lv2:parameter <{URI}#param-cutoff-0> , <{URI}#param-resonance-1> , \
         <{URI}#param-level-2> , <{URI}#param-glide-3>"
        )),
        "{plugin}"
    );

    let Some(lib_path) = built("vieww_synth") else {
        eprintln!("VIEWW_PLUGIN_DIR not set; TTL bundle checked, library not loaded");
        if !keep {
            std::fs::remove_dir_all(dir.parent().unwrap()).ok();
        }
        return;
    };
    let so = dir.join(&binary);
    std::fs::copy(&lib_path, &so).unwrap();

    let world = World::new();
    // SAFETY: our own example plugin.
    let lib = unsafe { Library::open(&so) }.expect("load the bundle's library");
    assert_eq!(lib.plugins(), vec![URI.to_owned()]);
    let mut p = lib.instantiate(URI, &world, 48_000.0, (0, 2)).unwrap();
    p.connect_ports();
    assert!(p.activate());

    // Notes ride midi:MidiEvent atoms; a note at frame 100 sounds from
    // there, and the Level parameter rides a patch:Set in the same block.
    let out = p.run(
        512,
        &[],
        &[
            HostEvent::NoteOn {
                time: 100,
                channel: 0,
                key: 69,
                velocity: 1.0,
            },
            HostEvent::Param {
                time: 0,
                uri: &format!("{URI}#param-level-2"),
                value: 0.5,
            },
        ],
    );
    assert!(
        out[0][..100].iter().all(|v| *v == 0.0),
        "silent before the note"
    );
    assert!(
        out[0][101..].iter().any(|v| v.abs() > 0.01),
        "the note sounds"
    );
    assert_eq!(out[0], out[1], "stereo");

    // Note off, then the release (a quarter second) decays to silence:
    // the block that spans it still carries the tail, the next is quiet.
    let out = p.run(
        256,
        &[],
        &[HostEvent::NoteOff {
            time: 0,
            channel: 0,
            key: 69,
        }],
    );
    assert!(out[0].iter().all(|v| v.is_finite()));
    let _release = p.run(65_536, &[], &[]);
    let tail = p.run(4_096, &[], &[]);
    assert!(
        tail[0].iter().all(|v| v.abs() < 1e-3),
        "released: {}",
        tail[0].iter().fold(0.0f32, |m, v| m.max(v.abs()))
    );

    // The parameter is readable back, and the state carries it.
    p.run(16, &[], &[HostEvent::Get]);
    assert_eq!(
        p.replies()
            .iter()
            .find(|(u, _)| u.ends_with("param-level-2")),
        Some(&(format!("{URI}#param-level-2"), 0.5))
    );
    let saved = p.save().expect("state saved");
    let mut q = lib.instantiate(URI, &world, 48_000.0, (0, 2)).unwrap();
    q.connect_ports();
    assert!(q.activate());
    assert!(q.restore(&saved));
    q.run(16, &[], &[HostEvent::Get]);
    assert_eq!(
        q.replies()
            .iter()
            .find(|(u, _)| u.ends_with("param-level-2")),
        Some(&(format!("{URI}#param-level-2"), 0.5))
    );

    drop(p);
    drop(q);
    drop(lib);
    if !keep {
        std::fs::remove_dir_all(dir.parent().unwrap()).ok();
    }
}
