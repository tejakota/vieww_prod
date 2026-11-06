# Everything a Windows PC with a real GPU and a desktop can close, in one
# unattended run. The Linux twin is `ci/hardware/linux.sh`; both write their
# results in the same shape (receipts, baseline reports, summary.txt).
#
#   pwsh -ExecutionPolicy Bypass -File ci\hardware\windows.ps1
#   pwsh -ExecutionPolicy Bypass -File ci\hardware\windows.ps1 -Steps baseline,stress
#
# Steps, and what each closes (PENDING.md / the audit's P0 list):
#   env       the machine, GPU, driver, power state and toolchain, recorded first
#   gpu       sec 1.1/sec 1.3  Vulkan suites, vieww-3d GPU parity and a GPU frame in a
#             real window, on this PC's own driver
#   d3d12     sec 1.3  the D3D12 suite on a HARDWARE adapter (CI only ever had
#             WARP). Correctness/bring-up only: the D3D12 backend does not
#             execute a ScenePlan yet, so there is no D3D12 frame to time
#   gallery   sec 1.1 sec 2.3 sec 4.1  CPU gallery + GPU census (Vulkan), 30 runs each,
#             with receipts and planner shadow logs
#   baseline  P0    a real window, launched N times on the CPU path and N on the
#             GPU path: launch-to-first-frame, working set, idle frames and
#             CPU, animated fps, frame-stage medians (examples/baseline.rs).
#             Energy is not measured on Windows and is reported as null
#   size      P0    size of that windowed app as shipped (`--profile dist`)
#   pipelines FW-05 the GPU pipeline-cache proof: the baseline app launched
#             twice against a cache this step owns -- the second launch
#             must start from the bytes the first saved and reach its
#             first frame sooner (the hit-rate a user gets from day two);
#             FW-16: the warm first frame must also clear the absolute
#             350 ms gate, and both receipts' startup census is asserted
#             (first-frame mark present, every stage carries a time)
#   stress    --     the animation stress suite five times: one reproducible p95
#   desktop   --     the on-desktop suite, through Git Bash
#   remote    sec 3.3  the studio's out-of-process preview tests
#   audio     --     vieww-audio on this PC's real sound card: WASAPI endpoint
#             lists, a soak on the default output under full-core load
#             (shared, then exclusive); with a loopback cable, every
#             frame checked both ways; with a MIDI cable, a WinMM round
#             trip. Environment: VIEWW_HW_AUDIO_SOAK (seconds, default
#             600), VIEWW_HW_AUDIO_GAIN (default 0 = silent but
#             streamed), VIEWW_HW_AUDIO_LOOP=1 (line out wired to line
#             in, both defaults), VIEWW_MIDI_LOOP='<out id>><in id>'
#
# Needs: the pinned Rust toolchain (MSVC), Visual Studio Build Tools, the GPU
# vendor's driver (it ships the Vulkan driver; the Vulkan SDK adds vulkaninfo),
# Git for Windows (bash, for `desktop`), Python 3 for the baseline summary.
# Run from an interactive desktop session: a GitHub runner installed as a
# Windows *service* runs in session 0 and cannot open a window -- start it with
# `run.cmd` from a logged-in desktop instead.
#
# Every step runs even when an earlier one failed. Exit status is non-zero if
# any step failed; a step this machine cannot run is SKIP, never PASS.
#
# # Why `pwsh` and not `powershell` in the usage above
#
# This file is deliberately ASCII-only, and that is a correctness rule rather
# than a style one.
#
# Windows PowerShell 5.1 reads a BOM-less script as the system ANSI codepage.
# A UTF-8 em-dash is the three bytes E2 80 94, and cp1252 reads them as three
# characters -- the last being 0x94, which is U+201D RIGHT DOUBLE QUOTATION
# MARK. PowerShell accepts smart quotes as string delimiters (so that code
# pasted out of Word still parses), so that byte CLOSES THE STRING. One
# em-dash inside a double-quoted Say line therefore ends the string early,
# turns the rest of the line into stray tokens, and takes brace tracking down
# with it -- which is the eight phantom "Missing closing '}'" errors the gate
# reported on 2026-11-06, none of them in a line that was wrong.
#
# A BOM fixes it, and the file carries one. But a BOM only has to be lost
# once -- by an editor, a copy-paste, a zip, a checkout filter -- for the
# whole file to stop parsing. ASCII cannot be lost. Keep it that way: no
# em-dashes, no section signs, no box drawing. `ci/hardware/linux.sh` has no
# such constraint and may keep its typography.

