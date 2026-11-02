//! The Apple-target half of AUv3: the `AUAudioUnit` subclass this crate
//! builds at run time. Read [`super`]'s module doc first — the status,
//! the ABI reasoning and the unverifiable list live there.
//!
//! What is in this file:
//!
//! * the Objective-C runtime and CoreFoundation declarations, in the
//!   same hand-rolled style as vieww-audio's `coreaudio.rs` and
//!   `midi_coremidi.rs` (extern blocks, `# Safety` comments on every
//!   call, no bindings crate);
//! * typed `objc_msgSend` helpers — one small wrapper per signature
//!   used, each transmuting the one arm64 entry point, which is the
//!   whole reason the module is `aarch64`-only (see the module doc);
//! * the clang block-literal machinery (`Block-ABI-03`): a global-`isa`
//!   literal with one pointer capture, boxed, whose memory this module
//!   owns — the runtime's retain/release on a global block is a no-op,
//!   so ownership never doubles;
//! * [`install`], which allocates the class pair, adds the methods and
//!   registers it — idempotent per class name;
//! * the IMPs themselves and [`Auv3State`], the Rust state they drive
//!   through the type-erased [`Erased`] trait (the registry maps class
//!   name → factory, so the IMPs stay non-generic).
//!
//! The parameter tree is built the way Apple's own AUv3 examples build
//! one: `AUParameterTree createParameterWithIdentifier:…` per
//! parameter, an `implementorValueObserver` block that forwards
//! `(parameter, value)` into the Rust state (the AUv3-native automation
//! path — the host sets the `AUParameter`, the block applies it), an
//! `implementorValueStringer` for display, and
//! `createTreeWithChildren:` over one group. `internalRenderBlock`
//! returns a literal that marshals the `AudioBufferList`s (interleaved
//! or planar, whichever the host passed), reads the musical context
//! block if the host installed one, drains the MIDI schedule into the
//! driver and renders. State travels through `fullStateForDocument` as
//! a `CFData` under one key — dictionaries are toll-free bridged, so
//! CoreFoundation calls build what the host reads as `NSDictionary`.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use super::events::{self, MidiSchedule};
use super::AudioComponentDescription;
use crate::{Driver, Event, ParamChange, ParamInfo, Plugin, Transport, MAX_CHANNELS};

/// An Objective-C object.
pub type Id = *mut c_void;
/// A selector.
pub type Sel = *const c_void;
/// A class.
pub type Class = *const c_void;

/// `OSStatus`: 0 is `noErr`.
type OsStatus = i32;
/// `AUAudioFrameCount`.
type Frames = u32;
/// `AUParameterAddress`.
pub type ParameterAddress = u64;
/// `AUEventSampleTime`.
type SampleTime = i64;

const NO_ERR: OsStatus = 0;

/// `kCFStringEncodingUTF8`.
const UTF8: u32 = 0x0800_0100;
/// The error domain this wrapper reports.
const ERROR_DOMAIN: &str = "dev.vieww.audio-plugin.auv3";

/// `AudioUnitParameterOptions` (AudioUnitProperties.h):
/// `kAudioUnitParameterFlag_IsReadable` is bit 30,
/// `kAudioUnitParameterFlag_IsWritable` bit 31 — verified against the
/// vendored public header, which this module's transcription cites by
/// field.
const PARAM_READABLE: u32 = 1 << 30;
const PARAM_WRITABLE: u32 = 1 << 31;
/// `AudioUnitParameterUnit_Generic` (AudioUnitProperties.h: 0).
const PARAM_UNIT_GENERIC: u32 = 0;

/// `kAudioTimeStampSampleTimeValid`.
const TS_SAMPLE_TIME_VALID: u32 = 1 << 0;

/// `AudioTimeStamp` — the render timeline's clock. Only the sample time
/// is read here.
#[repr(C)]
#[allow(non_snake_case, reason = "the header's own field names")]
struct AudioTimeStamp {
    mHostTime: u64,
    mWordClockTime: u64,
    mSampleTime: f64,
    mRateScalar: f64,
    mFlags: u32,
    mReserved: u32,
}

/// `AudioBuffer` / `AudioBufferList`, as every other Apple-facing
/// module in this workspace declares them. `mBuffers` is the C array of
/// one and the rest follow in memory, which is why access goes through
/// pointer arithmetic, never indexing.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_snake_case, reason = "the header's own field names")]
struct AudioBuffer {
    mNumberChannels: u32,
    mDataByteSize: u32,
    mData: *mut c_void,
}

#[repr(C)]
#[allow(non_snake_case, reason = "the header's own field names")]
struct AudioBufferList {
    mNumberBuffers: u32,
    mBuffers: [AudioBuffer; 1],
}

/// The bound the MIDI-event-list walk reads within: not the list's
/// length (the list carries none — it is walked by its own counts), but
/// a guard so a lying count cannot walk off the page. A megabyte is
/// tens of thousands of events, beyond any real schedule.
const LIST_GUARD: usize = 1 << 20;

// ── the runtime declarations ───────────────────────────────────────────────

#[repr(C)]
struct ObjcSuper {
    receiver: Id,
    super_class: Class,
}

/// The block ABI (clang's Block Implementation Specification): the
/// global literal flag.
const BLOCK_IS_GLOBAL: i32 = 1 << 28;

/// A block descriptor for literals with no copy/dispose helpers and no
/// signature: `reserved`, `size` — all the runtime reads for a global
/// block that is only ever invoked.
#[repr(C)]
struct BlockDescriptor {
    reserved: usize,
    size: usize,
}

/// The block literal head every clang-compiled block starts with.
#[repr(C)]
struct Block {
    isa: *const c_void,
    flags: i32,
    reserved: i32,
    invoke: *const c_void,
    descriptor: *const BlockDescriptor,
}

/// A block literal with one pointer capture, heap-allocated by this
/// module (the global `isa` makes the runtime's retains no-ops, so the
/// box's lifetime is simply the slot's).
#[repr(C)]
struct PtrBlock {
    head: Block,
    capture: Id,
}

static GLOBAL_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
    reserved: 0,
    size: std::mem::size_of::<PtrBlock>(),
};

#[link(name = "objc")]
unsafe extern "C" {
    /// The one arm64 entry point for every message (see the module doc
    /// for why this module does not exist on x86_64).
    fn objc_msgSend();
    fn objc_msgSendSuper();
    fn objc_getClass(name: *const c_char) -> Class;
    fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra: usize) -> Class;
    fn objc_registerClassPair(cls: Class);
    fn class_addMethod(
        cls: Class,
        sel: Sel,
        imp: unsafe extern "C" fn(),
        types: *const c_char,
    ) -> u8;
    fn class_getSuperclass(cls: Class) -> Class;
    fn class_getName(cls: Class) -> *const c_char;
    fn object_getClass(obj: Id) -> Class;
    fn object_getIndexedIvars(obj: Id) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> Sel;
    /// The method lookup, for the install test's assertions only.
    #[cfg(test)]
    fn class_getInstanceMethod(cls: Class, sel: Sel) -> *const c_void;
    /// The block runtime's concrete class for global literals
    /// (re-exported by libobjc; the module doc cites the ABI).
    static _NSConcreteGlobalBlock: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(alloc: *const c_void, cstr: *const c_char, encoding: u32) -> Id;
    fn CFRelease(p: *const c_void);
    fn CFDataCreate(alloc: *const c_void, bytes: *const u8, len: isize) -> Id;
    fn CFDataGetBytePtr(d: *const c_void) -> *const u8;
    fn CFDataGetLength(d: *const c_void) -> isize;
    fn CFArrayCreateMutable(alloc: *const c_void, capacity: isize, cbs: *const c_void) -> Id;
    fn CFArrayAppendValue(a: Id, value: *const c_void);
    fn CFDictionaryCreateMutable(
        alloc: *const c_void,
        capacity: isize,
        key_cbs: *const c_void,
        value_cbs: *const c_void,
    ) -> Id;
    fn CFDictionarySetValue(d: Id, key: *const c_void, value: *const c_void);
    fn CFDictionaryGetValue(d: Id, key: *const c_void) -> *const c_void;
    static kCFTypeDictionaryKeyCallBacks: CfCallBacks;
    static kCFTypeDictionaryValueCallBacks: CfCallBacks;
}

