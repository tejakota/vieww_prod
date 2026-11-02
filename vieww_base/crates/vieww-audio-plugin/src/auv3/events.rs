//! The AUv3 MIDI schedule: what `scheduleMIDIEventList:` delivers, held
//! until the render clock reaches it.
//!
//! AUv3 hosts schedule MIDI on a v3 unit by handing it a CoreMIDI
//! `MIDIEventList` — the packet list of the Universal MIDI Packet (UMP)
//! format MIDI 2.0 introduced — whose packets carry 64-bit timestamps
//! and 32-bit words. The walk below is
//! [`midi_io`](vieww_audio::midi_io)'s `MIDIPacketList` walk, moved to
//! the newer list: an 8-byte-aligned packet array (a packet is a 64-bit
//! timestamp, a 32-bit word count, then the words), decoded word by
//! word into plain MIDI 1.0 bytes.
//!
//! What is decoded (MIDI 1.0 protocol, the only one the engine speaks):
//!
//! | UMP type | Packet | Decoded to |
//! |---|---|---|
//! | `0x0` utility | 1 word | skipped (no MIDI meaning) |
//! | `0x1` system common / real time | 1 word | the system status byte plus its data |
//! | `0x2` MIDI 1.0 channel voice | 1 word | status, channel, one or two data bytes |
//! | `0x3` system exclusive (7-bit) | 2 words | up to six SysEx bytes, start/continue/end |
//! | `0x4` MIDI 2.0 channel voice | 2 words | **refused by name** — not converted down |
//! | `0x5`, `0xD`–`0xF` data / flex / stream | 3–4 words | skipped |
//!
//! MIDI 2.0 channel voice being refused rather than lossily translated
//! is the honest choice: a translation would have to invent controllers
//! and velocities the performer never sent.
//!
//! The walk is pure code over bytes, so its tests run on every platform;
//! the wrapper module calls it from the `scheduleMIDIEventList:` method
//! the host invokes. The *schedule* itself — [`MidiSchedule`] — is the
//! bounded buffer between that method (whatever thread the host
//! schedules from) and the render block (the real-time thread): a
//! fixed-capacity ring, scheduled with the event's sample time, drained
//! by the render clock, never allocating.

use std::sync::Mutex;

/// Scheduled entries buffered before the render block drains them.
pub const MAX_SCHEDULED: usize = 1_024;

/// Longest system-exclusive message buffered (longer ones are dropped,
/// counted, and reported — never allocated for).
pub const MAX_SYSEX: usize = 512;

/// `AUEventSampleTimeImmediate`: "this event's sample time is now",
/// delivered at the first frame of the next render call. The value is
/// `0xFFFFFFFF00000000` (a host may add a small — under 4096 — frame
/// offset to it, which is why the *window*, not the one value, is what
/// [`is_immediate`] tests).
pub const EVENT_TIME_IMMEDIATE: i64 = 0xFFFF_FFFF_0000_0000u64 as i64;

/// The widest offset a host may add to [`EVENT_TIME_IMMEDIATE`] and
/// still mean "immediately" (the header's own bound).
const IMMEDIATE_WINDOW: i64 = 4_096;

/// Whether `time` is in the immediate window.
#[must_use]
pub const fn is_immediate(time: i64) -> bool {
    time >= EVENT_TIME_IMMEDIATE && time < EVENT_TIME_IMMEDIATE + IMMEDIATE_WINDOW
}

/// One scheduled message: the AU event sample time and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduled {
    /// Sample time on the render timeline (`AUEventSampleTime`).
    pub time: i64,
    /// The bytes (`len` of `data` are used).
    pub len: u16,
    /// Message bytes (MIDI 1.0 wire format, no running status).
    pub data: [u8; MAX_SYSEX],
}

impl Scheduled {
    fn short(time: i64, bytes: &[u8]) -> Self {
        let mut data = [0u8; MAX_SYSEX];
        let len = bytes.len().min(MAX_SYSEX);
        data[..len].copy_from_slice(&bytes[..len]);
        Self {
            time,
            #[expect(clippy::cast_possible_truncation, reason = "capped above")]
            len: len as u16,
            data,
        }
    }

    /// The message's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        let n = usize::from(self.len).min(MAX_SYSEX);
        &self.data[..n]
    }
}

/// The bounded schedule between the host's MIDI calls and the render
/// block.
///
/// Capacity, not allocation: the ring is created once (at
/// `initWithComponentDescription:`, where allocating is legal) and every
/// later push either fits or is dropped and counted — the same contract
/// the CLAP/VST3 wrappers give their event scratch.
#[derive(Debug)]
pub struct MidiSchedule {
    entries: Mutex<Vec<Scheduled>>,
    dropped: std::sync::atomic::AtomicU64,
}

