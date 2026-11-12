# Market Watch — Valdi vs Vieww

One screen, written in both frameworks, measured the same way on real phones,
with each framework as it ships: Valdi as native Android views from a release
build, Vieww as its APK on its CPU rasterizer and on its GPU path.

The screen is a live stock-ticker list: rounded shadowed cards, a circular
avatar, two text lines, a 32-point sparkline, a price and a red/green pill,
under a gradient header holding the portfolio total. Both versions use the
same data generator, tick rule and load schedule, and
`cargo test -p market-watch` checks that the Rust data matches the
TypeScript generator row for row.

## Running it

Actions → **2 · Devices** → Run workflow → **Phone farm: "Valdi vs Vieww
(Android)"**, with the tiers you want (low / mid / high). It runs from `main`
only, like the rest of the farm, and needs the farm's secrets
(`DEVICE_FARM_PROJECT_ARN`, and `AWS_ROLE_ARN` or an access key pair).

| phase | job | does |
|---|---|---|
| 1 | `compare-vieww-apk` | Vieww's APK (`dev.vieww.marketwatch`) |
| 1 | `compare-valdi-apk` | Valdi's APK (`com.snap.valdi.marketwatch`), built as Valdi ships, Valdi pinned in `ci.toml` |
| 2 | `compare-farm` | both APKs on every picked phone |
| 3 | `compare-report` | `COMPARE.md` + `compare.json` → job summary and `ci-runs/compare-<branch>` |

## How Valdi is built

The way a Valdi developer ships an app: `valdi package android
--build_config release` for an arm64 phone. `compare/valdi/release-flags.mjs`
reads that command's flags from the pinned checkout's own CLI source
(`--snap_flavor=production -c opt`, the Android and arm64 flags), so the APK
follows Valdi if those flags change.

The JS engine is **QuickJS**. Valdi's workspace asks for Hermes, but Valdi
links Hermes only into debug-flavor and desktop builds, and JavaScriptCore only
on Apple platforms (`valdi/BUILD.bazel`), so a production Android build runs
QuickJS. The job asks Bazel's build graph which engines the APK links, rather
than reading strings in the binary, and fails if QuickJS is missing or Hermes is
present. It also checks for `lib/arm64-v8a/libvaldi.so` and that the APK is not
debuggable. It records all of this in `valdi-build.txt`, which the report
prints at the top.

### Build time

A cold build compiles Valdi's whole native stack from source: about 12,500
Bazel actions on a 4-core runner, about 80 minutes. Bazel's critical path is
under 6 minutes, so the rest is waiting for cores. The job keeps one cache
(`~/.cache/valdi-mw`):

- When Valdi's commit and `compare/valdi/` are unchanged, it reuses the built
  APK, and no build runs.
- When only the app changed, or Valdi moved, it warms Bazel's action and
  download caches, so only what changed is rebuilt. The action cache is capped
  at 3 GB to leave room for vieww's own caches.

The cache is saved when a job succeeds, so the first successful run pays the
80 minutes once. A bigger runner shortens that first build roughly in
proportion to its cores; set `runs_on` for `compare-valdi-apk` in `ci.toml`
to a larger runner label if your organisation has one.

## What each phone runs

Three configurations, each through the full schedule: **Vieww on its CPU
rasterizer**, **Valdi** (native Android views, drawn by HWUI) and **Vieww on
its GPU path**. They run in that order, with a minute's rest between them, so
heat from one run doesn't throttle the next.

Per configuration: 5 cold starts (`am start -W`), then this schedule, which
each app plays by itself:

| phase | what changes | length |
|---|---|---|
| `load` | the list is replaced with 2,000 (then 20,000) rows | 2 s |
| `idle` | nothing; a framework that draws here is wasting battery | 2 s |
| `tick` | three rows and the header get new prices | 1 s |
| `jump` | the list jumps 4,000 px | 1 s |
| `scroll` | 8 px every frame | 8 s |
| `live` | 8 px and three rows ticking every frame | 8 s |
| `storm` | every visible row and the header ticking every frame | 8 s |
| `thermal` | `live` for a minute, at 2,000 rows | 60 s |

Each continuous phase applies one change per drawn frame: Vieww in
`before_frame`, Valdi on its platform draw callback (`onNextDraw`).

## What is measured

SurfaceFlinger's per-layer timestats are the common yardstick. They time
Vieww's NativeActivity surface and Valdi's HWUI views the same way, giving
frames presented and the gap between them. Alongside that: each app's own
frame cost, HWUI's gfxinfo (Valdi), process CPU time, RSS and PSS, battery
temperature and thermal status, APK size, and a screenshot after each event.

Valdi's numbers reach the runner as `console.log` lines in logcat. That still
works in a release build: Valdi's Android `DefaultLogger` enables every log
level, so the release-build gate on console output lets them through.

## Files

| path | what |
|---|---|
| `src/lib.rs` | the Vieww screen, model, schedule |
| `src/main.rs` | a development tool: the schedule headless, `cargo run --release -p market-watch -- OUT [--thermal]` |
| `examples/android.rs` | the Vieww phone app |
| `compare/valdi/` | the Valdi app, `checkout.sh` (pins and prepares a Valdi tree), `release-flags.mjs` (Valdi's release flags) |
| `device-farm/` | the farm's test spec, the on-phone runner, the package builder |
| `compare_report.py` | raw output → `COMPARE.md`; `--self-test` checks its parsers |
| `ci.toml`, `ci/jobs/` | the four jobs |

## Known limits

- Android only. An iPhone leg would need a signed Valdi `.ipa` built on
  macOS, which this doesn't set up yet.
- SurfaceFlinger timestats need Android 10 or later. On older phones the
  presented-fps column is empty, and the app-side numbers remain.
