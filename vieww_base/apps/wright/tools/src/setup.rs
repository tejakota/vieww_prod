//! The `wright doctor --fix` machinery (WR-14): an unattended bootstrap
//! for the Windows WSL 2 + Ubuntu + Bubblewrap sandbox stack, and the
//! honest isolation matrix doctor prints.
//!
//! WR-14's complaint was that the Windows sandbox was correct but had
//! the longest setup path in the comparison set: a wall of elevated
//! PowerShell before anything fires. The fix keeps the fail-closed
//! behavior and removes the wall:
//!
//! * **`wright doctor --fix`** runs the bootstrap script itself —
//!   elevation prompt, progress in the window, verification at the end.
//!   The script is *generated from this module* ([`bootstrap_ps1`]) and
//!   pinned byte-for-byte into `packaging/windows/bootstrap-wsl.ps1`
//!   by a test, so the shipped file and the code cannot drift.
//! * **The plan is data** ([`wsl_fix_plan`]): doctor prints what the
//!   fix will do before doing it, and CI checks the script against the
//!   same steps — the plan/report layer is pure and unit-tested on
//!   every platform, because no sandbox here has Windows.
//! * **The matrix** ([`matrix`]) is the honest output WR-14(c) asked
//!   for: for every class of command, where it runs on THIS machine —
//!   kernel sandbox, degraded native tier (WR-05's Job/AppContainer
//!   ladder, via [`crate::degraded`]), or refused — never a silent
//!   surprise.
//!
//! Honest limits: doctor can install the WSL 2 runtime, an Ubuntu
//! distribution and Bubblewrap; it does NOT install project toolchains
//! inside WSL (Rust/Node/Python there are the user's choice, as the
//! README has always said), and on Linux/macOS there is nothing doctor
//! can install — the kernel either has Landlock or it does not, and the
//! fix says so instead of pretending.
//!
//! This module is std-only and safe (the elevation relaunch lives in
//! the PowerShell script, not in an unsafe block).

/// One step of the bootstrap plan. `detail` is what the step runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixStep {
    pub name: &'static str,
    pub detail: &'static str,
}

/// The plan `wright doctor --fix` executes on Windows, in order. Pure
/// data: doctor reports it before running it, the script implements it,
/// and a test asserts the script carries every step.
pub fn wsl_fix_plan() -> Vec<FixStep> {
    vec![
        FixStep {
            name: "elevate",
            detail: "re-launch this script with the UAC prompt when not already administrator (the features need it)",
        },
        FixStep {
            name: "enable WSL 2",
            detail: "dism: Microsoft-Windows-Subsystem-Linux + VirtualMachinePlatform, no restart forced",
        },
        FixStep {
            name: "install Ubuntu",
            detail: "wsl --install -d Ubuntu --no-launch (unattended; the interactive first-run user creation can happen later)",
        },
        FixStep {
            name: "install bubblewrap",
            detail: "wsl -d Ubuntu -u root -- apt-get install -y bubblewrap util-linux coreutils",
        },
        FixStep {
            name: "verify",
            detail: "the same bwrap namespace probe Wright's sandbox runs, then `wright doctor` says WSL 2 + Bubblewrap",
        },
    ]
}

/// The bootstrap script, verbatim. This is the single source:
/// `packaging/windows/bootstrap-wsl.ps1` is this exact text (pinned by
/// `the_shipped_script_is_the_generated_one`), so editing one without
/// the other fails a test.
pub fn bootstrap_ps1() -> String {
    SCRIPT.to_string()
}

const SCRIPT: &str = r#"# Wright sandbox bootstrap (WR-14a): WSL 2 + Ubuntu + Bubblewrap.
#
# Generated from tools/src/setup.rs (setup::bootstrap_ps1()). Do not edit
# by hand -- regenerate the copy in packaging/windows/ from that source;
# a unit test pins the two together.
#
# What it does (in order): elevate, enable the WSL 2 features, install
# Ubuntu without launching it, apt-install the isolation utilities as
# root, then run the exact bwrap namespace probe Wright's sandbox uses.
# It does NOT install project toolchains inside WSL (Rust/Node/Python
# there are the user's choice) and never restarts the machine.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Write-Step([string]$name) {
    Write-Host ("[wright bootstrap] " + $name)
}

