//! Web MIDI, through the `vieww_midi` shim — the FFI half.
//!
//! The Web Audio backend ([`crate::backend::web`]) set the pattern this
//! follows: no bindings crate, a wasm import module the page maps to a
//! shipped JavaScript file, and plain numbers across the boundary. The
//! Rust side imports eight functions from the module `"vieww_midi"` and
//! exports one; `web/vieww-midi.js` (shipped with this crate) provides
//! the imports — map the module name to it with an import map, call its
//! `attach(instance.exports)` after instantiating the wasm, and the
//! ports it offers are this crate's.
//!
//! **The shape.** `navigator.requestMIDIAccess` is asynchronous and
//! permission-gated, which no synchronous wasm import can wait for, so
//! the contract is staged and says so: `attach` starts the request, and
//! until it resolves the port lists are empty and opening one fails —
//! a page that needs the ports at once awaits the shim's `ready()`
//! first. Ports are matched by their `MIDIPort.id` (UTF-8 across the
//! boundary); every call re-reads the access's current maps, so a
//! hot-plug between listing and opening is a not-found, not a stale
//! handle, and a port that appears later is listed as soon as it exists.
//!
//! **Input.** The shim sets each open port's `onmidimessage`; the
//! handler copies `event.data` into the input's scratch buffer in wasm
//! memory and calls [`vieww_midi_receive`], which parses, timestamps
//! against the input's own start, and queues — the same [`Feed`] every
//! platform shares. Web MIDI delivers complete messages (a SysEx
//! arrives whole); a message longer than the scratch is the shim's own
//! chunking, which the running-status parser reassembles. Timestamps
//! count from the shim's clock ([`clock_ms`],
//! `performance.now()`), because `std::time::Instant` panics on this
//! target.
//!
//! **Output.** [`Out::send`] hands each flushed message's bytes to
//! `vieww_midi_out_send`; the shim copies them out of wasm memory (a
//! view of the buffer must not be kept — growth detaches it) and calls
//! `MIDIOutput.send`. A SysEx sent without the permission the shim
//! asked for is the browser's security error, which the result-code
//! half ([`shim_check`](super::shim_check)) reports as
//! [`PermissionDenied`](std::io::ErrorKind::PermissionDenied).
//!
//! **What is verified where.** This half compiles for
//! `wasm32-unknown-unknown` (the audio-backends `targets` CI leg,
//! clippy `-D warnings`); the pure half of the protocol — the
//! string-copy and result-code conventions
//! ([`shim_string`](super::shim_string),
//! [`shim_check`](super::shim_check)) — is tested on every platform,
//! in [`midi_io`](super); and the shim's own logic runs under Node
//! against a simulated Web MIDI (`web/test-midi.mjs`, the same shape
//! as the audio shim's test). None of it has run in a real browser
//! from this sandbox — no Chrome here, no MIDI keyboard either — the
//! first browser run is the owner's to make, and this paragraph is
//! where that is said plainly.

use std::cell::{Cell, RefCell};

use super::{shim_check, shim_string, Feed, MidiPort};

#[link(wasm_import_module = "vieww_midi")]
unsafe extern "C" {
    /// The shim's clock: `performance.now()`, milliseconds since the
    /// page loaded — what input timestamps count from.
    #[link_name = "vieww_midi_clock"]
    pub(super) fn clock_ms() -> f64;
    /// How many input (`dir` 0) or output (`dir` 1) ports MIDI access
    /// offers — 0 before access has resolved, or where there is none.
    fn vieww_midi_count(dir: u32) -> u32;
    /// Copy port `i`'s id (`which` 0) or name (`which` 1) into `buf`,
    /// at most `cap` bytes of it; returns the string's full byte
    /// length. The protocol [`shim_string`](super::shim_string) reads.
    fn vieww_midi_string(dir: u32, i: u32, which: u32, buf: *mut u8, cap: u32) -> u32;
    /// Open input port `id` as `handle`, whose scratch the shim copies
    /// each arriving message into. 0, or a code
    /// [`shim_check`](super::shim_check) maps.
    fn vieww_midi_open(
        handle: u32,
        scratch: *mut u8,
        scratch_len: u32,
        id: *const u8,
        id_len: u32,
    ) -> i32;
    /// Close input `handle`: the feed is gone, stop delivering.
    fn vieww_midi_close(handle: u32);
    /// Open output port `id` as `handle`. As [`vieww_midi_open`]'s
    /// codes.
    fn vieww_midi_out_open(handle: u32, id: *const u8, id_len: u32) -> i32;
    /// Send the `len` bytes at `bytes` to output `handle`.
    fn vieww_midi_out_send(handle: u32, bytes: *const u8, len: u32) -> i32;
    /// Close output `handle`.
    fn vieww_midi_out_close(handle: u32);
}

/// One open input: the feed (parse, timestamp, queue) and the scratch
/// buffer the shim copies each message into before calling
/// [`vieww_midi_receive`]. The buffer lives here until the input
/// closes, so the address the shim was told at open stays valid.
struct Entry {
    feed: Feed,
    scratch: Vec<u8>,
}

thread_local! {
    static INPUTS: RefCell<Vec<Option<Entry>>> = const { RefCell::new(Vec::new()) };
}

/// The scratch a message is delivered through: room for the longest
/// SysEx a device is likely to send whole; longer ones arrive in
/// pieces, which the running-status parser reassembles.
const SCRATCH: usize = 4_096;

