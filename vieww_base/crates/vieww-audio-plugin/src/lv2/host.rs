//! An LV2 host: a URID world, a sequence forge, and a loader that drives
//! plugins through the `lv2_descriptor` entry point.
//!
//! [`World`] is the `urid:map` feature the host side of LV2 always
//! provides (plus `urid:unmap`, which the wrapper does not need).
//! [`Library::open`] `dlopen`s a plugin library and collects its
//! `lv2_descriptor` table; [`Library::from_descriptors`] wraps a table
//! already in this process (tests, embedding). An [`Instance`] owns the
//! port buffers, builds the control sequence from [`HostEvent`]s in the
//! same wire format the wrapper reads, runs blocks, parses `patch:Set`
//! replies, and round-trips state through the `state:interface`.
//!
//! LV2 plugins describe their ports in RDF, not in the library — so this
//! host takes the audio shape as given (it drives vieww-exported plugins,
//! whose layout the wrapper documents and [`ttl`](super::ttl) writes).
//! The TTL is the format's discovery mechanism; this in-house host is the
//! test gate, as the CLAP and VST3 hosts are for those formats.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use super::sys::*;
use super::{parse_set, read_u32, Urids};
use crate::Plugin;

/// Why a host operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The library could not be loaded.
    Load(String),
    /// It exports no `lv2_descriptor`.
    NoDescriptor,
    /// No descriptor with that URI.
    NoPlugin(String),
    /// Instantiation failed (missing `urid:map`, a broken map, or OOM).
    Instantiate(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(e) => write!(f, "could not load the plugin library: {e}"),
            Self::NoDescriptor => f.write_str("the library exports no lv2_descriptor"),
            Self::NoPlugin(u) => write!(f, "no plugin with URI {u}"),
            Self::Instantiate(u) => write!(f, "could not instantiate {u}"),
        }
    }
}

impl std::error::Error for HostError {}

/// The URID world: one interned string table standing behind the
/// `urid:map` feature. URIDs start at 1; 0 is the reserved failure value.
#[derive(Debug, Default)]
pub struct World {
    ids: Mutex<Vec<CString>>,
}

// SAFETY: the table is behind a mutex; the map callback may be called from
// any context LV2 allows (the wrapper maps at instantiate only).
unsafe impl Send for World {}
// SAFETY: as above.
unsafe impl Sync for World {}

impl World {
    /// A fresh empty world.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The URID of a URI, interning it if needed.
    pub fn urid(&self, uri: &str) -> LV2_URID {
        let Ok(mut ids) = self.ids.lock() else {
            return 0;
        };
        if let Some(i) = ids.iter().position(|s| s.to_bytes() == uri.as_bytes()) {
            return u32::try_from(i + 1).unwrap_or(0);
        }
        match CString::new(uri) {
            Ok(c) => {
                ids.push(c);
                u32::try_from(ids.len()).unwrap_or(0)
            }
            Err(_) => 0,
        }
    }

    /// The URI an URID maps to, if it is in this world.
    #[must_use]
    pub fn uri(&self, urid: LV2_URID) -> Option<String> {
        let ids = self.ids.lock().ok()?;
        ids.get(urid as usize - 1)
            .map(|s| s.to_string_lossy().into_owned())
    }

    /// The `LV2_URID_Map` view of this world (the plugin copies it).
    fn map_view(&self) -> LV2_URID_Map {
        LV2_URID_Map {
            handle: std::ptr::from_ref(self).cast::<c_void>().cast_mut(),
            map: Some(world_map),
        }
    }

    /// The URIDs the wrapper will map, resolved through this world — the
    /// same numbers the plugin sees, because it maps through this world
    /// too.
    fn urids(&self, plugin_uri: &str) -> Option<Urids> {
        let map = self.map_view();
        let map_feature = LV2_Feature {
            URI: LV2_URID__map.as_ptr().cast(),
            data: std::ptr::from_ref(&map).cast::<c_void>().cast_mut(),
        };
        let features = [&raw const map_feature, null()];
        // SAFETY: a NULL-terminated one-feature array whose map data is
        // this world, which outlives the call.
        unsafe { Urids::from_features(features.as_ptr(), plugin_uri) }
    }
}

