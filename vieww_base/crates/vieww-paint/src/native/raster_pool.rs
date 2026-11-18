//! A persistent pool of raster workers: the spawn-free form of the
//! row-parallel scanline walk.
//!
//! # Why a pool, and why now
//!
//! `geometry/fill.rs` splits a mask's rows across `std::thread::scope`
//! workers, which is correct and free of shared state — but each mask pays
//! a thread spawn per worker, so the gate that keeps that worthwhile
//! (`RASTER_MIN_PARALLEL_DIM`, 128 device pixels on both axes) excludes
//! exactly the masks a storm of live content rasterises: a 2.75x-density
//! sparkline's stroke coverage is about 198×99, under the gate, serial.
//! Thirteen of those a frame is the biggest CPU cost of a market-watch
//! storm on both render paths — the CPU rasteriser's walk and the GPU
//! planner's mask build run the very same code.
//!
//! A pool that keeps its workers alive removes the spawn from the per-mask
//! cost, which lets the gate come down to where those masks qualify. The
//! workers are plain condvar loops; handing them a batch costs a mutex and
//! a broadcast — tens of microseconds against walks measured in hundreds.
//!
//! # The scoped pattern, and its one safety argument
//!
//! The row-span jobs borrow the mask's edge table, which no `'static` job
//! queue can hold. The pool therefore offers a *scoped* run: the
//! submitting thread puts one pointer to a stack `Job` — the borrowed data
//! plus the per-chunk function — into the pool, works chunks itself, and
//! then blocks until every chunk has run. The safety argument is the one
//! crossbeam's scoped threads rest on: the borrow is still alive because
//! its owner is blocked inside the call, and no worker touches the pointer
//! after its chunk returns — a worker reads the `Job` only while running a
//! chunk of the batch, and the owner does not return until the batch's
//! `remaining` counter reaches zero, which a worker bumps only after its
//! chunk has returned.
//!
//! # Parallelism policy
//!
//! The pool is created lazily on the first multi-thread request, sized by
//! that request (the platform layer's `set_process_raster_threads`, read
//! once per process); a request of one runs everything on the calling
//! thread, exactly as before. Desktops keep the core count — a large
//! window's full repaint is the long raster the walk was built for.
//!
//! Since C-12 the platform layer's Android CPU present routes its row
//! copies through `run_chunks` as well — the same rows through the same
//! workers, no spawn per present. Two callers means the "first multi-thread
//! request" that sizes the pool is whichever of them goes parallel first;
//! a later call with a smaller request simply finds its chunks claimed
//! sooner. That is safe because the pool's contract is per-call: every
//! chunk of *this* batch runs exactly once before *this* call returns —
//! the invariant both callers' SAFETY arguments rest on — and neither
//! request changes while the process runs, so whichever sized the pool,
//! it sized it once and forever.

use std::sync::{Condvar, Mutex, OnceLock};

/// Issues batch identities: a submitter only claims and retires chunks of
/// the batch *it* installed. See `Batch::id` for the race that makes that
/// check load-bearing.
static BATCH_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// One batch of chunks over a caller's stack `Job`.
struct Batch {
    /// Which submission this is — `BATCH_IDS`'s counter at install. The
    /// submitting thread keeps the id of its own batch and claims and
    /// decrements only against it, because a batch can be drained and a
    /// *foreign* one installed in the window between the submitter's own
    /// last chunk finishing and its next claim: without the check, the
    /// submitter would take the foreign batch's next index, run **its own**
    /// `data` at that index, and decrement the foreign batch's `remaining`
    /// for a chunk that never ran on the foreign data — the foreign batch
    /// would retire with a span of its output missing. Found by
    /// `concurrent_submitters_get_their_own_chunks`, which fails within
    /// seconds without this field on a 2-core machine.
    id: u64,
    /// The caller's `Job`, on the submitting thread's frame.
    job: *const (),
    /// How many chunks the batch counts, `0..count`.
    count: usize,
    /// Runs one chunk: the `Job` pointer and the chunk's index.
    run: unsafe fn(*const (), usize),
    /// The next chunk index to claim; workers take it under the mutex.
    next: usize,
    /// How many chunks have not finished. The batch is over at zero.
    remaining: usize,
}

// SAFETY: `Batch`'s raw `job` pointer points into the submitting thread's
// frame, which is blocked inside `scoped` for exactly as long as the batch
// is installed — the pointer is valid on every thread that reads it, and
// no thread reads it after its chunk returns (the batch is uninstalled
// only once every chunk has).
unsafe impl Send for Batch {}

/// The pool's shared state. One batch is in flight at a time; a submit
/// from a second thread waits for the current batch to drain first.
struct State {
    batch: Option<Batch>,
    /// Set at teardown; workers exit when they see it.
    shutdown: bool,
}

// SAFETY: workers read the installed batch only while its owner is blocked
// inside `scoped`, and `shutdown` is a plain bool.
unsafe impl Sync for State {}

struct Pool {
    state: Mutex<State>,
    /// Signalled when a batch is submitted or on shutdown.
    work: Condvar,
    /// Signalled when a batch's last chunk finishes.
    finished: Condvar,
}