impl MidiSchedule {
    /// A schedule with its ring preallocated.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::with_capacity(MAX_SCHEDULED)),
            dropped: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Buffer one message for `time`. `false` (and a counted drop) if the
    /// ring is full or the message is empty or too long.
    pub fn schedule(&self, time: i64, bytes: &[u8]) -> bool {
        if bytes.is_empty() || bytes.len() > MAX_SYSEX {
            self.dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return false;
        }
        if let Ok(mut ring) = self.entries.lock() {
            if ring.len() < ring.capacity() {
                ring.push(Scheduled::short(time, bytes));
                return true;
            }
        }
        self.dropped
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        false
    }

    /// Drop every buffered event (a reset).
    pub fn clear(&self) {
        if let Ok(mut ring) = self.entries.lock() {
            ring.clear();
        }
    }

    /// Events due within the block starting at `block_start` and
    /// `frame_count` frames long: removed and handed to `visit` as
    /// `(frame within the block, bytes)`; an immediate event lands at
    /// frame 0, a past event clamps there too.
    ///
    /// The render block calls this with its block's start sample time
    /// and frame count. Never allocates: the visitor borrows the ring's
    /// entries, and the removal is an in-place retain.
    pub fn drain_into(
        &self,
        block_start: i64,
        frame_count: u32,
        mut visit: impl FnMut(u32, &[u8]),
    ) {
        let horizon = block_start.saturating_add(i64::from(frame_count));
        let Ok(mut ring) = self.entries.lock() else {
            return;
        };
        // Scheduled events are appended in host order, which is not
        // guaranteed to be time order: sort by time, stable, in place.
        vieww_audio::rt::sort_stable_in_place(&mut ring, |a, b| a.time < b.time);
        for e in ring.iter() {
            if e.time <= horizon || is_immediate(e.time) {
                let at = if is_immediate(e.time) {
                    0
                } else {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "clamped to the block above"
                    )]
                    let off = (e.time - block_start).max(0).min(i64::from(frame_count)) as u32;
                    off
                };
                visit(at, e.bytes());
            }
        }
        ring.retain(|e| e.time > horizon && !is_immediate(e.time));
    }

    /// How many events were dropped (full ring, empty or over-long
    /// message).
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for MidiSchedule {
    fn default() -> Self {
        Self::new()
    }
}

// ── the MIDIEventList walk ─────────────────────────────────────────────────

/// Where the first packet starts: a 32-bit packet count, then padding to
/// the packets' 8-byte alignment.
const LIST_HEAD: usize = 8;
/// Where a packet's words start: a 64-bit timestamp, a 32-bit word count,
/// then the words.
const WORDS_AT: usize = 12;

