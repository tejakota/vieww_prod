//! Live MIDI input and output.
//!
//! [`midi`](crate::midi) is everything above the port; this is the port.
//!
//! **Ports.** [`input_ports`] and [`output_ports`] list what the system
//! offers, and [`MidiInput::open`] / [`MidiOutput::open`] take a
//! [`MidiPort::id`] (or a port's exact name):
//!
//! | Platform | API | Port ids |
//! |---|---|---|
//! | Linux | ALSA rawmidi (`/dev/snd/midiC{c}D{d}`, the file system only) and the ALSA sequencer — software ports: other applications' virtual ports, `snd-virtmidi` — over `libasound.so.2` loaded at run time | `hw:C,D`, `seq:C,P` |
//! | Windows | WinMM (`midiIn*` with a callback, `midiOut*`; SysEx both ways) | device index |
//! | macOS, iOS | CoreMIDI (a client, an input port connected to the source, `MIDISend`) | `kMIDIPropertyUniqueID` |
//! | Android | AMidi, the NDK side of `android.media.midi` (`libmediandk.so` loaded at run time); the Java side owns enumeration, so the host opens the device it holds through `MidiInput::open_amidi` | — |
//! | Web | (feature `web-midi`) Web MIDI through the `vieww_midi` JS shim (`web/vieww-midi.js`, the same shape as the Web Audio backend's), once `navigator.requestMIDIAccess` has resolved | the `MIDIPort.id` string |
//!
//! On Linux a pair of **virtual ports** can be published for other
//! applications to connect to ([`MidiInput::open_virtual`],
//! [`MidiOutput::open_virtual`]): a synth announces an input, a controller
//! an output, and `aconnect` lists both under the client `vieww-audio`.
//!
//! **Input.** Bytes are parsed with the running-status [`Parser`] and each
//! message is timestamped on arrival — in the OS callback itself on
//! Windows and macOS, on the reader thread on Linux, on the shim's call on
//! the web — against the input's own start instant. The audio thread drains
//! messages with [`MidiInput::poll`], which never blocks.
//! [`MidiInput::from_reader`] does the same over any `Read`.
//!
//! **Output.** [`MidiOutput`] writes messages to any `Write`; a port's
//! sink is a [`PortSink`], which sends each flushed message the way the
//! platform wants it (a packed short message or a SysEx buffer on WinMM,
//! a packet list on CoreMIDI, bytes on rawmidi, a sequencer event on
//! ALSA-seq, an `AMidiInputPort_send` on Android, a `MIDIOutput.send` on
//! the web).

use std::io::{Read, Write};
use std::sync::mpsc;

use crate::midi::{Message, Parser};

/// A message and when it arrived, in seconds since the input opened.
#[derive(Debug, Clone, PartialEq)]
pub struct Timed {
    /// Arrival time.
    pub time: f64,
    /// The message.
    pub message: Message,
}

/// When an input opened — the zero its timestamps count from. `Instant`
/// wherever std has one; on the web `performance.now()` through the
/// shim's imported `vieww_midi_clock`, because `std::time::Instant`
/// **panics** on `wasm32-unknown-unknown`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Started {
    #[cfg(not(target_arch = "wasm32"))]
    at: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    at: f64,
}

impl Started {
    #[cfg(not(target_arch = "wasm32"))]
    fn now() -> Self {
        Self {
            at: std::time::Instant::now(),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn now() -> Self {
        // SAFETY: the shim's clock, milliseconds since the page loaded.
        Self {
            at: unsafe { platform::clock_ms() },
        }
    }

    /// Seconds since `now` was taken.
    fn elapsed_secs(self) -> f64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.at.elapsed().as_secs_f64()
        }
        #[cfg(target_arch = "wasm32")]
        {
            // SAFETY: the shim's clock again.
            (unsafe { platform::clock_ms() } - self.at).max(0.0) / 1_000.0
        }
    }
}

/// A MIDI port the system offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiPort {
    /// What [`MidiInput::open`] / [`MidiOutput::open`] take.
    pub id: String,
    /// Its display name.
    pub name: String,
}

/// The MIDI inputs present (sources: keyboards, controllers, interfaces).
#[must_use]
pub fn input_ports() -> Vec<MidiPort> {
    platform::inputs()
}

/// The MIDI outputs present (destinations: synths, interfaces).
#[must_use]
pub fn output_ports() -> Vec<MidiPort> {
    platform::outputs()
}

/// The port in `ports` that `id` names — by id, else by exact name.
fn find<'a>(ports: &'a [MidiPort], id: &str) -> Option<&'a MidiPort> {
    ports
        .iter()
        .find(|p| p.id == id)
        .or_else(|| ports.iter().find(|p| p.name == id))
}

fn not_found(id: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, format!("no MIDI port {id:?}"))
}

/// Where a port's bytes go: parse, timestamp, queue. Called from the
/// platform's callback (or reader thread).
pub(crate) struct Feed {
    tx: mpsc::Sender<Timed>,
    started: Started,
    parser: Parser,
}

impl Feed {
    /// Bytes that just arrived. Returns `false` once the input is gone.
    pub(crate) fn bytes(&mut self, b: &[u8]) -> bool {
        let time = self.started.elapsed_secs();
        for message in self.parser.feed(b) {
            if self.tx.send(Timed { time, message }).is_err() {
                return false;
            }
        }
        true
    }
}

/// A live MIDI input.
pub struct MidiInput {
    rx: mpsc::Receiver<Timed>,
    started: Started,
    /// The platform's open port, closed when the input is dropped.
    _port: Option<Box<dyn Send>>,
}

