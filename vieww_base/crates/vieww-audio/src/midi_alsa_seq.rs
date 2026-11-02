//! The ALSA sequencer: where software MIDI lives on Linux — the FFI half.
//!
//! Rawmidi ([`open_rawmidi`](super::MidiInput::open_rawmidi)) sees hardware
//! interfaces only; the sequencer is how applications exchange MIDI: a
//! client with ports, subscriptions wiring them, `snd-virtmidi` bridging
//! to hardware-shaped devices. `libasound.so.2` is loaded at run time
//! through the same [`Library`](crate::backend::dl::Library) the audio
//! backends use, so a machine without ALSA still gets an honest error
//! (and the port lists simply have no `seq:` entries).
//!
//! **The shape.** Every open port owns one sequencer client named
//! `vieww-audio` — that is what `aconnect -l` shows. A **virtual input**
//! ([`open_virtual_input`]) creates a port with WRITE and SUBS_WRITE
//! caps: a destination other applications connect their outputs to.
//! A **virtual output** ([`Out::open_virtual`]) creates a READ/SUBS_READ
//! port: a source other applications connect their inputs to, and every
//! message sent through it is broadcast to the subscribers.
//! [`open_input`] and [`Out::open`] target an existing `seq:C,P` port the
//! same way `aconnect` would: a subscription (`snd_seq_connect_from` /
//! `snd_seq_connect_to`), with the events we *send* additionally
//! addressed directly, so a port that cannot be subscribed still hears us.
//!
//! **The mapping.** MIDI wire bytes and sequencer events are two
//! spellings of one thing; the pure half of that mapping — [`seq_encode`]
//! / [`seq_decode`](super::seq_decode), the event-type numbers, the data
//! union's offsets — lives in [`midi_io`](super) so it compiles, and its
//! tests run, on every platform. This file holds it to the real ABI:
//! `RawEvent` is `snd_seq_event_t` (28 bytes: type, flags, tag, queue, an
//! 8-byte timestamp union, source, dest, and a 12-byte data union — 12,
//! not 16, because the `ext` and `quote` members are `packed`), and the
//! tests below ask the library itself wherever one is installed:
//! `snd_seq_event_length` reports the struct's own size for a fixed
//! event, and `sizeof + ext.len` for a variable one, which pins the data
//! union's offset too. The type numbers are the kernel ABI
//! (`<sound/asequencer.h>`, unchanged since 1998), verified here against
//! this machine's copy of both it and alsa-lib's `seq_event.h`.
//!
//! **Delivery.** A pump thread polls the sequencer's poll descriptor
//! (`poll(2)`; std links libc, so the symbol is declared like
//! `clock_gettime` in `backend::aaudio`) and drains the input queue with
//! `snd_seq_event_input`, copying each event out before the next call —
//! the library overwrites the event, and a variable event's SysEx bytes
//! live in the kernel's ring, so both are read immediately. What the
//! running-status [`Parser`](crate::midi::Parser) cannot spell (the
//! sequencer's 14-bit and RPN/NRPN events, its own bookkeeping) is
//! dropped by name in the pure half.
//!
//! **What runs where.** The unit tests here need only an installed
//! `libasound.so.2` (every Linux CI leg has one; where none is, they skip
//! with a line in the log, and the pure half still ran). The round trip
//! through the kernel — virtual ports, a subscription, encode, delivery,
//! decode — is `alsa_seq_virtual_ports_round_trip` in
//! `tests/live_backends.rs`, ignored by default like every live test,
//! run by the VM leg of `audio-backends.yml` (a stock kernel's sequencer,
//! `snd-virtmidi` for the rawmidi-shaped world).

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

use super::{seq_decode, seq_encode, seq_event, Feed, MidiPort, SeqEvent};
use crate::backend::dl::Library;

// std links libc, so `poll` is already in the process — the same
// declaration trick `backend::aaudio` uses for `clock_gettime`.
#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}
unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
}
const POLLIN: i16 = 0x001;
/// How long a pump waits before checking `stop`, so teardown never waits
/// longer than this.
const POLL_MS: c_int = 200;