/// The process-wide pool, started once with the first multi-thread
/// request's worker count.
static POOL: OnceLock<Pool> = OnceLock::new();

impl Pool {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                batch: None,
                shutdown: false,
            }),
            work: Condvar::new(),
            finished: Condvar::new(),
        }
    }

    /// Start `workers - 1` loops; the submitting thread is the last worker.
    fn start_workers(&'static self, workers: usize) {
        for worker in 0..workers.saturating_sub(1) {
            let (state, work, finished) = (&self.state, &self.work, &self.finished);
            let _ = std::thread::Builder::new()
                .name(format!("vieww-raster-{worker}"))
                .spawn(move || {
                    let mut guard = state.lock().expect("raster pool worker");
                    loop {
                        if guard.shutdown {
                            return;
                        }
                        let claimed = guard.batch.as_mut().and_then(|batch| {
                            let index = batch.next;
                            (index < batch.count).then(|| {
                                batch.next += 1;
                                index
                            })
                        });
                        let Some(index) = claimed else {
                            guard = work.wait(guard).expect("raster pool worker");
                            continue;
                        };
                        let (job, run) = {
                            let batch = guard
                                .batch
                                .as_ref()
                                .expect("a claimed index means a batch is installed");
                            (batch.job, batch.run)
                        };
                        drop(guard);
                        // SAFETY: the `Job` lives on the submitting
                        // thread's frame, which is blocked in `scoped`
                        // until every chunk of this batch — this one
                        // included — has returned.
                        unsafe { run(job, index) };
                        guard = state.lock().expect("raster pool worker");
                        let done = {
                            let batch = guard
                                .batch
                                .as_mut()
                                .expect("a claimed index means a batch is installed");
                            batch.remaining -= 1;
                            batch.remaining == 0
                        };
                        if done {
                            guard.batch = None;
                            finished.notify_all();
                        }
                    }
                });
        }
    }
}

