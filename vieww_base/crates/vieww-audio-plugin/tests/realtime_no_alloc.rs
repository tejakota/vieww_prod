//! AU-18: a plugin `Driver` renders without allocating — events out of
//! order, parameter changes inside the block, many blocks.
//!
//! No test harness (`harness = false`): the counter sees every thread, so
//! the process holds nothing else (see vieww-audio's realtime_no_alloc).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use vieww_audio_plugin::{
    Buffers, Driver, Event, Kind, ParamChange, ParamInfo, Parametric, Plugin, PluginInfo, Status,
    Transport,
};

struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged; the only
// addition is an atomic counter, which cannot affect the returned memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System.alloc` via `alloc` above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

static INFO: PluginInfo = PluginInfo {
    id: "dev.vieww.test-rt",
    name: "RT Test",
    vendor: "vieww",
    url: "https://vieww.dev",
    version: "1.0.0",
    description: "A gain that counts notes, for the no-alloc harness",
    kind: Kind::Effect,
    inputs: 2,
    outputs: 2,
    note_input: true,
    vst3_class_id: *b"vieww-rt-test!!!",
    features: &["utility"],
};

/// A gain that also counts the note events it is handed: allocation-free
/// itself, so anything the counter sees is the Driver's.
#[derive(Default)]
struct RtGain {
    gain: f64,
    notes: u64,
}

impl Parametric for RtGain {
    fn param_info(&self) -> Vec<ParamInfo> {
        vec![ParamInfo::new(1, "Gain", 0.0, 2.0, 1.0)]
    }
    fn get_param(&self, id: u32) -> Option<f64> {
        (id == 1).then_some(self.gain)
    }
    fn set_param(&mut self, id: u32, value: f64) {
        if id == 1 {
            self.gain = value;
        }
    }
}

impl Plugin for RtGain {
    fn info() -> &'static PluginInfo {
        &INFO
    }
    fn create() -> Self {
        Self {
            gain: 1.0,
            notes: 0,
        }
    }
    fn process(&mut self, b: &mut Buffers<'_, '_, '_>, events: &[Event], _: &Transport) -> Status {
        self.notes += events.len() as u64;
        #[expect(clippy::cast_possible_truncation, reason = "audio")]
        let g = self.gain as f32;
        for ch in 0..b.output_channels() {
            let n = b.frames();
            for i in 0..n {
                let x = b.input(ch).get(i).copied().unwrap_or(0.0);
                b.output(ch)[i] = x * g;
            }
        }
        Status::Continue
    }
}

fn main() {
    const FRAMES: usize = 256;
    let mut driver: Driver<RtGain> = Driver::new();
    let input = vec![0.25f32; FRAMES];
    let mut left = vec![0.0f32; FRAMES];
    let mut right = vec![0.0f32; FRAMES];

    let mut block = |driver: &mut Driver<RtGain>, k: u32| {
        // 300 note events, pushed in reverse frame order (a host may send
        // them unsorted), and changes inside the block: enough that the
        // standard stable sort would reach for heap scratch.
        for i in (0..300u32).rev() {
            let time = (i * 7 + k) % FRAMES as u32;
            let key = u8::try_from(i % 128).unwrap_or(0);
            driver.push_event(Event::NoteOff {
                time,
                channel: 0,
                key,
                velocity: 0.5,
                note_id: -1,
            });
        }
        for t in [200u32, 17, 128, 64] {
            driver.push_change(ParamChange {
                time: t,
                id: 1,
                value: f64::from(t) / 256.0,
            });
        }
        let inputs: [&[f32]; 2] = [&input, &input];
        let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
        driver.run(&inputs, &mut outputs, FRAMES)
    };

    // Warm-up: anything lazily sized happens here, not on the clock.
    block(&mut driver, 0);
    let before = driver.plugin.notes;
    ARMED.store(true, Ordering::SeqCst);
    for k in 1..=500 {
        block(&mut driver, k);
    }
    ARMED.store(false, Ordering::SeqCst);
    let allocs = ALLOCS.load(Ordering::SeqCst);
    assert_eq!(
        driver.plugin.notes - before,
        500 * 300,
        "every event was delivered"
    );
    assert_eq!(
        allocs, 0,
        "the Driver's render path allocated {allocs} times"
    );
    println!("realtime_no_alloc: 500 blocks through a plugin Driver (300 unsorted events and 4 parameter changes each) allocated nothing");
}
