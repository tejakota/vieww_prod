# Vieww Beta Release Checklist

> **Filled by `ci/vieww checklist`** on 2026-10-25 07:01 UTC
> from 4 run(s): `gate-linux-export` (linux-cpu), `gate-macos-cpu` (macos-cpu), `gate-macos-gpu` (macos-gpu), `gate-windows-cpu` (windows-cpu).
> ✅ 36 passed · ❌ 7 failed · ⚠️ 0 software GPU · ⏭️ 5 skipped · ☐ 128 still open (manual, or no run for that column).
> Only automated cells are filled. Every ☐ left is for a person.

| | |
|---|---|
| **Release** | |
| **Version** | 0.1.0 |
| **Certified commit / tag** | e37c4ca2069192a1edaab617cc5ecf02635cc41c |
| **Release date** | |
| **Release owner** | |

## How to use this

This beta is certified with the machines at hand:

| Leg | Where | Command |
|---|---|---|
| Linux, CPU pass | Your laptop | `ci/vieww gate --mode cpu` |
| Linux, GPU pass | The same laptop, afterwards | `ci/vieww gate --mode gpu`. This is the narrow GPU slice; see below. |
| Windows, CPU pass | GitHub-hosted runner | Actions → **release-gate** → Run workflow |
| macOS, CPU pass | GitHub-hosted runner | Same workflow run as Windows |
| macOS, GPU pass (Apple paravirtual GPU through MoltenVK) | GitHub-hosted runner | Same workflow run. Pixels are valid evidence; timings are not. |
| Linux export route (Android `.apk`, Windows cross `.exe`) | GitHub-hosted Ubuntu runner | Same workflow run, job `linux-export`. Your laptop's gate also runs it if the Android toolchain is installed. |
| Windows / macOS GPU and real windows | A real PC and Mac, if one can be borrowed | `ci/vieww gate --mode gpu`; otherwise WAIVED (see B15) |

1. Freeze a commit and tag it as a candidate. Every run must come from that exact commit, and G0.12 checks it.
2. **Linux:** run `ci/vieww gate --mode cpu`, then `ci/vieww gate --mode gpu`. Keep the laptop plugged in and don't use it heavily while the timing suites run.
3. **Windows and macOS:** push the tag, or run the workflow by hand. It uploads `gate-windows-cpu`, `gate-macos-cpu`, `gate-macos-gpu`, `gate-linux-export`, the installers, and a merged `checklist-ci` that also contains every log.
4. **Merge** the downloaded run folders with the Linux ones:

   ```bash
   ci/vieww checklist target/release-gate/linux-cpu-*/ target/release-gate/linux-gpu-*/ gate-windows-cpu/ gate-macos-cpu/ gate-macos-gpu/ gate-linux-export/ -o CHECKLIST.md
   ```

5. Do the **[manual]** rows by hand on the installed packages.

**Disk:** builds are lean, and each build tree is deleted when its stages finish. Plan for roughly 30 GB free for a CPU pass and under 10 GB for the GPU pass. Both are estimates; `logs/disk.tsv` records the real numbers.

### The GPU pass (`--mode gpu`)

Only the checks a GPU can change. Everything else renders on the CPU and gives the same answer with or without a GPU, so the CPU pass or CI already covered it.

- Device class: real hardware, not a software renderer.
- The Vulkan compositor parity suite (17 pixel-exact tests at 128×96).
- The GPU workload (640×400, 24 frames).
- The fixture census.
- The real-window desktop suite.

Every GPU test is small: its largest render target is a few MB, so a 2 GB GPU is ample. On a laptop with two GPUs, the framework picks the discrete one. Choose explicitly with `VIEWW_VK_DEVICE=<part of the name>`, e.g. `VIEWW_VK_DEVICE=intel`. `vulkaninfo --summary` lists the names. Use whichever GPU has a working Vulkan driver; the Intel HD 620 on Mesa does.

**Verdicts**

- **PASS**: verified, with evidence.
- **FAIL**: a known defect. P0 and P1 items block the release.
- **N/A**: not applicable, with a reason.
- **WAIVED**: an accepted risk, with owner, rationale and issue.

**Release policy:** the beta ships when every P0 below is closed, and every row in the Linux, Windows and macOS columns has a verdict with evidence.

### What "CPU" and "GPU" mean for this product

These are facts about the current code, and they decide what each column can prove.

| Layer | Linux | Windows | macOS |
|---|---|---|---|
| **Pixels users see** | CPU renderer (`vieww_paint::native`) | CPU renderer | CPU renderer |
| **How pixels reach the window** | Vulkan swapchain | Vulkan swapchain | Vulkan through **MoltenVK** |
| **Runtime needed to open any window** | Vulkan loader + driver (Mesa/NVIDIA/AMD) | Vulkan runtime (ships with GPU drivers) | MoltenVK. **Not bundled today (B1).** |
| **GPU compositor** (`vieww-gpu` + Vulkan executor) | Headless only, parity-tested | Headless only | Headless via MoltenVK |
| **D3D12 / Metal executors** | — | **Not implemented**: BUILD-ONLY, not claimed | **Not implemented**: BUILD-ONLY, not claimed |

So "**GPU testing**" means two things:

- Presentation works on the real driver.
- The headless GPU compositor matches the CPU renderer on real hardware.

"**CPU testing**" means the product still works and stays within budget on a weak machine. A machine with no Vulkan driver at all cannot open Studio. That is a documented limitation (B6), not a pass.

Software Vulkan (lavapipe, llvmpipe, SwiftShader) proves correctness only. `--mode gpu` fails on it.

---

## Gate 0: Release identity and repository integrity

| ID | Check | How | Result | Evidence |
|---|---|---|---|---|
| G0.1 | Workspace version is the intended beta version, e.g. `0.1.0-beta.1` | [auto] records the version; a person confirms | 📝 RECORDED (0.1.0) | `gate-macos-cpu/host.txt`<br>`gate-windows-cpu/host.txt` |
| G0.2 | `Cargo.lock` is current | [auto] `cargo metadata --locked` | ✅ PASS | `gate-macos-cpu/logs/G0.2.txt`<br>`gate-windows-cpu/logs/G0.2.txt` |
| G0.3 | `rustc` matches `rust-toolchain.toml` (1.98.1) on every machine | [auto] | ✅ PASS | `gate-macos-cpu/host.txt`<br>`gate-macos-gpu/host.txt`<br>`gate-windows-cpu/host.txt` |
| G0.4 | No `.orig`, `.rej` or `.bak` files, nested archives, stale evidence, generated output or local paths | [auto] `ci/vieww clean` | ✅ PASS | `gate-macos-cpu/logs/G0.4.txt`<br>`gate-windows-cpu/logs/G0.4.txt` |
| G0.5 | Git working tree is clean and the tag points at the certified commit | [auto] plus a person checks the tag | ✅ PASS | `gate-macos-cpu/logs/G0.5.txt`<br>`gate-windows-cpu/logs/G0.5.txt` |
| G0.6 | `LICENSE` present and correct | [auto] present; [manual] correct | ✅ PASS | `gate-macos-cpu/logs/G0.6.txt`<br>`gate-windows-cpu/logs/G0.6.txt` |
| G0.7 | No private keys, tokens or credentials | [auto] pattern scan | ✅ PASS | `gate-macos-cpu/logs/G0.7.txt`<br>`gate-windows-cpu/logs/G0.7.txt` |
| G0.8 | `cargo deny check all` (licences, advisories, bans, sources) | [auto]. Fails if cargo-deny is missing. | ✅ PASS | `gate-macos-cpu/logs/G0.8.txt`<br>`gate-windows-cpu/logs/G0.8.txt` |
| G0.9 | `README.md` describes the beta and its supported platforms accurately | [manual] | ☐ |  |
| G0.10 | Known limitations are documented (release notes template, Known limitations section) | [manual] | ☐ |  |
| G0.11 | `CHANGELOG.md` or release notes are finalized. **Neither exists yet (B4).** | [manual] | ☐ |  |
| G0.12 | Every run in this checklist used the same source digest (`cert/meta/source.txt`) | [auto] `ci/vieww checklist` across all runs | ✅ PASS | `gate-linux-export/cert/meta/source.txt`<br>`gate-macos-cpu/cert/meta/source.txt`<br>`gate-macos-gpu/cert/meta/source.txt`<br>`gate-windows-cpu/cert/meta/source.txt` |

## Gate 1: Framework certification

`ci/vieww gate` covers every [auto] row below.

For a single-row rerun:

- `ci/vieww checks`
- `ci/vieww platform <os>`
- `ci/vieww certify` (Windows: `pwsh ci/certify/certify-windows.ps1`)

`checks` runs every stage and lists every failure at the end.

### 1.1 Build and test matrix

Columns: Linux · Windows · macOS