/// CoreFoundation's callback structs, declared only to take the address
/// of the two above.
#[repr(C)]
struct CfCallBacks {
    version: isize,
    f: [*const c_void; 5],
}

// AudioToolbox owns `AUAudioUnit` (and `AUParameterTree`); nothing in
// this file calls a C function from it, but linking it is what makes
// `objc_getClass("AUAudioUnit")` resolve — in a test binary as much as
// in an app extension.
#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {}

// ── typed message sends ────────────────────────────────────────────────────

/// `sel_registerName` on a Rust string (main-thread paths only; the
/// render path caches its selectors).
fn sel(name: &str) -> Sel {
    let c = CString::new(name).expect("a plain selector name");
    // SAFETY: a valid C string; sel_registerName never fails.
    unsafe { sel_registerName(c.as_ptr()) }
}

/// The render-path selector, resolved once (`sel_registerName` takes a
/// lock; the render block must not).
fn sel_musical_context() -> Sel {
    static S: OnceLock<usize> = OnceLock::new();
    // SAFETY: a valid C string; sel_registerName never fails. The
    // selector is held as an integer so the cell is plain data.
    let s =
        *S.get_or_init(|| unsafe { sel_registerName(c"musicalContextBlock".as_ptr()) as usize });
    s as Sel
}

macro_rules! send {
    ($name:ident, $ret:ty, $($arg:ident: $ty:ty),*) => {
        /// A typed `objc_msgSend` wrapper (see the module doc: one
        /// arm64 entry point, transmuted per signature).
        ///
        /// # Safety
        /// `recv` is a live object (or class) that answers `sel` with
        /// exactly this signature.
        #[allow(
            clippy::too_many_arguments,
            reason = "one typed wrapper per signature; the widest is the parameter-tree factory"
        )]
        unsafe fn $name(recv: *const c_void, sel: Sel, $($arg: $ty),*) -> $ret {
            #[allow(
                clippy::missing_transmute_annotations,
                reason = "the symbol is variadic by nature"
            )]
            let f: unsafe extern "C" fn(*const c_void, Sel, $($ty),*) -> $ret =
                unsafe { core::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
            // SAFETY: the caller's contract above.
            unsafe { f(recv, sel, $($arg),*) }
        }
    };
}

/// Coerce an IMP function item to the erased IMP type the block literal
/// and `class_addMethod` take. A direct `as` cast refuses (E0605, on
/// this toolchain, even pointer-to-pointer), so the item first binds to
/// its own pointer type and is then transmuted to the erased one — the
/// same erase C compilers do when they store an IMP. Nothing is ever
/// *called* through the erased type; it is storage only, and every
/// invoke goes through the signature the IMP was written with.
///
/// The expansion is an `unsafe` operation (the transmute): call it from
/// an `unsafe` context.
macro_rules! imp {
    ($f:ident as $sig:ty) => {{
        let imp: $sig = $f;
        // SAFETY: storage-only erase; never invoked through it.
        core::mem::transmute::<$sig, unsafe extern "C" fn()>(imp)
    }};
}

send!(msg_id, Id,);
send!(msg_id_id, Id, a: Id);
send!(msg_id_id_id_id, Id, a: Id, b: Id, c: Id);
send!(msg_id_id_id_i64_id, Id, a: Id, b: i64, c: Id);
send!(
    msg_id_param,
    Id,
    identifier: Id,
    name: Id,
    address: u64,
    min: f32,
    max: f32,
    unit: u32,
    unit_name: Id,
    flags: u32,
    value_strings: Id,
    dependent: Id
);
send!(msg_void_id, (), a: Id);
send!(msg_u64, u64,);
send!(msg_f32, f32,);

/// A typed `objc_msgSendSuper` wrapper for the initializer's signature.
///
/// # Safety
/// `sup` names a live receiver and its real superclass; the superclass
/// answers `sel` with exactly this signature.
unsafe fn super_init(sup: *const ObjcSuper, sel: Sel, a: *const c_void, b: u32, c: *mut Id) -> Id {
    #[allow(
        clippy::missing_transmute_annotations,
        reason = "the symbol is variadic by nature"
    )]
    let f: unsafe extern "C" fn(*const ObjcSuper, Sel, *const c_void, u32, *mut Id) -> Id =
        unsafe { core::mem::transmute(objc_msgSendSuper as unsafe extern "C" fn()) };
    // SAFETY: the caller's contract above.
    unsafe { f(sup, sel, a, b, c) }
}

/// A typed `objc_msgSendSuper` wrapper for `dealloc` (no arguments).
///
/// # Safety
/// `sup` names a live receiver and its real superclass.
unsafe fn super_void(sup: *const ObjcSuper, sel: Sel) {
    #[allow(
        clippy::missing_transmute_annotations,
        reason = "the symbol is variadic by nature"
    )]
    let f: unsafe extern "C" fn(*const ObjcSuper, Sel) =
        unsafe { core::mem::transmute(objc_msgSendSuper as unsafe extern "C" fn()) };
    // SAFETY: the caller's contract above.
    unsafe { f(sup, sel) };
}

/// An `NSString` from Rust bytes, owned by the caller (`CFRelease` it).
fn nsstring(s: &str) -> Id {
    let c = CString::new(s).expect("a plain string");
    // SAFETY: a valid C string; the result is released by the caller.
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
}

/// An `NSError` with this wrapper's domain, or null (allocation failure
/// only).
fn ns_error(code: i64, message: &str) -> Id {
    let domain = nsstring(ERROR_DOMAIN);
    let info = nsstring(message);
    // SAFETY: the dictionaries and strings below are ours; NSError
    // answers alloc and its designated initializer.
    let dict = unsafe {
        CFDictionaryCreateMutable(
            std::ptr::null(),
            0,
            (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
            (&raw const kCFTypeDictionaryValueCallBacks).cast(),
        )
    };
    let mut out = std::ptr::null_mut();
    if !dict.is_null() && !info.is_null() && !domain.is_null() {
        let key = c"NSLocalizedDescription".as_ptr();
        unsafe {
            CFDictionarySetValue(dict, key.cast(), info);
            let cls = objc_getClass(c"NSError".as_ptr());
            if !cls.is_null() {
                let a = msg_id(cls.cast(), sel("alloc"));
                if !a.is_null() {
                    out = msg_id_id_id_i64_id(
                        a,
                        sel("initWithDomain:code:userInfo:"),
                        domain,
                        code,
                        dict,
                    );
                    CFRelease(a);
                }
            }
        }
    }
    // SAFETY: objects we created and no longer hold.
    unsafe {
        if !info.is_null() {
            CFRelease(info);
        }
        if !dict.is_null() {
            CFRelease(dict);
        }
    }
    out
}

// ── the block literals ─────────────────────────────────────────────────────

/// Build a heap block literal whose invoke is `f` and whose capture is
/// `capture`. The box's address is the block pointer.
fn block(f: unsafe extern "C" fn(), capture: Id) -> Box<PtrBlock> {
    Box::new(PtrBlock {
        head: Block {
            // SAFETY: the runtime's own global-concrete class, read as
            // a symbol; the literal below it matches the ABI.
            isa: unsafe { _NSConcreteGlobalBlock },
            flags: BLOCK_IS_GLOBAL,
            reserved: 0,
            invoke: f as *const c_void,
            descriptor: &raw const GLOBAL_DESCRIPTOR,
        },
        capture,
    })
}

/// Read a block's capture (its first word past the literal head).
///
/// # Safety
/// `b` is a block this module built.
unsafe fn capture_of(b: Id) -> Id {
    // SAFETY: the layout is ours.
    unsafe { (*b.cast::<PtrBlock>()).capture }
}

/// Call the view-controller completion handler with a (nullable)
/// controller.
///
/// # Safety
/// `b` is a live block of that signature.
unsafe fn call_completion(b: Id, controller: Id) {
    #[allow(
        clippy::missing_transmute_annotations,
        reason = "the symbol is variadic by nature"
    )]
    let f: unsafe extern "C" fn(Id, Id) =
        unsafe { core::mem::transmute((*b.cast::<Block>()).invoke) };
    // SAFETY: the caller's contract above.
    unsafe { f(b, controller) };
}