/// Run `count` chunks of work across `workers` threads, this one included:
/// `run` sees the borrowed `data` and the chunk's index, and every chunk
/// runs exactly once before this returns.
///
/// # Panics
///
/// A panic inside `run` on a worker unwinds into the worker's loop and
/// kills it, and the batch it was part of never finishes — the submitting
/// thread blocks. The walks this serves are panic-free by construction:
/// their maths is infallible and their allocations abort the process. The
/// same holds for the `std::thread::scope` walk this replaces, which
/// propagates a worker panic to the caller instead.
///
/// # Safety
///
/// `run` must not hand `data` anywhere that outlives this call.
unsafe fn scoped<T: Sync>(data: &T, count: usize, workers: usize, run: fn(&T, usize)) {
    if workers <= 1 || count == 0 {
        for index in 0..count {
            run(data, index);
        }
        return;
    }
    let pool = POOL.get_or_init(Pool::new);
    // Started once, on the first multi-thread request's count; the request
    // is process-wide, so the pool never wants a different size.
    static WORKERS: OnceLock<()> = OnceLock::new();
    WORKERS.get_or_init(|| pool.start_workers(workers));

    /// Invoke one chunk of a `Job<'a, T>` — a real `fn` item, monomorphised
    /// per `T`, so the pool can hold it as a plain pointer-sized function.
    ///
    /// # Safety
    ///
    /// `job` must point at a `Job` whose borrow outlives the batch.
    unsafe fn trampoline<T: Sync>(job: *const (), index: usize) {
        // SAFETY: the caller of `scoped` guarantees the Job's lifetime.
        let job = &*(job as *const Job<'_, T>);
        (job.run)(job.data, index);
    }

    struct Job<'a, T: Sync> {
        data: &'a T,
        run: fn(&'a T, usize),
    }
    let job = Job { data, run };

    let mut guard = pool.state.lock().expect("raster pool submit");
    // A second thread's batch drains first: the raster walk is
    // single-threaded call-side; this is the belt to that braces.
    while guard.batch.is_some() {
        guard = pool.finished.wait(guard).expect("raster pool submit waits");
    }
    let id = BATCH_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    guard.batch = Some(Batch {
        id,
        job: &job as *const Job<'_, T> as *const (),
        count,
        run: trampoline::<T>,
        next: 0,
        remaining: count,
    });
    drop(guard);
    pool.work.notify_all();

    // The submitting thread works too — it is one of the pool's workers for
    // the length of this call, but **of its own batch only**: a claim or a
    // retirement against whatever happens to be installed could land on a
    // foreign batch installed in the window after this one drained (see
    // `Batch::id`), running this call's data at the foreign batch's index
    // and retiring a chunk that never ran there.
    loop {
        let index = {
            let mut guard = pool.state.lock().expect("raster pool submit");
            match guard.batch.as_mut() {
                Some(batch) if batch.id == id && batch.next < batch.count => {
                    let index = batch.next;
                    batch.next += 1;
                    index
                }
                _ => break,
            }
        };
        run(data, index);
        let mut guard = pool.state.lock().expect("raster pool submit");
        let done = match guard.batch.as_mut() {
            Some(batch) if batch.id == id => {
                batch.remaining -= 1;
                batch.remaining == 0
            }
            // Not ours: ours drained while this chunk ran — some other
            // thread took the last retirement. Nothing left to wait for.
            _ => return,
        };
        if done {
            guard.batch = None;
            pool.finished.notify_all();
            return;
        }
    }
    // Every chunk of ours claimed: wait for the workers' last ones — which
    // is to say, wait while *our* batch is still the one installed.
    let mut guard = pool.state.lock().expect("raster pool submit");
    while guard.batch.as_ref().is_some_and(|batch| batch.id == id) {
        guard = pool.finished.wait(guard).expect("raster pool submit waits");
    }
}

/// The row-parallel walk's pool entry point: `count` chunks, each seeing
/// its own index in `0..count`, across `workers` threads including this
/// one.
///
/// This is the pool's only safe public face — the seam the fill walk uses
/// and, since C-12, the platform layer's Android CPU present. The chunks'
/// shared data is the caller's borrow; the pool's scoped argument is what
/// makes pointing workers at it sound.
pub fn run_chunks<T: Sync>(data: &T, count: usize, workers: usize, run: fn(&T, usize)) {
    // SAFETY: `run` is the caller's; the pool does not store it beyond the
    // batch, which this call outlives, and the data borrow is the caller's
    // own, alive for the whole call.
    unsafe { scoped(data, count, workers, run) };
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn every_chunk_runs_exactly_once_on_many_threads() {
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        static SUM: AtomicUsize = AtomicUsize::new(0);
        RUNS.store(0, Ordering::SeqCst);
        SUM.store(0, Ordering::SeqCst);
        struct Tally;
        fn run(_tally: &Tally, index: usize) {
            RUNS.fetch_add(1, Ordering::SeqCst);
            SUM.fetch_add(index, Ordering::SeqCst);
        }
        let count = 37;
        super::run_chunks(&Tally, count, 4, run);
        assert_eq!(RUNS.load(Ordering::SeqCst), count);
        assert_eq!(SUM.load(Ordering::SeqCst), (0..count).sum::<usize>());
    }

    #[test]
    fn one_worker_runs_every_chunk_here() {
        static RUNS: AtomicUsize = AtomicUsize::new(0);
        RUNS.store(0, Ordering::SeqCst);
        struct Solo;
        fn run(_solo: &Solo, _index: usize) {
            RUNS.fetch_add(1, Ordering::SeqCst);
        }
        super::run_chunks(&Solo, 11, 1, run);
        assert_eq!(RUNS.load(Ordering::SeqCst), 11);
    }

    #[test]
    fn two_batches_back_to_back_both_complete() {
        static SEEN: AtomicUsize = AtomicUsize::new(0);
        SEEN.store(0, Ordering::SeqCst);
        struct Marker;
        fn run(_marker: &Marker, _index: usize) {
            SEEN.fetch_add(1, Ordering::SeqCst);
        }
        super::run_chunks(&Marker, 9, 3, run);
        super::run_chunks(&Marker, 5, 3, run);
        assert_eq!(SEEN.load(Ordering::SeqCst), 14);
    }

    /// Eight submitters at once, each verifying every chunk of its own
    /// batch landed its own data. This is the test `Batch::id` exists for:
    /// before the id check, a submitter whose batch drained while it ran
    /// its last chunk would claim from the next batch installed, run *its
    /// own* data at that index, and retire a chunk the foreign batch never
    /// ran — one of the foreign batch's chunks simply missing from its
    /// output. It failed within seconds on a 2-core machine, and it is the
    /// pool's only test that exercises `scoped` from more than one thread
    /// at a time — which is also why nothing else caught it: the studio's
    /// repaint tests were its first concurrent callers, flaking about two
    /// runs in five until `VIEWW_RASTER_THREADS=1` isolated the pool.
    #[test]
    fn concurrent_submitters_get_their_own_chunks() {
        const SUBMITTERS: usize = 8;
        const CHUNKS: usize = 1024;
        const ROUNDS: usize = 300;
        let want: Vec<Vec<usize>> = (0..SUBMITTERS)
            .map(|submitter| (0..CHUNKS).map(|i| i * 31 + submitter).collect())
            .collect();
        struct Job<'a> {
            want: &'a [usize],
            got: &'a [AtomicUsize],
        }
        fn run(job: &Job, index: usize) {
            job.got[index].store(job.want[index], Ordering::Relaxed);
        }
        std::thread::scope(|scope| {
            for want in &want {
                scope.spawn(move || {
                    for round in 0..ROUNDS {
                        let got: Vec<AtomicUsize> =
                            (0..CHUNKS).map(|_| AtomicUsize::new(usize::MAX)).collect();
                        let job = Job { want, got: &got };
                        super::run_chunks(&job, CHUNKS, 4, run);
                        for (index, cell) in got.iter().enumerate() {
                            assert_eq!(
                                cell.load(Ordering::Relaxed),
                                want[index],
                                "round {round}: chunk {index} never ran its own data"
                            );
                        }
                    }
                });
            }
        });
    }
}
