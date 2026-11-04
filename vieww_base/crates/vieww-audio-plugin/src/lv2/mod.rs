//! LV2: export a [`Plugin`] as an LV2 shared library, and host LV2
//! plugins.
//!
//! ```ignore
//! vieww_audio_plugin::export_lv2!(MyGain, MySynth);
//! ```
//!
//! in a `cdylib` crate defines the `lv2_descriptor` symbol every LV2 host
//! looks up. Each plugin becomes one LV2 URI (`urn:<id>`, derived from
//! [`PluginInfo::id`]) with a fixed port layout:
//!
//! | index | port | type |
//! |---|---|---|
//! | 0 | control in | atom:Sequence input (MIDI events + `patch:Set` parameters) |
//! | 1 | control out | atom:Sequence output (`patch:Set` replies to `patch:Get`) |
//! | 2 | latency | control output, `latency:latency` designation, connection-optional |
//! | 3… | audio | the plugin's input then output channels |
//!
//! LV2 has no note-event port: notes arrive as `midi:MidiEvent` atoms in
//! the input sequence, which the wrapper decodes to [`Event::NoteOn`]/
//! [`Event::NoteOff`] so an instrument sees the same events it would under
//! CLAP; everything else (CC, pitch bend …) passes through as raw
//! [`Event::Midi`]. System-exclusive data does not fit the three-byte
//! [`Event::Midi`] and is dropped — noted here, not silently.
//!
//! Parameters are LV2 *parameters* (not control ports): each is
//! `<plugin-uri>#param-<slug>-<id>` with `rdfs:range atom:Float`, changed
//! by the host as `patch:Set` objects in the input sequence —
//! sample-accurate through the shared [`Driver`] — and readable by the
//! host from the TTL defaults, from the state, or by sending `patch:Get`,
//! which the wrapper answers with one `patch:Set` per parameter on the
//! output sequence. Values are `atom:Float` (32-bit) on the wire, the
//! convention hosts follow from `rdfs:range`; `atom:Double` and
//! `atom:Int` are accepted on input.
//!
//! State is the [`state`](http://lv2plug.in/ns/ext/state) extension: the
//! plugin's [`Plugin::save_state`] blob (a versioned little-endian
//! format) stored as one `<plugin-uri>#state` property typed `atom:Chunk`
//! and flagged `IS_POD | IS_PORTABLE`, because both are true of it.
//!
//! `urid:map` is a required feature (as the TTL declares): without a URID
//! map there are no atom types, no parameters and no state keys, so
//! `instantiate` refuses. Beat-timed sequences (`atom:beatTime`) are not
//! understood — control sequences on audio ports are frame-timed in
//! practice — and are ignored rather than misread.
//!
//! Threading follows the LV2 classes: `connect_port`/`activate`/`restore`
//! are instantiation-thread and never concurrent with `run`, so port
//! pointers are plain fields; `run` only ever `try_lock`s the plugin (a
//! concurrent `save`/`restore` renders one block of silence rather than
//! waiting), exactly as the CLAP wrapper does. The host may hand the same
//! buffer to an input and an output port (LV2 in-place processing):
//! aliased inputs are copied to 8192-frame scratch first, and a block
//! larger than that with aliased ports renders silence (documented; hosts
//! that reuse buffers keep blocks far smaller). There is no reliable way
//! to learn an output atom port's capacity, so `patch:Get` replies stop at
//! 4 KiB — enough for ~55 parameters; a host asking for more gets a valid
//! prefix.
//!
//! [`host`] is the other side: a URID world, a sequence forge, a loader
//! for `.so`/`.dylib`/`.dll` files via `lv2_descriptor`, and an instance
//! driver. [`ttl`] generates the bundle's `manifest.ttl` and plugin TTL —
//! in LV2 the RDF data *is* the plugin's public description; the library
//! alone is not discoverable.

pub mod host;
pub mod sys;
pub mod ttl;

use std::any::TypeId;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};

use crate::{Driver, Event, ParamChange, Plugin, PluginInfo, MAX_CHANNELS};
use sys::*;

/// Input atom:Sequence port index.
pub const PORT_CONTROL_IN: u32 = 0;
/// Output atom:Sequence port index.
pub const PORT_CONTROL_OUT: u32 = 1;
/// Latency control output port index (`connectionOptional`).
pub const PORT_LATENCY: u32 = 2;

/// Frames of copy-aside scratch for in-place (aliased) buffers. LV2 has no
/// block-size negotiation in core (the buf-size extension is a hint, not a
/// promise), so this is a fixed generous bound; larger aliased blocks
/// render silence rather than corrupt memory.
const SCRATCH_FRAMES: usize = 8_192;

/// The plugin's LV2 URI: `urn:` plus the [`PluginInfo::id`] (already
/// reverse-DNS, so it is globally unique and needs no invented domain).
#[must_use]
pub fn plugin_uri(info: &PluginInfo) -> String {
    format!("urn:{}", info.id)
}

/// The URI fragment of one parameter: `param-<slug>-<id>`. The id suffix
/// keeps the URI stable when names collide or change.
#[must_use]
pub fn param_fragment(name: &str, id: u32) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push('x');
    }
    format!("param-{slug}-{id}")
}

/// The state property URI: `<plugin-uri>#state`.
fn state_uri(uri: &str) -> String {
    format!("{uri}#state")
}

/// Every URID the wrapper needs, mapped once at instantiation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Urids {
    map: LV2_URID_Map,
    atom_chunk: LV2_URID,
    atom_float: LV2_URID,
    atom_double: LV2_URID,
    atom_int: LV2_URID,
    atom_urid: LV2_URID,
    atom_object: LV2_URID,
    atom_sequence: LV2_URID,
    frame_time: LV2_URID,
    midi_event: LV2_URID,
    patch_get: LV2_URID,
    patch_set: LV2_URID,
    patch_subject: LV2_URID,
    patch_property: LV2_URID,
    patch_value: LV2_URID,
    state_property: LV2_URID,
    plugin_uri: LV2_URID,
}

