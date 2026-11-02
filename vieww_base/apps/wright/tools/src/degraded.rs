//! The degraded native-Windows runners (WR-14b).
//!
//! Wright's Windows sandbox for `shell`, `git` and `run-tests` is WSL 2 +
//! Bubblewrap ([`crate::sandbox_windows`]) and fails closed when that
//! runtime is missing — the correct default. WR-14's honest middle
//! ground: when the POSIX sandbox is absent but the tool's program is
//! a *Windows-native binary*, the command can still run under WR-05's
//! own containment ladder instead of being refused outright:
//!
//! * **tier 2** — a native `.exe` (git.exe, cargo.exe) runs in the
//!   `wright.worker` AppContainer with THE PROJECT as its only writable
//!   tree, inside a Job object: process tree, memory and process count
//!   bounded, user profile unreadable, the program's install folders
//!   readable for the length of the run. The one capability is
//!   `internetClient` (the AppContainer machinery's only mode) — unused
//!   by the local git subcommands and offline test runs that get here.
//! * **tier 1** — a `.cmd`/`.bat` shim (npm's CLIs) cannot be contained
//!   (cmd.exe's parser must not be handed a container), so it gets a
//!   Job object and a scrubbed environment only: bounded and mortal,
//!   but NO filesystem sandbox.
//!
//! `shell` never takes this path: an arbitrary command is exactly what
//! the kernel sandbox exists for, and a Job object is not an answer to
//! it. The POSIX sandbox stays load-bearing for everything else.
//!
//! Every degraded run REPORTS its tier in the tool's own output (and
//! therefore in the run's receipt), using the same tier language as
//! the worker tool (WR-05) and `wright doctor`'s matrix
//! ([`crate::setup`]) — a vocabulary test pins all three together.
//!
//! Honest limits: no shell syntax is interpreted in degraded mode (the
//! test command is split on whitespace, and operators like `&&` are
//! refused rather than half-run); the environment is the scrubbed base
//! plus what the tool passes (git's identity and config isolation),
//! with POSIX null paths translated to `nul`; and this is a fallback
//! only — `wright doctor --fix` exists precisely so nobody stays here.

use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::time::Duration;

#[cfg(target_os = "windows")]
use crate::sandbox::Policy;
#[cfg(target_os = "windows")]
use crate::sandbox::RunOutput;
use crate::sandbox::MAX_OUTPUT;

/// WR-14b tier 2: a native `.exe` in the AppContainer, project-writable.
pub const TIER2_APPCONTAINER: &str = "tier 2: AppContainer (the project is its only writable tree; process tree, memory and process-count bounded; inside a Job object). DEGRADED: this ran without the WSL 2 + Bubblewrap sandbox — `wright doctor --fix` installs it";

/// WR-14b tier 1: a `.cmd`/`.bat` shim in a Job object only — the
/// container cannot hold cmd.exe's parser.
pub const TIER1_JOB_OBJECT: &str = "tier 1: Job object (process tree, memory and process-count bounded; NO filesystem sandbox — .cmd shims cannot be contained). DEGRADED: this ran without the WSL 2 + Bubblewrap sandbox — `wright doctor --fix` installs it";

/// Resolve `command` (a program name or path) to the binary a native
/// run would use, and whether it is a Windows PE (`.exe`, containable
/// at tier 2) or a shim (`.cmd`/`.bat`, tier 1 at best). `None` when it
/// is not installed.
pub fn resolve_native(command: &str) -> Option<(PathBuf, bool)> {
    if command.is_empty() || command.contains('/') || command.contains('\\') {
        let p = PathBuf::from(command);
        let pe = is_pe(&p);
        return if p.is_file() { Some((p, pe)) } else { None };
    }
    let exts: Vec<String> = if cfg!(target_os = "windows") {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{command}{ext}"));
            let pe = is_pe(&candidate);
            if candidate.is_file() {
                return Some((candidate, pe));
            }
        }
    }
    None
}

fn is_pe(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("exe"))
        .unwrap_or(false)
}

/// True when `command` carries shell syntax the degraded runner refuses
/// to guess at (it runs the program directly, not through a shell):
/// operators, quotes, or expansions — anything a shell would interpret.
pub fn has_shell_syntax(command: &str) -> bool {
    command.contains(['$', '`', '"', '\''])
        || command
            .split_whitespace()
            .any(|w| w.contains(['&', '|', ';', '<', '>', '(', ')']))
}