/// `snd_seq_addr_t`: `client:port`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Addr {
    client: u8,
    port: u8,
}

/// `snd_seq_event_t`, the ABI layout of `<sound/asequencer.h>`: type,
/// flags, tag and queue at 0..4, the 8-byte timestamp union at 4, source
/// at 12, dest at 14, and the 12-byte data union at 16. It is 4-aligned
/// and 28 bytes on every word size — the union stays 12 because its
/// widest members (`ext`, `quote`) are `packed`, so `ext.ptr` sits at
/// data+4 unaligned. The test asking `snd_seq_event_length` holds all of
/// this to the library itself.
#[repr(C, align(4))]
struct RawEvent {
    type_: u8,
    flags: u8,
    tag: u8,
    queue: u8,
    time: [u8; 8],
    source: Addr,
    dest: Addr,
    data: [u8; 12],
}

// `SND_SEQ_OPEN_DUPLEX`: subscriptions are made through the output side
// even for a pure input, so every client is opened both ways.
const OPEN_DUPLEX: c_int = 3;
const NONBLOCK: c_int = 0x0001;
// Port capabilities (`SND_SEQ_PORT_CAP_*`).
const CAP_READ: u32 = 1 << 0;
const CAP_WRITE: u32 = 1 << 1;
const CAP_SUBS_READ: u32 = 1 << 5;
const CAP_SUBS_WRITE: u32 = 1 << 6;
// Port type: what we publish is an application port carrying MIDI
// (`SND_SEQ_PORT_TYPE_APPLICATION | SND_SEQ_PORT_TYPE_MIDI_GENERIC`).
const TYPE_PORT: u32 = (1 << 20) | (1 << 1);
// Addresses (`SND_SEQ_ADDRESS_*`, `SND_SEQ_QUEUE_DIRECT`).
const CLIENT_SYSTEM: i32 = 0;
const ADDRESS_SUBSCRIBERS: u8 = 254;
const ADDRESS_UNKNOWN: u8 = 253;
const QUEUE_DIRECT: u8 = 253;

/// The symbols this module needs, resolved once.
struct Api {
    _lib: Library,
    open: unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int, c_int) -> c_int,
    close: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_client_name: unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int,
    client_id: unsafe extern "C" fn(*mut c_void) -> c_int,
    create_simple_port: unsafe extern "C" fn(*mut c_void, *const c_char, u32, u32) -> c_int,
    delete_port: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    connect_from: unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int) -> c_int,
    connect_to: unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int) -> c_int,
    event_output: unsafe extern "C" fn(*mut c_void, *mut RawEvent) -> c_int,
    drain_output: unsafe extern "C" fn(*mut c_void) -> c_int,
    event_input: unsafe extern "C" fn(*mut c_void, *mut *mut RawEvent) -> c_int,
    event_input_pending: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    poll_descriptors_count: unsafe extern "C" fn(*mut c_void, i16) -> c_int,
    poll_descriptors: unsafe extern "C" fn(*mut c_void, *mut PollFd, u32, i16) -> c_int,
    /// Only the layout test calls this — it is the library's own witness
    /// that `RawEvent` is `snd_seq_event_t` (a fixed event's length is
    /// the struct's size, a variable one's adds `ext.len`).
    #[cfg_attr(not(test), expect(dead_code, reason = "the layout test calls it"))]
    event_length: unsafe extern "C" fn(*mut RawEvent) -> isize,
    strerror: unsafe extern "C" fn(c_int) -> *const c_char,
    client_info_malloc: unsafe extern "C" fn(*mut *mut c_void) -> c_int,
    client_info_free: unsafe extern "C" fn(*mut c_void),
    client_info_set_client: unsafe extern "C" fn(*mut c_void, c_int),
    client_info_get_client: unsafe extern "C" fn(*mut c_void) -> c_int,
    client_info_get_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    query_next_client: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    port_info_malloc: unsafe extern "C" fn(*mut *mut c_void) -> c_int,
    port_info_free: unsafe extern "C" fn(*mut c_void),
    port_info_set_client: unsafe extern "C" fn(*mut c_void, c_int),
    port_info_set_port: unsafe extern "C" fn(*mut c_void, c_int),
    port_info_get_port: unsafe extern "C" fn(*mut c_void) -> c_int,
    port_info_get_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    port_info_get_capability: unsafe extern "C" fn(*mut c_void) -> u32,
    query_next_port: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
}

