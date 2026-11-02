//! AUv3 (Audio Unit v3): Apple's plugin format for macOS and iOS —
//! **written and type-checked, never run** (see the status section).
//!
//! An AUv3 plugin is not a plain shared library the way CLAP and VST3
//! plugins are: it ships inside an app extension whose `Info.plist`
//! declares an `AudioComponents` entry, and the host instantiates the
//! extension's principal class — an Objective-C subclass of `AUAudioUnit`
//! — through the Objective-C runtime. So this module is shaped
//! differently from [`clap`](crate::clap) and [`vst3`](crate::vst3):
//! there is no `clap_entry`-style symbol to export. Instead:
//!
//! * [`manifest`] derives the `AudioComponents` dictionary the extension's
//!   `Info.plist` must carry (type, subtype, manufacturer four-char codes,
//!   version, name) from a [`PluginInfo`](crate::PluginInfo). Pure code,
//!   tested on every platform.
//! * [`wrapper::install`] (Apple targets only) allocates the
//!   `AUAudioUnit` subclass at run time with the Objective-C runtime —
//!   `objc_allocateClassPair`, `class_addMethod`, `objc_registerClassPair`
//!   — and installs methods for the whole lifecycle:
//!   `initWithComponentDescription:options:error:`, the `parameterTree`
//!   getter, `internalRenderBlock`, `allocateRenderResourcesAndReturnError:`,
//!   `deallocateRenderResources`, `scheduleMIDIEventList:`,
//!   `requestViewControllerWithCompletionHandler:` and the document state
//!   pair. The Rust state lives in the instance's trailing bytes
//!   (`objc_allocateClassPair`'s extra size, read back with
//!   `object_getIndexedIvars`).
//! * [`events`] is the MIDI schedule: the walk of CoreMIDI's
//!   `MIDIEventList` (Universal MIDI Packets) that
//!   `scheduleMIDIEventList:` delivers, and the bounded queue that holds
//!   events until the render clock reaches them. Pure code, tested on
//!   every platform.
//! * [`view`] (behind the `gui` feature) is the view story, stubbed to
//!   the host-neutral [`gui`](crate::gui) abstraction exactly as the
//!   CLAP wiring is: the offscreen `Panel` renders frames a presenter
//!   would embed; `requestViewControllerWithCompletionHandler:` completes
//!   with `NULL` and says why.
//!
//! # Status: written, type-checked, never run
//!
//! **No byte of the Apple-target code below has executed.** It is
//! `cargo check`-clean for `aarch64-apple-darwin`, this repository's CI
//! compiles it on the macOS runner (where the pure parts' tests also
//! run), and it is written against the Objective-C runtime's and
//! clang's documented, stable ABIs — but no Apple hardware was available
//! to this sandbox, so nothing has been loaded by `AUAudioUnit`.
//!
//! Why the ABIs used here are the stable ones: the Objective-C runtime
//! functions (`objc_msgSend`, class-pair allocation) are documented C
//! entry points of `libobjc`; the blocks this module hands the host
//! (`internalRenderBlock`, the parameter tree's implementor blocks, the
//! MIDI output block) are built by hand as *block literals* exactly per
//! clang's Block Implementation Specification (`Block-ABI-03`, the ABI
//! every clang-compiled block has used since Mac OS X 10.6) — a leading
//! `isa`/`flags`/`invoke`/`descriptor` word quad plus captured data,
//! called by dispatching through `invoke` with the literal itself as the
//! first argument. The literals here use the global-concrete `isa`
//! (`_NSConcreteGlobalBlock`) with plain pointer captures, which the
//! runtime retains and releases as no-ops; the module owns their memory.
//!
//! ## The arm64-only gate
//!
//! The wrapper compiles only on `aarch64` Apple targets, and that is a
//! design decision, not an accident: returning a struct larger than 16
//! bytes (an `AudioStreamBasicDescription`, for one) goes through a
//! hidden result pointer on some architectures and registers on others,
//! and on x86_64 the struct-return flavour of `objc_msgSend` is a
//! *different symbol* (`objc_msgSend_stret`) — declaring the wrong one
//! compiles and silently corrupts at run time. arm64 has one
//! `objc_msgSend` for every return, which is why the hand-rolled calls
//! here can be honest there. Apple Silicon is also the only Apple
//! target this workspace's CI runs.
//!
//! ## What could not be verified without the SDK
//!
//! No Apple SDK is vendored in this repository (the CLAP headers are,
//! the AUv3 headers are not), so the parts below that transcribe
//! `AUAudioUnit.h` were written from its documented surface and carry an
//! honest burden:
//!
//! * **`AUInternalRenderBlock`'s event argument.** The render block's
//!   first nine parameters (action flags, timestamp, frame count, input
//!   bus, input buffers, input timestamp, output bus, output buffers,
//!   output timestamp) are transcribed; a further events-list head
//!   argument is declared opaque (`*const c_void`) and **not parsed**.
//!   On arm64 a trailing register or stack argument beyond the ones the
//!   caller pushed is simply never read, so the block is call-compatible
//!   with either arity; walking the scheduled-event list is left undone
//!   rather than guessed. Scheduled *parameters* arrive through the
//!   parameter tree's implementor blocks (the AUv3-native path) and
//!   scheduled *MIDI* through `scheduleMIDIEventList:`.
//! * **`AudioUnitParameterOptions` flag values.** The readable/writable
//!   bits this wrapper sets are transcribed from `AUComponent.h` from
//!   memory; they are the one numeric detail to re-check against the
//!   header before the first hardware run.
//! * **`AUMusicalContextBlock`'s out-parameter list** (beat position,
//!   tempo, meter, musical and downbeat positions) is transcribed
//!   best-effort; only beat and tempo are used.
//! * **Bus negotiation.** `AUAudioUnitBus`/`AUAudioUnitBusArray`/
//!   `AVAudioFormat` wiring is not transcribed at all; the render path
//!   takes channel counts from the `AudioBufferList` the host passes and
//!   activates at 48 kHz. On the first hardware run the bus formats are
//!   the first thing to add.
//!
//! Everything else — the block literal layout, the `MIDIEventList` walk,
//! the `AudioComponentDescription` and the `Info.plist` manifest — is
//! either an ABI this module pins by test or data this module generates
//! itself.
//!
//! # AAX, refused by design
//!
//! Avid's AAX (Pro Tools) is the one major format this crate will never
//! wrap: its SDK is proprietary, distributed under NDA, and tied to
//! Avid's signed-binary pace regime — it cannot be legally reimplemented
//! from documentation the way CLAP's, VST3's and LV2's C ABIs (all
//! published) or AUv3's (the Objective-C runtime plus published headers)
//! have been here. This is the same named-refusal discipline the codecs
//! use (the AAC decoder names what it refuses); see [`crate::aax`].

