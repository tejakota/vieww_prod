// Tests web/vieww-midi.js under Node with a simulated Web MIDI: a fake
// navigator.requestMIDIAccess hands out an access object with Map-backed
// inputs and outputs; a fake wasm module provides the memory the shim
// copies through and a vieww_midi_receive that records what arrived. The
// test drives both directions — listing before and after access grants,
// the string protocol (short reads, truncation, UTF-8), opening by id,
// delivery and its chunking, the gone-input cue, closing, sending and
// its refusals, hot-plug, and the no-Web-MIDI and no-SysEx modes — and
// swaps the memory out from under an open input to pin the never-keep-a-
// view rule. Run: node web/test-midi.mjs
import assert from "node:assert/strict";
import * as shim from "./vieww-midi.js";

const enc = new TextEncoder();
const dec = new TextDecoder();

// ---- fake ports -----------------------------------------------------------
class FakeInput {
  constructor(id, name) {
    this.id = id;
    this.name = name;
    this.type = "input";
    this.state = "connected";
    this.onmidimessage = null;
    this.opened = 0;
    this.closed = 0;
  }
  open() {
    this.opened++;
    return Promise.resolve(this);
  }
  close() {
    this.closed++;
    return Promise.resolve(this);
  }
}
class FakeOutput {
  constructor(id, name, opts = {}) {
    this.id = id;
    this.name = name;
    this.type = "output";
    this.state = "connected";
    this.sent = [];
    this.refuse = opts.refuse || null; // an error name to throw from send
    this.opened = 0;
    this.closed = 0;
  }
  open() {
    this.opened++;
    return Promise.resolve(this);
  }
  close() {
    this.closed++;
    return Promise.resolve(this);
  }
  send(data) {
    if (this.refuse) {
      const e = new Error("not permitted");
      e.name = this.refuse;
      throw e;
    }
    this.sent.push(Array.from(data));
  }
}

// ---- the access, granted only when the test says so -----------------------
let grant = null;
let requestedWith = null;
const access = { inputs: new Map(), outputs: new Map(), onstatechange: null, sysexEnabled: true };
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    requestMIDIAccess(opts) {
      requestedWith = opts;
      return new Promise((res) => {
        grant = () => res(access);
      });
    },
  },
});

// ---- fake wasm: the memory both sides copy through, and receive -----------
const memory = { buffer: new ArrayBuffer(1 << 16) };
const received = []; // one [handle, bytes] per vieww_midi_receive call
let respond = 1; // what receive answers: 1 keep listening, 0 input gone
const SCRATCH_AT = 8192; // where the wasm side put its input scratch
const ID_AT = 16384; // where it wrote the id it asked to open
const OUT_AT = 40960; // where it wrote the bytes it asked to send

function putBytes(at, bytes) {
  new Uint8Array(memory.buffer, at, bytes.length).set(bytes);
  return at;
}

shim.attach({
  memory,
  vieww_midi_receive(handle, len) {
    received.push([handle, Array.from(new Uint8Array(memory.buffer, SCRATCH_AT, len))]);
    return respond;
  },
});

// ---- before access resolves: staged and honest ----------------------------
assert.deepEqual(requestedWith, { sysex: true }, "attach asked for SysEx by default");
assert.equal(shim.vieww_midi_count(0), 0, "no inputs before access resolves");
assert.equal(shim.vieww_midi_count(1), 0, "no outputs before access resolves");
putBytes(ID_AT, enc.encode("kb-1"));
assert.equal(shim.vieww_midi_open(0, SCRATCH_AT, 64, ID_AT, 4), -1, "open before access → -1");
assert.equal(shim.vieww_midi_out_open(0, ID_AT, 4), -1, "out open before access → -1");

// ---- grant: the lists read fresh from the maps ----------------------------
grant();
const wasmAccess = await shim.ready();
assert.equal(wasmAccess, access, "ready resolved with the access");
const kb = new FakeInput("kb-1", "Clavier mécanique");
const synth = new FakeOutput("synth-1", "Test Synth");
access.inputs.set("kb-1", kb);
access.outputs.set("synth-1", synth);
assert.equal(shim.vieww_midi_count(0), 1);
assert.equal(shim.vieww_midi_count(1), 1);

