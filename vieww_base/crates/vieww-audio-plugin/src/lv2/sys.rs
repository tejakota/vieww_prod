//! The LV2 C ABI, transcribed from the LV2 headers (ISC,
//! <https://github.com/lv2/lv2>): `lv2/core/lv2.h` for the descriptor and
//! feature, `lv2/urid/urid.h`, `lv2/atom/atom.h`, `lv2/midi/midi.h`,
//! `lv2/state/state.h` and `lv2/patch/patch.h` URIs. Field order, types
//! and constants are the headers'; names keep their C spelling so the two
//! can be read side by side. The layout tests at the bottom pin the same
//! numbers `tools/lv2_layout.c` prints from the real headers.

#![allow(
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    missing_docs,
    reason = "C ABI names"
)]

use std::ffi::{c_char, c_void};

/// An LV2_URID: an integer the host has assigned to a URI.
pub type LV2_URID = u32;
/// Host data for a URID map.
pub type LV2_URID_Handle = *mut c_void;
/// Host data for the state store/retrieve functions.
pub type LV2_State_Handle = *mut c_void;
/// A plugin instance (our `Instance` box).
pub type LV2_Handle = *mut c_void;

pub const LV2_CORE_PREFIX: &[u8] = b"http://lv2plug.in/ns/lv2core#\0";
pub const LV2_URID_PREFIX: &[u8] = b"http://lv2plug.in/ns/ext/urid#\0";
pub const LV2_URID__map: &[u8] = b"http://lv2plug.in/ns/ext/urid#map\0";
pub const LV2_URID__unmap: &[u8] = b"http://lv2plug.in/ns/ext/urid#unmap\0";
pub const LV2_ATOM_PREFIX: &[u8] = b"http://lv2plug.in/ns/ext/atom#\0";
pub const LV2_ATOM__Atom: &[u8] = b"http://lv2plug.in/ns/ext/atom#Atom\0";
pub const LV2_ATOM__Chunk: &[u8] = b"http://lv2plug.in/ns/ext/atom#Chunk\0";
pub const LV2_ATOM__Bool: &[u8] = b"http://lv2plug.in/ns/ext/atom#Bool\0";
pub const LV2_ATOM__Int: &[u8] = b"http://lv2plug.in/ns/ext/atom#Int\0";
pub const LV2_ATOM__Long: &[u8] = b"http://lv2plug.in/ns/ext/atom#Long\0";
pub const LV2_ATOM__Float: &[u8] = b"http://lv2plug.in/ns/ext/atom#Float\0";
pub const LV2_ATOM__Double: &[u8] = b"http://lv2plug.in/ns/ext/atom#Double\0";
pub const LV2_ATOM__URID: &[u8] = b"http://lv2plug.in/ns/ext/atom#URID\0";
pub const LV2_ATOM__String: &[u8] = b"http://lv2plug.in/ns/ext/atom#String\0";
pub const LV2_ATOM__Object: &[u8] = b"http://lv2plug.in/ns/ext/atom#Object\0";
pub const LV2_ATOM__Sequence: &[u8] = b"http://lv2plug.in/ns/ext/atom#Sequence\0";
pub const LV2_ATOM__frameTime: &[u8] = b"http://lv2plug.in/ns/ext/atom#frameTime\0";
pub const LV2_ATOM__beatTime: &[u8] = b"http://lv2plug.in/ns/ext/atom#beatTime\0";
pub const LV2_MIDI_PREFIX: &[u8] = b"http://lv2plug.in/ns/ext/midi#\0";
pub const LV2_MIDI__MidiEvent: &[u8] = b"http://lv2plug.in/ns/ext/midi#MidiEvent\0";
pub const LV2_PATCH_PREFIX: &[u8] = b"http://lv2plug.in/ns/ext/patch#\0";
pub const LV2_PATCH__Get: &[u8] = b"http://lv2plug.in/ns/ext/patch#Get\0";
pub const LV2_PATCH__Set: &[u8] = b"http://lv2plug.in/ns/ext/patch#Set\0";
pub const LV2_PATCH__Put: &[u8] = b"http://lv2plug.in/ns/ext/patch#Put\0";
pub const LV2_PATCH__subject: &[u8] = b"http://lv2plug.in/ns/ext/patch#subject\0";
pub const LV2_PATCH__property: &[u8] = b"http://lv2plug.in/ns/ext/patch#property\0";
pub const LV2_PATCH__value: &[u8] = b"http://lv2plug.in/ns/ext/patch#value\0";
pub const LV2_STATE_PREFIX: &[u8] = b"http://lv2plug.in/ns/ext/state#\0";
pub const LV2_STATE__interface: &[u8] = b"http://lv2plug.in/ns/ext/state#interface\0";
pub const LV2_LATENCY_PREFIX: &[u8] = b"http://lv2plug.in/ns/ext/latency#\0";
pub const LV2_LATENCY__latency: &[u8] = b"http://lv2plug.in/ns/ext/latency#latency\0";