impl std::fmt::Debug for MidiInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidiInput")
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl MidiInput {
    fn with_feed(
        open: impl FnOnce(Feed) -> std::io::Result<Option<Box<dyn Send>>>,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let started = Started::now();
        let port = open(Feed {
            tx,
            started,
            parser: Parser::new(),
        })?;
        Ok(Self {
            rx,
            started,
            _port: port,
        })
    }

    /// Start reading `source` on a thread of its own.
    pub fn from_reader<R: Read + Send + 'static>(source: R) -> Self {
        Self::with_feed(|feed| {
            reader_thread(source, feed);
            Ok(None)
        })
        .unwrap_or_else(|_| unreachable!("spawning a reader cannot fail here"))
    }

    /// Open the input port `id` (a [`MidiPort::id`] from [`input_ports`],
    /// or a port's exact name).
    ///
    /// # Errors
    /// No such port ([`NotFound`](std::io::ErrorKind::NotFound)), the
    /// platform refused it, or MIDI is not supported here.
    pub fn open(id: &str) -> std::io::Result<Self> {
        let ports = platform::inputs();
        let port = find(&ports, id).ok_or_else(|| not_found(id))?.clone();
        Self::with_feed(|feed| platform::open_input(&port, feed).map(Some))
    }

    /// Open an ALSA rawmidi device.
    ///
    /// # Errors
    /// The device does not exist or cannot be opened (no Linux, no device,
    /// no permission) — said as the OS said it.
    pub fn open_rawmidi(card: u32, device: u32) -> std::io::Result<Self> {
        let f = std::fs::File::open(format!("/dev/snd/midiC{card}D{device}"))?;
        Ok(Self::from_reader(f))
    }

    /// A virtual input port on the ALSA sequencer (Linux), that other
    /// applications connect their outputs to — as `aconnect` shows it,
    /// under the client `vieww-audio` with this port's name. Events are
    /// received whichever side connected, and from any sender that
    /// addresses the port directly.
    ///
    /// The port is listed by [`output_ports`] (as a `seq:C,P` id) once
    /// created — a virtual input is a MIDI *destination* — including by
    /// this process, which is how a counterpart here can find it — and
    /// disappears when the input is dropped.
    ///
    /// # Errors
    /// No ALSA sequencer (the library missing, or `/dev/snd/seq` —
    /// `sudo modprobe snd-seq`), or the port could not be created.
    #[cfg(all(target_os = "linux", not(target_os = "android")))]
    pub fn open_virtual(name: &str) -> std::io::Result<Self> {
        Self::with_feed(|feed| platform::open_virtual_input(name, feed).map(Some))
    }

    /// Open the AMidi output port `port` (0-based) of `device` — the bytes
    /// other applications send to the Java `MidiDevice` arrive here,
    /// timestamped against this input's own clock like every other input.
    /// The Java side owns enumeration (there is no port list to `open` by
    /// id), which is why this takes the device and a number.
    ///
    /// # Errors
    /// No `libmediandk.so` (or the `AMidi*` symbols in it — API 29+), the
    /// port number out of range, or the port refused to open.
    #[cfg(target_os = "android")]
    pub fn open_amidi(device: &AmidiDevice, port: usize) -> std::io::Result<Self> {
        Self::with_feed(|feed| platform::open_amidi_input(device, port, feed).map(Some))
    }

    /// Every message received since the last poll. Never blocks.
    pub fn poll(&self) -> Vec<Timed> {
        self.rx.try_iter().collect()
    }

    /// Seconds since this input opened (the timestamps' clock).
    #[must_use]
    pub fn now(&self) -> f64 {
        self.started.elapsed_secs()
    }
}

/// An AMidi device: what the Java side holds after
/// `MidiManager.openDevice`, passed here as the `IBinder`
/// `MidiDevice.getBinder()` returns (Android). Inputs and outputs open on
/// it by number ([`MidiInput::open_amidi`], [`MidiOutput::open_amidi`]);
/// the lists stay empty because the Java side owns enumeration.
#[cfg(target_os = "android")]
pub use platform::AmidiDevice;

/// Read `source` into `feed` on a thread until either ends.
fn reader_thread<R: Read + Send + 'static>(mut source: R, mut feed: Feed) {
    std::thread::Builder::new()
        .name("vieww-midi-in".into())
        .spawn(move || {
            // Arrival timestamps are only as good as the wake-up.
            let _rt = crate::rt::promote_helper_thread();
            let mut buf = [0u8; 256];
            loop {
                match source.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if !feed.bytes(&buf[..n]) {
                            return;
                        }
                    }
                }
            }
        })
        .ok();
}

/// A live MIDI output over any byte sink.
#[derive(Debug)]
pub struct MidiOutput<W: Write> {
    sink: W,
}

impl<W: Write> MidiOutput<W> {
    /// Wrap a sink.
    pub const fn new(sink: W) -> Self {
        Self { sink }
    }

    /// Send one message.
    ///
    /// # Errors
    /// The sink's write error.
    pub fn send(&mut self, m: &Message) -> std::io::Result<()> {
        self.sink.write_all(&m.to_bytes())?;
        self.sink.flush()
    }

    /// The sink back.
    pub fn into_inner(self) -> W {
        self.sink
    }
}

impl MidiOutput<PortSink> {
    /// Open the output port `id` (a [`MidiPort::id`] from
    /// [`output_ports`], or a port's exact name).
    ///
    /// # Errors
    /// As [`MidiInput::open`].
    pub fn open(id: &str) -> std::io::Result<Self> {
        let ports = platform::outputs();
        let port = find(&ports, id).ok_or_else(|| not_found(id))?;
        Ok(Self::new(PortSink {
            pending: Vec::new(),
            parser: Parser::new(),
            out: platform::Out::open(port)?,
        }))
    }