/// The world's `urid:map` callback.
///
/// # Safety
/// `handle` is the `World` the feature came from; `uri` is a
/// NUL-terminated string.
unsafe extern "C" fn world_map(handle: LV2_URID_Handle, uri: *const c_char) -> LV2_URID {
    if handle.is_null() || uri.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract.
    let world = unsafe { &*(handle.cast::<World>()) };
    // SAFETY: as above.
    let uri = unsafe { CStr::from_ptr(uri) };
    world.urid(&uri.to_string_lossy())
}

/// A loaded plugin library.
#[derive(Debug)]
pub struct Library {
    handle: *mut c_void,
    descriptors: Vec<*const LV2_Descriptor>,
    from_disk: bool,
}

// SAFETY: the handle and descriptors are used from one thread at a time
// (instantiate and cleanup are instantiation-thread calls).
unsafe impl Send for Library {}

impl Library {
    /// Load the library at `path` and collect its `lv2_descriptor` table.
    ///
    /// # Safety
    /// Loading a library runs its initialisers: it must be trusted code.
    pub unsafe fn open(path: impl AsRef<Path>) -> Result<Self, HostError> {
        let path = path.as_ref();
        let p = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|e| HostError::Load(e.to_string()))?;
        // SAFETY: the caller trusts the library.
        let handle = unsafe { load(&p) };
        if handle.is_null() {
            return Err(HostError::Load(last_error()));
        }
        // SAFETY: a handle from `load`.
        let sym = unsafe { symbol(handle, c"lv2_descriptor".as_ptr()) };
        if sym.is_null() {
            // SAFETY: as above.
            unsafe { unload(handle) };
            return Err(HostError::NoDescriptor);
        }
        type DescFn = unsafe extern "C" fn(index: u32) -> *const LV2_Descriptor;
        // SAFETY: `lv2_descriptor` has this prototype by the LV2 contract.
        let f: DescFn = unsafe { std::mem::transmute::<*mut c_void, DescFn>(sym) };
        let mut descriptors = Vec::new();
        for i in 0.. {
            // SAFETY: the library's own accessor.
            let d = unsafe { f(i) };
            if d.is_null() {
                break;
            }
            descriptors.push(d);
            if descriptors.len() > 256 {
                break; // a table that never ends: hostile, stop reading
            }
        }
        if descriptors.is_empty() {
            // SAFETY: a handle from `load`.
            unsafe { unload(handle) };
            return Err(HostError::NoDescriptor);
        }
        Ok(Self {
            handle,
            descriptors,
            from_disk: true,
        })
    }

    /// Wrap a table of descriptors already in this process.
    ///
    /// # Errors
    /// An empty table.
    pub fn from_descriptors(table: &[*const LV2_Descriptor]) -> Result<Self, HostError> {
        if table.is_empty() {
            return Err(HostError::NoDescriptor);
        }
        Ok(Self {
            handle: null_mut(),
            descriptors: table.to_vec(),
            from_disk: false,
        })
    }

    /// The plugins on offer: their URIs (ports and parameters are the
    /// RDF's to describe, not the library's).
    #[must_use]
    pub fn plugins(&self) -> Vec<String> {
        self.descriptors
            .iter()
            .filter_map(|d| {
                // SAFETY: descriptors live for the library's lifetime.
                let uri = unsafe { (**d).URI };
                (!uri.is_null()).then(|| {
                    // SAFETY: a NUL-terminated descriptor URI.
                    unsafe { CStr::from_ptr(uri) }
                        .to_string_lossy()
                        .into_owned()
                })
            })
            .collect()
    }

    /// Instantiate the plugin with `uri` against `world`, with `shape`
    /// audio input/output channels (the wrapper's documented layout:
    /// ports 0/1 control, 2 latency, then audio).
    ///
    /// # Errors
    /// Unknown URI, or the plugin refused instantiation.
    pub fn instantiate<'w>(
        &'w self,
        uri: &str,
        world: &'w World,
        rate: f64,
        shape: (usize, usize),
    ) -> Result<Instance<'w>, HostError> {
        let desc = self
            .descriptors
            .iter()
            .copied()
            .find(|d| {
                // SAFETY: descriptors live for the library's lifetime.
                let u = unsafe { (**d).URI };
                !u.is_null()
                    // SAFETY: a NUL-terminated descriptor URI.
                    && unsafe { CStr::from_ptr(u) }.to_bytes() == uri.as_bytes()
            })
            .ok_or_else(|| HostError::NoPlugin(uri.into()))?;
        let urids = world
            .urids(uri)
            .ok_or_else(|| HostError::Instantiate(uri.into()))?;
        let instantiate = unsafe {
            (*desc)
                .instantiate
                .ok_or_else(|| HostError::Instantiate(uri.into()))?
        };
        let map = world.map_view();
        let map_feature = LV2_Feature {
            URI: LV2_URID__map.as_ptr().cast(),
            data: std::ptr::from_ref(&map).cast::<c_void>().cast_mut(),
        };
        let features = [&raw const map_feature, null()];
        let bundle = CString::new("").unwrap_or_default();
        // SAFETY: the descriptor's own instantiate with a valid feature
        // array; the world outlives the instance (LV2 contract).
        let handle = unsafe { instantiate(desc, rate, bundle.as_ptr(), features.as_ptr()) };
        if handle.is_null() {
            return Err(HostError::Instantiate(uri.into()));
        }
        let (n_in, n_out) = shape;
        let mut inst = Instance {
            _lib: self,
            world,
            handle,
            desc,
            urids,
            in_buf: vec![0; Instance::BUF],
            out_buf: vec![0; Instance::BUF],
            audio_in: vec![Vec::with_capacity(4_096); n_in],
            audio_out: vec![Vec::with_capacity(4_096); n_out],
            latency: 0.0,
            active: false,
        };
        for b in &mut inst.audio_in {
            b.resize(4_096, 0.0);
        }
        for b in &mut inst.audio_out {
            b.resize(4_096, 0.0);
        }
        Ok(inst)
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        if self.from_disk && !self.handle.is_null() {
            // SAFETY: a handle from `load`, dropped once.
            unsafe { unload(self.handle) };
        }
    }
}

