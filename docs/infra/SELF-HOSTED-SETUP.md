# Self-Hosted Runner Setup — the personal machines
# Target: `docs/infra/SELF-HOSTED-SETUP.md`
#
# The research mapped every test to GitHub CI / self-hosted Linux /
# self-hosted Windows / device farm. This is the runbook for the two boxes
# (Kaby Lake i5-7200U — the same laptop class as the receipts in
# receipts/2026-10-15-kabylake-i5-7200u-{linux,windows}/) so they can run
# everything assigned to them without babysitting.

## Linux box (`vieww-linux` runner)

### Labels
`vieww-linux` — the one `hardware.yml` waits for (GitHub adds `self-hosted, Linux, X64`). Add `android` too if a phone with USB debugging is attached (framework-checks' device suite). Descriptive extras (`gpu-intel-hd620`, …) are harmless but nothing selects on them.

### One-time setup
```bash
# Runner
mkdir -p ~/actions-runner && cd ~/actions-runner
curl -L -o runner.tgz https://github.com/actions/runner/releases/download/v2.320.0/actions-runner-linux-x64-2.320.0.tar.gz
tar xzf runner.tgz && ./config.sh --url https://github.com/tejakota/vieww_prod --token <TOKEN> \
  --name vieww-linux --labels vieww-linux --unattended
# The window steps need the desktop session: run `./run.sh` from a terminal in
# it (linux.sh's header), or install the service and export DISPLAY for it.
sudo ./svc.sh install && sudo ./svc.sh start

# Toolchain (pinned — the repo's rust-toolchain.toml pins 1.98.1)
rustup toolchain install 1.98.1
rustup component add clippy rustfmt

# GPU stack: Intel HD 620 (Mesa) + NVIDIA 920MX — both already proven in receipts
sudo apt-get install -y mesa-vulkan-drivers vulkan-tools nvidia-driver-XXX

# Audio stack (hardware.yml's audio step + audio-backends local matrix)
sudo apt-get install -y pipewire pipewire-audio jackd2 pulseaudio \
  alsa-utils loopback-dkms  # snd-aloop + snd-virmidi for MIDI round-trips
systemctl --user enable --now pipewire pipewire-pulse wireplumber

# Desktop session for the a11y + window tests (Orca/AT-SPI, FW-10)
sudo apt-get install -y orca at-spi2-core weston
# The repo already runs headless Weston for monitor tests — reuse it.

# Parity + perf gates
git -C /opt/vieww_artifacts clone https://github.com/tejakota/vieww_artifacts
echo 'export VIEWW_DEVICE_CLASS=kabylake-linux' | sudo tee /etc/profile.d/vieww.sh
```

### What runs here (nightly, via hardware.yml + sandbox-escape.yml + studio.yml)
| Leg | What it proves |
|---|---|
| `baseline, stress, gallery` + **perf-budget gate** | Real-GPU correctness; FW-07/08 perf regression gate |
| Parity suites on Intel Mesa + NVIDIA (B21 post-pass check) | Hardware parity, rows for the certificate |
| Audio: live backends 10 s + 600 s default-output soak + wired loopback + MIDI cable | Real-driver stability; latency/xrun receipts (PoC 12b) |
| Orca/AT-SPI tree assertions on a real session | FW-10 desktop screen-reader proof |
| Weston: Wayland clipboard round-trip, two-monitor refresh | FW-19 |
| wright escape suite FULL set (Landlock ABI logged per run) | WR-01/02/10 |
| wright 7-day daemon soak + kill -9 crash recovery | Session UX |
| Studio full suite with real rlibs + certify quick pass | ST-12 |

## Windows box (`vieww-windows` runner)

### Labels
`vieww-windows` — the one `hardware.yml` waits for (GitHub adds `self-hosted, Windows, X64`).

**Clock first.** The runner's OAuth tokens are minutes long: a machine whose clock is off by the time-zone offset (time typed by hand with the wrong zone) gets every token rejected as expired and its registration deleted. `Set-TimeZone -Id "India Standard Time"; w32tm /resync /force` before configuring.

### One-time setup
```powershell
# Runner (as a Windows service)
# Not under C:\Users\<you>: the service runs as NETWORK SERVICE, which cannot
# read a user profile ("Access to the path 'C:\Users\<you>' is denied").
mkdir C:\actions-runner; cd C:\actions-runner
Invoke-WebRequest -Uri https://github.com/actions/runner/releases/latest/download/actions-runner-win-x64-2.320.0.zip -OutFile runner.zip
Expand-Archive runner.zip -DestinationPath .; .\config.cmd --url https://github.com/tejakota/vieww_prod --token <TOKEN> --name vieww-windows --labels vieww-windows --runasservice --unattended
.\svc.cmd install; .\svc.cmd start   # run as a dedicated low-priv account

# Toolchain
rustup toolchain install 1.98.1

# wright's POSIX sandbox (WR-14 — doctor --fix will automate this)
wsl --install -d Ubuntu
wsl -d Ubuntu -- sudo apt-get install -y bubblewrap util-linux coreutils

# Audio: VB-CABLE for loopback CI (same tool Mozilla cubeb CI uses)
#   install VB-CABLE from vb-audio.com, set as default rec device
# Optional: Bluetooth USB dongle for the pair/unpair soak (AU-09)

# Studio SDK for the full pipeline tests
#   install the studio bundle once; install::target_dir finds it beside the executable
```

### What runs here (nightly)
| Leg | What it proves |
|---|---|
| `windows.ps1` baseline + **perf-budget gate** (`kabylake-windows`) | FW-16: warm first-frame <= 350 ms gate |
| D3D12 scene suite on the Intel iGPU (real drivers, not WARP) | FW-05 — the branch's renderer on real hardware |
| WASAPI shared + exclusive through VB-CABLE, loopback analyzer on | AU-10 latency contract on silicon |
| Studio remote tests + in-process pipeline + MSI smoke | ST-04/ST-12 |
| wright WSL2 sandbox suite + worker isolation + Credential Manager | WR-05/06/14 |
| Updater e2e (serve v2 locally -> swap -> relaunch) | FW-24/WR-04 channel dogfooding |
| Crash-report child test on a shipped-profile build | FW-12 |

## Mac (`vieww-macos` runner)

### Labels
`vieww-macos` — the one `hardware.yml`'s `macos` job waits for (GitHub adds `self-hosted, macOS` and the architecture). Apple silicon or Intel.

### One-time setup
```bash
# Runner — in the logged-in user's account: the window steps need its WindowServer.
mkdir -p ~/actions-runner && cd ~/actions-runner
curl -L -o runner.tgz https://github.com/actions/runner/releases/download/v2.320.0/actions-runner-osx-arm64-2.320.0.tar.gz   # osx-x64 on Intel
tar xzf runner.tgz
./config.sh --url https://github.com/tejakota/vieww_prod --token <TOKEN> \
  --name vieww-macos --labels vieww-macos --unattended
./svc.sh install && ./svc.sh start   # a LaunchAgent for this user — never sudo (a LaunchDaemon has no GUI session)

# Toolchain
xcode-select --install
rustup toolchain install 1.98.1

# Optional, each turns a SKIP into a run:
brew install molten-vk vulkan-loader vulkan-tools   # `moltenvk` step: Vulkan suites + GPU census
brew install coreutils                              # gtimeout (perl is the fallback)
brew install --cask blackhole-2ch && brew install switchaudio-osx
SwitchAudioSource -s "BlackHole 2ch" -t output; SwitchAudioSource -s "BlackHole 2ch" -t input
#   then dispatch with VIEWW_HW_AUDIO_LOOP=coreaudio set in the runner's .env file
```
Grant the terminal that started the runner Screen Recording and Accessibility (System Settings → Privacy & Security) once; keep the Mac on power with sleep off (`sudo pmset -c sleep 0 displaysleep 0`).

### What runs here (`ci/hardware/macos.sh`, dispatch Hardware with `macos: true`)
| Leg | What it proves |
|---|---|
| `gpu` — Metal suites + a GPU frame in a real window | FW-04 — Metal on real Apple silicon, where hosted CI has a paravirtual GPU |
| `moltenvk` — Vulkan suites + GPU census through MoltenVK | the path `package.sh --bundle-moltenvk` ships |
| `gallery` + perf-budget (`hardware-macos` class) | FW-07/08 — first run is "new (no budget)"; record a row from its receipt |
| `baseline`, idle gate, RSS budget | FW-15 on macOS |
| `size`, `stress`, `desktop` | as on Linux |
| `audio` — Core Audio live tests, soak, BlackHole loopback, MIDI | AU-08/10 on silicon |

## Shared policies
1. **A skipped leg is "not run", never "passed"** — the repo's own rule
   (framework-checks.yml comment). Both runners MUST report per-step
   verdicts into `receipts/` on every run (the `ci-results/*` branch
   pattern already exists).
2. **Budgets live in-repo** (`budgets.json`, PoC 3): the machines are the
   measurement, the repo is the contract, `--record` is the only way to
   move a budget, and every move is a reviewed commit.
3. **Thermal identity**: both boxes are the same laptop class — that is a
   feature (Linux-vs-Windows deltas on identical silicon are the cleanest
   cross-OS signal in the whole program). Keep them plugged in, on power,
   with fixed brightness (screens off is fine — tests are headless).
4. **Cost cadence**: nightly everything; the device farm stays
   weekly/per-release (AWS minutes) — the farm's manual-dispatch guard for
   non-main refs stays.