/// A feature the host passes at instantiation.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Feature {
    pub URI: *const c_char,
    pub data: *mut c_void,
}

/// URID → integer mapping (`urid:map`, required).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_URID_Map {
    pub handle: LV2_URID_Handle,
    pub map: Option<unsafe extern "C" fn(handle: LV2_URID_Handle, uri: *const c_char) -> LV2_URID>,
}

/// Integer → URID mapping (`urid:unmap`, optional here).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_URID_Unmap {
    pub handle: LV2_URID_Handle,
    pub unmap:
        Option<unsafe extern "C" fn(handle: LV2_URID_Handle, urid: LV2_URID) -> *const c_char>,
}

/// The plugin descriptor `lv2_descriptor` hands out.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Descriptor {
    /// The plugin's URI.
    pub URI: *const c_char,
    pub instantiate: Option<
        unsafe extern "C" fn(
            descriptor: *const LV2_Descriptor,
            sample_rate: f64,
            bundle_path: *const c_char,
            features: *const *const LV2_Feature,
        ) -> LV2_Handle,
    >,
    pub connect_port:
        Option<unsafe extern "C" fn(instance: LV2_Handle, port: u32, data: *mut c_void)>,
    pub activate: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub run: Option<unsafe extern "C" fn(instance: LV2_Handle, n_samples: u32)>,
    pub deactivate: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub cleanup: Option<unsafe extern "C" fn(instance: LV2_Handle)>,
    pub extension_data: Option<unsafe extern "C" fn(uri: *const c_char) -> *const c_void>,
}

/* ---- atom -------------------------------------------------------------- */

/// The atom header every atom starts with.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom {
    /// Total size of the body, in bytes (not counting this header).
    pub size: u32,
    /// Type URID.
    pub type_: LV2_URID,
}

/// A float atom.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Float {
    pub atom: LV2_Atom,
    pub body: f32,
}

/// A double atom.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Double {
    pub atom: LV2_Atom,
    pub body: f64,
}

/// A URID atom.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_URID {
    pub atom: LV2_Atom,
    pub body: LV2_URID,
}

/// The body of an atom:Object: an id and a type, then properties.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Object_Body {
    /// URID of the subject (atom:Resource) or blank id (atom:Blank).
    pub id: LV2_URID,
    /// URID of the object type (patch:Set, patch:Get …).
    pub otype: LV2_URID,
}

/// An atom:Object.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Object {
    pub atom: LV2_Atom,
    pub body: LV2_Atom_Object_Body,
}

/// One property of an object: key, context, then the value atom.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Property_Body {
    /// Key (predicate) URID.
    pub key: LV2_URID,
    /// Context URID, or 0.
    pub context: LV2_URID,
    /// Value atom header; its body follows.
    pub value: LV2_Atom,
}

/// An atom:Property (an atom header plus a property body).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Property {
    pub atom: LV2_Atom,
    pub body: LV2_Atom_Property_Body,
}

/// The body of a sequence: the time unit, then events.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Sequence_Body {
    /// URID of the event time unit (atom:frameTime or atom:beatTime).
    pub unit: LV2_URID,
    /// Currently unused.
    pub pad: u32,
}

/// An atom:Sequence.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_Atom_Sequence {
    pub atom: LV2_Atom,
    pub body: LV2_Atom_Sequence_Body,
}

/// An event in a sequence: a 64-bit time then the event's atom.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Atom_Event {
    /// Time, read as frames (int64) or beats (double) per the sequence unit.
    pub time: EventTime,
    /// The event's atom header; its body follows.
    pub body: LV2_Atom,
}

/// The event time union (C anonymous; named here to read both ways).
#[repr(C)]
#[derive(Clone, Copy)]
pub union EventTime {
    /// Frames when the sequence unit is atom:frameTime.
    pub frames: i64,
    /// Beats when the sequence unit is atom:beatTime.
    pub beats: f64,
}

impl std::fmt::Debug for EventTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SAFETY: both union fields are trivially valid to read.
        let (frames, beats) = unsafe { (self.frames, self.beats) };
        write!(f, "EventTime({frames} frames, {beats} beats)")
    }
}

impl std::fmt::Debug for LV2_Atom_Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SAFETY: a union read of the raw 64 bits; both are trivially valid.
        let (frames, beats) = unsafe { (self.time.frames, self.time.beats) };
        f.debug_struct("LV2_Atom_Event")
            .field("frames", &frames)
            .field("beats", &beats)
            .field("size", &self.body.size)
            .field("type", &self.body.type_)
            .finish()
    }
}

