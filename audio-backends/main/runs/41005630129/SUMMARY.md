# Audio backends — FAILURE

- branch: `main`  sha: `7a9e76951688bed17b8424d7173bd503612b9aa2`  event: workflow_dispatch
- run: https://github.com/tejakota/vieww_prod/actions/runs/41005630129

| job | verdict | failed steps |
|---|---|---|
| Android and web | PASS |  |
| macos-latest (native backend) | PASS |  |
| Linux (ALSA and MIDI through kernel loopbacks, in a VM) | FAIL | Tools, kernel and its sound modules |
| Windows (WASAPI shared and exclusive through VB-CABLE) | FAIL | Loopback, shared mode |
| macOS (Core Audio through BlackHole, CoreMIDI virtual ports) | PASS |  |
| Linux (PipeWire, JACK, PulseAudio) | PASS |  |
| windows-latest (native backend) | PASS |  |

## Steps

### Android and web — PASS
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

### macos-latest (native backend) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Tests and clippy
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Linux (ALSA and MIDI through kernel loopbacks, in a VM) — FAIL
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Let the runner user use KVM
- PASS: Which stock kernel
- PASS: Cache the stock kernel packages
- FAIL: Tools, kernel and its sound modules
- skipped: ALSA (every format, rate matching both ways) and MIDI, in the VM
- skipped: Post Cache the stock kernel packages
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Windows (WASAPI shared and exclusive through VB-CABLE) — FAIL
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

### macOS (Core Audio through BlackHole, CoreMIDI virtual ports) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Install BlackHole and make it both defaults
- PASS: Allow microphone access (capture)
- PASS: Devices list and open by UID
- PASS: Loopback through BlackHole
- PASS: CoreMIDI virtual endpoints round trip
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

### Linux (PipeWire, JACK, PulseAudio) — PASS
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

### windows-latest (native backend) — PASS
- PASS: Set up job
- PASS: Run actions/checkout@v4
- PASS: Install Rust toolchain
- PASS: Tests and clippy
- PASS: Post Run actions/checkout@v4
- PASS: Complete job

- artifacts: 
