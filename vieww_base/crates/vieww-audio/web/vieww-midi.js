// The browser side of vieww-audio's Web MIDI backend (src/midi_web.rs).
//
// The Rust code imports vieww_midi_clock, vieww_midi_count,
// vieww_midi_string, vieww_midi_open, vieww_midi_close,
// vieww_midi_out_open, vieww_midi_out_send and vieww_midi_out_close from
// the wasm import module "vieww_midi", and exports vieww_midi_receive. Map
// the module name to this file (an import map, or your bundler's alias),
// and after instantiating the wasm call `attach(instance.exports)`:
//
//   <script type="importmap">{ "imports": { "vieww_midi": "./vieww-midi.js" } }</script>
//   import init from "./app.js";            // wasm-bindgen output, say
//   import { attach, ready } from "vieww_midi";
//   const wasm = await init();
//   attach(wasm);
//   await ready();            // now the port lists are real
//
// navigator.requestMIDIAccess is asynchronous and permission-gated, which
// no synchronous wasm import can wait for, so the contract is staged and
// honest: attach() starts the request; until it resolves the port counts
// are zero and opening one returns the "no access" code; after it
// resolves every call reads the access's current maps, so hot-plugs
// appear as the browser reports them. A page that wants the ports at
// once awaits ready() first; onMidiState(cb) hands it each statechange
// event's port as it happens.
//
// SysEx: the crate's MIDI model carries system exclusive both ways, so
// the default request asks for it ({ sysex: true }) — Chrome answers
// with its permission prompt. Pass { sysex: false } to attach() to skip
// the prompt and lose SysEx (on input it is simply not delivered; on
// output the browser raises its SecurityError, which the Rust side
// reports as a permission error). A permission revoked between opens is
// the same "no access" code on the next one.
//
// Timestamps: vieww_midi_clock is performance.now() (Date.now() where
// that is missing) — the clock the Rust side's input timestamps count
// from, because std::time::Instant cannot exist on this target.
//
// Buffers: each open input owns a scratch buffer inside wasm memory whose
// address the open call carried; an arriving message is copied there and
// vieww_midi_receive(handle, len) is called — chunked at the scratch's
// length for a message longer than it (Web MIDI delivers messages whole,
// so the pieces are this shim's own chunking, never the browser's). No
// view of wasm memory is ever kept: the memory can grow and detach views,
// so every copy makes a fresh one (the same rule as the audio shim).

let wasm = null;
let access = null;
let accessError = null;
let requested = false;
let wantSysex = true;
const inputs = new Map(); // handle → { handle, port, scratch, scratchLen, closed }
const outputs = new Map(); // handle → MIDIOutput
const stateListeners = new Set();
const waiters = [];

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * Give the shim the wasm instance's exports (memory and
 * vieww_midi_receive). Options: `sysex` — ask for system exclusive
 * (default true, the model carries it; false skips the browser's
 * permission prompt and loses SysEx). Starts the MIDI access request.
 */
export function attach(exports, options = {}) {
  wasm = exports;
  wantSysex = options.sysex !== false;
  request();
}

/** Resolve once MIDI access has been granted; reject with the browser's (or the page's) reason it was not. */
export function ready() {
  if (access) return Promise.resolve(access);
  if (accessError) return Promise.reject(accessError);
  return new Promise((res, rej) => waiters.push([res, rej]));
}

/**
 * Call `cb(port, event)` on every MIDI state change (a port connected or
 * disconnected). Returns an unsubscribe function.
 */
export function onMidiState(cb) {
  stateListeners.add(cb);
  return () => stateListeners.delete(cb);
}

function request() {
  if (requested) return;
  requested = true;
  const nav = globalThis.navigator;
  if (!nav || typeof nav.requestMIDIAccess !== "function") {
    accessError = new Error("no Web MIDI on this page");
    for (const [, rej] of waiters.splice(0)) rej(accessError);
    return;
  }
  nav.requestMIDIAccess({ sysex: wantSysex }).then(
    (a) => {
      access = a;
      access.onstatechange = (e) => {
        for (const cb of stateListeners) {
          try {
            cb(e && e.port, e);
          } catch (_) {
            /* a listener's error is its own */
          }
        }
      };
      for (const [res] of waiters.splice(0)) res(a);
    },
    (e) => {
      accessError = e;
      for (const [, rej] of waiters.splice(0)) rej(e);
    },
  );
}

// ---- the port maps, read fresh every call ----------------------------------

function portsOf(dir) {
  if (!access) return [];
  const map = dir === 0 ? access.inputs : access.outputs;
  return [...map.values()];
}

function byId(dir, id) {
  return portsOf(dir).find((p) => p.id === id) || null;
}