param(
  [string[]]$Steps = @('env','gpu','d3d12','gallery','baseline','size','pipelines','stress','desktop','remote','audio'),
  [string]$OutDir = '',
  [int]$Launches = 5
)

$ErrorActionPreference = 'Continue'
# `-File script.ps1 -Steps a,b` hands the script ONE string, "a,b" -- the array
# syntax only works when the script is invoked from inside PowerShell. Split
# either form the same way so both mean what they say.
$Steps = @($Steps | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $Root
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $OutDir) { $OutDir = Join-Path $Root "hardware-results\windows-$stamp" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$Summary = Join-Path $OutDir 'summary.txt'
$script:Failed = $false
$script:NoCargo = $false

function Say([string]$Line) {
  Write-Host $Line
  Add-Content -Path $Summary -Value $Line
}

function Skip-Step([string]$Name, [string]$Reason) { Say "SKIP  $Name ($Reason)" }

# Whether this run asked for $Step -- and whether this machine can attempt it.
#
# # Why the toolchain question lives here
#
# Every step but `env` shells out to cargo. When cargo was not on PATH, the
# run of 2026-11-06 produced twenty-five identical FAIL lines, one per
# Run-Step, each blaming a different suite for a problem none of them had --
# and the gate that reads summary.txt duly reported twenty-five defects.
#
# One question, asked once, in the one place every step passes through, turns
# that into one named SKIP per requested step against a single explanation at
# the top. It also honours the rule in this file's header: a step this machine
# cannot run is SKIP, never PASS.
function Want([string]$Step) {
  if (-not ($Steps -contains $Step)) { return $false }
  if ($script:NoCargo -and $Step -ne 'env') {
    Skip-Step $Step 'cargo is not on PATH -- see the toolchain line above'
    return $false
  }
  return $true
}

# Run a native command with optional extra environment variables, tee its
# output to NAME.log, and record PASS/FAIL by its exit code.
function Run-Step([string]$Name, [hashtable]$Env, [string]$Exe, [string[]]$ArgList) {
  # **A command that is not there did not fail -- it never ran.**
  #
  # Without this, `& $Exe` throws CommandNotFoundException, the native command
  # never sets $LASTEXITCODE, and the FAIL line below prints whatever that
  # variable happened to be holding: blank on the first step, and thereafter
  # the *previous* step's exit code, reported as this one's.
  if (-not (Get-Command $Exe -ErrorAction SilentlyContinue)) {
    Skip-Step $Name "$Exe is not on PATH"
    return $false
  }
  $log = Join-Path $OutDir "$Name.log"
  Write-Host ''
  Write-Host "==> ${Name}: $Exe $($ArgList -join ' ')"
  $saved = @{}
  if ($Env) {
    foreach ($key in $Env.Keys) {
      $saved[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
      [Environment]::SetEnvironmentVariable($key, $Env[$key], 'Process')
    }
  }
  $rc = $null
  try {
    # Cleared before the call so that a command which exits without setting it
    # cannot be reported with the previous step's number. $null is
    # distinguishable from 0 and is handled below.
    $global:LASTEXITCODE = $null
    # To the host, not the pipeline: anything left on the pipeline becomes
    # part of this function's return value, and `if (Run-Step ...)` would then
    # be true for any step that printed a line.
    # Not Tee-Object: in Windows PowerShell 5.1 it writes UTF-16, which grep
    # and every reader on the other side of the first real run (2026-10-15)
    # saw as a binary file. Out-File -Encoding utf8 behaves the same in 5.1
    # and 7.
    & $Exe @ArgList 2>&1 | ForEach-Object { "$_" } |
      ForEach-Object { Write-Host $_; $_ } |
      Out-File -FilePath $log -Encoding utf8
    $rc = $LASTEXITCODE
  } finally {
    foreach ($key in $saved.Keys) {
      [Environment]::SetEnvironmentVariable($key, $saved[$key], 'Process')
    }
  }
  if ($rc -eq 0) { Say "PASS  $Name"; return $true }
  if ($null -eq $rc) {
    Say "FAIL  $Name (no exit status -- the command did not run to completion; see $Name.log)"
  } else {
    Say "FAIL  $Name (exit $rc; see $Name.log)"
  }
  $script:Failed = $true
  return $false
}

# A Python that actually runs, or $null.
#
# # Why `Get-Command` on its own is the wrong question
#
# Windows ships an **App Execution Alias** at
# `%LOCALAPPDATA%\Microsoft\WindowsApps\python.exe`: a stub whose whole job is
# to print
#
#     Python was not found; run without arguments to install from the
#     Microsoft Store...
#
# and exit non-zero. It is a real file on PATH, so `Get-Command python`
# **succeeds** -- and the loop this replaced took that as the answer, set
# `python`, stopped looking, and never tried `py`. The first real run on a
# machine with the alias enabled therefore skipped `perf_budget.py` and
# `summarize_baseline.py` silently while printing the Store's advertisement
# into the middle of the gate's output.
#
# So this asks the only question that distinguishes a Python from a stub: it
# runs one. `py` first because the launcher is what a real install puts there,
# and because it is the one name the alias never takes.
function Find-Python {
  foreach ($candidate in 'py','python3','python') {
    if (-not (Get-Command $candidate -ErrorAction SilentlyContinue)) { continue }
    & $candidate -c 'import sys' 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { return $candidate }
  }
  return $null
}

# -- header ----------------------------------------------------------------
$commit = $null; $dirty = $null
if (Get-Command git -ErrorAction SilentlyContinue) {
  $commit = (git rev-parse HEAD 2>$null)
  $dirty = (git status --porcelain --untracked-files=no 2>$null)
}
# The first real run printed an empty commit line: git was not on this
# shell's PATH (Git for Windows installs it on the PATH only if asked).
if (-not $commit) { $commit = 'unknown (git not on PATH, or not a git checkout)' }
$pinned = ((Get-Content rust-toolchain.toml | Select-String '^channel').Line -split '"')[1]
# Guarded, like the `git` probe above it. `2>$null` redirects a native
# command's stderr, but a command that does not exist never gets to have
# stderr -- PowerShell raises CommandNotFoundException instead, and that is an
# error record, not a stream, so it printed in red above the report on
# 2026-11-06 and no redirection could have stopped it.
$rustc = $null
if (Get-Command rustc -ErrorAction SilentlyContinue) { $rustc = (rustc -V 2>$null) }
if (-not $rustc) { $rustc = 'NOT RESOLVED - no rustc on PATH, or rustup could not provide the pinned toolchain' }
Set-Content -Path $Summary -Value @(
  "vieww Windows hardware run $stamp",
  "commit: $commit$(if ($dirty) { ' (dirty)' })",
  "host: $env:COMPUTERNAME - $([Environment]::OSVersion.VersionString)",
  "cpu: $((Get-CimInstance Win32_Processor | Select-Object -First 1).Name)",
  "rustc: $rustc (pinned: $pinned)",
  ''
)

# ---- toolchain preflight -------------------------------------------------
#
# Asked once, before any step, because the answer decides all of them; Want()
# is what acts on it.
#
# A missing cargo is a misconfigured machine rather than a capability this one
# honestly lacks, so every step is recorded SKIP -- nothing may claim PASS --
# while the run as a whole still exits non-zero. A gate reporting success here
# would be reporting that nothing was tested.
$script:NoCargo = -not (Get-Command cargo -ErrorAction SilentlyContinue)
if ($script:NoCargo) {
  Say 'FAIL  toolchain (cargo is not on PATH; every step below needs it)'
  Say '      rustup installs cargo into %USERPROFILE%\.cargo\bin and puts that on'
  Say '      the *user* PATH. A GitHub runner registered as a Windows service runs'
  Say '      as another account, never sees it, and -- being in session 0 -- cannot'
  Say '      open a window either, which the gpu, baseline and desktop steps need.'
  Say '      Stop the service and start the runner with run.cmd from a logged-in'
  Say '      desktop; or, if Rust is installed for the service account, add that'
  Say '      directory to the runner PATH.'
  $script:Failed = $true
}

$hasVulkan = $false
$vulkanText = ''
if (Get-Command vulkaninfo -ErrorAction SilentlyContinue) {
  $vulkanText = (vulkaninfo --summary 2>&1 | Out-String)
  $hasVulkan = ($LASTEXITCODE -eq 0)
  Set-Content -Path (Join-Path $OutDir 'vulkaninfo.txt') -Value $vulkanText
} else {
  # No SDK: the loader may still be there (every GPU driver installs it). The
  # suites will say for themselves; assume present and let them fail loudly.
  $hasVulkan = Test-Path (Join-Path $env:SystemRoot 'System32\vulkan-1.dll')
}

if (Want 'env') {
  foreach ($gpu in Get-CimInstance Win32_VideoController) {
    Say "gpu: $($gpu.Name) (driver $($gpu.DriverVersion))"
  }
  $battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($battery) {
    if ($battery.BatteryStatus -eq 2) { Say 'power: on mains' }
    else { Say 'power: ON BATTERY - throttling and power saving will move every number' }
  } else { Say 'power: desktop (no battery)' }
  Say "power plan: $((powercfg /getactivescheme) -replace '^.*\((.*)\).*$','$1')"
  if ($vulkanText) {
    foreach ($m in ([regex]'(?m)^\s*deviceName\s*=\s*(.+)$').Matches($vulkanText)) {
      Say "vulkan: $($m.Groups[1].Value.Trim())"
    }
  } elseif ($hasVulkan) {
    Say 'vulkan: loader present (install the Vulkan SDK for vulkaninfo)'
  } else {
    Say 'vulkan: NO LOADER - the gpu, gallery and baseline steps will fail'
  }
  if ($rustc -like 'NOT RESOLVED*') {
    # Not "a different compiler" -- there is no compiler. Saying the wrong one
    # of these sends the reader looking for a toolchain mismatch that is not
    # the problem.
    Say "      WARNING: no rustc on PATH - nothing below can produce a number at all"
  } elseif ($rustc -notmatch " $([regex]::Escape($pinned)) ") {
    Say "      WARNING: rustc is not the pinned $pinned - numbers are for a different compiler"
  }
  Say '      energy: not measured on Windows (no package energy counter without a driver); reported as null'
}

# -- GPU (Vulkan on this PC's driver) ---------------------------------------
if (Want 'gpu') {
  if ($hasVulkan) {
    Run-Step 'gpu-hal' @{ VIEWW_REQUIRE_HARDWARE = 'gpu' } 'cargo' @('test','--release','-p','vieww-hal','--features','vulkan','--','--ignored','--test-threads=1') | Out-Null
    Run-Step 'gpu-3d' @{ VIEWW_REQUIRE_HARDWARE = 'gpu' } 'cargo' @('test','--release','-p','vieww-3d','--features','vulkan','--','--ignored','--test-threads=1') | Out-Null
    Run-Step 'gpu-window' @{ VIEWW_REQUIRE_HARDWARE = 'display,gpu' } 'cargo' @('test','--release','-p','vieww-platform-winit','--test','wait_loop') | Out-Null
  } else { Skip-Step 'gpu' 'no Vulkan loader' }
}

# -- D3D12 on a hardware adapter --------------------------------------------
if (Want 'd3d12') {
  # No VIEWW_D3D12_ALLOW_WARP: this must be the real adapter, which is the
  # whole difference from the hosted CI leg.
  Run-Step 'd3d12-hardware' @{ VIEWW_REQUIRE_HARDWARE = 'gpu'; VIEWW_D3D12_ALLOW_WARP = $null } 'cargo' @('test','--release','-p','vieww-hal','--features','d3d12','--','--ignored','--test-threads=1') | Out-Null
  Say '      d3d12: bring-up and smoke suite only - the backend does not execute a ScenePlan yet (PENDING 1.3)'
}

# -- gallery and census -----------------------------------------------------
if (Want 'gallery') {
  $fx = Join-Path $OutDir 'fixtures'
  $cs = Join-Path $OutDir 'census'
  Run-Step 'gallery' @{} 'cargo' @('run','--release','-p','fixtures','--',$fx,'--runs','30','--receipt',(Join-Path $fx 'receipt.json'),'--shadow',(Join-Path $fx 'shadow.jsonl')) | Out-Null
  Run-Step 'census' @{} 'cargo' @('run','--release','-p','fixtures','--',$cs,'--census','--runs','30','--receipt',(Join-Path $cs 'receipt.json'),'--shadow',(Join-Path $cs 'shadow.jsonl')) | Out-Null
  # FW-08: this machine's figures against ci/check/perf-budgets.json, or,
  # for a class with no committed budget, against its previous run (as
  # linux.sh does); this run's figures become the next run's baseline.
  $py = Find-Python
  $fxr = Join-Path $fx 'receipt.json'
  $csr = Join-Path $cs 'receipt.json'
  if ($py -and (Test-Path $fxr) -and (Test-Path $csr)) {
    $base = Join-Path $OutDir 'perf-baseline.json'
    & $py -I ci\check\perf_baseline.py hardware-self-hosted-linux-and-windows hardware-windows $base | Out-Host
    Run-Step 'perf-budget' @{} $py @('-I','ci\check\perf_budget.py','check','ci\check\perf-budgets.json',$fxr,$csr,'--baseline',$base,'--write-measured',(Join-Path $OutDir 'perf-measured.json')) | Out-Null
  } elseif (-not $py) {
    Skip-Step 'perf-budget' 'no working Python (the Store alias is not one; install from python.org or disable the alias)'
  }
  $receipt = Join-Path $cs 'receipt.json'
  if (Test-Path $receipt) {
    $r = Get-Content $receipt -Raw | ConvertFrom-Json
    $gpuName = if ($r.gpu) { "$($r.gpu.name) [$($r.gpu.device_type)]" } else { 'none' }
    Say "      census: gpu=$gpuName rendered_on_gpu=$($r.outcome.rendered_on_gpu)/$($r.outcome.fixtures) parity=$($r.outcome.parity_verdict)"
  }
}

# -- baseline: a real window, launched N times per path ---------------------
$python = Find-Python
if (Want 'baseline') {
  if (Run-Step 'baseline-build' @{} 'cargo' @('build','--release','-p','vieww-platform-winit','--example','baseline')) {
    $bin = Join-Path $Root 'target\release\examples\baseline.exe'
    $dir = Join-Path $OutDir 'baseline'
    if (-not (Test-Path $bin)) {
      Say "FAIL  baseline (built, but $bin is not there)"
      $script:Failed = $true
      $Launches = 0
    }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Say '      baseline start-up is warm-cache, and launch-to-first-frame includes PowerShell''s process creation (main-to-first-frame does not)'
    foreach ($mode in 'cpu','gpu') {
      $ok = $true
      for ($i = 1; $i -le $Launches; $i++) {
        $argList = @('--out', (Join-Path $dir "$mode-$i.json"))
        if ($mode -eq 'gpu') {
          $argList = @('--gpu') + $argList
          $env:VIEWW_PLANNER_SHADOW = (Join-Path $dir 'gpu-shadow.jsonl')
        } else {
          Remove-Item Env:\VIEWW_PLANNER_SHADOW -ErrorAction SilentlyContinue
        }
        # Unix nanoseconds from .NET ticks (100 ns since 0001-01-01).
        $env:VIEWW_BASELINE_LAUNCH_UNIX_NS = [string](([DateTime]::UtcNow.Ticks - 621355968000000000) * 100)
        $p = Start-Process -FilePath $bin -ArgumentList $argList -NoNewWindow -PassThru `
          -RedirectStandardOutput (Join-Path $OutDir "baseline-$mode-$i.out.log") `
          -RedirectStandardError (Join-Path $OutDir "baseline-$mode-$i.err.log")
        $null = $p.Handle # keeps ExitCode readable after exit
        if (-not $p.WaitForExit(120000)) {
          $p.Kill()
          Add-Content (Join-Path $OutDir "baseline-$mode-$i.err.log") 'baseline: killed after 120 s'
          $ok = $false
        } elseif ($p.ExitCode -ne 0) { $ok = $false }
      }
      Remove-Item Env:\VIEWW_PLANNER_SHADOW -ErrorAction SilentlyContinue
      Remove-Item Env:\VIEWW_BASELINE_LAUNCH_UNIX_NS -ErrorAction SilentlyContinue
      if ($Launches -eq 0) { continue }
      if ($ok) { Say "PASS  baseline-$mode ($Launches launches)" }
      else { Say "FAIL  baseline-$mode (see baseline-$mode-*.err.log)"; $script:Failed = $true }
    }
    if ($python) {
      $text = & $python (Join-Path $Root 'ci\hardware\summarize_baseline.py') $dir 2>&1 | ForEach-Object { "$_" }
      Set-Content -Path (Join-Path $dir 'summary.txt') -Value $text
      foreach ($line in $text) { Say "      $line" }
    } else { Say '      (no working Python: baseline/*.json are the raw reports; summarize_baseline.py was not run)' }
  }
}

# -- size of what ships -----------------------------------------------------
if (Want 'size') {
  # `dist` (Cargo.toml): LTO, stripped, dependencies at opt-level "s".
  if (Run-Step 'size-build' @{} 'cargo' @('build','--profile','dist','-p','vieww-platform-winit','--example','baseline','--target-dir','target\size')) {
    $exe = Join-Path $Root 'target\size\dist\examples\baseline.exe'
    if (Test-Path $exe) {
      $bytes = (Get-Item $exe).Length
      Set-Content -Path (Join-Path $OutDir 'size-baseline-bytes.txt') -Value $bytes
      Say ("      windowed baseline app, stripped release: {0} bytes ({1:N1} MB)" -f $bytes, ($bytes / 1MB))
    }
  }
}

# -- pipeline-cache hit-rate: the second launch must be cheaper -------------
# FW-05: the cache exists (vieww_hal::vulkan::pipeline_cache) and every GPU
# launch seeds from it, but nothing on Windows had ever *proved* the seed
# survives a process boundary. So this step owns a cache file -- deleted
# first, so run 1 is cold whatever ran before -- launches the baseline app
# twice, and reads both reports back: run 1 must start from 0 bytes, run 2
# from the bytes run 1 saved, and run 2's first frame must arrive sooner.
# The drop is what the Device Farm paid for starting it: 1463 ms on a Stylo 5
# first launch against 82 ms warm on an S26 Ultra.
if (Want 'pipelines') {
  if (-not $hasVulkan) {
    Skip-Step 'pipelines' 'no Vulkan loader'
  } elseif (Run-Step 'pipelines-build' @{} 'cargo' @('build','--release','-p','vieww-platform-winit','--example','baseline')) {
    $bin = Join-Path $Root 'target\release\examples\baseline.exe'
    if (-not (Test-Path $bin)) {
      Say "FAIL  pipelines (built, but $bin is not there)"
      $script:Failed = $true
    } else {
      $dir = Join-Path $OutDir 'pipelines'
      New-Item -ItemType Directory -Force -Path $dir | Out-Null
      $cache = Join-Path $dir 'pipelines.bin'
      Remove-Item $cache -ErrorAction SilentlyContinue
      $ok = $true
      $report = @{}
      foreach ($run in 1, 2) {
        # This step's own cache: run 1 cold by construction, run 2 warm.
        $env:VIEWW_PIPELINE_CACHE = $cache
        $out = Join-Path $dir "run-$run.json"
        $p = Start-Process -FilePath $bin -ArgumentList @('--gpu','--settle','1','--idle','1','--animate','1','--out',$out) `
          -NoNewWindow -PassThru `
          -RedirectStandardOutput (Join-Path $OutDir "pipelines-$run.out.log") `
          -RedirectStandardError (Join-Path $OutDir "pipelines-$run.err.log")
        $null = $p.Handle
        if (-not $p.WaitForExit(120000)) {
          $p.Kill()
          Add-Content (Join-Path $OutDir "pipelines-$run.err.log") 'pipelines: killed after 120 s'
          $ok = $false
        } elseif ($p.ExitCode -ne 0) { $ok = $false }
        if (Test-Path $out) {
          $report[$run] = Get-Content $out -Raw | ConvertFrom-Json
        } else { $ok = $false }
      }
      Remove-Item Env:\VIEWW_PIPELINE_CACHE -ErrorAction SilentlyContinue
      if ($ok -and $report.Count -eq 2) {
        $seed = @{}
        foreach ($run in 1, 2) {
          $text = [string]$report[$run].pipeline_cache
          $m = [regex]::Match($text, 'started from (\d+) bytes')
          if (-not $m.Success) {
            Say "FAIL  pipelines (run $run reported no pipeline_cache observation; is the GPU path up?)"
            $ok = $false
            break
          }
          $seed[$run] = [long]$m.Groups[1].Value
          # `${run}:` and not `$run:` -- in a double-quoted string PowerShell
          # reads `run:` as a drive/scope qualifier (the `$env:`/`$script:`
          # syntax), fails to find a `run:` drive, and rejects the whole file
          # with "Variable reference is not valid", taking brace tracking with
          # it and reporting eight phantom "Missing closing '}'" errors in
          # lines that are fine. `Run-Step` above already writes `${Name}:`.
          Say "      pipelines run ${run}: started from $($seed[$run]) bytes; first frame $($report[$run].main_to_first_frame_ms) ms"
          if ($report[$run].startup_stages) {
            Say "      pipelines run $run stages: $($report[$run].startup_stages)"
          }
        }
        # FW-16: the receipt's startup census is asserted, not just printed.
        # The first frame's mark must be there (a run that presented has one),
        # the census must have enough stages to be one, and every stage must
        # carry a time -- a malformed line is a broken receipt, not a style
        # problem, and every later gate reads this receipt.
        if ($ok) {
          $stages = [string]$report[2].startup_stages
          if ([string]::IsNullOrWhiteSpace($stages)) {
            Say 'FAIL  pipelines (run 2 reported no startup_stages -- the receipt must carry the startup census)'
            $ok = $false
          } else {
            if (-not ($stages.Contains('first raster and present'))) {
              Say 'FAIL  pipelines (run 2 startup_stages has no "first raster and present" mark -- did the first frame actually present?)'
              $ok = $false
            }
            $segs = $stages.Split('|')
            if ($segs.Count -lt 5) {
              Say "FAIL  pipelines (run 2 startup_stages carries only $($segs.Count) segment(s) -- the census is incomplete)"
              $ok = $false
            }
            foreach ($seg in $segs) {
              if ($seg -match '=' -or $seg -match 'before run unknown') { continue }
              if (-not ($seg -match '\d')) {
                Say "FAIL  pipelines (run 2 startup stage carries no time: `"$seg`")"
                $ok = $false
              }
            }
          }
        }
        if ($ok) {
          if ($seed[1] -ne 0) {
            Say "FAIL  pipelines (run 1 started from $($seed[1]) bytes -- the cache was not cold; this step's own setup failed)"
            $script:Failed = $true
          } elseif ($seed[2] -le 0) {
            Say 'FAIL  pipelines (run 2 started from 0 bytes -- what run 1 saved did not seed it: cache not written, or the header no longer matches this GPU/driver)'
            $script:Failed = $true
          } elseif ($report[2].main_to_first_frame_ms -ge $report[1].main_to_first_frame_ms) {
            Say ("FAIL  pipelines (warm first frame {0:N1} ms is not under the cold {1:N1} ms)" -f $report[2].main_to_first_frame_ms, $report[1].main_to_first_frame_ms)
            $script:Failed = $true
          } elseif ($report[2].main_to_first_frame_ms -gt 350) {
            # FW-16's absolute gate: the warm cache may not merely be faster
            # than this machine's cold start -- a person still waited for it.
            # 350 ms is the receipt-based budget the report names; the relative
            # win above does not excuse the absolute number.
            Say ("FAIL  pipelines (warm first frame {0:N1} ms is over FW-16's 350 ms gate)" -f $report[2].main_to_first_frame_ms)
            $script:Failed = $true
          } else {
            Say ("PASS  pipelines (cold {0:N1} ms -> warm {1:N1} ms, under the 350 ms gate, seeded from {2} bytes)" -f $report[1].main_to_first_frame_ms, $report[2].main_to_first_frame_ms, $seed[2])
          }
        } else { $script:Failed = $true }
      } else {
        Say 'FAIL  pipelines (a launch failed or wrote no report; see pipelines-*.err.log)'
        $script:Failed = $true
      }
    }
  } else {
    Say 'FAIL  pipelines (pipelines-build failed)'
    $script:Failed = $true
  }
}

