//! AMidi: the NDK side of Android's `android.media.midi` (API 29+).
//!
//! Java owns everything user-visible here: `MidiManager` enumerates the
//! devices (USB adapters, BLE keyboards, virtual ports other apps
//! publish), `MidiManager.openDevice` opens one, and
//! `MidiDevice.getBinder()` hands out the `IBinder` that is the native
//! door to it. That binder — a JNI `jobject`, a global reference the
//! Java side keeps alive — is what [`AmidiDevice::from_binder`] wraps,
//! through `AMidiDevice_fromBinder`. There is no port list to `open` by
//! id on this side, which is why [`inputs`](super::input_ports) and
//! [`outputs`](super::output_ports) stay empty and the openings take a
//! device and a port number ([`super::MidiInput::open_amidi`],
//! [`super::MidiOutput::open_amidi`]).
//!
//! The NDK's naming is the device's, not ours: an AMidi *input* port is
//! one the device receives from — our [`Out`] sends into it with
//! `AMidiInputPort_send` — and an AMidi *output* port is one the device
//! sends from — our reader thread blocks in
//! `AMidiOutputPort_receive`, timestamping arrivals against the input's
//! own clock like every other input. Teardown closes the port and lets
//! a blocked receive return its error: the thread owns its [`Feed`] and
//! exits on it, so nothing is freed under it.
//!
//! `libmediandk.so` is loaded at run time through the same
//! [`Library`](crate::backend::dl::Library) the audio backends use (an
//! APK built for an older API level still starts, and reports the API
//! missing — [`AmidiDevice::available`] is that report, which the
//! Device Farm's audio step prints from a real phone).
//!
//! **What is verified where.** The signatures are `<AMidi/AMidi.h>`'s,
//! transcribed as the other FFI halves are; the crate compiles for
//! `aarch64-linux-android` in CI (the audio-backends `targets` leg).
//! It has never run on a real device from this sandbox — no MIDI
//! adapter, no `MidiManager` — and says so. The farm's audio step is
//! the honest receipt for the library half (`amidi_symbols`: the
//! library loads and the symbols resolve on a real phone); the byte
//! round trip needs a farm phone with a MIDI gadget, which the farm
//! does not list today.

use std::ffi::{c_int, c_void, CStr};
use std::sync::OnceLock;

use super::{Feed, MidiPort};

type Status = c_int; // media_status_t: AMEDIA_OK == 0
const AMEDIA_OK: Status = 0;
/// `AMIDI_OPCODE_DATA`: the receive carried bytes. (`SETUP` and `FLUSH`
/// carry none.)
const OPCODE_DATA: i32 = 1;
/// A receive buffer the largest documented MIDI packet fits in; a
/// longer SysEx arrives fragmented, which the running-status
/// [`Parser`](crate::midi::Parser) reassembles across feeds.
const RECV_BUF: usize = 1024;

/// The NDK's opaque handles: pointers to structs the header never
/// defines — `*mut c_void` with a name each, as `codec::platform` does
/// for `AMediaCodec`.
type DeviceHandle = *mut c_void;
type InputPortHandle = *mut c_void;
type OutputPortHandle = *mut c_void;
/// A JNI object reference — the `IBinder` the Java side handed over.
type JObject = *mut c_void;

/// The symbols this module needs, resolved once.
struct Api {
    _lib: crate::backend::dl::Library,
    device_from_binder: unsafe extern "C" fn(JObject, *mut DeviceHandle) -> Status,
    device_release: unsafe extern "C" fn(DeviceHandle),
    device_num_input_ports: unsafe extern "C" fn(DeviceHandle) -> c_int,
    device_num_output_ports: unsafe extern "C" fn(DeviceHandle) -> c_int,
    input_port_open: unsafe extern "C" fn(DeviceHandle, c_int, *mut InputPortHandle) -> Status,
    input_port_close: unsafe extern "C" fn(InputPortHandle),
    input_port_send: unsafe extern "C" fn(InputPortHandle, *const u8, isize) -> Status,
    output_port_open: unsafe extern "C" fn(DeviceHandle, c_int, *mut OutputPortHandle) -> Status,
    output_port_close: unsafe extern "C" fn(OutputPortHandle),
    output_port_receive: unsafe extern "C" fn(
        OutputPortHandle,
        *mut c_int,
        *mut u8,
        usize,
        *mut usize,
        *mut i64,
    ) -> isize,
}