// ── the state ──────────────────────────────────────────────────────────────

/// The type-erased plugin state the IMPs drive (the registry maps class
/// names to factories for it, keeping the IMPs non-generic).
trait Erased: Send {
    /// The plugin's parameter table (for tree building).
    fn params(&self) -> &[ParamInfo];
    /// Apply a parameter value (the tree's implementor path: main
    /// thread, may lock the audio state).
    fn set_value(&mut self, address: u64, value: f64);
    /// A parameter's display string.
    fn display(&self, address: u64, value: f64) -> String;
    /// Prepare for processing.
    fn allocate(&mut self) -> bool;
    /// Stop processing.
    fn deallocate(&mut self);
    /// Whether `allocate` succeeded (the render path's guard).
    fn is_allocated(&self) -> bool;
    /// The render clock when the host's timestamps carry no sample
    /// time.
    fn clock(&self) -> SampleTime;
    /// Render one block: pull the input through the host's pull block,
    /// deliver the realtime event list, render into the planar scratch,
    /// and scatter — or re-point — the host's output buffers.
    ///
    /// # Safety
    /// The buffer list, event list and pull block are the host's, valid
    /// for this call.
    #[allow(
        clippy::too_many_arguments,
        reason = "the render block's own ABI, one argument per parameter"
    )]
    unsafe fn render(
        &mut self,
        block_start: SampleTime,
        frame_count: Frames,
        action_flags: *mut u32,
        timestamp: *const AudioTimeStamp,
        output: *mut AudioBufferList,
        events: *const events::RenderEventHeader,
        pull: Id,
        transport: Transport,
    ) -> OsStatus;
    /// Buffer scheduled MIDI.
    fn schedule_midi(&self, time: SampleTime, bytes: &[u8]);
    /// The plugin's state bytes.
    fn save(&self) -> Vec<u8>;
    /// Restore; false if not ours.
    fn load(&mut self, data: &[u8]) -> bool;
    /// Latency in samples.
    fn latency(&self) -> u32;
    /// Attach the offscreen panel (the view story, `gui` feature) and
    /// return the edits it has queued as `(id, value)` pairs — the AU's
    /// main thread applies them through the tree's own path.
    #[cfg(feature = "gui")]
    fn attach_view(&mut self) -> Vec<(u32, f64)>;
}

/// The per-instance Rust state: a [`Driver`], the parameter mirror the
/// tree's stringer reads, the MIDI schedule, and the bounded queue that
/// carries main-thread parameter edits to the render block.
struct Auv3State<P: Plugin> {
    driver: Driver<P>,
    params: Vec<ParamInfo>,
    /// Plain values, mirrored after every rendered block — what display
    /// strings and the view read without touching audio state.
    values: Vec<AtomicU64>,
    schedule: MidiSchedule,
    /// Main-thread parameter edits (the parameter tree's observer
    /// path) waiting for the next render call: an AUv3 host sets
    /// `AUParameter`s on the main thread while rendering continues, so
    /// the observer may not touch the driver — it queues, the render
    /// block drains. Same shape and bound as [`MidiSchedule`]'s ring.
    edits: Mutex<Vec<ParamChange>>,
    allocated: bool,
    /// The clock used when the host's timestamps carry no sample time.
    frames_done: u64,
    /// Rate and ceiling (bus formats are the untranscribed piece — see
    /// the module doc — so both are fixed until they land).
    rate: f64,
    max_block: u32,
    scratch_in: Vec<Vec<f32>>,
    scratch_out: Vec<Vec<f32>>,
    /// The buffer list handed to the host's pull-input block: a count
    /// plus `MAX_CHANNELS` buffers, laid out exactly as an
    /// `AudioBufferList` with that many buffers is in memory.
    pull_list: PullList,
    #[cfg(feature = "gui")]
    view: MainThreadView,
}

/// The offscreen view, wrapped so the state it lives in can be `Send`.
///
/// `Send` is about travel, not access: the state is created on whatever
/// thread ran the initializer and must be movable to the AU's render
/// thread as part of [`Auv3State`] — but the view inside it is then
/// only ever touched from the main thread, the same discipline the
/// CLAP wiring gives its panel (the audio path never touches it; the
/// render path reads the atomic value mirror, and the view syncs from
/// those atomics on the main thread — see [`Auv3State::mirror`]). The
/// panel's rasterizer is `!Send` only because its image cache is keyed
/// by raw pointers; moving those pointer values across a thread
/// boundary does not dereference them.
#[cfg(feature = "gui")]
#[derive(Default)]
struct MainThreadView(Mutex<super::view::Auv3View>);

#[cfg(feature = "gui")]
// SAFETY: the view is main-thread-only by the wrapper's own discipline
// (the type's doc above); sending it with the state never accesses it.
unsafe impl Send for MainThreadView {}

impl<P: Plugin> Auv3State<P> {
    fn new() -> Self {
        let driver = Driver::<P>::new();
        let params = driver.params.clone();
        let values = params
            .iter()
            .map(|p| AtomicU64::new(driver.plugin.get_param(p.id).unwrap_or(p.default).to_bits()))
            .collect();
        Self {
            driver,
            params,
            values,
            schedule: MidiSchedule::new(),
            edits: Mutex::new(Vec::with_capacity(crate::MAX_EVENTS)),
            allocated: false,
            frames_done: 0,
            rate: 48_000.0,
            max_block: 4_096,
            scratch_in: Vec::new(),
            scratch_out: Vec::new(),
            pull_list: PullList::default(),
            #[cfg(feature = "gui")]
            view: MainThreadView::default(),
        }
    }

    /// Queue a parameter value from the main thread: the change reaches
    /// the driver at frame 0 of the next rendered block (an AUv3 host
    /// may set parameters while rendering continues, so the driver is
    /// the render thread's, never the main thread's), while the mirror
    /// and the view see it immediately.
    fn apply(&mut self, id: u32, value: f64) {
        if let Ok(mut q) = self.edits.lock() {
            if q.len() < q.capacity() {
                q.push(ParamChange { time: 0, id, value });
            }
        }
        let mirrored = self
            .params
            .iter()
            .position(|p| p.id == id)
            .and_then(|i| self.values.get(i));
        if let Some(slot) = mirrored {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        #[cfg(feature = "gui")]
        if let Ok(mut view) = self.view.0.lock() {
            view.param_notify(id, value);
        }
    }

    /// Push the plugin's current values into the atomic mirror.
    ///
    /// Render-path safe by construction: the view is **not** touched
    /// here — the CLAP wiring's own rule (the audio path never touches
    /// the panel) — the main thread syncs the view from these atomics
    /// when it looks at it (see [`Self::attach_view`]).
    fn mirror(&mut self) {
        for (info, slot) in self.params.iter().zip(&self.values) {
            let v = self
                .driver
                .plugin
                .get_param(info.id)
                .unwrap_or(info.default);
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
    }
}

/// The mutable buffer accessor, by index (the list is variable-length
/// in C: pointer arithmetic, never array indexing).
///
/// # Safety
/// `list` is a valid buffer list with at least `i + 1` buffers.
unsafe fn buffer_at_mut(list: *mut AudioBufferList, i: usize) -> *mut AudioBuffer {
    // SAFETY: the contract above; `mBuffers` is the first of many.
    unsafe { (*list).mBuffers.as_mut_ptr().add(i) }
}

/// The buffer list the wrapper hands the host's pull-input block: the
/// `AudioBufferList` shape with `MAX_CHANNELS` buffers inline, which is
/// exactly how the C type's variable tail lays out in memory.
#[repr(C)]
struct PullList {
    /// `mNumberBuffers`.
    count: u32,
    /// `mBuffers` — one per channel, planar.
    buffers: [AudioBuffer; MAX_CHANNELS],
}

impl Default for PullList {
    fn default() -> Self {
        Self {
            count: 0,
            buffers: [AudioBuffer {
                mNumberChannels: 0,
                mDataByteSize: 0,
                mData: std::ptr::null_mut(),
            }; MAX_CHANNELS],
        }
    }
}

// SAFETY: the pull list's `mData` pointers are either null or point at
// the render thread's own scratch, valid only within a single render
// call and never dereferenced from any other thread. `Send` here is
// travel: the state moves from the initialising thread to the render
// thread once, as a whole, before any render call fills the list.
unsafe impl Send for PullList {}

/// Call the host's pull-input block
/// (`AURenderPullInputBlock`: action flags, timestamp, frame count,
/// input bus, buffer list → status).
///
/// # Safety
/// `pull` is the host's live pull block for this render call, and
/// `list` is ours, valid to write.
unsafe fn call_pull(
    pull: Id,
    action_flags: *mut u32,
    timestamp: *const AudioTimeStamp,
    frame_count: Frames,
    bus: isize,
    list: *mut AudioBufferList,
) -> OsStatus {
    #[allow(
        clippy::missing_transmute_annotations,
        reason = "the symbol is variadic by nature"
    )]
    let f: unsafe extern "C" fn(
        Id,
        *mut u32,
        *const AudioTimeStamp,
        Frames,
        isize,
        *mut AudioBufferList,
    ) -> OsStatus = unsafe { core::mem::transmute((*pull.cast::<Block>()).invoke) };
    // SAFETY: the caller's contract above.
    unsafe { f(pull, action_flags, timestamp, frame_count, bus, list) }
}