# --- 1. elevation (the features need an administrator) -------------------
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Step "re-launching elevated (answer the UAC prompt)"
    Start-Process -FilePath 'powershell.exe' `
        -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File', $PSCommandPath) `
        -Verb RunAs -Wait
    exit 0
}
Write-Step "already administrator"

# --- 2. the WSL 2 features (idempotent; no restart forced) ----------------
Write-Step "enabling Microsoft-Windows-Subsystem-Linux"
dism.exe /online /enable-feature /featurename:Microsoft-Windows-Subsystem-Linux /all /norestart | Out-Null
Write-Step "enabling VirtualMachinePlatform"
dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart | Out-Null
Write-Step "default WSL version 2"
wsl.exe --set-default-version 2 2>$null | Out-Null

# --- 3. an Ubuntu distribution, unattended -------------------------------
$distro = $null
try {
    $distro = @(wsl.exe --list --quiet) | Where-Object { $_ -match '\S' } | Select-Object -First 1
} catch { $distro = $null }
if ($distro) {
    Write-Step ("a distribution is already installed: " + $distro)
} else {
    Write-Step "installing Ubuntu (no first-run launch)"
    wsl.exe --install --distribution Ubuntu --no-launch
    if ($LASTEXITCODE -ne 0) {
        # Older WSL versions lack --no-launch: install plainly and keep
        # going; the apt step below still runs as root.
        Write-Step "this WSL has no --no-launch; installing plainly"
        wsl.exe --install --distribution Ubuntu
    }
}

# --- 4. the isolation utilities, as root ---------------------------------
Write-Step "apt-get update + bubblewrap util-linux coreutils (as root, unattended)"
wsl.exe --distribution Ubuntu --user root --exec sh -c "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y bubblewrap util-linux coreutils"
if ($LASTEXITCODE -ne 0) { throw "apt-get failed; run it by hand: wsl -d Ubuntu -u root -- apt-get install -y bubblewrap util-linux coreutils" }

# --- 5. verify with Wright's own probe -----------------------------------
Write-Step "verifying: the bwrap namespace probe Wright's sandbox runs"
wsl.exe --exec bwrap --die-with-parent --unshare-user --unshare-pid --unshare-ipc --unshare-uts --unshare-net --ro-bind / / --dev /dev --proc /proc -- /bin/true
if ($LASTEXITCODE -ne 0) { throw 'the bwrap probe failed; run wright doctor for the exact error' }

Write-Step "done: WSL 2 + Ubuntu + Bubblewrap ready"
Write-Host "WRIGHT-BOOTSTRAP-OK"
Write-Host "Next: finish Ubuntu's first-run user creation when you like, install the toolchains your projects need inside WSL, then run: wright doctor"
"#;