impl Api {
    fn load() -> Result<&'static Self, String> {
        static API: OnceLock<Result<Api, String>> = OnceLock::new();
        API.get_or_init(|| {
            let lib = crate::backend::dl::Library::open(&["libmediandk.so"])?;
            macro_rules! sym {
                ($name:literal) => {
                    lib.get(CStr::from_bytes_with_nul_unchecked(
                        concat!($name, "\0").as_bytes(),
                    ))?
                };
            }
            // SAFETY: every symbol is looked up by its exact NDK name and
            // transmuted to its exact NDK type, as `dl.rs` does for the
            // audio backends; this runs once.
            unsafe {
                Ok(Api {
                    device_from_binder: sym!("AMidiDevice_fromBinder"),
                    device_release: sym!("AMidiDevice_release"),
                    device_num_input_ports: sym!("AMidiDevice_getNumInputPorts"),
                    device_num_output_ports: sym!("AMidiDevice_getNumOutputPorts"),
                    input_port_open: sym!("AMidiInputPort_open"),
                    input_port_close: sym!("AMidiInputPort_close"),
                    input_port_send: sym!("AMidiInputPort_send"),
                    output_port_open: sym!("AMidiOutputPort_open"),
                    output_port_close: sym!("AMidiOutputPort_close"),
                    output_port_receive: sym!("AMidiOutputPort_receive"),
                    _lib: lib,
                })
            }
        })
        .as_ref()
        .map_err(Clone::clone)
    }
}

fn check(r: Status, what: &str) -> std::io::Result<()> {
    if r == AMEDIA_OK {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{what} failed (media_status_t {r})"
        )))
    }
}

fn unsupported() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Android MIDI devices are enumerated by Java (MidiManager); open the \
         device it holds through MidiInput::open_amidi / MidiOutput::open_amidi",
    )
}

/// An AMidi device: the NDK handle on the `IBinder` a Java
/// `MidiDevice.getBinder()` returns, kept valid by the Java side for as
/// long as the device is used here.
///
/// Inputs and outputs open on it by number (0-based, in the order the
/// Java `MidiDeviceInfo` lists them). Dropping it releases the native
/// handle — the Java `MidiDevice` is the owner of the underlying
/// service and stays open until the Java side closes it.
pub struct AmidiDevice {
    api: &'static Api,
    device: DeviceHandle,
}

// SAFETY: the NDK's device functions take the handle by value from any
// thread (they are `const` in the header); the ports opened from it are
// used by one thread each.
unsafe impl Send for AmidiDevice {}

impl std::fmt::Debug for AmidiDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The handle's address; the NDK type is opaque.
        f.debug_struct("AmidiDevice")
            .field("device", &(self.device as usize))
            .finish_non_exhaustive()
    }
}

impl AmidiDevice {
    /// Wrap the `IBinder` a Java `MidiDevice.getBinder()` returned. The
    /// reference must be a JNI **global** one (or the caller must keep
    /// the Java object alive some other way): the NDK keeps using the
    /// binder through this handle.
    ///
    /// # Safety
    /// `binder` must be a valid JNI object reference — global, or
    /// otherwise held for as long as this device is — pointing at the
    /// `IBinder` of an open Java `MidiDevice`.
    ///
    /// # Errors
    /// No `libmediandk.so` or missing `AMidi*` symbols (API 29+), or
    /// the binder is not a MIDI device's.
    pub unsafe fn from_binder(binder: *mut c_void) -> std::io::Result<Self> {
        let api = Api::load().map_err(std::io::Error::other)?;
        let mut device: DeviceHandle = std::ptr::null_mut();
        // SAFETY: the caller's JNI reference per the contract above;
        // `device` is ours.
        check(
            unsafe { (api.device_from_binder)(binder, &raw mut device) },
            "AMidiDevice_fromBinder",
        )?;
        if device.is_null() {
            return Err(std::io::Error::other(
                "AMidiDevice_fromBinder gave no device",
            ));
        }
        Ok(Self { api, device })
    }