impl<P: Plugin> Erased for Auv3State<P> {
    fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    fn set_value(&mut self, address: u64, value: f64) {
        let Some(p) = self.params.iter().find(|p| u64::from(p.id) == address) else {
            return;
        };
        let (id, v) = (p.id, p.clamp(value));
        self.apply(id, v);
    }

    fn display(&self, address: u64, value: f64) -> String {
        match self.params.iter().find(|p| u64::from(p.id) == address) {
            Some(p) => p.display(value),
            None => String::new(),
        }
    }

    fn allocate(&mut self) -> bool {
        self.rate = 48_000.0;
        self.max_block = 4_096;
        let widest = P::info()
            .outputs
            .max(P::info().inputs)
            .clamp(1, u32::try_from(MAX_CHANNELS).unwrap_or(u32::MAX));
        let ch = usize::try_from(widest).unwrap_or(MAX_CHANNELS);
        self.scratch_in = vec![vec![0.0f32; self.max_block as usize]; ch];
        self.scratch_out = vec![vec![0.0f32; self.max_block as usize]; ch];
        self.allocated = self.driver.plugin.activate(self.rate, self.max_block);
        self.allocated
    }

    fn deallocate(&mut self) {
        if self.allocated {
            self.driver.plugin.deactivate();
            self.allocated = false;
            self.schedule.clear();
        }
    }

    fn is_allocated(&self) -> bool {
        self.allocated
    }

    fn clock(&self) -> SampleTime {
        #[expect(clippy::cast_possible_wrap, reason = "stream time")]
        let t = self.frames_done as SampleTime;
        t
    }

    unsafe fn render(
        &mut self,
        block_start: SampleTime,
        frame_count: Frames,
        action_flags: *mut u32,
        timestamp: *const AudioTimeStamp,
        output: *mut AudioBufferList,
        events: *const events::RenderEventHeader,
        pull: Id,
        transport: Transport,
    ) -> OsStatus {
        if !self.allocated || output.is_null() {
            zero_output(output, frame_count);
            return NO_ERR;
        }
        let frames = (frame_count as usize).min(self.max_block as usize);
        self.driver.transport = transport;

        // Main-thread edits queued through the parameter tree's observer
        // (see [`Auv3State::apply`]): drained here, at frame 0, before
        // the block's own events.
        if let Ok(mut q) = self.edits.lock() {
            for c in q.drain(..) {
                self.driver.push_change(c);
            }
        }

        // Input: the render block pulls it through the host's own
        // `pullInputBlock` (that is how AUv3 delivers input — the block
        // does not receive input buffers as parameters). Our planar
        // scratch backs the buffer list we hand it; the host may fill
        // those pointers or replace them with its own, so the gather
        // reads the list back after the call.
        if !pull.is_null() && !self.scratch_in.is_empty() {
            let n = self.scratch_in.len().min(MAX_CHANNELS);
            self.pull_list.count = n as u32;
            for (i, buf) in self.pull_list.buffers.iter_mut().take(n).enumerate() {
                buf.mNumberChannels = 1;
                buf.mDataByteSize = (frames * 4) as u32;
                buf.mData = self.scratch_in[i].as_mut_ptr().cast();
            }
            // SAFETY: our own list, the host's live pull block for this
            // call, and the render call's own flags and timestamp.
            let status = unsafe {
                call_pull(
                    pull,
                    action_flags,
                    timestamp,
                    frame_count,
                    0,
                    &raw mut self.pull_list as *mut AudioBufferList,
                )
            };
            if status == NO_ERR {
                // Gather: one buffer per channel (the planar layout we
                // asked for); the host may have re-pointed `mData`.
                for (i, buf) in self.pull_list.buffers.iter().take(n).enumerate() {
                    if buf.mData.is_null() || buf.mNumberChannels == 0 {
                        self.scratch_in[i]
                            .iter_mut()
                            .take(frames)
                            .for_each(|s| *s = 0.0);
                        continue;
                    }
                    let len = (buf.mDataByteSize as usize) / 4;
                    // SAFETY: the host's own pointer, valid until the
                    // next render cycle per the pull block's contract.
                    let src = unsafe { std::slice::from_raw_parts(buf.mData.cast::<f32>(), len) };
                    let dst = &mut self.scratch_in[i];
                    for (f, slot) in dst.iter_mut().enumerate() {
                        if f >= frames {
                            break;
                        }
                        *slot = src.get(f).copied().unwrap_or(0.0);
                    }
                }
            } else {
                // A failed pull means the input is invalid: silence.
                for c in &mut self.scratch_in {
                    c.iter_mut().take(frames).for_each(|s| *s = 0.0);
                }
            }
        }

        // The realtime event list: parameter changes at their frame, MIDI
        // as `Event::Midi` (a message longer than the three bytes
        // `Event::Midi` carries — SysEx — is dropped, exactly as the LV2
        // wrapper drops what does not fit the model, by name).
        let ev_driver = &mut self.driver;
        // SAFETY: the host's event list for this call; every event's
        // `length` bytes are readable at its `data` field.
        unsafe {
            events::walk_render_events(events, |time, kind, bytes| {
                let at = if events::is_immediate(time) {
                    0
                } else {
                    let off = time - block_start;
                    if off < 0 {
                        0
                    } else {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "clamped to the block above"
                        )]
                        let at = off.min(i64::from(frame_count)) as u32;
                        at
                    }
                };
                match kind {
                    events::RenderEventKind::Parameter { address, value, .. } => {
                        if let Some(p) =
                            ev_driver.params.iter().find(|p| u64::from(p.id) == address)
                        {
                            ev_driver.push_change(ParamChange {
                                time: at,
                                id: p.id,
                                value: f64::from(value),
                            });
                        }
                    }
                    events::RenderEventKind::Midi { .. } => {
                        if bytes.len() <= 3 {
                            let mut data = [0u8; 3];
                            data[..bytes.len()].copy_from_slice(bytes);
                            ev_driver.push_event(Event::Midi { time: at, data });
                        }
                        // Longer (SysEx) messages: dropped, counted by
                        // nothing — the same named refusal as the LV2
                        // wrapper's, documented in the module doc.
                    }
                }
            });
        }
        // The scheduled-MIDI path (`scheduleMIDIEventList:`) delivers its
        // due events here too.
        self.schedule
            .drain_into(block_start, frame_count, |at, bytes| {
                let mut data = [0u8; 3];
                let n = bytes.len().min(3);
                data[..n].copy_from_slice(&bytes[..n]);
                ev_driver.push_event(Event::Midi { time: at, data });
            });
        // Render into the planar scratch.
        let ch_out = self.scratch_out.len();
        let mut ins: [&[f32]; MAX_CHANNELS] = [&[]; MAX_CHANNELS];
        let mut outs: [&mut [f32]; MAX_CHANNELS] = Default::default();
        for (i, v) in self.scratch_in.iter().take(MAX_CHANNELS).enumerate() {
            ins[i] = &v[..frames];
        }
        for (i, slot) in self.scratch_out.iter_mut().take(MAX_CHANNELS).enumerate() {
            let end = slot.len().min(frames);
            outs[i] = &mut slot[..end];
        }
        {
            let inputs = &ins[..self.scratch_in.len().min(MAX_CHANNELS)];
            let outputs = &mut outs[..ch_out.min(MAX_CHANNELS)];
            let _ = self.driver.run(inputs, outputs, frames);
        }
        // Publish: a host buffer with a null `mData` is ours to point —
        // the scratch stays valid until the next render or
        // `deallocateRenderResources`, which is the header's own
        // contract; a host buffer with memory gets the frames copied
        // in, interleaving where a buffer carries more than one channel.
        // SAFETY: the host's output list for this call.
        let n = unsafe { (*output).mNumberBuffers } as usize;
        let mut ch = 0usize;
        for b in 0..n {
            // SAFETY: buffer `b` of a list that declares `n`.
            let buf = unsafe { buffer_at_mut(output, b) };
            // SAFETY: the host's buffer for this call.
            let (channels, data) = unsafe { ((*buf).mNumberChannels, (*buf).mData) };
            if channels == 0 {
                continue;
            }
            if data.is_null() {
                if ch < self.scratch_out.len() {
                    // SAFETY: the host's buffer struct for this call.
                    unsafe {
                        (*buf).mData = self.scratch_out[ch].as_mut_ptr().cast();
                        (*buf).mDataByteSize = (frames * 4) as u32;
                    }
                    ch += 1;
                }
                continue;
            }
            let len = ((*buf).mDataByteSize as usize) / 4;
            // SAFETY: `channels * len` floats, per the byte size.
            let dst = unsafe {
                std::slice::from_raw_parts_mut(data.cast::<f32>(), len * channels as usize)
            };
            for c in 0..channels as usize {
                if ch >= self.scratch_out.len() {
                    break;
                }
                let src = &self.scratch_out[ch];
                for (f, &s) in src.iter().enumerate().take(frames.min(len)) {
                    dst[f * channels as usize + c] = s;
                }
                ch += 1;
            }
        }
        let advanced = u64::try_from(block_start.max(0)).unwrap_or(0);
        self.frames_done = advanced.saturating_add(frames as u64);
        self.mirror();
        NO_ERR
    }

    fn schedule_midi(&self, time: SampleTime, bytes: &[u8]) {
        self.schedule.schedule(time, bytes);
    }

    fn save(&self) -> Vec<u8> {
        self.driver.plugin.save_state()
    }

    fn load(&mut self, data: &[u8]) -> bool {
        let ok = self.driver.plugin.load_state(data);
        if ok {
            self.mirror();
        }
        ok
    }

    fn latency(&self) -> u32 {
        self.driver.plugin.latency()
    }

    #[cfg(feature = "gui")]
    fn attach_view(&mut self) -> Vec<(u32, f64)> {
        // The main thread's moment with the view: attach the panel if
        // needed, sync the atomic value mirror into it (the render path
        // kept them current without ever touching the view), and take
        // the edits the panel queued.
        let Ok(mut view) = self.view.0.lock() else {
            return Vec::new();
        };
        if !view.is_attached() {
            view.attach(&self.params);
        }
        for (i, info) in self.params.iter().enumerate() {
            let v = self
                .values
                .get(i)
                .map(|slot| f64::from_bits(slot.load(Ordering::Relaxed)))
                .unwrap_or(info.default);
            view.param_notify(info.id, v);
        }
        view.edits()
            .take()
            .into_iter()
            .map(|e| (e.id, e.value))
            .collect()
    }
}