/// Walk a CoreMIDI `MIDIEventList` and hand each message to `visit` as
/// `(timestamp, group, bytes)`.
///
/// Layout, per `<CoreMIDI/CoreMIDI.h>`: a 32-bit packet count, then the
/// packets, each 8-byte aligned — a 64-bit timestamp, a 32-bit word
/// count, then that many 32-bit UMP words. `group` is the UMP group
/// nibble (the virtual "cable" a v1 packet had).
///
/// Malformed structure (counts that overrun the buffer, a zero word
/// count) stops the walk with the offending offset: the host's list is
/// trusted exactly as far as its own numbers say.
///
/// # Errors
/// `Err(offset)` names the byte offset the walk gave up at.
pub fn walk_event_list(
    bytes: &[u8],
    mut visit: impl FnMut(u64, u8, &[u8]),
) -> Result<(), usize> {
    // A system-exclusive message being assembled across packets of the
    // SAME list (the accumulator lives here because each packet is its
    // own decode call; a walk may allocate — it runs on whatever thread
    // the host schedules from, never the render block).
    let mut sysex: Vec<u8> = Vec::new();
    let read_u32 = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let read_u64 = |at: usize| -> Option<u64> {
        bytes
            .get(at..at + 8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    };
    let Some(n) = read_u32(0) else {
        return Err(0);
    };
    let mut at = LIST_HEAD; // count + padding to the first packet
    for _ in 0..n as usize {
        let time = read_u64(at).ok_or(at)?;
        let words = read_u32(at + 8).ok_or(at)? as usize;
        if words == 0 {
            return Err(at);
        }
        let Some(end) = (at + WORDS_AT).checked_add(words.checked_mul(4).ok_or(at)?) else {
            return Err(at);
        };
        if end > bytes.len() {
            return Err(at);
        }
        decode_packet(time, &bytes[at + WORDS_AT..end], &mut sysex, &mut visit);
        // The next packet starts at the next 8-byte boundary.
        at = end.div_ceil(8) * 8;
    }
    Ok(())
}

/// One byte of a UMP word, at `shift` bits up.
fn byte(w: u32, shift: u32) -> u8 {
    ((w >> shift) & 0xFF) as u8
}

/// Decode one packet's UMP words into MIDI 1.0 bytes. `sysex` is the
/// cross-packet accumulator the walk owns.
fn decode_packet(
    time: u64,
    words: &[u8],
    sysex: &mut Vec<u8>,
    visit: &mut impl FnMut(u64, u8, &[u8]),
) {
    let mut buf = [0u8; 3];
    // Byte 3 of the first word is (type << 4) | group — the group is
    // the LOW nibble (the UMP word is big-endian in its meaning).
    let group = words.get(3).map_or(0, |b| b & 0x0F);
    let mut i = 0;
    while i + 4 <= words.len() {
        let w = u32::from_le_bytes(words[i..i + 4].try_into().unwrap());
        let ty = w >> 28;
        match ty {
            0x1 => {
                // System common / real time: status in bits 16–23.
                buf[0] = byte(w, 16);
                buf[1] = byte(w, 8);
                buf[2] = byte(w, 0);
                let len = match buf[0] {
                    0xF1 | 0xF3 => 2,
                    0xF2 => 3,
                    _ => 1,
                };
                visit(time, group, &buf[..len]);
            }
            0x2 => {
                // MIDI 1.0 channel voice: byte 1 (bits 16–23) is the
                // whole status byte — status nibble and channel — and
                // the two data bytes follow in the low half-word.
                buf[0] = byte(w, 16);
                buf[1] = byte(w, 8);
                buf[2] = byte(w, 0);
                let len = match buf[0] & 0xF0 {
                    0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 3,
                    _ => 2,
                };
                visit(time, group, &buf[..len]);
            }
            0x3 => {
                // SysEx7, a two-word packet: byte 1's nibbles are the
                // packet kind (0 complete, 1 start, 2 continue, 3 end)
                // and the count of valid data bytes (0..=6) this packet
                // carries; bytes 2–7 are those bytes. The count nibble is
                // the UMP specification's own: it is what makes a
                // short message know where it ends.
                let b1 = byte(w, 16);
                let kind = b1 >> 4;
                let count = usize::from(b1 & 0x0F).min(6);
                let mut data = [0u8; 6];
                data[0] = byte(w, 8);
                data[1] = byte(w, 0);
                let have_w1 = i + 8 <= words.len();
                if have_w1 {
                    let w1 = u32::from_le_bytes(words[i + 4..i + 8].try_into().unwrap());
                    data[2] = byte(w1, 24);
                    data[3] = byte(w1, 16);
                    data[4] = byte(w1, 8);
                    data[5] = byte(w1, 0);
                }
                let n = count.min(if have_w1 { 6 } else { 2 });
                match kind {
                    0 => {
                        let mut whole = vec![0xF0];
                        whole.extend_from_slice(&data[..n]);
                        whole.push(0xF7);
                        visit(time, group, &whole);
                    }
                    1 | 2 => {
                        if kind == 1 {
                            sysex.push(0xF0);
                        }
                        sysex.extend_from_slice(&data[..n]);
                    }
                    _ => {
                        sysex.extend_from_slice(&data[..n]);
                        sysex.push(0xF7);
                        visit(time, group, sysex);
                        sysex.clear();
                    }
                }
                i += 4; // the data word, consumed here
            }
            // Utility (0x0), MIDI 2.0 channel voice (0x4, refused by
            // name in the module doc), and the 3–4-word data classes:
            // no MIDI 1.0 to decode.
            _ => {}
        }
        i += 4;
    }
}

/// Encode messages as a `MIDIEventList` — the tests' oracle and the
/// mirror of the walk.
///
/// Each `(timestamp, group, bytes)` message becomes one packet: channel
/// voice and system messages as single type-`0x2`/`0x1` words,
/// system-exclusive messages as `0x3` packets of six bytes (the classic
/// F0 … F7 envelope stripped by the encoder and re-added by the packet
/// kinds; a message whose body fits one packet is marked complete).
#[must_use]
pub fn encode_event_list(messages: &[(u64, u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u32.to_le_bytes()); // packet count, fixed below
    let mut packets = 0u32;
    for &(time, group, bytes) in messages {
        let first = *bytes.first().unwrap_or(&0);
        if first == 0xF0 {
            // Strip the F0/F7 envelope; chunk the body into six bytes.
            let body: Vec<u8> = bytes
                .iter()
                .copied()
                .filter(|&b| b != 0xF0 && b != 0xF7)
                .collect();
            for (i, chunk) in body.chunks(6).enumerate() {
                let mut data = [0u8; 6];
                data[..chunk.len()].copy_from_slice(chunk);
                let kind = if body.len() <= 6 {
                    0u32 // complete in one packet
                } else if i == 0 {
                    1u32 // start
                } else {
                    let end = (i + 1) * 6;
                    if end >= body.len() {
                        3u32 // end
                    } else {
                        2u32 // continue
                    }
                };
                let w0 = (0x3 << 28)
                    | (u32::from(group) << 24)
                    | ((kind << 4) | u32::try_from(chunk.len()).unwrap_or(6)) << 16
                    | (u32::from(data[0]) << 8)
                    | u32::from(data[1]);
                let w1 = (u32::from(data[2]) << 24)
                    | (u32::from(data[3]) << 16)
                    | (u32::from(data[4]) << 8)
                    | u32::from(data[5]);
                push_packet(&mut out, &mut packets, time, &[w0, w1]);
            }
        } else if first >= 0xF0 {
            let mut w = (0x1 << 28) | (u32::from(group) << 24);
            w |= u32::from(first) << 16;
            if let Some(&b) = bytes.get(1) {
                w |= u32::from(b) << 8;
            }
            if let Some(&b) = bytes.get(2) {
                w |= u32::from(b);
            }
            push_packet(&mut out, &mut packets, time, &[w]);
        } else {
            // Channel voice: byte 1 (bits 16–23) carries the whole
            // status byte.
            let mut w =
                (0x2 << 28) | (u32::from(group) << 24) | (u32::from(first) << 16);
            if let Some(&b) = bytes.get(1) {
                w |= u32::from(b) << 8;
            }
            if let Some(&b) = bytes.get(2) {
                w |= u32::from(b);
            }
            push_packet(&mut out, &mut packets, time, &[w]);
        }
    }
    out[..4].copy_from_slice(&packets.to_le_bytes());
    out
}

/// Append one packet (its words), 8-byte aligned.
fn push_packet(out: &mut Vec<u8>, packets: &mut u32, time: u64, words: &[u32]) {
    while !out.len().is_multiple_of(8) {
        out.push(0); // pad to the packet alignment
    }
    out.extend_from_slice(&time.to_le_bytes());
    let count = u32::try_from(words.len()).unwrap_or(0);
    out.extend_from_slice(&count.to_le_bytes());
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    *packets += 1;
}

// ── the realtime render-event list ─────────────────────────────────────────
//
// `AUInternalRenderBlock`'s `realtimeEventListHead`: the time-ordered
// linked list through which the base class delivers everything a host
// scheduled — parameter changes (immediate and ramped) and MIDI — for
// this render cycle. Transcribed from AudioToolbox's public
// `AUAudioUnitImplementation.h`:
//
// ```c
// typedef NS_ENUM(uint8_t, AURenderEventType) {
//     AURenderEventParameter     = 1,
//     AURenderEventParameterRamp = 2,
//     AURenderEventMIDI          = 8,
//     AURenderEventMIDISysEx     = 9
// };
// #pragma pack(4)
// typedef struct AURenderEventHeader {
//     union AURenderEvent *next;      // the next event, or null
//     AUEventSampleTime   eventSampleTime;
//     AURenderEventType   eventType;
//     uint8_t             reserved;   // must be 0
// } AURenderEventHeader;
// typedef struct AUParameterEvent {   // types 1, 2
//     ... same head, reserved[3], rampDurationSampleFrames,
//     parameterAddress, value
// } AUParameterEvent;
// typedef struct AUMIDIEvent {        // types 8, 9
//     ... same head, uint16_t length, uint8_t cable, uint8_t data[3]
// } AUMIDIEvent;
// typedef union AURenderEvent { AURenderEventHeader head;
//     AUParameterEvent parameter; AUMIDIEvent MIDI; } AURenderEvent;
// ```
//
// The three are modelled as three `#[repr(C)]` structs that each begin
// with the head's fields — `repr(C)` guarantees identical leading-field
// offsets, so one pointer casts between the views exactly as the C union
// does. The header's `#pragma pack(4)` changes only the *total* sizes
// (20/24/36 packed vs 24/24/40 natural), never a field offset on arm64;
// the walk follows `next` and reads by field, so it depends on no total
// size — a test pins every offset that matters.

/// `AURenderEventParameter` / `AURenderEventParameterRamp`.
pub const EVENT_PARAMETER: u8 = 1;
pub const EVENT_PARAMETER_RAMP: u8 = 2;
/// `AURenderEventMIDI` (and SysEx, which is type 9 — same layout, longer
/// `length`).
pub const EVENT_MIDI: u8 = 8;
pub const EVENT_MIDI_SYSEX: u8 = 9;

/// The common head of every render-event variant.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case, reason = "the header's own field names")]
pub struct RenderEventHeader {
    /// The next event in the list, or null.
    pub next: *const RenderEventHeader,
    /// The sample time at which the event is scheduled.
    pub eventSampleTime: i64,
    /// One of the `EVENT_*` types.
    pub eventType: u8,
    /// Must be zero; never read.
    pub reserved: u8,
}

/// The MIDI variant: types [`EVENT_MIDI`] and [`EVENT_MIDI_SYSEX`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case, reason = "the header's own field names")]
pub struct MidiRenderEvent {
    /// As the header.
    pub next: *const RenderEventHeader,
    /// As the header.
    pub eventSampleTime: i64,
    /// As the header.
    pub eventType: u8,
    /// As the header.
    pub reserved: u8,
    /// Valid MIDI bytes in `data` — 1, 2, 3, or more for SysEx.
    pub length: u16,
    /// The virtual cable.
    pub cable: u8,
    /// The first three bytes; a longer SysEx event's remaining bytes
    /// follow in the event's own memory.
    pub data: [u8; 3],
}

/// The parameter variant: types [`EVENT_PARAMETER`] and
/// [`EVENT_PARAMETER_RAMP`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case, reason = "the header's own field names")]
pub struct ParameterRenderEvent {
    /// As the header.
    pub next: *const RenderEventHeader,
    /// As the header.
    pub eventSampleTime: i64,
    /// As the header.
    pub eventType: u8,
    /// The header's `reserved[3]`; never read.
    pub reserved: [u8; 3],
    /// Ramp length in frames (0: immediate).
    pub rampDurationSampleFrames: u32,
    /// The parameter's address.
    pub parameterAddress: u64,
    /// The value (the end value, if ramped).
    pub value: f32,
}

/// One decoded render event, handed to [`walk_render_events`]' visitor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderEventKind {
    /// A parameter change (`rampDurationSampleFrames` is carried for the
    /// caller that wants ramps; the immediate form applies at the event's
    /// frame).
    Parameter {
        /// The parameter's address.
        address: u64,
        /// The value.
        value: f32,
        /// Ramp length in frames (0: immediate).
        ramp: u32,
    },
    /// A MIDI message: the cable and `length` bytes (`length` may exceed
    /// 3 for SysEx — the visitor's slice is those bytes, copied).
    Midi {
        /// The virtual cable.
        cable: u8,
    },
}

/// Walk the realtime event list, handing `(sample time, kind, bytes)` to
/// `visit` in list order (the list is time-ordered by contract).
///
/// Parameter events deliver an empty byte slice; MIDI events deliver
/// their `length` bytes (SysEx bytes beyond the struct's three are read
/// from the event's own memory, capped at [`MAX_SYSEX`]; the cap is the
/// same bound the schedule imposes). Unknown event types are skipped,
/// not guessed at. The walk never allocates: SysEx bytes land in a stack
/// buffer.
///
/// `reserved` fields are never read; `next` is followed until null.
///
/// # Safety
/// `head` is null or a valid event whose `next` chain stays valid for
/// the walk; every MIDI event's `length` bytes are readable at its
/// `data` field.
pub unsafe fn walk_render_events(
    mut head: *const RenderEventHeader,
    mut visit: impl FnMut(i64, RenderEventKind, &[u8]),
) {
    let mut sysex = [0u8; MAX_SYSEX];
    while !head.is_null() {
        // SAFETY: the caller's contract: a valid event.
        let ev = unsafe { &*head };
        match ev.eventType {
            EVENT_MIDI | EVENT_MIDI_SYSEX => {
                // SAFETY: the same memory viewed as the MIDI variant —
                // repr(C) puts every field of the head at the same
                // offset in both.
                let m = unsafe { &*(head.cast::<MidiRenderEvent>()) };
                let n = usize::from(m.length);
                let bytes: &[u8] = if n <= m.data.len() {
                    &m.data[..n]
                } else {
                    let take = n.min(MAX_SYSEX);
                    // SAFETY: `length` valid bytes at `data` — the
                    // header's own contract for SysEx events.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            m.data.as_ptr(),
                            sysex.as_mut_ptr(),
                            take,
                        );
                    }
                    &sysex[..take]
                };
                visit(
                    m.eventSampleTime,
                    RenderEventKind::Midi {
                        cable: m.cable,
                    },
                    bytes,
                );
            }
            EVENT_PARAMETER | EVENT_PARAMETER_RAMP => {
                // SAFETY: the same memory viewed as the parameter
                // variant.
                let p = unsafe { &*(head.cast::<ParameterRenderEvent>()) };
                visit(
                    p.eventSampleTime,
                    RenderEventKind::Parameter {
                        address: p.parameterAddress,
                        value: p.value,
                        ramp: p.rampDurationSampleFrames,
                    },
                    &[],
                );
            }
            // The header defines no other types today; an unknown one is
            // skipped rather than guessed at.
            _ => {}
        }
        head = ev.next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(bytes: &[u8]) -> Vec<(u64, u8, Vec<u8>)> {
        let mut got = Vec::new();
        walk_event_list(bytes, |t, g, b| got.push((t, g, b.to_vec())))
            .expect("a well-formed list");
        got
    }

    #[test]
    fn channel_voice_round_trips() {
        let messages: Vec<(u64, u8, &[u8])> = vec![
            (100, 0, &[0x90, 60, 100]),
            (100, 2, &[0x80, 61, 0]),
            (120, 1, &[0xB3, 74, 12]),
            (130, 0, &[0xC5, 9]),
            (140, 0, &[0xD2, 70]),
            (150, 0, &[0xE1, 0, 96]),
        ];
        let bytes = encode_event_list(&messages);
        let got = collect(&bytes);
        assert_eq!(got.len(), messages.len());
        for (got, want) in got.iter().zip(&messages) {
            assert_eq!((got.0, got.1, got.2.as_slice()), (want.0, want.1, want.2));
        }
    }

    #[test]
    fn system_common_round_trips() {
        let messages: Vec<(u64, u8, &[u8])> = vec![
            (10, 0, &[0xF8]),
            (11, 0, &[0xF1, 7]),
            (12, 0, &[0xF2, 1, 2]),
        ];
        let bytes = encode_event_list(&messages);
        assert_eq!(
            collect(&bytes),
            messages
                .iter()
                .map(|&(t, g, b)| (t, g, b.to_vec()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_short_sysex_is_one_complete_packet() {
        let msg = [0xF0, 0x7D, 1, 2, 0xF7];
        let bytes = encode_event_list(&[(5, 3, &msg[..])]);
        let got = collect(&bytes);
        assert_eq!(got, vec![(5, 3, msg.to_vec())]);
    }

    #[test]
    fn a_long_sysex_spans_start_continue_end_packets() {
        // 14 payload bytes: three packets (6 + 6 + 2).
        let mut msg = vec![0xF0, 0x7D];
        msg.extend_from_slice(&[7u8; 14]);
        msg.push(0xF7);
        let bytes = encode_event_list(&[(5, 3, &msg)]);
        let got = collect(&bytes);
        assert_eq!(got.len(), 1, "one assembled message");
        assert_eq!((got[0].0, got[0].1), (5, 3));
        assert_eq!(got[0].2, msg);
    }

    #[test]
    fn zero_word_counts_and_overruns_are_refused() {
        // count says one packet, but nothing follows.
        assert!(walk_event_list(&1u32.to_le_bytes(), |_, _, _| {}).is_err());
        // A packet whose words run past the buffer.
        let mut bytes = vec![0u8; 16];
        bytes[..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&100u32.to_le_bytes());
        assert!(walk_event_list(&bytes, |_, _, _| {}).is_err());
        // A zero word count.
        let mut zero = vec![0u8; 32];
        zero[..4].copy_from_slice(&1u32.to_le_bytes());
        assert!(walk_event_list(&zero, |_, _, _| {}).is_err());
    }

    #[test]
    fn the_schedule_delivers_due_events_in_time_order() {
        let s = MidiSchedule::new();
        assert!(s.schedule(1_000, &[0x90, 60, 100]));
        assert!(s.schedule(1_050, &[0xB2, 7, 64]));
        assert!(s.schedule(1_020, &[0x80, 60, 0]));
        // Not due yet.
        assert!(s.schedule(2_000, &[0xF8]));
        let mut got = Vec::new();
        s.drain_into(1_000, 128, |at, b| got.push((at, b.to_vec())));
        assert_eq!(
            got,
            vec![
                (0, vec![0x90, 60, 100]),
                (20, vec![0x80, 60, 0]),
                (50, vec![0xB2, 7, 64])
            ],
            "time-sorted, offsets within the block"
        );
        // The future event stays for its block.
        let mut later = Vec::new();
        s.drain_into(1_990, 64, |at, b| later.push((at, b.to_vec())));
        assert_eq!(later, vec![(10, vec![0xF8])]);
        assert_eq!(s.dropped(), 0);
    }

    #[test]
    fn immediate_events_land_at_frame_zero_and_past_events_clamp() {
        let s = MidiSchedule::new();
        assert!(s.schedule(EVENT_TIME_IMMEDIATE, &[0xFA]));
        assert!(s.schedule(0, &[0x90, 1, 1]));
        let mut got = Vec::new();
        s.drain_into(5_000, 32, |at, b| got.push((at, b.to_vec())));
        assert_eq!(got, vec![(0, vec![0xFA]), (0, vec![0x90, 1, 1])]);
    }

    #[test]
    fn a_full_or_overlong_schedule_drops_and_counts() {
        let s = MidiSchedule::new();
        for i in 0..MAX_SCHEDULED {
            let t = i64::try_from(i).unwrap_or(0);
            assert!(s.schedule(t, &[0xF8]));
        }
        assert!(!s.schedule(9_999, &[0xF8]), "the ring is full");
        assert!(!s.schedule(9_999, &vec![0u8; MAX_SYSEX + 1]), "too long");
        assert!(!s.schedule(9_999, &[]), "empty");
        assert_eq!(s.dropped(), 3);
    }

    #[test]
    fn clear_empties_the_ring() {
        let s = MidiSchedule::new();
        assert!(s.schedule(1, &[0xF8]));
        s.clear();
        let mut got = 0;
        s.drain_into(0, 64, |_, _| got += 1);
        assert_eq!(got, 0);
    }

    #[test]
    fn the_immediate_constant_is_the_headers_and_forms_a_window() {
        // AUAudioUnit.h: AUEventSampleTimeImmediate = 0xffffffff00000000.
        assert_eq!(EVENT_TIME_IMMEDIATE, -0x1_0000_0000);
        assert_eq!(
            EVENT_TIME_IMMEDIATE,
            0xFFFF_FFFF_0000_0000u64 as i64
        );
        // The constant itself and the documented sub-4096 offsets are
        // immediate; a true sample time never is.
        assert!(is_immediate(EVENT_TIME_IMMEDIATE));
        assert!(is_immediate(EVENT_TIME_IMMEDIATE + 4_095));
        assert!(!is_immediate(EVENT_TIME_IMMEDIATE + 4_096));
        assert!(!is_immediate(0));
        assert!(!is_immediate(-1));
    }

    #[test]
    fn an_immediate_window_event_lands_at_frame_zero() {
        let s = MidiSchedule::new();
        assert!(s.schedule(EVENT_TIME_IMMEDIATE + 64, &[0xFB]));
        let mut got = Vec::new();
        s.drain_into(9_000, 128, |at, b| got.push((at, b.to_vec())));
        assert_eq!(got, vec![(0, vec![0xFB])]);
    }

    #[test]
    fn render_event_layout_offsets_match_the_header() {
        // The three views share the head's offsets (repr(C) guarantees
        // it for leading fields; the test pins it so a refactor cannot
        // silently drift from AUAudioUnitImplementation.h).
        assert_eq!(
            std::mem::offset_of!(RenderEventHeader, next),
            std::mem::offset_of!(MidiRenderEvent, next)
        );
        assert_eq!(
            std::mem::offset_of!(RenderEventHeader, eventSampleTime),
            std::mem::offset_of!(MidiRenderEvent, eventSampleTime)
        );
        assert_eq!(
            std::mem::offset_of!(RenderEventHeader, eventType),
            std::mem::offset_of!(MidiRenderEvent, eventType)
        );
        assert_eq!(
            std::mem::offset_of!(RenderEventHeader, eventSampleTime),
            std::mem::offset_of!(ParameterRenderEvent, eventSampleTime)
        );
        assert_eq!(
            std::mem::offset_of!(RenderEventHeader, eventType),
            std::mem::offset_of!(ParameterRenderEvent, eventType)
        );
        // The offsets the walk (and the header's pack(4)) imply:
        assert_eq!(std::mem::offset_of!(RenderEventHeader, next), 0);
        assert_eq!(std::mem::offset_of!(RenderEventHeader, eventSampleTime), 8);
        assert_eq!(std::mem::offset_of!(RenderEventHeader, eventType), 16);
        assert_eq!(std::mem::offset_of!(MidiRenderEvent, length), 18);
        assert_eq!(std::mem::offset_of!(MidiRenderEvent, cable), 20);
        assert_eq!(std::mem::offset_of!(MidiRenderEvent, data), 21);
        assert_eq!(
            std::mem::offset_of!(ParameterRenderEvent, rampDurationSampleFrames),
            20
        );
        assert_eq!(
            std::mem::offset_of!(ParameterRenderEvent, parameterAddress),
            24
        );
        assert_eq!(std::mem::offset_of!(ParameterRenderEvent, value), 32);
    }

    #[test]
    fn the_render_event_walk_delivers_midi_parameters_and_sysex() {
        // A three-event list: a note, a parameter change, a 5-byte SysEx
        // (whose two extra bytes live past `data[3]`, in the event's own
        // memory — a 32-byte backing buffer per event, as a host's
        // allocator would).
        let mk = |ty: u8, time: i64| -> Box<[u8; 64]> {
            let mut b = Box::new([0u8; 64]);
            let h = b.as_mut_ptr().cast::<RenderEventHeader>();
            unsafe {
                (*h).eventSampleTime = time;
                (*h).eventType = ty;
            }
            b
        };
        let note = mk(EVENT_MIDI, 100);
        let param = mk(EVENT_PARAMETER, 120);
        let mut sysex = mk(EVENT_MIDI_SYSEX, 140);

        // Wire the list: note → param → sysex → null.
        let link = |b: &[u8; 64], next: *const RenderEventHeader| {
            // SAFETY: our own boxed event, valid for this test.
            unsafe {
                (*b.as_ptr().cast::<RenderEventHeader>().cast_mut()).next = next;
            }
        };
        // SAFETY: the boxes outlive the walk below.
        unsafe {
            link(&note, param.as_ptr().cast());
            link(&param, sysex.as_ptr().cast());
            link(&sysex, std::ptr::null());

            // Fill the variants.
            let n = note.as_ptr().cast::<MidiRenderEvent>().cast_mut();
            (*n).length = 3;
            (*n).cable = 2;
            (*n).data = [0x90, 60, 100];
            let p = param.as_ptr().cast::<ParameterRenderEvent>().cast_mut();
            (*p).parameterAddress = 7;
            (*p).value = 0.5;
            (*p).rampDurationSampleFrames = 0;
            let s = sysex.as_ptr().cast::<MidiRenderEvent>().cast_mut();
            (*s).length = 5;
            (*s).cable = 0;
            (*s).data = [0xF0, 0x7D, 1];
            // The SysEx tail: two more bytes in the event's own memory,
            // past `data[3]` (offsets 24, 25 of the backing buffer).
            sysex[24] = 2;
            sysex[25] = 0xF7;

            let mut got = Vec::new();
            walk_render_events(note.as_ptr().cast(), |t, k, b| {
                got.push((t, k, b.to_vec()));
            });
            assert_eq!(
                got,
                vec![
                    (
                        100,
                        RenderEventKind::Midi { cable: 2 },
                        vec![0x90, 60, 100]
                    ),
                    (
                        120,
                        RenderEventKind::Parameter {
                            address: 7,
                            value: 0.5,
                            ramp: 0
                        },
                        Vec::new()
                    ),
                    (
                        140,
                        RenderEventKind::Midi { cable: 0 },
                        vec![0xF0, 0x7D, 1, 2, 0xF7]
                    ),
                ]
            );
        }
    }

    #[test]
    fn an_empty_render_event_list_delivers_nothing() {
        // SAFETY: a null head is the documented empty list.
        unsafe {
            let mut got = 0;
            walk_render_events(std::ptr::null(), |_, _, _| got += 1);
            assert_eq!(got, 0);
        }
    }

    #[test]
    fn unknown_render_event_types_are_skipped_not_guessed() {
        let mut b = Box::new([0u8; 64]);
        let h = b.as_mut_ptr().cast::<RenderEventHeader>();
        // SAFETY: our own event, wired to a null next.
        unsafe {
            (*h).eventType = 0x55; // no such type in the header
            (*h).next = std::ptr::null();
            let mut got = 0;
            walk_render_events(h, |_, _, _| got += 1);
            assert_eq!(got, 0);
        }
    }
}