    /// A virtual output port on the ALSA sequencer (Linux), that other
    /// applications connect their inputs to. Messages sent through this
    /// output reach every connected subscriber.
    ///
    /// The port is listed by [`input_ports`] once created — a virtual
    /// output is a MIDI *source* — and disappears when the output is
    /// dropped.
    ///
    /// # Errors
    /// As [`MidiInput::open_virtual`].
    #[cfg(all(target_os = "linux", not(target_os = "android")))]
    pub fn open_virtual(name: &str) -> std::io::Result<Self> {
        Ok(Self::new(PortSink {
            pending: Vec::new(),
            parser: Parser::new(),
            out: platform::open_virtual_out(name)?,
        }))
    }

    /// Open the AMidi input port `port` (0-based) of `device` — messages
    /// sent here are written into the Java `MidiDevice`, the direction a
    /// controller app's output goes. `MidiOutput::send` flushes each
    /// message as one `AMidiInputPort_send`.
    ///
    /// # Errors
    /// As [`MidiInput::open_amidi`].
    #[cfg(target_os = "android")]
    pub fn open_amidi(device: &AmidiDevice, port: usize) -> std::io::Result<Self> {
        Ok(Self::new(PortSink {
            pending: Vec::new(),
            parser: Parser::new(),
            out: platform::Out::open_amidi(device, port)?,
        }))
    }
}

/// A system MIDI output port as a byte sink: bytes are collected, and
/// each flush parses them into messages and sends each the platform's
/// way.
pub struct PortSink {
    pending: Vec<u8>,
    parser: Parser,
    out: platform::Out,
}

impl std::fmt::Debug for PortSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortSink")
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl Write for PortSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let bytes = std::mem::take(&mut self.pending);
        for m in self.parser.feed(&bytes) {
            self.out.send(&m.to_bytes())?;
        }
        Ok(())
    }
}

impl MidiOutput<std::fs::File> {
    /// Open an ALSA rawmidi device for output.
    ///
    /// # Errors
    /// As [`MidiInput::open_rawmidi`].
    pub fn open_rawmidi(card: u32, device: u32) -> std::io::Result<Self> {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(format!("/dev/snd/midiC{card}D{device}"))?;
        Ok(Self::new(f))
    }
}

/// The ALSA rawmidi devices present, as `(card, device)`.
#[must_use]
pub fn list_rawmidi() -> Vec<(u32, u32)> {
    let Ok(dir) = std::fs::read_dir("/dev/snd") else {
        return Vec::new();
    };
    let mut out: Vec<(u32, u32)> = dir
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let rest = name.strip_prefix("midiC")?;
            let (c, d) = rest.split_once('D')?;
            Some((c.parse().ok()?, d.parse().ok()?))
        })
        .collect();
    out.sort_unstable();
    out
}

/// The length of a short message from its status byte (WinMM hands them
/// over packed in a `u32`).
#[cfg_attr(not(any(windows, test)), expect(dead_code, reason = "WinMM only"))]
pub(crate) const fn short_len(status: u8) -> usize {
    match status {
        0xC0..=0xDF | 0xF1 | 0xF3 => 2,
        0xF6 | 0xF8..=0xFF => 1,
        _ => 3,
    }
}

/// Walk a CoreMIDI `MIDIPacketList`, handing each packet's bytes to `f`.
/// Layout (`#pragma pack(4)`): count at 0, first packet at 4; a packet's
/// 64-bit timestamp at 0, length at 8, data at 10; with `align4` (arm64)
/// each following packet starts at the next 4-byte boundary, as the SDK's
/// `MIDIPacketNext` does.
///
/// # Safety
/// `list` points at a valid packet list of that layout.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "ios", test)),
    expect(dead_code, reason = "CoreMIDI only")
)]
pub(crate) unsafe fn walk_packet_list(list: *const u8, align4: bool, mut f: impl FnMut(&[u8])) {
    // SAFETY: within the list, per the layout above.
    unsafe {
        let n = list.cast::<u32>().read_unaligned();
        let mut p = list.add(4);
        for _ in 0..n {
            let len = usize::from(p.add(8).cast::<u16>().read_unaligned());
            f(std::slice::from_raw_parts(p.add(10), len));
            let next = p.add(10 + len);
            p = if align4 {
                next.add(next.align_offset(4))
            } else {
                next
            };
        }
    }
}

// ---- the Web MIDI bridge's pure half (wasm) --------------------------------
//
// The `vieww_midi` shim (web/vieww-midi.js, `midi_web.rs` the FFI half)
// speaks two small conventions across the wasm boundary, and they are
// protocol — wrong on either side and bytes or errors go astray — so
// they live here, compile on every platform, and their tests (below,
// and web/test-midi.mjs on the JS side) hold both halves to the same
// numbers.

/// Read one string out of the shim: `read` is handed a buffer and
/// returns the string's full byte length while copying at most the
/// buffer's worth into it — a short read, then this retry with the
/// exact size. DOMStrings are well-formed UTF-16, so the bytes are
/// UTF-8 the shim encoded; the lossy decode only guards a shim bug
/// (and a name changed between the two reads — ids, what `open`
/// matches on, are stable).
#[cfg_attr(
    not(any(all(target_arch = "wasm32", feature = "web-midi"), test)),
    expect(dead_code, reason = "the Web MIDI shim is wasm with feature web-midi")
)]
pub(crate) fn shim_string(mut read: impl FnMut(&mut [u8]) -> u32) -> String {
    let mut buf = [0u8; 128];
    let n = read(&mut buf) as usize;
    if n <= buf.len() {
        return String::from_utf8_lossy(&buf[..n]).into_owned();
    }
    let mut exact = vec![0u8; n];
    let m = read(&mut exact) as usize;
    String::from_utf8_lossy(&exact[..m.min(exact.len())]).into_owned()
}

