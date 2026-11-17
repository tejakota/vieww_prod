//! CPU frames on Android: straight into the `ANativeWindow`'s own buffer.
//!
//! `softbuffer` works here too, but its Android backend hands out a fresh
//! `Vec<u32>` per frame, asks for `0x00RRGGBB` words and then swizzles them
//! back into the window's RGBA bytes on present — an allocation and three
//! full-frame passes, on the phones where a frame is 2.5 million pixels. The
//! rasterizer's output is already RGBA8, which is the window's own
//! `R8G8B8X8` layout byte for byte, so this copies rows and nothing else.
//!
//! **Only what each buffer is missing — and never Android's copy-back.** The
//! first version passed the frame's damage to `ANativeWindow_lock`, which then
//! copies the rest of the previous frame into the new buffer itself. Measured
//! on Device Farm (2026-10-16, same build, same phone, presented this way and
//! through Vulkan): present 7.1 ms here against 3.2 ms through Vulkan on a
//! 1440x3200 Galaxy S20. A window buffer is typically uncached memory, and
//! copy-back *reads* nearly all of it every frame. Now the whole buffer is
//! locked (no copy-back), each buffer is recognised by its address
//! (`BufferAges`), and only the regions changed since that buffer last held a
//! frame are written — writes only, which is what uncached memory is good at.
//!
//! **C-12: the copies run on the rasterizer's persistent pool.** They used to
//! spawn scoped threads per present and re-derive the core count per present;
//! both per-frame costs are gone — `present_cores` reads the count once and
//! `vieww_paint::native::raster_pool` keeps the workers alive, the same pool
//! the fill walk runs on. Pixels cannot move: whichever worker copies a row,
//! the row lands the bytes it always landed — `run_piece`'s own argument.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use ndk::hardware_buffer_format::HardwareBufferFormat;
use ndk::native_window::NativeWindow;
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use vieww_paint::native::raster_pool::run_chunks;
use winit::window::Window;

use super::{BufferAges, Changed, NativeError};

pub(super) struct CpuPresenter {
    native: NativeWindow,
    pub(super) extent: (u32, u32),
    ages: BufferAges,
}

fn device_error(message: impl std::fmt::Display) -> NativeError {
    NativeError::Device(format!("CPU present: {message}"))
}

/// The core count presents parallelise across, read once per process.
///
/// This used to be derived on every present — and `available_parallelism`,
/// which on Linux and Android opens and reads the process's cgroup files to
/// find a CPU quota, is not a cheap call: the same pattern
/// `geometry/fill.rs`'s `process_raster_thread_request` records for the
/// rasterizer, measured there at 3 `openat`, 8 `read` and 4 `statx` per call,
/// ~15 µs on an x86-64 desktop (2026-10-20) and several times that on a
/// Cortex-A53. Paid per present that is per frame, at 60-120 Hz, on the same
/// thread the frame has just rasterised on — for an answer that cannot change
/// while the process runs (the quota and the cores are fixed for a process's
/// lifetime), so exactly as the fill walk's request is, it is read once.
fn present_cores() -> usize {
    static CORES: OnceLock<usize> = OnceLock::new();
    *CORES.get_or_init(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    })
}

/// The present's row jobs for the pool: the frame the rows come from, the
/// flattened `(source byte index, destination address, bytes)` list, and how
/// many consecutive jobs one pool chunk covers.
///
/// Plain `usize`s again — the same shape the scoped version handed its
/// threads, so no raw-pointer `Send` dance: the addresses only become pointers
/// inside a chunk, where the copy's own SAFETY argument covers them, and
/// `&[u8]`/`&[(usize, usize, usize)]` are `Sync`, so the pool's `T: Sync`
/// bound is satisfied without a single `unsafe impl`.
struct RowWork<'a> {
    frame: &'a [u8],
    jobs: &'a [(usize, usize, usize)],
    piece: usize,
}

/// Run one pool chunk: `piece` consecutive row jobs from
/// `index * piece` — the same contiguous span `jobs.chunks(piece)` handed a
/// scoped thread, so a pool worker copies exactly the rows a spawned thread
/// would have: same rows, same bytes, same order within the piece. Chunks run
/// in any interleaving across the pool's workers, but never reordered in
/// outcome: every row job belongs to exactly one chunk, `run_chunks` runs
/// every chunk exactly once, and the jobs' rows are disjoint — so whoever
/// copies a row, that row gets the same bytes it always got.
fn run_piece(work: &RowWork, index: usize) {
    let start = index * work.piece;
    let end = (start + work.piece).min(work.jobs.len());
    for &(src, dst, len) in &work.jobs[start..end] {
        // SAFETY: `present` built each job as one row's span of the locked
        // window buffer — `dst` is `(y * stride + left) * 4` past the
        // buffer's base, with `y < height` and `left < right <= width <=
        // stride`, and the buffer is `stride * height * 4` bytes of the
        // 4-byte format checked at creation, so the span is in bounds;
        // `src` is the same row and span of our own `frame`, which cannot
        // overlap the window's buffer; and no two jobs write the same
        // bytes — the plan's regions are disjoint (`disjoint_regions`)
        // and `coalesce_regions` merges only exact unions — so the pool's
        // workers never race, whichever chunk each takes.
        unsafe {
            std::ptr::copy_nonoverlapping(work.frame.as_ptr().add(src), dst as *mut u8, len);
        }
    }
}

fn present_error(message: impl std::fmt::Display) -> NativeError {
    NativeError::Present(format!("CPU present: {message}"))
}