pub mod events;
pub mod manifest;
#[cfg(feature = "gui")]
pub mod view;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod wrapper;

/// A four-char code (`'auou'`, `'aumu'`, …): the identifier type Apple's
/// component system is built on.
///
/// AUv3 component *types* (the first field of
/// [`AudioComponentDescription`]):
///
/// | Code | Meaning | Used for |
/// |---|---|---|
/// | `aumu` | music device (multi-timbral instrument) | [`Kind::Instrument`](crate::Kind::Instrument) |
/// | `aufx` | effect | [`Kind::Effect`](crate::Kind::Effect) |
/// | `aumf` | music effect (MIDI-controlled) | [`Kind::NoteEffect`](crate::Kind::NoteEffect) |
#[must_use]
pub const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

/// The component identity a host instantiates: the C struct behind every
/// `AudioComponentDescription` in AudioToolbox, 5 × `UInt32`.
///
/// An AUv3 extension's `Info.plist` carries the same five values as
/// strings (see [`manifest`]); the host hands the struct back to the
/// principal class's `initWithComponentDescription:`. The `flags` and
/// `flagsMask` fields are zero for AUv3 (they carry the v2
/// `kAudioComponentFlag_`s; sandbox-safety is an `Info.plist` key
/// instead).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_snake_case, reason = "the header's own field names")]
pub struct AudioComponentDescription {
    /// Component type, e.g. [`fourcc`](self::fourcc)`(b"aumu")`.
    pub componentType: u32,
    /// Component subtype: the plugin's own four chars.
    pub componentSubType: u32,
    /// Manufacturer four chars, e.g. `fourcc(b"view")`.
    pub componentManufacturer: u32,
    /// Zero for AUv3.
    pub componentFlags: u32,
    /// Zero for AUv3.
    pub componentFlagsMask: u32,
}

impl AudioComponentDescription {
    /// The description for a [`PluginInfo`](crate::PluginInfo): type from
    /// the [`Kind`](crate::Kind), subtype and manufacturer derived by
    /// [`manifest::subtype`] / [`manifest::manufacturer`].
    #[must_use]
    pub fn for_info(info: &crate::PluginInfo) -> Self {
        Self {
            componentType: manifest::component_type(info),
            componentSubType: manifest::subtype(info),
            componentManufacturer: manifest::manufacturer(info),
            componentFlags: 0,
            componentFlagsMask: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;

    #[test]
    fn four_char_codes_are_big_endian() {
        assert_eq!(fourcc(b"aumu"), u32::from_be_bytes(*b"aumu"));
        assert_eq!(fourcc(b"aufx"), 0x61_75_66_78);
    }

    #[test]
    fn descriptions_carry_the_kind_and_derivation() {
        let info = crate::PluginInfo {
            id: "dev.vieww.test-gain",
            name: "Test Gain",
            vendor: "vieww",
            url: "https://vieww.dev",
            version: "1.2.3",
            description: "gain",
            kind: Kind::Effect,
            inputs: 2,
            outputs: 2,
            note_input: false,
            vst3_class_id: *b"vieww-test-gain!",
            features: &["utility"],
        };
        let d = AudioComponentDescription::for_info(&info);
        assert_eq!(d.componentType, fourcc(b"aufx"));
        assert_eq!(d.componentSubType, fourcc(b"test"), "derived in manifest");
        assert_eq!(d.componentManufacturer, fourcc(b"view"));
        assert_eq!((d.componentFlags, d.componentFlagsMask), (0, 0));
        // The plain-C layout: 5 × u32, no padding (the header is packed).
        assert_eq!(
            std::mem::size_of::<AudioComponentDescription>(),
            5 * std::mem::size_of::<u32>()
        );
    }
}