/// Map a shim result code to what it is: 0 is success; −1, no MIDI
/// access (the shim not attached, `requestMIDIAccess` not resolved,
/// rejected, or absent); −2, no such port; −3, the browser's refusal
/// (SysEx without its permission). Anything else is a shim this file
/// does not know — said as the unknown it is, never guessed at.
#[cfg_attr(
    not(any(all(target_arch = "wasm32", feature = "web-midi"), test)),
    expect(dead_code, reason = "the Web MIDI shim is wasm with feature web-midi")
)]
pub(crate) fn shim_check(code: i32, what: &str) -> std::io::Result<()> {
    match code {
        0 => Ok(()),
        -1 => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("{what}: Web MIDI is not available (or its permission was denied)"),
        )),
        -2 => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{what}: no such Web MIDI port"),
        )),
        -3 => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{what}: the browser refused"),
        )),
        c => Err(std::io::Error::other(format!(
            "{what}: the shim returned {c}"
        ))),
    }
}

// ---- the ALSA sequencer bridge's pure half (Linux) ------------------------
//
// The sequencer is where software MIDI lives on Linux — another
// application's virtual ports, `snd-virtmidi` — which rawmidi cannot see.
// `midi_alsa_seq.rs` is the FFI half (through `libasound.so.2` loaded at
// run time, the same `dl.rs` the audio backends use); these functions are
// the mapping between MIDI wire bytes and sequencer events, written over
// a byte view of the event's data union so they compile — and their tests
// run — on every platform, like `walk_packet_list` above.

/// `SND_SEQ_EVENT_*` values this bridge uses. The numbers are the ABI
/// `libasound` and the kernel's sequencer share; the tests below (and
/// `midi_alsa_seq.rs`'s cross-check, which asks the library itself
/// wherever one is installed) hold them to it.
#[cfg_attr(
    not(any(all(target_os = "linux", not(target_os = "android")), test)),
    expect(dead_code, reason = "the ALSA sequencer is Linux")
)]
pub(crate) mod seq_event {
    /// Note on.
    pub(crate) const NOTEON: u8 = 6;
    /// Note off.
    pub(crate) const NOTEOFF: u8 = 7;
    /// Polyphonic key pressure (aftertouch).
    pub(crate) const KEYPRESS: u8 = 8;
    /// Control change.
    pub(crate) const CONTROLLER: u8 = 10;
    /// Program change.
    pub(crate) const PGMCHANGE: u8 = 11;
    /// Channel pressure.
    pub(crate) const CHANPRESS: u8 = 12;
    /// Pitch bend (the value is the *centered* −8192..=8191 one).
    pub(crate) const PITCHBEND: u8 = 13;
    /// 14-bit controller value — refused on decode (see [`seq_decode`]);
    /// the refusal tests pin these numbers.
    #[cfg_attr(not(test), expect(dead_code, reason = "the refusal tests pin these"))]
    pub(crate) const CONTROL14: u8 = 14;
    /// Non-registered parameter — refused on decode.
    #[cfg_attr(not(test), expect(dead_code, reason = "the refusal tests pin these"))]
    pub(crate) const NONREGPARAM: u8 = 15;
    /// Registered parameter — refused on decode.
    #[cfg_attr(not(test), expect(dead_code, reason = "the refusal tests pin these"))]
    pub(crate) const REGPARAM: u8 = 16;
    /// Song position pointer.
    pub(crate) const SONGPOS: u8 = 20;
    /// Song select.
    pub(crate) const SONGSEL: u8 = 21;
    /// MIDI time code quarter frame.
    pub(crate) const QFRAME: u8 = 22;
    /// Clock.
    pub(crate) const START: u8 = 30;
    /// Continue.
    pub(crate) const CONTINUE: u8 = 31;
    /// Stop.
    pub(crate) const STOP: u8 = 32;
    /// Timing clock.
    pub(crate) const CLOCK: u8 = 36;
    /// Tune request.
    pub(crate) const TUNE: u8 = 40;
    /// Reset.
    pub(crate) const RESET: u8 = 41;
    /// Active sensing.
    pub(crate) const SENSING: u8 = 42;
    /// System exclusive — the bytes ride the event's variable data.
    pub(crate) const SYSEX: u8 = 130;
    /// A subscription was made — the sequencer's own bookkeeping, refused
    /// on decode.
    #[cfg_attr(not(test), expect(dead_code, reason = "the refusal tests pin these"))]
    pub(crate) const PORT_SUBSCRIBED: u8 = 66;
    /// A subscription was undone — bookkeeping, refused on decode.
    #[cfg_attr(not(test), expect(dead_code, reason = "the refusal tests pin these"))]
    pub(crate) const PORT_UNSUBSCRIBED: u8 = 67;
    /// `SND_SEQ_EVENT_LENGTH_VARIABLE`, the flag a variable-length event
    /// (SysEx) carries.
    pub(crate) const LENGTH_VARIABLE: u8 = 1 << 2;
}

/// One MIDI message as a sequencer event: the event type, its flags, and
/// the event's 12-byte `data` union as bytes — the `note` and `control`
/// members' fields at their C offsets (`note.channel` 0, `note.note` 1,
/// `note.velocity` 2; `control.param` 4, `control.value` 8, both native
/// endianness; `ext.len` 0 for the variable kind, with `ext.ptr` at 4:
/// the union is 12 bytes because `ext` and `quote` are `packed`, which
/// `midi_alsa_seq.rs`'s cross-check against the library itself confirms).
/// The FFI half copies `data` into the real `snd_seq_event_t` and fills
/// `ext.ptr` for SysEx.
#[cfg_attr(
    not(any(all(target_os = "linux", not(target_os = "android")), test)),
    expect(dead_code, reason = "the ALSA sequencer is Linux")
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeqEvent {
    /// The `SND_SEQ_EVENT_*` type.
    pub type_: u8,
    /// Event flags — `LENGTH_VARIABLE` for SysEx, else 0.
    pub flags: u8,
    /// The event's data union, as bytes.
    pub data: [u8; 12],
}