#[cfg(unix)]
mod dl {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        pub(super) fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        pub(super) fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        pub(super) fn dlclose(handle: *mut c_void) -> c_int;
        pub(super) fn dlerror() -> *const c_char;
    }
    pub(super) const RTLD_NOW: c_int = 2;
}

#[cfg(windows)]
mod dl {
    use std::ffi::{c_char, c_void};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(super) fn LoadLibraryW(name: *const u16) -> *mut c_void;
        pub(super) fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        pub(super) fn FreeLibrary(module: *mut c_void) -> i32;
    }
}

#[cfg(unix)]
unsafe fn load(p: &CString) -> *mut c_void {
    // SAFETY: plain libc with a valid C string.
    unsafe { dl::dlopen(p.as_ptr(), dl::RTLD_NOW) }
}

#[cfg(unix)]
unsafe fn last_error() -> String {
    // SAFETY: returns the last dl error as a C string or NULL.
    unsafe {
        let e = dl::dlerror();
        if e.is_null() {
            "unknown".into()
        } else {
            CStr::from_ptr(e).to_string_lossy().into_owned()
        }
    }
}

#[cfg(unix)]
unsafe fn symbol(h: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: a handle from `load` and a valid symbol name.
    unsafe { dl::dlsym(h, name) }
}

#[cfg(unix)]
unsafe fn unload(h: *mut c_void) {
    // SAFETY: a handle from `load`.
    unsafe {
        dl::dlclose(h);
    }
}

#[cfg(windows)]
unsafe fn load(p: &CString) -> *mut c_void {
    use std::os::windows::ffi::OsStrExt;
    let path = std::path::Path::new(p.to_string_lossy().as_ref());
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: kernel32 with a valid wide path.
    unsafe { dl::LoadLibraryW(wide.as_ptr()) }
}

#[cfg(windows)]
unsafe fn last_error() -> String {
    // SAFETY: the last Windows error.
    unsafe { std::io::Error::last_os_error().to_string() }
}

#[cfg(windows)]
unsafe fn symbol(h: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: a handle from `load` and a valid symbol name.
    unsafe { dl::GetProcAddress(h, name) }
}

#[cfg(windows)]
unsafe fn unload(h: *mut c_void) {
    // SAFETY: a handle from `load`.
    unsafe {
        dl::FreeLibrary(h);
    }
}

