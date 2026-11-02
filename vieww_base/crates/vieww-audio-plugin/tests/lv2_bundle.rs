//! The LV2 bundle, end to end: the TTLs [`ttl`](vieww_audio_plugin::lv2::ttl)
//! generates for the real example plugin, and — when
//! `VIEWW_PLUGIN_DIR` points at the built libraries (`tools/check-plugins.sh`
//! sets it) — the library loaded from the bundle through the in-house
//! `dlopen` host and driven: parameters by `patch:Set`, notes by MIDI
//! atoms, state through the state extension.
//!
//! The example plugin's source is included rather than duplicated, so the
//! bundle is generated from the same `PluginInfo` the `cdylib` exports.
//! One example per test binary: two would define `clap_entry` twice.
//! `VIEWW_LV2_BUNDLE_DIR` (also set by `tools/check-plugins.sh`) moves the
//! bundle out of a scratch directory so the C host compiled against the
//! official headers can load the very same bundle afterwards.

// The example is `pub` for its own export macros; as a test module
// that pub is unreachable, which is fine here.
#[allow(unreachable_pub)]
#[path = "../examples/vieww_gain.rs"]
mod vieww_gain;

use std::path::PathBuf;

use vieww_audio_plugin::lv2::host::{HostEvent, Library, World};
use vieww_audio_plugin::lv2::ttl;

/// The library file name inside a bundle, per platform (the manifest's
/// `lv2:binary` names it; hosts load exactly what the RDF points at).
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

/// Where the bundle lands: `VIEWW_LV2_BUNDLE_DIR` when the C host wants it
/// afterwards, a scratch directory otherwise.
fn bundle_root() -> PathBuf {
    std::env::var_os("VIEWW_LV2_BUNDLE_DIR").map_or_else(
        || std::env::temp_dir().join(format!("vieww-lv2-bundle-{}", std::process::id())),
        PathBuf::from,
    )
}

const URI: &str = "urn:dev.vieww.gain";

#[test]
fn the_bundle_is_complete_and_the_library_in_it_runs() {
    let keep = std::env::var_os("VIEWW_LV2_BUNDLE_DIR").is_some();
    let dir = bundle_root().join("vieww_gain.lv2");
    let binary = bundle_binary("vieww_gain");
    ttl::write_bundle::<vieww_gain::Gain>(&dir, &binary, "vieww_gain").expect("write the bundle");

    let manifest = std::fs::read_to_string(dir.join("manifest.ttl")).unwrap();
    assert!(manifest.contains(&format!("<{URI}>")), "{manifest}");
    assert!(manifest.contains(&format!("lv2:binary <{binary}>")));
    assert!(manifest.contains("rdfs:seeAlso <vieww_gain.ttl>"));
    let plugin = std::fs::read_to_string(dir.join("vieww_gain.ttl")).unwrap();
    assert!(plugin.contains("lv2:symbol \"latency\""));
    // The parameter list in the wrapper's order, joined as one predicate.
    assert!(
        plugin.contains(&format!(
            "lv2:parameter <{URI}#param-gain-0> , <{URI}#param-drive-1> , \
         <{URI}#param-pan-2> , <{URI}#param-bypass-3>"
        )),
        "{plugin}"
    );

    let Some(lib_path) = built("vieww_gain") else {
        eprintln!("VIEWW_PLUGIN_DIR not set; TTL bundle checked, library not loaded");
        if !keep {
            std::fs::remove_dir_all(dir.parent().unwrap()).ok();
        }
        return;
    };
    // A real bundle carries the library itself.
    let so = dir.join(&binary);
    std::fs::copy(&lib_path, &so).unwrap();

    let world = World::new();
    // SAFETY: our own example plugin.
    let lib = unsafe { Library::open(&so) }.expect("load the bundle's library");
    assert_eq!(lib.plugins(), vec![URI.to_owned()]);
    let mut p = lib.instantiate(URI, &world, 48_000.0, (2, 2)).unwrap();
    p.connect_ports();
    assert!(p.activate());

    // Parameters ride patch:Set; the change at frame 128 lands there and
    // the 5 ms smoother glides (never steps), as the CLAP test asserts.
    // -30 dB is inside the example's [-48, +24] range, so it round-trips
    // exactly (an out-of-range Set would be clamped by the Driver).
    let input = vec![vec![0.5f32; 256]; 2];
    let out = p.run(
        256,
        &input,
        &[HostEvent::Param {
            time: 128,
            uri: &format!("{URI}#param-gain-0"),
            value: -30.0,
        }],
    );
    assert!((out[0][10] - 0.5).abs() < 1e-3);
    assert!((out[0][127] - 0.5).abs() < 1e-3);
    assert!(out[0][128] < 0.5 && out[0][255] < out[0][128] && out[0][255] < 0.35);
    assert!(
        out[0][129..].windows(2).all(|w| w[1] <= w[0]),
        "monotonic glide"
    );

    // patch:Get answers with every parameter.
    p.run(16, &[], &[HostEvent::Get]);
    let replies = p.replies();
    assert_eq!(replies.len(), 4, "{replies:?}");
    assert_eq!(replies[0], (format!("{URI}#param-gain-0"), -30.0));

    // State through the extension, into a second instance.
    let saved = p.save().expect("state saved");
    let mut q = lib.instantiate(URI, &world, 48_000.0, (2, 2)).unwrap();
    q.connect_ports();
    assert!(q.activate());
    assert!(q.restore(&saved));
    q.run(16, &[], &[HostEvent::Get]);
    assert_eq!(q.replies()[0], (format!("{URI}#param-gain-0"), -30.0));

    drop(p);
    drop(q);
    drop(lib);
    if !keep {
        std::fs::remove_dir_all(dir.parent().unwrap()).ok();
    }
}
