# Redmi Note 7 Pro (Medium tier): PERF-01 / PERF-02 findings

Measured over USB with `examples/market-watch/usb/run.sh` (Android 16, 3.5 GiB, 8 cores).

| Path | Result | Reading |
|------|--------|---------|
| CPU rasteriser, serial | storm raster about 24 ms, misses 16.7 ms | The Medium tier cannot hold 60 fps on the CPU path under load. |
| CPU rasteriser, 4 threads | slower than serial | big.LITTLE: work lands on little cores. Serial stays the Medium default. |
| GPU (Vulkan, FIFO) | holds 60 fps, GPU-bound | Slow cold first frame and one 46 ms scroll hitch; probably a cold pipeline cache. Run scenario 2 twice to confirm. |

## PERF-01 (CPU speed-up): not closed

A local callgrind profile of the storm phase is flat. Paths and sparklines dominate on x86, not rects.
No change was found that keeps the output byte-identical (the CPU path is the fidelity reference, docs/FIDELITY.md).
Options, each needing a decision:

1. Let `auto_rasterizer` prefer the GPU path on Medium handhelds (no pixel change on the CPU path).
2. A CPU-path resolution scale for handheld Medium/Low (pixels change; needs a `Pixels-Changed:` line).
3. NEON paths for the premultiplied composite (needs a device to measure).

## PERF-02

- The swapchain is already FIFO, so "unpaced present" is stale.
- Per-tier cache budgets exist (`quality::cache_budgets_for`).
- Open: GPU pipeline-cache warm-up for the first frame and first scroll, and incremental present.

## Cold start

`since_process_start_ms` returned null on every phone because it read `/proc/uptime`. It now reads `CLOCK_BOOTTIME`
directly. Unverified until the next device run.