/// Zero the host's output buffers (the not-allocated path).
///
/// # Safety
/// `output` is null or the host's buffer list for this call.
unsafe fn zero_output(output: *mut AudioBufferList, frame_count: Frames) {
    if output.is_null() {
        return;
    }
    // SAFETY: the contract above.
    unsafe {
        let n = (*output).mNumberBuffers as usize;
        for b in 0..n {
            let buf = buffer_at_mut(output, b);
            let (channels, data) = ((*buf).mNumberChannels, (*buf).mData);
            if channels == 0 || data.is_null() {
                continue;
            }
            let len = ((*buf).mDataByteSize as usize) / 4;
            let dst = std::slice::from_raw_parts_mut(data.cast::<f32>(), len);
            dst.iter_mut().take(frame_count as usize).for_each(|s| {
                *s = 0.0;
            });
        }
    }
}

// ── the per-instance slot ─────────────────────────────────────────────────

/// What lives in the ObjC instance's trailing bytes: the state box and
/// the objects the getters hand out (owned; released in `dealloc`).
#[repr(C)]
struct Slot {
    state: *mut Box<dyn Erased>,
    /// The built `AUParameterTree` (owned; the getter hands it out).
    tree: Id,
    /// The render block literal (boxed; the box is the block).
    render_block: *mut PtrBlock,
    /// The observer and stringer blocks (boxed).
    observer: *mut PtrBlock,
    stringer: *mut PtrBlock,
}

const SLOT_EMPTY: Slot = Slot {
    state: std::ptr::null_mut(),
    tree: std::ptr::null_mut(),
    render_block: std::ptr::null_mut(),
    observer: std::ptr::null_mut(),
    stringer: std::ptr::null_mut(),
};

/// The slot pointer for an instance.
///
/// # Safety
/// `obj` is an instance of a class this module installed.
unsafe fn slot_of(obj: Id) -> *mut Slot {
    // SAFETY: the extra bytes `objc_allocateClassPair` reserved, exactly
    // one `Slot`'s worth.
    unsafe { object_getIndexedIvars(obj).cast() }
}

/// The state behind an instance.
///
/// # Safety
/// `obj` is an initialised instance of a class this module installed.
unsafe fn state_of<'a>(obj: Id) -> Option<&'a mut Box<dyn Erased>> {
    // SAFETY: the slot was filled in `initWithComponentDescription:`.
    unsafe { (*slot_of(obj)).state.as_mut() }
}

// ── the registry ───────────────────────────────────────────────────────────

type Factory = fn() -> Box<dyn Erased>;

fn factories() -> &'static Mutex<HashMap<String, Factory>> {
    static F: OnceLock<Mutex<HashMap<String, Factory>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(HashMap::new()))
}

fn factory_for(name: &str) -> Option<Factory> {
    factories().lock().ok().and_then(|f| f.get(name).copied())
}

/// Install the `AUAudioUnit` subclass for plugin type `P` under
/// `class_name` — the name the app extension's `Info.plist` declares as
/// its `NSExtensionPrincipalClass`, and that
/// [`manifest`](super::manifest) writes into the `AudioComponents`
/// entry.
///
/// Idempotent per name: a second call for the same name is a no-op (the
/// same process loading the same extension twice). Errors name the
/// runtime refusal (no `AUAudioUnit` class, the name taken by something
/// else, a method the runtime refused).
///
/// Call once, at extension load: `install::<MyPlugin>("MyPluginAU")`,
/// then ship the manifest's plist entry naming the same class.
///
/// # Errors
/// See above; the string says what the runtime refused.
pub fn install<P: Plugin>(class_name: &str) -> Result<(), String> {
    {
        let mut f = factories()
            .lock()
            .map_err(|e| format!("registry poisoned ({e})"))?;
        if f.contains_key(class_name) {
            return Ok(());
        }
        f.insert(class_name.to_owned(), || Box::new(Auv3State::<P>::new()));
    }
    let name = CString::new(class_name).map_err(|e| format!("class name ({e})"))?;
    // SAFETY: the whole sequence below is the documented runtime API;
    // every failure path leaves nothing half-registered.
    unsafe {
        let au = objc_getClass(c"AUAudioUnit".as_ptr());
        if au.is_null() {
            return Err("no AUAudioUnit class (AudioToolbox not linked?)".into());
        }
        if !objc_getClass(name.as_ptr()).is_null() {
            return Err(format!("{class_name} is already a class"));
        }
        let cls = objc_allocateClassPair(au, name.as_ptr(), std::mem::size_of::<Slot>());
        if cls.is_null() {
            return Err("objc_allocateClassPair refused".into());
        }
        for (sel_name, imp, types) in METHODS {
            let s = sel(sel_name);
            // SAFETY: our class pair, a selector we just registered, an
            // IMP of ours, and a C string.
            if class_addMethod(cls, s, *imp, types.as_ptr()) == 0 {
                return Err(format!("class_addMethod refused {sel_name}"));
            }
        }
        objc_registerClassPair(cls);
    }
    Ok(())
}