impl Api {
    fn load() -> Result<Self, String> {
        let lib = Library::open(&["libasound.so.2"])?;
        macro_rules! sym {
            ($name:literal) => {
                lib.get(CStr::from_bytes_with_nul_unchecked(
                    concat!($name, "\0").as_bytes(),
                ))?
            };
        }
        // SAFETY: the types are alsa-lib's signatures, as declared in
        // <alsa/seq.h> / <alsa/seqmid.h> (checked against the 1.2.14
        // headers, including `snd_seq_poll_descriptors`' four arguments
        // and `snd_seq_event_length`'s `ssize_t`).
        unsafe {
            Ok(Self {
                open: sym!("snd_seq_open"),
                close: sym!("snd_seq_close"),
                set_client_name: sym!("snd_seq_set_client_name"),
                client_id: sym!("snd_seq_client_id"),
                create_simple_port: sym!("snd_seq_create_simple_port"),
                delete_port: sym!("snd_seq_delete_port"),
                connect_from: sym!("snd_seq_connect_from"),
                connect_to: sym!("snd_seq_connect_to"),
                event_output: sym!("snd_seq_event_output"),
                drain_output: sym!("snd_seq_drain_output"),
                event_input: sym!("snd_seq_event_input"),
                event_input_pending: sym!("snd_seq_event_input_pending"),
                poll_descriptors_count: sym!("snd_seq_poll_descriptors_count"),
                poll_descriptors: sym!("snd_seq_poll_descriptors"),
                event_length: sym!("snd_seq_event_length"),
                strerror: sym!("snd_strerror"),
                client_info_malloc: sym!("snd_seq_client_info_malloc"),
                client_info_free: sym!("snd_seq_client_info_free"),
                client_info_set_client: sym!("snd_seq_client_info_set_client"),
                client_info_get_client: sym!("snd_seq_client_info_get_client"),
                client_info_get_name: sym!("snd_seq_client_info_get_name"),
                query_next_client: sym!("snd_seq_query_next_client"),
                port_info_malloc: sym!("snd_seq_port_info_malloc"),
                port_info_free: sym!("snd_seq_port_info_free"),
                port_info_set_client: sym!("snd_seq_port_info_set_client"),
                port_info_set_port: sym!("snd_seq_port_info_set_port"),
                port_info_get_port: sym!("snd_seq_port_info_get_port"),
                port_info_get_name: sym!("snd_seq_port_info_get_name"),
                port_info_get_capability: sym!("snd_seq_port_info_get_capability"),
                query_next_port: sym!("snd_seq_query_next_port"),
                _lib: lib,
            })
        }
    }
}

/// The library, loaded once. `Err` is why (no libasound, or a missing
/// symbol — the honest "this machine cannot do sequencer MIDI").
fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(Api::load).as_ref().map_err(Clone::clone)
}

/// `r`, or the library's own text for it, as the error.
fn check(api: &Api, r: c_int, what: &str) -> std::io::Result<()> {
    if r >= 0 {
        return Ok(());
    }
    Err(std::io::Error::other(format!("{what}: {}", text(api, r))))
}