| ID | Check | Linux | Windows | macOS |
|---|---|:-:|:-:|:-:|
| G1.1 | `ci/vieww checks`: fmt, clippy `-D warnings` and docs `-D warnings`, all with `--features vieww-paint/native,vieww-hal/vulkan`; check `--all-targets`; MSRV 1.89; deny; workspace tests; fixtures; hot reload; packaging smoke | ☐ | ❌ FAIL (exit=1) | ❌ FAIL (exit=1) |
| G1.2 | `ci/vieww platform <os>`: native backends build for this OS | ☐ | ✅ PASS | ✅ PASS |
| G1.3 | `ci/vieww certify`: overall verdict `FAILED=0`. The GPU run should also show `COMPLETE=true`. | ☐ | ❌ FAIL (exit=1) | ❌ FAIL (exit=1) |

### 1.2 Rendering: CPU and GPU

Columns: Linux CPU · Linux GPU · Windows CPU · Windows GPU · macOS CPU · macOS GPU

| ID | Check | L-CPU | L-GPU | W-CPU | W-GPU | M-CPU | M-GPU |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|
| G1.4 | GPU device class: real hardware | N/A | ☐ | N/A | ☐ | N/A | ✅ PASS |
| G1.5 | Vulkan compositor pixel parity (17 tests, max channel difference 1) | N/A | ☐ | N/A | ☐ | N/A | ❌ FAIL (exit=101) |
| G1.6 | GPU census: 23/23 fixtures plan complete, 0 flat-region mismatches | N/A | ☐ | N/A | ☐ | N/A | ✅ PASS |
| G1.7 | CPU renderer: premium UI (0 overflows) and fixtures | ☐ | N/A (CPU work) | ✅ PASS | N/A (CPU work) | ✅ PASS | N/A (CPU work) |
| G1.8 | Animation stress p95 ≤ 16.6 ms **on release-class hardware** (B7). Hosted CI runners are shared, so treat their timings as indicative. | ☐ | N/A (CPU work) | ✅ PASS | N/A (CPU work) | ✅ PASS | N/A (CPU work) |
| G1.9 | Vieww Standard: all 12 clauses measured, 0 steady allocations, 0 unsupported GPU commands | ☐ | N/A (CPU work) | ✅ PASS | N/A (CPU work) | ✅ PASS | N/A (CPU work) |
| G1.10 | Desktop suite: real windows open, first frame is not blank, no panic | ☐ | ☐ | ⏭️ SKIPPED | ☐ | ⏭️ SKIPPED | ⏭️ SKIPPED |
| G1.11 | D3D12 backend | N/A | N/A | N/A | N/A (BUILD-ONLY) | N/A | N/A |
| G1.12 | Metal backend | N/A | N/A | N/A | N/A | N/A | N/A (BUILD-ONLY) |
| G1.13 | Cross-machine shots: pixels byte-identical to another OS's `shots/`. Run `ci/vieww shots --against gate-windows/shots` (and the macOS one) on Linux. | ☐ | N/A (CPU pixels) | ☐ | N/A (CPU pixels) | ☐ | N/A (CPU pixels) |
| G1.14 | A machine **without** a Vulkan driver shows a clear error, not a crash or a blank window | ☐ | N/A | ☐ | N/A | ☐ | N/A |

## Gate 2: Vieww Studio

| ID | Check | Linux | Windows | macOS |
|---|---|:-:|:-:|:-:|
| G2.1 | [auto] `ci/vieww studio` passes in full (not `--quick`): `release_check`, `tour`, `walkthrough` | ☐ | ✅ PASS | ✅ PASS |
| G2.2 | [manual] Gallery `studio/index.html` reviewed: light and dark, normal and small window. No clipped widgets, missing glyphs, overlaps, stray lines or stale layers. Icons aligned. Hover, focus and pressed states intentional. Empty, error and loading states intentional. | ☐ | ☐ | ☐ |
| G2.3 | [auto with `gate --hidpi`] desktop suite on a HiDPI display (N/A if none) | ☐ | ☐ | ☐ |
| G2.4 | [auto with `gate --multi-monitor`] desktop suite across two displays (N/A if none) | ☐ | ☐ | ☐ |
| G2.5 | [manual] Window: move, resize, minimize, restore, close. Dialogs and child windows behave. Exits with no hang. | ☐ | ☐ | ☐ |
| G2.6 | [manual] OS conventions: ⌘ on macOS and Ctrl elsewhere. Native clipboard round-trip. File picker. Data dir: Linux `$XDG_DATA_HOME` or `~/.local/share`; Windows `%APPDATA%`; macOS `~/Library/Application Support`. | ☐ | ☐ | ☐ |

### 2.1 Studio's export route: desktop, Windows, Android, iOS

This is what a user's **Export** button runs, not the framework's own examples. `ci/vieww gate` runs `ci/certify/export-suite.sh`, which:

1. Scaffolds a Rust project and a Say project exactly as New Project does.
2. Compiles both for every target.
3. Runs Studio's own export plans and checks each produced its file.

The projects are built **outside the repository**, like a user's project, so this repo's `.cargo/config.toml` can't affect them. A missing toolchain is recorded as SKIPPED with the install command Studio would show. The hosted CI jobs install everything needed.

**Toolchains each export needs**

| Export | Needs | Hosted CI job |
|---|---|---|
| Desktop binary | cargo | all |
| Windows `.exe` | Windows: nothing extra. Linux/macOS: MinGW-w64 + `rustup target add x86_64-pc-windows-gnu` | windows-cpu (native), linux-export (cross) |
| Android `.apk` | Android SDK (`ANDROID_HOME`), NDK (`ANDROID_NDK_HOME`), JDK 17, Gradle 8.7+, `cargo install cargo-ndk`, `rustup target add aarch64-linux-android` | linux-export, windows-cpu, macos-cpu |
| iOS simulator `.app` | macOS, Xcode, `rustup target add aarch64-apple-ios-sim` | macos-cpu |
| iOS device `.ipa` | The above, plus an Apple signing identity | manual (G2.13) |

A scaffolded project pins `channel = "stable"`, so on your own machine add those targets for `stable` as well, e.g. `rustup target add --toolchain stable aarch64-linux-android`.