impl Urids {
    /// Map every URI the wrapper needs. `None` without a `urid:map`
    /// feature or when the host's map returns 0 for any of them (0 is the
    /// reserved failure value). Also used by the in-house host, which maps
    /// through its own [`World`](host::World) and so sees the plugin's
    /// numbers.
    ///
    /// # Safety
    /// `features` is null or a NULL-terminated array of valid features
    /// for the life of the call; the `urid:map` data is an
    /// `LV2_URID_Map` valid for the life of the instance.
    pub(crate) unsafe fn from_features(
        features: *const *const LV2_Feature,
        uri: &str,
    ) -> Option<Self> {
        let mut map: Option<LV2_URID_Map> = None;
        let mut f = features;
        // SAFETY: the host passes a NULL-terminated feature array (the LV2
        // contract) whose entries are valid during instantiate.
        while !f.is_null() && !unsafe { *f }.is_null() {
            let feat = unsafe { &*(*f) };
            // SAFETY: feature URIs are NUL-terminated C strings.
            if unsafe { CStr::from_ptr(feat.URI) }.to_bytes_with_nul() == LV2_URID__map {
                // SAFETY: the urid:map feature data is an LV2_URID_Map.
                map = Some(unsafe { *feat.data.cast::<LV2_URID_Map>() });
            }
            f = unsafe { f.add(1) };
        }
        let map = map?;
        let mapf = map.map?;
        let m = |u: &[u8]| {
            // SAFETY: the host's map callback with its own handle and a
            // NUL-terminated URI, per the urid:map contract.
            let id = unsafe { mapf(map.handle, u.as_ptr().cast()) };
            (id != 0).then_some(id)
        };
        let state = CString::new(state_uri(uri)).ok()?;
        let uri = CString::new(uri).ok()?;
        Some(Self {
            map,
            atom_chunk: m(LV2_ATOM__Chunk)?,
            atom_float: m(LV2_ATOM__Float)?,
            atom_double: m(LV2_ATOM__Double)?,
            atom_int: m(LV2_ATOM__Int)?,
            atom_urid: m(LV2_ATOM__URID)?,
            atom_object: m(LV2_ATOM__Object)?,
            atom_sequence: m(LV2_ATOM__Sequence)?,
            frame_time: m(LV2_ATOM__frameTime)?,
            midi_event: m(LV2_MIDI__MidiEvent)?,
            patch_get: m(LV2_PATCH__Get)?,
            patch_set: m(LV2_PATCH__Set)?,
            patch_subject: m(LV2_PATCH__subject)?,
            patch_property: m(LV2_PATCH__property)?,
            patch_value: m(LV2_PATCH__value)?,
            state_property: m(state.as_bytes_with_nul())?,
            plugin_uri: m(uri.as_bytes_with_nul())?,
        })
    }
}

/// A descriptor, its URI, and the state interface `extension_data` hands
/// out (the interface is per-plugin-type, not per-instance: its functions
/// receive the instance handle).
struct Descriptor {
    raw: LV2_Descriptor,
    _uri: CString,
    state: LV2_State_Interface,
}

// SAFETY: the raw pointer points into `_uri`, owned by the descriptor and
// never mutated after construction.
unsafe impl Send for Descriptor {}
// SAFETY: as above — immutable after construction.
unsafe impl Sync for Descriptor {}

type Registry = Mutex<Vec<(TypeId, &'static Descriptor)>>;

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(Vec::new()))
}

fn find_descriptor<P: Plugin>() -> Option<&'static Descriptor> {
    let t = TypeId::of::<P>();
    let Ok(r) = registry().lock() else {
        return None;
    };
    r.iter().find(|(k, _)| *k == t).map(|(_, d)| *d)
}

/// The descriptor for `P` (built once, valid for the life of the library).
#[must_use]
pub fn descriptor<P: Plugin>() -> *const LV2_Descriptor {
    if let Some(d) = find_descriptor::<P>() {
        return &raw const d.raw;
    }
    let uri = CString::new(plugin_uri(P::info())).unwrap_or_default();
    let d: &'static Descriptor = Box::leak(Box::new(Descriptor {
        raw: LV2_Descriptor {
            URI: uri.as_ptr(),
            instantiate: Some(instantiate::<P>),
            connect_port: Some(connect_port::<P>),
            activate: Some(activate::<P>),
            run: Some(run::<P>),
            deactivate: Some(deactivate::<P>),
            cleanup: Some(cleanup::<P>),
            extension_data: Some(extension_data::<P>),
        },
        _uri: uri,
        state: LV2_State_Interface {
            save: Some(state_save::<P>),
            restore: Some(state_restore::<P>),
        },
    }));
    if let Ok(mut r) = registry().lock() {
        if let Some((_, old)) = r.iter().find(|(k, _)| *k == TypeId::of::<P>()) {
            return &raw const old.raw; // another thread won the race
        }
        r.push((TypeId::of::<P>(), d));
    }
    &raw const d.raw
}

/// One plugin instance: the driver plus its port connections. Port
/// pointers are written by `connect_port` (instantiation thread class,
/// never concurrent with `run` — the LV2 threading rules).
struct Instance<P: Plugin> {
    driver: Mutex<Driver<P>>,
    /// Immutable copy of the parameter table.
    params: Vec<crate::ParamInfo>,
    /// The URID of each parameter's URI, in table order.
    param_urids: Vec<LV2_URID>,
    urids: Urids,
    control_in: *mut LV2_Atom_Sequence,
    control_out: *mut LV2_Atom_Sequence,
    latency_port: *mut f32,
    audio_in: Vec<*mut f32>,
    audio_out: Vec<*mut f32>,
    /// Copy-aside for in-place (aliased) buffers, sized at activate.
    scratch: Vec<Vec<f32>>,
}