/// The methods the class carries: (selector, IMP, type encoding).
///
/// The encodings are introspection metadata (what `NSMethodSignature`
/// reads); the ABI the IMPs must match is the C calling convention, and
/// the large struct parameter of the initializer is memory-passed on
/// arm64 (a composite over 16 bytes), which is why its IMP takes a
/// pointer.
static METHODS: &[(&str, unsafe extern "C" fn(), &CStr)] = &[
    (
        "initWithComponentDescription:options:error:",
        unsafe {
            imp!(
                imp_init
                    as unsafe extern "C" fn(
                        Id,
                        Sel,
                        *const AudioComponentDescription,
                        u32,
                        *mut Id,
                    ) -> Id
            )
        },
        c"@@:^{AudioComponentDescription=IIIII}Q^@",
    ),
    (
        "parameterTree",
        unsafe { imp!(imp_parameter_tree as unsafe extern "C" fn(Id, Sel) -> Id) },
        c"@@:",
    ),
    (
        "internalRenderBlock",
        unsafe { imp!(imp_internal_render_block as unsafe extern "C" fn(Id, Sel) -> Id) },
        c"@?@:",
    ),
    (
        "allocateRenderResourcesAndReturnError:",
        unsafe { imp!(imp_allocate as unsafe extern "C" fn(Id, Sel, *mut Id) -> u8) },
        c"B@:^@",
    ),
    (
        "deallocateRenderResources",
        unsafe { imp!(imp_deallocate as unsafe extern "C" fn(Id, Sel)) },
        c"v@:",
    ),
    (
        "scheduleMIDIEventList:",
        unsafe { imp!(imp_schedule_midi as unsafe extern "C" fn(Id, Sel, *const c_void)) },
        c"v@:^{AUMIDIEventList=}",
    ),
    (
        "requestViewControllerWithCompletionHandler:",
        unsafe { imp!(imp_request_view as unsafe extern "C" fn(Id, Sel, Id)) },
        c"v@:@?",
    ),
    (
        "fullStateForDocument",
        unsafe { imp!(imp_full_state as unsafe extern "C" fn(Id, Sel) -> Id) },
        c"@@:",
    ),
    (
        "setFullStateForDocument:",
        unsafe { imp!(imp_set_full_state as unsafe extern "C" fn(Id, Sel, Id)) },
        c"v@:@",
    ),
    (
        "latency",
        unsafe { imp!(imp_latency as unsafe extern "C" fn(Id, Sel) -> f64) },
        c"d@:",
    ),
    (
        "dealloc",
        unsafe { imp!(imp_dealloc as unsafe extern "C" fn(Id, Sel)) },
        c"v@:",
    ),
];

// ── the IMPs ───────────────────────────────────────────────────────────────

/// The state key in the document-state dictionary.
const STATE_KEY: &str = "dev.vieww.audio-plugin.state";

unsafe extern "C" fn imp_init(
    obj: Id,
    _sel: Sel,
    desc: *const AudioComponentDescription,
    options: u32,
    error: *mut Id,
) -> Id {
    // SAFETY: the superclass is AUAudioUnit (the class pair was
    // allocated with it); the initializer's signature is the one this
    // IMP itself answers.
    let sup = ObjcSuper {
        receiver: obj,
        super_class: {
            let cls = object_getClass(obj);
            class_getSuperclass(cls)
        },
    };
    let parent = unsafe {
        super_init(
            &sup,
            sel("initWithComponentDescription:options:error:"),
            desc.cast(),
            options,
            error,
        )
    };
    if parent.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `obj` is an instance of a class this module installed
    // (the IMP is only reachable through one).
    let name = unsafe { class_name_of(obj) };
    let Some(factory) = factory_for(&name) else {
        if !error.is_null() {
            // SAFETY: the host's out-pointer for the failure reason.
            unsafe {
                *error = ns_error(
                    1,
                    "no state factory for this class (install was not called?)",
                );
            }
        }
        return std::ptr::null_mut();
    };
    let state = factory();
    // SAFETY: the slot is the trailing bytes `install` reserved, and
    // this is the first (only) initializer.
    unsafe {
        let slot = slot_of(obj);
        std::ptr::write(slot, SLOT_EMPTY);
        (*slot).state = Box::into_raw(Box::new(state));
    }
    obj
}

/// The class name of an instance, as an owned `String`.
///
/// # Safety
/// `obj` is an instance of a class this module installed.
unsafe fn class_name_of(obj: Id) -> String {
    // SAFETY: `class_getName` on a live class returns the runtime's own
    // storage, which lives as long as the class does.
    unsafe {
        let cls = object_getClass(obj);
        CStr::from_ptr(class_getName(cls))
            .to_string_lossy()
            .into_owned()
    }
}

unsafe extern "C" fn imp_parameter_tree(obj: Id, _sel: Sel) -> Id {
    // SAFETY: an initialised instance of ours.
    unsafe {
        let slot = slot_of(obj);
        if !(*slot).tree.is_null() {
            return (*slot).tree;
        }
        let Some(state) = state_of(obj) else {
            return std::ptr::null_mut();
        };
        let tree = build_tree(obj, state);
        (*slot).tree = tree;
        tree
    }
}

