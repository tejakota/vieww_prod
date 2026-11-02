//! The `Info.plist` side of an AUv3 extension: the `AudioComponents`
//! entry that makes the host see the audio unit at all.
//!
//! An AUv3 ships as an app extension; the extension's `Info.plist`
//! carries `NSExtension` → `NSExtensionAttributes` → `AudioComponents`,
//! an array of dictionaries — one per audio unit — naming the principal
//! class and describing the component (type, subtype, manufacturer,
//! version, name, description, sandbox safety, tags). The values in
//! that dictionary must agree with what the class answers from
//! `initWithComponentDescription:`, which is why this module derives
//! both sides from one [`PluginInfo`]: the [`AudioComponentDescription`]
//! (see [`super`]) and the plist entry ([`components_entry`],
//! [`plist_snippet`]).
//!
//! **Four-char codes.** Apple's component registry keys on four-char
//! codes for the subtype and manufacturer, and expects the developer to
//! own theirs. This module derives them deterministically — the last
//! reverse-DNS segment of [`PluginInfo::id`](crate::PluginInfo::id) for
//! the subtype, the vendor for the manufacturer, lower-case letters and
//! digits only, first four, space-padded — so a plugin and its manifest
//! cannot drift apart. A developer with registered codes should extend
//! [`PluginInfo`] rather than special-case this module; the derivation
//! is the documented contract, and [`plist_snippet`] prints the codes it
//! chose, in place, where a collision would be seen.

use super::{fourcc, AudioComponentDescription};
use crate::{Kind, PluginInfo};

/// The component type code for a plugin kind: `aumu` (music device) for
/// instruments, `aufx` (effect) for effects, `aumf` (music effect) for
/// note effects.
#[must_use]
pub fn component_type(info: &PluginInfo) -> u32 {
    fourcc(match info.kind {
        Kind::Instrument => b"aumu",
        Kind::Effect => b"aufx",
        Kind::NoteEffect => b"aumf",
    })
}

/// The four chars for `s`: lower-case letters and digits, first four,
/// space-padded — the deterministic subtype/manufacturer derivation.
fn chars4(s: &str) -> [u8; 4] {
    let mut out = *b"    ";
    let mut n = 0;
    for b in s.as_bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            out[n] = *b;
            n += 1;
            if n == 4 {
                break;
            }
        } else if b.is_ascii_uppercase() {
            out[n] = b.to_ascii_lowercase();
            n += 1;
            if n == 4 {
                break;
            }
        }
    }
    out
}

/// The component subtype code, from the id's last reverse-DNS segment
/// (`"dev.vieww.gain"` → `"gain"`).
#[must_use]
pub fn subtype(info: &PluginInfo) -> u32 {
    let tail = info.id.rsplit('.').next().unwrap_or(info.id);
    fourcc(&chars4(tail))
}

/// The manufacturer code, from the vendor (`"vieww"` → `"view"`).
#[must_use]
pub fn manufacturer(info: &PluginInfo) -> u32 {
    fourcc(&chars4(info.vendor))
}