/// Encode one complete MIDI message's wire bytes as a sequencer event.
/// SysEx keeps only its length here — the bytes themselves stay the
/// caller's (`midi_alsa_seq.rs` hands them to the library with the event,
/// synchronously). `None` for what is not a complete message this model
/// carries: a truncated one, or a status byte (`F4`, `F5`, bare `F7`) the
/// model cannot spell.
#[cfg_attr(
    not(any(all(target_os = "linux", not(target_os = "android")), test)),
    expect(dead_code, reason = "the ALSA sequencer is Linux")
)]
pub(crate) fn seq_encode(bytes: &[u8]) -> Option<SeqEvent> {
    let mut ev = SeqEvent {
        type_: 0,
        flags: 0,
        data: [0; 12],
    };
    let put = |d: &mut [u8; 12], at: usize, v: u32| {
        d[at..at + 4].copy_from_slice(&v.to_ne_bytes());
    };
    let status = *bytes.first()?;
    let ch = status & 0x0f;
    ev.data[0] = ch;
    match status {
        0x80..=0xAF if bytes.len() == 3 => {
            ev.type_ = match status & 0xf0 {
                0x80 => seq_event::NOTEOFF,
                0x90 => seq_event::NOTEON,
                _ => seq_event::KEYPRESS,
            };
            ev.data[1] = bytes[1];
            ev.data[2] = bytes[2];
        }
        0xB0..=0xBF if bytes.len() == 3 => {
            ev.type_ = seq_event::CONTROLLER;
            put(&mut ev.data, 4, u32::from(bytes[1]));
            put(&mut ev.data, 8, u32::from(bytes[2]));
        }
        0xC0..=0xDF if bytes.len() == 2 => {
            ev.type_ = if status < 0xD0 {
                seq_event::PGMCHANGE
            } else {
                seq_event::CHANPRESS
            };
            put(&mut ev.data, 8, u32::from(bytes[1]));
        }
        0xE0..=0xEF if bytes.len() == 3 => {
            ev.type_ = seq_event::PITCHBEND;
            // The centered value the event carries, as two's complement.
            let raw = (u32::from(bytes[2]) << 7) | u32::from(bytes[1]);
            put(&mut ev.data, 8, raw.wrapping_sub(8192));
        }
        // A SysEx is its framing: `F0 … F7`. An unterminated one is not a
        // complete message — the parser only ever hands complete ones to
        // the sink, and this refusal is the honest guard.
        0xF0 if bytes.len() >= 2 && bytes.last() == Some(&0xF7) => {
            ev.type_ = seq_event::SYSEX;
            ev.flags = seq_event::LENGTH_VARIABLE;
            put(
                &mut ev.data,
                0,
                u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            );
        }
        0xF1 if bytes.len() == 2 => {
            ev.type_ = seq_event::QFRAME;
            put(&mut ev.data, 8, u32::from(bytes[1]));
        }
        0xF2 if bytes.len() == 3 => {
            ev.type_ = seq_event::SONGPOS;
            put(
                &mut ev.data,
                8,
                (u32::from(bytes[2]) << 7) | u32::from(bytes[1]),
            );
        }
        0xF3 if bytes.len() == 2 => {
            ev.type_ = seq_event::SONGSEL;
            put(&mut ev.data, 8, u32::from(bytes[1]));
        }
        0xF6 => ev.type_ = seq_event::TUNE,
        0xF8 => ev.type_ = seq_event::CLOCK,
        0xFA => ev.type_ = seq_event::START,
        0xFB => ev.type_ = seq_event::CONTINUE,
        0xFC => ev.type_ = seq_event::STOP,
        0xFE => ev.type_ = seq_event::SENSING,
        0xFF => ev.type_ = seq_event::RESET,
        _ => return None,
    }
    Some(ev)
}

/// Decode a sequencer event back to MIDI wire bytes. `ext` is the event's
/// variable data — a SysEx, framing bytes included, as the kernel delivers
/// it. `None` for events with no spelling in this model: 14-bit
/// (`CONTROL14`) and registered/non-registered parameter events, whose
/// channel-voice semantics the model's 7-bit control change does not
/// carry, and the sequencer's own bookkeeping events (a subscription made,
/// a port gone).
#[cfg_attr(
    not(any(all(target_os = "linux", not(target_os = "android")), test)),
    expect(dead_code, reason = "the ALSA sequencer is Linux")
)]
pub(crate) fn seq_decode(ev: &SeqEvent, ext: &[u8]) -> Option<Vec<u8>> {
    let ch = ev.data[0] & 0x0f;
    let word =
        |d: &[u8; 12], at: usize| u32::from_ne_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]);
    let param = word(&ev.data, 4);
    let value = word(&ev.data, 8);
    let b7 = |v: u32| (v & 0x7f) as u8;
    match ev.type_ {
        t if t == seq_event::NOTEON => Some(vec![0x90 | ch, ev.data[1] & 0x7f, ev.data[2] & 0x7f]),
        t if t == seq_event::NOTEOFF => Some(vec![0x80 | ch, ev.data[1] & 0x7f, ev.data[2] & 0x7f]),
        t if t == seq_event::KEYPRESS => {
            Some(vec![0xA0 | ch, ev.data[1] & 0x7f, ev.data[2] & 0x7f])
        }
        t if t == seq_event::CONTROLLER => Some(vec![0xB0 | ch, b7(param), b7(value)]),
        t if t == seq_event::PGMCHANGE => Some(vec![0xC0 | ch, b7(value)]),
        t if t == seq_event::CHANPRESS => Some(vec![0xD0 | ch, b7(value)]),
        t if t == seq_event::PITCHBEND => {
            // The centered value, as two's complement; clamped, because a
            // sender that is not MIDI-shaped can hand anything.
            let v = (value as i32).clamp(-8192, 8191) + 8192;
            Some(vec![0xE0 | ch, (v & 0x7f) as u8, ((v >> 7) & 0x7f) as u8])
        }
        t if t == seq_event::SONGPOS => Some(vec![0xF2, b7(value), b7(value >> 7)]),
        t if t == seq_event::SONGSEL => Some(vec![0xF3, b7(value)]),
        t if t == seq_event::QFRAME => Some(vec![0xF1, b7(value)]),
        t if t == seq_event::START => Some(vec![0xFA]),
        t if t == seq_event::CONTINUE => Some(vec![0xFB]),
        t if t == seq_event::STOP => Some(vec![0xFC]),
        t if t == seq_event::CLOCK => Some(vec![0xF8]),
        t if t == seq_event::TUNE => Some(vec![0xF6]),
        t if t == seq_event::RESET => Some(vec![0xFF]),
        t if t == seq_event::SENSING => Some(vec![0xFE]),
        t if t == seq_event::SYSEX => Some(ext.to_vec()),
        _ => None,
    }
}