#[cfg(not(any(unix, windows)))]
unsafe fn load(_: &CString) -> *mut c_void {
    null_mut()
}

#[cfg(not(any(unix, windows)))]
fn last_error() -> String {
    "no dynamic loader on this platform".into()
}

#[cfg(not(any(unix, windows)))]
unsafe fn symbol(_: *mut c_void, _: *const c_char) -> *mut c_void {
    null_mut()
}

#[cfg(not(any(unix, windows)))]
unsafe fn unload(_: *mut c_void) {}

/// An event the host sends on the control sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent<'a> {
    /// A raw MIDI message (status first).
    Midi {
        /// Frame.
        time: u32,
        /// Status and data bytes.
        data: [u8; 3],
    },
    /// A note on (built into MIDI by the forge).
    NoteOn {
        /// Frame.
        time: u32,
        /// Channel 0..15.
        channel: u8,
        /// Key 0..127.
        key: u8,
        /// Velocity 0..1.
        velocity: f32,
    },
    /// A note off (built into MIDI by the forge).
    NoteOff {
        /// Frame.
        time: u32,
        /// Channel.
        channel: u8,
        /// Key.
        key: u8,
    },
    /// A parameter change, as a `patch:Set` for the parameter's URI.
    Param {
        /// Frame.
        time: u32,
        /// The parameter's URI (as [`ttl`](super::ttl) writes it).
        uri: &'a str,
        /// The plain value.
        value: f64,
    },
    /// A `patch:Get` for the plugin itself.
    Get,
}

/// A plugin's state, as the host collected it from the state interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostState {
    /// The property key the plugin stored under.
    pub key: LV2_URID,
    /// The stored bytes.
    pub data: Vec<u8>,
}

/// A hosted plugin instance.
pub struct Instance<'a> {
    _lib: &'a Library,
    world: &'a World,
    handle: LV2_Handle,
    desc: *const LV2_Descriptor,
    urids: Urids,
    in_buf: Vec<u8>,
    out_buf: Vec<u8>,
    audio_in: Vec<Vec<f32>>,
    audio_out: Vec<Vec<f32>>,
    latency: f32,
    active: bool,
}

impl std::fmt::Debug for Instance<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Instance")
            .field("handle", &self.handle)
            .finish()
    }
}