/// Run one native command degraded (Windows only). `pol` carries the
/// workdir (the project — the only writable tree), the timeout and the
/// tool's environment additions. Returns the run plus the tier it ran
/// at; the CALLER must put that tier in its report.
#[cfg(target_os = "windows")]
pub fn run_native(
    program: &Path,
    args: &[String],
    pol: &Policy,
) -> Result<(RunOutput, &'static str), String> {
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    if is_pe(program) {
        contained(program, &arg_refs, pol)
    } else {
        jobbed(program, &arg_refs, pol)
    }
}

/// The tool environment for a degraded run: the scrubbed base every
/// sandboxed command gets (mirroring worker's tier ladder), plus the
/// tool's own additions, with POSIX null paths translated.
#[cfg(target_os = "windows")]
fn native_env(pol: &Policy, scratch: &Path) -> Vec<(String, String)> {
    let home = scratch.display().to_string();
    let mut env: Vec<(String, String)> =
        ["PATH", "SystemRoot", "PATHEXT", "COMSPEC", "LOCALAPPDATA"]
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect();
    for k in ["HOME", "USERPROFILE", "APPDATA", "TEMP", "TMP", "TMPDIR"] {
        env.push((k.to_string(), home.clone()));
    }
    for (k, v) in &pol.env {
        // git's POSIX null paths mean "no config from there"; Windows
        // git wants the null device.
        let v = if v == "/dev/null" {
            "nul".to_string()
        } else {
            v.clone()
        };
        env.push((k.clone(), v));
    }
    env
}

/// A scratch directory inside the project (the only writable tree at
/// tier 2), removed when the run ends.
#[cfg(target_os = "windows")]
struct Scratch(PathBuf);

#[cfg(target_os = "windows")]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Tier 2: the AppContainer with the project as its only writable tree.
#[cfg(target_os = "windows")]
fn contained(
    program: &Path,
    args: &[&str],
    pol: &Policy,
) -> Result<(RunOutput, &'static str), String> {
    let scratch = Scratch(pol.workdir.join(".wright-native"));
    std::fs::create_dir_all(&scratch.0).map_err(|e| format!("{}: {e}", scratch.0.display()))?;
    let job = crate::winjob::Job::new(
        crate::winjob::WORKER_MAX_PROCESSES,
        Some(crate::winjob::WORKER_MAX_MEMORY),
    )
    .map_err(|e| format!("could not create a Job object: {e}"))?;
    // The program's own install folders are readable for the run (the
    // same grant the worker's tier 2 makes).
    let readable = program_dirs(program);
    let env = native_env(pol, &scratch.0);
    let out = crate::appcontainer::run(
        program,
        args,
        &pol.workdir,
        &readable,
        &env,
        &job,
        pol.timeout.max(Duration::from_secs(1)),
        &wright_runtime::killswitch::is_stopped,
    )
    .map_err(|e| format!("the AppContainer run failed: {e}"))?;
    if out.stopped {
        return Err("the run was stopped by the kill switch".into());
    }
    Ok((
        RunOutput {
            code: Some(out.exit_code as i32),
            output: clip(&out.output),
            timed_out: out.timed_out,
            truncated: out.output.len() > MAX_OUTPUT,
        },
        TIER2_APPCONTAINER,
    ))
}

/// Tier 1: a Job object and a scrubbed environment, nothing more.
#[cfg(target_os = "windows")]
fn jobbed(
    program: &Path,
    args: &[&str],
    pol: &Policy,
) -> Result<(RunOutput, &'static str), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let scratch = Scratch(pol.workdir.join(".wright-native"));
    std::fs::create_dir_all(&scratch.0).map_err(|e| format!("{}: {e}", scratch.0.display()))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(&pol.workdir)
        .env_clear()
        .envs(native_env(pol, &scratch.0))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW, and suspended until the Job object holds it.
        cmd.creation_flags(0x0800_0000 | crate::winjob::CREATE_SUSPENDED);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))?;
    let job = crate::winjob::Job::new(
        crate::winjob::WORKER_MAX_PROCESSES,
        Some(crate::winjob::WORKER_MAX_MEMORY),
    )
    .and_then(|j| j.adopt(&mut child).map(|()| j))
    .map_err(|e| {
        let _ = child.kill();
        format!("could not contain the command in a Job object: {e}")
    })?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut keep = Vec::new();
            if let Some(mut r) = pipe {
                let mut buf = [0u8; 8192];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    keep.extend_from_slice(&buf[..n]);
                    // Only the tail is ever shown; memory stays bounded.
                    if keep.len() > 256 * 1024 {
                        keep.drain(..keep.len() - 128 * 1024);
                    }
                }
            }
            keep
        })
    };
    let out_thread = drain(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err_thread = drain(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let started = std::time::Instant::now();
    let mut timed_out = false;
    let mut stopped = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if wright_runtime::killswitch::is_stopped() {
                    stopped = true;
                } else if started.elapsed() > pol.timeout {
                    timed_out = true;
                } else {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                job.kill_all();
                break child
                    .wait()
                    .map_err(|e| format!("wait for the command: {e}"))?;
            }
            Err(e) => return Err(format!("wait for the command: {e}")),
        }
    };
    let mut output = String::from_utf8_lossy(&out_thread.join().unwrap_or_default()).into_owned();
    let err = String::from_utf8_lossy(&err_thread.join().unwrap_or_default()).into_owned();
    output.push_str(&err);
    if stopped {
        return Err("the run was stopped by the kill switch".into());
    }
    Ok((
        RunOutput {
            code: status.code(),
            output: clip(&output),
            timed_out,
            truncated: output.len() > MAX_OUTPUT,
        },
        TIER1_JOB_OBJECT,
    ))
}

