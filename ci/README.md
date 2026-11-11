# CI: projects, manifests and the five workflows

Every run starts by hand, from **Actions → Run workflow**, on one of five
workflows. Nothing runs on a push, a pull request or a schedule.

| Workflow | Asks | Runs |
|---|---|---|
| **1 · Check** | Project · Depth (quick / thorough) · Run on (GitHub's machines / My Linux machine) | the projects' checks |
| **2 · Devices** | Linux / Windows / Mac machine · what to run · USB phone · phone farm (Android / iPhone / both; low / mid / high-end; video) | hardware suites, the phone on USB, AWS Device Farm (from `main` only) |
| **3 · Build** | Project · platforms (Linux, Windows, macOS, Android, iOS, web) · version | installers, APKs, IPAs, sites, as artifacts |
| **4 · Release** | Mode (rehearse / gate and build / publish) · include my machines · version · pre-release | the release gate, supply chain, crate checks; then builds; then publishing, after approval |
| **5 · Housekeeping** | Task (goldens / models / everything) | refresh golden screenshots and reference models |

The workflows hold only their forms. **What runs comes from the projects.**

## Projects

A project is a folder with a `ci.toml`. Today:

| Project | Folder | Kind |
|---|---|---|
| vieww | `vieww_base` (the workspace, and anything in it without its own ci.toml) | library |
| vieww-audio, vieww-audio-plugin | `vieww_base/crates/…` | library |
| viewwstudio, wright | `vieww_base/apps/…` | desktop-app |
| wright-flow | `vieww_base/apps/wright/prototypes/wright-flow` | desktop-app |
| U | `vieww_base/examples/u` | mobile-app |
| mytlabs, viewwsite, wrightsite | `vieww_base/apps/…` | site |
| vavlt, snapsearch, three, upxcale | `apps/…` | mobile-app |

`ci/projects list` prints them with their jobs per stage.

A file belongs to the **deepest** project folder that contains it, so a new
crate under `vieww_base/crates/` with no ci.toml is checked as part of
vieww. Project ▸ **What this branch changed** (the default) picks the
projects that own a changed file since the branch left `main`, plus every
project that `uses` one of them. Changes under `ci/kinds/<kind>` pick every
project of that kind; other changes under `ci/` or `.github/` pick vieww
(whose `workflows` job lints all of this).

## Adding a crate or an app

```sh
ci/projects new vieww_base/crates/vieww-maps library     # or desktop-app, mobile-app, site
```

writes `ci.toml` from `ci/templates/<kind>.toml` and runs `ci/projects sync`,
which adds the project to the Project dropdowns. `viewwstudio-cli new <name>`
does the same for a new app inside this repository. Commit both. If a
dropdown is stale, every workflow's first job fails and says to run sync,
so a project cannot be left out by accident.

With no jobs of its own a project gets its **kind's** jobs
(`ci/kinds/<kind>.toml`):

| Kind | Gets |
|---|---|
| library | Check (quick): clippy `-D warnings` and `cargo test -p <crate>` |
| desktop-app | that check, and in Build a `cargo build --release` per platform (artifact `<name>-<platform>`) |
| mobile-app | an APK and an IPA from `[build] android = {…}` / `ios = {…}` (Build; Check thorough) |
| site | `build-site.sh` → `dist/` (Check, Build), and Release ▸ Publish deploys it to `[deploy] cloudflare_pages` |

A project's own jobs replace its kind's for their stage; `kind_jobs = false`
turns the kind's off, `kind_jobs = ["check"]` keeps only those.

## ci.toml

```toml
name = "wright"
kind = "desktop-app"                 # library | desktop-app | mobile-app | site
platforms = ["linux", "macos", "windows"]
uses = ["vieww"]                     # a change there also checks this

[[job]]
id = "installers"
title = "build"
file = "ci/jobs/installers.toml"     # the steps, relative to this folder
stage = "build"                      # check | devices | build | release | publish | housekeeping
also = ["check"]                     # other stages it runs in
depth = "thorough"                   # check: quick (default) or thorough
where = "machine"                    # check: only on "My Linux machine"
task = "goldens"                     # housekeeping: the task that runs it
phase = 2                            # 1–3; a phase starts when the one before has finished
needs = ["installers"]               # jobs of this project; needs.<id>.result works in steps
if = "inputs.publish"                # GitHub expression, evaluated on the job's machine
when = "form.linux"                  # planner flag: the job is planned only when set
runs_on = "${{ matrix.os }}"         # a label, a list, "machine" (FEATURE_RUNNER), or from the matrix
matrix = { include = [{ name = "linux-x64", os = "ubuntu-22.04" }] }
platforms_from = "matrix.name"       # Build's platform boxes filter rows by this prefix
expand = "ci/jobs/gate-legs.py"      # or: a script that prints matrix={"include": [...]}
timeout = 120
continue_on_error = false
main_only = true                     # devices that run from main only
write = true                         # may push / release / deploy (contents: write)
secrets = ["WRIGHT_SECRET_WINDOWS_PFX"]   # the only secrets this job sees
inputs = { version = "{form.version}", publish = "{release.publish}" }
checkout = { fetch-depth = 0, persist-credentials = false, clean = true, sparse-checkout = "…" }

[[job.setup]]                        # actions the engine runs before the steps
uses = "java"                        # java node gradle tools rust-cache cache aws emulator attest
with = { distribution = "temurin", java-version = "17" }
if = "matrix.mode == 'cpu'"

[[job.upload]]                       # up to 4; each fires where its marker step sits
name = "wright-${{ env.VERSION }}-${{ matrix.name }}"
path = "release/*"
if-no-files-found = "error"

[[job.download]]                     # up to 2
pattern = "wright-${{ env.VERSION }}-*"
path = "release"
merge-multiple = true
```

`inputs` are what the job's steps read as `${{ inputs.x }}`. A value that is
exactly one `{…}` keeps its type (a form checkbox stays a boolean). The
placeholders are `{form.<field>}` (the Run workflow form), `{release.publish}`
`{release.rerun}` `{release.legs}`, `{farm.platform}` `{farm.tiers}`,
`{suite.linux|windows|macos}` (the hardware steps for Devices ▸ what to run),
and anything in the manifest: `{name}` `{path}` `{workspace}` `{dir}`
`{package}` `{wasm_bindgen}` `{build.android.package}` …

## Job files

`ci/jobs/<id>.toml` is a workflow job's `steps:` without the `uses:` steps
(those are `[[job.setup]]`, uploads and downloads in ci.toml):

```toml
name = "Release gate"
[defaults]
shell = "bash"
working-directory = "vieww_base"
[env]
SCOPE = "${{ inputs.scope }}"

[[step]]
name = "Release gate"
id = "gate"
if = "env.SCOPE == 'gate'"
continue-on-error = false
timeout-minutes = 120
env = { ONLY = "${{ matrix.only }}" }
run = '''
ci/release/gate.sh
'''
```

`ci/lib/run.py` runs them with GitHub's meaning: the same shells (bash
with `-eo pipefail` when named, pwsh, powershell, python, cmd), `${{ }}`
with `inputs`, `matrix`, `env`, `github`, `runner`, `steps`, `needs`,
`secrets`, `vars`; `if:` with the implicit `success()`; `GITHUB_ENV`,
`GITHUB_PATH`, `GITHUB_OUTPUT`; `continue-on-error`. Each step is a
collapsible group in the log and the job summary ends with a step table.
Every job moved here from the old workflows kept its steps byte for byte.