// The string protocol: full length back, at most `cap` written (the
// retry protocol src/midi_io.rs's shim_string relies on).
function readShimString(dir, i, which, cap) {
  const at = 32768;
  const n = shim.vieww_midi_string(dir, i, which, at, cap);
  return [n, dec.decode(new Uint8Array(memory.buffer, at, Math.min(cap, n)))];
}
const [n, id] = readShimString(0, 0, 0, 256);
assert.equal(n, 4, "the id's full byte length");
assert.equal(id, "kb-1");
const [n1] = readShimString(0, 0, 0, 2);
assert.equal(n1, 4, "a short read still reports the full length");
const [, head] = readShimString(0, 0, 0, 2);
assert.equal(head, "kb", "and wrote only what fit");
const [nn, name] = readShimString(0, 0, 1, 256);
assert.equal(nn, enc.encode("Clavier mécanique").length, "the name's byte length (UTF-8, not chars)");
assert.equal(name, "Clavier mécanique");
assert.equal(shim.vieww_midi_string(0, 9, 0, 32768, 256), 0, "no port 9: empty string");

// ---- input: open, deliver, chunk, gone, close -----------------------------
assert.equal(shim.vieww_midi_open(3, SCRATCH_AT, 8, ID_AT, 4), 0, "opened kb-1 as handle 3");
assert.equal(kb.opened, 1, "the shim opened the port");
assert.ok(kb.onmidimessage, "delivery is wired");
kb.onmidimessage({ data: Uint8Array.of(0x90, 60, 100) });
assert.deepEqual(received, [[3, [0x90, 60, 100]]], "one message, one receive, bytes intact");
// A message longer than the scratch (8 here) arrives in pieces.
received.length = 0;
kb.onmidimessage({ data: Uint8Array.of(0xF0, 1, 2, 3, 4, 5, 6, 7, 8, 0xF7) });
assert.deepEqual(
  received,
  [
    [3, [0xF0, 1, 2, 3, 4, 5, 6, 7]],
    [3, [8, 0xF7]],
  ],
  "chunked at the scratch length, in order",
);
// The gone cue: receive answers 0, the shim stops delivering and closes.
received.length = 0;
respond = 0;
kb.onmidimessage({ data: Uint8Array.of(0xF8) });
assert.equal(received.length, 1, "the message that ended it was delivered once");
assert.equal(kb.onmidimessage, null, "no delivery to a gone input");
assert.equal(kb.closed, 1, "the port was closed");
respond = 1;
// Close from the wasm side: the same teardown.
assert.equal(shim.vieww_midi_open(4, SCRATCH_AT, 8, ID_AT, 4), 0);
kb.closed = 0;
assert.ok(kb.onmidimessage);
shim.vieww_midi_close(4);
assert.equal(kb.onmidimessage, null, "close removed the handler");
assert.equal(kb.closed, 1, "close closed the port");
shim.vieww_midi_close(4); // idempotent
// An id that is not a port.
putBytes(ID_AT, enc.encode("nope"));
assert.equal(shim.vieww_midi_open(5, SCRATCH_AT, 8, ID_AT, 4), -2, "unknown id → -2");
// Reopening the same handle replaces, not doubles.
putBytes(ID_AT, enc.encode("kb-1"));
kb.closed = 0;
assert.equal(shim.vieww_midi_open(6, SCRATCH_AT, 8, ID_AT, 4), 0, "id back to kb-1");
assert.equal(kb.closed, 0, "a fresh handle closes nothing");
assert.equal(shim.vieww_midi_open(6, SCRATCH_AT, 8, ID_AT, 4), 0, "same handle again");
assert.equal(kb.closed, 1, "the first opening was closed");
assert.ok(kb.onmidimessage);
shim.vieww_midi_close(6);