/// Run the fix (WR-14a). Windows: write the script to a private temp
/// file and run it — the script re-launches itself elevated when it
/// must, so a plain `wright doctor --fix` shows one UAC prompt and then
/// works unattended. Other operating systems: the honest sentence; the
/// sandbox there is a kernel capability doctor cannot install.
pub fn doctor_fix() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let dir = std::env::temp_dir().join(format!("wright-doctor-fix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let script = dir.join("bootstrap-wsl.ps1");
        std::fs::write(&script, bootstrap_ps1())
            .map_err(|e| format!("{}: {e}", script.display()))?;
        let out = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script.display().to_string(),
            ])
            .output()
            .map_err(|e| format!("could not start PowerShell: {e}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
        if out.status.success() {
            Ok(format!(
                "the bootstrap ran; doctor re-probes the sandbox below. {}",
                last_line(&text)
            ))
        } else {
            Err(format!(
                "the bootstrap failed (its own words, tail): {}",
                tail(&text, 1600)
            ))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "linux")]
        let why = "the Linux sandbox is the kernel's own Landlock (5.13+); that is not something doctor can install — update the kernel or enable CONFIG_LANDLOCK, then run `wright doctor` again";
        #[cfg(target_os = "macos")]
        let why = "the macOS sandbox is Seatbelt (/usr/bin/sandbox-exec), shipped with the OS; if doctor reports it missing the system install needs repair — wright cannot and will not substitute one";
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let why = "this platform has no sandbox backend at all; wright refuses commands rather than run them unsandboxed";
        Err(format!(
            "nothing to auto-fix on {}: {why}",
            std::env::consts::OS
        ))
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn last_line(text: &str) -> String {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("(no output)")
        .trim()
        .to_string()
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn tail(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let cut = t.len().saturating_sub(max);
    let mut start = cut;
    while !t.is_char_boundary(start) {
        start += 1;
    }
    t[start..].to_string()
}

// ── the honest matrix (WR-14c) ───────────────────────────────────

/// The operating system the matrix is describing — a field, not a
/// `cfg!`, so the unit tests can pin every OS's wording from any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
    Other,
}

/// What doctor knows about one machine's command isolation.
#[derive(Debug)]
pub struct Isolation {
    /// Which OS this machine is (drives the wording, not the honesty).
    pub os: Os,
    /// `sandbox::capabilities()` verbatim: the POSIX backend's state.
    pub posix: Result<String, String>,
    /// A Windows-native git.exe on PATH (the degraded git runner).
    pub native_git: Option<String>,
    /// The project's test command resolved to a Windows-native program
    /// (path), when it is an `.exe` (tier 2) or a `.cmd`/`.bat` shim
    /// (tier 1). `None`: not Windows-native, or no test command.
    pub native_tests: Option<String>,
}

impl Isolation {
    /// Probe this machine. `test_cmd` is the project's test command
    /// (empty when the project has none).
    pub fn probe(test_cmd: &str) -> Isolation {
        let os = match std::env::consts::OS {
            "windows" => Os::Windows,
            "linux" => Os::Linux,
            "macos" => Os::Macos,
            _ => Os::Other,
        };
        Isolation {
            os,
            posix: crate::sandbox::capabilities(),
            native_git: if os == Os::Windows {
                crate::degraded::resolve_native("git")
                    .filter(|(p, _)| p.ends_with(".exe"))
                    .map(|(p, _)| p.display().to_string())
            } else {
                None
            },
            native_tests: if os == Os::Windows && !test_cmd.is_empty() {
                crate::degraded::resolve_native(test_cmd.split_whitespace().next().unwrap_or(""))
                    .map(|(p, _)| p.display().to_string())
            } else {
                None
            },
        }
    }
}

/// The matrix rows doctor prints: (row, cell). Pure — the vocabulary is
/// pinned by tests, so the tier language cannot drift from what the
/// degraded runners actually report.
pub fn matrix(iso: &Isolation) -> Vec<(&'static str, String)> {
    let posix_ok = iso.posix.is_ok();
    let refused = || {
        iso.posix
            .as_ref()
            .map(|_| String::new())
            .unwrap_or_else(|e| format!("REFUSED ({e})"))
    };
    let fix_hint = if iso.os == Os::Windows {
        " — `wright doctor --fix` installs the WSL 2 stack"
    } else {
        ""
    };
    let backend = || {
        iso.posix
            .as_ref()
            .map(|_| match iso.os {
                Os::Windows => "kernel sandbox: WSL 2 + Bubblewrap".to_string(),
                Os::Linux => "kernel sandbox: Landlock + namespaces".to_string(),
                Os::Macos => "kernel sandbox: Seatbelt".to_string(),
                Os::Other => "kernel sandbox".to_string(),
            })
            .unwrap_or_else(|_| String::new())
    };
    let mut rows = Vec::new();
    rows.push((
        "shell",
        if posix_ok {
            backend()
        } else {
            format!("{}{fix_hint}", refused())
        },
    ));
    rows.push(("git", {
        if posix_ok {
            backend()
        } else if let Some(git) = &iso.native_git {
            format!(
                "DEGRADED, tier 2: {git} in the AppContainer (the project is its only writable tree; inside a Job object) — no WSL sandbox on this machine, reported per run{fix_hint}"
            )
        } else {
            format!("{}{fix_hint}", refused())
        }
    }));
    rows.push(("run-tests", {
        if posix_ok {
            backend()
        } else if let Some(test) = &iso.native_tests {
            let tier = if test.to_ascii_lowercase().ends_with(".exe") {
                "tier 2: AppContainer (the project is its only writable tree; inside a Job object)"
            } else {
                "tier 1: Job object (process tree, memory and process count bounded; NO filesystem sandbox — .cmd shims cannot be contained)"
            };
            format!("DEGRADED, {tier} — {test}, no WSL sandbox on this machine, reported per run{fix_hint}")
        } else {
            format!("{}{fix_hint}", refused())
        }
    }));
    if iso.os == Os::Windows {
        rows.push((
            "worker (.exe)",
            "tier 2: AppContainer (workspace copy; internet only; inside a Job object) — WR-05"
                .into(),
        ));
        rows.push((
            "worker (.cmd)",
            "tier 1: Job object (workspace copy; NO filesystem sandbox) — WR-05, reported per run"
                .into(),
        ));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped script is the generated one, byte for byte — the
    /// doc-drift discipline applied to WR-14's two copies.
    #[test]
    fn the_shipped_script_is_the_generated_one() {
        let shipped = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../packaging/windows/bootstrap-wsl.ps1"
        ))
        .unwrap_or_else(|e| panic!("packaging/windows/bootstrap-wsl.ps1: {e}"));
        assert_eq!(shipped, bootstrap_ps1());
    }

    /// Every plan step is IN the script — the plan is what runs, and a
    /// step that is planned but not executed (or the reverse) is a bug.
    #[test]
    fn the_script_carries_every_plan_step() {
        let script = bootstrap_ps1();
        for step in wsl_fix_plan() {
            assert!(
                script.contains(step.name) || script_matches(&script, step),
                "plan step '{}' is missing from the script",
                step.name
            );
        }
        // And the exact commands the plan names are the ones run.
        assert!(script.contains("Microsoft-Windows-Subsystem-Linux"));
        assert!(script.contains("VirtualMachinePlatform"));
        assert!(script.contains("--install --distribution Ubuntu --no-launch"));
        assert!(script.contains("apt-get install -y bubblewrap util-linux coreutils"));
        assert!(script.contains("--unshare-user"));
        assert!(script.contains("WRIGHT-BOOTSTRAP-OK"));
    }

    fn script_matches(script: &str, step: FixStep) -> bool {
        // Steps are named in the script's section headers; the detail
        // strings paraphrase, so match the operative words.
        let keys: &[&str] = match step.name {
            "elevate" => &["Verb RunAs"],
            "enable WSL 2" => &["enable-feature"],
            "install Ubuntu" => &["--distribution Ubuntu"],
            "install bubblewrap" => &["apt-get install -y bubblewrap"],
            "verify" => &["bwrap"],
            _ => &[],
        };
        keys.iter().all(|k| script.contains(k))
    }

    /// The plan is ordered and complete.
    #[test]
    fn the_plan_is_ordered_and_complete() {
        let plan = wsl_fix_plan();
        let names: Vec<_> = plan.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "elevate",
                "enable WSL 2",
                "install Ubuntu",
                "install bubblewrap",
                "verify"
            ]
        );
    }

    /// The matrix says the honest thing in every state (WR-14c): a
    /// machine with the sandbox, a machine degraded to native tiers,
    /// and a machine where everything refuses. The OS is a field, so
    /// every wording is pinnable from any host.
    #[test]
    fn the_matrix_names_every_state() {
        // A healthy Windows machine: everything on the POSIX backend.
        let ok = Isolation {
            os: Os::Windows,
            posix: Ok("WSL 2 + Bubblewrap".into()),
            native_git: None,
            native_tests: None,
        };
        let rows = matrix(&ok);
        assert_eq!(rows[0].0, "shell");
        assert!(rows[0].1.contains("WSL 2 + Bubblewrap"), "{}", rows[0].1);
        assert!(rows[1].1.contains("WSL 2 + Bubblewrap"));
        assert!(rows[2].1.contains("WSL 2 + Bubblewrap"));
        // The worker ladder is on the Windows matrix, in tier language.
        assert_eq!(rows[3].0, "worker (.exe)");
        assert!(rows[3].1.contains("tier 2"));
        assert!(rows[4].1.contains("tier 1"));

        // A healthy Linux machine says Landlock, not WSL.
        let linux = Isolation {
            os: Os::Linux,
            posix: Ok("Landlock ABI 4".into()),
            native_git: None,
            native_tests: None,
        };
        let rows = matrix(&linux);
        assert!(rows[0].1.contains("Landlock + namespaces"), "{}", rows[0].1);
        assert_eq!(rows.len(), 3, "no worker rows off Windows");

        // Degraded: no WSL, but git.exe and an .exe test command exist.
        let degraded = Isolation {
            os: Os::Windows,
            posix: Err("the WSL 2 + Bubblewrap runtime is missing".into()),
            native_git: Some(r"C:\Program Files\Git\cmd\git.exe".into()),
            native_tests: Some(r"C:\Users\u\.cargo\bin\cargo.exe".into()),
        };
        let rows = matrix(&degraded);
        assert!(rows[0].1.contains("REFUSED"), "{}", rows[0].1);
        assert!(
            rows[0].1.contains("doctor --fix"),
            "the refusal names the fix"
        );
        assert!(rows[1].1.contains("DEGRADED, tier 2"), "{}", rows[1].1);
        assert!(rows[1].1.contains("AppContainer"));
        assert!(rows[1].1.contains("only writable tree"));
        assert!(rows[2].1.contains("DEGRADED, tier 2"), "{}", rows[2].1);

        // A .cmd test shim degrades to tier 1 and SAYS there is no
        // filesystem sandbox — never silent.
        let shim = Isolation {
            os: Os::Windows,
            posix: Err("missing".into()),
            native_git: Some("git.exe".into()),
            native_tests: Some(r"C:\Program Files\nodejs\npm.cmd".into()),
        };
        let rows = matrix(&shim);
        assert!(rows[2].1.contains("tier 1"), "{}", rows[2].1);
        assert!(rows[2].1.contains("NO filesystem sandbox"), "{}", rows[2].1);

        // Nothing native at all: full refusal (the worker rows keep
        // their own tier ladder — workers never needed the POSIX
        // sandbox).
        let bare = Isolation {
            os: Os::Windows,
            posix: Err("the WSL 2 + Bubblewrap runtime is missing".into()),
            native_git: None,
            native_tests: None,
        };
        let rows = matrix(&bare);
        for row in rows.iter().take(3) {
            assert!(row.1.contains("REFUSED"), "{} must refuse", row.0);
        }
        assert!(rows[3].1.contains("tier 2"), "the worker ladder stands");
    }

    /// On non-Windows, `doctor_fix` refuses with the OS's own reason —
    /// it never pretends to install a kernel.
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn the_fix_is_honest_off_windows() {
        let err = doctor_fix().unwrap_err();
        assert!(err.contains("nothing to auto-fix"), "{err}");
        assert!(err.contains(std::env::consts::OS), "{err}");
    }

    /// The tail helpers bound what doctor shows.
    #[test]
    fn tails_are_bounded() {
        assert_eq!(tail("short", 8), "short");
        let long = "x".repeat(100);
        let cut = tail(&long, 10);
        assert!(cut.len() == 10, "{cut}");
        assert_eq!(last_line("a\nb\n\n"), "b");
    }
}