/// The `version` integer the plist carries: `0xMMmmpp` for
/// `"M.m.p"` (the first component field Apple's own templates use).
///
/// Unparsable versions are `0` — visible in the plist, never a guess.
#[must_use]
pub fn version(info: &PluginInfo) -> u32 {
    let mut parts = [0u32; 3];
    for (slot, part) in parts.iter_mut().zip(info.version.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    (parts[0] << 16) | (parts[1] << 8) | parts[2]
}

/// The tags the plist carries, from the plugin's feature tags: the
/// AudioComponents vocabulary Apple's hosts filter on.
#[must_use]
pub fn tags(info: &PluginInfo) -> Vec<&'static str> {
    let mut out = Vec::new();
    for f in info.features {
        let t = match *f {
            "synthesizer" | "sampler" | "drum" | "drum-machine" => "Synthesizer",
            "equalizer" => "Equalizer",
            "filter" => "Filter",
            "delay" => "Delay",
            "reverb" => "Reverb",
            "distortion" => "Distortion",
            "compressor" | "limiter" | "dynamics" | "gate" => "Dynamics",
            "chorus" | "flanger" | "phaser" => "Modulation",
            "pitch-shifter" => "Pitch Shift",
            "analyzer" => "Analyzer",
            "spatial" | "surround" => "Panner",
            _ => continue,
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.is_empty() {
        out.push(if info.kind == Kind::Instrument {
            "Synthesizer"
        } else {
            "Effect"
        });
    }
    out
}

/// The `AudioComponents` dictionary for `info` (one array entry), as
/// key/value pairs in plist order. The class name is the app
/// extension's choice, so it is passed in: it must be the name
/// [`super::wrapper::install`] registered the class under.
#[must_use]
pub fn components_entry(class_name: &str, info: &PluginInfo) -> Vec<(&'static str, String)> {
    let d = AudioComponentDescription::for_info(info);
    let code = |c: u32| {
        let b = c.to_be_bytes();
        let end = b.iter().rposition(|x| *x != b' ').map_or(4, |p| p + 1);
        String::from_utf8(b[..end].to_vec()).unwrap_or_default()
    };
    vec![
        ("name", format!("{}: {}", info.vendor, info.name)),
        ("description", info.description.to_owned()),
        ("manufacturer", code(d.componentManufacturer)),
        ("type", code(d.componentType)),
        ("subtype", code(d.componentSubType)),
        ("version", format!("0x{:08X}", version(info))),
        ("sandbox-safe", "true".into()),
        ("tags", tags(info).join(", ")),
        ("principal-class", class_name.to_owned()),
    ]
}

/// The same entry as an XML plist snippet — what a developer pastes into
/// the extension's `Info.plist` (or generates at build time).
///
/// The `version` value is the integer [`version`] computes, and the
/// four-char codes are quoted: they are strings in the plist even when
/// they look like words, because that is what CoreAudio parses them as.
#[must_use]
pub fn plist_snippet(class_name: &str, info: &PluginInfo) -> String {
    let mut out = String::from("\t\t\t<key>AudioComponents</key>\n\t\t\t<array>\n\t\t\t\t<dict>\n");
    for (k, v) in components_entry(class_name, info) {
        match k {
            "principal-class" => {
                out.push_str(&format!("\t\t\t\t\t<key>{k}</key>\n\t\t\t\t\t<string>{v}</string>\n"));
            }
            "version" => {
                out.push_str(&format!("\t\t\t\t\t<key>{k}</key>\n\t\t\t\t\t<integer>{v}</integer>\n"));
            }
            "sandbox-safe" => {
                out.push_str(&format!("\t\t\t\t\t<key>{k}</key>\n\t\t\t\t\t<true/>\n"));
            }
            "tags" => {
                out.push_str(&format!("\t\t\t\t\t<key>{k}</key>\n\t\t\t\t\t<array>\n"));
                for t in v.split(", ") {
                    out.push_str(&format!("\t\t\t\t\t\t<string>{t}</string>\n"));
                }
                out.push_str("\t\t\t\t\t</array>\n");
            }
            _ => {
                out.push_str(&format!("\t\t\t\t\t<key>{k}</key>\n\t\t\t\t\t<string>{v}</string>\n"));
            }
        }
    }
    out.push_str("\t\t\t\t</dict>\n\t\t\t</array>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> PluginInfo {
        PluginInfo {
            id: "dev.vieww.gain",
            name: "vieww Gain",
            vendor: "vieww",
            url: "https://vieww.dev",
            version: "1.2.3",
            description: "Gain, soft drive and balance",
            kind: Kind::Effect,
            inputs: 2,
            outputs: 2,
            note_input: false,
            vst3_class_id: *b"dev.vieww.gain\0\0",
            features: &["utility", "distortion", "stereo"],
        }
    }

    #[test]
    fn codes_derive_from_the_id_and_vendor() {
        let i = info();
        assert_eq!(subtype(&i), fourcc(b"gain"));
        assert_eq!(manufacturer(&i), fourcc(b"view"));
        assert_eq!(component_type(&i), fourcc(b"aufx"));
        // Upper-case, separators and overflow all fold to the same rule.
        assert_eq!(subtype(&PluginInfo { id: "a.b.C-xx-yy", ..info() }), fourcc(b"cxxy"));
        assert_eq!(
            subtype(&PluginInfo { id: "a.verylongname", ..info() }),
            fourcc(b"very")
        );
        assert_eq!(
            manufacturer(&PluginInfo { vendor: "ACME Corp", ..info() }),
            fourcc(b"acme")
        );
        // Kinds map to the component types.
        assert_eq!(
            component_type(&PluginInfo { kind: Kind::Instrument, ..info() }),
            fourcc(b"aumu")
        );
        assert_eq!(
            component_type(&PluginInfo { kind: Kind::NoteEffect, ..info() }),
            fourcc(b"aumf")
        );
    }

    #[test]
    fn versions_pack_and_refuse_to_guess() {
        assert_eq!(version(&info()), 0x0001_0203);
        assert_eq!(
            version(&PluginInfo { version: "2.0", ..info() }),
            0x0002_0000
        );
        assert_eq!(version(&PluginInfo { version: "junk", ..info() }), 0);
    }

    #[test]
    fn tags_map_to_apples_vocabulary_with_a_default() {
        assert_eq!(tags(&info()), vec!["Distortion"]);
        assert_eq!(
            tags(&PluginInfo {
                features: &["synthesizer", "delay"],
                ..
                info()
            }),
            vec!["Synthesizer", "Delay"]
        );
        assert_eq!(
            tags(&PluginInfo {
                features: &[],
                kind: Kind::Instrument,
                ..info()
            }),
            vec!["Synthesizer"]
        );
    }

    #[test]
    fn the_entry_and_the_snippet_agree() {
        let entry = components_entry("ViewwGainAU", &info());
        assert_eq!(entry[0], ("name", "vieww: vieww Gain".to_owned()));
        assert_eq!(entry[2], ("manufacturer", "view".to_owned()));
        assert_eq!(entry[3], ("type", "aufx".to_owned()));
        assert_eq!(entry[4], ("subtype", "gain".to_owned()));
        assert_eq!(entry[5], ("version", "0x00010203".to_owned()));
        assert_eq!(entry[6], ("sandbox-safe", "true".to_owned()));
        assert_eq!(entry[8], ("principal-class", "ViewwGainAU".to_owned()));

        let plist = plist_snippet("ViewwGainAU", &info());
        assert!(plist.contains("<key>AudioComponents</key>"));
        assert!(plist.contains("<string>gain</string>"));
        assert!(plist.contains("<integer>0x00010203</integer>"));
        assert!(plist.contains("<string>ViewwGainAU</string>"));
        assert!(plist.contains("<true/>"));
        assert!(plist.contains("<string>Distortion</string>"));
    }
}