impl CpuPresenter {
    pub(super) fn new(window: &Arc<Window>, width: u32, height: u32) -> Result<Self, NativeError> {
        let handle = window.window_handle().map_err(device_error)?;
        let RawWindowHandle::AndroidNdk(handle) = handle.as_raw() else {
            return Err(device_error("not an Android window"));
        };
        // SAFETY: winit's handle points at this window's live `ANativeWindow`;
        // `clone_from_ptr` takes a reference of its own (`ANativeWindow_acquire`)
        // and `NativeWindow`'s `Drop` releases it.
        let native = unsafe { NativeWindow::clone_from_ptr(handle.a_native_window.cast()) };
        let mut presenter = Self {
            native,
            extent: (0, 0),
            ages: BufferAges::default(),
        };
        presenter.resize(width, height)?;
        // The copy below is byte for byte, so anything but a 4-byte RGBA
        // layout goes back to Vulkan rather than showing wrong colours.
        match presenter.native.format() {
            HardwareBufferFormat::R8G8B8X8_UNORM | HardwareBufferFormat::R8G8B8A8_UNORM => {
                Ok(presenter)
            }
            other => Err(device_error(format!(
                "window buffers are {other:?}, not RGBA8"
            ))),
        }
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) -> Result<(), NativeError> {
        if width == 0 || height == 0 || self.extent == (width, height) {
            return Ok(());
        }
        let w = i32::try_from(width).map_err(present_error)?;
        let h = i32::try_from(height).map_err(present_error)?;
        self.native
            .set_buffers_geometry(w, h, Some(HardwareBufferFormat::R8G8B8X8_UNORM))
            .map_err(present_error)?;
        self.extent = (width, height);
        self.ages.reset();
        Ok(())
    }

    pub(super) fn present(
        &mut self,
        frame: &[u8],
        extent: (u32, u32),
        changed: Changed,
    ) -> Result<Duration, NativeError> {
        self.resize(extent.0, extent.1)?;
        let (width, height) = (self.extent.0 as usize, self.extent.1 as usize);
        if frame.len() != width * height * 4 {
            // The rasterizer has not caught up with a resize; the next frame
            // repaints in full.
            return Ok(Duration::ZERO);
        }
        // Nothing changed: every buffer already shows this frame's pixels
        // where it matters, and posting nothing keeps the queue as it is.
        if matches!(&changed, Changed::Regions(regions) if regions.is_empty()) {
            return Ok(Duration::ZERO);
        }
        // The whole buffer: no copy-back (see the module docs). Timed: this
        // is where a full buffer queue makes the frame wait.
        let locking = Instant::now();
        let mut buffer = self.native.lock(None).map_err(present_error)?;
        let acquired = locking.elapsed();
        let cols = width.min(buffer.width());
        let rows = height.min(buffer.height());
        let stride = buffer.stride();
        let bits = buffer.bits().cast::<u8>();
        if cols != width || rows != height {
            // A buffer from before a resize: unknown contents, and this frame
            // fills what overlaps.
            self.ages.reset();
        }
        let plan = self.ages.plan(bits as usize, changed);
        let full = [(0, 0, cols as u32, rows as u32)];
        let regions: &[(u32, u32, u32, u32)] = plan.as_deref().unwrap_or(&full);
        // The row jobs, flattened: (source byte index, destination address,
        // bytes). Plain `usize`s, so the parallel copy below can hand them to
        // threads without a raw-pointer `Send` dance.
        let mut jobs: Vec<(usize, usize, usize)> = Vec::new();
        for &(l, t, r, b) in regions {
            let (left, right) = ((l as usize).min(cols), (r as usize).min(cols));
            let (top, bottom) = ((t as usize).min(rows), (b as usize).min(rows));
            if left >= right {
                continue;
            }
            for y in top..bottom {
                jobs.push((
                    (y * width + left) * 4,
                    (bits as usize) + (y * stride + left) * 4,
                    (right - left) * 4,
                ));
            }
        }
        // A window buffer is uncached memory, and the rows are independent —
        // several cores' pending stores fill its bandwidth in a way one
        // core's cannot. On the phones that need this most (a scroll's carry
        // plus damage is most of the screen) that is the difference between a
        // present that eats the frame's budget and one that does not.
        //
        // The gate is the scoped version's, kept as it was: serial at or
        // below 16 row jobs (16 rows of damage — under ~70 KB of copies on a
        // 1080-wide phone, less than a pool handoff is worth) and on
        // one-core devices. Above it the work goes to the rasterizer's
        // persistent pool (`vieww_paint::native::raster_pool`) instead of
        // `std::thread::scope`: the scoped form paid a thread spawn per
        // worker per present — per frame, at 60-120 Hz, up to seven spawns —
        // the same per-call cost the pool was built to take out of the fill
        // walk. Handing the pool a batch costs a mutex and a broadcast, tens
        // of microseconds (its own doc), against presents measured at
        // 0.2-3.8 ms across the Device Farm phones (PENDING.md §4.2,
        // 2026-10-16). The scoped form also scaled its worker count down for
        // mid-size presents, because every worker cost a spawn; the pool's
        // workers cost nothing per use, so above the gate every present asks
        // for all of them.
        let threads = present_cores().min(8);
        if threads <= 1 || jobs.len() <= 16 {
            // Serial, on this thread, below the gate — where the serial path
            // always was. One piece covering every job: `run_piece` walks them
            // in order, exactly as the `for_each` it replaces.
            run_piece(
                &RowWork {
                    frame,
                    jobs: &jobs,
                    piece: jobs.len(),
                },
                0,
            );
        } else {
            let piece = jobs.len().div_ceil(threads);
            run_chunks(
                &RowWork {
                    frame,
                    jobs: &jobs,
                    piece,
                },
                jobs.len().div_ceil(piece),
                threads,
                run_piece,
            );
        }
        // Dropping the guard unlocks and posts.
        drop(buffer);
        Ok(acquired)
    }
}