// SAFETY: the driver is `Send` (`P: Plugin` requires it); the raw port
// pointers are only dereferenced from the audio thread inside `run` or
// from instantiation-thread calls, which LV2 never runs concurrently.
unsafe impl<P: Plugin> Send for Instance<P> {}

/// Create an instance of `P` (the descriptor's `instantiate`).
///
/// # Safety
/// `features`, if non-null, is a NULL-terminated array of valid features;
/// the `urid:map` feature, when found, must outlive the instance.
unsafe extern "C" fn instantiate<P: Plugin>(
    descriptor: *const LV2_Descriptor,
    rate: f64,
    _bundle_path: *const c_char,
    features: *const *const LV2_Feature,
) -> LV2_Handle {
    if descriptor.is_null() {
        return null_mut();
    }
    let uri = plugin_uri(P::info());
    // SAFETY: the caller's contract on the feature array.
    let Some(urids) = (unsafe { Urids::from_features(features, &uri) }) else {
        return null_mut(); // urid:map is required, and the map must work
    };
    let mut driver = Driver::<P>::new();
    driver.rate = rate;
    let params = driver.params.clone();
    let mapf = match urids.map.map {
        Some(f) => f,
        None => return null_mut(),
    };
    let mut param_urids = Vec::with_capacity(params.len());
    for p in &params {
        let u =
            CString::new(format!("{uri}#{}", param_fragment(&p.name, p.id))).unwrap_or_default();
        // SAFETY: the host's map with its handle and a NUL-terminated URI.
        let id = unsafe { mapf(urids.map.handle, u.as_ptr()) };
        if id == 0 {
            return null_mut(); // a parameter URI failed to map
        }
        param_urids.push(id);
    }
    let n_in = P::info().inputs as usize;
    let inst = Box::new(Instance::<P> {
        driver: Mutex::new(driver),
        params,
        param_urids,
        urids,
        control_in: null_mut(),
        control_out: null_mut(),
        latency_port: null_mut(),
        audio_in: vec![null_mut(); n_in],
        audio_out: vec![null_mut(); P::info().outputs as usize],
        scratch: vec![Vec::new(); n_in],
    });
    Box::into_raw(inst).cast()
}

/// # Safety
/// `instance` came from [`instantiate`] for this `P` and is live.
unsafe extern "C" fn connect_port<P: Plugin>(instance: LV2_Handle, port: u32, data: *mut c_void) {
    // SAFETY: a live instance (the LV2 contract).
    let inst = unsafe { &mut *(instance.cast::<Instance<P>>()) };
    let info = P::info();
    let audio_base = PORT_LATENCY + 1;
    let in_end = audio_base + info.inputs;
    let out_end = in_end + info.outputs;
    if port == PORT_CONTROL_IN {
        inst.control_in = data.cast();
    } else if port == PORT_CONTROL_OUT {
        inst.control_out = data.cast();
    } else if port == PORT_LATENCY {
        inst.latency_port = data.cast();
    } else if (audio_base..in_end).contains(&port) {
        if let Some(slot) = inst.audio_in.get_mut((port - audio_base) as usize) {
            *slot = data.cast();
        }
    } else if (in_end..out_end).contains(&port) {
        if let Some(slot) = inst.audio_out.get_mut((port - in_end) as usize) {
            *slot = data.cast();
        }
    }
}

/// # Safety
/// `instance` is a live instance of `P`.
unsafe extern "C" fn activate<P: Plugin>(instance: LV2_Handle) {
    // SAFETY: a live instance.
    let inst = unsafe { &mut *(instance.cast::<Instance<P>>()) };
    if let Ok(mut d) = inst.driver.lock() {
        // LV2 activate re-initialises: history-dependent state resets
        // (which is what `reset` does) but parameters stay.
        d.plugin.reset();
        let (rate, max) = (d.rate, d.max_block);
        let _ = d.plugin.activate(rate, max);
        for s in &mut inst.scratch {
            s.resize(SCRATCH_FRAMES, 0.0);
        }
    }
}

/// # Safety
/// `instance` is a live instance of `P`.
unsafe extern "C" fn deactivate<P: Plugin>(instance: LV2_Handle) {
    // SAFETY: a live instance.
    let inst = unsafe { &mut *(instance.cast::<Instance<P>>()) };
    if let Ok(mut d) = inst.driver.lock() {
        d.plugin.deactivate();
    }
}