    /// Whether the AMidi API is usable here: `libmediandk.so` loads and
    /// its symbols resolve — Android 10 (API 29) or newer. `Err` says
    /// why, for the log; the Device Farm's audio step prints it from a
    /// real phone.
    pub fn available() -> Result<(), String> {
        Api::load().map(|_| ())
    }

    /// The device's input ports — ports it *receives* from, so the
    /// number an [`Out`] opens. (What the Java `MidiDeviceInfo` calls
    /// the device's input port count.)
    fn num_input_ports(&self) -> c_int {
        // SAFETY: the device handle we hold.
        unsafe { (self.api.device_num_input_ports)(self.device) }
    }

    /// The device's output ports — ports it *sends* from, so the number
    /// an input opens.
    fn num_output_ports(&self) -> c_int {
        // SAFETY: the device handle we hold.
        unsafe { (self.api.device_num_output_ports)(self.device) }
    }
}

impl Drop for AmidiDevice {
    fn drop(&mut self) {
        // SAFETY: the handle `from_binder` gave us, released once.
        unsafe { (self.api.device_release)(self.device) };
    }
}

// ---- the platform module midi_io expects ------------------------------------

pub(super) fn inputs() -> Vec<MidiPort> {
    // Java owns enumeration (the module docs); an empty list is the
    // honest answer, not a missing one.
    Vec::new()
}

pub(super) fn outputs() -> Vec<MidiPort> {
    Vec::new()
}

pub(super) fn open_input(_port: &MidiPort, _feed: Feed) -> std::io::Result<Box<dyn Send>> {
    Err(unsupported())
}

/// An open AMidi output port — the *device's* output, our input — and
/// the detached reader thread that turns its bytes into messages. The
/// thread owns the feed; closing the port makes its blocked receive
/// return, and it exits on the error without touching anything freed.
struct Input {
    api: &'static Api,
    port: OutputPortHandle,
}

// SAFETY: the port handle is used by the reader thread only, which
// outlives this box but owns nothing it does not (the feed moved into
// it); `drop` closes the handle, which ends the reader's receive.
unsafe impl Send for Input {}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: the port we opened. This is what wakes a blocked
        // `AMidiOutputPort_receive` — the NDK documents the close as the
        // way to stop a reader.
        unsafe { (self.api.output_port_close)(self.port) };
    }
}

pub(super) fn open_amidi_input(
    device: &AmidiDevice,
    port: usize,
    feed: Feed,
) -> std::io::Result<Box<dyn Send>> {
    let n = device.num_output_ports();
    let port = i32::try_from(port)
        .map_err(|_| std::io::Error::other(format!("output port {port} is not a port number")))?;
    if port >= n {
        return Err(std::io::Error::other(format!(
            "output port {port} is out of range (the device has {n})"
        )));
    }
    let mut handle: OutputPortHandle = std::ptr::null_mut();
    // SAFETY: the device handle and a valid port number; `handle` is ours.
    check(
        unsafe { (device.api.output_port_open)(device.device, port, &raw mut handle) },
        "AMidiOutputPort_open",
    )?;
    if handle.is_null() {
        return Err(std::io::Error::other("AMidiOutputPort_open gave no port"));
    }
    let api = device.api;
    // Detached by design (the module docs): a blocked receive cannot be
    // interrupted any other way, and the thread owns its feed. The
    // handle crosses as a `Send` word — the wrapper, not the raw field,
    // is what the closure captures.
    let port = PortPtr(handle);
    let _ = std::thread::Builder::new()
        .name("vieww-midi-amidi".into())
        .spawn(move || {
            // Arrival timestamps are only as good as the wake-up.
            let _rt = crate::rt::promote_helper_thread();
            pump(api, port, feed);
        });
    Ok(Box::new(Input { api, port: handle }))
}

