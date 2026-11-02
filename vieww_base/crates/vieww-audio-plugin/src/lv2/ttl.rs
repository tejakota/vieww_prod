//! The RDF side of an LV2 bundle: the TTL files that make a library
//! discoverable and describe its ports and parameters.
//!
//! In LV2 the manifest and plugin data *are* the plugin's description —
//! hosts read RDF, then load the library the RDF points at. These
//! generators are the single source of truth alongside the wrapper: the
//! URIs, port indices, symbols, parameter fragments and state property
//! here are exactly what `lv2/mod.rs` implements, because both call
//! [`plugin_uri`](super::plugin_uri) and
//! [`param_fragment`](super::param_fragment).
//!
//! A bundle on disk is
//!
//! ```text
//! vieww_gain.lv2/
//!     manifest.ttl      # the plugin URI → binary + seeAlso
//!     vieww_gain.ttl    # ports, parameters, features, state
//!     vieww_gain.so     # the library (bundle.sh copies it)
//! ```
//!
//! [`write_bundle`] writes the two TTL files; `tools/bundle.sh` lays out
//! the directory and the library. Generated for a plugin `P` with
//! `write_bundle::<P>(dir, "vieww_gain.so", "vieww_gain")`.

use std::io;
use std::path::Path;

use super::{param_fragment, plugin_uri, PORT_CONTROL_IN, PORT_CONTROL_OUT, PORT_LATENCY};
use crate::{Kind, Plugin, PluginInfo};

/// An LV2 plugin class for a plugin, from its kind and feature tags.
#[must_use]
pub fn class(info: &PluginInfo) -> &'static str {
    if info.kind == Kind::Instrument {
        return "InstrumentPlugin";
    }
    for f in info.features {
        let c = match *f {
            "allpass" => "AllpassPlugin",
            "amplifier" | "gain" => "AmplifierPlugin",
            "analyser" | "analyzer" => "AnalyserPlugin",
            "bandpass" => "BandpassPlugin",
            "drum" | "drum-machine" | "sampler" => "InstrumentPlugin",
            "chorus" => "ChorusPlugin",
            "compressor" | "limiter" | "dynamics" => "CompressorPlugin",
            "delay" => "DelayPlugin",
            "distortion" | "waveshaper" => "DistortionPlugin",
            "equalizer" => "EQPlugin",
            "expander" => "ExpanderPlugin",
            "filter" => "FilterPlugin",
            "flanger" => "FlangerPlugin",
            "gate" => "GatePlugin",
            "highpass" => "HighpassPlugin",
            "lowpass" => "LowpassPlugin",
            "modulator" | "phaser" => "ModulatorPlugin",
            "pitch-shifter" => "PitchPlugin",
            "reverb" => "ReverbPlugin",
            "spatial" | "surround" => "SpatialPlugin",
            "synthesizer" | "instrument" => "InstrumentPlugin",
            "utility" => "UtilityPlugin",
            _ => continue,
        };
        return c;
    }
    "UtilityPlugin"
}

/// Escape a Turtle string literal body.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// A double as Turtle syntax: always a decimal or plain number, never
/// bare `inf`/`NaN` (which Turtle has no syntax for).
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0.0".into(); // unreachable for real parameter bounds
    }
    if (v - v.trunc()).abs() < f64::EPSILON && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// The bundle's `manifest.ttl`: the plugin URI, its binary, and the
/// seeAlso that carries the rest.
#[must_use]
pub fn manifest_ttl(info: &PluginInfo, binary: &str, ttl_file: &str) -> String {
    format!(
        "@prefix lv2:  <http://lv2plug.in/ns/lv2core#> .\n\
         @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
         \n\
         <{uri}>\n\
         \x20   a lv2:Plugin ;\n\
         \x20   lv2:binary <{binary}> ;\n\
         \x20   rdfs:seeAlso <{ttl}> .\n",
        uri = plugin_uri(info),
        binary = escape(binary),
        ttl = escape(ttl_file),
    )
}