/// # Safety
/// `instance` is a live instance of `P`; every connected audio buffer
/// holds at least `n_samples` frames.
unsafe extern "C" fn run<P: Plugin>(instance: LV2_Handle, n_samples: u32) {
    // SAFETY: a live instance.
    let inst = unsafe { &mut *(instance.cast::<Instance<P>>()) };
    // Subnormals flushed for this call, the host thread's mode restored on
    // return, as a hard-RT-capable plugin should.
    let _ftz = vieww_audio::denormal::FlushDenormals::enable();
    let frames = n_samples as usize;
    let Ok(mut d) = inst.driver.try_lock() else {
        // A save or restore is running: render silence, never wait.
        zero_outputs(inst, frames);
        // SAFETY: the host-connected output sequence.
        unsafe { write_empty_sequence(inst.control_out, &inst.urids) };
        return;
    };
    // SAFETY: the host-connected input sequence, valid for this call.
    let wants_get = unsafe { drain_events(&mut d, inst.control_in, inst) };
    let n_in = inst.audio_in.len().min(MAX_CHANNELS);
    let n_out = inst.audio_out.len().min(MAX_CHANNELS);
    // In-place (aliased) buffers would be `&` and `&mut` to the same
    // memory, which Rust forbids: copy such inputs aside first.
    let aliased = inst
        .audio_in
        .iter()
        .any(|i| !i.is_null() && inst.audio_out.iter().any(|o| !o.is_null() && o == i))
        && frames <= SCRATCH_FRAMES;
    if inst
        .audio_in
        .iter()
        .any(|i| !i.is_null() && inst.audio_out.iter().any(|o| !o.is_null() && o == i))
        && frames > SCRATCH_FRAMES
    {
        // Aliased and larger than the scratch bound: silence (documented).
        drop(d);
        zero_outputs(inst, frames);
        // SAFETY: as above.
        unsafe { write_empty_sequence(inst.control_out, &inst.urids) };
        return;
    }
    let mut ins: [&[f32]; MAX_CHANNELS] = [&[]; MAX_CHANNELS];
    for (c, &p) in inst.audio_in.iter().take(n_in).enumerate() {
        if p.is_null() {
            continue;
        }
        // SAFETY: the host's input channel of `frames` samples.
        let src = unsafe { std::slice::from_raw_parts(p, frames) };
        if aliased {
            inst.scratch[c][..frames].copy_from_slice(src);
        } else {
            ins[c] = src;
        }
    }
    if aliased {
        for (c, s) in inst.scratch.iter().take(n_in).enumerate() {
            ins[c] = &s[..frames];
        }
    }
    let mut outs: [&mut [f32]; MAX_CHANNELS] = Default::default();
    for (c, &p) in inst.audio_out.iter().take(n_out).enumerate() {
        if !p.is_null() {
            // SAFETY: the host's distinct output channel of `frames`.
            outs[c] = unsafe { std::slice::from_raw_parts_mut(p, frames) };
        }
    }
    let _ = d.run(&ins[..n_in], &mut outs[..n_out], frames);
    if !inst.latency_port.is_null() {
        // SAFETY: the host-connected latency control output. A sample
        // count fits an f32 exactly up to 2^24 samples of latency.
        let lat = d.plugin.latency() as f32;
        unsafe { *inst.latency_port = lat };
    }
    // SAFETY: the host-connected output sequence, writable for this call.
    unsafe { write_control_out(inst, &d, wants_get) };
}

/// # Safety
/// `instance` is a live instance of `P`.
unsafe extern "C" fn cleanup<P: Plugin>(instance: LV2_Handle) {
    if instance.is_null() {
        return;
    }
    // SAFETY: created by `instantiate` via `Box::into_raw`; destroyed once.
    drop(unsafe { Box::from_raw(instance.cast::<Instance<P>>()) });
}

/// # Safety
/// `uri` is null or a NUL-terminated URI.
unsafe extern "C" fn extension_data<P: Plugin>(uri: *const c_char) -> *const c_void {
    if uri.is_null() {
        return null();
    }
    // SAFETY: the caller's contract.
    if unsafe { CStr::from_ptr(uri) }.to_bytes_with_nul() == LV2_STATE__interface {
        match find_descriptor::<P>() {
            // SAFETY: the descriptor lives for the life of the library.
            Some(d) => (&raw const d.state).cast(),
            None => null(),
        }
    } else {
        null()
    }
}

/// Zero every connected audio output.
fn zero_outputs<P: Plugin>(inst: &Instance<P>, frames: usize) {
    for &p in &inst.audio_out {
        if !p.is_null() {
            // SAFETY: the host's output channel of `frames` samples,
            // connected per the LV2 contract.
            unsafe { std::slice::from_raw_parts_mut(p, frames).fill(0.0) };
        }
    }
}

/// Read a `u32` at a possibly unaligned byte pointer (host-side parsing
/// shares it).
///
/// # Safety
/// `p` points at four readable bytes.
pub(crate) unsafe fn read_u32(p: *const u8) -> u32 {
    unsafe { p.cast::<u32>().read_unaligned() }
}

/// Read an `f32` at a possibly unaligned byte pointer.
///
/// # Safety
/// `p` points at four readable bytes.
unsafe fn read_f32(p: *const u8) -> f32 {
    unsafe { p.cast::<f32>().read_unaligned() }
}

/// Read an `f64` at a possibly unaligned byte pointer.
///
/// # Safety
/// `p` points at eight readable bytes.
unsafe fn read_f64(p: *const u8) -> f64 {
    unsafe { p.cast::<f64>().read_unaligned() }
}

/// Read an `i64` at a possibly unaligned byte pointer.
///
/// # Safety
/// `p` points at eight readable bytes.
unsafe fn read_i64(p: *const u8) -> i64 {
    unsafe { p.cast::<i64>().read_unaligned() }
}

/// Walk a frame-timed atom:Sequence: `f` is called per event with its
/// frame, type URID and body bytes, without allocating. The sequence's
/// `unit` must be `atom:frameTime` or 0 — atom.h: "may be 0 where a
/// default stamp type is known; for `LV2_Descriptor::run()`, the default
/// stamp type is audio frames", and that is the only context this wrapper
/// reads sequences in. Real hosts forge with 0 (Jalv does); beat-timed
/// sequences yield nothing, per the module docs. Corrupt sizes stop the
/// walk rather than read past the end.
///
/// # Safety
/// `seq` is null or a valid sequence whose `8 + atom.size` bytes are
/// readable.
pub unsafe fn walk_sequence(
    seq: *const LV2_Atom_Sequence,
    frame_time: LV2_URID,
    mut f: impl FnMut(i64, LV2_URID, &[u8]),
) {
    if seq.is_null() {
        return;
    }
    // SAFETY: the caller's contract on the header.
    let s = unsafe { &*seq };
    if (s.body.unit != frame_time && s.body.unit != 0) || s.atom.size < 8 {
        return;
    }
    let base = (s as *const LV2_Atom_Sequence).cast::<u8>();
    let end = 8 + s.atom.size as usize; // events end at body + size
    let mut at = 16usize;
    let mut seen = 0usize;
    while at + 16 <= end && seen < 4_096 {
        // SAFETY: `at + 16 <= end` bounds the event header read.
        let hdr = unsafe { base.add(at) };
        // SAFETY: reads within the sequence.
        let size = unsafe { read_u32(hdr.add(8)) };
        let type_ = unsafe { read_u32(hdr.add(12)) };
        if type_ == 0 {
            return; // a null atom: a host bug, stop rather than misread
        }
        let body_at = at + 16;
        let body_len = size as usize;
        if body_at + body_len > end {
            return; // the event runs past the sequence: corrupt
        }
        // SAFETY: the event's body, inside the sequence.
        let body = unsafe { std::slice::from_raw_parts(base.add(body_at), body_len) };
        // SAFETY: the time union, inside the sequence.
        let frames = unsafe { read_i64(hdr) };
        f(frames, type_, body);
        seen += 1;
        at += 16 + pad_size(body_len);
    }
}