impl Instance<'_> {
    /// Both atom port buffers' size (the host's own capacity policy).
    const BUF: usize = 8_192;

    /// Connect every port to this instance's own buffers. Call once, after
    /// the instance has reached its final resting place: the latency
    /// control port points into the struct itself, so a move after
    /// connecting would leave the plugin writing to the old address (the
    /// atom and audio buffers are heap allocations and never move).
    pub fn connect_ports(&mut self) {
        // SAFETY: a live instance and our own buffers (LV2 contract).
        unsafe {
            let cp = (*self.desc).connect_port.unwrap();
            cp(
                self.handle,
                super::PORT_CONTROL_IN,
                self.in_buf.as_mut_ptr().cast(),
            );
            cp(
                self.handle,
                super::PORT_CONTROL_OUT,
                self.out_buf.as_mut_ptr().cast(),
            );
            cp(
                self.handle,
                super::PORT_LATENCY,
                std::ptr::from_mut(&mut self.latency).cast(),
            );
            let mut port = super::PORT_LATENCY + 1;
            for b in &mut self.audio_in {
                cp(self.handle, port, b.as_mut_ptr().cast());
                port += 1;
            }
            for b in &mut self.audio_out {
                cp(self.handle, port, b.as_mut_ptr().cast());
                port += 1;
            }
        }
    }

    /// Activate the plugin.
    pub fn activate(&mut self) -> bool {
        if self.active {
            return true;
        }
        // LV2's activate returns nothing: calling it is activating.
        // SAFETY: a live instance.
        self.active = unsafe {
            if let Some(a) = (*self.desc).activate {
                a(self.handle);
            }
            true
        };
        self.active
    }

    /// Run one block: copy `inputs` into the audio input buffers, build
    /// the control sequence from `events`, run, and return the outputs.
    pub fn run(
        &mut self,
        frames: u32,
        inputs: &[Vec<f32>],
        events: &[HostEvent<'_>],
    ) -> Vec<Vec<f32>> {
        let n = frames as usize;
        let mut reconnect = false;
        for b in &mut self.audio_in {
            if b.len() < n {
                b.resize(n, 0.0); // may move: reconnect below
                reconnect = true;
            }
        }
        for b in &mut self.audio_out {
            if b.len() < n {
                b.resize(n, 0.0);
                reconnect = true;
            }
        }
        if reconnect {
            self.connect_ports();
        }
        for (ch, src) in self.audio_in.iter_mut().zip(inputs) {
            let k = n.min(src.len());
            ch[..k].copy_from_slice(&src[..k]);
            ch[k..].fill(0.0);
        }
        for ch in &mut self.audio_out {
            ch[..n].fill(0.0);
        }
        self.build_sequence(events);
        // SAFETY: a live, activated instance with connected buffers.
        unsafe {
            if let Some(r) = (*self.desc).run {
                r(self.handle, frames);
            }
        }
        self.audio_out.iter().map(|c| c[..n].to_vec()).collect()
    }

    /// The latency the plugin last reported on its latency port.
    #[must_use]
    pub fn latency(&self) -> f32 {
        self.latency
    }

    /// Build the input control sequence.
    fn build_sequence(&mut self, events: &[HostEvent<'_>]) {
        let urids = self.urids;
        let world = self.world;
        let mut forge = Forge {
            urids,
            buf: &mut self.in_buf,
        };
        forge.header();
        for e in events {
            match *e {
                HostEvent::Midi { time, data } => forge.midi(i64::from(time), &data),
                HostEvent::NoteOn {
                    time,
                    channel,
                    key,
                    velocity,
                } => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a MIDI velocity byte"
                    )]
                    let vel = (velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
                    forge.midi(i64::from(time), &[0x90 | (channel & 0x0F), key & 0x7F, vel]);
                }
                HostEvent::NoteOff { time, channel, key } => {
                    forge.midi(i64::from(time), &[0x80 | (channel & 0x0F), key & 0x7F, 64]);
                }
                HostEvent::Param { time, uri, value } => {
                    let u = world.urid(uri);
                    if u != 0 {
                        forge.set(i64::from(time), u, value);
                    }
                }
                HostEvent::Get => forge.get(),
            }
        }
    }

    /// The `patch:Set` replies in the output sequence: parameter URI and
    /// value pairs (the answer to a `patch:Get`).
    #[must_use]
    pub fn replies(&self) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        let urids = &self.urids;
        // SAFETY: our own 8 KiB buffer, header written by the plugin.
        let seq = self.out_buf.as_ptr().cast::<LV2_Atom_Sequence>();
        unsafe {
            super::walk_sequence(seq, urids.frame_time, |_frames, type_, body| {
                if type_ != urids.atom_object || body.len() < 8 {
                    return;
                }
                // SAFETY: the object body's first eight bytes.
                let otype = read_u32(body.as_ptr().add(4));
                if otype != urids.patch_set {
                    return;
                }
                // SAFETY: the body is an object of `body.len()` bytes.
                if let Some(set) = parse_set(body.as_ptr(), body.len(), urids) {
                    if let Some(uri) = self.world.uri(set.property) {
                        out.push((uri, set.value));
                    }
                }
            });
        }
        out
    }

    /// Save the plugin's state (whatever it stored, under whichever key).
    #[must_use]
    pub fn save(&self) -> Option<HostState> {
        // SAFETY: a live instance; the state interface for this plugin.
        let iface = unsafe {
            let e = (*self.desc).extension_data?;
            e(LV2_STATE__interface.as_ptr().cast())
        };
        if iface.is_null() {
            return None;
        }
        // SAFETY: the descriptor's state interface.
        let iface = unsafe { &*(iface.cast::<LV2_State_Interface>()) };
        let save = iface.save?;
        let mut sink = StateSink::default();
        // SAFETY: our own sink behind the store callback.
        unsafe {
            save(
                self.handle,
                Some(sink_store),
                std::ptr::from_mut(&mut sink).cast(),
                0,
                null(),
            )
        };
        sink.data.map(|data| HostState {
            key: sink.key,
            data,
        })
    }

    /// Restore a state saved by [`save`](Self::save).
    pub fn restore(&mut self, state: &HostState) -> bool {
        // SAFETY: a live instance; the state interface for this plugin.
        let iface = unsafe {
            let Some(e) = (*self.desc).extension_data else {
                return false;
            };
            e(LV2_STATE__interface.as_ptr().cast())
        };
        if iface.is_null() {
            return false;
        }
        // SAFETY: the descriptor's state interface.
        let iface = unsafe { &*(iface.cast::<LV2_State_Interface>()) };
        let Some(restore) = iface.restore else {
            return false;
        };
        let mut src = StateSrc {
            key: state.key,
            data: &state.data,
        };
        // SAFETY: our own source behind the retrieve callback.
        unsafe {
            restore(
                self.handle,
                Some(src_retrieve),
                std::ptr::from_mut(&mut src).cast(),
                0,
                null(),
            ) == LV2_STATE_SUCCESS
        }
    }
}

