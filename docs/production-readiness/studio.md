# viewwstudio — Production-Readiness Deep Research (Task 2-b)

## 0. Method & verification notes

Everything below was verified by reading the code on `main` at `vieww_base/apps/viewwstudio/` (74,403 lines across `src/`, `src/state/`, `src/ui/`, `tests/`, `examples/`, `build.rs`, `ci/`, `docs/`, `Cargo.toml`), plus the framework crates the studio depends on (`vieww-say-codegen`, `vieww-asset`, `vieww-platform-winit`, `vieww-paint`) and the crates it *could* depend on but does not (`vieww-devtools`, `vieww-reload`, `vieww-collab`, `vieww-cli` — verified by `rg -ln "vieww_collab|vieww_devtools|vieww_reload" vieww_base/apps/viewwstudio/` returning nothing). No cargo build/test was run. CI facts come from `.github/workflows/*.yml`, `.github/workflows-pending/`, `vieww_base/ci/` scripts, `vieww_base/TRACKER.md` and `vieww_base/PENDING.md`. Leader mechanisms were checked against live documentation URLs (§ 8); all URLs cited returned HTTP 200 at research time unless noted. Line numbers reference `main` (commit `98367be`).

One definitional finding that shapes the whole report: **the studio is a code editor with a real-runtime preview, not a canvas manipulator.** There is no freeform canvas, no drag-drop of widgets, no property editing, no layers tree. Its two representations are *code* (Rust or Say) and *the rendered truth*. Every gap below is measured against what a FlutterFlow/Rive/Figma/Xcode user gets.

## 1. Executive verdict