# -- animation stress, five times -------------------------------------------
if (Want 'stress') {
  $p95s = @()
  for ($i = 1; $i -le 5; $i++) {
    $dir = Join-Path $OutDir "stress-$i"
    Run-Step "stress-$i" @{} 'cargo' @('run','--release','-p','test-animation-stress','--',$dir) | Out-Null
    $metrics = Join-Path $dir 'metrics.txt'
    if (Test-Path $metrics) {
      $line = Get-Content $metrics | Where-Object { $_ -like 'render_p95_ms=*' } | Select-Object -First 1
      if ($line) { $p95s += [double]($line -split '=')[1] }
    }
  }
  if ($p95s.Count -gt 0) {
    Say "      animation stress p95 over $($p95s.Count) runs (ms): $(($p95s | Sort-Object) -join ' ')"
  }
}

# -- the on-desktop suite ---------------------------------------------------
# Git Bash, found the way Git for Windows actually installs it: only
# `git.exe` (from Git\cmd) goes on PATH by default, while bash.exe is in
# Git\bin -- so "git is installed" and "bash is not on PATH" are both true on
# a normal machine, which is what the first real run hit. Never
# System32\bash.exe: that is WSL's, and it would run the suite in Linux.
function Find-GitBash {
  $candidates = @()
  $git = Get-Command git -ErrorAction SilentlyContinue
  if ($git) {
    $dir = Split-Path -Parent $git.Source           # ...\Git\cmd or ...\Git\mingw64\bin
    $candidates += (Join-Path $dir '..\bin\bash.exe')
    $candidates += (Join-Path $dir '..\..\bin\bash.exe')
  }
  $roots = @($env:ProgramFiles, ${env:ProgramFiles(x86)})
  if ($env:LOCALAPPDATA) { $roots += (Join-Path $env:LOCALAPPDATA 'Programs') }
  foreach ($root in $roots) {
    if ($root) { $candidates += (Join-Path $root 'Git\bin\bash.exe') }
  }
  $onPath = Get-Command bash -ErrorAction SilentlyContinue
  if ($onPath -and $onPath.Source -notmatch '[\\/]System32[\\/]') { $candidates += $onPath.Source }
  foreach ($c in $candidates) {
    if ($c -and (Test-Path $c)) { return (Resolve-Path $c).Path }
  }
  return $null
}