#[derive(Default)]
struct StateSink {
    key: LV2_URID,
    data: Option<Vec<u8>>,
}

/// The host's state store callback.
///
/// # Safety
/// `handle` is a `StateSink`; `value` has `size` readable bytes.
unsafe extern "C" fn sink_store(
    handle: LV2_State_Handle,
    key: LV2_URID,
    value: *const c_void,
    size: usize,
    _type_: u32,
    _flags: u32,
) -> i32 {
    // SAFETY: the caller's contract.
    let sink = unsafe { &mut *(handle.cast::<StateSink>()) };
    if value.is_null() {
        return LV2_STATE_ERR_UNKNOWN;
    }
    // SAFETY: `size` readable bytes, per the store contract.
    let data = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) };
    sink.key = key;
    sink.data = Some(data.to_vec());
    LV2_STATE_SUCCESS
}

struct StateSrc<'a> {
    key: LV2_URID,
    data: &'a [u8],
}

/// The host's state retrieve callback.
///
/// # Safety
/// `handle` is a `StateSrc`; the out pointers are valid.
unsafe extern "C" fn src_retrieve(
    handle: LV2_State_Handle,
    key: LV2_URID,
    size: *mut usize,
    type_: *mut u32,
    flags: *mut u32,
) -> *const c_void {
    // SAFETY: the caller's contract.
    let src = unsafe { &*(handle.cast::<StateSrc>()) };
    if key != src.key {
        return null();
    }
    // SAFETY: valid out pointers, per the retrieve contract.
    unsafe {
        if !size.is_null() {
            *size = src.data.len();
        }
        if !type_.is_null() {
            *type_ = 0; // not recorded; the wrapper accepts any type
        }
        if !flags.is_null() {
            *flags = 0;
        }
    }
    src.data.as_ptr().cast()
}

/// Writes an atom:Sequence into a fixed buffer: the same wire format
/// `lv2/mod.rs` reads back (and `write_control_out` writes).
struct Forge<'b> {
    urids: Urids,
    buf: &'b mut [u8],
}