/// The plugin's own TTL: class, metadata, features, state, ports and
/// parameters, matching the wrapper's fixed layout exactly.
#[must_use]
pub fn plugin_ttl<P: Plugin>() -> String {
    let info = P::info();
    let uri = plugin_uri(info);
    let params = P::create().param_info();
    let mut t = String::new();
    t.push_str(
        "@prefix atom:  <http://lv2plug.in/ns/ext/atom#> .\n\
         @prefix doap:  <http://usefulinc.com/ns/doap#> .\n\
         @prefix foaf:  <http://xmlns.com/foaf/0.1/> .\n\
         @prefix lv2:   <http://lv2plug.in/ns/lv2core#> .\n\
         @prefix midi:  <http://lv2plug.in/ns/ext/midi#> .\n\
         @prefix patch: <http://lv2plug.in/ns/ext/patch#> .\n\
         @prefix rdfs:  <http://www.w3.org/2000/01/rdf-schema#> .\n\
         @prefix urid:  <http://lv2plug.in/ns/ext/urid#> .\n\
         \n",
    );
    let (minor, micro) = version_parts(info.version);
    let supports_midi = info.note_input;
    t.push_str(&format!(
        "<{uri}>\n\
         \x20   a lv2:Plugin , lv2:{class} ;\n\
         \x20   doap:name \"{name}\" ;\n\
         \x20   doap:maintainer [ foaf:name \"{vendor}\" ] ;\n\
         \x20   doap:shortdesc \"{desc}\" ;\n\
         \x20   lv2:minorVersion {minor} ;\n\
         \x20   lv2:microVersion {micro} ;\n\
         \x20   lv2:requiredFeature urid:map ;\n\
         \x20   lv2:extensionData <http://lv2plug.in/ns/ext/state#interface> ;\n",
        uri = uri,
        class = class(info),
        name = escape(info.name),
        vendor = escape(info.vendor),
        desc = escape(info.description),
        minor = minor,
        micro = micro,
    ));
    // Ports: control in/out, latency, then audio — assembled as one
    // `lv2:port [..] , [..]` list, ended with `;` before the parameters.
    let mut ports = Vec::new();
    ports.push(port_block(
        PORT_CONTROL_IN,
        "control_in",
        "Control In",
        "lv2:InputPort , atom:AtomPort",
        &format!(
            "atom:bufferType atom:Sequence ;\n         atom:supports {}",
            if supports_midi {
                "midi:MidiEvent , patch:Message"
            } else {
                "patch:Message"
            }
        ),
    ));
    ports.push(port_block(
        PORT_CONTROL_OUT,
        "control_out",
        "Control Out",
        "lv2:OutputPort , atom:AtomPort",
        "atom:bufferType atom:Sequence ;\n         atom:supports patch:Message",
    ));
    ports.push(port_block(
        PORT_LATENCY,
        "latency",
        "Latency",
        "lv2:OutputPort , lv2:ControlPort",
        "lv2:portProperty lv2:connectionOptional ;\n         \
         lv2:designation <http://lv2plug.in/ns/ext/latency#latency>",
    ));
    for (i, _) in (0..info.inputs).enumerate() {
        ports.push(port_block(
            PORT_LATENCY + 1 + i as u32,
            &format!("in_{i}"),
            &format!("Input {}", i + 1),
            "lv2:InputPort , lv2:AudioPort",
            "",
        ));
    }
    for (i, _) in (0..info.outputs).enumerate() {
        ports.push(port_block(
            PORT_LATENCY + 1 + info.inputs + i as u32,
            &format!("out_{i}"),
            &format!("Output {}", i + 1),
            "lv2:OutputPort , lv2:AudioPort",
            "",
        ));
    }
    t.push_str("    lv2:port ");
    for (i, block) in ports.iter().enumerate() {
        t.push_str(block);
        t.push_str(if i + 1 == ports.len() {
            " ;\n"
        } else {
            " ,\n    "
        });
    }
    if params.is_empty() {
        // No parameters: the port list ends the subject.
        t.truncate(t.trim_end_matches(" ;\n").len());
        t.push_str(" .\n");
    } else {
        let list = params
            .iter()
            .map(|p| format!("<{}#{}>", uri, param_fragment(&p.name, p.id)))
            .collect::<Vec<_>>()
            .join(" , ");
        t.push_str(&format!("    lv2:parameter {list} .\n"));
    }
    t.push('\n');
    for p in &params {
        let frag = param_fragment(&p.name, p.id);
        t.push_str(&format!(
            "<{uri}#{frag}>\n\
             \x20   a lv2:Parameter ;\n\
             \x20   rdfs:label \"{name}\" ;\n\
             \x20   rdfs:range atom:Float ;\n\
             \x20   lv2:default {def} ;\n\
             \x20   lv2:minimum {min} ;\n\
             \x20   lv2:maximum {max}",
            uri = uri,
            frag = frag,
            name = escape(&p.name),
            def = num(p.default),
            min = num(p.min),
            max = num(p.max),
        ));
        if p.taper == crate::Taper::Stepped && !p.choices.is_empty() {
            t.push_str(&format!(
                " ;\n    rdfs:comment \"stepped: {}\"",
                escape(&p.choices.join(" | "))
            ));
        } else if p.taper == crate::Taper::Stepped {
            t.push_str(" ;\n    rdfs:comment \"stepped\"");
        } else if !p.unit.is_empty() {
            t.push_str(&format!(
                " ;\n    rdfs:comment \"unit: {}\"",
                escape(p.unit)
            ));
        }
        t.push_str(" .\n");
    }
    t
}