if (Want 'desktop') {
  $bash = Find-GitBash
  if ($bash) {
    Say "      desktop: using $bash"
    Run-Step 'desktop' @{} $bash @('ci/certify/desktop-suite.sh','--log',(Join-Path $OutDir 'desktop-app.log')) | Out-Null
  } else { Skip-Step 'desktop' 'Git Bash not found (looked next to git.exe and in Program Files\Git\bin)' }
}

# -- the studio's out-of-process preview ------------------------------------
if (Want 'remote') {
  Run-Step 'remote' @{} 'cargo' @('test','-p','viewwstudio','--test','remote','--','--test-threads=1') | Out-Null
}

# -- audio on the real sound card -------------------------------------------
if (Want 'audio') {
  $soak = if ($env:VIEWW_HW_AUDIO_SOAK) { $env:VIEWW_HW_AUDIO_SOAK } else { '600' }
  $gain = if ($env:VIEWW_HW_AUDIO_GAIN) { $env:VIEWW_HW_AUDIO_GAIN } else { '0' }
  $endpoints = @(Get-PnpDevice -Class AudioEndpoint -Status OK -ErrorAction SilentlyContinue)
  if ($endpoints.Count -eq 0) {
    Skip-Step 'audio' 'no active audio endpoint'
  } else {
    Say ("      audio endpoints: " + (($endpoints | ForEach-Object { $_.FriendlyName }) -join '; '))
    Run-Step 'audio-devices' @{} 'cargo' @('test','--release','-p','vieww-audio','--lib','backend::wasapi','--','--nocapture') | Out-Null
    # AU-01: the registered player (EnginePlayer) on this machine's output.
    Run-Step 'audio-player' @{ VIEWW_LIVE_GAIN = $gain } 'cargo' @('test','--release','-p','vieww-audio','--test','player_smoke','--','--ignored','--nocapture') | Out-Null
    # AU-10: IAudioClient3 low-latency shared mode on this machine's real
    # endpoint: under 8 ms, and the soak with no late render.
    Run-Step 'audio-low-latency-shared' @{ VIEWW_SOAK_SECS = $soak } 'cargo' @('test','--release','-p','vieww-audio','--lib','low_latency_shared_on_a_real_endpoint','--','--ignored','--nocapture') | Out-Null
    # AU-09: the default endpoint flipped mid-stream (IPolicyConfig), with
    # the notification heard within 100 ms and one reopen. Needs a second
    # render endpoint (VB-CABLE beside the speakers will do).
    $renders = @($endpoints | Where-Object { $_.FriendlyName -notmatch 'Microphone|Line In|CABLE Output|Stereo Mix' })
    if ($renders.Count -ge 2) {
      Run-Step 'audio-default-flip' @{} 'cargo' @('test','--release','-p','vieww-audio','--lib','a_default_endpoint_flip_is_followed_at_once','--','--ignored','--nocapture') | Out-Null
    } else {
      Skip-Step 'audio-default-flip' 'one render endpoint (install VB-CABLE for a second)'
    }
    foreach ($b in @('wasapi','wasapi-exclusive')) {
      Run-Step "audio-soak-$b" @{ VIEWW_REQUIRE_LOOPBACK = '1'; VIEWW_SOAK_BACKEND = $b; VIEWW_SOAK_SECS = $soak; VIEWW_LIVE_GAIN = $gain } 'cargo' @('test','--release','-p','vieww-audio','--test','device_loopback','default_output_soak','--','--ignored','--nocapture') | Out-Null
    }
    if ($env:VIEWW_HW_AUDIO_LOOP) {
      foreach ($b in @('wasapi','wasapi-exclusive')) {
        Run-Step "audio-loopback-$b" @{ VIEWW_REQUIRE_LOOPBACK = '1'; VIEWW_LOOP_BACKEND = $b } 'cargo' @('test','--release','-p','vieww-audio','--test','device_loopback','default_device','--','--ignored','--nocapture') | Out-Null
      }
    } else {
      Skip-Step 'audio-loopback' 'VIEWW_HW_AUDIO_LOOP not set (needs a line-out to line-in cable)'
    }
    if ($env:VIEWW_MIDI_LOOP) {
      Run-Step 'audio-midi' @{ VIEWW_REQUIRE_LOOPBACK = '1' } 'cargo' @('test','--release','-p','vieww-audio','--test','device_loopback','midi_port','--','--ignored','--nocapture') | Out-Null
    } else {
      Skip-Step 'audio-midi' 'VIEWW_MIDI_LOOP not set (needs a MIDI cable between two ports)'
    }
  }
}

# -- summary ----------------------------------------------------------------
#
# **After every step, which it was not.** This block sat above the `audio`
# section, so `Get-Content $Summary` ran before a single audio step had, and
# the printed summary ended at `remote` on every run that included audio. The
# lines were in summary.txt -- `Say` had written them -- just not in the output
# anybody actually reads. Nothing below this line may run a step.
Write-Host ''
Write-Host '================ summary ================'
Get-Content $Summary
Write-Host ''
Write-Host "results: $OutDir"

if ($script:Failed) { exit 1 } else { exit 0 }