| ID | Check | Linux | Windows | macOS |
|---|---|:-:|:-:|:-:|
| G2.7 | [auto] Scaffolded Rust and Say projects compile for desktop, Android and Windows, plus iOS on macOS (`cargo check --target`) | ✅ PASS | ✅ PASS | ❌ FAIL (compile/ios-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 |     [1m[94m|[0m | [1m;compile/ios-sim-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 |     [1m[94m|[0m | ) |
| G2.8 | [auto] Export → desktop binary is produced | ✅ PASS | ✅ PASS | ✅ PASS |
| G2.9 | [auto] Export → Windows `.exe` is produced (native on Windows, MinGW cross elsewhere) | ✅ PASS | ✅ PASS | ⏭️ SKIPPED |
| G2.10 | [auto] Export → Android `.apk` is produced (cargo-ndk + Gradle) | ✅ PASS | ✅ PASS | ✅ PASS |
| G2.11 | [auto] Export → iOS simulator `.app` is produced | N/A | N/A | ❌ FAIL (export/ios-sim FAIL step 'Build (iOS simulator)' (cargo build --release --target aarch64-apple-ios-sim): exit exit status: 101; last lines:     [1m[94m|[0m | [1m[94m201[0m [1m[94m|[0m             l) |
| G2.12 | [manual] The exported `.apk` installs and launches on an arm64 phone (developer mode), and the `.app` launches in the iOS simulator (`xcrun simctl install booted …`) | ☐ | ☐ | ☐ |
| G2.13 | [manual] Signed iOS `.ipa` from a machine with an Apple developer identity (see B3) | N/A | N/A | ☐ |
| G2.14 | [manual] The same exports from an **installed** Studio, not a checkout (see B16) | ☐ | ☐ | ☐ |

## Gate 3: Packaged build and install

| ID | Check | Linux | Windows | macOS |
|---|---|:-:|:-:|:-:|
| G3.1 | [auto] `ci/vieww artifacts`: installers build, and the bundle compiles a guest against its own SDK | ☐ | ✅ PASS | ✅ PASS |
| G3.2 | [manual] Clean machine (VM or fresh user with no checkout and no Rust): install, launch, launcher or Start-menu entry, icon | ☐ | ☐ | ☐ |
| G3.3 | [manual] Uninstall removes the app and **keeps user projects**. Reinstall or upgrade over the previous beta works. | ☐ | ☐ | ☐ |
| G3.4 | [manual] No missing runtime: Linux Vulkan loader; Windows DLLs and Vulkan runtime; macOS **MoltenVK (B1)** | ☐ | ☐ | ☐ |

**Artifacts `packaging/package.sh --installers` produces today**

| OS | Artifacts | Build tools |
|---|---|---|
| Linux | `viewwstudio-linux-x86_64.deb`, `.AppImage`, `.tar.gz` | `dpkg-deb`; `APPIMAGETOOL` must be set |
| Windows | `viewwstudio-windows-x86_64.msi`, `.zip` | WiX (`dotnet tool install --global wix`) |
| macOS | `viewwstudio-macos-<arch>.dmg`, one per architecture | `hdiutil`. **No macOS archive exists**, unlike the draft's assumption. |

Linux and Windows file names say `x86_64` whatever the host. Do not publish ARM64 builds under those names (B8).

## Gate 4: Packaged runtime smoke test

Run the **installed** app. Columns: Linux · Windows · macOS

| ID | Check | L | W | M |
|---|---|:-:|:-:|:-:|
| G4.1 | Launch from installed location; startup; main window renders | ☐ | ☐ | ☐ |
| G4.2 | Create or open a workspace, edit, save, reopen | ☐ | ☐ | ☐ |
| G4.3 | **Preview/guest build using the installed SDK bundle** (not the checkout) | ☐ | ☐ | ☐ |
| G4.4 | Export sheet from the installed app: desktop, and Android where its toolchain is installed (G2.14) | ☐ | ☐ | ☐ |
| G4.5 | Clipboard, file picker, theme switch, resize and DPI | ☐ | ☐ | ☐ |
| G4.6 | Quit and relaunch: session and settings restored | ☐ | ☐ | ☐ |

## Gate 5: Upgrade, persistence and failure recovery

| ID | Check | L | W | M |
|---|---|:-:|:-:|:-:|
| G5.1 | Previous beta's workspace and settings open after upgrade (N/A for the first beta) | ☐ | ☐ | ☐ |
| G5.2 | Unsaved-changes warning is correct | ☐ | ☐ | ☐ |
| G5.3 | A failed guest build is reported clearly; Studio stays usable; the next build succeeds | ☐ | ☐ | ☐ |
| G5.4 | Restart after a failure needs no data deletion | ☐ | ☐ | ☐ |

## Gate 6: Performance and stability

Record **release-hardware** numbers separately from headless results. The measured suites land in `cert/quality/metrics.txt`.

| ID | Check | L | W | M |
|---|---|:-:|:-:|:-:|
| G6.1 | Startup time (cold and warm) recorded | ☐ | ☐ | ☐ |
| G6.2 | Idle CPU and steady-state memory recorded | ☐ | ☐ | ☐ |
| G6.3 | Scroll and animation feel smooth on the weakest supported machine (the CPU-mode machine) | ☐ | ☐ | ☐ |
| G6.4 | 30 minutes of open, edit and rebuild cycles: no growth or degradation | ☐ | ☐ | ☐ |
| G6.5 | Large file (10k+ lines) editing is responsive | ☐ | ☐ | ☐ |

## Gate 7: Accessibility, input and text

| ID | Check | L | W | M |
|---|---|:-:|:-:|:-:|
| G7.1 | Keyboard-only navigation of primary workflows; focus visible | ☐ | ☐ | ☐ |
| G7.2 | Selection, cut, copy, paste, undo, redo, find and replace | ☐ | ☐ | ☐ |
| G7.3 | IME entry (e.g. Japanese, Chinese) | ☐ | ☐ | ☐ |
| G7.4 | Screen reader exposes the main controls (Orca, Narrator, VoiceOver) | ☐ | ☐ | ☐ |
| G7.5 | Unicode, long lines, wrapping and mixed sizes render without layout corruption | ☐ | ☐ | ☐ |

## Gate 8: Supply chain and artifact integrity

| ID | Check | Result |
|---|---|:-:|
| G8.1 | [auto] `SHA256SUMS` generated for every artifact on each OS | ✅ PASS |
| G8.2 | [auto] `MANIFEST.txt` records version, commit, toolchain, host and signing state | ✅ PASS |
| G8.3 | [manual] Merge the three OS files with `ci/vieww sums linux/ windows/ macos/`, then publish as `SHA256SUMS`. The download page links that exact name. | ☐ |
| G8.4 | [auto] `ci/vieww checklist`: all artifacts built from the certified commit (compare `git_rev` in the three manifests) | ✅ PASS |
| G8.5 | [manual] Release archive contents reviewed (`tar tzf`, `unzip -l`, DMG mounted) | ☐ |
| G8.6 | [manual] Reproducibility documented: **not reproducible today** (timestamps, bundled toolchain) | ☐ |
| G8.7 | Optional: SBOM (`cargo cyclonedx`) and provenance attestation | ✅ PASS |

## Gate 9: Signing and trust

`packaging/package.sh` signs nothing. There are two options:

- Ship an **unsigned developer beta** and say so on the download page and in the release notes. Mark G9.* WAIVED with an owner.
- Close B2 and B3.

| ID | Check | Result |
|---|---|:-:|
| G9.1 | Linux: package signing policy decided; integrity via published `SHA256SUMS` documented | ☐ |
| G9.2 | Windows: Authenticode certificate; `.exe` and `.msi` signed; signature validates on a clean machine; SmartScreen behaviour documented | ☐ |
| G9.3 | macOS: Developer ID signed with Hardened Runtime; notarized; ticket stapled; Gatekeeper launch verified on a clean Mac | ☐ |

## Gate 10: Publishing

| ID | Check | Result |
|---|---|:-:|
| G10.1 | Release notes written from `docs/release/RELEASE-NOTES-TEMPLATE.md`: supported OS and arch, install steps, known limitations, signing state | ☐ |
| G10.2 | Tag created from the certified commit; release draft created | ☐ |
| G10.3 | `packaging/site/index.html` `__REPO__` resolved; every artifact name matches the page | ☐ |
| G10.4 | Assets and checksums uploaded and reviewed before publishing | ☐ |
| G10.5 | After publishing: download, verify checksum, install and launch on one clean machine per OS | ☐ |

---

## Known blockers (pre-filled from the repository review)

| ID | Sev | Description | Fix | Status |
|---|---|---|---|---|
| B1 | **P0** | **MoltenVK is not bundled in the macOS `.app`.** Without the Vulkan SDK installed, Studio cannot open a window on a user's Mac. The instance and device portability flags needed for MoltenVK were added in this pass, but have **not been run on a Mac yet**. | Copy `libMoltenVK.dylib` into `Contents/Frameworks` and `MoltenVK_icd.json` into `Contents/Resources/vulkan/icd.d`. `vieww_hal::vulkan::load_entry` now looks there first (`Contents/Frameworks/libvulkan.1.dylib`, then `libMoltenVK.dylib`), then in `$VULKAN_SDK` and Homebrew. The remaining work is copying those files in `package.sh`. Verify with G3.2 and G3.4 on a clean Mac. | Open |
| B2 | P0 for public, P2 for dev preview | Windows artifacts are unsigned (SmartScreen warning) | Authenticode certificate plus a `signtool` step, or WAIVE with disclosure | Open |
| B3 | P0 for public, P2 for dev preview | macOS artifacts are neither signed nor notarized. Gatekeeper blocks the first launch. | Developer ID, Hardened Runtime, `notarytool`, staple, or WAIVE with disclosure | Open |
| B4 | P1 | No `CHANGELOG.md` and no release notes | Fill in `RELEASE-NOTES-TEMPLATE.md` | Open |
| B5 | P1 | Version is `0.1.0`, not a beta pre-release version | Set `[workspace.package] version` and re-lock | Open |
| B6 | P1 | A Vulkan runtime is required on every OS; machines or VMs without one cannot open Studio | Document it in the release notes (G1.14). A CPU-only presentation fallback would be later work. | Open |
| B7 | P1 | `test-animation-stress` misses its 16.6 ms budget. p95 was 24 ms on a 2-core container; on the first real run (Linux, i5-7200U, 2017 laptop) it was **35 ms**, with 72 of 72 frames over budget. | Decide the minimum supported CPU and publish it, or optimise the 200-tile animation path. Re-measure G1.8 on that minimum machine. | Open |
| B8 | P2 | Linux and Windows artifact names hard-code `x86_64` | Use the host arch in `package.sh`, or build x86_64 only | Open |
| B9 | P2 | No hosted CI | `.github/workflows/release-gate.yml` runs the Windows and macOS legs | Fixed |
| B10 | P2 | GPU compositor: geometry edges have no anti-aliasing; not used by the live window | Not user-facing in this beta. Keep it out of the marketing claims. | Accepted |
| B11 | **P0** | **Desktop app segfaulted on real Linux (Wayland, Intel Mesa)** during the desktop suite. Cause: window teardown destroyed the winit window and the Vulkan device while the swapchain and `VkSurfaceKHR` were still alive (Vulkan validation: `VUID-vkDestroyDevice-device-05137`, `VUID-vkDestroyInstance-instance-00629`). Any closing dialog or app exit could crash. | **Fixed** in this tree (`Drop for Gpu` releases the swapchain first). Validation errors are now 0 on X11 and Wayland. Confirm with G1.10 on the same laptop. | Fixed, awaiting re-run |
| B12 | P1 | Vieww Standard startup took 52.7 ms against a 33.3 ms budget on the i5-7200U. The same run measured the **system font scan at 46 s**. Every window runs that scan synchronously before its first frame (`use_system_fonts`), so Studio may take tens of seconds to open on slower disks. | Measure Studio launch time (G6.1) on that laptop. If it's slow, move the scan off the first frame or cache it. | Open |
| B13 | P2 | Layout overflow warnings while Studio runs: `RenderRow` overflowed by 9 px (release_check) and by up to 73 px (walkthrough); `RenderColumn` overflowed by 41 px in the desktop demo. | Review in G2.2; fix or accept | Open |
| B14 | P2 | On a pure Wayland session with no XWayland, the clipboard does not work: `arboard` is built without Wayland data-control. | Document it, or enable `arboard`'s `wayland-data-control` feature and test on GNOME and KDE | Open |
| B15 | P1 | **No real Windows or Mac in the test plan.** Hosted CI runners have no GPU and no Vulkan driver, so Studio cannot open a window there. Automated builds, tests and installers are covered, but "the installed app opens and works" (G1.10, G3.2–G3.4, G4) is not, on either OS. | Borrow one Windows PC (any GPU with a current driver) and one Mac for an hour each: install the CI-built installer and do G3.2 + G4. Run `ci/vieww gate --mode gpu` there if Rust is installed. Otherwise WAIVE for a Linux-first developer preview and say so in the release notes. | Open |
| B16 | **P0 for exports** | **A project created by an installed Studio cannot be built.** New Project writes `vieww = { path = <checkout> }` only when the Studio binary finds the source checkout it was compiled from (`scaffold::checkout_root`, a compile-time path). On a user's machine that path doesn't exist, so the project gets `vieww = "0.0.1"`. No such crate is on crates.io, and the workspace version is 0.1.0. Preview still works (it uses the bundled SDK), but `cargo build`, Export and every mobile build fail with a dependency resolution error. Invisible on a developer machine, where the checkout exists. | Publish the vieww crates at the workspace version and scaffold that version, or ship the crate sources in the SDK bundle and scaffold a path to them. Verify with G2.14. | Open |
| B17 | P1 | **Android templates did not compile.** The Say template called `run_android(mount, android)` with its arguments swapped, and both templates called `log::error!` without depending on `log`. Hidden because those lines are `#[cfg(target_os = "android")]`, which no desktop build compiles. | **Fixed** in this tree, with a regression test (`the_mobile_entry_points_match_the_platform_api`) and a real Android compile in G2.7. Confirm on CI. | Fixed, awaiting CI |
| B18 | P1 | **iOS simulator export could not work.** It demanded a signing identity (simulator builds don't need one), and the `.app` stayed in Xcode's DerivedData, so the export folder never got the file. | **Fixed**: identity only required for `.ipa`; `xcodebuild` writes into the export folder. Confirm with G2.11 on macOS CI. | Fixed, awaiting CI |
| B19 | P2 | Android export builds `arm64-v8a` only, so it won't install on an x86_64 emulator on an Intel/AMD PC. | Document, or add `x86_64` to the `cargo ndk -t` list and `abiFilters` | Open |
| B20 | P2 | The scaffold pins `channel = "stable"`, but Studio's Toolchains view checks the targets of Studio's own toolchain (1.98.1 here). A user can see "ready" and still have the export fail with a missing target for `stable`. | Pin the scaffold to the Studio's exact version, or check targets inside the project directory | Open |
| B21 | P1 | Vulkan compositor diverges on NVIDIA (Tesla T4, Linux): 10/17 parity tests fail — every test that runs a post pass (layers, blur, blend, shadow, mask); plain draws, gradients, images and strokes pass; a steady frame renders differently the second time; workload parity 31%, census flat-region mismatch. lavapipe passes 17/17 with 0 core validation errors. Not user-facing today (Studio presents CPU pixels, B10), but blocks G1.5/G1.6. | Shader image loads are now bounds-checked (naga `ReadZeroSkipWrite`) to remove one class of driver-dependent UB; `certify` re-runs a failing suite under the Khronos validation layer (`gpu/vulkan-validation.txt`). Re-run `ci/vieww gate --mode gpu` on NVIDIA with `vulkan-validationlayers` installed and fix what it names | Open |

---

## Platform evidence records

Paste `host.txt` from each gate run.

| Field | Linux | Windows | macOS |
|---|---|---|---|
| Machine / OS version | runnervmtai0g · Ubuntu 24.04.5 LTS | runnervmdaln5 · Microsoft Windows [Version 10.0.26100.33650] | cpu: sjc20-cw713-3ed8185c-761c-4fe7-a21a-731850858275-D687AC89F8F8.local · macOS 26.6.2<br>gpu: sjc22-bt151-55998397-d4bb-484e-abd8-fab8b7201825-4640D7798A7F.local · macOS 26.6.2 |
| Arch / CPU | x86_64 · AMD EPYC 9V45 96-Core Processor | x86_64 · AMD EPYC 9V74 80-Core Processor | cpu: arm64 · Apple M1 (Virtual)<br>gpu: arm64 · Apple M1 (Virtual) |
| GPU / driver / Vulkan device | not found (vulkaninfo missing) | Microsoft Hyper-V Video (driver 10.0.26100.1150) | cpu: Apple Paravirtual device · 1.4.2<br>gpu: Apple Paravirtual device · 1.4.2 |
| Display server |  | N/A | N/A |
| Rust / Cargo | rustc 1.98.1 (48a229cea 2026-09-01) · cargo 1.98.1 (797e8a9bc 2026-08-05) | rustc 1.98.1 (48a229cea 2026-09-01) · cargo 1.98.1 (797e8a9bc 2026-08-05) | cpu: rustc 1.98.1 (48a229cea 2026-09-01) · cargo 1.98.1 (797e8a9bc 2026-08-05)<br>gpu: rustc 1.98.1 (48a229cea 2026-09-01) · cargo 1.98.1 (797e8a9bc 2026-08-05) |
| Commit / source digest | e37c4ca2069192a1edaab617cc5ecf02635cc41c | e37c4ca2069192a1edaab617cc5ecf02635cc41c · git-tree:ac106d6 | cpu: e37c4ca2069192a1edaab617cc5ecf02635cc41c · git-tree:ac106d6<br>gpu: e37c4ca2069192a1edaab617cc5ecf02635cc41c · git-tree:ac106d6 |
| Gate folders (gpu, cpu) | `gate-linux-export` | `gate-windows-cpu` | cpu: `gate-macos-cpu`<br>gpu: `gate-macos-gpu` |
| Artifacts + SHA256SUMS |  | viewwstudio-windows-x86_64.zip, viewwstudio-windows-x86_64.msi, viewwstudio.cdx.json | cpu: viewwstudio-macos-aarch64.dmg, viewwstudio.cdx.json |
| Signing status |  | UNSIGNED: viewwstudio-windows-x86_64.msi | cpu: UNSIGNED: viewwstudio-macos-aarch64.dmg |
| Known deviations |  |  |  |
| **Verdict** | cpu: auto ✅ | cpu: auto ❌ 2 failed | cpu: auto ❌ 4 failed<br>gpu: auto ❌ 2 failed |

## Final release decision

| Gate | Status | Owner | Evidence |
|---|---|---|---|
| 0 Repository / identity | 🟡 3 open |  |  |
| 1 Framework (CPU + GPU) | ❌ 5 failed |  |  |
| 2 Studio | ❌ 2 failed |  |  |
| 3–4 Packaged install / runtime | 🟡 28 open |  |  |
| 5 Upgrade / recovery | 🟡 12 open |  |  |
| 6 Performance | 🟡 15 open |  |  |
| 7 Accessibility / input | 🟡 15 open |  |  |
| 8 Supply chain | 🟡 3 open |  |  |
| 9 Signing / trust | 🟡 3 open |  |  |
| 10 Publishing | 🟡 5 open |  |  |
| Blockers B1–B21 closed or waived | ☐ | | |

**BETA RELEASE: NOT APPROVED — 7 automated cell(s) failed**

Approved by: ______ Date: ______ Certified commit: ______

---

## Appendix: automated results per run

### `gate-linux-export` (linux-cpu)

| Row | Check | Verdict | Evidence |
|---|---|---|---|
| PRE.1 | at least 15 GB free for target/ | PASS(95 GB free) | `host.txt` |
| G2.7 | scaffolded Rust + Say projects compile for every target | PASS | `export/results.txt` |
| G2.8 | Studio export: desktop binary | PASS | `export/results.txt` |
| G2.9 | Studio export: Windows .exe | PASS | `export/results.txt` |
| G2.10 | Studio export: Android .apk (cargo-ndk + Gradle) | PASS | `export/results.txt` |
| G2.11 | Studio export: iOS simulator .app | N/A(macOS only — Apple's toolchain) |  |
| G0.12 | same source digest in every run | PASS | `cert/meta/source.txt` |

### `gate-macos-cpu` (macos-cpu)

| Row | Check | Verdict | Evidence |
|---|---|---|---|
| PRE.1 | at least 30 GB free for target/ | PASS(100 GB free) | `host.txt` |
| G0.1 | workspace version | RECORDED(0.1.0) — confirm it is the beta version | `host.txt` |
| G0.2 | Cargo.lock is current (--locked) | PASS | `logs/G0.2.txt` |
| G0.3 | rustc matches rust-toolchain.toml (1.98.1) | PASS | `host.txt` |
| G0.4 | release-clean (no patch leftovers, stale evidence, local paths) | PASS | `logs/G0.4.txt` |
| G0.5 | git working tree clean | PASS | `logs/G0.5.txt` |
| G0.6 | LICENSE present | PASS | `logs/G0.6.txt` |
| G0.7 | no private keys or credentials in the tree | PASS | `logs/G0.7.txt` |
| G0.8 | cargo deny check all | PASS | `logs/G0.8.txt` |
| G1.1 | ci/vieww checks (fmt, clippy, check, docs, MSRV, tests, fixtures, packaging) | FAIL(exit=1) | `logs/G1.1.txt` |
| G1.2 | ci/vieww platform macos | PASS | `logs/G1.2.txt` |
| G1.3 | certification (cpu) | FAIL(exit=1) | `logs/G1.3.txt` |
| G1.4 | GPU device class | SKIPPED(CPU-only run requested) | `cert/quality/stages.txt` |
| G1.5 | Vulkan compositor pixel parity (17 tests) | SKIPPED(CPU-only run requested) | `cert/quality/stages.txt` |
| G1.6 | GPU census: every fixture plans complete | NOT RUN | `cert/quality/stages.txt` |
| G1.7 | CPU renderer: premium UI, fixtures | PASS | `cert/quality/stages.txt` |
| G1.8 | animation stress p95 within budget | PASS | `cert/quality/stages.txt` |
| G1.9 | Vieww Standard (12 clauses, measured) | PASS | `cert/quality/stages.txt` |
| G1.10 | desktop suite (real windows) | SKIPPED(VIEWW_CERT_NO_DESKTOP=1) | `cert/quality/stages.txt` |
| G1.11 | D3D12 backend | N/A(BUILD-ONLY: no ScenePlan executor; not claimed) |  |
| G1.12 | Metal backend | N/A(BUILD-ONLY: no ScenePlan executor; not claimed) |  |
| G1.13x | feature screenshots captured | PASS | `logs/G1.13x.txt` |
| G2.1 | Studio release_check + tour + walkthrough (full, not --quick) | PASS | `logs/G2.1.txt` |
| G2.7 | scaffolded Rust + Say projects compile for every target | FAIL(compile/ios-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 /     [1m[94m/[0m / [1m;compile/ios-sim-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 /     [1m[94m/[0m / ) | `export/results.txt` |
| G2.8 | Studio export: desktop binary | PASS | `export/results.txt` |
| G2.9 | Studio export: Windows .exe | SKIPPED(export/windows: MinGW-w64 linker is missing — install it with: `apt install mingw-w64`, `dnf install mingw64-gcc`, `brew install mingw-w64`, or `choco install mingw`) | `export/results.txt` |
| G2.10 | Studio export: Android .apk (cargo-ndk + Gradle) | PASS | `export/results.txt` |
| G2.11 | Studio export: iOS simulator .app | FAIL(export/ios-sim FAIL step 'Build (iOS simulator)' (cargo build --release --target aarch64-apple-ios-sim): exit exit status: 101; last lines:     [1m[94m/[0m / [1m[94m201[0m [1m[94m/[0m             l) | `export/results.txt` |
| G3.1 | installers build (package.sh --installers) + bundle compiles a guest | PASS | `logs/G3.1.txt` |
| G8.1 | SHA256SUMS generated for every artifact | PASS | `package/SHA256SUMS` |
| G8.2 | toolchain + source recorded with the artifacts | PASS | `package/MANIFEST.txt` |
| G8.7 | SBOM (CycloneDX) for the shipped binary | PASS | `package/viewwstudio.cdx.json` |
| G0.12 | same source digest in every run | PASS | `cert/meta/source.txt` |
| G8.4 | artifacts from the certified commit | PASS | `package/MANIFEST.txt` |

### `gate-macos-gpu` (macos-gpu)

| Row | Check | Verdict | Evidence |
|---|---|---|---|
| PRE.1 | at least 10 GB free for target/ | PASS(100 GB free) | `host.txt` |
| G0.3 | rustc matches rust-toolchain.toml (1.98.1) | PASS | `host.txt` |
| G1.3 | certification (gpu) | FAIL(exit=1) | `logs/G1.3.txt` |
| G1.4 | GPU device class | PASS(virtual GPU: Apple Paravirtual device; pixels are evidence, timings are not) | `cert/quality/stages.txt` |
| G1.5 | Vulkan compositor pixel parity (17 tests) | FAIL(exit=101) | `cert/quality/stages.txt` |
| G1.6 | GPU census: every fixture plans complete | PASS | `cert/quality/stages.txt` |
| G1.10 | desktop suite (real windows) | SKIPPED(VIEWW_CERT_NO_DESKTOP=1) | `cert/quality/stages.txt` |
| G1.11 | D3D12 backend | N/A(BUILD-ONLY: no ScenePlan executor; not claimed) |  |
| G1.12 | Metal backend | N/A(BUILD-ONLY: no ScenePlan executor; not claimed) |  |
| G0.12 | same source digest in every run | PASS | `cert/meta/source.txt` |

### `gate-windows-cpu` (windows-cpu)

| Row | Check | Verdict | Evidence |
|---|---|---|---|
| PRE.1 | at least 30 GB free for target/ | PASS(218 GB free) | `host.txt` |
| G0.1 | workspace version | RECORDED(0.1.0) — confirm it is the beta version | `host.txt` |
| G0.2 | Cargo.lock is current (--locked) | PASS | `logs/G0.2.txt` |
| G0.3 | rustc matches rust-toolchain.toml (1.98.1) | PASS | `host.txt` |
| G0.4 | release-clean (no patch leftovers, stale evidence, local paths) | PASS | `logs/G0.4.txt` |
| G0.5 | git working tree clean | PASS | `logs/G0.5.txt` |
| G0.6 | LICENSE present | PASS | `logs/G0.6.txt` |
| G0.7 | no private keys or credentials in the tree | PASS | `logs/G0.7.txt` |
| G0.8 | cargo deny check all | PASS | `logs/G0.8.txt` |
| G1.1 | ci/vieww checks (fmt, clippy, check, docs, MSRV, tests, fixtures, packaging) | FAIL(exit=1) | `logs/G1.1.txt` |
| G1.2 | ci/vieww platform windows | PASS | `logs/G1.2.txt` |
| G1.3 | certification (cpu) | FAIL(exit=1) | `logs/G1.3.txt` |
| G1.4 | GPU device class | SKIPPED(CPU-only run requested) | `cert/quality/stages.txt` |
| G1.5 | Vulkan compositor pixel parity (17 tests) | SKIPPED(CPU-only run requested) | `cert/quality/stages.txt` |
| G1.6 | GPU census: every fixture plans complete | NOT RUN | `cert/quality/stages.txt` |
| G1.7 | CPU renderer: premium UI, fixtures | PASS | `cert/quality/stages.txt` |
| G1.8 | animation stress p95 within budget | PASS | `cert/quality/stages.txt` |
| G1.9 | Vieww Standard (12 clauses, measured) | PASS | `cert/quality/stages.txt` |
| G1.10 | desktop suite (real windows) | SKIPPED(VIEWW_CERT_NO_DESKTOP=1) | `cert/quality/stages.txt` |
| G1.11 | D3D12 backend | N/A(BUILD-ONLY: no ScenePlan executor; not claimed) |  |
| G1.12 | Metal backend | N/A(BUILD-ONLY: no ScenePlan executor; not claimed) |  |
| G1.13x | feature screenshots captured | PASS | `logs/G1.13x.txt` |
| G2.1 | Studio release_check + tour + walkthrough (full, not --quick) | PASS | `logs/G2.1.txt` |
| G2.7 | scaffolded Rust + Say projects compile for every target | PASS | `export/results.txt` |
| G2.8 | Studio export: desktop binary | PASS | `export/results.txt` |
| G2.9 | Studio export: Windows .exe | PASS | `export/results.txt` |
| G2.10 | Studio export: Android .apk (cargo-ndk + Gradle) | PASS | `export/results.txt` |
| G2.11 | Studio export: iOS simulator .app | N/A(macOS only — Apple's toolchain) |  |
| G3.1 | installers build (package.sh --installers) + bundle compiles a guest | PASS | `logs/G3.1.txt` |
| G8.1 | SHA256SUMS generated for every artifact | PASS | `package/SHA256SUMS` |
| G8.2 | toolchain + source recorded with the artifacts | PASS | `package/MANIFEST.txt` |
| G8.7 | SBOM (CycloneDX) for the shipped binary | PASS | `package/viewwstudio.cdx.json` |
| G0.12 | same source digest in every run | PASS | `cert/meta/source.txt` |
| G8.4 | artifacts from the certified commit | PASS | `package/MANIFEST.txt` |

## Failures, with logs

### `gate-linux-export`

No failed rows.

### `gate-macos-cpu`

#### ❌ G1.1 — ci/vieww checks (fmt, clippy, check, docs, MSRV, tests, fixtures, packaging)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.1.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: items after a test module
   --> examples/wright_film/src/socials.rs:499:1
    |
499 | mod tests {
    | ^^^^^^^^^
...
521 | pub(crate) fn p1_question(ctx: &Ctx) -> WidgetNode {

error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking tinytemplate v1.2.1
    Checking ciborium v0.2.2
    Checking criterion v0.8.2
    Checking feature-canvas-dataviz v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/features/87-canvas-dataviz)
    Checking feature-graph-game-collab-lottie v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/features/88-graph-game-collab-lottie)

    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)

warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: unresolved link to `wright_app`
 --> examples/wright_film/src/m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
  |                                                         ^^^^^^^^^^ no item named `wright_app` in scope
  |
  = help: to escape `[` and `]` characters, add '\' before them like `\[` or `\]`

error: could not document `wright_film`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/wrightsite)
 Documenting viewwsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/viewwsite)
 Documenting vieww-plugin-macros v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-plugin-macros)
 Documenting vieww-widget-macros v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-widget-macros)
 Documenting vieww-build v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-build)

error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates/vieww-render-planner/src/policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this item is private
   |
   = note: this link will resolve properly if you pass `--document-private-items`

error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates/vieww-render-planner/src/probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
    |                                        ^^^^^^^^^^ this item is private
    |
    = note: this link will resolve properly if you pass `--document-private-items`

error: public documentation for `plan_for_frame` links to private item `Self::plan_measured`
    --> crates/vieww-render-planner/src/shadow.rs:1046:35
     |
1046 |     /// measured-augmented one ([`Self::plan_measured`]): real GPU timings by
     |                                   ^^^^^^^^^^^^^^^^^^^ this item is private
     |
     = note: this link will resolve properly if you pass `--document-private-items`

error: could not document `vieww-render-planner`
 Documenting vieww-hardware v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hardware)
 Documenting vieww-hal v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hal)
 Documenting vieww-render-graph v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-render-graph)
 Documenting audit-sheets v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/audit-sheets)
error: unresolved link to `Step`
 --> crates/vieww-hal/src/vulkan/scene.rs:9:59

… more errors in the full log
```

</details>

<details><summary>end of logs/G1.1.txt</summary>

```text
… 2834 earlier lines omitted

--- tests (workspace): its diagnostics ---
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/baseline
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/baseline.dSYM
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/bench
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/bench.dSYM
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/render
warning: output filename collision at /Users/runner/work/vieww_prod/vieww_prod/vieww_base/target/debug/examples/render.dSYM
warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
warning: `wright-tools` (lib) generated 1 warning
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
test rt::tests::promotion_reports_what_the_os_did ... FAILED
---- rt::tests::promotion_reports_what_the_os_did stdout ----
thread 'rt::tests::promotion_reports_what_the_os_did' (92291) panicked at crates/vieww-audio/src/rt.rs:523:9:
error: test failed, to rerun pass `-p vieww-audio --lib`

--- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow): its diagnostics ---

4 stage(s) failed:
  - clippy (workspace, all targets, warnings are errors) (exit 101)
  - docs (workspace, broken intra-doc links are errors) (exit 101)
  - tests (workspace) (exit 101)
  - raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
exit=1
```

</details>

Failed stages in `checks`:

- clippy (workspace, all targets, warnings are errors) (exit 101)
- docs (workspace, broken intra-doc links are errors) (exit 101)
- tests (workspace) (exit 101)
- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)

<details><summary>errors in stage: clippy (workspace, all targets, warnings are errors)</summary>

```text
error: items after a test module
   --> examples/wright_film/src/socials.rs:499:1
    |
499 | mod tests {
error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking tinytemplate v1.2.1
    Checking ciborium v0.2.2
warning: build failed, waiting for other jobs to finish...
    Checking tinytemplate v1.2.1
    Checking ciborium v0.2.2
    Checking criterion v0.8.2
    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)
```

</details>

<details><summary>errors in stage: docs (workspace, broken intra-doc links are errors)</summary>

```text
error: unresolved link to `wright_app`
 --> examples/wright_film/src/m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
error: could not document `wright_film`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/wrightsite)
 Documenting viewwsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/viewwsite)
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/wrightsite)
 Documenting viewwsite v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/apps/viewwsite)
 Documenting vieww-plugin-macros v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-plugin-macros)
error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates/vieww-render-planner/src/policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates/vieww-render-planner/src/probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
error: public documentation for `plan_for_frame` links to private item `Self::plan_measured`
    --> crates/vieww-render-planner/src/shadow.rs:1046:35
     |
1046 |     /// measured-augmented one ([`Self::plan_measured`]): real GPU timings by
error: could not document `vieww-render-planner`
 Documenting vieww-hardware v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hardware)
 Documenting vieww-hal v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-hal)
 Documenting vieww-render-graph v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-render-graph)
error: unresolved link to `Step`
 --> crates/vieww-hal/src/vulkan/scene.rs:9:59
  |
9 | //! records **one** command buffer for the plan's whole [`Step`] program, and
error: unresolved link to `vieww_3d::scene::Texture::texels`
    --> crates/vieww-hal/src/vulkan/mod.rs:2656:19
     |
2656 |     /// texels ([`vieww_3d::scene::Texture::texels`]), uploaded as f16.
error: unresolved link to `ScenePlan`
  |
  = note: the link appears in this line:
error: unresolved link to `ScenePlan`
  |
  = note: the link appears in this line:
error: unresolved link to `frame_passes`
  |
  = note: the link appears in this line:
error: unresolved link to `SceneOp`
  |
  = note: the link appears in this line:
error: unresolved link to `frame_passes`
  |
  = note: the link appears in this line:
error: unresolved link to `PostPush`
  |
  = note: the link appears in this line:
error: unresolved link to `mode`
  |
  = note: the link appears in this line:
error: unresolved link to `blend_index`
  |
  = note: the link appears in this line:
error: unresolved link to `PassKind`
  |
  = note: the link appears in this line:
error: unresolved link to `PassKind::Blit`
  |
  = note: the link appears in this line:
error: could not document `vieww-hal`
 Documenting vector-probe v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/vector-probe)
 Documenting full-showcase v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/full-showcase)
 Documenting keynote v0.1.0 (/Users/runner/work/vieww_prod/vieww_prod/vieww_base/examples/keynote)
    FAILED: docs (workspace, broken intra-doc links are errors) (exit 101)
```

</details>

<details><summary>errors in stage: tests (workspace)</summary>

```text
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/main.rs (target/debug/deps/animation_showcase-7b70550038a68567)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

</details>

<details><summary>errors in stage: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow)</summary>

```text
    FAILED: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
```

</details>

#### ❌ G1.3 — certification (cpu)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.3.txt</summary>

```text
FAILED: tests/workspace (see target/release-gate/macos-cpu/cert/tests/workspace.txt)

FAILED=1
```

</details>

<details><summary>end of logs/G1.3.txt</summary>

```text
… 51 earlier lines omitted
gpu/metal-build              PASS
gpu/metal-backend            BUILD-ONLY(metal does not execute a ScenePlan yet; not certified)
gpu/device-class             SKIPPED(CPU-only run requested)
gpu/adapter                  SKIPPED(CPU-only run requested)
gpu/vulkan-suite             SKIPPED(CPU-only run requested)
gpu/workload                 SKIPPED(CPU-only run requested)
gpu/census                   SKIPPED(CPU-only run requested)
visual/premium               PASS
visual/fixtures              PASS
suites/layout-stress         PASS
suites/animation-stress      PASS
suites/text-fidelity         PASS
suites/image-effects         PASS
suites/scroll-stress         PASS
suites/native-surface        PASS
standard/vieww-standard      PASS
web/wasm-check               PASS
web/build                    SKIPPED(wasm-bindgen not on PATH)
web/baseline                 PASS
web/verify                   SKIPPED(no freshly built wasm to verify)
desktop/suite                SKIPPED(VIEWW_CERT_NO_DESKTOP=1)
gate/premium-overflows-zero  PASS
gate/standard-passed         PASS
gate/standard-allocations-measured PASS
exit=1
```

</details>

- certification stage **tests/workspace**: `FAIL(exit=101)`

<details><summary>errors in cert/tests/workspace.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps/wright/tools/src/paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

test shared::tests::animation_advances_while_the_calling_thread_is_blocked ... FAILED

failures:

---- shared::tests::animation_advances_while_the_calling_thread_is_blocked stdout ----

thread 'shared::tests::animation_advances_while_the_calling_thread_is_blocked' (150104) panicked at crates/vieww-animation/src/shared.rs:317:9:
moved while blocked: 100


failures:

test result: FAILED. 324 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.96s

error: test failed, to rerun pass `-p vieww-animation --lib`
     Running unittests src/lib.rs (target/debug/deps/vieww_asset-d74ed72ed0aaa70e)

running 17 tests
test bundle::tests::a_missing_asset_is_not_found_rather_than_unreadable ... ok
test bundle::tests::a_path_that_climbs_out_of_the_bundle_is_refused ... ok
test bundle::tests::a_bundle_error_becomes_a_service_failure_rather_than_unsupported ... ok

test rt::tests::promotion_reports_what_the_os_did ... FAILED

failures:

---- rt::tests::promotion_reports_what_the_os_did stdout ----

thread 'rt::tests::promotion_reports_what_the_os_did' (152043) panicked at crates/vieww-audio/src/rt.rs:523:9:
assertion `left == right` failed
  left: Elevated
 right: Normal


failures:

test result: FAILED. 502 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 29.88s

error: test failed, to rerun pass `-p vieww-audio --lib`
     Running tests/audit_probes.rs (target/debug/deps/audit_probes-a2dcd1066be22050)

running 7 tests
test probe_cpu_headroom ... ignored, benchmark: timing-dependent, run with --ignored --nocapture
test probe_mix_is_order_independent ... ok
test probe_nan_is_not_propagated_by_mixer ... ok

error: could not find `Cargo.toml` in `/private/var/folders/n6/hn37q6pj7sj_9dmqqc2113800000gn/T/vieww-cli-cmd-cargo-test` or any parent directory
test tests::cmd_cargo_runs_in_the_given_directory_and_propagates_failure ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

     Running tests/cli.rs (target/debug/deps/cli-688d7e8fc8ca6a52)


error: 2 targets failed:
    `-p vieww-animation --lib`
    `-p vieww-audio --lib`
exit=101
seconds=435
```

</details>

<details><summary>end of cert/tests/workspace.txt</summary>

```text
… 8343 earlier lines omitted

   Doc-tests viewwsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwstudio

running 1 test
test apps/viewwstudio/src/lib.rs - (line 35) - compile ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s

   Doc-tests wrightsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 2 targets failed:
    `-p vieww-animation --lib`
    `-p vieww-audio --lib`
exit=101
seconds=435
```

</details>

#### ❌ G2.7 — scaffolded Rust + Say projects compile for every target

`FAIL(compile/ios-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 |     [1m[94m|[0m | [1m;compile/ios-sim-rust FAIL exit exit status: 101; last lines:    [1m[94m--> [0m/Users/runner/work/vieww_prod/vieww_prod/vieww_base/crates/vieww-platform-winit/src/startup.rs:201:19 |     [1m[94m|[0m | )`

#### ❌ G2.11 — Studio export: iOS simulator .app

`FAIL(export/ios-sim FAIL step 'Build (iOS simulator)' (cargo build --release --target aarch64-apple-ios-sim): exit exit status: 101; last lines:     [1m[94m|[0m | [1m[94m201[0m [1m[94m|[0m             l)`

### `gate-macos-gpu`

#### ❌ G1.3 — certification (gpu)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.3.txt</summary>

```text
FAILED: gpu/vulkan-suite (see target/release-gate/macos-gpu/cert/gpu/vulkan-suite.txt)

FAILED=1
```

</details>

<details><summary>end of logs/G1.3.txt</summary>

```text
… 7 earlier lines omitted
>>> cargo run --release -p test-gpu-work -- target/release-gate/macos-gpu/cert/gpu/workload
>>> cargo run --release -p fixtures -- target/release-gate/macos-gpu/cert/gpu/census-out --census --receipt target/release-gate/macos-gpu/cert/gpu/census-out/receipt.json --shadow target/release-gate/macos-gpu/cert/gpu/census-out/shadow.jsonl
    skipped: desktop/suite — VIEWW_CERT_NO_DESKTOP=1
Vieww macos certification
===========================
COMPLETE=false (skipped stages: 1)
FAILED=1
git_rev=e37c4ca2069192a1edaab617cc5ecf02635cc41c
git_dirty=0
source_digest=git-tree:ac106d6efcb0517e3d4666cfe598e1dcd75ae982

gpu/hal-build                PASS
gpu/metal-build              PASS
gpu/metal-backend            BUILD-ONLY(metal does not execute a ScenePlan yet; not certified)
gpu/adapter                  PASS
gpu/device-class             PASS(virtual GPU: Apple Paravirtual device; pixels are evidence, timings are not)
gpu/vulkan-suite             FAIL(exit=101)
gpu/vulkan-validation        INFO(validation layer not installed: apt install vulkan-validationlayers)
gpu/workload                 PASS
gpu/census                   PASS
desktop/suite                SKIPPED(VIEWW_CERT_NO_DESKTOP=1)
gate/gpu-unsupported-zero    PASS
gate/census-all-complete     PASS
gate/census-no-flat-region-mismatch PASS
exit=1
```

</details>

- certification stage **gpu/vulkan-suite**: `FAIL(exit=101)`

<details><summary>errors in cert/gpu/vulkan-suite.txt</summary>

```text
test gpu_time_is_split_by_the_kind_of_work_that_spent_it ... FAILED

failures:

---- gpu_time_is_split_by_the_kind_of_work_that_spent_it stdout ----

thread 'gpu_time_is_split_by_the_kind_of_work_that_spent_it' (43531) panicked at crates/vieww-hal/tests/vulkan_damage.rs:436:5:
GpuPassTimes { draw: 0ns, layers: 0ns, filters: 0ns, shadows: 0ns, blit: 0ns }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace



failures:

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.24s

error: test failed, to rerun pass `-p vieww-hal --test vulkan_damage`
exit=101
seconds=72
```

</details>

<details><summary>end of cert/gpu/vulkan-suite.txt</summary>

```text
… 64 earlier lines omitted
test a_backdrop_filter_inside_the_damage_keeps_it ... ok
test a_backdrop_filter_reaching_outside_the_damage_redraws_everything ... ok
test a_damaged_frame_is_the_full_frame_through_blurs_shadows_and_blends ... ok
test a_new_clear_colour_or_size_redraws_everything ... ok
test gpu_time_is_split_by_the_kind_of_work_that_spent_it ... FAILED
test overlapping_damage_rectangles_draw_each_pixel_once ... ok
test pixels_outside_the_damage_are_last_frames ... ok

failures:

---- gpu_time_is_split_by_the_kind_of_work_that_spent_it stdout ----

thread 'gpu_time_is_split_by_the_kind_of_work_that_spent_it' (43531) panicked at crates/vieww-hal/tests/vulkan_damage.rs:436:5:
GpuPassTimes { draw: 0ns, layers: 0ns, filters: 0ns, shadows: 0ns, blit: 0ns }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    gpu_time_is_split_by_the_kind_of_work_that_spent_it

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.24s

error: test failed, to rerun pass `-p vieww-hal --test vulkan_damage`
exit=101
seconds=72
```

</details>

#### ❌ G1.5 — Vulkan compositor pixel parity (17 tests)

`FAIL(exit=101)`

### `gate-windows-cpu`

#### ❌ G1.1 — ci/vieww checks (fmt, clippy, check, docs, MSRV, tests, fixtures, packaging)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.1.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: items after a test module
   --> examples\wright_film\src\socials.rs:499:1
    |
499 | mod tests {
    | ^^^^^^^^^
...
521 | pub(crate) fn p1_question(ctx: &Ctx) -> WidgetNode {

error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking feature-checkbox v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\26-checkbox)
    Checking feature-timelines v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\82-timelines)
    Checking feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
    Checking feature-rigging v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\83-rigging)
    Checking feature-split-text v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\77-split-text)

    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)

warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

error: unresolved link to `crate::ios`
 --> crates\vieww-interaction\src\drag_out_windows.rs:6:28
  |
6 | //! The same standing as [`crate::ios`] on the other end of the platform
  |                            ^^^^^^^^^^ no item named `ios` in module `vieww_interaction`
  |
  = note: `-D rustdoc::broken-intra-doc-links` implied by `-D warnings`

error: could not document `vieww-interaction`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\apps\wrightsite)
    Checking softbuffer v0.4.8
    Checking vieww-platform-winit v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-platform-winit)
    Checking anstyle-query v1.1.5
    Checking anstyle-wincon v3.0.11

error: unresolved link to `wright_app`
 --> examples\wright_film\src\m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
  |                                                         ^^^^^^^^^^ no item named `wright_app` in scope
  |
  = help: to escape `[` and `]` characters, add '\' before them like `\[` or `\]`

error: could not document `wright_film`
 Documenting feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
 Documenting feature-easing-presets v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\63-easing-presets)
 Documenting feature-switch v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\27-switch)
 Documenting feature-keyframes v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\56-keyframes)
 Documenting feature-locale-formatting v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\51-locale-formatting)
 Documenting feature-grayscale v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\33-grayscale)

error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates\vieww-render-planner\src\policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
   |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this item is private
   |
   = note: this link will resolve properly if you pass `--document-private-items`

error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates\vieww-render-planner\src\probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
    |                                        ^^^^^^^^^^ this item is private
    |
    = note: this link will resolve properly if you pass `--document-private-items`

… more errors in the full log
```

</details>

<details><summary>end of logs/G1.1.txt</summary>

```text
… 2892 earlier lines omitted
error: unresolved link to `PassKind`
error: unresolved link to `PassKind::Blit`
error: could not document `vieww-hal`

--- tests (workspace): its diagnostics ---
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\baseline.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\baseline.pdb
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\bench.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\bench.pdb
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\render.exe
warning: output filename collision at D:\a\vieww_prod\vieww_prod\vieww_base\target\debug\examples\render.pdb
warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
warning: `wright-tools` (lib) generated 1 warning
thread 'main' (4240) panicked at crates\vieww-audio\tests\realtime_no_alloc.rs:182:5:
error: test failed, to rerun pass `-p vieww-audio --test realtime_no_alloc`

--- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow): its diagnostics ---

4 stage(s) failed:
  - clippy (workspace, all targets, warnings are errors) (exit 101)
  - docs (workspace, broken intra-doc links are errors) (exit 101)
  - tests (workspace) (exit 101)
  - raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
exit=1
```

</details>

Failed stages in `checks`:

- clippy (workspace, all targets, warnings are errors) (exit 101)
- docs (workspace, broken intra-doc links are errors) (exit 101)
- tests (workspace) (exit 101)
- raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)

<details><summary>errors in stage: clippy (workspace, all targets, warnings are errors)</summary>

```text
error: items after a test module
   --> examples\wright_film\src\socials.rs:499:1
    |
499 | mod tests {
error: could not compile `wright_film` (bin "wright_film" test) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
    Checking feature-checkbox v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\26-checkbox)
    Checking feature-timelines v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\82-timelines)
warning: build failed, waiting for other jobs to finish...
    Checking feature-checkbox v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\26-checkbox)
    Checking feature-timelines v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\82-timelines)
    Checking feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
    FAILED: clippy (workspace, all targets, warnings are errors) (exit 101)
```

</details>

<details><summary>errors in stage: docs (workspace, broken intra-doc links are errors)</summary>

```text
error: unresolved link to `crate::ios`
 --> crates\vieww-interaction\src\drag_out_windows.rs:6:28
  |
6 | //! The same standing as [`crate::ios`] on the other end of the platform
error: could not document `vieww-interaction`
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\apps\wrightsite)
    Checking softbuffer v0.4.8
warning: build failed, waiting for other jobs to finish...
 Documenting wrightsite v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\apps\wrightsite)
    Checking softbuffer v0.4.8
    Checking vieww-platform-winit v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-platform-winit)
error: unresolved link to `wright_app`
 --> examples\wright_film\src\m2_run.rs:3:57
  |
3 | //! Three scenes, all one mounting of the real shell ([`wright_app`]):
error: could not document `wright_film`
 Documenting feature-motion-tokens v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\46-motion-tokens)
 Documenting feature-easing-presets v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\63-easing-presets)
 Documenting feature-switch v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\features\27-switch)
error: public documentation for `hybrid_min_pixels` links to private item `HYBRID_SPLIT_OVERHEAD_MULTIPLES`
  --> crates\vieww-render-planner\src\policy.rs:55:7
   |
55 | /// [`HYBRID_SPLIT_OVERHEAD_MULTIPLES`] × overhead-pixels, clamped to
error: public documentation for `calibrate_cpu_fill` links to private item `BENCH_RUNS`
   --> crates\vieww-render-planner\src\probe.rs:104:40
    |
104 |     /// plain-fill scene, timed over [`BENCH_RUNS`] runs and reported as the
error: public documentation for `plan_for_frame` links to private item `Self::plan_measured`
    --> crates\vieww-render-planner\src\shadow.rs:1046:35
     |
1046 |     /// measured-augmented one ([`Self::plan_measured`]): real GPU timings by
error: could not document `vieww-render-planner`
 Documenting vieww-render-graph v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-render-graph)
 Documenting vieww-hal v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-hal)
 Documenting vector-probe v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\vector-probe)
error: unresolved link to `Step`
 --> crates\vieww-hal\src\vulkan\scene.rs:9:59
  |
9 | //! records **one** command buffer for the plan's whole [`Step`] program, and
error: unresolved link to `vieww_3d::scene::Texture::texels`
    --> crates\vieww-hal\src\vulkan\mod.rs:2656:19
     |
2656 |     /// texels ([`vieww_3d::scene::Texture::texels`]), uploaded as f16.
error: unresolved link to `ScenePlan`
  |
  = note: the link appears in this line:
error: unresolved link to `ScenePlan`
  |
  = note: the link appears in this line:
error: unresolved link to `frame_passes`
  |
  = note: the link appears in this line:
error: unresolved link to `SceneOp`
  |
  = note: the link appears in this line:
error: unresolved link to `frame_passes`
  |
  = note: the link appears in this line:
error: unresolved link to `PostPush`
  |
  = note: the link appears in this line:
error: unresolved link to `mode`
  |
  = note: the link appears in this line:
error: unresolved link to `blend_index`
  |
  = note: the link appears in this line:
error: unresolved link to `PassKind`
  |
  = note: the link appears in this line:
error: unresolved link to `PassKind::Blit`
  |
  = note: the link appears in this line:
error: could not document `vieww-hal`
 Documenting vieww-shaders v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\crates\vieww-shaders)
 Documenting keynote v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\keynote)
 Documenting screenshot v0.1.0 (D:\a\vieww_prod\vieww_prod\vieww_base\examples\screenshot)
    FAILED: docs (workspace, broken intra-doc links are errors) (exit 101)
```

</details>

<details><summary>errors in stage: tests (workspace)</summary>

```text
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

</details>

<details><summary>errors in stage: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow)</summary>

```text
ci/check/raster-identity.sh: line 51: cd: /tmp/tmp.FL6sf3gxSS/base//d/a/vieww_prod/vieww_prod/vieww_base: No such file or directory
raster-identity: the gallery failed to render in /tmp/tmp.FL6sf3gxSS/base//d/a/vieww_prod/vieww_prod/vieww_base (see /tmp/tmp.FL6sf3gxSS/out-base.log)
tail: cannot open '/tmp/tmp.FL6sf3gxSS/out-base.log' for reading: No such file or directory
    FAILED: raster identity (every fixture byte-identical to the merge base with origin/main; builds BASE in its own worktree — slow) (exit 1)
```

</details>

#### ❌ G1.3 — certification (cpu)

`FAIL(exit=1)`

<details><summary>errors in logs/G1.3.txt</summary>

```text
FAILED: tests\workspace

FAILED=True
```

</details>

<details><summary>end of logs/G1.3.txt</summary>

```text
… 20 earlier lines omitted
gpu\hal-build-vulkan         PASS
gpu\device-class             SKIPPED(CPU-only run requested)
gpu\adapter                  SKIPPED(CPU-only run requested)
gpu\vulkan-suite             SKIPPED(CPU-only run requested)
gpu\workload                 SKIPPED(CPU-only run requested)
gpu\census                   SKIPPED(CPU-only run requested)
gpu\d3d12-build              PASS
gpu\d3d12-backend            BUILD-ONLY(d3d12 does not execute a ScenePlan yet; not certified)
visual\premium               PASS
visual\fixtures              PASS
suites\layout-stress         PASS
suites\animation-stress      PASS
suites\text-fidelity         PASS
suites\image-effects         PASS
suites\scroll-stress         PASS
suites\native-surface        PASS
standard\vieww-standard      PASS
web\baseline                 PASS
web\build                    SKIPPED(wasm32 target, wasm-bindgen or bash unavailable)
web\verify                   SKIPPED(no freshly built wasm to verify)
desktop\suite                SKIPPED(VIEWW_CERT_NO_DESKTOP=1)
gate\premium-overflows-zero  PASS
gate\standard-passed         PASS
gate\standard-allocations-measured PASS
exit=1
```

</details>

- certification stage **tests/workspace**: `FAIL(exit=101)`

<details><summary>errors in cert/tests/workspace.txt</summary>

```text
warning: function `refused_link` is never used
   --> apps\wright\tools\src\paths.rs:227:4
    |
227 | fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    |    ^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

thread 'main' (1760) panicked at crates\vieww-audio\tests\realtime_no_alloc.rs:182:5:
assertion `left == right` failed: parallel rendering allocated
  left: 9
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
error: test failed, to rerun pass `-p vieww-audio --test realtime_no_alloc`
     Running tests\tags.rs (target\debug\deps\tags-d4da70959570a1b1.exe)

error: could not find `Cargo.toml` in `C:\Users\RUNNER~1\AppData\Local\Temp\vieww-cli-cmd-cargo-test` or any parent directory
test tests::cmd_cargo_runs_in_the_given_directory_and_propagates_failure ... ok
test tests::a_nonzero_exit_status_is_propagated ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s

     Running tests\cli.rs (target\debug\deps\cli-26c50449557a57bd.exe)

thread '<unnamed>' (4656) panicked at C:\Users\RUNNER~1\AppData\Local\Temp\vieww-studio-3333\preview-0001.rs:5:5:
this is what user code does at three in the morning
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test a_panicking_screen_is_caught_at_the_boundary_rather_than_taking_the_host_down ... ok

thread '<unnamed>' (7956) panicked at C:\Users\RUNNER~1\AppData\Local\Temp\vieww-studio-3334\preview-0001.rs:11:9:
this is what user code does at three in the morning

error: 1 target failed:
    `-p vieww-audio --test realtime_no_alloc`
exit=101
seconds=379
```

</details>

<details><summary>end of cert/tests/workspace.txt</summary>

```text
… 8303 earlier lines omitted
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests viewwstudio

running 1 test
test apps\viewwstudio\src\lib.rs - (line 35) - compile ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

   Doc-tests wrightsite

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 1 target failed:
    `-p vieww-audio --test realtime_no_alloc`
exit=101
seconds=379
```

</details>
