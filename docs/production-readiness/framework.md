# vieww framework — Production-Readiness Deep Research (Task 2-a)

## 0. Method & verification notes

- **Code-first, static only.** No `cargo build`/`test` was run (653K LOC). Every "exists" claim below was verified by reading source under `/home/z/my-project/vieww_artifacts/vieww_base/` on `main` (98367be) and, where noted, on `origin/feature/main_fixes` (60adf6f, 13 commits, +8,915/−1,035 lines across 35 files).
- **Honesty system cross-checked.** `vieww_base/TRACKER.md` (2,308 lines) and `PENDING.md` (1,076 lines) were read in full and spot-verified against code. Two stale claims were found and are called out (FW-02: "B9 Fixed"; the guide's `layout.md` link).
- **Counting.** "5003 tests" = `#[test]` occurrences under `vieww_base/crates/*/src` + `*/tests`. Unsafe counts are `grep -c "unsafe"` per crate (vieww-hal: 283 sites, all in Vulkan/D3D12/Metal FFI; vieww-platform-winit: 60; vieww-plugin: 34; vieww-paint: 16; vieww-reload: 4; vieww-render/element/widget/text/scene/foundation/accessibility/runtime/build/cli: **0**).
- **CI audit** read every workflow at the repo root (9 files), the 4 duplicates under `vieww_base/.github/workflows/`, and the 7 unrestored files in `.github/workflows-pending/`, plus every script in `vieww_base/ci/{check,certify,hardware,mobile,release,tools}/`.
- **Leader research** via live fetch of Flutter docs (Impeller, web renderers, deployment, testing), Shorebird, Expo EAS Update/Submit, React Native (new arch, Fast Refresh, Hermes), Sentry, Slint, Dioxus, iced, libcosmic, Makepad, Fastlane, Apple notarization docs — URLs listed in §8; all returned HTTP 200 at research time unless noted.
- **Test-infrastructure mapping.** Every proposed test maps to exactly one of: GitHub-hosted CI / self-hosted Linux (`vieww-linux` runner, Kaby Lake i5-7200U) / self-hosted Windows (`vieww-windows`, same laptop under Windows 10) / device farm (AWS Device Farm, Android + iOS, driven by `.github/workflows/device-farm.yml`).

---

## 1. Executive verdict

| # | Dimension | Score | Justification (evidence) |
|---|---|---|---|
| 1 | API stability, versioning, crates.io | **3/10** | Workspace `version = "0.1.0"` (`vieww_base/Cargo.toml:115`); **zero crates published** (checklist B16: scaffolded projects resolve `vieww = "0.0.1"` which does not exist on crates.io); `repository` field deliberately absent (Cargo.toml:144-155); no CHANGELOG (B4); no semver/deprecation policy doc. Internal layering *is* disciplined (sealed `Device` trait, `vieww-hal/src/lib.rs:78-88`). |
| 2 | Platform parity | **5/10** | Linux: fully real (Vulkan real-GPU, receipts). Windows: D3D12 SceneRenderer **only on the branch** (`vieww-hal/src/d3d12/scene.rs`, 1,478 lines, untested on real D3D12 hardware); startup 840 ms vs Linux 221 ms (PENDING §3.5). macOS/iOS: Metal SceneRenderer **only on the branch** (`metal/scene.rs`, 811 lines); live path = Vulkan-on-MoltenVK, GPU path ~16 ms pacing-bound on all iPhones (PENDING §1.4). Android: real (farm, 3 phones + more), GPU path not default. Web: canvas parity on Chromium/Firefox/WebKit; **no WebGPU** (`vieww-platform-web/src/lib.rs:86` anticipates it). |
| 3 | Performance budgets & regression gates | **6/10** | Genuinely gated: device suite `frames_inside_their_budget` + `known-failures.txt` (ci/mobile/device-suite.sh:307); coverage floor 70% (`ci/check/coverage.sh:43`); size budget (`ci/check/size-budget.sh`, 593,128 B); raster byte-identity (`raster-identity.sh`). Not gated: hardware receipts are recorded but never **trend-compared**; 2 fixtures still over 60 Hz (PENDING §4.1); animation-stress 35 ms p95 on the reference laptop (B7). |
| 4 | Testing depth | **7/10** | 5,003 test fns; CPU↔GPU pixel parity suites at 1–2/255 tolerance (`vulkan_scene/text/compositor.rs`); mutation-checked parity (PENDING §1.7); panic-boundary tests (`vieww-render/tests/panic_boundary.rs`); `assert_partial_repaint_is_complete`. Missing: fuzzing beyond audio decode (`vieww-audio/fuzz/` only), no property-based testing of scene/element layers, no randomized desktop soak. |
| 5 | Error handling & crash resilience | **6/10** | `ErrorPolicy` Placeholder/Custom/Propagate (`vieww-element/src/error.rs`), panic boundary in `ElementTree::build`, crash reporter with `FileSink` + pending-replay (`vieww-platform-winit/src/crash.rs`), plugin trampolines catch where possible. But `dist` profile aborts (Cargo.toml:338-343) so the placeholder red-box cannot catch in shipped builds; no crash transport off-device (FW-12). |
| 6 | Accessibility | **5/10** | AccessKit bridge (`a11y.rs`), Android bridge + TalkBack heard + automated suite (`a11y-android.sh`), WCAG contrast + tree audit crate (`vieww-accessibility/src/{audit,contrast,theme_audit}.rs`), focus manager with traps/traversal (`vieww-render/src/focus.rs`). iOS: `ios_accessibility.rs` **written, type-checked, never run**; "VoiceOver has never been heard" (PENDING §3.6). Desktop screen readers (NVDA/Orca) unproven. |
| 7 | Text & i18n | **6/10** | cosmic-text 0.19 for shaping/bidi/fallback (`vieww-text/src/lib.rs:26`), COLRv0 + bitmap emoji (`native/color_glyphs.rs`), CJK subset fixtures, IME events with Preedit + `apply_ime` (`vieww-foundation/src/editing.rs:1000-1016`), intl crate (datetime/number/list/message), RTL example (feature 22). Deductions: COLRv1 a non-goal, `ttf-parser` unmaintained (deny.toml:93), 46 s system font scan worst case (B12), no on-device IME proof in CI. |
| 8 | Hot reload & DX | **6/10** | State-preserving reload with type-path identity (`vieww-reload`, `hot-reload` feature, fingerprint `Verdict`), **works on Android** (measured on Redmi Note 7 Pro), documented impossible on iOS devices. Gaps: relies on external `cargo watch`, leaks a cdylib per reload by design, reordered-state undetected, studio out-of-process preview incomplete (PENDING §3.3). |
| 9 | Packaging & app-store distribution | **4/10** | `vieww-build/src/package.rs` produces Linux tree / `.app` / `.msi` (WiX) / APK (cargo-apk) / `.ipa`; `packaging/package.sh` builds studio `.deb/.AppImage/.msi/.dmg/.zip`; dist APK 2.6 MB. But: **no signing or notarization anywhere** (package.sh:77 says so explicitly), MoltenVK not bundled (B1), no AAB/Play/App Store/Steam pipeline, no OTA. A user **cannot** ship a vieww app to any store today. |
| 10 | Docs | **5/10** | Outstanding internal honesty docs (TRACKER/PENDING/FIDELITY/GPU-RENDERER-STATUS), 7-page user guide, docs `-D warnings` in the gate. But guide `layout.md` referenced and **missing** (`docs/guide/README.md` table), stale "vello backend" text in `crates/vieww/src/lib.rs`, no hosted API docs (blocked on publish), no changelog. |
| 11 | Supply-chain security | **6/10** | Rigorous `deny.toml` (9 targets, yanked=deny, wildcards=deny, crates.io-only sources, dated advisory ignore with reasoning), size budget, MSRV actually checked (`checks.sh:195-204`). Missing: scheduled deny/audit runs, SBOM (G8.7 optional), reproducible builds ("not reproducible today", G8.6), no provenance attestation. |
| 12 | Release engineering | **3/10** | The `release-gate` **workflow exists with push-to-main and `v*` tag triggers but is trapped in `vieww_base/.github/workflows/ci.yml` where GitHub never reads it** — the root `ci.yml` is a different workflow ("Build Mobile Apps"). All 9 live workflows are `workflow_dispatch`-only. No tag, no release, no channel has ever been cut; the beta checklist (B1–B21) has zero completed rows. |
| 13 | Plugin ecosystem readiness | **4/10** | Real C-ABI vtable plugin system (`vieww-plugin/src/{abi,host,registry}.rs`, 740-line abi.rs, `AbiVersion` checked before init, `#[vieww_plugin]` macro). But: nothing published to depend on, no ABI stability commitment document, no process isolation for untrusted plugins (documented as the honest answer, not implemented), zero third-party plugins. |
| 14 | Observability | **5/10** | Rich on-device: `FrameReport`, frame-stage medians, `GpuPassTimes` (draw/layers/filters/shadows/blit), input-latency wiring (`latency.rs`), startup stages, quality overlay via devtools, PSS in farm passes. Missing: no off-device log/metric export path, crash reports never leave the device, no symbolication story for stripped `dist` builds. |

**Overall: 5/10 — an engineering core operating at 8/10 wrapped in a product shell at 2/10.** The renderer/text/planner stack is verified to a standard no competitor matches (byte-identity gates, per-pixel CPU↔GPU parity, real-device receipts). But nothing has ever been published, signed, released, or gated continuously: there is no push CI, no tag, no changelog, and a freshly scaffolded app cannot compile. Production-readiness is blocked almost entirely by **release engineering and distribution**, not by rendering.

**The five things that matter most:**
1. **FW-01**: publish the crates / make `vieww new` produce a buildable project (B16) — until then the framework has zero external users by construction.
2. **FW-02**: make CI fire on push/tag — the infrastructure already exists (release-gate workflow) and is dead on arrival because of file placement.
3. **FW-04/05/06**: signable, installable, GPU-working Windows/macOS artifacts (MoltenVK bundling, Authenticode, Developer ID + notarization, D3D12/Metal scene renderers from the branch).
4. **FW-07/08**: close the open perf-budget regressions and turn receipts into an automated trend gate.
5. **FW-10/11**: accessibility and IME proofs on iOS/desktop — the last unverifiable claims standing between "Android parity" and "platform parity".

---

## 2. What exists today (verified inventory)

### 2.1 Core rendering path
- **`vieww-render`** (14,197 lines src): three-tree runtime — `tree.rs` (2,649), `frame.rs`, `event_loop.rs` (LoopHarness/FrameDriver/FrameScheduler, `step`/`settle`), `focus.rs` (FocusManager, scopes, traps, traversal, IME dispatch), `semantics.rs` (1,011 lines: Role/SemanticAction/Liveness/SemanticsTree with `reading_order`, `announcements`, `live_nodes`), 43 render objects in `objects/` incl. `editable_text.rs`, `sliver_app_bar.rs`, `repaint_boundary.rs`, `performance_overlay.rs`.
- **`vieww-element`** (10,452 lines): reconciliation keyed by type+key (`tree.rs:342`), `snapshot.rs` (Compose-style optimistic transactions with `Conflict`), `error.rs` (ErrorPolicy + custom placeholders with recursion guard), `signal.rs`, `hero.rs`/`shared_element.rs`, `inspect.rs`, `navigator.rs`. Tests: reconciliation, state-across-remount, hot_reload_identity, error_boundary.
- **`vieww-paint`**: scene/canvas/damage/layers/scheduler + **`native/` own CPU rasterizer (13,087 lines)** with SSE2+NEON SIMD, `fill_cache.rs`, `color_glyphs.rs` (COLRv0/CBDT/sbix), `blend_oracle.rs`, row-parallel scanline walk (`VIEWW_RASTER_THREADS`).
- **`vieww-scene`** (+`cost.rs` CostHint complexity pricing), **`vieww-render-graph`** (`graph/pass/plan/resource/scene_bridge/transient.rs`), **`vieww-render-planner`** (`policy.rs` decide→Placement::{Cpu,Gpu,Hybrid}, `shadow.rs` 768+ lines ShadowPlanner with Shadow→ShadowCalibrated→Authoritative mode machine, RMSE gate, `capabilities.rs` DeviceProfile), **`vieww-gpu`** (ScenePlan, mask/glyph/ramp/image atlases, `is_complete` gate), **`vieww-shaders`** (WGSL library, baked at build time — no naga at runtime in shipped binaries), **`vieww-hal`** (sealed `Device`; `vulkan/{mod,scene,swapchain,compute,pipeline_cache,present_history}.rs`; cfg-gated `metal.rs`, `d3d12.rs`).
- **`vieww-widget`** + macros: **59 widget modules**, Material3-grade controls, theming (`theme.rs`, `tokens.rs`), localizations, `tiers.rs`.

### 2.2 Platform layer
- **`vieww-platform-winit`** (18,354 lines, 31 modules): `app.rs` (4,010), `native.rs`, `a11y.rs` (AccessKit) + `a11y_android.rs` (741) + `ios_accessibility.rs` (written-not-run), `cadence.rs` (adaptive frame rate), `rasterizer_choice.rs` (681, `auto` calibration), `device_tier.rs`/`quality.rs` (Low/Medium/High, resolution scale), `latency.rs` (input→first-frame), `crash.rs`, `deep_links.rs`, `insets.rs`, `thermal.rs`, `perf_hint.rs`, `frame_budget.rs` (FrameBudgetGuard), `windows.rs`, `ios.rs`, `startup.rs`, `stats.rs`.
- **`vieww-platform-web`** (canvas, wasm32, byte-certified on 3 engines) and **`vieww-platform-web-dom`** (DOM/CSS translator backend — a genuinely differentiated "two web backends" story). **`vieww-hardware`** (capability probes + `skip_without!`). **`vieww-foundation`**: geometry/color/imaging, `editing.rs` (IME), `intl/`, `capability/` (camera/biometrics/location/platform_view…), `crash.rs`, `json.rs` (no-serde JSON).

### 2.3 Tooling & lifecycle
- **`vieww-cli`** (496 lines): new/doctor/build/run/test/profile/package. **`vieww-build`**: `scaffold.rs` (real template compiled by test), `doctor.rs`, `package.rs` (5 targets, honest MissingTool/WrongHost gates).
- **`vieww-reload`**: host/guest cdylib split, `fingerprint.rs` (state-shape `Verdict`), `watch.rs` (staging for Android `noexec` dirs). Works desktop+Android; iOS device impossible by policy (lib.rs:24-40).
- **`vieww-devtools`**: inspector, frame timeline, GPU/memory/damage/semantics/render-graph inspectors, `json_export.rs` (no-serde), `png_compare.rs`, `quality_overlay.rs`.
- **`vieww-plugin`** (+macros): `#[repr(C)]` vtable ABI, `AbiVersion` gate before `init`, registry loader, `stderr_host_vtable`; 34 unsafe sites confined to trampolines/loader.
- **`vieww-runtime`**: scheduler with budget accounting (`FrameReport::over_budget`), priorities.

### 2.4 Distribution & supply chain
- `packaging/package.sh` (studio: `.deb/.AppImage/.tar.gz/.msi/.zip/.dmg` + bundled rustc/rlib SDK), `vieww-build/package.rs` (user apps), `ci/release/{gate,artifacts,package-source,fill-checklist,report-failures}` — a complete release automation suite, **never fired**.
- `deny.toml` (verified above), `rust-toolchain.toml` pinned 1.98.1, MSRV 1.89 real and CI-checked, workspace lints, `dist` profile (LTO fat, strip, deps at `opt-level="s"`, `panic="abort"`) with `dist-unwind` opt-in, measured 9.11 MB stripped windowed app / 2.6 MB APK.
- Docs: `docs/guide/` (7 pages), `docs/architecture/` (5), `FIDELITY.md` (byte-identity contract table), `GPU-RENDERER-STATUS.md`, `VIEWW-QUALITY-CONTRACTS.md`, `docs/release/BETA-RELEASE-CHECKLIST.md` (G0–G10, B1–B21).

### 2.5 CI reality (audited)
- Root `.github/workflows/` (9): ci.yml=**Build Mobile Apps** (manual), framework-checks.yml (manual, 12 legs incl. lavapipe GPU, macOS Metal, Windows WARP D3D12, web 3-engine, cross type-check, coverage gate, **wire-check keeping root↔vieww_base copies identical**), hardware.yml (self-hosted Linux+Windows, manual, main-only), device-farm.yml (AWS, manual, main-only), 5 audio/wright workflows (manual).
- **Dead but complete**: `vieww_base/.github/workflows/ci.yml` is the **`release-gate`** workflow — `push: branches: [main]`, `tags: ["v*"]`, plan job + matrix legs (Windows/macOS hosted, installers, screenshots, merged checklist) — unreadable by GitHub from that path. The beta checklist's B9 ("release-gate.yml runs the Windows and macOS legs — **Fixed**") is stale.
- `.github/workflows-pending/`: audio-ci.yml + updated ci/hardware/device-farm/audio-* waiting for a workflow-scope token; `restore.sh` exists.

---

## 3. Branch delta: main → feature/main_fixes

13 commits (3b10b84…60adf6f), 35 files, +8,915/−1,035. What it delivers for the framework:

1. **`vieww-hal/src/scene_exec.rs` (1,560 lines, new)** — the plan→passes traversal extracted from `vulkan/scene.rs` so Vulkan, Metal and D3D12 execute *the same* op stream (`SceneOp`, `TargetId`, `PostPush`, `blend_index`, `PassKind`). This is the structural prerequisite for cross-backend parity as a contract (§6.1) and kills the "three backends gradually diverging" failure mode named in its module doc.
2. **D3D12 SceneRenderer** (`d3d12/scene.rs`, 1,478 lines + `tests/d3d12_scene.rs` 415) and **Metal SceneRenderer** (`metal/scene.rs`, 811 + tests 414) — the ports PENDING §1.2/§1.3 asked for. On main both backends are smoke-only. Status: written + tested on WARP/paravirtual GPU; **no real-GPU D3D12/Apple-silicon run recorded**.
3. **GPU 3D seam advanced** (PENDING §1.7): Phong + point/spot lights (`lit3d.wgsl` +265), textures and the transparent pass (`gpu_textured.rs` 256, `gpu_transparent.rs` 301) — all held to the CPU oracle on lavapipe.
4. **Planner becomes measurable and actionable**: `probe.rs` (299 lines) — real CPU-core detection + opt-in `calibrate_cpu_fill` micro-benchmark replacing round-number profiles; `policy.rs` — `hybrid_min_pixels(device)` derived from dispatch overhead (clamped 250k–4M px) instead of a constant; `shadow.rs` +768 — `MeasuredCostModel` fed by **live GPU timestamp buckets** from the render loop (`native.rs` wires `GpuPassTimes` → `note_gpu_pass_kind`); **`mixed.rs` (180 lines, new in vieww-platform-winit)** — band-split execution makes `Placement::Mixed` actionable for the first time (CPU rows + GPU band-as-damage).
5. **`vieww-test-harness/src/parity.rs` (270 lines)** — `ParityContract::{Deterministic, Performance}` with the workspace's documented tolerances (2/255 interior, edge-gradient 8) as named, enforceable contracts.

**What it does NOT fix (open regardless of branch):** crates.io publishing (FW-01), live CI (FW-02), release train (FW-03), MoltenVK bundling/signing (FW-04/06), Windows startup (FW-16), accessibility proofs (FW-10), WebGPU (FW-09), crash transport (FW-12), real-Apple-GPU and real-D3D12-GPU timing/parity runs (needs a Mac with Apple silicon and a Windows dGPU machine — the user's self-hosted Windows laptop is WARP-class for D3D12 purposes at best; Kaby Lake iGPU has D3D12 drivers, so the self-hosted Windows runner **can** run it once the branch lands). FIDELITY.md's Metal/D3D12 rows still read "not declared" on both branches — the branch adds the tests but not the contract rows.

---

## 4. GAP CATALOG

> Severity: **P0** release blocker · **P1** parity gap vs leaders · **P2** polish · **P3** moonshot differentiator. Status `[OPEN]` / `[FIXED ON feature/main_fixes]` where applicable.

---

### FW-01 · P0 · The framework is unpublishable and unusable outside the monorepo

- **Evidence.** `vieww_base/Cargo.toml:115` `version = "0.1.0"`; `repository` deliberately absent (lines 144-155: the old URL 404'd). BETA-RELEASE-CHECKLIST **B16**: "A project created by an installed Studio cannot be built… No such crate is on crates.io". `grep -rn "publish = false" crates/*/Cargo.toml` → 0 hits, so manifests are publish-ready, but `cargo search vieww` finds nothing (nothing was ever pushed).
- **Blocks.** Every external user, ever. `vieww new` scaffolds `vieww = "0.0.1"` (nonexistent) unless the scaffold finds a checkout; plugin authors have no SDK; docs.rs has no pages; semver cannot start.
- **Leaders.** Flutter ships a package manager as a product (pub.dev, https://pub.dev) and pins SDK versions in `pubspec.yaml`; React Native ships npm + Expo SDK versioning with 52-week support windows (https://docs.expo.dev/); iced/Slint/Dioxus publish to crates.io on a train (https://crates.io/crates/iced, https://crates.io/crates/slint, https://crates.io/crates/dioxus). None of them has a "the framework cannot be installed" state.
- **Fix.** (a) Restore `repository` + `[package.metadata]` on the 15 publishable crates (the Cargo.toml comment documents the exact two halves). (b) Decide the public crate set: publish `vieww-foundation/-widget/-element/-paint/-text/-render/-scene/-hal/-gpu/-render-graph/-render-planner/-platform*/-widget-macros/-animation/-gestures/-devtools/-test-harness/vieww` at `0.1.0`; keep app crates private. (c) `cargo publish --dry-run` in CI (order matters: leaf-first; add a `publish-order.txt`). (d) Point `scaffold.rs` at the published version and keep the SDK-bundle path fallback. (e) Adopt a written semver policy: pre-1.0, `0.x.y` minor = breaking allowed, changelog mandatory per release (copy iced's https://github.com/iced-rs/iced/blob/master/CHANGELOG.md discipline).
- **Tests.** GitHub CI: new job `publish-dry-run` (`cargo publish --dry-run -p <each>`, ~10 min); GitHub CI: `scaffold-external` job — `vieww new demo` into `/tmp`, `cargo build` with **no path override** (this is B16's regression test); Linux self-hosted: same with the installed-SDK route.
- **Status:** [OPEN]

---

### FW-02 · P0 · No CI runs on push or PR — the entire gate is manual

- **Evidence.** Every root workflow is `workflow_dispatch`-only (framework-checks.yml:59-77, ci.yml:24, hardware.yml:31, device-farm.yml:21). The **only** push-triggered workflow in the repo — `vieww_base/.github/workflows/ci.yml`, name `release-gate`, `push: branches: [main]`, `tags: ["v*"]` — sits in a directory GitHub never reads; the root `ci.yml` is a different file ("Build Mobile Apps"). `audio-ci.yml` still waits in `.github/workflows-pending/`. Checklist B9 "release-gate.yml runs the Windows and macOS legs — **Fixed**" is therefore **stale**.
- **Blocks.** Regression protection. A bad commit to `main` today reds nothing until a human clicks; the parity suites, coverage floor, size budget, cross-check and deny gate all exist and none of them defend the default branch. "A skipped self-hosted leg is 'not run', never 'passed'" (framework-checks.yml comment) — currently *every* leg is not-run by default.
- **Leaders.** Flutter runs a sprawling push/PR CI (https://github.com/flutter/flutter/blob/master/.github/workflows/ci.yaml) plus bringup/try builders; iced runs tests on push (README badge, test.yml); Slint runs CI on all PRs (https://github.com/slint-ui/slint/actions). React Native: CI on every PR + a "nightly" release train (https://reactnative.dev/contributing/overview).
- **Fix.** Decision needed (owner-level): restore `release-gate` from `vieww_base/.github/workflows/ci.yml` to `.github/workflows/release-gate.yml` via web UI or a workflow-scoped token (exactly the documented `restore.sh` path — rename to avoid colliding with the apps-build `ci.yml`). Then: keep hosted legs on push (they are the CPU pass); leave self-hosted legs as `workflow_dispatch` + a nightly `schedule` (hardware.yml already refuses non-main refs). Add a minimal `pull_request: [opened, synchronize]` leg running `checks.sh --quick` so external PRs get signal without burning the matrix.
- **Tests.** GitHub CI: the restored workflow itself is the test — push a whitespace commit to a branch, expect `gate (linux-cpu)` green; tag `v0.1.0-beta.1-rc1`, expect full-gate + installers + merged-checklist artifacts. Self-hosted: nightly `hardware.yml` cron; Device farm: nightly only after cost decision (each run bills AWS).
- **Status:** [OPEN]

---

### FW-03 · P0 · No release has ever been cut: no tag, no changelog, no channel

- **Evidence.** `git tag` on main → none. No `CHANGELOG.md` (only `RELEASE-NOTES-TEMPLATE.md`). Checklist table headers blank ("Version | (currently 0.1.0 … decide)"). `ci/release/gate.sh` + `fill-checklist.py` + `artifacts.sh` + `package-source.sh` + `report-failures.py` are complete automation, never executed against a tag. B4/B5 open.
- **Blocks.** Any user-facing "beta" claim; upgrade testing (G5.1 is N/A forever until a first release exists); issue triage against versions.
- **Leaders.** Flutter: quarterly stable + hotfix channels with written release process (https://docs.flutter.dev/release/); React Native: monthly point releases + nightlies; Slint: patch/minor trains with migration notes (https://slint.dev/releases); iced: changelog-driven tagged releases.
- **Fix.** (a) Decide version semantics: `0.1.0-beta.1` (B5). (b) Add `CHANGELOG.md` generated from `TRACKER.md` entries + conventional commits; `fill-checklist.py` already merges run folders into `CHECKLIST.md`. (c) Cut `v0.1.0-beta.1` from a certified commit through the restored release-gate; publish SHA256SUMS + a GitHub Release with the checklist attached. (d) Document channels: `beta` tags only until 1.0; nightly = main's last green release-gate, retained 14 days.
- **Tests.** GitHub CI: release-gate on tag must produce `installers-*`, `checklist-ci`, `SHA256SUMS` artifacts (assert in a `release-verify` job); Linux self-hosted: `ci/vieww gate --mode cpu/gpu` on the tagged commit (G0.12 same-digest check).
- **Status:** [OPEN]

---

### FW-04 · P0 · macOS: end users cannot run a vieww app (MoltenVK not bundled, unsigned, Metal scene renderer not shipped)

- **Evidence.** B1 (P0): MoltenVK must be copied into `Contents/Frameworks` + `MoltenVK_icd.json` into Resources — `vieww_hal::vulkan::load_entry` already searches those paths, `package.sh` never copies the files. B3: no Developer ID/notarization → Gatekeeper blocks first launch. Metal `SceneRenderer` exists only on the branch, tested on a paravirtual GPU (PENDING §1.2). MoltenVK GPU path on iPhone: `gpu_submit` 7-14 ms of translation overhead (PENDING §1.4) — the strongest argument for the native Metal port.
- **Blocks.** Every macOS distribution channel; the studio DMG builds but cannot survive a clean-machine launch.
- **Leaders.** Flutter macOS: embeds no Vulkan layer at all (Metal directly, https://docs.flutter.dev/deployment/macos); SwiftUI is native by definition; Electron/Slint ship notarized universal binaries (https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).
- **Fix.** (a) `packaging/package.sh`: add `--bundle-moltenvk` stage copying `libMoltenVK.dylib` + ICD json from `VULKAN_SDK` or a vendored copy (license: Apache-2.0, bundling is permitted); fail the DMG build if absent. (b) Merge the branch's Metal SceneRenderer and add FIDELITY.md rows for it (interiors ≤2/255, text ≤1/255) — `scene_exec.rs` already guarantees identical traversal. (c) Signing: `codesign --options runtime --timestamp` + `xcrun notarytool submit` + `stapler` behind `MACOS_SIGNING_ID`/`APPLE_API_KEY` secrets; unsigned fallback stays allowed but stamps "unsigned" into MANIFEST.txt.
- **Tests.** GitHub CI (macos runner): package + verify MoltenVK present in the `.app` (`plutil`/`ls` assertions); launch the app under `xcodebuild`-less smoke (open, first frame, quit) on the runner's GUI session. Device farm (iOS): existing iPhone suite already covers MoltenVK device runs. Self-hosted Linux: none (N/A). A borrowed/owned Mac remains the only notarization proof (G3.2/G9.3) — **decision: buy/borrow a Mac or waive macOS for beta**.
- **Status:** Metal renderer [FIXED ON feature/main_fixes]; bundling/signing [OPEN]

---

### FW-05 · P0 · Windows: D3D12 scene path absent on main, 840 ms startup, unsigned

- **Evidence.** Main's `d3d12.rs` = smoke tests only (WARP); branch adds `d3d12/scene.rs` + tests (WARP-passing). PENDING §3.5: first frame 840 ms Windows vs 221 ms Linux (Vulkan device ready 215 ms, first present 197-236 ms, pipeline creation 302 ms); `pipeline_cache.rs` (160 lines) added, device-farm benefit unproven. B2: no Authenticode.
- **Blocks.** Windows as a first-class desktop target; store/SmartScreen distribution.
- **Leaders.** Flutter Windows ships release-mode defaults + `msix` packaging guidance (https://docs.flutter.dev/deployment/windows); WinUI3/SwiftUI peers are native; Tauri (Rust peer) ships signed MSIs as the default story (https://tauri.app/distribute/).
- **Fix.** (a) Land the branch; (b) create the Vulkan device in parallel with window creation (PENDING §3.5 start); (c) seed pipeline cache before first frame — already implemented, verify on Windows self-hosted; (d) Authenticode via `signtool` in `package.sh --installers` behind `WINDOWS_CERT_THUMBPRINT` secret (or Azure Trusted Signing); (e) consider D3D12-present path skipping the Vulkan requirement entirely on Windows (B6's real answer).
- **Tests.** Self-hosted Windows (`hardware.yml windows`): `baseline`, `gpu` steps with before/after receipts — gate: warm first-frame < 300 ms on the i5-7200U; D3D12 scene suite on the Intel iGPU (it has real D3D12 drivers). GitHub CI: windows leg compiles + WARP scene suite (already in framework-checks) + MSI smoke (install into a temp dir, launch, quit).
- **Status:** D3D12 renderer [FIXED ON feature/main_fixes]; startup + signing [OPEN]

---

### FW-06 · P0 · No code signing, notarization, or attestation on any artifact

- **Evidence.** `packaging/package.sh:77`: "No code signing or notarisation. Those need certificates that cannot live in a repository…". G9.1-G9.3 all ☐. Android APKs: debug keystore only (ci.yml:192-214). iOS: ad-hoc, re-signed by device clouds.
- **Blocks.** Play Store (requires Play App Signing release key), App Store (requires distribution cert), SmartScreen reputation, Gatekeeper, enterprise distribution.
- **Leaders.** Flutter/RN treat signing as a first-class documented pipeline (https://docs.flutter.dev/deployment/android#signing-the-app, https://docs.flutter.dev/deployment/ios#review-xcode-project-settings); Expo automates store credentials end-to-end (https://docs.expo.dev/app-signing/); Fastlane `match` standardizes cert storage (https://docs.fastlane.tools/actions/match/).
- **Fix.** Per-OS secrets + steps (all behind optional inputs so the open-source pipeline still runs unsigned): Android `apksigner` + Play upload via `fastlane supply` or `gradlew bundleRelease` (add **AAB** output — Play requires it, current output is APK-only); Windows `signtool`; macOS `notarytool`. Add SBOM: `cargo cyclonedx` in `artifacts.sh` (G8.7). Human decision required: **which certificate vendor/budget** (OV Authenticode ~$200-400/yr; Apple Developer $99/yr; Google Play $25 once).
- **Tests.** GitHub CI: `release-gate` job verifying signature presence when secrets exist (`codesign --verify`, `signtool verify /pa`, `apksigner verify`); device farm: install the signed APK directly (no re-sign) — that's the regression test for signing.
- **Status:** [OPEN]

---

### FW-07 · P1 · Open performance-budget misses on certified reference hardware

- **Evidence.** PENDING §4.1: `00-layers-nested` and `23-editor-glass` 20-25 ms vs 16.7 ms on the i5-7200U both OSes (receipts 2026-10-15). B7: animation-stress p95 35 ms, 72/72 frames over. iOS first frame 280-400 ms vs 120 ms target (§1.4). LG Stylo 5 misses on every path (§4.5, RC5 — partially addressed by GPU damage, now 17.3 ms vs 16.7). Memory: baseline RSS 150 MB Linux vs 40 MB target (§4.2).
- **Blocks.** "Smoother than Flutter on weak hardware" — vieww's differentiating claim — currently unproven on its own reference machine.
- **Leaders.** Flutter publishes jank budgets and ships Impeller precisely to eliminate shader-compilation jank (https://docs.flutter.dev/perf/impeller); iOS 120 Hz requires first-frame < ~400 ms for TTI budget compliance (Apple HIG).
- **Fix.** Prioritized: (1) land §4.6/§4.7 follow-ups (render directly into swapchain image — removes the host round trip; two-frames-in-flight resources); (2) an SSE2 blur + half-float layer storage behind a **tolerance decision** (PENDING names it: changes bytes — needs the fidelity policy to permit a `Pixels-Changed:` commit); (3) startup: parallel device creation + pipeline cache warm + defer system font scan (B12: 46 s worst case → background scan already exists `system_fallback_in_background`). (4) Set the official support floor (B7's "decide the minimum supported CPU") — recommend: 2-core/4-thread Skylake-class CPU = the reference laptop, with the receipt as the published floor.
- **Tests.** Self-hosted Linux + Windows: `hardware.yml baseline, stress, gallery` with **budget.json** (see FW-08) — hard gate: nested/glass p50 ≤ 16.7 ms *or* an explicit known-limitation entry; animation-stress p95 ≤ 16.6 ms on the floor machine. Device farm: LG Stylo 5 gpu_pass ≤ 16.7 ms (remove from known-failures.txt when green — the file flags stale entries automatically).
- **Status:** [OPEN]

---

### FW-08 · P1 · Receipts are recorded, not gated: no automated performance trend regression

- **Evidence.** `examples/fixtures/src/receipt.rs` writes `vieww.bench-receipt/1` JSON naming GPU, driver, build, commit — but nothing compares run N to run N−1. `hardware.yml` reds only if a *step fails*; `linux.sh` `run()` records PASS/FAIL per step, not per metric. Device farm *does* gate (device-suite budget checks + known-failures.txt) — the only true perf gate in the repo, and it costs AWS money per run.
- **Blocks.** Silent perf regressions landing on main between farm runs (e.g. the fill cache that *cost* 1.9-6.6 ms on three phones was caught only by a farm run, §4.5).
- **Leaders.** Flutter runs device benchmarks in CI with deviation alarms (https://github.com/flutter/flutter/blob/master/dev/devicelab/); Chromium perf sheriffs every landed CL; RN ships Hermes esbench CI. None gate on *user-owned hardware receipts* — which is where vieww can leapfrog (§6.4).
- **Fix.** Add `ci/check/perf-budget.sh`: a `budgets.json` keyed `fixture × device-class` (reference-laptop classes: `kabylake-linux`, `kabylake-windows`), values = p50/p95 ms + RSS MB, each with tolerance ±10%; script runs `fixtures --runs 10 --receipt`, loads the newest receipt (`receipt.rs`'s own JSON), compares, fails on breach with a diff table; `--record` updates budgets as a reviewed commit (same UX as `size-budget.sh`, which this repo already trusts). Hardware.yml gains a `perf-gate` step calling it.
- **Tests.** Self-hosted Linux + Windows (these are the only machines with stable thermal identity): every hardware run gates; GitHub CI: `budgets.json` schema check + a dry parse test so hosted PRs can't rot it.
- **Status:** [OPEN]

---

### FW-09 · P1 · Web: no WebGPU path — canvas-CPU only

- **Evidence.** `vieww-platform-web` renders via the CPU rasterizer onto `<canvas>`; `vieww-platform-web-dom` translates to DOM/CSS. FIDELITY.md WebGPU row: "not declared". The only WebGPU mention is an aspiration at `crates/vieww-platform-web/src/lib.rs:86` ("a WebGPU backend under the same `ScenePlan` is a…"). `grep -rni webgpu crates/` → 3 hits, none implementing.
- **Blocks.** Web parity with Flutter (Skwasm on WebGPU/WebGL: "Flutter renders your UI using hardware-accelerated browser graphics APIs (WebGL and WebGPU) through its CanvasKit and Skwasm renderers" — https://docs.flutter.dev/platform-integration/web), and with Dioxus/Makepad web stories. Heavy animated scenes on web run at CPU speed; "most of a megabyte of WebAssembly before the first pixel" (web-dom's own doc) is the current cost.
- **Leaders.** Flutter: CanvasKit (WebGL2) + Skwasm (WebGPU, Wasm GC) — https://docs.flutter.dev/platform-integration/web; Slint web uses WebGL via femtovg/skia renderers; RN web is DOM. WebGPU shipped in Chrome 113, Safari 26, Firefox 141 — three-engine availability now matches vieww's existing 3-engine test matrix.
- **Fix.** Implement `vieww-hal/src/webgpu.rs` as a fourth `Device` consuming the **branch's `scene_exec.rs` op stream** (this is precisely why scene_exec was extracted): WGSL already exists (vieww-shaders); WebGPU is WGSL-native, so no naga translation needed. Map atlases to `r8unorm` textures, present via `getCurrentTexture()`. Keep canvas-CPU as the deterministic fallback and DOM for document-like pages. Add the FIDELITY row (Performance contract, interiors ≤2/255).
- **Tests.** GitHub CI web leg: extend `verify_web.py` (already runs Chromium/Firefox/WebKit via Playwright) with a `--gpu` variant asserting scene parity frames against the CPU backend inside a tolerance; skip on engines without WebGPU. Frequency: every push once FW-02 lands.
- **Status:** [OPEN] (architecture pre-placed by ScenePlan + scene_exec)

---

### FW-10 · P1 · Accessibility: iOS VoiceOver never heard; desktop screen readers unproven

- **Evidence.** PENDING §3.6: "VoiceOver has never been heard on vieww"; `ios_accessibility.rs` + `ci/mobile/xcuitest/ViewwA11yTests.swift` + `a11y-ios.sh` written, never run (XCUITest blocked on an Xcode project the repo deliberately doesn't carry). Desktop: no NVDA/JAWS/Orca run recorded anywhere; G7.4 unchecked. What *is* proven: TalkBack heard (2026-10-14), `a11y-android.sh` suite, AccessKit bridge, WCAG contrast audits that already caught 2 failing built-in schemes.
- **Blocks.** The "better than leaders" a11y claim; enterprise/government procurement (Section 508, EN 301 549) needs at least one desktop-reader verification.
- **Leaders.** Flutter ships per-platform a11y with documented testing (https://docs.flutter.dev/development/accessibility-and-localization/accessibility); Apple requires XCUITest a11y audits for App Store feature placement; RN has an a11y API + Jest a11y checks.
- **Fix.** (a) Create the minimal Xcode project wrapper via `xcodegen` (project.yml checked in — solves "repo deliberately does not carry a .xcodeproj" without hand-maintaining one) and run `a11y-ios.sh --sim` on the GitHub macOS runner with VoiceOver assertions via XCUITest's `XCUIElement` traits; (b) desktop: Linux self-hosted runner already has a desktop session — add Orca (`sudo apt install orca`) AT-SPI assertions using `at-spi2` Python bindings reading vieww's published tree (AccessKit already speaks AT-SPI); Windows self-hosted: NVDA add-on-free smoke via UIA (AccessKit windows consumer) asserting the tree; (c) add `audit.rs` findings as a **build-failing** lint for the widget gallery (`examples/features`), so no example ships unlabeled controls.
- **Tests.** GitHub CI (macOS leg): XCUITest a11y suite on simulator (weekly + on a11y-touching diffs). Self-hosted Linux: Orca/AT-SPI tree assertions (nightly). Self-hosted Windows: UIA tree dump vs golden (nightly). Device farm: keep TalkBack pass in the Android suite (already gated).
- **Status:** [OPEN]

---

### FW-11 · P1 · IME: implemented, never proven automatically on-device

- **Evidence.** `vieww-foundation/src/editing.rs:1000-1016` (`apply_ime`, `ImeEvent::Preedit`), `vieww-render/src/focus.rs:393` (`dispatch_ime`, `ime_cursor_area`), `editable_text.rs` object; PENDING §1.4 records the system keyboard + shared selection working by hand on iPhone 13 Pro. No automated CJK input test exists in any CI leg; G7.3 unchecked.
- **Blocks.** Any East-Asian market; "text is the weak point of Rust UIs" is vieww's own thesis (vieww-text lib.rs:3) — unproven IME undermines it.
- **Leaders.** Flutter/Compose have IME integration tests on device farms; RN has a dedicated TextInput e2e matrix; Slint's IME support is a documented per-backend feature.
- **Fix.** (a) Desktop: a `tests/ime_replay.rs` harness driving `ImeEvent` sequences (Japanese romaji→kanji commit, Korean double-compose, Chinese pinyin) through `EditableText` and asserting committed strings + candidate box rect — pure unit, runs in hosted CI; (b) device farm: extend the Android suite with an `adb shell input text` + IME-switching step (install a CJK IME, drive it via `adb shell ime enable`), assert committed text via the suite's `__VIEWW_SUITE__` channel; (c) iOS: XCUITest `typeText` with a CJK keyboard layout in the a11y suite.
- **Tests.** GitHub CI: the replay harness (every push). Device farm: Android IME step (nightly); iOS via XCUITest (weekly). Self-hosted Linux: fcitx5 headless IME test (fcitx5 has a test protocol) — optional.
- **Status:** [OPEN]

---

### FW-12 · P1 · Crash reports dead-end on device; stripped release binaries lack symbols

- **Evidence.** `vieww-platform-winit/src/crash.rs` + `vieww_foundation::crash::FileSink` write reports and replay `pending()` on next launch — but the doc example's `send_somewhere()` is the whole transport story: none exists. `dist` profile strips symbols (Cargo.toml:343), so a shipped crash backtrace is "a wall of hex"; release keeps `debug = "line-tables-only"` but that's not the shipping profile. No Sentry/error-aggregation integration anywhere (grep: 0 hits).
- **Blocks.** Field debugging of shipped apps — the single biggest post-launch firehose for any framework.
- **Leaders.** Flutter: Sentry SDK with native symbol upload + dSYM stripping guides (https://docs.sentry.io/platforms/flutter/); RN: Crashlytics is the default; Apple: MetricKit crash diagnostics built-in.
- **Fix.** (a) Ship a **breakpad/symbol-server story**: `cargo build --profile dist` + `llvm-symbolizer` mapping file uploaded at release time (minidump-style: write `crashreport + build-id` in `FileSink`, keep a `.map` per release in the artifacts); (b) add an opt-in `vieww-foundation` `CrashSink` trait with a `HttpPostSink` (no serde — reuse `json.rs`) so apps can point at Sentry-compatible endpoints (`/api/{project}/store/` accepts minidump/JSON); (c) document the privacy stance: telemetry-free by default, explicit opt-in — a differentiator vs Firebase-defaulting ecosystems.
- **Tests.** GitHub CI: unit test of sink serialization + a `#[cfg(panic=abort)]` child-process crash test writing a report and replaying it; self-hosted Windows: real `abort()` in a shipped-profile child, assert report on relaunch. Device farm: the suite already survives crashes — add "crash report present after forced panic" assertion.
- **Status:** [OPEN]

---

### FW-13 · P1 · Hot reload: solid core, rough product edges

- **Evidence.** `vieww-reload/src/lib.rs`: leaks a cdylib per reload by design; reordering same-size state fields undetected (documented sharp edge); requires external `cargo watch`; iOS devices impossible (signed-page policy), simulator untried. Studio's out-of-process preview: keyboard/IME/scroll/hover not forwarded over the wire (PENDING §3.3). No reload-time metrics (how long state survived across N reloads) anywhere.
- **Blocks.** DX parity with Flutter's sub-second stateful hot reload (the benchmark every comparison names) and RN Fast Refresh (https://reactnative.dev/docs/fast-refresh).
- **Leaders.** Flutter: stateful hot reload in <1 s + hot restart, deeply integrated with DevTools; RN: Fast Refresh by default; Dioxus: `dx serve` live-reload across web/desktop/mobile (https://dioxuslabs.com/learn/0.6/getting_started/); Slint: live-preview in VS Code (https://marketplace.visualstudio.com/items?itemName=Slint.slint).
- **Fix.** (a) Fold the watcher into `vieww-cli`: `vieww dev` = notify-based watch + guest rebuild + host spawn (removes the two-terminal dance in the module doc); (b) add a reload-metrics counter (reload count, state-preservation rate) surfaced in `vieww-devtools` overlay; (c) document state migration: when `Verdict` says "rebuilt", print *which* type paths changed (fingerprint already hashes them) — turns the sharp edge into a diagnosable event; (d) finish studio remote preview event forwarding (keys/scroll/hover/IME) — the wire protocol exists.
- **Tests.** GitHub CI: existing `hot_reload_identity.rs` + add a 50-cycle reload soak asserting state survives and RSS growth ≤ bounded leak/MB (the leak is accepted, but *measured*). Self-hosted Linux: `examples/reload-host` soak as part of desktop suite.
- **Status:** [OPEN]

---

### FW-14 · P1 · Golden/snapshot infrastructure has no PR-level teeth

- **Evidence.** `vieww-test-harness/src/visual.rs:335` `assert_matches_golden`; `examples/shots/` holds only 12 committed screenshots, and cross-OS comparison is **by hand** via `ci/vieww gate --against <dir>` (G1.13); `raster-identity.sh` guards byte-identity on the same machine. But: no baselines are stored as CI artifacts, no per-PR visual diff report, and the whole apparatus is manual-only today (FW-02).
- **Blocks.** Visual regression protection at the only moment it matters (a landing PR).
- **Leaders.** Flutter golden tests run per-PR with failure images attached (https://api.flutter.dev/flutter/flutter_test/matchesGoldenFile-class.html + https://docs.flutter.dev/testing/overview); Skia/Chromium require goldens on every CL; Compose has screenshot testing (Paparazzi at Square, now AndroidX).
- **Fix.** (a) Commit goldens under `examples/shots/baseline/` (they're already deterministic — same bytes across OSes per FIDELITY); (b) a `shots` job in the restored release-gate: render on Linux, compare to baseline, upload a `shots-diff` artifact with PNG triptychs on failure (paint's `png_compare.rs` already builds them); (c) fonts: the CI already subsets fonts (`ci/tools/subset-fonts.py`, `make_test_fonts.py`) — assert those exact fonts in the golden env so hosted runners can't drift.
- **Tests.** GitHub CI (ubuntu): shots job every push; self-hosted Windows/Linux: cross-OS byte check weekly (G1.13 automated).
- **Status:** [OPEN]

---

### FW-15 · P2 · Memory: 150 MB RSS vs 40 MB target; no heap profiling in any gate

- **Evidence.** PENDING §4.2 table: baseline window 150 MB Linux / 89 MB Windows vs "under 40 MB" target; phones 76-92 MB PSS CPU path; idle Linux 3.4% CPU with zero frames (unexplained wakeups — a real defect class). The 2026-10-19 farm run *added* 10-16 MB (row-parallel + caches) — reversed by §4.5's cache defaults.
- **Blocks.** "Lightest framework" claim (AIMS §E); low-RAM embedded/TV devices; battery.
- **Leaders.** Flutter's baseline is ~50-90 MB but it doesn't claim lightness; Slint's embedded story (frame buffer + <1 MB heap on MCUs — https://slint.dev/showcases) is the actual lightness leader; Makepad targets similar minimalism.
- **Fix.** (a) Add `heaptrack` capture to `hardware.yml baseline` on Linux self-hosted with a top-10 allocations report into the receipt; (b) fix the idle-wakeup (likely a timer/poller — `stats.rs` already counts idle frames, extend to count wakeups via `/proc/<pid>/wakeup` or `strace -c`); (c) `cargo bloat --release -p vieww --example counter -n 40` printed in the size step; (d) consider the 16-byte-per-pixel layer format → f16 with a `Pixels-Changed:` decision (same as FW-07).
- **Tests.** Self-hosted Linux: heaptrack + RSS assertion ≤ 120 MB (interim budget, ratchet down) in the perf-gate (FW-08); idle-CPU assertion ≤ 0.5% over 60 s.
- **Status:** [OPEN]

---

### FW-16 · P2 · Startup: Windows 4× Linux; font scan can cost 46 s

- **Evidence.** PENDING §3.5 (840 ms vs 221 ms; trace attributes 215 ms device, 197-236 ms first present, 302 ms pipeline creation); B12: system font scan 46 s measured synchronously pre-first-frame (`use_system_fonts`); `pipeline_cache.rs` landed, benefits unproven on real drivers; iOS first frame 280-400 ms vs 120 ms target.
- **Blocks.** TTI on every platform; App Store review impressions; the "fast" in every framework pitch.
- **Leaders.** Flutter invested years in first-frame (engine warm-up, deferred components); iOS launch screens are contractual UX (HIG); Android cold-start is graded in Play vitals (https://developer.android.com/topic/performance/vitals).
- **Fix.** Startup census already exists (`startup.rs`, Device Farm Claude-Session receipts carry startup stages) — the missing pieces: (a) parallelize device creation with window creation; (b) render the first frame with embedded fonts while the system scan continues in background (already half-exists: `system_fallback_in_background`); (c) verify pipeline-cache hit-rate on self-hosted Windows and device farm (observation `pipeline_cache` exists).
- **Tests.** Self-hosted Windows: warm first-frame ≤ 350 ms gate (receipt-based, FW-08). Device farm: first-frame observation already recorded — add budget 500 ms on mid-tier phones, 120 Hz phones report pacing separately.
- **Status:** [OPEN]

---

### FW-17 · P1 · Plugin ecosystem: ABI exists, ecosystem doesn't

- **Evidence.** `vieww-plugin/src/abi.rs` (740 lines): `#[repr(C)]` vtables, `AbiVersion` checked before `init` (registry.rs:266), panic trampolines with `STATUS_PANICKED`. No published SDK (FW-01), no versioned ABI document, no compatibility guarantees statement, no example third-party plugin in-tree, no process isolation for untrusted code (Cargo.toml's own comment: "process isolation is the honest answer for untrusted code" — not built).
- **Blocks.** Third-party extensibility — every leader's moat (pub.dev 50k packages, npm, Swift packages, even iced's widget ecosystem).
- **Leaders.** Flutter: federated platform channels + pub.dev with score/verification; RN: native modules + Expo modules; VS Code extension API proves the versioned-host/guest model at scale.
- **Fix.** (a) Publish `vieww-plugin` + macros first (smallest dependency closure); (b) write `docs/plugin-abi.md`: what `AbiVersion` promises (additive-only within a major, host-provided allocators, panic contract, `Send/Sync` rules for the 34 unsafe sites); (c) add `examples/plugin-minimal` (a cdylib outside the workspace compiled against *published* crates — the real compat test); (d) design the isolation option: `PluginRegistry::spawn_sandboxed` via a child process + the studio's existing IPC (remote.rs) — no new infrastructure needed.
- **Tests.** GitHub CI: compile `plugin-minimal` against the last *published* version + run it against current host (compatibility matrix test — this is what semver actually means); unit tests already cover the loader.
- **Status:** [OPEN]

---

### FW-18 · P2 · Documentation gaps: missing page, stale claims, no hosted docs

- **Evidence.** `docs/guide/README.md` links `layout.md` — **file does not exist** (verified). `crates/vieww/src/lib.rs` closing line still says "enable the `gpu` feature for the vello backend" — vello was removed (RENDERER-MIGRATION.md). No CHANGELOG, no API-stability page, guide not versioned per release. Positives: docs build with `-D warnings` in the gate; TRACKER/PENDING are best-in-class engineering honesty.
- **Leaders.** Flutter's docs are a product (docs.flutter.dev, API refs, cookbook, codelabs); Slint's book (https://docs.slint.dev/latest/) + SlintPad playground; Dioxus learn pages per version.
- **Fix.** (a) Write `layout.md` (content exists in `docs/architecture/ui-tree.md` + intrinsics tests — adapt); (b) fix the vello line; (c) on publish (FW-01), docs.rs hosts the API automatically — add `#![doc = include_str!("../README.md")]` polish per crate; (d) add a versioned guide: `docs/guide` copied per tag into the release artifact (viewwsite can host).
- **Tests.** GitHub CI: a docs-link checker (lychee or a 30-line script over the guide) in checks.sh; docs build already gated.
- **Status:** [OPEN]

---

### FW-19 · P2 · Platform polish debt (DnD out, monitors, Wayland clipboard, APK ABIs, toolchain pin)

- **Evidence.** Outgoing drag-drop: Windows OLE `DoDragDrop` implementation written, type-checked, **never run**; other platforms stubs (`NullDragStarter`, PENDING §3.1). Monitor-change re-read: written, needs a 2-monitor session (§3.2, paused). B14: Wayland-only sessions have no clipboard (arboard without `wayland-data-control`). B19: APK is arm64-only. B20: scaffold pins `stable` while toolchain checks target 1.98.1.
- **Fix.** (a) Self-hosted Windows session: run the DnD [session-verifies] list (drag file → Explorer), wire `WindowsDragStarter` into the adapter; (b) enable arboard's `wayland-data-control` + test under Weston (the repo already runs a headless Weston for monitor tests); (c) add x86_64 to cargo-ndk targets (B19) for emulator installs; (d) scaffold writes the studio's exact toolchain version.
- **Tests.** Self-hosted Windows: manual DnD step once, then a scripted `device-suite`-style check where possible; GitHub CI: android leg builds both ABIs; self-hosted Linux: Weston clipboard round-trip.
- **Status:** [OPEN]

---

### FW-20 · P2 · Embedded native views: no consumer, unknown shape (PENDING §6.2)

- **Evidence.** `vieww_foundation::capability::platform_view` types exist; "no application has ever used one… decided by the second consumer" (PENDING §6.2). Every leader has an answer: Flutter PlatformViews/UiKitView/AndroidView, RN native components, Compose AndroidView.
- **Fix.** Ship the first consumer as a *test*, not an app: a `VideoSurface` capability implemented on Android (`SurfaceView` via JNI, the a11y_android.rs pattern proves the JNI path) and on desktop via winit's `raw-window-handle` + a media crate, with clipping/positioning assertions. This decides the shape with a real referee.
- **Tests.** Device farm: Android SurfaceView compositing check (position/clip/z-order vs scene pixels); GitHub CI: type-check only.
- **Status:** [OPEN]

---

### FW-21 · P2 · GPU 3D: per-render bring-up; CPU-side shadow map; no IBL/GI

- **Evidence.** PENDING §1.7 (pre-branch) — the branch closes Phong/point/spot/textures/transparent, but "the whole pipeline… is still brought up and torn down per render; a cached-pipeline struct is the step the 2D path took with SceneRenderer". Shadow map still rasterized on CPU and uploaded. No image-based lighting/GI probes.
- **Fix.** A `GpuRenderer`-level pipeline cache mirroring `vulkan::pipeline_cache.rs` (which already exists for 2D — extend `set_pipeline_cache_file` to 3D pipelines); GPU shadow map must reproduce `raster_depth` sampling bit-exactly (PENDING names the constraint); measure whether CPU shadow pass matters before porting (bring-up dominates).
- **Tests.** GitHub CI lavapipe leg: existing gpu_lit/textured/transparent suites (branch added them); self-hosted Linux: real-GPU parity + timing receipts.
- **Status:** Phong/lights/textures/transparent [FIXED ON feature/main_fixes]; caching/IBL [OPEN]

---

### FW-22 · P2 · Planner actuation is still data-starved; calibration underestimates CPU 34-38%

- **Evidence.** PENDING §2.3 (gate never fed real frames; CPU-side shadow log has zero samples), §4.6 (calibration underestimates steady CPU cost 34-38% on Stylo/A73), §1.5 (tier promotion awaits the farm run; `gpuscale_verdict.py` + `quality.rs` one-line flip ready). Branch adds the live-loop feed (`native.rs` → `note_gpu_pass_kind`) and `mixed.rs` execution.
- **Fix.** The repo already names it: run `close-remaining.sh gallery` with `VIEWW_PLANNER_SHADOW=<log>` + `VIEWW_PLANNER_AUTHORITATIVE=1` on the Linux self-hosted box; land the per-fixture gallery shadow log in `receipts/`. Then fix calibration bias: launch frames vs steady state (§4.6's named cause) — exclude the first N frames or weight by frame index.
- **Tests.** Self-hosted Linux: planner-authoritative A/B (frame time authoritative-vs-`is_complete` on same content — PENDING defines the number); device farm: `gpuscale` pass re-run (existing) + `cpubare` (existing).
- **Status:** machinery [FIXED ON feature/main_fixes]; actuation + calibration bias [OPEN]

---

### FW-23 · P2 · Supply chain: no scheduled audits, no SBOM, non-reproducible builds

- **Evidence.** deny.toml runs inside `checks.sh` (manual today, FW-02); no cron/scheduled cargo-deny or `cargo audit`; G8.6 "not reproducible today (timestamps, bundled toolchain)"; G8.7 SBOM optional-and-absent; RUSTSEC-2026-0192 (ttf-parser unmaintained) accepted with a written rationale — the risky dependency is *also* load-bearing for the whole text stack (deny.toml:60-93).
- **Leaders.** Slint ships SBOMs for embedded customers; Tauri documents a full signed-supply-chain path (https://tauri.app/distribute/); Google Play now *requires* SBOM data for some app categories.
- **Fix.** (a) `schedule: cron nightly` job running `cargo deny check advisories` + `cargo audit` (cheap, catches the RUSTSEC feed moving); (b) `cargo cyclonedx` in `artifacts.sh` (G8.7); (c) reproducibility: `SOURCE_DATE_EPOCH` + `--lockfile` + fixed toolchain gets Rust artifacts close; document the *known* non-reproducible parts (bundled rustc in studio SDK) and scope the claim to framework crates; (d) start the skrifa port for vieww-paint glyph outlines (deny.toml already sketches it; removes one of two font parsers).
- **Tests.** GitHub CI: nightly deny+audit job; release-gate asserts SBOM exists per artifact.
- **Status:** [OPEN]

---

### FW-24 · P1 · No OTA/update channel for shipped apps

- **Evidence.** Grep for "OTA|code push|update channel" across crates → nothing. `vieww-build/package.rs` produces artifacts; no updater, no version negotiation, no delta/patching. For native binaries there's no story at all; for web it's implicit (reload).
- **Blocks.** Post-launch bug response — the reason Shorebird exists for Flutter and EAS Update for RN ("Deploy patches without delays / Waiting for the next app review cycle" — https://shorebird.dev/, https://docs.expo.dev/eas-update/introduction/). Store review cycles are 1-7 days; every serious app ships an escape hatch.
- **Leaders.** Shorebird (Flutter code push), EAS Update (RN/Expo), Sparkle (macOS), Squirrel/electron-updater (Electron), Steampipe for games. Slint/iced have no first-party answer — this is a real open flank even among peers.
- **Fix.** Native-first, honest scope: (a) desktop updater — check a signed manifest URL, download full package, verify SHA256 + signature, swap-and-restart (Sparkle-style; ~1 week of work using existing SHA256SUMS infra); (b) *code patch* on Android is hard for Rust (no JIT; `dlopen` of new `.so` is exactly what `vieww-reload` already does in dev — a release-mode "guest update" is technically the same mechanism with signature checks; document the security posture carefully); (c) decision required: build first-party hosting (viewwsite) vs document third-party (Sparkle) — recommend document-first (Sparkle/WinSparkle/squirrel) + a `vieww update check` API stub.
- **Tests.** Self-hosted Linux/Windows: an updater e2e — v1 installed, serve v2 manifest locally, assert swap + relaunch + version bump; GitHub CI: manifest signing/verification unit tests.
- **Status:** [OPEN]

---

### FW-25 · P3 · Scene-IR has no stable serialization (tool interop locked inside Rust)

- **Evidence.** `vieww_paint::Scene`/`Command` is an in-memory enum; `vieww-devtools/json_export.rs` proves no-serde JSON works but is devtools-shaped, not a stable format. Every leader has a stable interchange: Flutter has SkiaPicture/Flutter scene serialization for devtools replay, RN the JSON bridge protocol, Slint's `.slint` markup *is* the product, Android has proto-rendernode for testing.
- **Fix / PoC.** Define `vieww-ir`: a versioned binary encoding of `Scene` (tag+len+payload, hand-rolled like json_export — no serde), schema version in the header, decoder for N and N−1. Enables: external profilers, the studio writing scenes, fuzzers generating scenes directly (see FW-26 test plan), cross-language bindings, and deterministic-replay bug reports (§6.2). Scope guard: IR is a *testing/tooling* contract, never the on-the-hot-path format.
- **Tests.** GitHub CI: round-trip property test (random scenes → encode → decode → byte-identical render) + golden IR fixtures locked per schema version.
- **Status:** [OPEN]

---

### FW-26 · P3 · Cross-backend parity is tested but not contracted

- **Evidence.** FIDELITY.md Metal/D3D12 rows: "not declared; the port must add its rows here before it draws a user's frame". Branch ships Metal+D3D12 SceneRenderers and shared `scene_exec.rs`, plus `ParityContract` in test-harness — but FIDELITY.md (on both branches) still shows no rows, and no release artifact *certifies* backend agreement. Also a stated **deliberate non-goal** in PENDING.md: "Cross-backend parity as a suite. There is one renderer compiled two ways" — the branch's scene_exec work is exactly what converts this from non-goal to cheap contract.
- **Fix.** See §6.1 (this is the flagship novel methodology).
- **Status:** renderers [FIXED ON feature/main_fixes]; contract rows + certification [OPEN]

---

**Catalog totals: 26 gaps — 6× P0, 8× P1, 8× P2, 2× P3, plus 2 sub-items resolved on the branch.**

---

## 5. Where vieww already leads (verified strengths)

1. **Own-GPU-stack with a pixel-parity oracle.** CPU rasterizer is the reference; Vulkan executes the *same* plan and is held to interiors ≤2/255 (text ≤1/255, compositor ≤2/255 *every* pixel) — `vulkan_scene.rs`, `vulkan_text.rs`, `vulkan_compositor.rs`, contract written down in `docs/FIDELITY.md` with per-backend rows and a rule that loosening a bound requires the test and the doc in one commit. No leader publishes a *per-backend numeric fidelity contract*; Flutter/Skia have internal tolerances, not product-facing ones.
2. **Hybrid CPU/GPU planner with device calibration.** `vieww-render-planner` prices per-command work (`cost.rs` CostHint), derives split thresholds from measured dispatch overhead (branch `policy.rs`), calibrates the CPU by micro-benchmark (`probe.rs`), feeds the GPU side from live timestamp queries, and gates its own authority behind an RMSE consistency test (`shadow.rs` mode machine). Flutter/Impeller, Skia, Qt all pick one renderer per process; none *plans per frame per device*.
3. **Deterministic rendering as a first-class property.** `raster-identity.sh` byte-compares the entire fixture gallery across commits; `retained_equivalence.rs` proves damaged repaint == full repaint byte-for-byte; partial mask uploads are frame-for-frame identical; `ParityContract::Deterministic` (branch) names the zero-tolerance mode. Flutter goldens fight nondeterminism (font AA, GPU timing); vieww's default is bytes.
4. **The honesty system.** TRACKER.md/PENDING.md with four-field items (what/why/blocks/start-a-real-command), receipts that name GPU+driver+commit, `known-failures.txt` as "a debt with a label, not a mute button", mutation-checked parity tests. No leader has anything comparable publicly.
5. **no-serde / no-wgpu discipline.** Verified: zero serde deps in framework crates (json.rs hand-rolled); wgpu/vello removed with RENDERER-MIGRATION.md recording why. This is why a 2.6 MB APK and 9.11 MB desktop app exist at all — Flutter's minimal Android app is ~4-5 MB, RN ~8-20 MB.
6. **Receipt-based benching on real user hardware.** `vieww.bench-receipt/1` (fixtures/src/receipt.rs) + hardware.yml publishing receipts to `ci-results/*` branches + energy (RAPL) + PSS + startup stages. Framework benchmarks that a third party can re-verify on their own laptop with one script.
7. **Quality tiers + adaptive cadence + GPU damage** (§4.5-4.7 work): per-tier resolution scale with a promotion *procedure* (`gpuscale_verdict.py`), adaptive frame rate (panel vs steady 60 Hz), GPU-side damage tracking with retained targets — Flutter has dynamic resolution in Impeller but no per-tier policy product, and no leader auto-promotes device tiers from farm evidence.

---

## 6. Novel methodologies NO leader has today (PoC candidates)

### 6.1 Cross-backend pixel-parity as a release CONTRACT ("parity certificate")
- **What.** Every release artifact ships a machine-checkable certificate: for scene suite S (the fixture census + compositor suite), backend B ∈ {CPU, Vulkan, Metal, D3D12, WebGPU} agrees with the CPU oracle under contract C (Deterministic for CPU itself; Performance ≤2/255 interior + edge-hugging for the rest), *measured on CI + at least one real device per backend family*, with the receipt signed into the release.
- **Why no leader has it.** Flutter/Skia *have* the tests (Skia's DM, Flutter goldens) but treat parity as an internal property; there is no user-visible artifact, no cross-backend numeric claim, and no per-release attestation — because their backends legitimately differ (Skia CPU vs Ganesh differ at edges and they don't promise otherwise). vieww's architecture (one ScenePlan, one `scene_exec.rs` traversal, CPU coverage reused as GPU masks) makes agreement *structural*, so the contract is cheap to keep.
- **Feasibility.** Branch already contains: shared traversal (`scene_exec.rs`), contracts (`parity.rs`), Metal+D3D12 executors with test suites, WebGPU is the only missing backend (FW-09). The certificate is an extension of the existing receipt schema: add `parity: {suite, backend, contract, worst_channel_delta, edge_violations}` per row.
- **PoC sketch.** New `crates/vieww-test-harness/src/parity_matrix.rs`: `pub struct ParityRow { suite: &'static str, backend: Backend, contract: ParityContract, result: Result<(), Diff> }` + `pub fn certify(plan: &ScenePlan, oracle: &Frame, backends: &[Backend]) -> ParityCertificate`; a `ci/check/parity-matrix.sh` running it on lavapipe (CI) + real GPUs (self-hosted) and writing `parity-certificate.json` into the release artifacts; FIDELITY.md rows become *generated* from it.
- **Proves.** "Same pixels, every GPU, every release" — a marketing-grade engineering claim no competitor can make without re-architecting.
- **Tests.** GitHub CI (lavapipe + WARP + paravirtual Metal) every push; self-hosted Linux (Intel+NVIDIA) + Windows (D3D12 iGPU) nightly; device farm monthly (Adreno/MoltenVK rows).

### 6.2 Deterministic record/replay of full UI sessions as the bug-report format
- **What.** A shipped app can write "session tapes": the ordered input/event stream + seedable clock + the exact crate versions, such that any machine replays the session to *frame-exact* pixels. A user's bug report becomes `report.viewwtape` that a developer renders and sees byte-identical frames.
- **Why no leader has it.** Everyone is close and no one is there: Flutter has golden tests and integration recordings but rendering nondeterminism (GPU timing, raster cache) breaks replay; RN records network/HAR but not pixels; game engines have input replay but nondeterministic float paths. vieww's byte-identical raster + seeded `FrameDriver` clock (`examples/features/harness/src/clock.rs` exists) + no-heap-noise frame pipeline is the missing ingredient *nobody else owns*.
- **Feasibility.** `FrameDriver` already steps deterministically (LoopHarness `step`/`settle`); input events are value types in `vieww-foundation`; the tape needs: (a) an event tap in `vieww-platform-winit/src/input.rs` writing to a ring buffer, (b) a `ReplayClock` implementing the scheduler's time source, (c) a `vieww replay <tape>` CLI subcommand, (d) version-stamping with the scene-IR (FW-25) so a tape from an older version renders against that version's semantics.
- **PoC sketch.** `crates/vieww-reload`-style new crate `vieww-tape`: `TapeRecorder::attach(&mut App)`, `Tape::save(path)` (no-serde binary), `replay(tape) -> Vec<Frame>` compared against stored frame hashes (a "proof-carrying session").
- **Proves.** Support cost → ~zero: "cannot reproduce" is eliminated for rendering bugs; doubles as the ultimate regression test corpus (user tapes become CI fixtures).
- **Tests.** GitHub CI: record/replay round-trip on the fixture interaction suite; self-hosted: cross-OS tape replay (tape recorded on Windows replays on Linux byte-identically — only possible because pixels are deterministic).

### 6.3 Per-device hybrid render planning shipped as a *product feature* (not an internal heuristic)
- **What.** The planner's decision — CPU, GPU, or band-split Mixed, per frame, calibrated on this device — exposed, logged, and *explained* to the developer, with the calibration receipt inspectable in devtools and the tier promotion auditable from farm data.
- **Why no leader has it.** Leaders hard-code the strategy per process (Impeller always-GPU; Qt renderscene heuristics are opaque; Lynx/Flutter pick threads not devices). None ship: (a) per-frame CPU/GPU banding, (b) on-device calibration with a consistency gate, (c) an audit trail. It's architecturally foreign to them — they don't have a CPU rasterizer that is *also* the oracle, so a CPU fallback is a degradation, not a co-equal executor.
- **Feasibility.** Branch has all machinery (probe, MeasuredCostModel, ShadowPlanner modes, mixed.rs bands). Missing: actuation trust data (FW-22) and the devtools surface (`vieww-devtools` already has gpu_inspector + quality_overlay — add a planner panel showing the last N decisions and their predicted vs measured costs).
- **PoC sketch.** `vieww-devtools/src/planner_inspector.rs` consuming the shadow log format; a farm pass emitting `planner_decisions.json` per phone; `gpuscale_verdict.py` extended to also verdict *decision quality* (predicted vs actual ms per decision).
- **Proves.** "The framework measures your device and chooses the renderer — here is the receipt" — a defensible, data-backed differentiator vs every leader's one-size strategy.
- **Tests.** Self-hosted + device farm: planner-decision telemetry in every run; CI: unit tests of the policy (branch already added threshold-derivation tests).

### 6.4 Receipt-verified performance budgets as first-class CI artifacts
- **What.** FW-08's `budgets.json`, generalized: budgets are *receipts from the last certified release*, stored in-repo, compared on every self-hosted run, and attached to releases; a third party can verify the published claim on their own hardware with `ci/vieww gate --mode perf`.
- **Why no leader has it.** Flutter's device lab benchmarks are internal infra with dashboards (https://github.com/flutter/flutter/blob/master/dev/devicelab/) — not committed, not per-release-attested, not user-replicable. Chromium perf has sheriffing but not a shippable "verify our numbers" path. vieww already has receipts naming hardware+driver+commit; the only missing piece is the compare-and-gate script + the published budget file.
- **Feasibility.** High: `receipt.rs` JSON exists, `size-budget.sh` is the template, self-hosted runners exist, `hardware.yml` already publishes to `ci-results/*`.
- **PoC sketch.** `ci/check/perf-budget.sh` (FW-08) + `budgets/release-0.1.0-beta.1.json` + a `--verify-published` mode that downloads the release's budget and validates the local machine's class before comparing. Release notes embed the budget table.
- **Proves.** Performance claims become falsifiable artifacts — trust-as-product.

### 6.5 Scene-IR stable serialization for tool interop
- Covered as FW-25; as methodology: Slint's `.slint` markup is the closest analogue (it *is* a stable IR for designers); Flutter's Flutter-Designer attempts failed partly for lack of a stable scene contract. vieww's `Scene` is already the unit the planner, both executors, the census, and devtools all share — serializing it (versioned, no-serde) makes the studio, fuzzers, and third-party tools first-class citizens without linking Rust. **PoC**: `vieww-ir` crate + `vieww render scene.ir --backend cpu --out png` (a headless render CLI that doubles as a fuzzer target and a designer-tool backend).

### 6.6 Proof-carrying frames
- **What.** Per frame (or per N), a cheap rolling hash of the presented bytes + device profile + contract id + build id — written into the existing `FrameReport` and into crash reports. A crash report then carries the last K frame hashes: support can instantly distinguish "rendered wrong pixels then crashed" from "crashed during render", and a user can prove what they saw.
- **Why no leader has it.** Nobody hashes frames because nobody can afford determinism; GPU-timing-dependent pipelines make hashes unstable. vieww's deterministic CPU path makes the hash *meaningful*; the GPU path under the Performance contract can hash the interior (edge-excluded) region.
- **Feasibility.** `FrameReport` exists (`vieww-runtime/src/frame.rs`); a `FxHash`/xxhash over the presented buffer is ~0.1 ms at 1080p (measure; gate behind a debug flag + crash-time capture only if it costs).
- **PoC sketch.** `vieww-runtime/src/frame.rs`: `frame_hash: Option<u64>` populated in `FrameDriver::present` when `VIEWW_FRAME_HASHES=1`; crash.rs embeds the last 64 hashes; a devtools view decodes them against a replay tape (6.2) to re-render the exact frames.
- **Proves.** End-to-end accountability of the render pipeline — the substrate for 6.1/6.2 productization.

---

## 7. Stream test plan

| # | Test | Runs where | Gates what | Frequency |
|---|---|---|---|---|
| T1 | `checks.sh` (fmt, clippy -D, docs -D, tests, MSRV 1.89, deny) | GitHub CI (ubuntu) | Code health, MSRV truth, licences/advisories | Every push (needs FW-02) |
| T2 | Coverage floor 70% (`coverage.sh`) | GitHub CI | Test regression | Every push to main, nightly on PRs |
| T3 | Cross type-check (macOS/iOS/Windows/Android from Linux) | GitHub CI | Platform compile breakage | Every push |
| T4 | Vulkan suites on lavapipe + 3D GPU parity + Xvfb window | GitHub CI | GPU correctness (software) | Every push |
| T5 | Metal suite (paravirtual) + D3D12 suite (WARP) + NEON bit-for-bit | GitHub CI (macos-14, windows-2022) | Backend correctness | Every push |
| T6 | Web: wasm-check + canvas parity Chromium/Firefox/WebKit | GitHub CI | Web backend | Every push |
| T7 | **Parity matrix certificate** (§6.1): CPU oracle vs Vulkan(lavapipe)+Metal+d3d12(WARP) | GitHub CI | Cross-backend contract rows | Every push |
| T8 | `raster-identity.sh` byte gate on fixture gallery (30 files) | GitHub CI | Determinism regression | Every push |
| T9 | Golden shots vs committed baseline + diff artifact | GitHub CI | Visual regression | Every push (needs FW-14) |
| T10 | Scaffold-external build (`vieww new` → cargo build, no path deps) | GitHub CI | FW-01/B16 | Every push + pre-publish |
| T11 | `publish-dry-run` (ordered crates) + plugin-minimal vs last published | GitHub CI | Publishing + ABI compat | Weekly + on release |
| T12 | `size-budget.sh` (counter example, 593,128 B ±5%) | GitHub CI + self-hosted | Binary bloat | Every push |
| T13 | IME replay harness (CJK commit/preedit/candidate rect) | GitHub CI | Text input correctness | Every push |
| T14 | A11y: audit crate + widget-gallery lint; XCUITest on simulator; AT-SPI (Orca) assertions; UIA tree dump | GitHub CI (macOS), self-hosted Linux, self-hosted Windows | FW-10 | macOS weekly; self-hosted nightly |
| T15 | Crash-report child-process test (abort → FileSink → replay) | GitHub CI + self-hosted Windows | FW-12 | Every push |
| T16 | Hot-reload 50-cycle soak + leak bound + state-preservation rate | GitHub CI | FW-13 | Every push |
| T17 | Hardware run: env, gpu, nvidia, gallery, baseline, size, stress, desktop, audio | Self-hosted Linux | Real-GPU correctness + startup/RSS/idle | Nightly + on demand |
| T18 | **perf-budget gate** (FW-08): fixtures receipts vs `budgets.json` (p50/p95/RSS) | Self-hosted Linux + Windows | Performance regression | Every hardware run (nightly) |
| T19 | Parity suites on real GPUs (Intel Mesa + NVIDIA + D3D12 iGPU) | Self-hosted Linux + Windows | Hardware parity incl. B21 NVIDIA post-pass issue | Nightly |
| T20 | Desktop suite on real windows + HiDPI + two-monitor | Self-hosted | G1.10, §3.2 | Nightly / weekly |
| T21 | Device farm: device suite (cpu/gpu/gpuscale/cpubare/gpuserial/auto passes), a11y, IME, PSS, planner telemetry | Device farm (3 Android + 3 iOS) | Mobile budgets, known-failures.txt honesty | Weekly + pre-release |
| T22 | Device farm: planner-decision quality + tier promotion verdict (`gpuscale_verdict.py`) | Device farm | FW-22 | Pre-release |
| T23 | Release train: tag → full `ci/vieww gate` on 3 OS + installers + SHA256SUMS + SBOM + parity certificate attached | GitHub CI + both self-hosted | FW-03/06 | Per release |
| T24 | Updater e2e (serve v2 → swap → relaunch) | Self-hosted Linux + Windows | FW-24 | Per release |
| T25 | Scene-IR round-trip property test (random scene → encode → decode → identical bytes) | GitHub CI | FW-25 | Every push once built |

Priority if capacity is constrained: T1→T4→T10→T17→T18→T21 — i.e., make the existing gate live, prove external usability, then put the hardware receipts to work.

---

## 8. Sources

**Code (read in full or in targeted depth):**
`vieww_base/Cargo.toml` (workspace, profiles, lints); `rust-toolchain.toml`; `deny.toml`; `PENDING.md` (all 1,076 lines); `TRACKER.md` (headers + 2026-10-19/18/15 sections); `docs/`: `FIDELITY.md`, `GPU-RENDERER-STATUS.md`, `FRAMEWORK-CAPABILITY-PARITY.md`, `release/BETA-RELEASE-CHECKLIST.md`, `guide/*`, `RELEASE-CHECK.md`; `crates/`: `vieww-hal/src/lib.rs`, `vulkan/*` (structure + unsafe counts), branch: `scene_exec.rs`, `d3d12/scene.rs`, `metal/scene.rs` (headers), `vulkan/scene.rs` diff; `vieww-render/src/{event_loop,focus,semantics}.rs`; `vieww-element/src/{error,snapshot,tree}.rs`; `vieww-paint/src/native/*` (listing); `vieww-text/src/lib.rs`, `font.rs`; `vieww-foundation/src/editing.rs` (IME); `vieww-platform-winit/src/{crash,latency,a11y,quality,cadence,rasterizer_choice}.rs` + branch `mixed.rs`/`native.rs` diff; `vieww-platform-web/src/lib.rs`; `vieww-platform-web-dom/src/lib.rs`; `vieww-plugin/src/{lib,abi,registry}.rs`; `vieww-reload/src/lib.rs`; `vieww-cli/src/main.rs`; `vieww-build/src/{lib,package,scaffold}.rs`; `vieww-devtools/` (listing); `vieww-accessibility/src/lib.rs`; `vieww-test-harness/src/{visual,parity}.rs`; `examples/fixtures/src/receipt.rs`; `packaging/package.sh`; CI: `.github/workflows/{ci,framework-checks,hardware,device-farm}.yml`, `vieww_base/.github/workflows/ci.yml` (=dead release-gate), `.github/workflows-pending/*`, `ci/check/{checks,coverage,size-budget,raster-identity,cross-check,wasm-check}.sh`, `ci/hardware/{linux.sh,close-remaining.sh,README.md}`, `ci/mobile/{device-suite.sh,device_farm.py,device-farm/known-failures.txt}`, `ci/release/gate.sh`, `ci/vieww`. Branch diff: `git diff main origin/feature/main_fixes --stat` and per-file diffs for planner/mixed/native.

**Web (fetched, HTTP 200 at research time):**
- Flutter: Impeller — https://docs.flutter.dev/perf/impeller · Web renderers (WebGL/WebGPU, CanvasKit/Skwasm) — https://docs.flutter.dev/platform-integration/web · Testing overview — https://docs.flutter.dev/testing/overview · Integration tests — https://docs.flutter.dev/testing/integration-tests · Android release — https://docs.flutter.dev/deployment/android · iOS release — https://docs.flutter.dev/deployment/ios · Obfuscation — https://docs.flutter.dev/deployment/obfuscate · CI workflow — https://github.com/flutter/flutter/blob/master/.github/workflows/ci.yaml · Devicelab — https://github.com/flutter/flutter/blob/master/dev/devicelab/
- Shorebird (Flutter code push) — https://shorebird.dev/
- React Native: New architecture — https://reactnative.dev/architecture/landing-page · Fast Refresh — https://reactnative.dev/docs/fast-refresh · Hermes — https://reactnative.dev/docs/hermes
- Expo: EAS Update — https://docs.expo.dev/eas-update/introduction/ · Submit — https://docs.expo.dev/submit/introduction/ · App signing — https://docs.expo.dev/app-signing/
- Sentry for Flutter (crash/symbol pipeline) — https://docs.sentry.io/platforms/flutter/
- Apple: Notarization — https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- Fastlane (store automation) — https://docs.fastlane.tools/ · match — https://docs.fastlane.tools/actions/match/
- Slint — https://slint.dev/ · docs — https://docs.slint.dev/latest/ · VS Code live preview — https://marketplace.visualstudio.com/items?itemName=Slint.slint
- Dioxus — https://dioxuslabs.com/ · getting started (dx serve) — https://dioxuslabs.com/learn/0.6/getting_started/
- iced — https://iced.rs/ + README (wgpu/tiny-skia renderers, Elm architecture) — https://github.com/iced-rs/iced
- libcosmic (iced-based COSMIC toolkit) — https://github.com/pop-os/libcosmic · COSMIC app template — https://github.com/pop-os/cosmic-app-template
- Makepad — https://makepad.nl/
- Tauri distribution (signing/SBOM peer reference) — https://tauri.app/distribute/
- Google Play Console app setup — https://support.google.com/googleplay/android-developer/answer/9859152

---

*End of Task 2-a research. 26 gaps catalogued (6 P0 / 8 P1 / 8 P2 / 2 P3, +2 branch-resolved). Overall verdict: 5/10 — world-class core, pre-product shell; the fastest path to "on top of the ladder" is release engineering (FW-01..06), not more renderer work.*