#[cfg(all(target_os = "linux", not(target_os = "android")))]
#[path = "midi_alsa_seq.rs"]
mod seq;

#[cfg(all(target_os = "linux", not(target_os = "android")))]
mod platform {
    use super::{reader_thread, seq, Feed, MidiPort};

    fn ports() -> Vec<MidiPort> {
        super::list_rawmidi()
            .into_iter()
            .map(|(c, d)| {
                let card = std::fs::read_to_string(format!("/proc/asound/card{c}/id"))
                    .map_or_else(|_| format!("card {c}"), |s| s.trim().to_owned());
                MidiPort {
                    id: format!("hw:{c},{d}"),
                    name: format!("{card} MIDI {d}"),
                }
            })
            .collect()
    }

    pub(super) fn inputs() -> Vec<MidiPort> {
        let mut v = ports();
        v.extend(seq::ports(true));
        v
    }

    pub(super) fn outputs() -> Vec<MidiPort> {
        let mut v = ports();
        v.extend(seq::ports(false));
        v
    }

    fn path(port: &MidiPort) -> String {
        let (c, d) = port
            .id
            .strip_prefix("hw:")
            .and_then(|r| r.split_once(','))
            .unwrap_or(("0", "0"));
        format!("/dev/snd/midiC{c}D{d}")
    }

    pub(super) fn open_input(port: &MidiPort, feed: Feed) -> std::io::Result<Box<dyn Send>> {
        if port.id.starts_with("seq:") {
            return seq::open_input(port, feed);
        }
        let f = std::fs::File::open(path(port))?;
        reader_thread(f, feed);
        Ok(Box::new(()))
    }

    pub(super) fn open_virtual_input(name: &str, feed: Feed) -> std::io::Result<Box<dyn Send>> {
        seq::open_virtual_input(name, feed)
    }

    pub(super) fn open_virtual_out(name: &str) -> std::io::Result<Out> {
        Ok(Out::Seq(seq::Out::open_virtual(name)?))
    }

    pub(super) enum Out {
        Raw(std::fs::File),
        Seq(seq::Out),
    }

    impl Out {
        pub(super) fn open(port: &MidiPort) -> std::io::Result<Self> {
            if port.id.starts_with("seq:") {
                Ok(Self::Seq(seq::Out::open(port)?))
            } else {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(path(port))
                    .map(Self::Raw)
            }
        }

        pub(super) fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            match self {
                Self::Raw(f) => {
                    use std::io::Write;
                    f.write_all(bytes)?;
                    f.flush()
                }
                Self::Seq(o) => o.send(bytes),
            }
        }
    }
}

#[cfg(windows)]
#[path = "midi_winmm.rs"]
mod platform;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "midi_coremidi.rs"]
mod platform;

#[cfg(target_os = "android")]
#[path = "midi_amidi.rs"]
mod platform;

#[cfg(all(target_arch = "wasm32", feature = "web-midi"))]
#[path = "midi_web.rs"]
mod platform;

#[cfg(not(any(
    all(target_os = "linux", not(target_os = "android")),
    windows,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    all(target_arch = "wasm32", feature = "web-midi")
)))]
mod platform {
    use super::{Feed, MidiPort};

    /// `Started`'s clock on wasm32 without `web-midi`: nothing opens there,
    /// so nothing reads it, but `std::time::Instant` would panic if it did.
    #[cfg(target_arch = "wasm32")]
    pub(super) unsafe fn clock_ms() -> f64 {
        0.0
    }