/// The output-port handle as a thread-movable word.
#[derive(Clone, Copy)]
struct PortPtr(OutputPortHandle);
// SAFETY: the handle is used by the reader thread only; `drop` closes
// it, which is what ends the reader's blocked receive.
unsafe impl Send for PortPtr {}

/// Read the port until it ends. One call per message: the buffer is
/// large for the largest packet, and the running-status parser
/// reassembles anything a long SysEx split across.
fn pump(api: &'static Api, port: PortPtr, mut feed: Feed) {
    let port = port.0;
    let mut buffer = [0u8; RECV_BUF];
    loop {
        let mut opcode: c_int = 0;
        let mut got: usize = 0;
        let mut stamp: i64 = 0;
        // SAFETY: the port handle, and out-parameters of matching types.
        // The call blocks until bytes arrive or the port closes (the
        // drop path); its return is negative only on that error.
        let r = unsafe {
            (api.output_port_receive)(
                port,
                &raw mut opcode,
                buffer.as_mut_ptr(),
                buffer.len(),
                &raw mut got,
                &raw mut stamp,
            )
        };
        if r < 0 {
            // The port closed or failed: the honest end of this input.
            return;
        }
        if opcode == OPCODE_DATA && got > 0 && got <= buffer.len() {
            // The device's own timestamp (`stamp`, µs) is not the clock
            // this crate's inputs promise — arrival against the input's
            // own start is, like every other platform.
            if !feed.bytes(&buffer[..got]) {
                return; // the input is gone
            }
        }
        // `SETUP` and `FLUSH` opcodes carry no bytes: nothing to feed.
    }
}

// ---- output ------------------------------------------------------------------

pub(super) struct Out {
    api: &'static Api,
    port: InputPortHandle,
}

// SAFETY: the port handle moves with the sink; the NDK's send is a
// binder call usable from any thread.
unsafe impl Send for Out {}

impl Out {
    pub(super) fn open(_port: &MidiPort) -> std::io::Result<Self> {
        Err(unsupported())
    }

    /// Open the device's input port `port` (0-based) — the port it
    /// *receives* on, so the one a sender writes into.
    pub(super) fn open_amidi(device: &AmidiDevice, port: usize) -> std::io::Result<Self> {
        let n = device.num_input_ports();
        let port = i32::try_from(port).map_err(|_| {
            std::io::Error::other(format!("input port {port} is not a port number"))
        })?;
        if port >= n {
            return Err(std::io::Error::other(format!(
                "input port {port} is out of range (the device has {n})"
            )));
        }
        let mut handle: InputPortHandle = std::ptr::null_mut();
        // SAFETY: the device handle and a valid port number; `handle` is
        // ours.
        check(
            unsafe { (device.api.input_port_open)(device.device, port, &raw mut handle) },
            "AMidiInputPort_open",
        )?;
        if handle.is_null() {
            return Err(std::io::Error::other("AMidiInputPort_open gave no port"));
        }
        Ok(Self {
            api: device.api,
            port: handle,
        })
    }

    /// Send one complete message's bytes into the device with one
    /// `AMidiInputPort_send` — the sink flushes once per message, which
    /// is exactly the packet shape the NDK asks for.
    pub(super) fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let n = isize::try_from(bytes.len())
            .map_err(|_| std::io::Error::other("a MIDI message too long for AMidi"))?;
        // SAFETY: the port we opened, and `n` bytes of ours for the call.
        check(
            unsafe { (self.api.input_port_send)(self.port, bytes.as_ptr(), n) },
            "AMidiInputPort_send",
        )
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        // SAFETY: the port we opened, closed once.
        unsafe { (self.api.input_port_close)(self.port) };
    }
}
