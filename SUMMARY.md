# Hardware run · main @ 37ae02292bb6fbd53906d85a1b6848e762636196

run: https://github.com/tejakota/vieww_prod/actions/runs/43419285613

- linux: failure
- windows: skipped
- macos: skipped

## linux

```
vieww Linux hardware run 20261113-171449
commit: 37ae02292bb6fbd53906d85a1b6848e762636196
host: teja-Lenovo-ideapad-320-15IKB · kernel 7.0.0-43-generic
cpu: Intel(R) Core(TM) i5-7200U CPU @ 2.50GHz
session: wayland ubuntu:GNOME
rustc: rustc 1.98.1 (48a229cea 2026-09-01) (pinned: 1.98.1)

governor: powersave
power: on mains
rapl: present but root-only — energy will be null. To enable for this boot:
      sudo chmod o+r /sys/class/powercap/intel-rapl:*/energy_uj
vulkan: 	deviceName         = Intel(R) HD Graphics 620 (KBL GT2)
vulkan: 	driverName         = Intel open-source Mesa driver
vulkan: 	deviceName         = NVIDIA GeForce 920MX
PASS  gpu-hal
PASS  gpu-3d
PASS  gpu-window
PASS  nvidia-hal
PASS  nvidia-census
PASS  gallery
PASS  census
PASS  perf-budget
      census: gpu=NVIDIA GeForce 920MX [DISCRETE_GPU] rendered_on_gpu=23/23 parity=pass
      census: over GPU-drawn fixtures cpu 221.9 ms vs gpu 965.1 ms (gpu includes readback) = 0.23x
PASS  baseline-build
      baseline start-up is warm-cache
PASS  baseline-cpu (5 launches)
PASS  baseline-gpu (5 launches)
      CPU path, 5 launches (medians):
        launch to first frame                129.18 ms
        main() to first frame                77.76 ms
        RSS at first frame                   17.6 MB
        peak RSS                             36.7 MB   [meets: baseline RSS under 40 MB]
        idle: frames presented               0   [meets: 0 frames presented while nothing changes]
        idle: pixels rasterised              0   [meets: 0 pixels rasterised while nothing changes]
        idle: CPU                            3.4%
        idle: package power                  n/a
        animate: fps                         59.591
        animate: GPU-drawn frames            0
        animate: frame work median           1.68 ms
        animate: frame work p95              2.20 ms
        animate: CPU                         12.8%
        animate: marginal energy / frame     n/a
        frame stages (median ms): cpu_raster 1.29, acquire 0.00, present 0.27
        energy: not measured on this machine (see the env step)
      GPU path, 5 launches (medians):
        launch to first frame                284.11 ms
        main() to first frame                260.40 ms
        RSS at first frame                   135.5 MB
        peak RSS                             150.4 MB   [MISSES: baseline RSS under 40 MB]
        idle: frames presented               0   [meets: 0 frames presented while nothing changes]
        idle: pixels rasterised              0   [meets: 0 pixels rasterised while nothing changes]
        idle: CPU                            0.6%
        idle: package power                  n/a
        animate: fps                         676.513
        animate: GPU-drawn frames            3383
        animate: frame work median           1.34 ms
        animate: frame work p95              2.10 ms
        animate: CPU                         60.6%
        animate: marginal energy / frame     n/a
        frame stages (median ms): plan 0.18, gpu_upload 0.01, gpu_record 0.08, gpu_submit 0.09, gpu_wait 0.40, acquire 0.26, present 0.12
        energy: not measured on this machine (see the env step)
PASS  idle-gate
PASS  rss-budget
SKIP  heaptrack (not installed — sudo apt install heaptrack (analysis: heaptrack-gui))
PASS  size-build
      windowed baseline app, dist profile, static std: 8804520 bytes (8.4M)
      dynamic libraries it loads: 3 (see size-baseline-ldd.txt)
PASS  size-budget
PASS  stress-1
PASS  stress-2
PASS  stress-3
FAIL  stress-4 (see stress-4.log)
PASS  stress-5
      animation stress p95 over 5 runs (ms): 11.586 13.185 13.497 16.396 16.980 
PASS  desktop
      sound cards:  0 [PCH ]: HDA-Intel - HDA Intel PCH;
FAIL  audio-devices (see audio-devices.log)
PASS  audio-live
PASS  audio-player
PASS  audio-soak
SKIP  audio-loopback (VIEWW_HW_AUDIO_LOOP not set (needs a line-out → line-in cable))
SKIP  audio-midi (VIEWW_MIDI_LOOP not set (needs a MIDI cable between two ports))
SKIP  wright-docker (no docker daemon on this machine)
```
