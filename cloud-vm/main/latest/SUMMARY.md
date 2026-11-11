# Cloud VM — FAILURE

- branch: `main`  sha: `cbc073a283fc7717ff7b3c97fd02213ec8bb4fd9`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/43009865183

| job | verdict | failed steps |
|---|---|---|
| plan | PASS |  |
| Machines · quick | skipped |  |
| Machines · wright installers | skipped |  |
| Machines · wright sandbox | skipped |  |
| Machines · studio | skipped |  |
| Mobile · U models | skipped |  |
| Machines · goldens | skipped |  |
| Machines · audio backends / macOS (Core Audio through BlackHole, CoreMIDI virtual ports) | CANCELLED |  |
| Mobile · apps | skipped |  |
| Machines · audio backends / Linux (ALSA and MIDI through kernel loopbacks, in a VM) | FAIL | ALSA (every format, rate matching both ways) and MIDI, in the VM |
| Machines · audio backends / Windows (WASAPI shared and exclusive through VB-CABLE) | FAIL | Loopback, shared mode |
| Machines · supply chain | skipped |  |
| Machines · audio backends / windows-latest (native backend) | PASS |  |
| Machines + Mobile · framework | skipped |  |
| Machines · wright models | skipped |  |
| Machines · release gate (hosted) | skipped |  |
| Machines · audio backends / Linux (PipeWire, JACK, PulseAudio) | PASS |  |
| Machines · audio backends / Android and web | PASS |  |
| Machines · audio CI / Audio decoders under cargo-fuzz (nightly, blocking) | PASS |  |
| Machines · audio CI / Audio and music crates (tests + clippy) | PASS |  |
| Machines · audio plugins / Plugins - macos-latest | FAIL | Build, bundle and validate (CLAP + VST3 + LV2) |
| Machines · audio plugins / Plugins - windows-latest | FAIL | Unit tests and clippy |
| Machines · audio plugins / Plugins - ubuntu-24.04-arm | PASS |  |
| Machines · audio backends / macos-latest (native backend) | PASS |  |
| Machines · audio plugins / Plugins - ubuntu-latest | PASS |  |
| Machines · crates | skipped |  |
| Mobile · audio | skipped |  |
| Mobile · U APK | skipped |  |
| save results / publish | running |  |

## Steps

### plan — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Run python3 .github/entry/plan.py cloud
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · quick — skipped

### Machines · wright installers — skipped

### Machines · wright sandbox — skipped

### Machines · studio — skipped

### Mobile · U models — skipped

### Machines · goldens — skipped

### Machines · audio backends / macOS (Core Audio through BlackHole, CoreMIDI virtual ports) — CANCELLED

### Mobile · apps — skipped

### Machines · audio backends / Linux (ALSA and MIDI through kernel loopbacks, in a VM) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Let the runner user use KVM
- PASS: Which stock kernel
- PASS: Cache the stock kernel packages
- PASS: Tools, kernel and its sound modules
- FAIL: ALSA (every format, rate matching both ways) and MIDI, in the VM
- skipped: ALSA sequencer virtual ports, in the VM (AU-19)
- skipped: Post Cache the stock kernel packages
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio backends / Windows (WASAPI shared and exclusive through VB-CABLE) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Install VB-CABLE
- PASS: Allow microphone access (capture)
- PASS: Endpoints list and open by id
- FAIL: Loopback, shared mode
- skipped: Loopback, exclusive mode
- skipped: MIDI ports list (WinMM)
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · supply chain — skipped

### Machines · audio backends / windows-latest (native backend) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Tests and clippy
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines + Mobile · framework — skipped

### Machines · wright models — skipped

### Machines · release gate (hosted) — skipped

### Machines · audio backends / Linux (PipeWire, JACK, PulseAudio) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Install audio servers
- PASS: Start servers
- PASS: Backend tests against the running servers
- PASS: Whole crate
- PASS: Every platform's backend type-checks and lints here
- PASS: Live tests (load, hot-plug) and a PulseAudio loopback
- skipped: Server logs
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio backends / Android and web — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain and targets
- PASS: AAudio (Android)
- PASS: Web Audio (wasm32)
- PASS: Run actions/setup-node@v4
- PASS: Web Audio shim (Node, simulated browser)
- PASS: Post Run actions/setup-node@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio CI / Audio decoders under cargo-fuzz (nightly, blocking) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Nightly and cargo-fuzz
- PASS: Seed each target's corpus
- PASS: Fuzz every target
- skipped: Findings
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio CI / Audio and music crates (tests + clippy) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Toolchain
- PASS: Run actions/cache@v4
- PASS: Tests (release)
- PASS: Clippy
- PASS: Real-time checks with a static std (AU-18)
- PASS: Notation export, validated (MusicXML XSD, LilyPond)
- PASS: Without the AAC decoder (AU-07, the distributor opt-out)
- PASS: Post Run actions/cache@v4
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio plugins / Plugins - macos-latest — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Unit tests and clippy
- FAIL: Build, bundle and validate (CLAP + VST3 + LV2)
- PASS: Upload plugin bundles
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio plugins / Plugins - windows-latest — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- FAIL: Unit tests and clippy
- skipped: Build, bundle and validate (CLAP + VST3 + LV2)
- PASS: Upload plugin bundles
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio plugins / Plugins - ubuntu-24.04-arm — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Unit tests and clippy
- PASS: Build, bundle and validate (CLAP + VST3 + LV2)
- PASS: Upload plugin bundles
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio backends / macos-latest (native backend) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Tests and clippy
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · audio plugins / Plugins - ubuntu-latest — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Unit tests and clippy
- PASS: Build, bundle and validate (CLAP + VST3 + LV2)
- PASS: Upload plugin bundles
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Machines · crates — skipped

### Mobile · audio — skipped

### Mobile · U APK — skipped

### save results / publish — running
- running: Set up job

- artifacts: vieww-plugins-ubuntu-latest vieww-plugins-macos-latest vieww-plugins-ubuntu-24.04-arm 