## How a run works

```text
1-check.yml … 5-housekeeping.yml   the form
  └ _plan.yml      ci/projects check, then ci/projects plan <workflow>
  └ _job.yml ×3    phase 1, 2, 3: every planned job of that phase
  │                  run   — read-only token
  │                  write — may push / release / deploy; waits for the
  │                          `release` environment's approval when publishing
  └ _results.yml   files the run on the ci-results branch
```

`_job.yml` is generated (`ci/lib/make_engine.py`, run by `ci/projects sync`)
because it lists every secret a ci.toml names; each job sees only its own.
For each job it checks out, runs `ci/lib/prepare.py` (which evaluates the
job's `if`, needs and setup on that machine), the setup actions, the
downloads, the job file, and the uploads.

## Machines

| Label | Used by |
|---|---|
| `self-hosted, Linux, X64, vieww-linux` | Check on "My Linux machine" (or the `FEATURE_RUNNER` variable, JSON), Devices ▸ Linux |
| `vieww-windows`, `vieww-macos` | Devices ▸ Windows, Mac |
| `vieww-gate` (+ `gpu`) | Release ▸ include my machines |
| `android` | Devices ▸ Android phone on USB |

Self-hosted machines need `bash` (Git for Windows on Windows), `git`, and
Python 3.11 or newer as `python3`. GitHub's machines get Python from the
engine.

## Approval and secrets

Create the **`release`** environment (Settings → Environments) with yourself
as required reviewer: 4 · Release ▸ publish pauses there before anything is
published. Site deploys need `CLOUDFLARE_API_TOKEN` and
`CLOUDFLARE_ACCOUNT_ID`; the rest are the secrets the old workflows used.

## Commands

```sh
ci/projects list
ci/projects check                          # what every run's first job runs
ci/projects sync                           # after adding a project or a secret
FORM='{"project":"wright","depth":"thorough"}' ci/projects plan check --dry
python3 -m unittest discover -s ci/tests   # expressions, runner, planner
```
