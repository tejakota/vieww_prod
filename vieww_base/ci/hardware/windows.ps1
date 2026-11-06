Run powershell -NoProfile -ExecutionPolicy Bypass -File ci\hardware\windows.ps1 -Steps "$env:STEPS" -Launches ([int]$env:LAUNCHES) -OutDir "$env:OUT"
rustc : The term 'rustc' is not recognized as the name of a cmdlet, function, script file, or operable program. Check 
the spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:177 char:11
+ $rustc = (rustc -V 2>$null)
+           ~~~~~
    + CategoryInfo          : ObjectNotFound: (rustc:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
gpu: Intel(R) HD Graphics 620 (driver 26.20.100.6859)
gpu: NVIDIA GeForce 920MX (driver 32.0.15.7652)
power: on mains
power plan: Balanced
vulkan: NVIDIA GeForce 920MX
vulkan: Intel(R) HD Graphics 620
      WARNING: rustc is not the pinned 1.98.1 - numbers are for a different compiler
      energy: not measured on Windows (no package energy counter without a driver); reported as null

==> gpu-hal: cargo test --release -p vieww-hal --features vulkan -- --ignored --test-threads=1
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  gpu-hal (exit ; see gpu-hal.log)

==> gpu-3d: cargo test --release -p vieww-3d --features vulkan -- --ignored --test-threads=1
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  gpu-3d (exit ; see gpu-3d.log)

==> gpu-window: cargo test --release -p vieww-platform-winit --test wait_loop
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  gpu-window (exit ; see gpu-window.log)

==> d3d12-hardware: cargo test --release -p vieww-hal --features d3d12 -- --ignored --test-threads=1
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  d3d12-hardware (exit ; see d3d12-hardware.log)
      d3d12: bring-up and smoke suite only - the backend does not execute a ScenePlan yet (PENDING 1.3)

==> gallery: cargo run --release -p fixtures -- C:\actions-runner\_work\_temp\hardware-windows\fixtures --runs 30 --receipt C:\actions-runner\_work\_temp\hardware-windows\fixtures\receipt.json --shadow C:\actions-runner\_work\_temp\hardware-windows\fixtures\shadow.jsonl
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  gallery (exit ; see gallery.log)

==> census: cargo run --release -p fixtures -- C:\actions-runner\_work\_temp\hardware-windows\census --census --runs 30 --receipt C:\actions-runner\_work\_temp\hardware-windows\census\receipt.json --shadow C:\actions-runner\_work\_temp\hardware-windows\census\shadow.jsonl
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  census (exit ; see census.log)
SKIP  perf-budget (no working Python (the Store alias is not one; install from python.org or disable the alias))

==> baseline-build: cargo build --release -p vieww-platform-winit --example baseline
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  baseline-build (exit ; see baseline-build.log)

==> size-build: cargo build --profile dist -p vieww-platform-winit --example baseline --target-dir target\size
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  size-build (exit ; see size-build.log)

==> pipelines-build: cargo build --release -p vieww-platform-winit --example baseline
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  pipelines-build (exit ; see pipelines-build.log)
FAIL  pipelines (pipelines-build failed)

==> stress-1: cargo run --release -p test-animation-stress -- C:\actions-runner\_work\_temp\hardware-windows\stress-1
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  stress-1 (exit ; see stress-1.log)

==> stress-2: cargo run --release -p test-animation-stress -- C:\actions-runner\_work\_temp\hardware-windows\stress-2
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  stress-2 (exit ; see stress-2.log)

==> stress-3: cargo run --release -p test-animation-stress -- C:\actions-runner\_work\_temp\hardware-windows\stress-3
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  stress-3 (exit ; see stress-3.log)

==> stress-4: cargo run --release -p test-animation-stress -- C:\actions-runner\_work\_temp\hardware-windows\stress-4
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  stress-4 (exit ; see stress-4.log)

==> stress-5: cargo run --release -p test-animation-stress -- C:\actions-runner\_work\_temp\hardware-windows\stress-5
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  stress-5 (exit ; see stress-5.log)
      desktop: using C:\Program Files\Git\bin\bash.exe

==> desktop: C:\Program Files\Git\bin\bash.exe ci/certify/desktop-suite.sh --log C:\actions-runner\_work\_temp\hardware-windows\desktop-app.log
desktop-suite: cargo not found.
FAIL  desktop (exit 127; see desktop.log)

==> remote: cargo test -p viewwstudio --test remote -- --test-threads=1
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  remote (exit ; see remote.log)
      audio endpoints: Microphone (Realtek High Definition Audio); Speakers (Realtek High Definition Audio)

==> audio-devices: cargo test --release -p vieww-audio --lib backend::wasapi -- --nocapture
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  audio-devices (exit ; see audio-devices.log)

==> audio-player: cargo test --release -p vieww-audio --test player_smoke -- --ignored --nocapture
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  audio-player (exit ; see audio-player.log)

==> audio-low-latency-shared: cargo test --release -p vieww-audio --lib low_latency_shared_on_a_real_endpoint -- --ignored --nocapture
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  audio-low-latency-shared (exit ; see audio-low-latency-shared.log)
SKIP  audio-default-flip (one render endpoint (install VB-CABLE for a second))

==> audio-soak-wasapi: cargo test --release -p vieww-audio --test device_loopback default_output_soak -- --ignored --nocapture
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  audio-soak-wasapi (exit ; see audio-soak-wasapi.log)

==> audio-soak-wasapi-exclusive: cargo test --release -p vieww-audio --test device_loopback default_output_soak -- --ignored --nocapture
& : The term 'cargo' is not recognized as the name of a cmdlet, function, script file, or operable program. Check the 
spelling of the name, or if a path was included, verify that the path is correct and try again.
At C:\actions-runner\_work\vieww_prod\vieww_prod\vieww_base\ci\hardware\windows.ps1:120 char:7
+     & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
+       ~~~~
    + CategoryInfo          : ObjectNotFound: (cargo:String) [], CommandNotFoundException
    + FullyQualifiedErrorId : CommandNotFoundException
 
FAIL  audio-soak-wasapi-exclusive (exit ; see audio-soak-wasapi-exclusive.log)
SKIP  audio-loopback (VIEWW_HW_AUDIO_LOOP not set (needs a line-out to line-in cable))
SKIP  audio-midi (VIEWW_MIDI_LOOP not set (needs a MIDI cable between two ports))

================ summary ================
vieww Windows hardware run 20261106-142149
commit: 7c3595fa89c83d768ec89f32ee0f64545ed15051
host: TEJA22 - Microsoft Windows NT 10.0.19045.0
cpu: Intel(R) Core(TM) i5-7200U CPU @ 2.50GHz
rustc: NOT RESOLVED - rustup could not provide the pinned toolchain (pinned: 1.98.1)

gpu: Intel(R) HD Graphics 620 (driver 26.20.100.6859)
gpu: NVIDIA GeForce 920MX (driver 32.0.15.7652)
power: on mains
power plan: Balanced
vulkan: NVIDIA GeForce 920MX
vulkan: Intel(R) HD Graphics 620
      WARNING: rustc is not the pinned 1.98.1 - numbers are for a different compiler
      energy: not measured on Windows (no package energy counter without a driver); reported as null
FAIL  gpu-hal (exit ; see gpu-hal.log)
FAIL  gpu-3d (exit ; see gpu-3d.log)
FAIL  gpu-window (exit ; see gpu-window.log)
FAIL  d3d12-hardware (exit ; see d3d12-hardware.log)
      d3d12: bring-up and smoke suite only - the backend does not execute a ScenePlan yet (PENDING 1.3)
FAIL  gallery (exit ; see gallery.log)
FAIL  census (exit ; see census.log)
SKIP  perf-budget (no working Python (the Store alias is not one; install from python.org or disable the alias))
FAIL  baseline-build (exit ; see baseline-build.log)
FAIL  size-build (exit ; see size-build.log)
FAIL  pipelines-build (exit ; see pipelines-build.log)
FAIL  pipelines (pipelines-build failed)
FAIL  stress-1 (exit ; see stress-1.log)
FAIL  stress-2 (exit ; see stress-2.log)
FAIL  stress-3 (exit ; see stress-3.log)
FAIL  stress-4 (exit ; see stress-4.log)
FAIL  stress-5 (exit ; see stress-5.log)
      desktop: using C:\Program Files\Git\bin\bash.exe
FAIL  desktop (exit 127; see desktop.log)
FAIL  remote (exit ; see remote.log)
      audio endpoints: Microphone (Realtek High Definition Audio); Speakers (Realtek High Definition Audio)
FAIL  audio-devices (exit ; see audio-devices.log)
FAIL  audio-player (exit ; see audio-player.log)
FAIL  audio-low-latency-shared (exit ; see audio-low-latency-shared.log)
SKIP  audio-default-flip (one render endpoint (install VB-CABLE for a second))
FAIL  audio-soak-wasapi (exit ; see audio-soak-wasapi.log)
FAIL  audio-soak-wasapi-exclusive (exit ; see audio-soak-wasapi-exclusive.log)
SKIP  audio-loopback (VIEWW_HW_AUDIO_LOOP not set (needs a line-out to line-in cable))
SKIP  audio-midi (VIEWW_MIDI_LOOP not set (needs a MIDI cable between two ports))

results: C:\actions-runner\_work\_temp\hardware-windows
Error: Process completed with exit code 1.