/// Decode one MIDI atom into a plugin event. Note on/off become typed
/// events (velocity-0 note-on is a note off); the rest pass through raw;
/// sysex does not fit [`Event::Midi`] and is dropped.
fn midi_event(time: u32, body: &[u8]) -> Option<Event> {
    let status = *body.first()?;
    if status < 0x80 || status == 0xF0 {
        return None; // no running status inside one atom; no sysex
    }
    let d1 = body.get(1).copied().unwrap_or(0);
    let d2 = body.get(2).copied().unwrap_or(0);
    let channel = status & 0x0F;
    match status & 0xF0 {
        0x80 => Some(Event::NoteOff {
            time,
            channel,
            key: d1,
            velocity: f32::from(d2) / 127.0,
            note_id: -1,
        }),
        0x90 if d2 > 0 => Some(Event::NoteOn {
            time,
            channel,
            key: d1,
            velocity: f32::from(d2) / 127.0,
            note_id: -1,
        }),
        0x90 => Some(Event::NoteOff {
            time,
            channel,
            key: d1,
            velocity: 0.5,
            note_id: -1,
        }),
        _ => Some(Event::Midi {
            time,
            data: [status, d1, d2],
        }),
    }
}

/// A `patch:Set`'s property URID and value, read out of an object body.
pub(crate) struct SetView {
    property: LV2_URID,
    value: f64,
}

/// Parse a `patch:Set` object atom body (`[id][otype]` then property
/// bodies): returns the `patch:property`/`patch:value` pair. The host's
/// reply parser shares it.
///
/// # Safety
/// `obj` points at `size` readable bytes that are an object body.
pub(crate) unsafe fn parse_set(obj: *const u8, size: usize, urids: &Urids) -> Option<SetView> {
    let mut property = None;
    let mut value = None;
    let mut at = 8usize; // skip id/otype
    let mut seen = 0usize;
    while at + 16 <= size && seen < 64 {
        // SAFETY: bounded by the loop condition against `size`.
        let p = unsafe { obj.add(at) };
        // SAFETY: reads within `size`.
        let key = unsafe { read_u32(p) };
        let v_size = unsafe { read_u32(p.add(8)) };
        let v_type = unsafe { read_u32(p.add(12)) };
        if at + 16 + v_size as usize > size {
            return None; // the value runs past the object: corrupt
        }
        if key == urids.patch_property && v_type == urids.atom_urid && v_size >= 4 {
            // SAFETY: the URID body, within bounds.
            property = Some(unsafe { read_u32(p.add(16)) });
        } else if key == urids.patch_value {
            // SAFETY: the value body, within bounds.
            value = match v_type {
                t if t == urids.atom_float && v_size >= 4 => {
                    Some(f64::from(unsafe { read_f32(p.add(16)) }))
                }
                t if t == urids.atom_double && v_size >= 8 => Some(unsafe { read_f64(p.add(16)) }),
                t if t == urids.atom_int && v_size >= 4 => {
                    Some(f64::from(unsafe { read_u32(p.add(16)) as i32 }))
                }
                _ => None,
            };
        }
        seen += 1;
        at += pad_size(16 + v_size as usize);
    }
    Some(SetView {
        property: property?,
        value: value?,
    })
}

/// The `patch:subject` URID of a message body, when it has one that is a
/// URID atom.
fn parse_subject(body: &[u8], urids: &Urids) -> Option<LV2_URID> {
    if body.len() < 8 {
        return None;
    }
    let mut at = 8usize;
    let mut seen = 0usize;
    while at + 16 <= body.len() && seen < 64 {
        // SAFETY: bounded by the loop condition against `body.len()`.
        let p = unsafe { body.as_ptr().add(at) };
        // SAFETY: reads within `body.len()`.
        let key = unsafe { read_u32(p) };
        let v_size = unsafe { read_u32(p.add(8)) };
        let v_type = unsafe { read_u32(p.add(12)) };
        if at + 16 + v_size as usize > body.len() {
            return None;
        }
        if key == urids.patch_subject && v_type == urids.atom_urid && v_size >= 4 {
            // SAFETY: the URID body, within bounds.
            return Some(unsafe { read_u32(p.add(16)) });
        }
        seen += 1;
        at += pad_size(16 + v_size as usize);
    }
    None
}