/// The directories a program needs readable to start (the worker's
/// tier-2 grant list, mirrored).
#[cfg(target_os = "windows")]
fn program_dirs(program: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = program.parent() {
        dirs.push(d.to_path_buf());
    }
    if let Ok(real) = std::fs::canonicalize(program) {
        if let Some(d) = real.parent() {
            dirs.push(d.to_path_buf());
            if let Some(pkg) = d.parent() {
                if pkg.parent().is_some() {
                    dirs.push(pkg.to_path_buf());
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

// The tail-keepers: used by the Windows fix path and the tests.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn clip(s: &str) -> String {
    if s.len() <= MAX_OUTPUT {
        return s.to_string();
    }
    let mut start = s.len() - MAX_OUTPUT;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tier vocabulary: the degraded runners, the worker ladder and
    /// doctor's matrix all speak the same numbers — never silent.
    #[test]
    fn the_degraded_tiers_are_named_not_silent() {
        assert!(
            TIER2_APPCONTAINER.contains("tier 2"),
            "{TIER2_APPCONTAINER}"
        );
        assert!(
            TIER2_APPCONTAINER.contains("only writable tree"),
            "the filesystem wall is stated"
        );
        assert!(
            TIER2_APPCONTAINER.contains("DEGRADED"),
            "the word is DEGRADED"
        );
        assert!(
            TIER2_APPCONTAINER.contains("doctor --fix"),
            "the fix is named"
        );
        assert!(TIER1_JOB_OBJECT.contains("tier 1"), "{TIER1_JOB_OBJECT}");
        assert!(
            TIER1_JOB_OBJECT.contains("NO filesystem sandbox"),
            "tier 1 admits it has no filesystem sandbox"
        );
        assert!(TIER1_JOB_OBJECT.contains("DEGRADED"));
        // And the worker's own ladder agrees on the numbers.
        assert!(crate::worker::TIER2_APPCONTAINER.contains("tier 2"));
        assert!(crate::worker::TIER1_JOB_OBJECT.contains("tier 1"));
    }

    /// Resolution marks PEs as containable and shims as not.
    #[test]
    fn resolution_marks_pe_vs_shim() {
        let (git, pe) = resolve_native("git").expect("git is on PATH for the tests");
        if cfg!(target_os = "windows") {
            assert_eq!(pe, git.extension().map(|e| e == "exe").unwrap_or(false));
        } else {
            assert!(!pe, "off Windows nothing is a PE");
        }
        assert!(resolve_native("no-such-tool-wright-xyz").is_none());
        // A path form resolves only when the file exists.
        assert!(resolve_native("/no/such/tool/wright-xyz.exe").is_none());
    }

    /// Shell syntax is refused, not guessed at.
    #[test]
    fn shell_syntax_is_refused_not_guessed() {
        assert!(!has_shell_syntax("cargo test"));
        assert!(has_shell_syntax("cargo test && echo done"));
        assert!(has_shell_syntax("npm test | tee log"));
        assert!(has_shell_syntax("sh -c 'x'; y"));
        assert!(has_shell_syntax("make > log"));
    }

    /// Clipping keeps UTF-8 boundaries.
    #[test]
    fn clipping_is_boundary_safe() {
        let s = "🙂".repeat(MAX_OUTPUT / 4 + 4);
        let c = clip(&s);
        assert!(c.len() <= MAX_OUTPUT);
        assert!(c.is_char_boundary(c.len()));
        assert!(c.ends_with("🙂"));
    }
}
