# Workflows

Everything runs from **two entry points**, and only by hand:

| Run workflow on | Runs on | Groups |
|---|---|---|
| **Cloud VM** (`cloud-vm.yml`) | GitHub's hosted virtual machines | Machines · Mobile |
| **Device** (`device.yml`) | Your own machines and phones (self-hosted runners, the phone farm) | Machines · Mobile |

Nothing starts on a push, a pull request, a schedule or another workflow
finishing. Every other file here is a **part**: it has only `workflow_call`,
so it has no Run workflow button of its own, and an entry point calls it
when its item is ticked.

## Running something

1. Actions → **Cloud VM** or **Device** → **Run workflow**.
2. Pick the branch in the dropdown.
3. Tick the items to run. Each description starts with its group,
   **Machines ·** or **Mobile ·**.
4. Optionally narrow or tune them in **steps**: items separated by `;`,
   each `item:` followed by step names and `key=value` options.

   ```text
   audio:fuzz test; release_gate:windows-cpu rerun=all
   wright_installers:version=1.2.0 publish=true
   hardware_linux:gpu stress launches=3; feature:areas=wright depth=full
   ```

   An item named in `steps` runs even if its box is not ticked. Choosing a
   step also runs the steps it needs (for example `phone_farm:phones` builds
   the APK first). An unknown item, step or option fails the run's first job
   with the list of valid ones, so a typo never runs a check that tests
   nothing.
5. **save results** (on by default) writes the run to the `ci-results`
   branch when everything else has finished.

The run's summary page starts with a table of exactly what was chosen.

## What each item runs

### Cloud VM

| Group | Item | Part | Steps · options |
|---|---|---|---|
| Machines | `quick` | `cloud-machines-quick.yml` | quick workflows wright-sandbox |
| Machines | `framework` | `shared-framework-checks.yml` | gate coverage cross gpu-lavapipe macos windows web audio-fuzz a11y-gallery plugin-compat |
| Machines | `studio` | `cloud-machines-studio.yml` | suite |
| Machines | `wright_sandbox` | `cloud-machines-wright-sandbox.yml` | linux-escape-suite macos-seatbelt-suite windows-fail-closed windows-doctor-fix · profile=debug\|release |
| Machines | `wright_models` | `cloud-machines-wright-models.yml` | reference platforms |
| Machines | `wright_installers` | `cloud-machines-wright-installers.yml` | build release update-channel-tests · version= publish=false prerelease=true |
| Machines | `audio` | `cloud-machines-audio-{ci,backends,plugins}.yml` | test fuzz linux-servers linux-alsa-loopback windows-loopback macos-loopback desktop targets plugins |
| Machines | `release_gate` | `shared-release-gate.yml` (hosted legs) | windows-cpu macos-cpu macos-gpu linux-export · scope= rerun= package= |
| Machines | `supply_chain` | `cloud-machines-supply-chain.yml` | advisories sbom repro |
| Machines | `goldens` | `cloud-machines-goldens.yml` | shots · update=false |
| Machines | `publish_crates` | `cloud-machines-publish.yml` | check verify scaffold-external publish (publishes only on a `v*` tag) |
| Mobile | `mobile_apps` | `cloud-mobile-apps.yml` | android ios |
| Mobile | `u_apk` | `cloud-mobile-u-apk.yml` | apk |
| Mobile | `u_models` | `cloud-mobile-u-models.yml` | model |
| Mobile | `mobile_audio` | `cloud-mobile-audio.yml` | logic android ios |
| Mobile | `framework_mobile` | `shared-framework-checks.yml` | android ios-simulator android-emulator |

### Device

| Group | Item | Part | Steps · options |
|---|---|---|---|
| Machines | `feature` | `device-machines-feature.yml` | areas=auto\|framework,wright,studio,prototype depth=quick\|full |
| Machines | `hardware_linux` | `device-machines-hardware.yml` | env gpu nvidia gallery baseline size stress desktop audio wright-docker · launches=5 |
| Machines | `hardware_windows` | `device-machines-hardware.yml` | env gpu d3d12 gallery baseline size pipelines stress desktop remote audio · launches=5 |
| Machines | `hardware_macos` | `device-machines-hardware.yml` | env gpu moltenvk gallery baseline size stress desktop audio · launches=5 |
| Machines | `release_gate_device` | `shared-release-gate.yml` (self-hosted legs) | linux-cpu linux-gpu windows-gpu · scope= rerun= package= |
| Mobile | `phone_farm` | `device-mobile-phone-farm.yml` | apk phones ipa iphones · platform=android\|ios\|both profile=dist\|release devices= count=3 video=false |
| Mobile | `android_phone` | `shared-framework-checks.yml` (android-device leg) | — |

`hardware` and the phone farm refuse to run from any branch but `main`, as
before. Device's machine items need the self-hosted runners registered with
their labels (`vieww-linux`, `vieww-windows`, `vieww-macos`, `vieww-gate`,
`android`); `feature` uses the repository variable `FEATURE_RUNNER` to
pick its machine.

## How the files fit together

```text
cloud-vm.yml ─┬─ plan  (.github/entry/plan.py cloud: reads the form)
              ├─ cloud-machines-*.yml   cloud-mobile-*.yml
              ├─ shared-framework-checks.yml   shared-release-gate.yml
              └─ shared-results.yml     (last, if save results)
device.yml ───┬─ plan  (.github/entry/plan.py device)
              ├─ device-machines-*.yml  device-mobile-*.yml
              ├─ shared-framework-checks.yml   shared-release-gate.yml
              └─ shared-results.yml
```

- `.github/entry/plan.py` holds the catalogue: every item, its steps and
  options. Adding a job to a part means adding its id there too.
- Each part's header names its entry point, group, item and steps.
- Secrets reach the parts through `secrets: inherit`; each call grants the
  permissions its part needs (for example `contents: write` for the parts
  that publish).
- Parts' concurrency groups start with the entry's name, so a Cloud VM run
  and a Device run never cancel each other.

## Renamed from

| Before | Now |
|---|---|
| pr-quick | cloud-machines-quick |
| framework-checks | shared-framework-checks |
| studio | cloud-machines-studio |
| wright-sandbox / wright-models / wright-release | cloud-machines-wright-sandbox / -models / -installers |
| audio-ci / audio-backends / audio-plugins | cloud-machines-audio-ci / -backends / -plugins |
| release-gate | shared-release-gate |
| supply-chain / goldens / publish | cloud-machines-supply-chain / -goldens / -publish |
| ci ("Build Mobile Apps") | cloud-mobile-apps |
| u-apk / u-models / mobile-audio | cloud-mobile-u-apk / -u-models / -audio |
| feature-runs | device-machines-feature |
| hardware | device-machines-hardware |
| device-farm | device-mobile-phone-farm |
| ci-results (ran after other workflows) | shared-results (the entries' last job) |

`feature_branch_variants/` keeps reference copies from the old
`feature/main_fixes` branch; GitHub does not read subfolders.