| # | Dimension | /10 | Justification (code-verified) |
|---|-----------|-----|-------------------------------|
| 1 | Editing depth (visual) | **3** | Text editing is excellent (multi-cursor, folding, 200-deep coalescing undo — `src/history.rs:44`), but the *visual* edit surface is pick-only (`src/state/editing.rs:1456-1482` selects an inspector row; nothing writes back). No drag, no reorder, no resize, no multi-select, no layers. |
| 2 | Round-trip code↔canvas | **3** | code→preview is exact (real `vieww` runtime mounted as a subtree, `src/ui/preview.rs:2308-2324`). canvas→code does not exist. Zero lock-in: projects are plain cargo crates (`src/scaffold.rs`); Say emits reviewable Rust with `// say:` markers (`vieww-say-codegen/tests/fixtures/counter_gen.rs`). |
| 3 | Preview fidelity & speed | **8** | Real `rustc` → `cdylib` → `dlopen` → same-process widget subtree under simulated device metrics (`src/compile.rs`, `src/loaded.rs`); ~0.8 s warm compile (measured in `src/compile.rs:44-54` docs), 600 ms auto-render debounce (`src/state/editing.rs:1432-1435`); out-of-process isolation with pixel streaming (`src/remote.rs`). Docked for: remote mode forwards no keys/IME/scroll/hover; cold compile 10.6 s; one device at a time. |
| 4 | "Say"/AI codegen | **4** | Say is a deterministic 18-phrase English DSL → byte-identical Rust (`vieww-say-codegen/src/lib.rs:92-163`), fixture-pinned and state-surviving-recompile. But there is **no LLM anywhere** (`rg -il "llm|openai|anthropic"` in studio sources: none) — no free-form prompt path at all in the AI-app-builder wave. |
| 5 | Inspector/debugging | **5** | Real render-tree inspector with pick-on-canvas, damage overlay, semantics overlay (`src/ui/inspector.rs`, `src/state/editing.rs:1453-1529`). But: no state/signal inspection, flat list only, and the killer features (frame attribution, frame cost) live only as headless `examples/`, not in the tool. |
| 6 | Export targets & store pipelines | **5** | 5 formats: desktop binary, cross-compiled Windows `.exe`, debug-signed Android APK, iOS simulator `.app`, signed iOS `.ipa` (`src/export.rs:48-72`); adb scan + install (`src/export.rs:632-691`). Missing: **Web** (framework has `vieww-platform-web-dom`), macOS/Linux user-app packaging formats, release-APK signing flow in-UI, any store upload. |
| 7 | Asset pipeline | **3** | `WorkspaceBundle` resolves project `assets/` into the preview (`src/assets.rs`); icon/iconsheet generators exist (`examples/icon.rs`, `examples/iconsheet.rs`). No asset browser UI, no font management, no import/rename pipeline. |
| 8 | Templates/onboarding/docs | **7** | New Project scaffolds Rust **and** Say templates whose every file is compiled by a test (`tests/scaffold.rs`); 6 in-app docs (`docs/01..06`); curated Learn lessons bridging ~50 feature examples (`src/lessons.rs`); welcome overlay; first Render auto-fires (`src/main.rs:334-355`). |
| 9 | Crash recovery & session persistence | **8** | Per-dirty-buffer recovery files on a clock (`src/recovery.rs`, `poll_recovery` in `src/main.rs:176-182`); quit-veto dialog (`src/main.rs:80-90`); session/geometry restore; on-disk-change conflict dialog; out-of-process preview contains `abort()` (`src/remote.rs:11-24`). |
| 10 | Performance | **7** | Instrumented to an unusual degree: `bench` (per-phase), `coldstart`, `frame_cost` (~49 ms full frame, 605 commands/323 shapes/1,447 glyphs per TRACKER), `raster_real` (damage ratio), `damage_probe`, `idle_cost`. Known open wound: Windows first frame 840 ms vs 221 ms Linux (PENDING §3.5). |
| 11 | Collaboration | **2** | `vieww-collab` (LwwMap/OrSet/Counter/RGA-text/Presence/UndoManager — `crates/vieww-collab/src/lib.rs`, `undo.rs`) is complete **and used by nothing in the studio**. Only git status+commit UI (`src/git.rs`). |
| 12 | Extensibility | **3** | User themes, keymaps, snippets (`src/customise.rs`). No studio plugin API (the framework's C-ABI plugin system is not surfaced). |
| 13 | Distribution & install | **6** | `packaging/package.sh` builds self-contained bundles (.deb/.AppImage/.tar.gz/.msi/.dmg/.zip) with a bundled rustc + rlib SDK verified by manifest (`src/sdk.rs`, `src/install.rs`), SHA256SUMS + MANIFEST, a download page. Missing: auto-update, code signing/notarization ("UNSIGNED" recorded in `ci/release/artifacts.sh:59-61`), and the bundle is ~600 MB with the toolchain. |
| 14 | Telemetry/diagnostics | **2** | None (privacy-positive, product-blind). No crash reporting, no opt-in usage signal, no diagnostics export from the About box. |
| 15 | Accessibility of studio itself | **5** | `large_ui`, `high_contrast`, blink-off settings (`src/state.rs:1713-1724`); semantics overlay for the *preview*; but no screen-reader audit of the studio chrome, no keyboard-only walkthrough test. |

**Overall:** viewwstudio is the best *live-rustc* UI preview loop in existence — it compiles the buffer with the real compiler against the real runtime, in-process, at typing speed, with crash isolation, state carry-over and deterministic screenshot verification. As a *visual app-builder* in the FlutterFlow/Rive/Figma sense it is at the very beginning: the entire direct-manipulation layer (canvas editing, properties, layers, components, assets, multiplayer) is absent. The path to "top of the ladder" is not to copy Figma; it is to make the code↔preview loop *bidirectional* and *provably faithful*, which no competitor can do because none of them own a deterministic pixel-parity oracle. The studio owns one.

**The 5 things that matter most:**
1. **Make "say" an AI loop with verification** (ST-03) — the wave is prompt→app; vieww has the unique ability to *prove* what the prompt produced (compile + render + pixel-stability) before showing it.
2. **Close the remote-preview input gap on Windows** (ST-04) — the platform the out-of-process preview was built for ships a degraded preview today.
3. **Bidirectional canvas: pick → edit property → code diff** (ST-01/ST-02) — even a minimal property panel for the Say subset changes the product class.
4. **Export Web + real-device verification** (ST-07/ST-08) — the framework runs on web and the device farm exists; neither is wired to the studio.
5. **Gate the studio's own 608-test suite + examples in CI** (ST-12) — today one test file on one OS is all that runs on GitHub.

## 2. What exists today (verified inventory)

**The shell.** `src/main.rs` opens one window (1440×900 default), restores geometry, wraps everything in a close-veto guard (`main.rs:80-90`), and installs a `before_frame` hook that polls, per frame: build errors, preview-state capture/restore around recompiles, the compile job, the file watcher, `cargo build` output, other jobs, the inspector tree, damage/semantics overlays, rust-analyzer, the auto-render debounce, settings, recovery files, dropped files, and window close (`main.rs:122-211`). `Studio` is a deliberate god-*object* (all state in signals above the tree, no widget owns state; `src/state.rs:37-60` documents the split into `state/*.rs` files). 12 sidebar views: Explorer, Search, Snippets, Problems, Inspector, Learn, Docs, Toolchain, Source Control, Export, Theme Tokens, Settings (`src/state.rs:82-101`).

**The editor.** Multi-buffer tabs; tree-sitter highlighting for Rust/TOML/JSON/YAML/Markdown (`Cargo.toml:49-65` — the `Language` enum claims 25 languages, 5 are highlighted, honestly documented); code folding; multi-cursor (`AddCursorBelow` etc.); find/replace with a *plan preview* before cross-file replace (`src/find.rs`, `src/state.rs:1693-1698`); snippets (built-in + user file); per-buffer undo/redo with structural coalescing at word boundaries and caret+multi-caret restore, depth 200 (`src/history.rs`); rustfmt on save; indent guides; inline diagnostics; completion + goto-definition + find-references via a real `rust-analyzer` child process speaking LSP over stdio (`src/lsp.rs:600-623`); file tree with watcher, rename, delete-to-trash, conflict dialog (`tests/explorer.rs` documents these were once "written, tested, and called by nothing" — now wired via `context_menu`); drag-and-drop file open (`main.rs:196-201`).

**The preview pipeline.** Render = one `rustc` invocation (not cargo) compiling the buffer + a hidden `ENTRY` appended (`src/compile.rs:~1060-1130`) that exports `vieww_preview_entry()` returning a `Box<Box<dyn Widget>>`, `vieww_preview_fingerprint()` returning a `TypeId`-hash ABI check, and `vieww_preview_probe()` that runs the first `build()` *inside the guest* so a panic is caught by the guest's own unwinder. The host `dlopen`s the cdylib, verifies the fingerprint, and mounts the widget as an ordinary subtree of the host tree inside `Inherited<ViewMetrics>` + adaptive `Theme` + `Overlay` (`src/ui/preview.rs:2308-2324`) — the real vieww element/render/paint/text stack, fully interactive (pointer events hit the guest directly; `ForwardInput` render object forwards input). Device simulation: 8 presets across iOS/Android/Desktop (`src/state.rs:367`), landscape with correctly rotated safe-area insets (`src/state.rs:338-362`), unsafe-region toggle. State survives recompiles: `capture_preview_state`/`restore_preview_state` snapshot the guest's `ElementState` around each remount (`main.rs:139-145`). Compile runs on a worker thread with cancel (`⇧⌘⏎`), 60 s backstop timeout (`src/compile.rs:69`, with the honest history of why 10 s was wrong), and the first Render auto-fires on launch (`main.rs:334-355`). Projects with their own rlib are supported via `ProjectLib` + textual `crate::`→`<name>::` rewrite that preserves line numbers (`src/compile.rs:126-227, 348-352`).

**The out-of-process preview.** `src/remote.rs` + `src/bin/viewwstudio-preview.rs`: the guest runs in a child process, mounted in the child's own `FrameDriver`, rasterized by the CPU `NativeRenderer`, frames streamed as RGBA over a hand-written length-prefixed pipe protocol; crashes return `RemoteError::Crashed { status, stderr }` with the guest's panic text. Default on Windows (where static libstd makes in-process panic catching impossible). **Not yet forwarded: keys, IME, scroll, hover** (module doc `src/remote.rs:44-60` + PENDING §3.3).

**Say.** A `.say` buffer is compiled to Rust *before* the entry-point check (`src/state.rs:2897-2937`), diagnostics land with real line/column in Problems, generated Rust is kept in `generated_rust` for viewing, and everything then flows through the identical compile pipeline. The language: `keep` state, screens as a route stack, 18 widget phrases, 20 properties, 11 actions, 10 icons (`vieww-say-codegen/src/lib.rs:92-163`); every generated statement carries `// say: file:line`; output is byte-deterministic with a committed fixture as the contract (`lib.rs:191-212`); reserved-but-unimplemented phrases answer `E050` with guidance. Generated state implements `ElementState` with `snapshot/restore` from the `keep` list — Say screens keep state across remounts.

**Live preview (flow).** `live.rs`: the user's `live.rs` at the workspace root describes screens/rows/buttons; the studio parses and renders it with its own widgets at typing speed, no rustc. Honest limits documented (no logic/data; the preview holds the counter state, not the app).

**Inspector & overlays.** `capture_tree` walks `RenderTree::describe_subtree` once per frame into a flat depth-tagged list (`src/ui/inspector.rs`); pick mode resolves a canvas click through `driver.hit_test_identify` to an inspector row and flips the right pane open (`src/state/editing.rs:1456-1482`); damage overlay reads `driver.damage().regions()`; semantics overlay reads the accessibility nodes with role+label (`editing.rs:1500-1529`). Guest panics and unregistered render objects are routed into Problems via sinks (`main.rs:239-285`); `ErrorPolicy::Custom` mounts a 48 px magenta placeholder rather than crashing (`src/lib.rs:148-172`).

**Build & export.** `builds.rs` runs real `cargo build` (Build / Build-and-Run) with JSON diagnostics parsed, streaming output, cancel; deliberately never escalates a preview into a build. `export.rs` is a *planner* producing a `Plan` of child processes + expected artefact, or a `Refusal` naming the missing tool and its install command (`Refusal::Missing { requirement, install }`); formats Desktop/Windows/Android/iOS-sim/iOS-ipa; `export_check` example actually runs the plans for both scaffold kinds. Device scan parses `adb devices -l` (`export.rs:649-689`) with `Unauthorised` called out; `install()` builds `adb install -r -s <id>`; the produced artefact is remembered so "install on device" works (`src/state.rs:2374-2381`). Signing: keystore/alias and Apple team are settings fields (`state.rs:1661-1670`), the keystore password deliberately only via `VIEWW_KEYSTORE_PASSWORD` at build time, never in a field that logs.

**Scaffold & assets.** `scaffold.rs` generates Rust or Say projects (screen/widget templates), path-vs-version dependency with the ABI consequence stated in the UI (`scaffold.rs:39-56`); `WorkspaceBundle` makes guest asset loads resolve the project's `assets/` first (`src/assets.rs`, installed at `main.rs:43`).

**Persistence & recovery.** Session (window size, workspace, open buffers) + settings via the framework `Storage` service; recovery files for dirty buffers written on a clock (`poll_recovery`, `RECOVERY_INTERVAL`), one hashed file per origin with a header naming where the text belongs; cleared on clean exit; restored dirty and announced on next launch (`main.rs:320-325`).

**Distribution.** `packaging/package.sh`: self-contained tree with bundled `rustc` + the exact `libvieww-<hash>.rlib` + `deps/` + `vieww-sdk.toml` manifest; host/guest become one compilation *by construction* (`src/sdk.rs` module doc — the third option after "stable ABI" and "relaxed guard"); `install::target_dir` search chain env → beside-executable → checkout; installers .deb/.AppImage/.dmg/.msi + archives; `ci/release/artifacts.sh` writes SHA256SUMS + MANIFEST.txt recording signing status as UNSIGNED; `packaging/site/index.html` is a download page with stable asset names.

**Tests & probes (the unusual part).** 26 integration test files (pipeline with real rustc; remote with real child pixels; persistence; sdk_bundle with a faked SDK tree; interaction/input/scroll_gestures/hover/hit_regions through the real driver; repaint incremental-vs-full in both themes; idle_cost; brand pixel test) + inline unit tests — 608 studio tests green at last full run (TRACKER.md:806,1284-1289, with the honest note that later disk-full prevented re-runs). `ci/standalone.sh` compiles 13 std-only modules with bare `rustc` — the whole logic layer tests with no network. Examples double as instrumentation: `bench` (build/layout/paint/raster phases), `coldstart`, `frame_cost`, `frame_attribution` (renders mutilated copies of the studio's *own scene* — clips stripped, glyphs removed, shadows removed — and attributes ms), `raster_real` (full-frame vs damaged-region cost ratio), `damage_probe`, `render_matrix` (every widget-catalog case through the real compile+dlopen path), `render_probe`, `screenshot` (headless PNG through the CPU rasterizer, no GPU needed), `motion` (frame-clock-driven deterministic frame sequences → GIF), `splash_probe`, `tour` (every shell state), `walkthrough` (one continuous 20-minute session, state leaks are the point), `release_check` (fires every palette command for real), `export_check`, `export_probe`, `scaffold_probe`, `icon`, `iconsheet`. `ci/certify/release-check.sh` runs tour+walkthrough+release_check into a browsable per-OS report — "run once on each of Linux, Windows and macOS before a beta goes out".

**What CI actually gates today.** `framework-checks.yml:269` — `cargo test -p viewwstudio --test remote -- --test-threads=1` on the GitHub Windows runner only; `ci/hardware/windows.ps1:317` — the same remote test on the self-hosted Windows machine. The other ~25 test files, all examples, and `certify/release-check.sh` are not wired into any workflow (manual scripts). The root `ci.yml` ("Build Mobile Apps") builds four sample apps (vavlt, snapsearch, upxcale, three) as debug-keystore APKs and ad-hoc IPAs — evidence both that app packaging works end-to-end on CI and that store pipelines are absent (debug keystore generated on the runner; device cloud re-signs).

## 3. Branch delta: main → feature/main_fixes

`git diff --stat main origin/feature/main_fixes -- vieww_base/apps/viewwstudio` is **empty**. The branch's 35 changed files (+8,915/−1,035) are all GPU-side: `vieww-hal` D3D12/Metal SceneRenderers, shared `scene_exec.rs`, `vieww-3d` lighting/textures/transparency, `vieww-render-planner` shadow planner + probe, `vieww-platform-winit` mixed-frame execution, `vieww-test-harness` parity/visual additions, plus PENDING/TRACKER updates and one wright worker tweak. **Every gap in § 4 is therefore [OPEN]** — none is fixed on `feature/main_fixes`. One second-order effect worth noting for the studio: the branch's `parity.rs` (270 lines in vieww-test-harness) and pixel-parity test contracts are exactly the machinery the studio's preview↔export proof (§ 6.1) should build on when merged.

## 4. GAP CATALOG

### ST-01 | P0 | No canvas manipulation: the preview is a window, not a workspace
**What it is.** The only canvas interaction is pick: `src/state/editing.rs:1456-1482` — `if let Some((x, y)) = self.pick_request.peek() { … driver.hit_test_identify(…) → self.inspect_selected.set(Some(index)) }`. Selection is a *read*. Nothing in `src/ui/preview.rs` or `src/state/` writes a structural edit back to a buffer from a pointer gesture (drag, reorder, resize, reparent do not exist as commands — see `src/command.rs`'s palette for what does).
**What it blocks.** A designer cannot move a card 8 px; they must know which `.padding(EdgeInsets::…)` line to change. Every FlutterFlow/Rive/Figma comparison ends here — the user opens Figma, gets their layout in 30 seconds, and never comes back. The "visual IDE" claim rests entirely on Say + preview.
**Leaders.** FlutterFlow: drag-drop widget tree with snapping + smart layout (https://docs.flutterflow.io/build-your-app/user-interface). Unity/Godot: full 2D scene manipulation, snapping, gizmos (https://docs.godotengine.org/en/stable/tutorials/2d/introduction_to_the_2d_editor.html). Android Studio Compose Layout Editor: drag UI elements into a design surface even for code-first Compose (https://developer.android.com/develop/ui/compose/tooling). Xcode deliberately has *no* freeform canvas (preview-only) and Apple still ships Interface Builder for AppKit — the two-representation tradeoff is a live industry argument.
**The fix.** Start with Say buffers only — the vocabulary is closed, so every phrase has a known span. (1) Extend pick to return the guest's `// say:` source line: the inspector already knows the render-object id; Say's generated code carries `// say: file:line` markers, so map render node → widget phrase → source span. (2) Add gesture affordances for the four manipulations that are pure property changes in Say grammar: `padded N`, `spaced N`, `N wide`, `N tall` — a drag handle on the picked widget edits the number and rewrites that line.
```rust
// src/state/canvas_edit.rs (new)
pub enum Nudge { Padding(f32), Spacing(f32), Width(f32), Height(f32) }
impl Studio {
    /// A drag on the picked widget's handle ended; rewrite its property line.
    pub fn apply_nudge(&self, nudge: Nudge, delta: f32) {
        let Some((line, col, len)) = self.say_span_of_picked() else { return };
        let old = self.active().map(|b| b.value.text.lines().nth(line));
        // rewrite `padded 16` → `padded 20` in the .say buffer; the
        // auto-render debounce (600 ms) turns it into pixels.
        self.edit_say_number(line, col, len, |v| v + delta);
    }
}
```
Target files: new `src/state/canvas_edit.rs`, `src/ui/preview.rs` (handle overlay on picked node bounds — the damage overlay already knows how to draw boxes over the pane), `src/command.rs` (new commands). The rust buffer path comes later via `vieww-say-codegen`'s registry only.
**Tests.** GitHub CI: unit tests in `tests/canvas_edit.rs` — a `.say` fixture, simulated drag sequence, assert the buffer's `padded` line and that `vieww_say_codegen::compile` still succeeds; pixel-diff `examples/`-style check that the preview PNG moved by exactly the delta. Self-hosted Linux: real-driver drag through `tests/interaction.rs` harness (X11). Self-hosted Windows: same via the remote child (drag already forwarded). Device farm: N/A (studio is desktop-only today).
**Status: [OPEN]**

### ST-02 | P0 | No property inspector: nothing about a widget is editable in the right pane
**What it is.** `src/ui/inspector.rs` renders `tree_snapshot` rows (name, children count, size); the picked row is highlighted (`inspect_selected`). There is no property grid, no `describe()`-driven fields, no write path. `RenderObject::describe` exists in the framework (`src/ui/inspector.rs:14-19` credits it) and is used for the label only.
**What it blocks.** "What font size is this Text?" requires reading Rust. "Make it primary colour" requires knowing `with the primary color` in Say or `theme.colors.primary` in Rust. Iteration speed for non-programmers collapses; the studio cannot be the design surface its keynote film implies (`studio_film/cut/16_the_inspector.png`).
**Leaders.** Unity Inspector: serialized properties with custom drawers (https://docs.unity3d.com/Manual/UsingTheInspector.html). Godot Inspector dock (https://docs.godotengine.org/en/stable/tutorials/editor/inspector_dock.html). Rive: full property panel with animatable fields (https://rive.app). FlutterFlow: property panel per selected widget.
**The fix.** Phase 1 (Say): property grid generated from `vieww_say_codegen::registry::PROPERTIES` — for the picked widget line, parse present properties into typed fields (number, colour swatch, bool, enum) and write back through the same line-rewrite machinery as ST-01. Phase 2 (Rust): read-only first (show `describe()` output, size, and the owning source file if the LSP can resolve it), then editable for the subset expressible as a literal (colour hex, padding numbers) using `rust-analyzer`'s range-rewrite.
```rust
// src/ui/inspector.rs — extend the picked-row detail view
fn say_property_fields(line: &say::Line) -> Vec<Field> {
    line.properties().map(|p| match p {
        say::Property::Padded(n) => Field::number("padding", n, |v| format!("padded {v}")),
        say::Property::Color(c) => Field::colour("color", c, |v| format!("color {v}")),
        …
    }).collect()
}
```
Target: `src/ui/inspector.rs`, `vieww-say-codegen/src/parse_line.rs` (expose a structured `Line` API — it already parses every property).
**Tests.** GitHub CI: `tests/inspector.rs` (new) — fixtures per property type, round-trip `.say` line → field → line is identity; write-back keeps `// say:` line numbers stable. Self-hosted Linux/Windows: interaction test picking a widget, editing a field, asserting the preview PNG changes. Device farm: N/A.
**Status: [OPEN]**

### ST-03 | P0 | "Say" has no AI: no prompt path in the prompt→app wave
**What it is.** `vieww-say-codegen` is a hand-written deterministic compiler over a fixed vocabulary (`src/lib.rs:92-163`). `rg -il "llm|openai|anthropic|gpt"` over the studio and say-codegen sources returns nothing. The studio's most marketable name — "say" — currently means *typed English with a grammar*, not *talk to it*.
**What it blocks.** The entire acquisition channel of 2025-26: bolt.new, v0.dev, Lovable all start from a sentence in a box. A user who types "a login screen with email, password, and a forgotten-password link" into viewwstudio gets… nothing. vieww answers with a file convention they must learn.
**Leaders.** bolt.new: full-stack prompt→running app in browser (https://bolt.new). v0: prompt→React/Next UI (https://v0.dev). Lovable: prompt→deployed app with GitHub sync (https://lovable.dev). Figma Make: prompt→prototype inside Figma (https://help.figma.com/hc/en-us/categories/360002051613). None of them can *prove* the result; the output is a diff the user eyeballs. Replit Agent adds zero-setup execution (https://docs.replit.com/replitai/agent).
**The fix.** Say's determinism is the moat — use it as the **target language for an LLM**, not the UX itself. Pipeline: prompt → LLM (local or BYO-key, pluggable trait) constrained to emit `.say` → deterministic codegen → real `rustc` compile → render one frame headless → require: compiles, and the frame's PNG is pixel-stable across 2 consecutive renders (the deterministic screenshot machinery already exists) → show the screen; on failure, feed the *exact diagnostic with line/column* back to the model for one retry round; show the `.say` diff for approval before it replaces the buffer.
```rust
// src/say_ai.rs (new) — the loop that no prompt-to-UI tool has
pub fn propose(studio: &Studio, prompt: &str) {
    let say = studio.model().complete_to_say(prompt);        // trait Model
    let generated = vieww_say_codegen::compile("proposal.say", &say);
    if let Ok(g) = generated {
        let png = studio.render_headless_png(&g.rust);       // existing pipeline
        if studio.png_is_stable(png, 2) { studio.offer_say_diff(say); return; }
    }
    // diagnostics (say line/col, or rustc) go back for exactly one retry
}
```
Target: new `src/say_ai.rs` (model trait + loop), `src/command.rs` (`Command::SayPrompt`), `src/ui/dialog.rs` (prompt box + diff approval). Privacy stance: model runs locally (ollama-style) or user key; no telemetry either way (matches ST-17).
**Tests.** GitHub CI: golden tests with a **recorded fake model** (fixture `tests/say_ai/fake_model.rs` returning canned `.say`), asserting: approval flow, one-retry cap, refusal to apply when compile fails twice. Self-hosted Linux/Windows: the headless-render-stability check via `examples/screenshot` machinery. Device farm: N/A.
**Status: [OPEN]**

### ST-04 | P0 | The out-of-process preview (Windows default) is missing keys, IME, scroll, hover, and state capture
**What it is.** `src/remote.rs:44-60`: "Keys, IME, scrolls, taps and drags are forwarded whole" — actually the wire covers pointer/scroll/taps per the enum, and the module doc's own "What is not here yet" plus PENDING.md §3.3: "Keyboard/IME, scroll and hover are not forwarded; preview state capture across Renders has no remote counterpart". Windows users — the platform where the remote path is *on by default* because static libstd makes in-process panics fatal — get a preview they can tap but not type into, and Say state resets on every Render.
**What it blocks.** Typing into a `text field` in the preview on Windows — the single most common preview interaction for a form screen — does nothing. Say's flagship `keep` (state survives recompile) silently doesn't apply on Windows. Any claim of "preview fidelity parity across platforms" is false today.
**Leaders.** Xcode Previews run the real SwiftUI in-process with full interactivity ("interactive previews", https://developer.apple.com/xcode/). Android Studio Compose Preview + Live Edit: static preview plus *code→running-app* push under a second (https://developer.android.com/studio/run#live-edit). Flutter hot reload preserves state across reloads by design (https://docs.flutter.dev/tools/hot-reload).
**The fix.** The wire protocol is hand-written and trivially extensible (`src/remote.rs:66+`: length-prefixed tag+payload). (1) Add `Key(u32, KeyState, Modifiers)` and `Ime(ImeEvent)` message tags — the child already mounts the guest through `guest_tree`, so delivery is `driver`-level, unchanged from the in-process path; (2) forward `ScrollEvent` (the enum exists in `vieww_foundation` imports at `src/remote.rs:41`); (3) port `capture_preview_state`/`restore_preview_state` to a `Snapshot(Vec<u8>)` message: serialize the guest's `ElementState` snapshot in the child, hand it to the host, hand it back to the new child before first frame — `ElementState::snapshot/restore` are already the mechanism in-process.
**Tests.** Self-hosted Windows: extend `tests/remote.rs` (already runs there via `windows.ps1:317`) with: type-into-field over the wire, IME composition, scroll a list, and Say `keep` surviving a remote re-Render. GitHub CI (windows-2022): same file, already wired at `framework-checks.yml:269`. Self-hosted Linux: run the suite with the remote setting forced on (settings toggle exists — "Run preview in its own process"). Device farm: N/A.
**Status: [OPEN]**

### ST-05 | P1 | No component system: custom widgets cannot be published, browsed, or reused across projects
**What it is.** Reuse today = snippets (`src/customise.rs`, user snippets file after built-ins) + the scaffold templates' `Screen`/`Widget` kinds (`src/scaffold.rs:175-212`). There is no widget library concept: no "extract this sub-tree into a named widget", no import of a widget from another project, no registry. The framework has ~40 widget-catalog examples (`examples/features/*`) reachable only as Learn lessons.
**What it blocks.** Teams can't share a `Card` once; every project re-declares it. The ecosystem flywheel (Rive's community animations, FlutterFlow's component marketplace, Figma's team libraries) has nothing to spin.
**Leaders.** FlutterFlow: reusable components + marketplace (https://docs.flutterflow.io/build-your-app/user-interface). Figma: team libraries with instances (https://help.figma.com/hc/en-us/categories/360002051620). Unity: prefabs (https://docs.unity3d.com/Manual/Prefabs.html); Godot: inherited scenes.
**The fix.** Rust makes this natural: a component = a crate. (1) "Extract widget" command: take the picked subtree's source span, generate `pub struct <Name>` + `impl Widget`, write to `src/widgets/<name>.rs`, replace the span with `<Name>` and let `ProjectLib` (already links the project rlib — `src/compile.rs:126-153`) make it previewable. (2) A local registry = a directory of template crates (`~/.local/share/viewwstudio/components/`) surfaced in the Snippets view with a "scaffold from component" action reusing `scaffold::files`. (3) Later: a git-URL component (`Dependency` already models path/version — extend with `Git`).
**Tests.** GitHub CI: `tests/extract_widget.rs` (new) — fixture project, run extract, assert the project still compiles via `tests/scaffold.rs`'s compile-everything approach and the preview renders identically (PNG diff). Self-hosted Linux/Windows: end-to-end through the real toolchain (`tests/pipeline.rs` pattern). Device farm: N/A.
**Status: [OPEN]**

### ST-06 | P1 | No layers tree / outline navigation and no multi-select
**What it is.** The inspector is "a flat list with a depth, not a nested tree… because that is what a list view can virtualise" (`src/ui/inspector.rs:22-26`) — honest, but there is no collapsible outline, no keyboard navigation between siblings, no multi-select (`inspect_selected` is `Option<usize>`, `src/state.rs`).
**What it blocks.** Finding "the third Text inside the second card" in a 400-row flat list; re-parenting; group-hiding. Every competitor's left rail does this in one click.
**Leaders.** Figma Layers panel; Unity Hierarchy; Godot Scene tree dock (https://docs.godotengine.org/en/stable/tutorials/editor/scene_tree.html); Xcode's SwiftUI hierarchy navigator in previews.
**The fix.** The snapshot already carries `depth` and `children` counts — a collapsible outline is a UI change over the same data: add `Vec<bool>` collapsed state keyed by node id, render `depth`-indented disclosure rows, and make ↑/↓/←/→ navigate. Multi-select can wait until ST-01's write path exists (multi-select without bulk edit is decoration).
**Tests.** GitHub CI: `tests/inspector_outline.rs` — build a deep guest fixture, assert collapse/filter/keyboard-nav behavior through the real driver (pattern: `tests/interaction.rs`). Self-hosted: same. Device farm: N/A.
**Status: [OPEN]**

### ST-07 | P1 | Export: no Web target, no user-app store pipelines, no macOS/Linux app packaging for user projects
**What it is.** `Format::ALL` is 5 entries (`src/export.rs:76-82`): DesktopBinary, WindowsExe, AndroidApk, IosSimulatorApp, IosIpa. The framework has a DOM web platform (`crates/vieww-platform-web-dom/` — real files in-repo) the studio never packages for. Desktop "binary" is a raw executable copied to `target/export` (`export.rs:178-180` doc) — not a `.app`/`.dmg`/`.deb` of the *user's* app, even though `packaging/package.sh` does exactly this for the studio itself. Android export is debug-signed by stated policy (`export.rs:54-59`); keystore fields exist (`state.rs:1661-1667`) but there is no release-signed flow in the UI, and no Play Store / App Store / TestFlight upload of any kind (root `ci.yml` builds debug-keystore APKs on runners and hands them to a device cloud).
**What it blocks.** "Ship to the web" — the cheapest distribution there is — is impossible from the studio. A tester with a Mac can't install the iOS build without Xcode knowledge (simulator-only). Releasing to Play requires leaving the studio entirely for Android Studio or manual CLI.
**Leaders.** FlutterFlow: one-click deploy to App Store, Play Store (with Fastlane-style code-signing management: https://docs.flutterflow.io/testing-and-deployment/deployment/overview), plus web deployment. Expo: `eas submit` for both stores (https://docs.expo.dev/submit/introduction/). Unity: full store pipelines in Build Profiles.
**The fix.** (1) `Format::Web` — planner steps: `cargo build --target wasm32-unknown-unknown` + `wasm-bindgen`/shell emit + an `index.html` (the studio's own `ci/check/wasm-check.sh` proves the web build path works in CI already); refusal-first via `toolchains::report` for the wasm target. (2) `Format::MacAppBundle` / `Format::LinuxDeb`: generalize `packaging/package.sh`'s layout logic (Info.plist, .desktop, icons from `examples/icon.rs`) into `src/export.rs` planners for *user* apps. (3) Release-APK: a signing dialog that runs `apksigner` with the password taken via a one-shot prompt → `VIEWW_KEYSTORE_PASSWORD`-style env injection into the child process only, never stored (`export.rs` already refuses to hold passwords). (4) Store upload: out of scope for OSS core, but a `Spec` for `fastlane supply`/`pilot` as an optional step keeps the planner honest.
```rust
// src/export.rs — add to Format
Web,   // "a wasm32 bundle + index.html in export/web/"
/// planner sketch
fn web(env: &Env, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    require_target(env, "wasm32-unknown-unknown")?;   // refusal names `rustup target add`
    Ok(Plan { format: Format::Web, steps: vec![
        Step { spec: cargo_build_web(root, name), why: "wasm32 build" },
        Step { spec: bindgen_gen(root, name),     why: "JS glue" },
        Step { spec: copy_index(root, out),       why: "the page" },
    ], artefact: out.join("index.html") })
}
```
**Tests.** GitHub CI: planner unit tests (the module's stated design makes every case reachable with no SDK); `export_check --only web` step added to a workflow job with `rustup target add wasm32-unknown-unknown`. Self-hosted Linux: run `export_check` full. Self-hosted Windows: `.msi`-style user-app packaging check. Device farm: upload the produced debug APK in the existing farm flow (already supported by `device-farm.yml`'s APK job).
**Status: [OPEN]**

### ST-08 | P1 | The preview is never verified on a real device; the device farm is not wired to the studio
**What it is.** Devices in the studio = 8 static presets (`src/state.rs:367`) plus adb *install* of a built artefact (`export.rs:691`). The repo's actual device farm (`.github/workflows/device-farm.yml`, AWS, low/mid/high Android phones, manual, main-only) runs framework conformance apps — nothing renders a *studio-previewed screen* on a phone and compares pixels back.
**What it blocks.** "It looked right in the preview" — the oldest lie in UI development. A layout that overflows on a 360 dp low-dpi phone is invisible in the studio's simulated frame, and the studio's core claim (preview = truth) is untested exactly where truth diverges most.
**Leaders.** Android Studio: deploy-and-run on plugged device with logcat round-trip (https://developer.android.com/studio/run). Xcode: run on device with wireless debugging. Flutter: `flutter run -d <device>` with hot reload over USB. None of them do automated preview↔device pixel comparison — which is the opening (§ 6.6).
**The fix.** Two layers. (1) Local: when an adb device is `Ready` (already known from `parse_adb_devices`), offer "mirror preview to device": push an exported debug APK (`install()` exists), launch it, and `adb exec-out screencap` back into the preview pane as a second frame — a `RemotePreview`-shaped transport with adb as the pipe. (2) Farm: a "verify on 3 devices" button in the Export sheet that dispatches the farm workflow (`workflow_dispatch` with inputs already supports custom device lists) and attaches receipts.
**Tests.** Self-hosted Linux: scripted adb loopback test with an emulator (CI already has an API 34 emulator story per repo README facts). Device farm: the verification run itself — export → farm render → screencap → `png_compare` (exists in `vieww-devtools/src/png_compare.rs`, currently unused by the studio) against the studio's headless PNG. GitHub CI: dry-run of the dispatch payload only (no device minutes spent).
**Status: [OPEN]**

### ST-09 | P1 | Frame attribution and perf overlays exist only as headless examples — no in-editor performance hints
**What it is.** `examples/frame_attribution.rs` renders mutilated copies of the studio's own scene (clips stripped, glyphs removed, shadows removed) and attributes milliseconds — a genuinely novel ablation technique — but it runs from a terminal against the *studio's* chrome, never against the *previewed screen*, and never surfaces in the UI. Flutter-style performance overlay exists in the framework (`vieww-render/src/objects/performance_overlay.rs`) but the studio doesn't mount it for the guest.
**What it blocks.** "Why is my screen janky?" has no answer inside the tool. The data is one layer down and the user can't reach it.
**Leaders.** Flutter: performance overlay + DevTools timeline (https://docs.flutter.dev/tools/devtools/performance). Xcode: debug gauges + Metal debugger. Unity: Profiler window with per-module attribution.
**The fix.** (1) Toggle "performance overlay" for the guest: mount `performance_overlay` inside `guest_tree` behind a `Studio` signal — it's a render object the framework already ships. (2) An "attribute this frame" button that runs the ablation on the *guest's* scene: snapshot `driver.scene()` (already how `capture_tree` reaches the tree), generate the mutilated copies (the example's code is directly reusable as a library call), and annotate inspector rows with per-subtree cost deltas.
**Tests.** GitHub CI: unit test that ablation of a fixture scene with N glyph commands drops ≥ the measured text cost (deterministic CPU rasterizer makes this stable — `raster_real` proves the measurement is reproducible). Self-hosted Linux: `examples/`-style regression on a recorded scene. Windows: same via remote child. Device farm: attribute on a real phone's raster (ties to ST-08).
**Status: [OPEN]**

### ST-10 | P1 | vieww-collab (CRDT + presence + multiplayer undo) exists and the studio uses none of it
**What it is.** `crates/vieww-collab/src/lib.rs` implements LwwMap, OrSet, PN-Counter, RGA collaborative `Text`, a `Doc` with vector-clock sync and JSON wire ops, and `Presence` (cursors/selections); `undo.rs` implements Figma-style selective multiplayer undo with id-remapping. `rg -ln "vieww_collab" vieww_base/apps/` → zero hits (only film_lab demos and one feature example use it).
**What it blocks.** No two people can edit one project. The single most-cited reason teams pick Figma over everything else — "we were already in it together" — is unavailable. The framework paid for the hard part and the product never cashed the check.
**Leaders.** Figma multiplayer (live cursors, presence). Google Docs-style text CRDT (Yjs) embedded by bolt.new's live sessions. Unity/Godot: no native multiplayer editing (Plastic SCM/Unity Versioning is VCS, not co-editing).
**The fix.** The studio's buffer text is the unit to sync. (1) Wrap the active buffer in `collab::Text` (RGA): local edits apply immediately and emit ops; the existing `History` stays as the local undo (or swap to `UndoManager` for cross-replica undo). (2) Transport v0 = a watched file (the `Watcher` infrastructure already exists in `file_tree.rs`): ops append as JSONL to `<workspace>/.vieww/collab.log`, replicas tail it — zero servers, works over any folder sync. (3) Presence v0 = remote carets drawn from `collab::Presence` in the editor using `layout_probe` (line/column → x/y already solved for inline diagnostics, `src/state.rs:1883-1890`).
**Tests.** GitHub CI: `tests/collab_session.rs` (new) — two `Studio` instances in one process, both on `collab::Text` over an in-memory channel (the crate's ops are values; transport is injectable), assert convergence on interleaved edits and presence positions. Self-hosted Linux: two processes over the file transport. Windows: same. Device farm: N/A.
**Status: [OPEN]**

### ST-11 | P1 | Undo is text-only; the only cross-file undo is git
**What it is.** `History` is per-buffer, whole-`TextEditingValue` snapshots, depth 200 (`src/history.rs:44`). The workspace replace flow explicitly refuses to have undo across files and shows a plan instead (`src/state.rs:1693-1698`: "there is no undo across files, so a replace-all with no preview is a change to thirty files"). Once ST-01/ST-02 land, canvas edits will need command-pattern undo anyway.
**What it blocks.** A destructive workspace operation (replace-all, extract-widget, format-on-save rewriting many files) can only be reverted via git — which requires the user to have committed first. Design-tool users expect Cmd+Z to cover *everything*, including property flips and file operations.
**Leaders.** Unity/Godot: editor-wide undo serializes every inspector change (https://docs.godotengine.org/en/stable/tutorials/editor/undo_redo.html). Xcode: source-editor undo + snapshots. Figma: unlimited session history.
**The fix.** Introduce a `Command`-level undo journal in the studio (the palette's `Command` enum is already the single vocabulary — `src/command.rs`): every structural mutation (file write, scaffold, rename, format, canvas nudge once ST-01 exists) records an inverse entry alongside the buffer histories; the palette's Undo walks the newest journal entry across types. Keep it out of the widget layer entirely, per the studio's state architecture.
**Tests.** GitHub CI: `tests/journal.rs` — perform file ops through commands, undo them in reverse, assert byte-identical directory trees (the `tests/explorer.rs` fixtures already model the FS). Self-hosted: same. Device farm: N/A.
**Status: [OPEN]**

### ST-12 | P1 | CI gates one studio test file on one OS; the 608-test suite and all probes are manual
**What it is.** `.github/workflows/framework-checks.yml:269` runs `cargo test -p viewwstudio --test remote` on windows-2022. `ci/hardware/windows.ps1:317` runs the same on the self-hosted Windows box. Nothing runs the remaining ~25 test files, `examples/` (screenshot/motion/release_check), `ci/standalone.sh`, or `certify/release-check.sh` on any schedule. TRACKER.md:1284-1289 records that the full suite last ran during the studio effort (608 tests) and later disk-full prevented re-runs.
**What it blocks.** Any studio regression on Linux CI lands silently. The deterministic screenshot/motion probes — the studio's most defensible quality assets — protect nothing if nobody runs them.
**Leaders.** Every serious IDE ships its own test suite on all its own platforms per PR (Flutter runs tool tests on 3 OSes: https://github.com/flutter/flutter/blob/master/.ci.yaml).
**The fix.** (1) New workflow `studio.yml` (GitHub-hosted, matrix ubuntu/macos/windows): `cargo test -p viewwstudio` (pipeline tests self-skip without rlibs — `tests/pipeline.rs` documents the skip), `bash apps/viewwstudio/ci/standalone.sh`, `cargo run -p viewwstudio --example screenshot -- /tmp/s.png` + PNG existence/hash gate, `cargo run --example motion -- /tmp/m --only=tabs` with frame-count assertion. (2) Self-hosted Linux job step: full `cargo test -p viewwstudio` with real rlibs + `certify/release-check.sh --quick`. (3) Self-hosted Windows: existing `remote` step plus `tests/pipeline.rs` (in-process path is what Windows *can't* do — keep remote). (4) Device farm: nightly `export_check` + APK upload already possible; keep manual for cost.
**Tests.** This gap *is* tests. Mapping: GitHub CI = the matrix above; self-hosted Linux = full suite + certify quick; self-hosted Windows = remote + pipeline-remote; device farm = nightly export_check artefact.
**Status: [OPEN]**

### ST-13 | P1 | No plugin/extension API for the studio
**What it is.** Customisation = themes, keymaps, snippets (`src/customise.rs`, `src/theme.rs::Custom`). The framework's C-ABI plugin system (per repo README) is not surfaced: no way for a third party to add a lint rule, an export format, a palette command, or a preview overlay.
**What it blocks.** The ecosystem. Linters for domain patterns, corporate design-system lints, extra export targets, custom probes — all impossible without forking. VS Code won on this axis alone.
**Leaders.** VS Code extensions; JetBrains plugins; Godot addons + editor plugins (https://docs.godotengine.org/en/stable/tutorials/plugins/editor/index.html); Unity packages + Asset Store.
**The fix.** Ship the smallest honest ABI: an `viewwstudio-plugin` crate exposing (a) `LintRule` (a `fn(&str, &Language) -> Vec<Diagnostic>` — the lint layer `src/lint.rs` already has pluggable-shaped rules), (b) `ExportFormat` (a `Plan` builder — `export.rs`'s planner/refusal split was designed so "every case here is reachable from a test", which is the same boundary a plugin needs), (c) `Command` palette entries via a registry. Load from `<data_dir>/plugins/*.dylib` with `libloading` (already a dependency, `Cargo.toml:47`) behind the same fingerprint guard the preview uses.
**Tests.** GitHub CI: an in-repo example plugin crate compiled and loaded by `tests/plugin.rs`; refusal when the fingerprint mismatches. Self-hosted: same on both OSes (dylib loading differs). Device farm: N/A.
**Status: [OPEN]**

### ST-14 | P1 | Distribution: no auto-update, unsigned installers, ~600 MB toolchain bundle
**What it is.** `packaging/package.sh --no-toolchain` is "~600 MB smaller, needs a matching rustc on PATH" (script doc) — i.e. the default bundle ships an entire rustc. `ci/release/artifacts.sh:59-61` stamps `signing="UNSIGNED (no Developer ID signature, not notarized)"` / `"UNSIGNED (no Authenticode signature)"`. There is no updater: `About` shows version/build but no check (src/about.rs `lines()`).
**What it blocks.** Real-world install friction: Gatekeeper/SmartScreen warnings on every macOS/Windows download; users on release N forever; 600+ MB download for what is, to them, an app.
**Leaders.** VS Code/Electron: delta auto-update + signed builds. JetBrains Toolbox. Figma/Rive dodge this by being web apps — the very reason they won designers.
**The fix.** (1) Split the SDK: ship a ~60 MB studio + first-launch download of the matching toolchain/rlib bundle (the `sdk::Manifest` verify already exists to make that download trustworthy; `install::target_dir`'s search chain already tolerates a late-arriving bundle). (2) Signing: wire `codesign --sign`/`notarytool` and `signtool` into `package.sh` behind env-held identities (the same never-in-a-logged-field policy as the keystore password); keep the no-identity path working and label it. (3) Update: a `--check-for-updates` Spec hitting the GitHub releases feed (the download page already uses stable asset names — `package.sh`'s "Asset names carry no version" section), showing a dialog with SHA256SUMS verification before applying.
**Tests.** GitHub CI: build the split bundle in the release workflow, assert `Manifest::read+verify` passes and the studio boots with a stub render (screenshot example). Self-hosted Linux/Windows: run the signed-artefact build once identities exist; assert the updater detects a fake "newer" feed and refuses a tampered checksum. Device farm: N/A.
**Status: [OPEN]**

### ST-15 | P2 | No asset browser or font pipeline UI
**What it is.** `WorkspaceBundle` (`src/assets.rs`) correctly resolves project assets for the *guest at runtime*, and `examples/icon.rs` generates the studio's own icons. But the Explorer shows files; nothing previews an image/font in the studio, validates icon sizes, or surfaces "which assets does this screen use".
**What it blocks.** "Drop in this PNG and see it" works only if the user knows the `assets/` convention. Designers live in the asset panel in every other tool.
**Leaders.** Unity/Godot import pipelines with inspectors; Rive asset panel; Figma assets panel with team libraries.
**The fix.** (1) An image preview when an image file is opened in the editor (decode via `vieww-asset/src/codec.rs` — already in the dependency tree — and mount like a guest screen). (2) "Insert into screen" in Say: a click on an asset inserts `an image from "assets/foo.png"` (extend the Say grammar with an `image` widget phrase backed by the existing `Image` widget — codegen change in `vieww-say-codegen/src/gen.rs`). (3) A usage report: scan the buffer for asset references and mark missing ones as Problems (the lint layer is the natural home).
**Tests.** GitHub CI: `tests/asset_preview.rs` — open a fixture PNG, assert decoded size via the headless driver; missing-reference lint fires. Self-hosted: same. Device farm: N/A.
**Status: [OPEN]**

### ST-16 | P2 | The studio UI is English-only
**What it is.** No i18n layer: all strings are literals in `src/ui/*` (rg for locale/i18n hits only incidental cases like a lowercasing comment in `src/find.rs:60`). The framework itself has an intl module (`vieww-foundation/src/intl/`) for *apps*, unused by the studio chrome.
**What it blocks.** Non-English markets; also the studio's own a11y story (screen-reader output quality tracks localized labels).
**Leaders.** Android Studio/Xcode/VS Code: full localization of the tool. Figma: 10+ languages.
**The fix.** Route chrome strings through the framework's own `intl::message` catalog (dogfood: the studio is built with vieww, so it can eat its own i18n); start with the 12 view titles + palette command names (`command.rs` is the single vocabulary — one enum, one translation table). Ship `en` + one more language as proof.
**Tests.** GitHub CI: a test asserting every `Command::title`/`View::title` has an entry in every loaded catalog (no missing-translation fallback silently shipping English). Self-hosted: smoke screenshot in the second language. Device farm: N/A.
**Status: [OPEN]**

### ST-17 | P2 | No telemetry or diagnostics export (opt-in or otherwise)
**What it is.** No telemetry code at all (verified by search). The About dialog lists version/build/target/data-dir (`src/about.rs:53-60`) but has no "copy diagnostics" action — the user cannot hand a supporter the toolchain report, last diagnostics, GPU census, or session log in one motion.
**What it blocks.** Product iteration on real usage (for a commercial push) and, more immediately, support: bug reports arrive with no environment attached.
**Leaders.** VS Code: opt-in telemetry + "export diagnostics"; Android Studio: built-in feedback with logs. For OSS-first positioning, a diagnostics *export* is the honest minimum and telemetry stays opt-in/absent.
**The fix.** `About → Copy diagnostics`: assemble the `toolchains::Env` report (already computed at construction, `src/state.rs:2446-2450`), the last N Problems entries, session/workspace paths (not contents), the `sdk::Manifest` if bundled, and the last render duration — as text into the clipboard (the `Clipboard` service is already in `SharedServices`). If telemetry is ever added: an explicit opt-in at first launch, local-first buffering, one flush endpoint, documented schema — the trust-first stance the repo already takes with passwords.
**Tests.** GitHub CI: `tests/diagnostics.rs` — assert the export contains the report fields and never buffer text (privacy test: fixture with a secret-looking string, assert absent). Self-hosted: clipboard round-trip on both OSes. Device farm: N/A.
**Status: [OPEN]**

### ST-18 | P2 | Studio accessibility is settings-deep, not audit-verified
**What it is.** Real affordances: `large_ui`, `high_contrast` (composes with custom themes, `src/state.rs:1713-1724`), caret blink off (`blink_enabled`), the semantics *overlay* for the guest. But: no keyboard-only walkthrough test exists (input tests exist for shortcuts, not for full task completion), no screen-reader verification of the studio chrome, and focus management is one `seed_focus` call (`src/lib.rs:185-189`).
**What it blocks.** Users relying on assistive tech cannot be *known* to be served; "we have high-contrast mode" is not a conformance claim. Xcode ships VoiceOver-first tooling (Accessibility Inspector).
**Leaders.** Apple Accessibility Inspector (audit mode); Android Studio's accessibility scanners for apps; VS Code's screen-reader detection and ARIA-grade web chrome.
**The fix.** (1) Add a keyboard-only task script to `examples/release_check.rs` (it already fires every palette command — add: open project → edit → render → export, keyboard only, assert completion). (2) Consume the framework's own accessibility tree of the *studio* (`vieww-accessibility` exists) and write an audit example that walks it and reports unlabeled controls — the semantics overlay proves the data is reachable. (3) Publish the audit result in `certify/release-check.sh`.
**Tests.** GitHub CI: the keyboard-only script runs headless (release_check is already windowless). Self-hosted Linux: the accessibility audit example against a real AT-SPI desktop session if available. Windows: UIA smoke via the audit example. Device farm: N/A.
**Status: [OPEN]**

### ST-19 | P2 | The "live" flow preview renders an approximation, honestly labeled, but it is still not the app
**What it is.** `src/live.rs:22-29`: the live preview parses the user's `live.rs` and "renders it with its own widgets… No logic, no data, no real controls — the switch flips and the counter counts because the *preview* holds those". The `// vieww:flow` convention (`src/state.rs:2880-2892`) silently swaps in the demo app if `live.rs` is missing, behind a caution dialog.
**What it blocks.** A user who lays out a whole flow in `live.rs` then presses Build gets something that looks similar and behaves differently — the classic mockup trap the rest of the studio is engineered to avoid (docs/04: "The preview is not a mockup of your screen, it is your screen").
**Leaders.** Xcode previews avoid this by running the real SwiftUI; Rive states run the real runtime in preview.
**The fix.** Make `live.rs` *generate* rather than *simulate*: `live.rs` becomes a Say-level artifact — the livedoc parser (`src/livedoc.rs`) emits real `.say` screens (one per flow screen, buttons → `open the screen "X"` actions), and "materialize flow" turns the approximation into compiled Rust the user then iterates on. The live pane stays for speed; a one-click upgrade path removes the cliff.
**Tests.** GitHub CI: `tests/live_materialize.rs` — parse fixture `live.rs`, emit `.say`, codegen, compile (pipeline test pattern), assert each screen has a `screen()` and navigation actions match. Self-hosted: same. Device farm: N/A.
**Status: [OPEN]**

### ST-20 | P2 | Say's vocabulary is tiny and closed: 18 widgets, 20 properties, 11 actions, 10 icons
**What it is.** `vieww-say-codegen/src/lib.rs:92-163` is the whole surface. No images, no scrollables beyond `a list of`, no async/data, no custom-widget escape hatch, no theming beyond fixed colour slots, no animation. Reserved phrases answer `E050` "what to do today".
**What it blocks.** Real apps outgrow Say in a day and drop to raw Rust — fine — but the *upgrade path* is a cliff: the generated Rust is readable but the user was chosen *not* to be a Rust programmer.
**Leaders.** FlutterFlow covers hundreds of widgets visually with an escape hatch to code; Rive covers animation/state machines deeply in one domain.
**The fix.** (1) Grow the grammar along the axes with exact Rust counterparts (the stated criterion): images (ties to ST-15), scroll views, tabs, theming (`theme` blocks mapping to `ThemeData` slots — the studio's Tokens view already enumerates slots, `src/tokens.rs`), transitions (`vieww-element/src/transition.rs` exists). (2) The escape hatch *inside* Say: a `rust` block (```rust …```) passed verbatim into the generated file with markers, so one screen can be 95% Say. (3) The registry already doubles as editor completion/docs — every phrase added gets both free.
**Tests.** GitHub CI: the fixture-contract pattern scales — one `.say`+`_gen.rs` pair per new phrase (existing test `the_counter_fixture_is_current` is the template); `render_matrix` gains a case per phrase through the real compile path. Self-hosted: pipeline tests compile every fixture. Device farm: N/A.
**Status: [OPEN]**

### ST-21 | P2 | One preview at a time: no multi-device matrix, no orientation side-by-side
**What it is.** `device: Signal<Option<Device>>` and `landscape: Signal<bool>` (`src/state.rs:1710-1712`) — a single chosen frame. The Devices *panel* (bottom) lists adb devices for install, not preview.
**What it blocks.** Checking a layout at phone + tablet + desktop simultaneously — table stakes in responsive tooling (Xcode's multi-preview variants, FlutterFlow's device preview modes).
**Leaders.** Xcode 16 previews support variants across devices/orientations in one canvas; FlutterFlow has device frame switching; Figma has multi-frame compare.
**The fix.** The in-process guest is cheap to mount twice: `guest_tree` is a pure function of (guest, metrics, platform, dark) — mount N frames in a horizontal row, each with its own `Inherited<ViewMetrics>`, sharing the *same* guest widget instance's state where state identity allows (same mechanism as `restore_preview_state`). Start with 3 fixed slots (phone/tablet/desktop) + a "+ device" menu reusing `Device::ALL`.
**Tests.** GitHub CI: `tests/multi_frame.rs` — one buffer, three frames, assert three distinct PNG regions and one state (tap in frame 1 visible in frame 2 for a Say screen). Self-hosted: driver interaction across frames. Device farm: N/A.
**Status: [OPEN]**

### ST-22 | P2 | The inspector shows structure, not state: no signal/ElementState values
**What it is.** The inspector reads the *render* tree (`describe_subtree`); a guest's `ElementState` — the Say `keep` values, the counter a user is watching misbehave — is invisible. `capture_preview_state` snapshots state for restore but never surfaces it.
**What it blocks.** "Why does it show 0 when I tapped?" — the most common debugging question for a stateful screen — has no in-tool answer.
**Leaders.** Flutter DevTools shows widget state via the widget inspector; React DevTools props/state tree; Unity shows serialized fields live in the inspector.
**The fix.** `ElementState` is `Any`-based (`as_any`/`as_any_mut` exist in the SayState fixture). Add `fn debug_fields(&self) -> Vec<(String, String)>` as a default-implemented extension in the framework trait (or a studio-side downcast registry seeded by Say's generated `SayState` — it knows its field names at codegen time and can emit a `fn say_fields(&self)` alongside). The inspector's picked-row detail then renders the state values; updates ride the existing once-per-frame capture.
**Tests.** GitHub CI: codegen fixture asserts `say_fields` output; inspector test asserts fields appear for a mounted counter guest (headless driver). Self-hosted: same. Device farm: N/A.
**Status: [OPEN]**

### ST-23 | P3 | Windows cold start is ~4× Linux (840 ms vs 221 ms) and the first compile of a session is 10.6 s cold-cache
**What it is.** PENDING.md §3.5: `main` → first frame 840 ms on Windows vs 221 ms Linux on the same i5-7200U; Vulkan device ready 215 ms, first present 197-236 ms, first-frame command recording 302 ms (no pipeline cache). `src/compile.rs:44-54`: first compile of a session 10.6 s cold cache (~100 rlibs), 0.8 s after.
**What it blocks.** First impressions on the platform with the most users; the studio's own splash (2.5 s) currently covers a chunk of this — barely.
**Leaders.** Xcode/AS/VS Code all invest heavily in startup (deferred extension host, pipeline caches, warm processes).
**The fix.** (1) A shared artefact cache for the session's first compile: pre-warm `rustc --emit=metadata` for the `--extern vieww` dependency at studio startup on a worker (the `Job` machinery and waker already exist — reuse `poll_compile`'s channel). (2) The feature branch's `probe.rs`/pipeline-cache work (vieww-render-planner `probe.rs` on `feature/main_fixes`) is the direct fix for the 302 ms first-frame recording once merged. (3) Measure with the existing `examples/coldstart.rs` in the ST-12 workflow and gate on a budget (e.g. < 500 ms Linux / < 900 ms Windows) so regressions are caught, not noticed.
**Tests.** GitHub CI: coldstart example timed with a soft budget gate (warn-level to start). Self-hosted Linux + Windows: the *real* gate — `ci/hardware` baseline already measures start-up; add `examples/coldstart.rs` medians to the receipts (the receipts/ infrastructure exists). Device farm: N/A.
**Status: [OPEN]**

**Pricing/accounts/licensing note (no gap ID needed):** there are no accounts, licensing, or billing systems anywhere in the studio — correct for an OSS tool. The only pricing-relevant infra that exists is the SDK-bundle download story (ST-14) and the device-farm spend controls (manual dispatch, main-only guard) — both appropriate. If a hosted "say" model is added (ST-03), keep it BYO-key/local to preserve the no-accounts property.

## 5. Where viewwstudio already leads

1. **rustc-in-the-loop, same-runtime preview.** The preview is not an approximation, a DSL mock, or a web canvas — it is the user's code compiled by the real compiler against the same `vieww` compilation the host links (`TypeId` fingerprint verified per guest, `src/compile.rs` ENTRY), mounted in-process under real device metrics. Xcode previews are the only comparable mechanism (real SwiftUI in-process); Compose's static preview is an approximation and Live Edit pushes to a running app instead. Flutter's hot reload is closest in spirit but targets the Dart VM, not the shipping AOT binary.
2. **Crash isolation with pixel streaming.** `src/remote.rs` gives Windows the property *no other editor preview has*: a guest `abort()`/stack-overflow/null-deref costs a child process and one frame of the studio, with the guest's stderr surfaced in Problems. Xcode preview crashes take down the preview session; Unity play-mode script errors routinely loop.
3. **Deterministic, windowless self-verification.** `examples/screenshot`, `motion` (frame-clock-driven, "a given millisecond always produces the same PNG"), `tour`, `walkthrough`, `release_check` — the studio can prove what it looks like and how it moves, in CI, with no GPU, on any OS, and `certify/release-check.sh` packages that proof per-platform. No leader publishes deterministic frame-clock regressions of its own IDE chrome.
4. **Frame attribution by ablation.** `examples/frame_attribution.rs` measures the studio's own scene minus one primitive class at a time. Flutter DevTools attributes by timeline events; attribution-by-ablation of the *app's actual scene* is a stronger, interaction-aware answer that exists nowhere else.
5. **The export refusal-first planner.** `export.rs` + `toolchains.rs` answer "you cannot, because X is missing, here is the install command" before spawning anything, with iOS-on-non-Mac stated as Apple's rule. Most tooling prints a linker error instead.
6. **State across recompiles.** `capture/restore_preview_state` + Say's generated `snapshot/restore` mean iterating on a screen does not reset it — the property Flutter advertises for hot reload, delivered through a recompile-and-remount cycle.
7. **Say's determinism contract.** Byte-identical output, committed fixture as the reviewable contract, `// say:` source map, all-diagnostics-at-once. Every prompt-to-UI competitor produces non-deterministic diffs the user must re-review from scratch each time; vieww's codegen can be diffed, cached by content hash, and trusted.
8. **SDK integrity by manifest.** A packaged studio refuses a partially-updated bundle with a sentence naming the mismatch (`src/sdk.rs`) — the failure mode nobody tests (files present, bundle wrong) is the one it checks.
9. **Recovery discipline.** Per-dirty-buffer hashed recovery files on a clock, quit-veto, conflict dialogs — many editors have autosave; few document the model this precisely and test the crash paths (`tests/persistence.rs`).

## 6. Novel methodologies NO leader has today (PoC candidates)

### 6.1 Preview ≡ export proof (the pixel-parity oracle pointed at the tool itself)
**Why no leader has it.** Flutter's preview and its AOT build go through different compilation paths (VM vs AOT); Xcode previews differ from release builds in ways Apple documents as caveats; no vendor owns a deterministic rasterizer on both sides, so the comparison is always approximate (screenshot-diff farms exist for *tests*, not as an IDE promise).
**Feasibility (code-level).** Both sides already rasterize through the same CPU `NativeRenderer`: the preview's remote child (`src/remote.rs`) and the exported app (any desktop binary renders headless per `examples/screenshot`'s no-GPU claim). `vieww-devtools/src/png_compare.rs` (currently unused by the studio) does the diff. Add an `examples/preview_export_parity.rs`: compile a fixture screen via the *preview* pipeline, render N frames; build the same file via the *export* planner (DesktopBinary), run the binary with a `--render-frame <n> --out <png>` flag (a small `harness.rs`-style main the scaffold template already models), diff. The `feature/main_fixes` branch adds `vieww-test-harness/src/parity.rs` (270 lines) — the same contract, GPU vs CPU — merge and reuse.
**PoC sketch.** `parity = PNG::diff(preview_png, export_png).max_channel_delta() == 0` asserted over the 12 `render_matrix` cases + 5 device presets.
**What it proves.** The studio's one-sentence claim — "your screen, looked at through a simulated window" — becomes a checkable release gate.

### 6.2 "Say" with compile-and-render verification (ST-03's proof loop, generalized)
**Why no leader has it.** bolt.new/v0/Lovable stream the model's diff into the editor and let the human be the verifier; none of them can cheaply *execute + render + assert stability* before showing the result, because their stacks are non-deterministic (web bundlers, live previews with state).
**Feasibility.** Every ingredient is in-repo: deterministic codegen (fixture-tested), the compile pipeline with worker + cancel, headless render to PNG (`examples/screenshot`), damage/stability reads (`driver.damage()`), and the Problems plumbing for feeding diagnostics back. The loop in ST-03's snippet is ~200 lines plus a model trait.
**PoC.** A fake model emitting three proposals (one clean, one with a Say syntax error, one that compiles but renders identically-empty twice) proving: accept / repair-once / reject-with-diff.
**What it proves.** "The AI's output is shown only after vieww has compiled it and watched it render stably" — a verifiable-AI claim nobody else in the prompt→app market can make.

### 6.3 CRDT multiplayer on the studio's own buffers (ST-10's dogfood)
**Why no leader has it.** Figma's multiplayer is proprietary infra; bolt.new's sessions are not CRDT-backed for the artifact itself; editors with CRDT backends (VS Code Live Share) bolt a separate protocol on. No UI framework ships its own collab crate *and* an IDE that could consume it — vieww has both, unused.
**Feasibility.** `collab::Text` (RGA) matches `TextEditingValue` semantics; ops are values over any transport; the studio's single-writer-per-frame signal architecture means one integration point (`Studio::put_buffers`). File-transport v0 needs zero servers.
**PoC.** Two studios, one buffer, in-memory channel: interleaved typing converges; remote presence cursor renders via `layout_probe` (same path as inline diagnostics).
**What it proves.** "The framework that builds your app also makes the tool collaborative" — and the `UndoManager` (id-remapping multiplayer undo, `crates/vieww-collab/src/undo.rs`) is already written.

### 6.4 Frame-attribution perf hints inside the canvas (ST-09's product form)
**Why no leader has it.** Profilers are separate windows with timelines (Flutter DevTools, Unity Profiler); none annotate *the widget itself* with "this subtree costs 3.1 ms/frame, 78% of it shadows" derived from ablation of the live scene.
**Feasibility.** The ablation function is a library call away from `examples/frame_attribution.rs`; the inspector already has per-node ids and bounds; per-subtree attribution = ablate the subtree's commands (clip/glyph/shadow flags already exist per command in the scene).
**PoC.** "Attribute frame" button → inspector rows gain a ms badge; the top offender row links to its source line (Say marker or LSP).
**What it proves.** Performance guidance at the point of design, not in a post-hoc profiler.

### 6.5 Receipt-verified exports
**Why no leader has it.** Nobody ships an app *with its proof*: stores give you crash reports after the fact; CI gives you logs. The repo already has a receipts culture (`receipts/`, `MANIFEST.txt`, per-fixture receipts) applied to the *framework*.
**Feasibility.** `export.rs` plans already end at an artefact; append a final `Spec` that renders the app headless on the build machine (and, via ST-08, on farm devices), writes a `render-receipt.json` (scene hashes, PNG hashes, frame timings) beside the artefact, and links it in the Output panel.
**What it proves.** Every exported app carries machine-checkable evidence of what it rendered when it shipped — the natural marketing artifact for a "receipts" brand.

### 6.6 Device-farm preview matrix (ST-08's proof form)
**Why no leader has it.** Firebase Test Lab runs *instrumented tests*; none of them answer "does my dev-previewed screen look the same on a real Pixel/Galaxy as in the IDE" as a one-click with diff-back.
**Feasibility.** `device-farm.yml` already dispatches with custom device lists and APK upload; `png_compare.rs` diffs; the studio already knows the artefact and can dispatch via `gh workflow run` (a `Spec`, like any other child process).
**PoC.** Export sheet → "verify on 3 phones" → farm run → three screenshots returned and overlaid on the preview pane with pass/fail per device.
**What it proves.** Preview fidelity is not a claim about the studio's rasterizer; it is checked against physical devices every time you ask.

## 7. Stream test plan

| Test | Runs where | Gates what | Frequency |
|---|---|---|---|
| `cargo test -p viewwstudio` full suite (26 files) | GitHub CI matrix (ubuntu/macos/windows, pipeline tests self-skip without rlibs) + self-hosted Linux with rlibs | All studio regression surfaces (ST-12) | Every PR + nightly on self-hosted |
| `ci/standalone.sh` (13 std-only modules, bare rustc, no network) | GitHub CI (ubuntu) | Logic layer without registry/network (ST-12) | Every PR |
| `tests/remote.rs` extended: keys/IME/scroll/state-capture (ST-04) | GitHub windows-2022 (`framework-checks.yml:269`) + self-hosted Windows (`windows.ps1:317`) + self-hosted Linux with remote forced on | Remote-preview parity with in-process preview | Every PR |
| `examples/screenshot` + hash gate; `examples/motion --only=tabs` frame-count gate | GitHub CI matrix | Chrome regressions, deterministic motion contract | Every PR |
| `examples/release_check` + `walkthrough` + `tour` via `certify/release-check.sh --quick` | Self-hosted Linux + self-hosted Windows | Release readiness per OS (existing script, now scheduled) | Per release branch + weekly |
| `examples/coldstart` + `frame_cost` with budgets (ST-23) | GitHub CI (warn) → self-hosted both OS (gate, medians into receipts/) | Startup and per-frame budgets | Nightly |
| `export_check` all formats compile-level | GitHub CI with `rustup target add` set | Scaffold→export plans actually compile (ST-07) | Every PR |
| `export_check` + APK/IPA full build | Root `ci.yml` pattern (manual) + device farm nightly (ST-08) | Real packaging incl. new Web format | Manual + nightly farm |
| Device-farm parity run: export → 3 phones → screencap → `png_compare` vs studio preview (§ 6.1/6.6) | Device farm (manual dispatch, main-only, as today) | Preview ≡ device claim | Per release + on demand |
| `tests/say_ai.rs` with recorded fake model (ST-03) | GitHub CI | Say-AI loop: accept/repair-once/reject, no unverified apply | Every PR |
| Say fixture contract + `render_matrix` through real compile path | GitHub CI | Codegen determinism + vocabulary coverage (ST-20) | Every PR |
| Collab convergence test (two Studios, in-memory channel) (ST-10) | GitHub CI; file-transport variant on self-hosted Linux | CRDT buffer sync + presence | Every PR / nightly |
| Keyboard-only task script + accessibility audit example (ST-18) | GitHub CI (headless) + self-hosted with desktop session | A11y of the studio chrome | Per release |
| Plugin load + fingerprint refusal (ST-13) | GitHub CI + both self-hosted (dylib per-OS) | Extension ABI safety | Every PR |
| Diagnostics export privacy test (ST-17) | GitHub CI | No buffer text leaks into exports | Every PR |

## 8. Sources

**Code read (main @ 98367be).** `apps/viewwstudio/`: `src/main.rs`, `src/lib.rs`, `src/compile.rs`, `src/remote.rs`, `src/live.rs`, `src/livedoc.rs`, `src/history.rs`, `src/recovery.rs`, `src/export.rs`, `src/install.rs`, `src/sdk.rs`, `src/scaffold.rs`, `src/assets.rs`, `src/lsp.rs` (head), `src/lessons.rs` (head), `src/builds.rs` (head), `src/state.rs` (Studio struct + View/Device/panels), `src/state/editing.rs` (auto-render, pick, overlays), `src/state/session.rs` (head), `src/ui/inspector.rs`, `src/ui/preview.rs` (guest_tree, ForwardInput refs), `build.rs`, `Cargo.toml`, `ci/standalone.sh`, `ci/gifs.sh`, `docs/01–06`, all `examples/*.rs` headers, all `tests/*.rs` headers. Framework: `crates/vieww-say-codegen/src/lib.rs` + fixtures `counter.say`/`counter_gen.rs`, `crates/vieww-collab/src/lib.rs` + `undo.rs`, `crates/vieww-devtools/src/*` (file list + `png_compare.rs`), `crates/vieww-reload/src/*` (file list), `crates/vieww-cli/src/main.rs` (head), `crates/vieww-asset/src/*` (list). CI/release: `.github/workflows/{ci,framework-checks,hardware,device-farm}.yml`, `vieww_base/ci/hardware/windows.ps1`, `vieww_base/ci/certify/release-check.sh`, `vieww_base/ci/release/artifacts.sh`, `vieww_base/packaging/package.sh`, `vieww_base/TRACKER.md`, `vieww_base/PENDING.md` (§3.3, §3.5). Branch: `git diff --stat main origin/feature/main_fixes` (studio delta: empty).

**URLs (checked live at research time).**
- Flutter hot reload: https://docs.flutter.dev/tools/hot-reload · Flutter DevTools performance: https://docs.flutter.dev/tools/devtools/performance
- FlutterFlow docs (UI building, deployment overview): https://docs.flutterflow.io/build-your-app/user-interface · https://docs.flutterflow.io/testing-and-deployment/deployment/overview · https://flutterflow.io/features
- Xcode (previews/interactive previews): https://developer.apple.com/xcode/
- Android Studio run + Live Edit: https://developer.android.com/studio/run#live-edit · Compose tooling: https://developer.android.com/develop/ui/compose/tooling
- Rive runtimes: https://help.rive.app/runtimes/overview · https://rive.app
- Figma help center (Make / Dev Mode categories): https://help.figma.com/hc/en-us/categories/360002051613
- bolt.new: https://bolt.new · v0: https://v0.dev · Lovable: https://lovable.dev (403 to curl; canonical site)
- Godot: Inspector dock https://docs.godotengine.org/en/stable/tutorials/editor/inspector_dock.html · Scene tree https://docs.godotengine.org/en/stable/tutorials/editor/scene_tree.html · Undo/Redo https://docs.godotengine.org/en/stable/tutorials/editor/undo_redo.html · Editor plugins https://docs.godotengine.org/en/stable/tutorials/plugins/editor/index.html
- Unity: Inspector https://docs.unity3d.com/Manual/UsingTheInspector.html · Prefabs https://docs.unity3d.com/Manual/Prefabs.html · Manual https://docs.unity3d.com/Manual/index.html
- Replit Agent: https://docs.replit.com/replitai/agent
- Expo submit: https://docs.expo.dev/submit/introduction/