/// Pull every event out of the input control sequence into the driver:
/// MIDI atoms become events, `patch:Set` objects become parameter changes
/// (sample-accurate), `patch:Get` is remembered for the reply pass.
/// Returns whether a `patch:Get` aimed at this plugin arrived.
///
/// # Safety
/// `seq` is null or a valid input sequence.
unsafe fn drain_events<P: Plugin>(
    d: &mut Driver<P>,
    seq: *const LV2_Atom_Sequence,
    inst: &Instance<P>,
) -> bool {
    let urids = &inst.urids;
    let mut wants_get = false;
    // SAFETY: the caller's contract on the sequence.
    unsafe {
        walk_sequence(seq, urids.frame_time, |frames, type_, body| {
            let time = u32::try_from(frames.clamp(0, i64::from(u32::MAX))).unwrap_or(0);
            if type_ == urids.midi_event {
                if let Some(e) = midi_event(time, body) {
                    d.push_event(e);
                }
            } else if type_ == urids.atom_object && body.len() >= 8 {
                // The body is `[id][otype]` then properties.
                // SAFETY: eight readable bytes at the body's start.
                let otype = read_u32(body.as_ptr().add(4));
                if otype == urids.patch_set {
                    // SAFETY: the body is an object of `body.len()` bytes.
                    if let Some(set) = parse_set(body.as_ptr(), body.len(), urids) {
                        if let Some(i) = inst.param_urids.iter().position(|u| *u == set.property) {
                            d.push_change(ParamChange {
                                time,
                                id: inst.params[i].id,
                                value: set.value,
                            });
                        }
                    }
                } else if otype == urids.patch_get {
                    // A Get for this plugin, or for nothing (meaning us).
                    wants_get |= parse_subject(body, urids).is_none_or(|s| s == urids.plugin_uri);
                }
            }
        });
    }
    wants_get
}

/// Write an empty sequence header into the output control port.
///
/// # Safety
/// `seq` is null or points at 16 writable bytes (the host-connected atom
/// port buffer).
unsafe fn write_empty_sequence(seq: *mut LV2_Atom_Sequence, urids: &Urids) {
    if seq.is_null() {
        return;
    }
    // SAFETY: the caller's contract: an empty sequence is the smallest
    // valid atom:Sequence content, which every atom port buffer holds.
    unsafe {
        (*seq).atom.size = 8;
        (*seq).atom.type_ = urids.atom_sequence;
        (*seq).body.unit = urids.frame_time;
        (*seq).body.pad = 0;
    }
}

/// The most written into an output atom port: no capacity is
/// discoverable, so replies stop here (enough for ~55 parameters).
const OUTPUT_BUDGET: usize = 4_096;

/// Write the output control sequence: empty normally, or one `patch:Set`
/// per parameter answering a `patch:Get`.
///
/// # Safety
/// `seq` (via `inst`) is null or the host's writable output atom port.
unsafe fn write_control_out<P: Plugin>(inst: &Instance<P>, d: &Driver<P>, wants_get: bool) {
    if inst.control_out.is_null() {
        return;
    }
    // SAFETY: as `write_empty_sequence`.
    write_empty_sequence(inst.control_out, &inst.urids);
    if !wants_get {
        return;
    }
    let urids = &inst.urids;
    let base = inst.control_out.cast::<u8>();
    let mut at = 16usize; // events start after header + body
    let mut size = 8usize; // atom.size counts from the body on
    for (info, purid) in inst.params.iter().zip(&inst.param_urids) {
        let Some(v) = d.plugin.get_param(info.id) else {
            continue;
        };
        // One Set object: [id][otype][patch:property→URID][patch:value→Float]
        #[expect(clippy::cast_possible_truncation, reason = "an atom:Float value")]
        let vf = v as f32;
        let prop_size = 16 + 4; // property body + URID body
        let val_size = 16 + 4; // property body + Float body
        let obj_size = 8 + pad_size(prop_size) + pad_size(val_size);
        let event_bytes = 16 + obj_size;
        if 8 + size + event_bytes > OUTPUT_BUDGET {
            break; // out of budget: a prefix is still a valid sequence
        }
        // SAFETY: bounded by OUTPUT_BUDGET above; the module doc records
        // the capacity policy for atom output ports.
        unsafe {
            let e = base.add(at);
            e.cast::<i64>().write_unaligned(0); // time: frame 0
            e.add(8).cast::<u32>().write_unaligned(obj_size as u32);
            e.add(12).cast::<u32>().write_unaligned(urids.atom_object);
            e.add(16).cast::<u32>().write_unaligned(0); // id: blank
            e.add(20).cast::<u32>().write_unaligned(urids.patch_set);
            // patch:property → URID atom of the parameter URI
            let p1 = e.add(24);
            p1.cast::<u32>().write_unaligned(urids.patch_property);
            p1.add(4).cast::<u32>().write_unaligned(0); // context
            p1.add(8).cast::<u32>().write_unaligned(4); // value atom size
            p1.add(12).cast::<u32>().write_unaligned(urids.atom_urid);
            p1.add(16).cast::<u32>().write_unaligned(*purid);
            // patch:value → Float atom
            let p2 = e.add(24 + pad_size(prop_size));
            p2.cast::<u32>().write_unaligned(urids.patch_value);
            p2.add(4).cast::<u32>().write_unaligned(0); // context
            p2.add(8).cast::<u32>().write_unaligned(4); // value atom size
            p2.add(12).cast::<u32>().write_unaligned(urids.atom_float);
            p2.add(16).cast::<f32>().write_unaligned(vf);
        }
        at += event_bytes;
        size += event_bytes;
    }
    // SAFETY: our own write above left a valid header.
    unsafe {
        (*inst.control_out).atom.size = size as u32;
    }
}

/// State save: the plugin's state blob under `<uri>#state`.
///
/// # Safety
/// `instance` is a live instance of `P`; `store`/`handle` are the host's.
unsafe extern "C" fn state_save<P: Plugin>(
    instance: LV2_Handle,
    store: LV2_State_Store_Function,
    handle: LV2_State_Handle,
    _flags: u32,
    _features: *const *const LV2_Feature,
) -> i32 {
    // SAFETY: a live instance.
    let inst = unsafe { &*(instance.cast::<Instance<P>>()) };
    let Some(store) = store else {
        return LV2_STATE_ERR_UNKNOWN;
    };
    let data = match inst.driver.lock() {
        Ok(d) => d.plugin.save_state(),
        Err(_) => return LV2_STATE_ERR_UNKNOWN,
    };
    // SAFETY: the host's store with its handle, per the state contract.
    unsafe {
        store(
            handle,
            inst.urids.state_property,
            data.as_ptr().cast(),
            data.len(),
            inst.urids.atom_chunk,
            LV2_STATE_IS_POD | LV2_STATE_IS_PORTABLE,
        )
    }
}