/// Build the `AUParameterTree`: one `AUParameter` per plugin parameter
/// (with the observer and stringer blocks below), one group, one tree.
///
/// # Safety
/// `obj` is an initialised instance of ours; `state` is its state.
unsafe fn build_tree(obj: Id, state: &mut Box<dyn Erased>) -> Id {
    unsafe {
        let tree_cls = objc_getClass(c"AUParameterTree".as_ptr());
        if tree_cls.is_null() {
            return std::ptr::null_mut();
        }
        // The observer and stringer blocks capture the instance; their
        // memory is the slot's.
        let slot = slot_of(obj);
        let observer = Box::into_raw(block(
            imp!(imp_value_observer as unsafe extern "C" fn(Id, Id, f32)),
            obj,
        ));
        let stringer = Box::into_raw(block(
            imp!(imp_value_stringer as unsafe extern "C" fn(Id, Id, *const f32) -> Id),
            obj,
        ));
        (*slot).observer = observer;
        (*slot).stringer = stringer;

        let children = CFArrayCreateMutable(std::ptr::null(), 0, std::ptr::null());
        let group_id = nsstring("root");
        let group_name = nsstring("Parameters");
        for p in state.params() {
            let identifier = nsstring(&format!("p{}", p.id));
            let name = nsstring(&p.name);
            let unit_name = nsstring(p.unit);
            // `AUValue` is `float`: the parameter table's `f64` bounds
            // narrow once, here, on the way into the tree.
            #[expect(clippy::cast_possible_truncation, reason = "AUValue is f32")]
            let min = p.min as f32;
            #[expect(clippy::cast_possible_truncation, reason = "AUValue is f32")]
            let max = p.max as f32;
            // SAFETY: AUParameterTree answers its documented class
            // method; the argument order is the selector's own.
            let param = msg_id_param(
                tree_cls.cast(),
                sel("createParameterWithIdentifier:name:address:min:max:unit:unitName:flags:valueStrings:dependentParameters:"),
                identifier,
                name,
                u64::from(p.id),
                min,
                max,
                PARAM_UNIT_GENERIC,
                unit_name,
                PARAM_READABLE | PARAM_WRITABLE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if !param.is_null() {
                // The observer (value changes) and the stringer (display
                // strings): the AUv3-native automation path. The property
                // names are AUParameterNode's own
                // (`implementorValueObserver`, set through its setter).
                // SAFETY: a parameter we just created.
                msg_void_id(param, sel("setImplementorValueObserver:"), observer.cast());
                msg_void_id(
                    param,
                    sel("setImplementorStringFromValueCallback:"),
                    stringer.cast(),
                );
                // SAFETY: our array.
                CFArrayAppendValue(children, param);
                CFRelease(param); // the tree retains its children; the
                                  // array only holds the pointer
            }
            // SAFETY: strings we created and no longer hold.
            CFRelease(identifier);
            CFRelease(name);
            if !unit_name.is_null() {
                CFRelease(unit_name);
            }
        }
        // SAFETY: AUParameterTree's group class method, with our group
        // identifiers and the children array.
        let group = msg_id_id_id_id(
            tree_cls.cast(),
            sel("createGroupWithIdentifier:name:children:"),
            group_id,
            group_name,
            children,
        );
        let tree_children = CFArrayCreateMutable(std::ptr::null(), 0, std::ptr::null());
        if !group.is_null() {
            // SAFETY: our array.
            CFArrayAppendValue(tree_children, group);
            // SAFETY: the group we created; the tree retains it.
            CFRelease(group);
        }
        // SAFETY: AUParameterTree's tree class method.
        let tree = msg_id_id(
            tree_cls.cast(),
            sel("createTreeWithChildren:"),
            tree_children,
        );
        // SAFETY: the arrays and strings we created.
        CFRelease(children);
        CFRelease(tree_children);
        CFRelease(group_id);
        CFRelease(group_name);
        tree
    }
}

/// The parameter-tree observer block: `(AUParameter *, AUValue)`.
unsafe extern "C" fn imp_value_observer(block: Id, parameter: Id, value: f32) {
    // SAFETY: the block is one of ours (built in `build_tree`), and the
    // instance it captured is alive for the tree's lifetime.
    unsafe {
        let obj = capture_of(block);
        // SAFETY: the parameter the host handed the block.
        let address = msg_u64(parameter, sel("address"));
        if let Some(state) = state_of(obj) {
            state.set_value(address, f64::from(value));
        }
    }
}

/// The parameter-tree stringer block:
/// `AUImplementorStringFromValueCallback` — `(AUParameter *, const
/// AUValue * __nullable)` → `NSString *`. The value arrives by
/// reference (null meaning "use the parameter's current value").
unsafe extern "C" fn imp_value_stringer(block: Id, parameter: Id, value: *const f32) -> Id {
    // SAFETY: as the observer.
    unsafe {
        let obj = capture_of(block);
        // SAFETY: the parameter the host handed the block.
        let address = msg_u64(parameter, sel("address"));
        // A null value pointer means "the parameter's current value";
        // the parameter is asked for it through its own `value` getter
        // (an `AUValue` — a float — comes back in the float return
        // register, which is why it gets its own typed send).
        let v = if value.is_null() {
            // SAFETY: the parameter the host handed the block.
            msg_f32(parameter, sel("value"))
        } else {
            *value
        };
        let Some(state) = state_of(obj) else {
            return std::ptr::null_mut();
        };
        let text = state.display(address, f64::from(v));
        // The caller owns what a stringer returns.
        nsstring(&text)
    }
}

unsafe extern "C" fn imp_internal_render_block(obj: Id, _sel: Sel) -> Id {
    // SAFETY: an initialised instance of ours.
    unsafe {
        let slot = slot_of(obj);
        if !(*slot).render_block.is_null() {
            return (*slot).render_block.cast();
        }
        let b = Box::into_raw(block(
            imp!(
                imp_render
                    as unsafe extern "C" fn(
                        Id,
                        *mut u32,
                        *const AudioTimeStamp,
                        Frames,
                        isize,
                        *mut AudioBufferList,
                        *const events::RenderEventHeader,
                        Id,
                    ) -> OsStatus
            ),
            obj,
        ));
        (*slot).render_block = b;
        b.cast()
    }
}

/// The render block — `AUInternalRenderBlock`, seven parameters, as
/// transcribed from `AUAudioUnitImplementation.h` (see the module doc):
/// action flags, timestamp, frame count, output bus, output buffers,
/// the realtime event list head, and the pull-input block.
///
/// # Safety
/// The host calls this with the buffers, event list and pull block it
/// owns; the block is one of ours, so the captured instance is
/// reachable.
unsafe extern "C" fn imp_render(
    block: Id,
    action_flags: *mut u32,
    timestamp: *const AudioTimeStamp,
    frame_count: Frames,
    _output_bus: isize,
    output_data: *mut AudioBufferList,
    realtime_events: *const events::RenderEventHeader,
    pull_input: Id,
) -> OsStatus {
    // SAFETY: the block is one of ours; the instance it captured is
    // alive while the host holds the block (the AU owns it).
    unsafe {
        let obj = capture_of(block);
        let Some(state) = state_of(obj) else {
            return NO_ERR;
        };
        if !state.is_allocated() || output_data.is_null() {
            zero_output(output_data, frame_count);
            return NO_ERR;
        }
        // The block's start sample time: the timestamp's own clock, or
        // the internal one when the host passes none.
        let start = if timestamp.is_null() {
            state.clock()
        } else {
            let ts = &*timestamp;
            if ts.mFlags & TS_SAMPLE_TIME_VALID != 0 {
                ts.mSampleTime as SampleTime
            } else {
                state.clock()
            }
        };
        let transport = musical(obj);
        state.render(
            start,
            frame_count,
            action_flags,
            timestamp,
            output_data,
            realtime_events,
            pull_input,
            transport,
        )
    }
}

/// Read the musical-context block, if the host installed one, into a
/// [`Transport`] — tempo and beats, the two this wrapper uses. The
/// out-parameter list is `AUHostMusicalContextBlock`'s own, in its own
/// order: tempo, time-signature numerator (`double *`), denominator
/// (`NSInteger *`), beat position, sample offset to the next beat
/// (`NSInteger *`), measure downbeat position.
///
/// # Safety
/// `obj` is an initialised instance of ours.
unsafe fn musical(obj: Id) -> Transport {
    let mut t = Transport::default();
    // SAFETY: our own instance; the property getter is AUAudioUnit's.
    let b = unsafe { msg_id(obj, sel_musical_context()) };
    if b.is_null() {
        return t;
    }
    let mut tempo = 0.0f64;
    let mut numerator = 0.0f64;
    let mut denominator = 0isize;
    let mut beats = 0.0f64;
    let mut offset = 0isize;
    let mut downbeat = 0.0f64;
    #[allow(
        clippy::missing_transmute_annotations,
        reason = "the symbol is variadic by nature"
    )]
    let f: unsafe extern "C" fn(
        Id,
        *mut f64,
        *mut f64,
        *mut isize,
        *mut f64,
        *mut isize,
        *mut f64,
    ) -> u8 = unsafe { core::mem::transmute((*b.cast::<Block>()).invoke) };
    // SAFETY: the host-installed musical-context block, called on the
    // render thread as its contract allows (it is documented as
    // real-time-safe by the header's own design).
    let ok = unsafe {
        f(
            b,
            &raw mut tempo,
            &raw mut numerator,
            &raw mut denominator,
            &raw mut beats,
            &raw mut offset,
            &raw mut downbeat,
        )
    };
    if ok != 0 && tempo > 0.0 {
        t.tempo = Some(tempo);
        t.beats = Some(beats);
    }
    t
}

unsafe extern "C" fn imp_allocate(obj: Id, _sel: Sel, error: *mut Id) -> u8 {
    // SAFETY: an initialised instance of ours.
    unsafe {
        let Some(state) = state_of(obj) else {
            if !error.is_null() {
                *error = ns_error(2, "no state (install was not called?)");
            }
            return 0;
        };
        if !state.allocate() {
            if !error.is_null() {
                *error = ns_error(3, "the plugin refused activation");
            }
            return 0;
        }
        #[cfg(feature = "gui")]
        {
            // The view exists alongside the audio: its panel attaches
            // here (main thread) and any edits it already queued go
            // through the tree's own apply path.
            for (id, value) in state.attach_view() {
                state.set_value(u64::from(id), value);
            }
        }
        1
    }
}

