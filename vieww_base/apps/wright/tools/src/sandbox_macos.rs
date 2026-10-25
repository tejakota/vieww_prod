//! macOS command isolation through Seatbelt (WR-03).
//!
//! The same policy as the Linux backend, expressed as a Seatbelt (SBPL)
//! profile and applied by `/usr/bin/sandbox-exec`:
//!
//! * **Writes** only inside the project, the extra writable roots and a
//!   private 0700 scratch HOME/TMPDIR that is deleted afterwards.
//! * **Reads**: the system is readable (toolchains live in `/usr`,
//!   `/Library/Developer`, Homebrew…), but **every user home is hidden**
//!   (`/Users`), Wright's own store with it, except the project, the scratch,
//!   the rustup/cargo toolchain directories and `PATH` entries that live
//!   under the home (each entry itself, never its parent — WR-01's lesson).
//! * **No network** unless the agent holds the network permission
//!   (`network*` denied; local Unix sockets included).
//! * **No keychain**: the Security framework's Mach services are denied, so
//!   generated code cannot read secrets through `security` or the API.
//! * **No reach into other processes**: signals and process-info are allowed
//!   only within the command's own sandbox.
//! * A scrubbed environment, its own process group (killed as a group on
//!   timeout and by the kill switch), core dumps off.
//!
//! In SBPL the last matching rule wins, which is what lets "deny all of
//! /Users" be followed by "allow these subpaths". Every path reaches the
//! profile as a `-D` parameter, never spliced into its text, so no path can
//! inject policy. `sandbox-exec` is marked deprecated in its man page but is
//! the documented front end to the same `sandbox_init` kernel interface
//! Chrome and Firefox use; `capabilities()` probes it on every run.
//!
//! If Seatbelt is unavailable the command is refused — never run unsandboxed.

// In Linux test builds only the profile generator is exercised.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::{Policy, RunOutput, MAX_OUTPUT};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
const SIGKILL: i32 = 9;