// A promise-returning method on a real MIDIPort; a fake may return
// nothing — either way a port that will not open is not an error to us
// (its messages simply do not arrive, its sends are discarded).
function settle(p) {
  if (p && typeof p.catch === "function") p.catch(() => {});
}

/** Import: the clock — performance.now(), milliseconds since the page loaded. */
export function vieww_midi_clock() {
  const p = globalThis.performance;
  return p && typeof p.now === "function" ? p.now() : Date.now();
}

/** Import: how many input (dir 0) or output (dir 1) ports access offers. */
export function vieww_midi_count(dir) {
  return portsOf(dir).length;
}

/**
 * Import: copy port `i`'s id (`which` 0) or name (`which` 1), as UTF-8,
 * into `buf` — at most `cap` bytes — and return the string's full byte
 * length, so a short buffer can be retried at the exact size (the
 * protocol src/midi_io.rs's `shim_string` reads).
 */
export function vieww_midi_string(dir, i, which, buf, cap) {
  const port = portsOf(dir)[i];
  if (!port || !wasm) return 0;
  // A nameless port is listed under its id rather than as an empty label.
  const bytes = encoder.encode(which === 0 ? port.id : port.name || port.id);
  const n = Math.min(cap, bytes.length);
  if (n > 0) new Uint8Array(wasm.memory.buffer, buf, n).set(bytes.subarray(0, n));
  return bytes.length;
}

// ---- input ------------------------------------------------------------------

function closeInput(st) {
  if (st.closed) return;
  st.closed = true;
  st.port.onmidimessage = null;
  inputs.delete(st.handle);
  if (typeof st.port.close === "function") settle(st.port.close());
}

/** Deliver one MIDIMessageEvent: copy its bytes into the input's scratch, chunk by chunk. */
function deliver(st, e) {
  if (st.closed || !wasm) return;
  const raw = e && e.data;
  if (!raw || raw.length === 0) return;
  const data = raw instanceof Uint8Array ? raw : Uint8Array.from(raw);
  for (let at = 0; at < data.length && !st.closed; at += st.scratchLen) {
    const n = Math.min(st.scratchLen, data.length - at);
    new Uint8Array(wasm.memory.buffer, st.scratch, n).set(data.subarray(at, at + n));
    if (wasm.vieww_midi_receive(st.handle, n) === 0) {
      // The Rust side's input is gone: stop delivering to it.
      closeInput(st);
      return;
    }
  }
}

/**
 * Import: open input port `id` for `handle`, whose scratch the shim
 * copies each message into. 0 ok; −1 no access (or no shim); −2 no such
 * port; −3 the browser refused.
 */
export function vieww_midi_open(handle, scratch, scratchLen, idPtr, idLen) {
  if (!wasm || !access) return -1;
  const id = decoder.decode(new Uint8Array(wasm.memory.buffer, idPtr, idLen));
  const port = byId(0, id);
  if (!port) return -2;
  if (inputs.has(handle)) closeInput(inputs.get(handle));
  const st = { handle, port, scratch, scratchLen: Math.max(1, scratchLen), closed: false };
  port.onmidimessage = (e) => deliver(st, e);
  if (typeof port.open === "function") settle(port.open());
  inputs.set(handle, st);
  return 0;
}

/** Import: close input `handle`. */
export function vieww_midi_close(handle) {
  const st = inputs.get(handle);
  if (st) closeInput(st);
}

// ---- output -----------------------------------------------------------------

/**
 * Import: open output port `id` for `handle`. Codes as vieww_midi_open.
 */
export function vieww_midi_out_open(handle, idPtr, idLen) {
  if (!wasm || !access) return -1;
  const id = decoder.decode(new Uint8Array(wasm.memory.buffer, idPtr, idLen));
  const port = byId(1, id);
  if (!port) return -2;
  if (typeof port.open === "function") settle(port.open());
  outputs.set(handle, port);
  return 0;
}

/**
 * Import: send the `len` bytes at `bytes` to output `handle`. Sending to
 * a disconnected port is a discard by the spec, not an error; the
 * refusal that can happen is the browser's (SecurityError for SysEx
 * without permission), which is the −3 the Rust side reports.
 */
export function vieww_midi_out_send(handle, bytes, len) {
  const port = outputs.get(handle);
  if (!port) return -2;
  try {
    // Copied out of wasm memory: a view of the buffer must not be kept.
    port.send(new Uint8Array(wasm.memory.buffer, bytes, len).slice());
    return 0;
  } catch (_) {
    return -3;
  }
}

/** Import: close output `handle`. */
export function vieww_midi_out_close(handle) {
  const port = outputs.get(handle);
  if (!port) return;
  outputs.delete(handle);
  if (typeof port.close === "function") settle(port.close());
}
