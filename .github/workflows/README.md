# Workflows

Everything runs from **four forms**, and only by hand: Actions → pick a form
→ **Run workflow**. Nothing starts on a push, a pull request, a schedule or
another workflow finishing.

| Form | Runs on |
|---|---|
| **Cloud VM · Machines** (`entry-cloud-machines.yml`) | GitHub's hosted Linux, macOS and Windows machines |
| **Cloud VM · Mobile** (`entry-cloud-mobile.yml`) | GitHub's hosted machines, building and testing for phones |
| **Device · Machines** (`entry-device-machines.yml`) | your own Linux, Windows and macOS machines |
| **Device · Mobile** (`entry-device-mobile.yml`) | real phones: the phone farm and your own Android phone |

Every other file here is a **part** (`cloud-machines-*`, `cloud-mobile-*`,
`device-machines-*`, `device-mobile-*`, and `shared-*` for the pieces two
forms use). Parts have only `workflow_call`, so they have no Run workflow
button; a form calls a part when its box is ticked.

## Choosing what runs

Every choice is a checkbox or a dropdown. An item's box runs it; the fields
marked `└` under it choose its steps and options. Choosing a step also runs
the steps it needs (the phone farm's `phones` builds the APK first). The run's
summary page starts with a table of exactly what was chosen.

`advanced` is the one text field, optional, for what the boxes don't offer:
`item:step step key=value; item2:…`, for example `release_gate:windows-cpu`
(one hosted leg) or `phone_farm:devices=A14,Pixel` (phones by part of their
name; no spaces). An unknown item, step or option fails the run's first job
with the valid list.

**save results** (on by default) writes the run to the `ci-results` branch.

### Cloud VM · Machines

| Field | Kind | Choices |
|---|---|---|
| Quick checks: fmt, clippy, workflow lint, Wright sandbox smoke | ☐ box |  |
| Framework checks | ☐ box |  |
| └ which framework checks | dropdown | all / core (gate, coverage, cross) / gpu (lavapipe) / platforms (macOS, Windows, web) / extras (audio fuzz, a11y, plugins) |
| Studio suite (Linux, macOS, Windows) | ☐ box |  |
| Wright sandbox suites | ☐ box |  |
| └ build profile | dropdown | debug / release |
| Wright built-in models | ☐ box |  |
| Wright desktop installers | ☐ box |  |
| └ version to stamp (no leading v) | text |  |
| └ build only, or publish | dropdown | build only / publish as pre-release / publish as release |
| Audio | ☐ box |  |
| └ which audio | dropdown | all / ci (tests, fuzz) / backends (servers, loopback, targets) / plugins (CLAP, VST3, LV2) |
| Release gate, hosted legs (Windows, macOS, Linux export) | ☐ box |  |
| └ scope | dropdown | gate / checks / screenshots |
| └ rerun | dropdown | failed / all |
| └ build installers | ☐ box | on by default |
| Supply chain: advisories, SBOM, reproducible build | ☐ box |  |
| Golden screenshots | ☐ box |  |
| └ capture a new baseline instead of comparing | ☐ box |  |
| Crates: check, verify (publishes only on a v* tag) | ☐ box |  |
| Optional, for what the boxes don't offer: item:step key=value; … (see .github/workflows/README.md) | text |  |
| Save this run's results to the ci-results branch | ☐ box | on by default |

### Cloud VM · Mobile

| Field | Kind | Choices |
|---|---|---|
| Build the mobile apps | ☐ box |  |
| └ platform | dropdown | both / android / ios |
| U APK | ☐ box |  |
| U model runtime | ☐ box |  |
| Mobile audio | ☐ box |  |
| └ which | dropdown | all / logic / android / ios |
| Framework: Android build | ☐ box |  |
| Framework: Android emulator | ☐ box |  |
| Framework: iOS simulator | ☐ box |  |
| Optional, for what the boxes don't offer: item:step key=value; … (see .github/workflows/README.md) | text |  |
| Save this run's results to the ci-results branch | ☐ box | on by default |

### Device · Machines

| Field | Kind | Choices |
|---|---|---|
| Feature run: the checks for what this branch changed | ☐ box |  |
| └ areas | dropdown | auto (from the branch's changes) / framework / wright / studio / prototype / all |
| └ depth | dropdown | quick / full |
| Hardware: Linux | ☐ box |  |
| Hardware: Windows | ☐ box |  |
| Hardware: macOS | ☐ box |  |
| └ which hardware steps | dropdown | all / gpu / visual (gallery, baseline) / performance (size, stress) / desktop (windows, audio) |
| └ launches per timing | dropdown | 5 / 3 / 1 / 10 |
| Release gate on your machines | ☐ box |  |
| └ Linux CPU | ☐ box | on by default |
| └ Linux GPU | ☐ box | on by default |
| └ Windows GPU | ☐ box | on by default |
| └ scope | dropdown | gate / checks / screenshots |
| └ rerun | dropdown | failed / all |
| └ build installers | ☐ box | on by default |
| Optional, for what the boxes don't offer: item:step key=value; … (see .github/workflows/README.md) | text |  |
| Save this run's results to the ci-results branch | ☐ box | on by default |

### Device · Mobile

| Field | Kind | Choices |
|---|---|---|
| Phone farm: run on real phones | ☐ box |  |
| └ low-end phone | ☐ box | on by default |
| └ mid-range phone | ☐ box | on by default |
| └ high-end phone | ☐ box | on by default |
| └ platform | dropdown | android / ios / both |
| └ phones spread from cheapest to most capable (before tiers) | dropdown | 3 / 5 / 8 |
| └ APK build profile | dropdown | dist / release |
| └ record video of each phone | ☐ box |  |
| Your Android phone (USB): the device suite | ☐ box |  |
| Optional, for what the boxes don't offer: item:step key=value; … (see .github/workflows/README.md) | text |  |
| Save this run's results to the ci-results branch | ☐ box | on by default |

## Changing a form

The forms are generated. Edit `.github/entry/plan.py` (the fields, and what
each choice runs), then:

```bash
python3 .github/entry/entries.py          # rewrite the four entry-*.yml
python3 -I .github/entry/test_plan.py    # what each choice runs; forms match the parts
```

Cloud VM · Machines → quick checks runs both, so a stale form or a step the
parts no longer have fails CI. Adding a job to a part means adding its id to
`ITEMS` in `plan.py` (the test names the mismatch).

## How the files fit together

```text
entry-<form>.yml ─┬─ plan            .github/entry/plan.py <form>: reads the form
                  ├─ <its parts>     cloud-machines-*.yml, device-mobile-*.yml, …
                  ├─ shared-*.yml    framework checks, release gate (hosted legs on
                  │                  Cloud VM, self-hosted legs on Device)
                  └─ shared-results.yml   last, if save results
```

- Secrets reach parts through `secrets: inherit`; each call grants only the
  permissions its part needs.
- Parts' concurrency groups start with the form's name, so runs of two forms
  never cancel each other.
- `hardware` and the phone farm still refuse any branch but `main`.
- Device · Machines needs the self-hosted runners registered with their
  labels (`vieww-linux`, `vieww-windows`, `vieww-macos`, `vieww-gate`,
  `android`); the feature run's machine is the repository variable
  `FEATURE_RUNNER`.

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
| ci-results (ran after other workflows) | shared-results (each form's last job) |

`feature_branch_variants/` keeps reference copies from the old
`feature/main_fixes` branch; GitHub does not read subfolders.
