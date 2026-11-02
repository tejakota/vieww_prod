# Vieww Quality Contracts

This document turns the premium-framework ambition into checks that can fail a build.

## Visual completeness

A GPU scene is considered renderable only when `vieww_gpu::ScenePlan::is_complete()` is true. The planner must never silently discard a command.

Every `vieww_paint::Command` now plans on the GPU: fills, strokes, transforms, per-fragment gradients, images (rotated, mip-blended), monochrome and colour text, rectangular and shaped clips, offscreen layers with group opacity, all 28 blend modes, layer and backdrop filters, and outer/inset/transformed shadows. `ScenePlan::unsupported` is reached only by resource limits (atlas maxima, layer depth). Geometry edge antialiasing on the GPU is the remaining visual gap — see `docs/GPU-RENDERER-STATUS.md`.

`unsupported_gpu_commands` in a certification is **measured** — summed from real `ScenePlan`s by `test-gpu-work` and by `examples/vieww-standard` — never written as a literal.

## Frame contract

`vieww_render_planner::QualityContract` defines refresh-specific frame deadlines, zero steady-state allocation targets, zero clean-scene rebuilds, zero unsupported GPU commands, a 2% visual drift ceiling, and one-refresh input-latency ceiling.

`steady_allocations` is read by a counting `#[global_allocator]` in `examples/vieww-standard`, over steady frames through the retained present path (`FrameDriver::draw_frame_at` + `NativeRenderer::render_retained_in_place`). The runner refuses to report a number if the allocator is not installed (a dynamically linked `std` ignores the override and would read zero).

The certification scripts collect measurements and test output from real machines. A passing unit test alone does not certify device performance.

## Typography contract

`vieww-text` must keep shaping, fallback, layout and glyph caching deterministic for the framework's embedded UI coverage. System fallback is allowed outside that coverage. GPU text uses the same native glyph coverage as the reference renderer and the Vulkan parity suite remains the visual oracle.

The remaining typography work is application-face/variable-font/color-glyph validation, not a replacement shaping stack.

## Antialiasing contract

Rasterised coverage is put through one perceptual transfer — `vieww_paint::native::linear::coverage_alpha`, the square root, the standard coverage gamma Skia-style renderers apply to **all** antialiased coverage (text, vector shapes, gradients, LCD sub-columns) — before it becomes alpha, on every path: the CPU compositors (`composite_coverage`, `composite_flat`, `composite_lcd`, the gradient loop), the glyph atlas (`InkedGlyph::alpha`), and the GPU seam's byte export (`gpu_seam::to_byte`). One shared function means the CPU, the GPU atlas and the reference oracle can never drift apart on the transfer itself; the existing 1/255 parity bounds are unchanged, they simply now bound a gamma-corrected pixel. Endpoints are fixed points (full stays full, empty stays empty), so opaque fast paths and the byte-exact interior are untouched. Subpixel (RGB-stripe) coverage remains the opt-in `AaMode::Lcd`, correct only on straight-RGB desktop panels and never assumed on PenTile/diamond OLEDs.

## Platform contract

Linux and Windows certification (`ci/certify/certify.sh`, `ci/certify/certify-windows.ps1`) run every suite in the tree — workspace tests, GPU planner and Vulkan suites, the GPU workload and fixture census, the premium UI, all six stress/fidelity suites, `vieww-standard`, and the web build + byte-for-byte browser verification. A stage the machine cannot run is recorded as `SKIPPED(reason)`, the summary's `COMPLETE=` line says whether anything was skipped, and `VIEWW_CERT_STRICT=1` / `-Strict` fails the run on any skip. Evidence is written under `target/` and carries a source digest. OS-specific backend results are labeled independently; Windows D3D12 build/bring-up is not upgraded to a rendering claim unless the backend actually renders and passes its tests.