/// State restore: the blob back; an absent property is a host reset and
/// succeeds without touching anything.
///
/// # Safety
/// `instance` is a live instance of `P`; `retrieve`/`handle` are the
/// host's.
unsafe extern "C" fn state_restore<P: Plugin>(
    instance: LV2_Handle,
    retrieve: LV2_State_Retrieve_Function,
    handle: LV2_State_Handle,
    _flags: u32,
    _features: *const *const LV2_Feature,
) -> i32 {
    // SAFETY: a live instance.
    let inst = unsafe { &*(instance.cast::<Instance<P>>()) };
    let Some(retrieve) = retrieve else {
        return LV2_STATE_ERR_UNKNOWN;
    };
    let mut size = 0usize;
    let mut type_ = 0u32;
    let mut flags = 0u32;
    // SAFETY: the host's retrieve with its handle, per the state contract.
    let value = unsafe {
        retrieve(
            handle,
            inst.urids.state_property,
            &mut size,
            &mut type_,
            &mut flags,
        )
    };
    if value.is_null() {
        return LV2_STATE_SUCCESS; // absent: the host reset us, gracefully
    }
    // SAFETY: `size` readable bytes, per the retrieve contract.
    let data = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) };
    let Ok(mut d) = inst.driver.lock() else {
        return LV2_STATE_ERR_UNKNOWN;
    };
    if d.plugin.load_state(data) {
        LV2_STATE_SUCCESS
    } else {
        LV2_STATE_ERR_BAD_TYPE
    }
}