    fn unsupported() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no MIDI port API on this platform",
        )
    }

    pub(super) fn inputs() -> Vec<MidiPort> {
        Vec::new()
    }

    pub(super) fn outputs() -> Vec<MidiPort> {
        Vec::new()
    }

    pub(super) fn open_input(_: &MidiPort, _: Feed) -> std::io::Result<Box<dyn Send>> {
        Err(unsupported())
    }

    pub(super) struct Out;

    impl Out {
        pub(super) fn open(_: &MidiPort) -> std::io::Result<Self> {
            Err(unsupported())
        }

        pub(super) fn send(&mut self, _: &[u8]) -> std::io::Result<()> {
            Err(unsupported())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_arrive_timestamped_through_a_pipe() {
        let (reader, writer) = std::io::pipe().unwrap();
        let input = MidiInput::from_reader(reader);
        let mut out = MidiOutput::new(writer);
        out.send(&Message::NoteOn {
            channel: 2,
            key: 64,
            velocity: 99,
        })
        .unwrap();
        out.send(&Message::ControlChange {
            channel: 2,
            controller: 1,
            value: 7,
        })
        .unwrap();
        out.send(&Message::TimingClock).unwrap();
        drop(out);
        let mut got = Vec::new();
        let t0 = std::time::Instant::now();
        while got.len() < 3 && t0.elapsed().as_secs() < 5 {
            got.extend(input.poll());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let msgs: Vec<Message> = got.iter().map(|t| t.message.clone()).collect();
        assert_eq!(
            msgs,
            vec![
                Message::NoteOn {
                    channel: 2,
                    key: 64,
                    velocity: 99
                },
                Message::ControlChange {
                    channel: 2,
                    controller: 1,
                    value: 7
                },
                Message::TimingClock
            ]
        );
        assert!(got.windows(2).all(|w| w[0].time <= w[1].time));
        assert!(input.now() >= got[2].time);
    }

    #[test]
    fn coremidi_packet_lists_walk_in_both_layouts() {
        for align4 in [false, true] {
            // A 4-aligned base, as CoreMIDI's allocations are.
            let mut store = vec![0u32; 16];
            let base = store.as_mut_ptr().cast::<u8>();
            let mut l = 2u32.to_le_bytes().to_vec();
            for data in [&[0x90u8, 60, 100][..], &[0xF8][..]] {
                l.extend_from_slice(&0u64.to_le_bytes());
                l.extend_from_slice(&u16::try_from(data.len()).unwrap().to_le_bytes());
                l.extend_from_slice(data);
                if align4 {
                    while !l.len().is_multiple_of(4) {
                        l.push(0);
                    }
                }
            }
            // SAFETY: `l` fits in `store` (64 bytes).
            unsafe { std::ptr::copy_nonoverlapping(l.as_ptr(), base, l.len()) };
            let mut got = Vec::new();
            // SAFETY: the list just written.
            unsafe { walk_packet_list(base, align4, |b| got.push(b.to_vec())) };
            assert_eq!(
                got,
                vec![vec![0x90, 60, 100], vec![0xF8]],
                "align4 {align4}"
            );
        }
    }

    #[test]
    fn short_message_lengths() {
        assert_eq!(short_len(0x90), 3);
        assert_eq!(short_len(0xC5), 2);
        assert_eq!(short_len(0xD0), 2);
        assert_eq!(short_len(0xF8), 1);
        assert_eq!(short_len(0xF2), 3);
        assert_eq!(short_len(0xF1), 2);
    }

    #[test]
    fn a_port_sink_sends_whole_messages() {
        // Through the pipe-backed `from_reader` path the bytes arrive
        // intact; here the sink's own splitting: two messages written in
        // one go, running status, flushed once.
        let mut p = Parser::new();
        let msgs = p.feed(&[0x90, 60, 100, 62, 100, 0xF8]);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1].to_bytes(), vec![0x90, 62, 100]);
    }

    #[test]
    fn ports_list_and_unknown_ids_are_not_found() {
        for p in input_ports().iter().chain(&output_ports()) {
            assert!(!p.id.is_empty() && !p.name.is_empty());
        }
        let e = MidiInput::open("no such port").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        let e = MidiOutput::open("no such port").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn missing_devices_are_reported_not_faked() {
        if list_rawmidi().contains(&(99, 99)) {
            return;
        }
        assert!(MidiInput::open_rawmidi(99, 99).is_err());
        assert!(MidiOutput::open_rawmidi(99, 99).is_err());
    }

    // ---- the ALSA sequencer bridge's pure half ----------------------------
    //
    // These run everywhere: the mapping between MIDI wire bytes and
    // sequencer events is pure, and `midi_alsa_seq.rs` (where a libasound
    // is installed) holds the numbers — and the FFI half — to the ABI.

    /// Every wire message this model carries, as bytes, for the round trip.
    const SEQ_WIRE: &[&[u8]] = &[
        &[0x80, 60, 0],
        &[0x90, 60, 100],
        &[0x9F, 0, 0],                // note-on velocity 0 (either spelling survives)
        &[0xA5, 61, 20],              // polyphonic key pressure
        &[0xB3, 74, 12],              // control change
        &[0xC7, 99],                  // program change
        &[0xD0, 64],                  // channel pressure
        &[0xE1, 0, 0],                // pitch bend, centered
        &[0xE1, 0x7F, 0x7F],          // pitch bend, top
        &[0xE1, 0x41, 0x00],          // pitch bend, bottom (LSB 7-bit, MSB 0)
        &[0xF0, 0x7D, 1, 2, 3, 0xF7], // system exclusive
        &[0xF1, 0x33],                // quarter frame
        &[0xF2, 0x22, 0x11],          // song position
        &[0xF3, 0x44],                // song select
        &[0xF6],                      // tune request
        &[0xF8],                      // timing clock
        &[0xFA],                      // start
        &[0xFB],                      // continue
        &[0xFC],                      // stop
        &[0xFE],                      // active sensing
        &[0xFF],                      // reset
    ];

    #[test]
    fn seq_events_round_trip_through_the_pure_half() {
        for &wire in SEQ_WIRE {
            let ev = seq_encode(wire).unwrap_or_else(|| panic!("encode {wire:02x?}"));
            // A SysEx rides the variable data, which the caller holds.
            let ext: &[u8] = if ev.type_ == seq_event::SYSEX {
                wire
            } else {
                &[]
            };
            let back = seq_decode(&ev, ext).unwrap_or_else(|| panic!("decode {wire:02x?}"));
            assert_eq!(back.as_slice(), wire, "{wire:02x?}");
        }
    }

    #[test]
    fn seq_events_carry_the_abi_fields() {
        let ev = seq_encode(&[0x92, 64, 99]).unwrap();
        assert_eq!(ev.type_, seq_event::NOTEON);
        assert_eq!(ev.flags, 0);
        assert_eq!(&ev.data[..3], &[2, 64, 99]);
        let ev = seq_encode(&[0xB0, 7, 100]).unwrap();
        assert_eq!(ev.type_, seq_event::CONTROLLER);
        assert_eq!(u32::from_ne_bytes(ev.data[4..8].try_into().unwrap()), 7);
        assert_eq!(u32::from_ne_bytes(ev.data[8..12].try_into().unwrap()), 100);
        let ev = seq_encode(&[0xE0, 0x00, 0x00]).unwrap();
        assert_eq!(ev.type_, seq_event::PITCHBEND);
        // The centered value the event carries: −8192, two's complement.
        assert_eq!(
            i32::from_ne_bytes(ev.data[8..12].try_into().unwrap()),
            -8_192
        );
        let ev = seq_encode(&[0xF0, 1, 2, 0xF7]).unwrap();
        assert_eq!(ev.type_, seq_event::SYSEX);
        assert_eq!(ev.flags, seq_event::LENGTH_VARIABLE);
        assert_eq!(u32::from_ne_bytes(ev.data[0..4].try_into().unwrap()), 4);
    }

    #[test]
    fn seq_decode_refuses_what_the_model_cannot_spell() {
        // The 14-bit and registered-parameter events: channel-voice
        // semantics this model's 7-bit control change does not carry.
        for t in [
            seq_event::CONTROL14,
            seq_event::NONREGPARAM,
            seq_event::REGPARAM,
        ] {
            let ev = SeqEvent {
                type_: t,
                flags: 0,
                data: [0; 12],
            };
            assert!(seq_decode(&ev, &[]).is_none(), "type {t}");
        }
        // The sequencer's own bookkeeping (a subscription made, a port gone).
        for t in [
            seq_event::PORT_SUBSCRIBED,
            seq_event::PORT_UNSUBSCRIBED,
            0,
            255,
        ] {
            let ev = SeqEvent {
                type_: t,
                flags: 0,
                data: [0; 12],
            };
            assert!(seq_decode(&ev, &[]).is_none(), "type {t}");
        }
        // A pitch bend beyond MIDI's range is clamped, not wrapped: a
        // sender that is not MIDI-shaped can hand anything.
        let ev = SeqEvent {
            type_: seq_event::PITCHBEND,
            flags: 0,
            data: {
                let mut d = [0; 12];
                d[8..12].copy_from_slice(&100_000i32.to_ne_bytes());
                d
            },
        };
        assert_eq!(
            seq_decode(&ev, &[]).as_deref(),
            Some(&[0xE0, 0x7F, 0x7F][..])
        );
    }

    #[test]
    fn seq_encode_refuses_truncated_and_unspellable() {
        assert!(seq_encode(&[]).is_none());
        assert!(seq_encode(&[0x90, 60]).is_none()); // truncated
        assert!(seq_encode(&[0xC0]).is_none());
        assert!(seq_encode(&[0xF2, 1]).is_none());
        assert!(seq_encode(&[0xF4]).is_none()); // undefined
        assert!(seq_encode(&[0xF5]).is_none()); // undefined
        assert!(seq_encode(&[0xF7]).is_none()); // bare EOX has no spelling
        assert!(seq_encode(&[0xF0, 1]).is_none()); // unterminated sysex
    }

    // ---- the Web MIDI bridge's pure half ------------------------------------
    //
    // The protocol the `vieww_midi` shim speaks; web/test-midi.mjs holds
    // the JavaScript half of it to the same numbers.

    #[test]
    fn shim_strings_read_short_and_retry_long() {
        // Fits the stack buffer: one read carries it whole.
        let short = |b: &mut [u8]| {
            let s = b"loopback";
            let n = b.len().min(s.len());
            b[..n].copy_from_slice(&s[..n]);
            s.len() as u32
        };
        assert_eq!(shim_string(short), "loopback");
        // Exactly the buffer's size is still the one-read path.
        let fill: Vec<u8> = "a".repeat(128).into_bytes();
        let exact = |b: &mut [u8]| {
            let n = b.len().min(fill.len());
            b[..n].copy_from_slice(&fill[..n]);
            fill.len() as u32
        };
        assert_eq!(shim_string(exact), "a".repeat(128));
        // Longer: the first read truncates but reports the full length,
        // and the exact-size retry carries every byte.
        let long: Vec<u8> = "id-".repeat(100).into_bytes();
        let over = |b: &mut [u8]| {
            let n = b.len().min(long.len());
            b[..n].copy_from_slice(&long[..n]);
            long.len() as u32
        };
        assert_eq!(shim_string(over), "id-".repeat(100));
    }

    #[test]
    fn shim_strings_replace_invalid_utf8() {
        // The bytes are a DOMString the shim encoded — UTF-8 by
        // construction — so this only fires on a shim bug; lossy, never
        // a panic, is the honest failure.
        let lossy = |b: &mut [u8]| {
            let s = [0xFF, 0xFE, b'x'];
            let n = b.len().min(s.len());
            b[..n].copy_from_slice(&s[..n]);
            s.len() as u32
        };
        assert_eq!(shim_string(lossy), "\u{FFFD}\u{FFFD}x");
    }

    #[test]
    fn shim_result_codes_map_to_io_kinds() {
        assert!(shim_check(0, "what").is_ok());
        let kinds = [
            (-1, std::io::ErrorKind::Unsupported),
            (-2, std::io::ErrorKind::NotFound),
            (-3, std::io::ErrorKind::PermissionDenied),
        ];
        for (code, kind) in kinds {
            let e = shim_check(code, "what").unwrap_err();
            assert_eq!(e.kind(), kind, "code {code}");
        }
        // An unknown code is said as the unknown it is.
        let e = shim_check(-9, "what").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::Other);
        assert!(e.to_string().contains("-9"));
    }
}