// Where output handles come from. An output keeps no Rust-side state,
// so a count of past opens is all the identity one needs; the shim
// keys its own map by these numbers and deletes the entry on close,
// so a number is never two ports at once.
thread_local! {
    static NEXT_OUT: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn inputs() -> Vec<MidiPort> {
    ports(0)
}

pub(super) fn outputs() -> Vec<MidiPort> {
    ports(1)
}

/// The ports access currently offers, `dir` 0 inputs / 1 outputs —
/// empty until `requestMIDIAccess` resolves (the module docs' staged
/// contract) and fresh on every call, so hot-plugs appear as they
/// happen.
fn ports(dir: u32) -> Vec<MidiPort> {
    // SAFETY: a pure query against the shim's current port maps.
    let n = unsafe { vieww_midi_count(dir) };
    let mut v = Vec::with_capacity(n as usize);
    for i in 0..n {
        // The id is what `open` matches on and the name what a person
        // reads — `which` 0 and 1 in the shim's protocol.
        let id = string(dir, i, 0);
        let name = string(dir, i, 1);
        v.push(MidiPort { id, name });
    }
    v
}

/// Port `i`'s id or name, through [`shim_string`]'s copy-and-retry
/// protocol.
fn string(dir: u32, i: u32, which: u32) -> String {
    shim_string(|buf| {
        // SAFETY: the shim copies at most `buf.len()` bytes of one
        // port's id or name into it.
        unsafe {
            vieww_midi_string(
                dir,
                i,
                which,
                buf.as_mut_ptr(),
                u32::try_from(buf.len()).unwrap_or(u32::MAX),
            )
        }
    })
}

pub(super) fn open_input(port: &MidiPort, feed: Feed) -> std::io::Result<Box<dyn Send>> {
    let (handle, scratch) = INPUTS.with_borrow_mut(|inputs| {
        let i = inputs.iter().position(Option::is_none).unwrap_or_else(|| {
            inputs.push(None);
            inputs.len() - 1
        });
        let mut scratch = vec![0u8; SCRATCH];
        let ptr = scratch.as_mut_ptr();
        inputs[i] = Some(Entry { feed, scratch });
        (u32::try_from(i).unwrap_or(u32::MAX), ptr)
    });
    let id = port.id.as_bytes();
    // SAFETY: the handle is ours and unused, the scratch lives in the
    // registry until close (wasm memory addresses are stable), and the
    // id is valid UTF-8 for the call.
    let r = unsafe {
        vieww_midi_open(
            handle,
            scratch,
            u32::try_from(SCRATCH).unwrap_or(u32::MAX),
            id.as_ptr(),
            u32::try_from(id.len()).unwrap_or(u32::MAX),
        )
    };
    shim_check(r, "opening the Web MIDI input").inspect_err(|_| {
        // The shim refused: give the slot back before anyone can be
        // handed it.
        INPUTS.with_borrow_mut(|inputs| {
            if let Some(slot) = inputs.get_mut(handle as usize) {
                *slot = None;
            }
        });
    })?;
    Ok(Box::new(Input(handle)))
}

/// An open input's lease: dropped when the input is — the shim's close
/// stops delivery, and the registry slot goes back.
struct Input(u32);

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: the handle we opened; the shim's close.
        unsafe { vieww_midi_close(self.0) };
        INPUTS.with_borrow_mut(|inputs| {
            if let Some(slot) = inputs.get_mut(self.0 as usize) {
                *slot = None;
            }
        });
    }
}

/// Called by the shim with a message it has copied into input
/// `handle`'s scratch: parse, timestamp against the input's own start,
/// queue. Returns 0 once the input is gone — the shim's cue to stop
/// delivering to it.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn vieww_midi_receive(handle: u32, len: u32) -> u32 {
    INPUTS.with_borrow_mut(|inputs| {
        let Some(Some(e)) = inputs.get_mut(handle as usize) else {
            return 0;
        };
        let n = (len as usize).min(e.scratch.len());
        u32::from(e.feed.bytes(&e.scratch[..n]))
    })
}

/// A Web MIDI output port: the handle `vieww_midi_out_send` sends
/// through, closed when the sink is dropped.
#[derive(Debug)]
pub(super) struct Out(u32);

impl Out {
    pub(super) fn open(port: &MidiPort) -> std::io::Result<Self> {
        let handle = NEXT_OUT.with(Cell::get);
        NEXT_OUT.set(handle.wrapping_add(1));
        let id = port.id.as_bytes();
        // SAFETY: a handle no live output holds, and a valid UTF-8 id.
        let r = unsafe {
            vieww_midi_out_open(
                handle,
                id.as_ptr(),
                u32::try_from(id.len()).unwrap_or(u32::MAX),
            )
        };
        shim_check(r, "opening the Web MIDI output")?;
        Ok(Self(handle))
    }

    pub(super) fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        // SAFETY: our handle; the bytes are ours, and the shim copies
        // them out of wasm memory before returning — a view of the
        // buffer is never kept, so growth cannot detach it mid-send.
        let r = unsafe {
            vieww_midi_out_send(
                self.0,
                bytes.as_ptr(),
                u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            )
        };
        shim_check(r, "sending to the Web MIDI output")
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        // SAFETY: our handle; the shim's close, which also forgets the
        // number, so the next open may reuse it after the wrap.
        unsafe { vieww_midi_out_close(self.0) };
    }
}