/// One `lv2:port` block body: `[ a … ]` without list separators.
fn port_block(index: u32, symbol: &str, name: &str, classes: &str, extra: &str) -> String {
    let mut s = format!(
        "[\n\
         \x20       a {classes} ;\n\
         \x20       lv2:index {index} ;\n\
         \x20       lv2:symbol \"{symbol}\" ;\n\
         \x20       lv2:name \"{name}\"",
        classes = classes,
        index = index,
        symbol = symbol,
        name = name,
    );
    if !extra.is_empty() {
        s.push_str(" ;\n        ");
        s.push_str(extra);
    }
    s.push_str("\n    ]");
    s
}

/// `major.minor.micro` → LV2's `(minorVersion, microVersion)` pair (LV2
/// has no major in TTL; incompatible change = new URI).
fn version_parts(version: &str) -> (u32, u32) {
    let mut it = version.split('.');
    let _major = it.next();
    let minor = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let micro = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (minor, micro)
}

/// Write `manifest.ttl` and `<ttl_stem>.ttl` into `dir` (the `.lv2`
/// bundle directory itself; create it first). Returns the bundle path.
///
/// # Errors
/// Any file error from writing the two TTL files.
pub fn write_bundle<P: Plugin>(dir: &Path, binary: &str, ttl_stem: &str) -> io::Result<()> {
    let info = P::info();
    let ttl_file = format!("{ttl_stem}.ttl");
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("manifest.ttl"),
        manifest_ttl(info, binary, &ttl_file),
    )?;
    std::fs::write(dir.join(&ttl_file), plugin_ttl::<P>())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{plugin_uri, PORT_LATENCY};
    use super::*;
    use crate::testing::{Gain, Synth};

    #[test]
    fn manifest_names_the_uri_binary_and_tll() {
        let m = manifest_ttl(Gain::info(), "vieww_gain.so", "vieww_gain.ttl");
        assert!(m.contains("<urn:dev.vieww.test-gain>"), "{m}");
        assert!(m.contains("a lv2:Plugin"));
        assert!(m.contains("lv2:binary <vieww_gain.so>"));
        assert!(m.contains("rdfs:seeAlso <vieww_gain.ttl>"));
    }

    #[test]
    fn effect_ttl_matches_the_wrapper_layout() {
        let t = plugin_ttl::<Gain>();
        assert!(t.contains("a lv2:Plugin , lv2:UtilityPlugin"), "{t}");
        assert!(t.contains("lv2:requiredFeature urid:map"));
        assert!(t.contains("lv2:extensionData <http://lv2plug.in/ns/ext/state#interface>"));
        // The three fixed ports, in the wrapper's index order.
        assert!(t.contains("lv2:index 0"));
        assert!(t.contains("lv2:symbol \"control_in\""));
        assert!(t.contains("atom:supports patch:Message"));
        assert!(t.contains("lv2:index 1"));
        assert!(t.contains("lv2:symbol \"control_out\""));
        assert!(t.contains(&format!("lv2:index {}", PORT_LATENCY)));
        assert!(t.contains("lv2:designation <http://lv2plug.in/ns/ext/latency#latency>"));
        // Audio after the fixed ports.
        assert!(t.contains("lv2:index 3"));
        assert!(t.contains("lv2:symbol \"in_0\""));
        assert!(t.contains("lv2:index 4"));
        assert!(t.contains("lv2:symbol \"in_1\""));
        assert!(t.contains("lv2:index 5"));
        assert!(t.contains("lv2:symbol \"out_0\""));
        // Parameters as LV2 parameters, with the wrapper's URIs and ranges.
        let uri = plugin_uri(Gain::info());
        assert!(t.contains(&format!(
            "lv2:parameter <{uri}#param-gain-1> , <{uri}#param-invert-2>"
        )));
        assert!(t.contains(&format!("<{uri}#param-gain-1>")));
        assert!(t.contains("rdfs:range atom:Float"));
        assert!(t.contains("lv2:default 0.0"));
        assert!(t.contains("lv2:minimum -60.0"));
        assert!(t.contains("lv2:maximum 12.0"));
        assert!(t.contains("rdfs:comment \"stepped: Off | On\""));
        assert!(t.contains("rdfs:comment \"unit: dB\""));
    }

    #[test]
    fn instrument_ttl_supports_midi_and_has_no_audio_inputs() {
        let t = plugin_ttl::<Synth>();
        assert!(t.contains("a lv2:Plugin , lv2:InstrumentPlugin"), "{t}");
        assert!(t.contains("atom:supports midi:MidiEvent , patch:Message"));
        // Two outputs at indices 3 and 4; no input ports.
        assert!(!t.contains("lv2:symbol \"in_0\""));
        assert!(t.contains("lv2:index 3"));
        assert!(t.contains("lv2:symbol \"out_0\""));
        assert!(t.contains("lv2:index 4"));
        assert!(t.contains("lv2:symbol \"out_1\""));
        assert!(t.contains("lv2:parameter <urn:dev.vieww.test-synth#param-level-10>"));
    }

    #[test]
    fn bundle_files_land_on_disk_with_the_generated_text() {
        let dir = std::env::temp_dir().join(format!("vieww-lv2-ttl-{}", std::process::id()));
        let r = write_bundle::<Gain>(&dir, "vieww_gain.so", "vieww_gain");
        assert!(r.is_ok(), "{r:?}");
        let manifest = std::fs::read_to_string(dir.join("manifest.ttl")).unwrap();
        assert_eq!(
            manifest,
            manifest_ttl(Gain::info(), "vieww_gain.so", "vieww_gain.ttl")
        );
        let plugin = std::fs::read_to_string(dir.join("vieww_gain.ttl")).unwrap();
        assert_eq!(plugin, plugin_ttl::<Gain>());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn numbers_and_versions_are_plain_turtle() {
        assert_eq!(num(0.0), "0.0");
        assert_eq!(num(12.0), "12.0");
        assert_eq!(num(-60.0), "-60.0");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(f64::NAN), "0.0");
        assert_eq!(version_parts("1.2.3"), (2, 3));
        assert_eq!(version_parts("1.2"), (2, 0));
        assert_eq!(version_parts("1"), (0, 0));
        assert_eq!(version_parts("x.y.z"), (0, 0));
    }

    #[test]
    fn plugin_class_comes_from_kind_and_features() {
        assert_eq!(class(Synth::info()), "InstrumentPlugin");
        let mut i = *Gain::info();
        i.features = &["reverb"];
        assert_eq!(class(&i), "ReverbPlugin");
        i.features = &["unknown-feature"];
        assert_eq!(class(&i), "UtilityPlugin");
        i.kind = Kind::NoteEffect;
        assert_eq!(class(&i), "UtilityPlugin");
    }
}