impl Forge<'_> {
    /// Write the empty-sequence header. The unit is 0 — the default stamp
    /// type (audio frames) for `run()`, per atom.h — which is what real
    /// hosts forge with, so every test covers that path too.
    fn header(&mut self) {
        let u = self.urids;
        self.put_u32(0, 8);
        self.put_u32(4, u.atom_sequence);
        self.put_u32(8, 0);
        self.put_u32(12, 0);
    }

    fn put_u32(&mut self, at: usize, v: u32) {
        if at + 4 <= self.buf.len() {
            self.buf[at..at + 4].copy_from_slice(&v.to_ne_bytes());
        }
    }

    fn put_i64(&mut self, at: usize, v: i64) {
        if at + 8 <= self.buf.len() {
            self.buf[at..at + 8].copy_from_slice(&v.to_ne_bytes());
        }
    }

    fn put_f32(&mut self, at: usize, v: f32) {
        if at + 4 <= self.buf.len() {
            self.buf[at..at + 4].copy_from_slice(&v.to_ne_bytes());
        }
    }

    /// The sequence's current size (from the body on).
    fn seq_size(&self) -> usize {
        if self.buf.len() < 4 {
            return 8;
        }
        u32::from_ne_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize
    }

    fn set_seq_size(&mut self, size: usize) {
        #[expect(clippy::cast_possible_truncation, reason = "a bounded buffer size")]
        let s = size as u32;
        self.put_u32(0, s);
    }

    /// Append a MIDI event.
    fn midi(&mut self, frame: i64, data: &[u8]) {
        let u = self.urids;
        let at = 8 + self.seq_size();
        let total = 16 + data.len();
        if at + total > self.buf.len() {
            return; // out of buffer: drop (a forge never overflows partly)
        }
        self.put_i64(at, frame);
        #[expect(clippy::cast_possible_truncation, reason = "a MIDI atom size")]
        self.put_u32(at + 8, data.len() as u32);
        self.put_u32(at + 12, u.midi_event);
        self.buf[at + 16..at + 16 + data.len()].copy_from_slice(data);
        self.set_seq_size(self.seq_size() + pad_size(total));
    }

    /// Append a `patch:Set` for `prop` = `value`.
    fn set(&mut self, frame: i64, prop: LV2_URID, value: f64) {
        let u = self.urids;
        let at = 8 + self.seq_size();
        // [id][otype][patch:property→URID][patch:value→Float]
        let prop_size = 16 + 4;
        let val_size = 16 + 4;
        let obj = 8 + pad_size(prop_size) + pad_size(val_size);
        let total = 16 + obj;
        if at + total > self.buf.len() {
            return;
        }
        self.put_i64(at, frame);
        #[expect(clippy::cast_possible_truncation, reason = "an object atom size")]
        self.put_u32(at + 8, obj as u32);
        self.put_u32(at + 12, u.atom_object);
        self.put_u32(at + 16, 0); // blank id
        self.put_u32(at + 20, u.patch_set);
        let p1 = at + 24;
        self.put_u32(p1, u.patch_property);
        self.put_u32(p1 + 4, 0); // context
        self.put_u32(p1 + 8, 4); // value atom size
        self.put_u32(p1 + 12, u.atom_urid);
        self.put_u32(p1 + 16, prop);
        let p2 = p1 + pad_size(prop_size);
        self.put_u32(p2, u.patch_value);
        self.put_u32(p2 + 4, 0); // context
        self.put_u32(p2 + 8, 4); // value atom size
        self.put_u32(p2 + 12, u.atom_float);
        #[expect(clippy::cast_possible_truncation, reason = "an atom:Float wire value")]
        let vf = value as f32;
        self.put_f32(p2 + 16, vf);
        self.set_seq_size(self.seq_size() + pad_size(total));
    }

    /// Append a `patch:Get` with no subject (for this plugin).
    fn get(&mut self) {
        let u = self.urids;
        let at = 8 + self.seq_size();
        let total = 16 + 8;
        if at + total > self.buf.len() {
            return;
        }
        self.put_i64(at, 0);
        self.put_u32(at + 8, 8);
        self.put_u32(at + 12, u.atom_object);
        self.put_u32(at + 16, 0); // blank id
        self.put_u32(at + 20, u.patch_get);
        self.set_seq_size(self.seq_size() + pad_size(total));
    }
}

impl Drop for Instance<'_> {
    fn drop(&mut self) {
        // SAFETY: the LV2 teardown order on a live plugin.
        unsafe {
            if self.active {
                if let Some(d) = (*self.desc).deactivate {
                    d(self.handle);
                }
            }
            if let Some(c) = (*self.desc).cleanup {
                c(self.handle);
            }
        }
    }
}

/// Instantiate the test plugin `P` in-process against `world` (what the
/// unit tests use instead of `dlopen`). The little in-process `Library`
/// header is leaked with the descriptor table it holds — descriptors are
/// `'static` anyway, and tests make a handful of instances.
///
/// # Errors
/// Instantiation refused.
pub fn instantiate<'w, P: Plugin>(
    world: &'w World,
    rate: f64,
    shape: (usize, usize),
) -> Result<Instance<'w>, HostError> {
    let table = [crate::lv2::descriptor::<P>()];
    let lib = Library::from_descriptors(&table)?;
    let uri = super::plugin_uri(P::info());
    let lib: &'static Library = Box::leak(Box::new(lib));
    lib.instantiate(&uri, world, rate, shape)
}