unsafe extern "C" fn imp_deallocate(obj: Id, _sel: Sel) {
    // SAFETY: an initialised instance of ours.
    unsafe {
        if let Some(state) = state_of(obj) {
            state.deallocate();
        }
    }
}

unsafe extern "C" fn imp_schedule_midi(obj: Id, _sel: Sel, list: *const c_void) {
    if list.is_null() {
        return;
    }
    // SAFETY: the host's list for this call; the walk reads only what
    // its own counts declare, within the documented guard.
    unsafe {
        let bytes = std::slice::from_raw_parts(list.cast::<u8>(), LIST_GUARD);
        let Some(state) = state_of(obj) else {
            return;
        };
        let _ = events::walk_event_list(bytes, |t, _group, b| {
            #[expect(clippy::cast_possible_wrap, reason = "a sample time")]
            state.schedule_midi(t as SampleTime, b);
        });
    }
}

unsafe extern "C" fn imp_request_view(obj: Id, _sel: Sel, handler: Id) {
    // The view story: no view controller — by name, with the reason,
    // exactly as the CLAP gui wiring refuses windowed apis. What exists
    // is the offscreen panel (crate::gui), whose frames a presenter
    // embeds; this completes the host's handler with nothing.
    let _ = obj;
    if !handler.is_null() {
        // SAFETY: the host's completion block, called once with NULL.
        unsafe { call_completion(handler, std::ptr::null_mut()) };
    }
}

unsafe extern "C" fn imp_full_state(obj: Id, _sel: Sel) -> Id {
    // SAFETY: an initialised instance of ours.
    unsafe {
        let Some(state) = state_of(obj) else {
            return std::ptr::null_mut();
        };
        let bytes = state.save();
        let key = nsstring(STATE_KEY);
        // SAFETY: our bytes and allocator.
        let data = CFDataCreate(
            std::ptr::null(),
            bytes.as_ptr(),
            isize::try_from(bytes.len()).unwrap_or(0),
        );
        // SAFETY: our allocator and the type-based callbacks.
        let dict = CFDictionaryCreateMutable(
            std::ptr::null(),
            0,
            (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
            (&raw const kCFTypeDictionaryValueCallBacks).cast(),
        );
        let mut out = std::ptr::null_mut();
        if !dict.is_null() && !data.is_null() && !key.is_null() {
            // SAFETY: our dictionary, key and data.
            CFDictionarySetValue(dict, key, data);
            out = dict;
            // SAFETY: the dictionary holds the data now.
            CFRelease(data);
        } else if !dict.is_null() {
            // SAFETY: ours, and being discarded.
            CFRelease(dict);
        }
        if !key.is_null() {
            // SAFETY: ours, and no longer held.
            CFRelease(key);
        }
        out
    }
}

unsafe extern "C" fn imp_set_full_state(obj: Id, _sel: Sel, dict: Id) {
    if dict.is_null() {
        return;
    }
    // SAFETY: the host's dictionary (toll-free bridged with our
    // CoreFoundation calls) and our key.
    unsafe {
        let key = nsstring(STATE_KEY);
        // SAFETY: the host's dictionary for this call.
        let data = CFDictionaryGetValue(dict, key);
        // SAFETY: ours.
        CFRelease(key);
        if data.is_null() {
            return;
        }
        // SAFETY: the value the host stored under our key.
        let len = usize::try_from(CFDataGetLength(data)).unwrap_or(0);
        let ptr = CFDataGetBytePtr(data);
        if ptr.is_null() || len == 0 {
            return;
        }
        // SAFETY: `len` bytes at `ptr`, per the data object.
        let bytes = std::slice::from_raw_parts(ptr, len);
        if let Some(state) = state_of(obj) {
            state.load(bytes);
        }
    }
}

unsafe extern "C" fn imp_latency(obj: Id, _sel: Sel) -> f64 {
    // SAFETY: an initialised instance of ours.
    unsafe {
        let Some(state) = state_of(obj) else {
            return 0.0;
        };
        // Bus formats are untranscribed (module doc): the rate is the
        // one the state activated at.
        f64::from(state.latency()) / 48_000.0
    }
}

unsafe extern "C" fn imp_dealloc(obj: Id, _sel: Sel) {
    // SAFETY: our instance, its slot, and the superclass's own dealloc.
    unsafe {
        let slot = slot_of(obj);
        if !(*slot).state.is_null() {
            drop(Box::from_raw((*slot).state));
            (*slot).state = std::ptr::null_mut();
        }
        if !(*slot).tree.is_null() {
            // SAFETY: the tree we created and own.
            CFRelease((*slot).tree);
        }
        for b in [(*slot).render_block, (*slot).observer, (*slot).stringer] {
            if !b.is_null() {
                drop(Box::from_raw(b));
            }
        }
        std::ptr::write(slot, SLOT_EMPTY);
        let sup = ObjcSuper {
            receiver: obj,
            super_class: class_getSuperclass(object_getClass(obj)),
        };
        // SAFETY: the superclass's dealloc, after our state is gone.
        super_void(&sup, sel("dealloc"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Gain;

    #[test]
    fn selectors_resolve_through_the_runtime() {
        // SAFETY: plain selector registration.
        let s = unsafe { sel_registerName(c"parameterTree".as_ptr()) };
        assert!(!s.is_null());
    }

    #[test]
    fn the_block_literal_layout_matches_the_abi() {
        // Block-ABI-03: the literal is one pointer, two ints, a
        // function pointer and a descriptor pointer, then the captures.
        assert_eq!(
            std::mem::size_of::<Block>(),
            4 * std::mem::size_of::<*const c_void>()
        );
        assert_eq!(
            std::mem::offset_of!(PtrBlock, capture),
            std::mem::size_of::<Block>()
        );
        assert_eq!(GLOBAL_DESCRIPTOR.size, std::mem::size_of::<PtrBlock>());
    }

    struct Probe {
        head: Block,
        hits: *const std::sync::atomic::AtomicUsize,
    }

    static PROBE_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
        reserved: 0,
        size: std::mem::size_of::<Probe>(),
    };

    unsafe extern "C" fn probe_invoke(block: Id) {
        // SAFETY: the block is the probe this test built.
        unsafe {
            let p = &*block.cast::<Probe>();
            (*p.hits).fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn a_built_block_calls_back_with_its_capture() {
        // SAFETY: our own literal, invoked by the runtime's convention.
        unsafe {
            let hit = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let probe = Box::new(Probe {
                head: Block {
                    isa: _NSConcreteGlobalBlock,
                    flags: BLOCK_IS_GLOBAL,
                    reserved: 0,
                    invoke: probe_invoke as *const c_void,
                    descriptor: &raw const PROBE_DESCRIPTOR,
                },
                hits: std::ptr::from_ref(&*hit),
            });
            #[allow(
                clippy::missing_transmute_annotations,
                reason = "the symbol is variadic by nature"
            )]
            let f: unsafe extern "C" fn(Id) = core::mem::transmute((*probe.cast::<Block>()).invoke);
            f(std::ptr::from_ref(&*probe).cast_mut());
            assert_eq!(hit.load(Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn install_registers_the_class_with_its_methods() {
        assert!(install::<Gain>("ViewwTestGainAU").is_ok(), "first install");
        assert!(install::<Gain>("ViewwTestGainAU").is_ok(), "idempotent");
        // SAFETY: the class we just registered answers these.
        unsafe {
            let cls = objc_getClass(c"ViewwTestGainAU".as_ptr());
            assert!(!cls.is_null(), "the pair is registered");
            for (name, _, _) in METHODS {
                let s = sel(name);
                assert!(!class_getInstanceMethod(cls, s).is_null(), "{name} landed");
            }
            let au = objc_getClass(c"AUAudioUnit".as_ptr());
            assert!(!au.is_null(), "AudioToolbox is linked");
            assert_eq!(class_getSuperclass(cls), au, "subclass of AUAudioUnit");
        }
    }
}