/// Is Seatbelt usable here?
pub fn capabilities() -> Result<String, String> {
    if !Path::new(SANDBOX_EXEC).exists() {
        return Err(format!(
            "{SANDBOX_EXEC} is missing; shell, git and run-tests will refuse"
        ));
    }
    let ok = Command::new(SANDBOX_EXEC)
        .args([
            "-p",
            "(version 1)(allow default)(deny network*)",
            "/usr/bin/true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Ok("Seatbelt (sandbox-exec): writes confined to the project, homes hidden, network and keychain denied".into())
    } else {
        Err("Seatbelt refused a trivial profile; shell, git and run-tests will refuse".into())
    }
}

/// The real path: Seatbelt matches resolved paths (`/tmp` is
/// `/private/tmp`, `$TMPDIR` is under `/private/var`).
fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `PATH` entries that live under the home — each entry itself, never its
/// parent, and never the home or an ancestor of it.
pub(super) fn home_path_entries(path: &str, home: &Path) -> Vec<PathBuf> {
    path.split(':')
        .filter(|d| d.starts_with('/'))
        .map(PathBuf::from)
        .filter(|d| d.starts_with(home) && d != home)
        .collect()
}

/// The profile, with `(param "…")` references for every path.
pub(super) fn profile(n_rw: usize, n_ro: usize, allow_network: bool) -> String {
    let mut p = String::from(
        "(version 1)\n\
         (allow default)\n\
         ; Writes: nowhere, then the project, the scratch and the extra roots.\n\
         (deny file-write*)\n\
         (allow file-write-data (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/dtracehelper\")\n\
           (regex #\"^/dev/tty\") (regex #\"^/dev/fd/\"))\n",
    );
    for i in 0..n_rw {
        p.push_str(&format!(
            "(allow file-write* (subpath (param \"RW{i}\")))\n"
        ));
    }
    p.push_str(
        "; Reads: every home hidden (Wright's store is in one), then the\n\
         ; project, scratch, toolchains and home PATH entries let back in.\n\
         (deny file-read* (subpath \"/Users\") (subpath \"/var/root\"))\n\
         (allow file-read-metadata (literal \"/Users\") (subpath (param \"HOME_REAL\")))\n",
    );
    for i in 0..n_rw {
        p.push_str(&format!("(allow file-read* (subpath (param \"RW{i}\")))\n"));
    }
    for i in 0..n_ro {
        p.push_str(&format!("(allow file-read* (subpath (param \"RO{i}\")))\n"));
    }
    p.push_str(
        "; No keychain: the Security framework's services.\n\
         (deny mach-lookup (global-name \"com.apple.SecurityServer\")\n\
           (global-name \"com.apple.securityd\") (global-name \"com.apple.securityd.xpc\")\n\
           (global-name \"com.apple.security.agent\") (global-name \"com.apple.secd\")\n\
           (global-name \"com.apple.trustd.agent\"))\n\
         ; No reach into other processes.\n\
         (deny signal)\n\
         (allow signal (target same-sandbox))\n\
         (deny process-info*)\n\
         (allow process-info* (target same-sandbox))\n\
         (deny process-info-pidinfo)\n\
         (allow process-info-pidinfo (target same-sandbox))\n",
    );
    if !allow_network {
        p.push_str("; No network without the permission.\n(deny network*)\n");
    }
    p
}

/// A private directory: 0700 from the moment it exists (WR-10).
fn private_dir(p: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn run(program: &str, args: &[&str], policy: &Policy) -> Result<RunOutput, String> {
    capabilities().map_err(|e| format!("{e}; refusing to run commands unsandboxed"))?;
    let workdir = policy
        .workdir
        .canonicalize()
        .map_err(|e| format!("workdir: {e}"))?;

    let scratch = std::env::temp_dir().join(format!(
        "wright-sbx-{}-{}",
        std::process::id(),
        SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    private_dir(&scratch).map_err(|e| format!("scratch: {e}"))?;
    private_dir(&scratch.join("tmp")).map_err(|e| format!("scratch: {e}"))?;
    let _cleanup = Cleanup(scratch.clone());
    let scratch = real(&scratch);

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/var/empty".into());
    let home_real = real(&home);

    let mut rw: Vec<PathBuf> = vec![workdir.clone(), scratch.clone()];
    rw.extend(policy.extra_rw.iter().map(|p| real(p)));
    let mut ro: Vec<PathBuf> = Vec::new();
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".rustup"));
    ro.push(real(&rustup));
    // cargo's binaries (not its registry credentials): ~/.cargo/bin.
    ro.push(real(&home.join(".cargo").join("bin")));
    if let Ok(path) = std::env::var("PATH") {
        ro.extend(home_path_entries(&path, &home).iter().map(|p| real(p)));
        ro.extend(home_path_entries(&path, &home_real).iter().map(|p| real(p)));
    }
    ro.extend(policy.extra_ro.iter().map(|p| real(p)));
    ro.sort();
    ro.dedup();

    let mut cmd = Command::new(SANDBOX_EXEC);
    cmd.arg("-p")
        .arg(profile(rw.len(), ro.len(), policy.allow_network));
    cmd.arg("-D")
        .arg(format!("HOME_REAL={}", home_real.display()));
    for (i, p) in rw.iter().enumerate() {
        cmd.arg("-D").arg(format!("RW{i}={}", p.display()));
    }
    for (i, p) in ro.iter().enumerate() {
        cmd.arg("-D").arg(format!("RO{i}={}", p.display()));
    }
    cmd.arg(program)
        .args(args)
        .current_dir(&workdir)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()),
        )
        .env("HOME", &scratch)
        .env("TMPDIR", scratch.join("tmp"))
        .env("LANG", "C.UTF-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("NO_COLOR", "1")
        .env("RUSTUP_HOME", &rustup)
        .envs(policy.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .process_group(0);
    let (mut reader, writer) = std::io::pipe().map_err(|e| format!("pipe: {e}"))?;
    let writer2 = writer.try_clone().map_err(|e| format!("pipe: {e}"))?;
    cmd.stdout(writer).stderr(writer2);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start '{program}' under Seatbelt: {e}"))?;
    drop(cmd); // our copies of the pipe's write ends
    let pid = child.id() as i32;

    let reader_thread = std::thread::spawn(move || {
        let mut keep: Vec<u8> = Vec::new();
        let mut truncated = false;
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            keep.extend_from_slice(&buf[..n]);
            if keep.len() > MAX_OUTPUT * 2 {
                let cut = keep.len() - MAX_OUTPUT;
                keep.drain(..cut);
                truncated = true;
            }
        }
        if keep.len() > MAX_OUTPUT {
            let cut = keep.len() - MAX_OUTPUT;
            keep.drain(..cut);
            truncated = true;
        }
        (String::from_utf8_lossy(&keep).into_owned(), truncated)
    });

    let deadline = Instant::now() + policy.timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if wright_runtime::killswitch::is_stopped() || Instant::now() >= deadline => {
                timed_out = true;
                // SAFETY: plain kill(2) of our own child's process group.
                unsafe { kill(-pid, SIGKILL) };
                break child.wait().map_err(|e| e.to_string())?;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(format!("waiting for '{program}': {e}")),
        }
    };
    // Stragglers left in the group. SAFETY: as above; ESRCH is harmless.
    unsafe { kill(-pid, SIGKILL) };
    let (output, truncated) = reader_thread.join().unwrap_or_default();
    Ok(RunOutput {
        code: status.code(),
        output,
        timed_out,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_shapes() {
        let p = profile(2, 1, false);
        assert!(p.contains("(subpath (param \"RW1\"))"));
        assert!(p.contains("(subpath (param \"RO0\"))"));
        assert!(p.contains("(deny network*)"));
        assert!(!profile(1, 0, true).contains("(deny network*)"));
        // The home deny comes before the allows (last match wins).
        assert!(p.find("(deny file-read* (subpath \"/Users\")").unwrap() < p.find("RO0").unwrap());
    }

    #[test]
    fn home_path_entries_are_entries_not_parents() {
        let home = Path::new("/Users/ada");
        assert_eq!(
            home_path_entries("/Users/ada/.cargo/bin:/usr/bin:/Users/ada:/Users", home),
            vec![PathBuf::from("/Users/ada/.cargo/bin")]
        );
    }
}

/// The behaviour suite: the Linux suite's contract, run on a real Mac
/// (GitHub's hosted macOS runners — `.github/workflows/wright-sandbox.yml`).
#[cfg(all(test, target_os = "macos"))]
mod behaviour {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-sbx-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn sh(cmd: &str, policy: &Policy) -> RunOutput {
        run("/bin/sh", &["-c", cmd], policy).expect("Seatbelt available on the test machine")
    }

    #[test]
    fn capabilities_are_reported() {
        assert!(capabilities().unwrap().contains("Seatbelt"));
    }

    #[test]
    fn commands_run_and_can_write_the_project() {
        let p = project("write");
        let out = sh("echo hello > made.txt && cat made.txt", &Policy::new(&p));
        assert!(out.success(), "{}", out.report());
        assert_eq!(out.output.trim(), "hello");
    }

    #[test]
    fn writes_outside_the_project_are_blocked() {
        let p = project("outside");
        let victim = std::env::temp_dir().join(format!("wright-victim-{}", std::process::id()));
        let out = sh(
            &format!("echo pwned > {}", victim.display()),
            &Policy::new(&p),
        );
        assert!(!out.success());
        assert!(!victim.exists(), "the sandbox let a write escape");
    }

    /// WR-01 on macOS: a secret under the real home is unreadable, even with
    /// `~/.cargo/bin` on PATH.
    #[test]
    fn home_secrets_are_invisible() {
        let p = project("home");
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let dir = home.join(format!(".wright-sbx-secret-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("id_rsa"), "PRIVATE").unwrap();
        let out = sh(&format!("cat {}/id_rsa", dir.display()), &Policy::new(&p));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!out.output.contains("PRIVATE"), "{}", out.report());
    }

    #[test]
    fn environment_is_scrubbed() {
        std::env::set_var("WRIGHT_SECRET_SHOULD_NOT_LEAK", "leaked");
        let p = project("env");
        let out = sh("env", &Policy::new(&p));
        assert!(!out.output.contains("leaked"));
    }

    #[test]
    fn scratch_home_is_private() {
        let p = project("scratch");
        let out = sh("stat -f %Lp \"$HOME\" \"$TMPDIR\"", &Policy::new(&p));
        assert_eq!(
            out.output.split_whitespace().collect::<Vec<_>>(),
            ["700", "700"],
            "{}",
            out.report()
        );
    }

    #[test]
    fn network_is_off_unless_granted() {
        let p = project("net");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let probe = format!("/usr/bin/nc -z -G 2 127.0.0.1 {port} && echo CON''NECTED");
        let denied = sh(&probe, &Policy::new(&p));
        assert!(!denied.output.contains("CONNECTED"), "{}", denied.report());
        let mut allowed = Policy::new(&p);
        allowed.allow_network = true;
        let ok = sh(&probe, &allowed);
        assert!(ok.output.contains("CONNECTED"), "{}", ok.report());
    }

    #[test]
    fn host_processes_cannot_be_signalled() {
        let p = project("signal");
        let mut decoy = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = decoy.id();
        let out = sh(
            &format!("kill -9 {pid} 2>/dev/null && echo KILLED || echo refused"),
            &Policy::new(&p),
        );
        let alive = decoy.try_wait().unwrap().is_none();
        let _ = decoy.kill();
        let _ = decoy.wait();
        assert!(alive && !out.output.contains("KILLED"), "{}", out.report());
    }

    #[test]
    fn keychain_is_unreachable() {
        let p = project("keychain");
        let out = sh(
            "/usr/bin/security list-keychains 2>&1; echo rc=$?",
            &Policy::new(&p),
        );
        assert!(!out.output.contains("login.keychain"), "{}", out.report());
    }

    #[test]
    fn kill_switch_is_out_of_reach() {
        let p = project("stop");
        let stop = wright_runtime::killswitch::path();
        let out = sh(
            &format!("mkdir -p {d} 2>/dev/null; echo x > {f} 2>/dev/null && echo ENGAGED || echo refused",
                d = stop.parent().unwrap().display(), f = stop.display()),
            &Policy::new(&p),
        );
        assert!(!out.output.contains("ENGAGED"), "{}", out.report());
    }

    #[test]
    fn timeouts_kill_the_whole_process_group() {
        let p = project("timeout");
        let mut pol = Policy::new(&p);
        pol.timeout = Duration::from_millis(300);
        let started = Instant::now();
        let out = sh("sleep 30 & sleep 30; echo never", &pol);
        assert!(out.timed_out);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
