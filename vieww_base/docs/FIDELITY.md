# Output fidelity

What vieww promises about the pixels it draws, and how each promise is
checked. Decided 2026-10-16: **byte-identical by default; tolerances only per
backend, and only written down here.**

## 1. The CPU rasterizer is the reference, and it does not drift

`vieww_paint::native::NativeRenderer` is the renderer every fixture, golden and
screenshot in this repository was drawn with. Its output is **byte-identical
across changes** unless a change says otherwise.

- A performance change (SIMD, threads, a cheaper blur, a new cache) must leave
  every byte where it was. "Visually the same" is not the bar; the same bytes
  are.
- A change that is *meant* to move pixels (a fixed antialiasing bug, a new
  default) says so in its commit message with a `Pixels-Changed:` trailer:
  what moved, and why that is better.
- **Gate:** `ci/check/raster-identity.sh [BASE]` renders the fixture gallery
  (`examples/fixtures`, every reference screen and animation) at `BASE` and at
  the working tree and compares the files byte for byte. It fails on any
  difference; `--allow-changed` turns that into a report, for a commit that
  carries the trailer. The 2026-10-16 blur speed-up (17.5 → 15.5 ms) was
  checked the same way by hand, every output identical.

Two identities hold with **no** exception, because a tolerance would hide
exactly the fault they exist for:

| Identity | Test |
|---|---|
| A retained, damaged repaint equals a full repaint, byte for byte | `crates/vieww-paint/tests/retained_equivalence.rs` |
| A frame drawn in tiles or threads equals the same frame drawn in one pass | to be added with multi-threaded raster (roadmap 2.1), before it is enabled |

## 2. Every other backend has its own tolerance against the reference

A second rasterizer is checked against the CPU one, not against stored
images, and each declares how close it must be. A tolerance belongs to one
backend and one kind of content; there is no global one.

| Backend | Content | Bound against the CPU reference | Test |
|---|---|---|---|
| Vulkan `SceneRenderer` | filled and stroked shapes | **interior** pixels within 2/255 per channel, none over; edge pixels free (analytic scanline coverage against triangulation plus hardware sampling disagree by design only there) | `crates/vieww-hal/tests/vulkan_scene.rs` |
| Vulkan `SceneRenderer` | text | **every** pixel within 1/255, rims included (one glyph rasterizer; the atlas stores coverage as `u8`) | `crates/vieww-hal/tests/vulkan_text.rs` |
| Vulkan compositor | layers, group opacity, blur, backdrop blur, colour matrix, shaped clips, shadows, transparent clear, winding, dashed strokes | **every** pixel within 2/255, none over | `crates/vieww-hal/tests/vulkan_compositor.rs` |
| Metal `SceneRenderer` | filled and stroked shapes, translucent overlaps, group-opacity layers | **interior** pixels within 2/255 per channel, none over; edge pixels free (the Vulkan rule, verbatim) | `crates/vieww-hal/tests/metal_scene.rs` (hosted macOS, paravirtual GPU) |
| D3D12 `SceneRenderer` | filled and stroked shapes, translucent overlaps, group-opacity layers, retained damage | **interior** pixels within 2/255 per channel, none over; edge pixels free (the Vulkan rule, verbatim) | `crates/vieww-hal/tests/d3d12_scene.rs` (hosted Windows, WARP) |
| WebGPU | — | not declared; same rule | — |

Where they run: lavapipe in ordinary CI, real GPUs in `ci/hardware/linux.sh`
and `ci/hardware/windows.ps1` (`cargo test -p vieww-hal --features vulkan --
--ignored`). Phones are not covered yet; Device Farm runs the device suite,
not these parity tests.

### The parity certificate (FW-26)

The rows above are a contract, and each CI run measures it. When the GPU
suites run with `VIEWW_PARITY_OUT` set, every comparison against the CPU
oracle appends a row: backend, adapter, test, contract, interior
violations, and the worst channel delta anywhere. The rows are written by
`crates/vieww-hal/tests/support/parity_row.rs`.
`ci/check/parity-certificate.py` merges them into
`parity-certificate-<leg>.json` and fails the leg if any row broke its
contract. The `framework-checks` Vulkan (lavapipe), Metal (paravirtual)
and D3D12 (WARP) legs each upload theirs, and the table is written to the
run summary. A release attaches the certificates of the commit it was cut
from. Real-GPU rows come from the self-hosted runners, through the same
script.

## 3. Changing a bound

A bound is loosened only by editing its row in the same commit as the test,
with the reason in the commit message — never by a constant changing alone.
Tightening needs no ceremony.

## 4. Why this matters more now

With `App::auto_rasterizer` the device, not the developer, picks the
rasterizer: the same build draws on the CPU on one phone and on the GPU on the
next. The bounds in section 2 are therefore also the bound on how different
two users' screens may look for the same build.