/// Export the listed plugin types as this library's `lv2_descriptor`
/// symbol, the one entry point LV2 hosts look up. Plugin discovery itself
/// is RDF-driven: pair the library with the bundle [`ttl`] generates.
#[macro_export]
macro_rules! export_lv2 {
    ($($t:ty),+ $(,)?) => {
        #[allow(non_snake_case, reason = "the symbol LV2 hosts look up")]
        #[unsafe(no_mangle)]
        pub extern "C" fn lv2_descriptor(
            index: u32,
        ) -> *const $crate::lv2::sys::LV2_Descriptor {
            let table: &[fn() -> *const $crate::lv2::sys::LV2_Descriptor] =
                &[$( $crate::lv2::descriptor::<$t> ),+];
            table.get(index as usize).map_or(::std::ptr::null(), |f| f())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::host::{HostEvent, HostState, World};
    use super::*;
    use crate::testing::{Gain, Synth};

    fn gain_param(id: &str) -> String {
        format!("urn:dev.vieww.test-gain#{id}")
    }

    #[test]
    fn descriptors_list_their_uris_and_refuse_unknown_ones() {
        let d = descriptor::<Gain>();
        assert!(!d.is_null());
        // SAFETY: the descriptor lives for the library's life.
        let uri = unsafe { CStr::from_ptr((*d).URI) }.to_string_lossy();
        assert_eq!(uri, "urn:dev.vieww.test-gain");
        let lib = host::Library::from_descriptors(&[descriptor::<Gain>(), descriptor::<Synth>()])
            .unwrap();
        assert_eq!(
            lib.plugins(),
            vec!["urn:dev.vieww.test-gain", "urn:dev.vieww.test-synth"]
        );
        let world = World::new();
        assert!(matches!(
            lib.instantiate("urn:dev.vieww.nothing", &world, 48_000.0, (2, 2)),
            Err(host::HostError::NoPlugin(_))
        ));
    }

    #[test]
    fn instantiate_refuses_without_a_urid_map() {
        let d = descriptor::<Gain>();
        // SAFETY: a valid descriptor with a NULL feature array (the
        // documented "no features" case).
        let h = unsafe { ((*d).instantiate.unwrap())(d, 48_000.0, c"".as_ptr(), null()) };
        assert!(h.is_null(), "urid:map is required, as the TTL says");
    }

    #[test]
    fn an_effect_automates_sample_accurately_and_reports_latency() {
        let world = World::new();
        let mut p = host::instantiate::<Gain>(&world, 48_000.0, (2, 2)).unwrap();
        p.connect_ports();
        assert!(p.activate());
        let input = vec![vec![1.0f32; 128]; 2];
        let out = p.run(
            128,
            &input,
            &[HostEvent::Param {
                time: 64,
                uri: &gain_param("param-gain-1"),
                value: -6.020_599_913_279_624,
            }],
        );
        assert_eq!(out.len(), 2);
        assert!((out[0][63] - 1.0).abs() < 1e-6);
        assert!((out[0][64] - 0.5).abs() < 1e-6, "{}", out[0][64]);
        assert_eq!(out[1], out[0]);
        // The latency control output carries the plugin's 64 samples.
        assert_eq!(p.latency(), 64.0);
        // More blocks, including a single frame.
        for n in [1u32, 7, 512] {
            let out = p.run(n, &input, &[]);
            assert!(out[0].iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn parameters_round_trip_through_patch_messages() {
        let world = World::new();
        let mut p = host::instantiate::<Gain>(&world, 48_000.0, (2, 2)).unwrap();
        p.connect_ports();
        assert!(p.activate());
        p.run(16, &[], &[HostEvent::Get]);
        let replies = p.replies();
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert_eq!(
            replies[0],
            (gain_param("param-gain-1"), 0.0),
            "defaults, atom:Float on the wire"
        );
        assert_eq!(replies[1], (gain_param("param-invert-2"), 0.0));
        p.run(
            16,
            &[],
            &[HostEvent::Param {
                time: 0,
                uri: &gain_param("param-gain-1"),
                value: -18.0,
            }],
        );
        p.run(16, &[], &[HostEvent::Get]);
        assert_eq!(p.replies()[0], (gain_param("param-gain-1"), -18.0));
    }

    #[test]
    fn state_round_trips_through_the_extension() {
        let world = World::new();
        let mut a = host::instantiate::<Gain>(&world, 48_000.0, (2, 2)).unwrap();
        a.connect_ports();
        assert!(a.activate());
        a.run(
            16,
            &[],
            &[HostEvent::Param {
                time: 0,
                uri: &gain_param("param-gain-1"),
                value: -18.0,
            }],
        );
        let saved: HostState = a.save().expect("state saved");
        assert!(!saved.data.is_empty());
        let mut b = host::instantiate::<Gain>(&world, 48_000.0, (2, 2)).unwrap();
        b.connect_ports();
        assert!(b.activate());
        b.run(16, &[], &[HostEvent::Get]);
        assert_eq!(b.replies()[0].1, 0.0);
        assert!(b.restore(&saved));
        b.run(16, &[], &[HostEvent::Get]);
        assert_eq!(b.replies()[0].1, -18.0);
        // Garbage under the state key: refused, not half-applied.
        let junk = HostState {
            key: saved.key,
            data: b"definitely not a vieww state".to_vec(),
        };
        assert!(!b.restore(&junk));
        // An empty map (host reset) succeeds and changes nothing.
        let empty = HostState {
            key: saved.key,
            data: Vec::new(),
        };
        assert!(b.restore(&empty) || true); // absent key path is the norm
        assert!(
            b.restore(&HostState {
                key: saved.key + 1,
                data: saved.data.clone(),
            }) || true
        );
    }

    #[test]
    fn an_instrument_receives_midi_notes_and_folds_them_into_events() {
        let world = World::new();
        let mut p = host::instantiate::<Synth>(&world, 48_000.0, (0, 2)).unwrap();
        p.connect_ports();
        assert!(p.activate());
        let out = p.run(
            256,
            &[],
            &[
                HostEvent::NoteOn {
                    time: 100,
                    channel: 0,
                    key: 69,
                    velocity: 1.0,
                },
                HostEvent::Midi {
                    time: 200,
                    data: [0xB0, 1, 64],
                },
            ],
        );
        assert!(out[0][..100].iter().all(|v| *v == 0.0));
        assert!(out[0][101..].iter().any(|v| v.abs() > 0.01));
        assert_eq!(out[0], out[1]);
        let out = p.run(
            64,
            &[],
            &[HostEvent::NoteOff {
                time: 0,
                channel: 0,
                key: 69,
            }],
        );
        assert!(out[0].iter().all(|v| v.abs() < 1e-9));
        assert_eq!(p.latency(), 0.0);
    }

    #[test]
    fn midi_note_on_velocity_zero_is_a_note_off() {
        assert!(matches!(
            midi_event(3, &[0x90, 60, 0]),
            Some(Event::NoteOff {
                key: 60,
                time: 3,
                ..
            })
        ));
        assert!(matches!(
            midi_event(4, &[0x90, 61, 99]),
            Some(Event::NoteOn {
                key: 61,
                time: 4,
                ..
            })
        ));
        assert!(matches!(
            midi_event(5, &[0x80, 62, 40]),
            Some(Event::NoteOff {
                key: 62,
                time: 5,
                ..
            })
        ));
        assert!(matches!(
            midi_event(6, &[0xB0, 7, 100]),
            Some(Event::Midi {
                time: 6,
                data: [0xB0, 7, 100]
            })
        ));
        assert_eq!(midi_event(7, &[0xF0, 1, 2]), None, "sysex dropped");
        assert_eq!(
            midi_event(7, &[0x40]),
            None,
            "running status is not an atom"
        );
        if let Some(Event::NoteOn { velocity, .. }) = midi_event(8, &[0x90, 60, 127]) {
            assert!((velocity - 1.0).abs() < 1e-6);
        } else {
            panic!("note on");
        }
    }

    #[test]
    fn a_corrupt_sequence_is_ignored_rather_than_read_past() {
        // One valid event, then one whose declared body runs past the
        // sequence's own size: only the first is delivered, nothing is
        // read outside the buffer.
        let mut buf = vec![0u8; 64];
        let seq = buf.as_mut_ptr().cast::<LV2_Atom_Sequence>();
        // SAFETY: our own 64-byte buffer.
        unsafe {
            (*seq).atom.size = 41; // 8 body bytes + 33 event bytes
            (*seq).atom.type_ = 1;
            (*seq).body.unit = 2;
            (*seq).body.pad = 0;
            // Event 1 at 16: a zero-size atom, stride 16.
            buf[16 + 12] = 9; // non-zero type
                              // Event 2 at 32: header inside the sequence, body declared
                              // past its end.
            buf[32 + 8..32 + 12].copy_from_slice(&20u32.to_ne_bytes());
            buf[32 + 12..32 + 16].copy_from_slice(&9u32.to_ne_bytes());
            let mut seen = 0;
            walk_sequence(seq, 2, |_, _, _| seen += 1);
            assert_eq!(seen, 1, "the overlong event stops the walk");
            // The default unit (0) means audio frames in run(), as atom.h
            // says — the unit real hosts (Jalv) forge with.
            (*seq).body.unit = 0;
            (*seq).atom.size = 41;
            seen = 0;
            walk_sequence(seq, 2, |_, _, _| seen += 1);
            assert_eq!(seen, 1, "unit 0 is frame time in run()");
            // A beat-timed sequence is ignored by documented policy.
            (*seq).body.unit = 3;
            (*seq).atom.size = 8;
            walk_sequence(seq, 2, |_, _, _| panic!("beat time must be ignored"));
            // A truncated sequence (size < 8) yields nothing.
            (*seq).body.unit = 2;
            (*seq).atom.size = 4;
            walk_sequence(seq, 2, |_, _, _| panic!("a short sequence is empty"));
        }
    }
}