/// An event's footprint: 16 header bytes plus the atom body.
#[must_use]
pub const fn size_of_event(ev: &LV2_Atom_Event) -> usize {
    16 + ev.body.size as usize
}

/// Size of an event, as `lv2_atom_sequence_next` steps it: the fixed 16
/// header bytes plus the atom body padded to 64 bits.
#[must_use]
pub const fn event_stride(ev: &LV2_Atom_Event) -> usize {
    16 + pad_size(ev.body.size as usize)
}

/// Round a byte size up to a multiple of 8 (`lv2_atom_pad_size`).
#[must_use]
pub const fn pad_size(size: usize) -> usize {
    (size + 7) & !7
}

/* ---- state ------------------------------------------------------------- */

/// State store/restore status codes (`LV2_State_Status`).
pub const LV2_STATE_SUCCESS: i32 = 0;
pub const LV2_STATE_ERR_UNKNOWN: i32 = 1;
pub const LV2_STATE_ERR_BAD_TYPE: i32 = 2;
pub const LV2_STATE_ERR_BAD_FLAGS: i32 = 3;
pub const LV2_STATE_ERR_NO_FEATURE: i32 = 4;
pub const LV2_STATE_ERR_NO_PROPERTY: i32 = 5;
pub const LV2_STATE_ERR_NO_SPACE: i32 = 6;

/// State value flags. The vieww state blob is a versioned little-endian
/// byte format, so both POD and PORTABLE are true of it.
pub const LV2_STATE_IS_POD: u32 = 1;
pub const LV2_STATE_IS_PORTABLE: u32 = 2;

/// The function a plugin calls to store one property.
pub type LV2_State_Store_Function = Option<
    unsafe extern "C" fn(
        handle: LV2_State_Handle,
        key: LV2_URID,
        value: *const c_void,
        size: usize,
        type_: u32,
        flags: u32,
    ) -> i32,
>;

/// The function a plugin calls to retrieve one property.
pub type LV2_State_Retrieve_Function = Option<
    unsafe extern "C" fn(
        handle: LV2_State_Handle,
        key: LV2_URID,
        size: *mut usize,
        type_: *mut u32,
        flags: *mut u32,
    ) -> *const c_void,
>;

/// The state extension (`state:interface`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LV2_State_Interface {
    pub save: Option<
        unsafe extern "C" fn(
            instance: LV2_Handle,
            store: LV2_State_Store_Function,
            handle: LV2_State_Handle,
            flags: u32,
            features: *const *const LV2_Feature,
        ) -> i32,
    >,
    pub restore: Option<
        unsafe extern "C" fn(
            instance: LV2_Handle,
            retrieve: LV2_State_Retrieve_Function,
            handle: LV2_State_Handle,
            flags: u32,
            features: *const *const LV2_Feature,
        ) -> i32,
    >,
}

/// Layout checks: every number here is printed by a C program compiled
/// against the official LV2 headers (x86-64 Linux); `tools/check-plugins.sh`
/// recompiles that program so CI re-derives them on each platform.
#[cfg(all(test, target_pointer_width = "64"))]
mod layout {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn sizes_and_offsets_match_the_headers() {
        assert_eq!(size_of::<LV2_Feature>(), 16);
        assert_eq!(size_of::<LV2_URID_Map>(), 16);
        assert_eq!(size_of::<LV2_URID_Unmap>(), 16);
        assert_eq!(size_of::<LV2_Descriptor>(), 64);
        assert_eq!(offset_of!(LV2_Descriptor, instantiate), 8);
        assert_eq!(offset_of!(LV2_Descriptor, run), 32);
        assert_eq!(offset_of!(LV2_Descriptor, extension_data), 56);
        assert_eq!(size_of::<LV2_Atom>(), 8);
        assert_eq!(size_of::<LV2_Atom_Float>(), 12);
        assert_eq!(size_of::<LV2_Atom_Double>(), 16);
        assert_eq!(size_of::<LV2_Atom_URID>(), 12);
        assert_eq!(size_of::<LV2_Atom_Object_Body>(), 8);
        assert_eq!(size_of::<LV2_Atom_Object>(), 16);
        assert_eq!(size_of::<LV2_Atom_Property_Body>(), 16);
        assert_eq!(offset_of!(LV2_Atom_Property_Body, value), 8);
        assert_eq!(size_of::<LV2_Atom_Property>(), 24);
        assert_eq!(size_of::<LV2_Atom_Sequence_Body>(), 8);
        assert_eq!(size_of::<LV2_Atom_Sequence>(), 16);
        assert_eq!(size_of::<LV2_Atom_Event>(), 16);
        assert_eq!(offset_of!(LV2_Atom_Event, body), 8);
        assert_eq!(size_of::<LV2_State_Interface>(), 16);
    }
}