/// The library's own string for an error code.
fn text(api: &Api, r: c_int) -> String {
    // SAFETY: alsa's static error string for this code.
    let p = unsafe { (api.strerror)(r) };
    if p.is_null() {
        format!("error {r}")
    } else {
        // SAFETY: a static NUL-terminated string.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// A NUL-terminated name, or the honest error for one that cannot be.
fn c_name(name: &str) -> std::io::Result<CString> {
    CString::new(name)
        .map_err(|_| std::io::Error::other(format!("port name {name:?} has an interior NUL")))
}

/// A client named `vieww-audio`, non-blocking, both directions. Returns
/// the handle and our client id (the source address of what we send).
fn open_client(api: &Api) -> std::io::Result<(*mut c_void, u8)> {
    let name = c"vieww-audio";
    let mut seq: *mut c_void = std::ptr::null_mut();
    // SAFETY: the out-pointer is ours; the handle is closed by the
    // caller's `Drop`.
    let r = unsafe { (api.open)(&raw mut seq, name.as_ptr(), OPEN_DUPLEX, NONBLOCK) };
    if r < 0 {
        return Err(std::io::Error::other(format!(
            "no ALSA sequencer (snd_seq_open: {})",
            text(api, r)
        )));
    }
    if seq.is_null() {
        return Err(std::io::Error::other("snd_seq_open gave no handle"));
    }
    let id = unsafe { (api.client_id)(seq) };
    if !(0..=255).contains(&id) {
        // SAFETY: the handle we opened.
        unsafe { (api.close)(seq) };
        return Err(std::io::Error::other(format!("client id {id} is not a byte")));
    }
    // The name, so `aconnect -l` shows `vieww-audio` and not `Client-73`.
    // A failure here is cosmetic: keep the client.
    // SAFETY: the handle and a static C string.
    let _ = unsafe { (api.set_client_name)(seq, name.as_ptr()) };
    Ok((seq, id as u8))
}

/// A port on our client: `caps` say what it is, the type says what it
/// carries. Returns its port number.
fn create_port(api: &Api, seq: *mut c_void, name: &str, caps: u32) -> std::io::Result<c_int> {
    let name = c_name(name)?;
    // SAFETY: the handle, a valid C string, and alsa's caps/type flags.
    let port = unsafe { (api.create_simple_port)(seq, name.as_ptr(), caps, TYPE_PORT) };
    if port < 0 {
        return Err(std::io::Error::other(format!(
            "snd_seq_create_simple_port: {}",
            text(api, port)
        )));
    }
    Ok(port)
}

/// The `seq:C,P` id back into numbers.
fn parse_seq_id(id: &str) -> std::io::Result<(i32, i32)> {
    let bad = || std::io::Error::other(format!("not a sequencer port id: {id:?}"));
    let rest = id.strip_prefix("seq:").ok_or_else(bad)?;
    let (c, p) = rest.split_once(',').ok_or_else(bad)?;
    let (c, p): (i32, i32) = (c.parse().map_err(|_| bad())?, p.parse().map_err(|_| bad())?);
    if !(0..=255).contains(&c) || !(0..=255).contains(&p) {
        return Err(bad());
    }
    Ok((c, p))
}

/// The sequencer's ports, as sources (`inputs`) or destinations. Needs a
/// client of our own for the queries — transient, opened and closed here.
pub(super) fn ports(inputs: bool) -> Vec<MidiPort> {
    let Ok(api) = api() else {
        return Vec::new();
    };
    let Ok((seq, _)) = open_client(api) else {
        // No `/dev/snd/seq` (or permission refused): there is no
        // sequencer to list. Rawmidi's list still says what it can.
        return Vec::new();
    };
    let out = enumerate(api, seq, inputs);
    // SAFETY: the client we opened.
    unsafe { (api.close)(seq) };
    out
}

/// One pass of `snd_seq_query_next_client` / `snd_seq_query_next_port`:
/// the ports whose caps say data can leave (`READ`) for the input list,
/// or enter (`WRITE`) for the output list. The info structs are
/// allocated by the library itself (`*_malloc`/`*_free`), so their
/// layouts are the library's, never ours.
fn enumerate(api: &Api, seq: *mut c_void, inputs: bool) -> Vec<MidiPort> {
    let mut out = Vec::new();
    let mut ci: *mut c_void = std::ptr::null_mut();
    let mut pi: *mut c_void = std::ptr::null_mut();
    // SAFETY: out-pointers; freed below on every path that allocated
    // them. The queries fill the library's own structs, and the getters
    // read them back — nothing here trusts a layout of ours.
    unsafe {
        if (api.client_info_malloc)(&raw mut ci) < 0 || ci.is_null() {
            return out;
        }
        if (api.port_info_malloc)(&raw mut pi) < 0 || pi.is_null() {
            (api.client_info_free)(ci);
            return out;
        }
        (api.client_info_set_client)(ci, -1);
        while (api.query_next_client)(seq, ci) >= 0 {
            let client = (api.client_info_get_client)(ci);
            // The system client's own ports are the timer and the
            // announce port — bookkeeping, not MIDI.
            if client == CLIENT_SYSTEM {
                continue;
            }
            let cname = c_str((api.client_info_get_name)(ci));
            (api.port_info_set_client)(pi, client);
            (api.port_info_set_port)(pi, -1);
            while (api.query_next_port)(seq, pi) >= 0 {
                let caps = (api.port_info_get_capability)(pi);
                let want = if inputs { CAP_READ } else { CAP_WRITE };
                if caps & want == 0 {
                    continue;
                }
                let port = (api.port_info_get_port)(pi);
                if !u8::try_from(port).is_ok() {
                    continue;
                }
                let pname = c_str((api.port_info_get_name)(pi));
                out.push(MidiPort {
                    id: format!("seq:{client},{port}"),
                    name: format!("{cname}:{pname}"),
                });
            }
        }
        (api.port_info_free)(pi);
        (api.client_info_free)(ci);
    }
    out
}

/// A static C string, or the empty one.
fn c_str(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the library's NUL-terminated port or client name.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

// ---- input -------------------------------------------------------------------

/// An open sequencer input: the client, the port events arrive on, and
/// the pump thread that turns events into bytes.
struct Input {
    api: &'static Api,
    seq: *mut c_void,
    port: c_int,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

// SAFETY: the handle is used by the pump thread until `drop` joins it,
// and by `drop` alone afterwards — one thread at a time; ALSA client
// handles are themselves thread-safe.
unsafe impl Send for Input {}

impl Drop for Input {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: the client and port we created; after the join no
        // reader exists. Deleting the port also undoes the subscriptions
        // that fed it.
        unsafe {
            (self.api.delete_port)(self.seq, self.port);
            (self.api.close)(self.seq);
        }
    }
}

/// Open the existing source `port` (a `seq:C,P` id) and feed what it
/// sends: our own writable port, a subscription from theirs to ours.
pub(super) fn open_input(port: &MidiPort, feed: Feed) -> std::io::Result<Box<dyn Send>> {
    let (c, p) = parse_seq_id(&port.id)?;
    let api = api().map_err(std::io::Error::other)?;
    let (seq, _) = open_client(api)?;
    let my_port = create_port(api, seq, "in", CAP_WRITE | CAP_SUBS_WRITE)?;
    // SAFETY: the handle, our port, and the parsed address.
    if let Err(e) = check(api, unsafe { (api.connect_from)(seq, my_port, c, p) }, "snd_seq_connect_from") {
        // SAFETY: the client we opened on the failing path.
        unsafe {
            (api.delete_port)(seq, my_port);
            (api.close)(seq);
        }
        return Err(e);
    }
    Ok(Box::new(pump(api, seq, my_port, feed)?))
}

/// Publish a port other applications connect their outputs to.
pub(super) fn open_virtual_input(name: &str, feed: Feed) -> std::io::Result<Box<dyn Send>> {
    let api = api().map_err(std::io::Error::other)?;
    let (seq, _) = open_client(api)?;
    let my_port = create_port(api, seq, name, CAP_WRITE | CAP_SUBS_WRITE)?;
    Ok(Box::new(pump(api, seq, my_port, feed)?))
}

/// Start the pump thread for an open input port.
fn pump(
    api: &'static Api,
    seq: *mut c_void,
    port: c_int,
    feed: Feed,
) -> std::io::Result<Input> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);
    // SAFETY: the handle moves to the pump thread and back to `drop`
    // (which joins the thread first) — one user at a time, and ALSA
    // client handles are themselves thread-safe.
    let seq = SeqPtr(seq);
    let thread = std::thread::Builder::new()
        .name("vieww-midi-seq".into())
        .spawn(move || {
            // Arrival timestamps are only as good as the wake-up.
            let _rt = crate::rt::promote_helper_thread();
            // The whole `SeqPtr` is captured (not its field), so the
            // closure is `Send` by the impl above.
            pump_loop(api, seq, &stop2, feed);
        })
        .map_err(|e| {
            // SAFETY: the client this pump failed to start on.
            unsafe {
                (api.delete_port)(seq.0, port);
                (api.close)(seq.0);
            }
            e
        })?;
    Ok(Input {
        api,
        seq: seq.0,
        port,
        stop,
        thread: Some(thread),
    })
}

/// The sequencer handle as a thread-movable word.
#[derive(Clone, Copy)]
struct SeqPtr(*mut c_void);
// SAFETY: see `pump`: exclusive by construction.
unsafe impl Send for SeqPtr {}

/// The pump: wait for the sequencer to have something, drain it into the
/// feed, repeat until `stop`. Never blocks longer than [`POLL_MS`], so
/// teardown is bounded.
fn pump_loop(api: &'static Api, seq: SeqPtr, stop: &AtomicBool, mut feed: Feed) {
    let seq = seq.0;
    // SAFETY: the handle; POLLIN is what an input waits for.
    let n = unsafe { (api.poll_descriptors_count)(seq, POLLIN) }.max(0) as usize;
    let mut fds = vec![
        PollFd {
            fd: -1,
            events: POLLIN,
            revents: 0,
        };
        n.max(1)
    ];
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        // SAFETY: the handle's own descriptors, `n` of them, with the
        // events mask they were asked for.
        let filled = unsafe { (api.poll_descriptors)(seq, fds.as_mut_ptr(), fds.len() as u32, POLLIN) };
        if filled > 0 {
            // SAFETY: the descriptors the library filled, as many as it
            // said, with a bounded timeout.
            let r = unsafe { poll(fds.as_mut_ptr(), fds.len(), POLL_MS) };
            if r < 0 {
                // EINTR or a bad fd: back off briefly, then look again.
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        } else {
            // No descriptors (or the library refused): poll the plain
            // timeout so `stop` is still checked.
            std::thread::sleep(std::time::Duration::from_millis(
                u64::try_from(POLL_MS.max(0)).unwrap_or(200),
            ));
            continue;
        }
        // Drain whatever arrived — also on a spurious wake, so nothing
        // queued is left for the next one.
        loop {
            // SAFETY: the handle; `1` fetches from the kernel first.
            if unsafe { (api.event_input_pending)(seq, 1) } <= 0 {
                break;
            }
            let mut ev: *mut RawEvent = std::ptr::null_mut();
            // SAFETY: the handle; non-blocking, so an empty queue returns
            // an error rather than waiting.
            let r = unsafe { (api.event_input)(seq, &raw mut ev) };
            if r < 0 || ev.is_null() {
                break;
            }
            // SAFETY: the event the library just handed us — valid until
            // the next call, which is why `deliver` copies it out first.
            unsafe { deliver(ev, &mut feed) };
        }
    }
}

/// One event, decoded to wire bytes and fed. The event is the library's
/// (`ev`), read before any further sequencer call: the library reuses
/// the event, and a variable event's SysEx bytes live in the kernel's
/// ring, which the next read advances.
///
/// # Safety
/// `ev` is a valid library-owned event for this call only.
unsafe fn deliver(ev: *mut RawEvent, feed: &mut Feed) {
    let raw = unsafe { &*ev };
    let e = SeqEvent {
        type_: raw.type_,
        flags: raw.flags,
        data: raw.data,
    };
    // The variable data: `ext.len` at data+0, `ext.ptr` at data+4 — the
    // packed union — a word-sized pointer inside the data bytes.
    let ext: Vec<u8> = if raw.flags & seq_event::LENGTH_VARIABLE != 0 {
        let len = u32::from_ne_bytes(raw.data[0..4].try_into().unwrap_or([0; 4])) as usize;
        let ps = std::mem::size_of::<usize>();
        let mut b = [0u8; 8];
        b[..ps].copy_from_slice(&raw.data[4..4 + ps]);
        let ptr = usize::from_ne_bytes(b);
        if ptr == 0 || len == 0 {
            Vec::new()
        } else {
            // SAFETY: `len` bytes of the kernel ring the event points
            // into, valid until the next input call — copied here.
            unsafe { std::slice::from_raw_parts(ptr as *const u8, len) }.to_vec()
        }
    } else {
        Vec::new()
    };
    if let Some(bytes) = seq_decode(&e, &ext) {
        feed.bytes(&bytes);
    }
}

// ---- output ------------------------------------------------------------------

/// An open sequencer output: our port, and where its events go.
pub(super) struct Out {
    api: &'static Api,
    seq: *mut c_void,
    port: c_int,
    /// Our client id: the source address on every event.
    client: u8,
    /// The destination: broadcast to this port's subscribers (a virtual
    /// output, or a subscribed-to port), or a port addressed directly
    /// (the one that could not be subscribed).
    dest: Addr,
}

// SAFETY: ALSA client handles are thread-safe (the library serializes);
// the port lives until `drop`.
unsafe impl Send for Out {}

impl Out {
    /// Open the existing destination `port` (a `seq:C,P` id): our own
    /// readable port, subscribed to theirs, with events broadcast to the
    /// subscribers. A destination without SUBS_WRITE cannot be
    /// subscribed; its events are then addressed to it directly — a WRITE
    /// port hears those, subscription or not.
    pub(super) fn open(port: &MidiPort) -> std::io::Result<Self> {
        let (c, p) = parse_seq_id(&port.id)?;
        let api = api().map_err(std::io::Error::other)?;
        let (seq, client) = open_client(api)?;
        let my_port = create_port(api, seq, "out", CAP_READ | CAP_SUBS_READ)?;
        // SAFETY: the handle, our port, and the parsed address.
        let subscribed =
            check(api, unsafe { (api.connect_to)(seq, my_port, c, p) }, "snd_seq_connect_to");
        let dest = if subscribed.is_ok() {
            Addr {
                client: ADDRESS_SUBSCRIBERS,
                port: ADDRESS_UNKNOWN,
            }
        } else {
            Addr {
                client: c as u8,
                port: p as u8,
            }
        };
        Ok(Self {
            api,
            seq,
            port: my_port,
            client,
            dest,
        })
    }

    /// Publish a port other applications connect their inputs to; every
    /// message sent through it reaches all of them.
    pub(super) fn open_virtual(name: &str) -> std::io::Result<Self> {
        let api = api().map_err(std::io::Error::other)?;
        let (seq, client) = open_client(api)?;
        let my_port = create_port(api, seq, name, CAP_READ | CAP_SUBS_READ)?;
        Ok(Self {
            api,
            seq,
            port: my_port,
            client,
            dest: Addr {
                client: ADDRESS_SUBSCRIBERS,
                port: ADDRESS_UNKNOWN,
            },
        })
    }

    /// Send one complete message's wire bytes as one sequencer event,
    /// direct-dispatched (no queue: the timing is the send's own).
    pub(super) fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let Some(e) = seq_encode(bytes) else {
            // The sink only ever writes whole model messages, which all
            // spell — this is the honest refusal if one ever does not.
            return Err(std::io::Error::other(
                "a message with no sequencer event spelling",
            ));
        };
        let mut raw = RawEvent {
            type_: e.type_,
            flags: e.flags,
            tag: 0,
            queue: QUEUE_DIRECT,
            time: [0; 8],
            source: Addr {
                client: self.client,
                port: u8::try_from(self.port).unwrap_or(0),
            },
            dest: self.dest,
            data: e.data,
        };
        if e.type_ == seq_event::SYSEX {
            // The bytes ride with the event: `ext.len` at data+0 (the
            // encode already wrote it), `ext.ptr` at data+4 — packed. The
            // library copies variable data synchronously, so the
            // pointer's life is this call.
            let ps = std::mem::size_of::<usize>();
            let ptr = bytes.as_ptr() as usize;
            raw.data[4..4 + ps].copy_from_slice(&ptr.to_ne_bytes()[..ps]);
        }
        // SAFETY: the handle, and a fully-formed event whose variable
        // data is `bytes`, alive for the call.
        check(
            self.api,
            unsafe { (self.api.event_output)(self.seq, &raw mut raw) },
            "snd_seq_event_output",
        )?;
        // SAFETY: the handle. Non-blocking: a full kernel queue is -EAGAIN
        // with the event still buffered in the library, flushed by the
        // next send or drop — not an error a sender can act on.
        let _ = unsafe { (self.api.drain_output)(self.seq) };
        Ok(())
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        // SAFETY: the client and port we created; deleting the port
        // removes the subscriptions that carried our events.
        unsafe {
            (self.api.delete_port)(self.seq, self.port);
            (self.api.close)(self.seq);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No libasound here? The pure half's tests in `midi_io.rs` still ran;
    /// say so and pass — an installed-library check cannot fail a machine
    /// that has no library to check against.
    fn api_or_skip() -> Option<&'static Api> {
        match api() {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("skipped: no libasound to check against ({e})");
                None
            }
        }
    }

    #[test]
    fn the_event_layout_is_the_librarys_own() {
        let Some(api) = api_or_skip() else {
            return;
        };
        // A fixed event's length is `sizeof(snd_seq_event_t)` — the
        // library's, not ours.
        let mut ev = RawEvent {
            type_: seq_event::NOTEON,
            flags: 0,
            tag: 0,
            queue: 0,
            time: [0; 8],
            source: Addr { client: 0, port: 0 },
            dest: Addr { client: 0, port: 0 },
            data: [0; 12],
        };
        // SAFETY: a plain stack event.
        let len = unsafe { (api.event_length)(&raw mut ev) };
        assert_eq!(len, size_of::<RawEvent>() as isize);
        // A variable event's length adds `ext.len`, which the library
        // reads at data+0 — pinning the data union's offset (16) too.
        ev.type_ = seq_event::SYSEX;
        ev.flags = seq_event::LENGTH_VARIABLE;
        ev.data[0..4].copy_from_slice(&17u32.to_ne_bytes());
        // SAFETY: as above.
        let len = unsafe { (api.event_length)(&raw mut ev) };
        assert_eq!(len, (size_of::<RawEvent>() + 17) as isize);
        // The fields this file fills by hand, at the ABI's offsets.
        assert_eq!(std::mem::offset_of!(RawEvent, time), 4);
        assert_eq!(std::mem::offset_of!(RawEvent, source), 12);
        assert_eq!(std::mem::offset_of!(RawEvent, dest), 14);
        assert_eq!(std::mem::offset_of!(RawEvent, data), 16);
        assert_eq!(size_of::<RawEvent>(), 28);
        assert_eq!(size_of::<Addr>(), 2);
    }

    #[test]
    fn seq_ids_parse_or_refuse() {
        assert_eq!(parse_seq_id("seq:128,3").unwrap(), (128, 3));
        assert_eq!(parse_seq_id("seq:0,0").unwrap(), (0, 0));
        for bad in [
            "hw:0,0", "seq:", "seq:1", "seq:1,", "seq:,1", "seq:-1,0", "seq:256,0", "seq:1,256",
            "seq:x,y",
        ] {
            assert!(parse_seq_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn without_a_sequencer_opening_refuses_honestly() {
        // Where `/dev/snd/seq` exists this opens and the live test's job
        // begins; where it does not (a container, a runner's kernel) the
        // refusal must say what is missing, not hang or fake a port.
        if api_or_skip().is_none() {
            return;
        }
        let (tx, _rx) = std::sync::mpsc::channel();
        let feed = Feed {
            tx,
            started: super::super::Started::now(),
            parser: crate::midi::Parser::new(),
        };
        match open_virtual_input("vieww-probe", feed) {
            Ok(input) => drop(input),
            Err(e) => assert!(
                e.to_string().to_lowercase().contains("sequencer")
                    || e.to_string().to_lowercase().contains("no such file"),
                "the refusal says why: {e}"
            ),
        }
        // And the port lists, asked the same thing, stay empty rather
        // than failing — enumeration is a query.
        let _ = ports(true);
        let _ = ports(false);
    }
}