// The memory can grow under an open input: no view of it was kept.
assert.equal(shim.vieww_midi_open(7, SCRATCH_AT, 8, ID_AT, 4), 0);
memory.buffer = new ArrayBuffer(1 << 17);
received.length = 0;
kb.onmidimessage({ data: Uint8Array.of(0xFE) });
assert.deepEqual(received, [[7, [0xFE]]], "delivery survives a memory growth");
shim.vieww_midi_close(7);

// ---- output ----------------------------------------------------------------
putBytes(ID_AT, enc.encode("synth-1"));
assert.equal(shim.vieww_midi_out_open(9, ID_AT, 7), 0, "opened synth-1 as handle 9");
assert.equal(synth.opened, 1);
assert.equal(shim.vieww_midi_out_send(9, 0, 0), 0, "a zero-length send is allowed");
putBytes(OUT_AT, Uint8Array.of(0x80, 60, 0));
assert.equal(shim.vieww_midi_out_send(9, OUT_AT, 3), 0);
assert.deepEqual(synth.sent, [[], [0x80, 60, 0]], "bytes arrived, copied out of wasm memory");
assert.equal(shim.vieww_midi_out_send(10, OUT_AT, 3), -2, "an unopened handle → -2");
// The browser's refusal (SecurityError for SysEx without permission).
const refuser = new FakeOutput("bad-1", "Refuser", { refuse: "SecurityError" });
access.outputs.set("bad-1", refuser);
putBytes(ID_AT, enc.encode("bad-1"));
assert.equal(shim.vieww_midi_out_open(10, ID_AT, 5), 0);
putBytes(OUT_AT, Uint8Array.of(0xF0, 1, 0xF7));
assert.equal(shim.vieww_midi_out_send(10, OUT_AT, 3), -3, "SecurityError → -3");
assert.deepEqual(refuser.sent, [], "nothing was sent");
// Close: the port is closed, the handle forgotten.
shim.vieww_midi_out_close(9);
assert.equal(synth.closed, 1);
assert.equal(shim.vieww_midi_out_send(9, OUT_AT, 3), -2, "a closed handle is gone");
assert.equal(shim.vieww_midi_out_send(10, OUT_AT, 3), -3, "the refuser still refuses");

// ---- hot-plug: the maps are read fresh, the state is reported --------------
const states = [];
const unsubscribe = shim.onMidiState((p) => states.push(p));
const pad = new FakeInput("pad-1", "Test Pad");
access.inputs.set("pad-1", pad);
assert.equal(shim.vieww_midi_count(0), 2, "a new port is listed as soon as it exists");
access.onstatechange({ port: pad });
assert.deepEqual(states, [pad], "the listener saw the port");
unsubscribe();
access.outputs.delete("bad-1");
assert.equal(shim.vieww_midi_count(1), 1, "a removed port is gone from the list");

// ---- the clock -------------------------------------------------------------
const t0 = shim.vieww_midi_clock();
assert.ok(Number.isFinite(t0) && t0 >= 0, "the clock is a real number");
await new Promise((r) => setTimeout(r, 2));
assert.ok(shim.vieww_midi_clock() > t0, "the clock advances");

// ---- a page with no Web MIDI at all ----------------------------------------
{
  const fresh = await import("./vieww-midi.js?noapi");
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: {} });
  fresh.attach({ memory, vieww_midi_receive: () => 1 });
  await assert.rejects(fresh.ready(), /no Web MIDI/);
  assert.equal(fresh.vieww_midi_count(0), 0);
  fresh.attach({ memory, vieww_midi_receive: () => 1 }); // requested once: no retry storm
  assert.equal(fresh.vieww_midi_open(0, SCRATCH_AT, 8, ID_AT, 4), -1);
}

// ---- the no-SysEx posture ---------------------------------------------------
{
  const fresh = await import("./vieww-midi.js?nosysex");
  let asked = null;
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: { requestMIDIAccess: (opts) => Promise.resolve((asked = opts, access)) },
  });
  fresh.attach({ memory, vieww_midi_receive: () => 1 }, { sysex: false });
  await fresh.ready();
  assert.deepEqual(asked, { sysex: false }, "sysex: false skips the permission prompt");
}

console.log("web midi shim: all checks passed");
