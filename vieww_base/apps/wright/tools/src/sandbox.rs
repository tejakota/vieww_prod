//! The process sandbox for `shell`, `git` and `run-tests`.
//!
//! Linux (v0.2): every command runs in a child process that, before it
//! executes anything, locks itself down with the kernel's own tools:
//!
//! * **Landlock** (kernel ≥ 5.13): the filesystem becomes read-only and
//!   executable only where the toolchain lives (`/usr`, `/bin`, `/lib`,
//!   `/etc`, `/opt`, the directories on `PATH` themselves — never their
//!   parents, and never `$HOME` or an ancestor of it — and the rustup
//!   toolchains), and writable ONLY inside the project (plus a private 0700
//!   scratch HOME/TMPDIR that is deleted afterwards). Everything else —
//!   `~/.ssh`, other projects, Wright's own store — is invisible (WR-01).
//!   `/proc` is readable, but Landlock scopes ptrace-mode access to the
//!   command's own domain, so no outside process's `environ`, `mem` or
//!   `status` (Wright's API keys) can be read — a test pins this.
//! * **Its own PID namespace** where unprivileged user namespaces exist:
//!   the command cannot name, signal or ptrace any host process, and when
//!   its first process exits the kernel kills everything it left behind.
//! * **A seccomp-BPF filter** (WR-02): ptrace, process_vm_*, keyctl,
//!   perf_event_open, bpf, io_uring, mount/namespace syscalls, module and
//!   kexec loading and the clock/reboot family fail with EPERM; `clone`
//!   with namespace flags too, and `clone3` reports ENOSYS so libc falls
//!   back to the filterable `clone`. A denylist rather than an allowlist,
//!   so toolchains keep working; Landlock remains the filesystem wall.
//! * **No network** unless the agent holds the network permission: a
//!   fresh network namespace (no interfaces), and Landlock's TCP rules
//!   (ABI ≥ 4) as a second wall. If neither is available the command is
//!   REFUSED — the sandbox fails closed, never open.
//! * `no_new_privs`, its own process group (killed as a group on
//!   timeout), core dumps off, a file-size limit, and a scrubbed
//!   environment (no inherited secrets, no API keys).
//!
//! Windows uses WSL 2 + Bubblewrap (`sandbox_windows.rs`): the project is
//! mounted as the only writable source tree, Windows drives and user homes
//! are hidden, and commands still fail closed if the isolation runtime is not
//! installed. macOS uses Seatbelt (`sandbox_macos.rs`, WR-03) with the same
//! policy, and also fails closed if Seatbelt is unavailable.
//!
//! This is one of the files AGENTS.md allows `unsafe` in: the FFI and the
//! `pre_exec` hook. Every block carries a SAFETY comment.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// What a sandboxed command may do.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Working directory, and the one writable tree.
    pub workdir: PathBuf,
    pub allow_network: bool,
    pub timeout: Duration,
    /// Extra writable directories (e.g. CARGO_HOME for Rust projects).
    pub extra_rw: Vec<PathBuf>,
    /// Extra read-only directories.
    pub extra_ro: Vec<PathBuf>,
    /// Environment on top of the scrubbed base.
    pub env: Vec<(String, String)>,
}

impl Policy {
    pub fn new(workdir: &Path) -> Policy {
        Policy {
            workdir: workdir.to_path_buf(),
            allow_network: false,
            timeout: Duration::from_secs(120),
            extra_rw: Vec::new(),
            extra_ro: Vec::new(),
            env: Vec::new(),
        }
    }
}

/// The result of one sandboxed command.
#[derive(Debug, Clone)]
pub struct RunOutput {
    /// Exit code, or None if killed by a signal / timeout.
    pub code: Option<i32>,
    /// stdout and stderr interleaved, tail-truncated to `MAX_OUTPUT`.
    pub output: String,
    pub timed_out: bool,
    pub truncated: bool,
}

impl RunOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// The text an agent sees.
    pub fn report(&self) -> String {
        let status = match (self.timed_out, self.code) {
            (true, _) => "TIMED OUT (killed)".to_string(),
            (false, Some(c)) => format!("exit code {c}"),
            (false, None) => "killed by a signal".to_string(),
        };
        let trunc = if self.truncated {
            "[... earlier output truncated ...]\n"
        } else {
            ""
        };
        format!("{status}\n{trunc}{}", self.output)
    }
}

/// Output kept per command (the tail; the end is where errors are).
pub const MAX_OUTPUT: usize = 32 * 1024;

/// What the sandbox can do on this machine, for `wright doctor`.
pub fn capabilities() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let abi = linux::landlock_abi();
        if abi <= 0 {
            return Err("Landlock is not available (kernel 5.13+ with Landlock enabled is required); shell, git and run-tests will refuse".into());
        }
        Ok(format!(
            "Landlock ABI {abi}{}; {}; {}",
            if abi >= 4 { ", network rules" } else { "" },
            if linux::pid_namespaces_available() {
                "PID namespace"
            } else {
                "NO PID namespace (unprivileged user namespaces are disabled; set WRIGHT_SANDBOX_STRICT=1 to refuse)"
            },
            if linux::seccomp::filter().is_some() {
                "seccomp filter"
            } else {
                "NO seccomp filter on this architecture"
            }
        ))
    }
    #[cfg(target_os = "windows")]
    {
        windows::capabilities()
    }
    #[cfg(target_os = "macos")]
    {
        macos::capabilities()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Err("the process sandbox is not available on this platform; shell, git and run-tests will refuse".into())
    }
}

/// Run `program args...` under `policy`.
pub fn run(program: &str, args: &[&str], policy: &Policy) -> Result<RunOutput, String> {
    #[cfg(target_os = "linux")]
    {
        linux::run(program, args, policy)
    }
    #[cfg(target_os = "windows")]
    {
        windows::run(program, args, policy)
    }
    #[cfg(target_os = "macos")]
    {
        macos::run(program, args, policy)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = (program, args, policy);
        Err(
            "sandboxed commands are unavailable on this platform; refusing to run unsandboxed"
                .into(),
        )
    }
}

#[cfg(target_os = "windows")]
#[path = "sandbox_windows.rs"]
mod windows;

// Compiled on macOS, and in Linux test builds so the profile generator's
// unit tests run everywhere (its behaviour suite is macOS-only).
#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
#[path = "sandbox_macos.rs"]
mod macos;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::ffi::{c_int, c_long, c_ulong, c_void};
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    const SYS_LANDLOCK_CREATE_RULESET: c_long = 444;
    const SYS_LANDLOCK_ADD_RULE: c_long = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: c_long = 446;
    const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;
    const LANDLOCK_RULE_PATH_BENEATH: c_int = 1;

    const EXECUTE: u64 = 1 << 0;
    const WRITE_FILE: u64 = 1 << 1;
    const READ_FILE: u64 = 1 << 2;
    const READ_DIR: u64 = 1 << 3;
    const NET_BIND_TCP: u64 = 1 << 0;
    const NET_CONNECT_TCP: u64 = 1 << 1;

    const PR_SET_NO_NEW_PRIVS: c_int = 38;
    const PR_SET_SECCOMP: c_int = 22;
    const SECCOMP_MODE_FILTER: c_ulong = 2;
    const SYS_CLOSE_RANGE: c_long = 436;
    const EINTR: c_int = 4;
    const CLONE_NEWPID: c_int = 0x2000_0000;
    const CLONE_NEWUSER: c_int = 0x1000_0000;
    const CLONE_NEWNET: c_int = 0x4000_0000;
    const RLIMIT_FSIZE: c_int = 1;
    const RLIMIT_CORE: c_int = 4;
    const SIGKILL: c_int = 9;
    const O_PATH: i32 = 0o10_000_000;

    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
        handled_access_net: u64,
    }

    #[repr(C, packed)]
    struct PathBeneath {
        allowed_access: u64,
        parent_fd: i32,
    }

    extern "C" {
        fn syscall(num: c_long, ...) -> c_long;
        fn prctl(option: c_int, a2: c_ulong, a3: c_ulong, a4: c_ulong, a5: c_ulong) -> c_int;
        fn setpgid(pid: c_int, pgid: c_int) -> c_int;
        fn kill(pid: c_int, sig: c_int) -> c_int;
        fn unshare(flags: c_int) -> c_int;
        fn setrlimit(resource: c_int, rlim: *const RLimit) -> c_int;
        fn close(fd: c_int) -> c_int;
        fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
        fn _exit(code: c_int) -> !;
        fn getpid() -> c_int;
        fn signal(sig: c_int, handler: usize) -> usize;
        #[link_name = "__errno_location"]
        fn errno() -> *mut c_int;
    }

    /// fork(2) as a bare system call: glibc's `fork()` runs atfork handlers
    /// that take locks, which a `pre_exec` child must not.
    ///
    /// SAFETY (caller): only async-signal-safe work may follow in the child.
    unsafe fn raw_fork() -> c_long {
        const SIGCHLD: c_long = 17;
        #[cfg(target_arch = "x86_64")]
        const SYS_CLONE: c_long = 56;
        #[cfg(target_arch = "aarch64")]
        const SYS_CLONE: c_long = 220;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        const SYS_CLONE: c_long = -1;
        if SYS_CLONE < 0 {
            return -1;
        }
        // SAFETY: clone with only SIGCHLD and no new stack is fork().
        unsafe { syscall(SYS_CLONE, SIGCHLD, 0usize, 0usize, 0usize, 0usize) }
    }

    /// Can this machine give a command its own user + PID namespace?
    pub fn pid_namespaces_available() -> bool {
        // SAFETY: a raw unshare in a throwaway child, nothing else.
        unsafe {
            Command::new("/bin/true")
                .pre_exec(|| {
                    if unshare(CLONE_NEWUSER | CLONE_NEWPID) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                })
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
    }

    /// Landlock ABI version, or <= 0 if unavailable.
    pub fn landlock_abi() -> c_long {
        // SAFETY: the documented version query: null attr, size 0.
        unsafe {
            syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<c_void>(),
                0usize,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        }
    }

    fn all_fs_rights(abi: c_long) -> u64 {
        let mut bits = (1u64 << 13) - 1; // ABI 1: 13 rights
        if abi >= 2 {
            bits |= 1 << 13; // REFER
        }
        if abi >= 3 {
            bits |= 1 << 14; // TRUNCATE
        }
        if abi >= 5 {
            bits |= 1 << 15; // IOCTL_DEV
        }
        bits
    }

    fn open_path(p: &Path) -> Option<OwnedFd> {
        OpenOptions::new()
            .read(true)
            .custom_flags(O_PATH | 0o2_000_000 /* O_CLOEXEC */)
            .open(p)
            .ok()
            .map(OwnedFd::from)
    }

    static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

    fn read_only_roots(policy: &Policy) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = [
            "/usr",
            "/bin",
            "/sbin",
            "/lib",
            "/lib64",
            "/lib32",
            "/etc",
            "/opt",
            "/nix",
            "/snap",
            "/proc",
            "/sys/devices/system/cpu",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Ok(path) = std::env::var("PATH") {
            dirs.extend(path_roots(&path, home.as_deref()));
        }
        // Rust toolchains live in the home directory: the toolchain store
        // itself, never the home.
        match std::env::var_os("RUSTUP_HOME") {
            Some(r) => dirs.push(PathBuf::from(r)),
            None => {
                if let Some(h) = &home {
                    dirs.push(h.join(".rustup"));
                }
            }
        }
        dirs.extend(policy.extra_ro.iter().cloned());
        dirs
    }

    /// The read-only roots a `PATH` contributes (WR-01): each absolute entry
    /// itself — never its parent, which for `~/.cargo/bin` is the whole home
    /// — and never `/`, `$HOME` or an ancestor of `$HOME`.
    pub(super) fn path_roots(path: &str, home: Option<&Path>) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for d in path.split(':').filter(|d| d.starts_with('/')) {
            let d = PathBuf::from(d);
            if d == Path::new("/") {
                continue;
            }
            if let Some(h) = home {
                if h.starts_with(&d) {
                    continue; // `$HOME` itself, or `/home`
                }
            }
            out.push(d);
        }
        out
    }

    /// A private directory: 0700 from the moment it exists (WR-10).
    pub(super) fn private_dir(p: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(p)?;
        // `recursive` leaves an existing directory's mode alone.
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
    }

    pub fn run(program: &str, args: &[&str], policy: &Policy) -> Result<RunOutput, String> {
        let abi = landlock_abi();
        if abi <= 0 {
            return Err(
                "Landlock is not available on this kernel; refusing to run commands unsandboxed"
                    .into(),
            );
        }
        let workdir = policy
            .workdir
            .canonicalize()
            .map_err(|e| format!("workdir: {e}"))?;

        // A private scratch HOME/TMPDIR, outside the project, deleted after.
        let scratch = std::env::temp_dir().join(format!(
            "wright-sbx-{}-{}",
            std::process::id(),
            SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        private_dir(&scratch).map_err(|e| format!("scratch: {e}"))?;
        private_dir(&scratch.join("tmp")).map_err(|e| format!("scratch: {e}"))?;
        let _cleanup = Cleanup(scratch.clone());

        // Open every rule's directory BEFORE fork: the child may only make
        // raw syscalls (no allocation) between fork and exec.
        let fs_all = all_fs_rights(abi);
        let ro = EXECUTE | READ_FILE | READ_DIR;
        let mut rules: Vec<(u64, OwnedFd)> = Vec::new();
        for d in read_only_roots(policy) {
            if let Some(fd) = open_path(&d) {
                rules.push((ro, fd));
            }
        }
        if let Some(fd) = open_path(Path::new("/dev")) {
            rules.push((READ_FILE | WRITE_FILE | READ_DIR, fd));
        }
        for d in std::iter::once(&workdir)
            .chain(policy.extra_rw.iter())
            .chain(std::iter::once(&scratch))
        {
            match open_path(d) {
                Some(fd) => rules.push((fs_all, fd)),
                None => return Err(format!("cannot open {} for the sandbox", d.display())),
            }
        }
        let rule_specs: Vec<(u64, c_int)> =
            rules.iter().map(|(a, fd)| (*a, fd.as_raw_fd())).collect();
        let allow_network = policy.allow_network;
        let filter = seccomp::filter();
        let strict = std::env::var_os("WRIGHT_SANDBOX_STRICT").is_some_and(|v| v == "1");
        // Diagnostic: exercise the no-user-namespace fallback on a machine
        // that has them (CI runs the suite both ways).
        let try_pid_ns = !std::env::var_os("WRIGHT_SANDBOX_NO_PIDNS").is_some_and(|v| v == "1");
        let handled_net = if abi >= 4 && !allow_network {
            NET_BIND_TCP | NET_CONNECT_TCP
        } else {
            0
        };
        if !allow_network && abi < 4 {
            // We will rely on the network namespace alone; check it works
            // here, in the parent, so a failure is a clean refusal.
            // (Probed in a throwaway child via the same unshare call.)
            let ok = Command::new("/bin/true")
                .pre_exec_unshare_probe()
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                return Err("cannot isolate the network on this kernel (no Landlock net rules, no network namespaces); refusing to run a command without network permission".into());
            }
        }

        let (mut reader, writer) = std::io::pipe().map_err(|e| format!("pipe: {e}"))?;
        let writer2 = writer.try_clone().map_err(|e| format!("pipe: {e}"))?;
        let mut cmd = Command::new(program);
        cmd.args(args)
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
            .envs(policy.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(writer)
            .stderr(writer2);
        if let Ok(rustup) = std::env::var("RUSTUP_HOME") {
            cmd.env("RUSTUP_HOME", rustup);
        } else if let Ok(home) = std::env::var("HOME") {
            cmd.env("RUSTUP_HOME", PathBuf::from(home).join(".rustup"));
        }

        // SAFETY: the closure runs in the forked child before exec. It
        // only performs raw syscalls on stack data and pre-opened fds
        // (no allocation, no locks), which is async-signal-safe.
        unsafe {
            cmd.pre_exec(move || {
                setpgid(0, 0);
                let fsize = RLimit {
                    cur: 512 << 20,
                    max: 512 << 20,
                };
                setrlimit(RLIMIT_FSIZE, &fsize);
                let core = RLimit { cur: 0, max: 0 };
                setrlimit(RLIMIT_CORE, &core);
                // A user namespace buys a PID namespace (and, without network
                // permission, an empty network namespace) in one call.
                let net_flag = if allow_network { 0 } else { CLONE_NEWNET };
                let pid_ns = try_pid_ns && unshare(CLONE_NEWUSER | CLONE_NEWPID | net_flag) == 0;
                if !pid_ns {
                    if strict {
                        return Err(std::io::Error::other(
                            "PID namespace unavailable and WRIGHT_SANDBOX_STRICT=1",
                        ));
                    }
                    if !allow_network
                        && unshare(CLONE_NEWUSER | CLONE_NEWNET) != 0
                        && handled_net == 0
                    {
                        return Err(std::io::Error::other("network isolation failed"));
                    }
                }
                if pid_ns {
                    // unshare(CLONE_NEWPID) applies to our children: fork once.
                    // The child is PID 1 of the new namespace and goes on to
                    // exec the command; this process waits and mirrors how it
                    // ended. When PID 1 exits the kernel kills the namespace.
                    let child = raw_fork();
                    if child < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if child > 0 {
                        // Drop every fd but stdio — above all std's exec-status
                        // pipe, or `spawn()` would block until the command ends.
                        syscall(SYS_CLOSE_RANGE, 3u32, u32::MAX, 0u32);
                        let mut st: c_int = 0;
                        loop {
                            let r = waitpid(child as c_int, &mut st, 0);
                            if r == child as c_int {
                                break;
                            }
                            if r < 0 && *errno() != EINTR {
                                _exit(127);
                            }
                        }
                        if st & 0x7f == 0 {
                            _exit((st >> 8) & 0xff);
                        }
                        let sig = st & 0x7f;
                        signal(sig, 0 /* SIG_DFL */);
                        kill(getpid(), sig);
                        _exit(128 + sig);
                    }
                }
                if prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let attr = RulesetAttr {
                    handled_access_fs: fs_all,
                    handled_access_net: handled_net,
                };
                let size = if abi >= 4 {
                    std::mem::size_of::<RulesetAttr>()
                } else {
                    8
                };
                let ruleset = syscall(
                    SYS_LANDLOCK_CREATE_RULESET,
                    &attr as *const RulesetAttr,
                    size,
                    0u32,
                ) as c_int;
                if ruleset < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for (access, fd) in rule_specs.iter() {
                    let rule = PathBeneath {
                        allowed_access: *access & fs_all,
                        parent_fd: *fd,
                    };
                    if syscall(
                        SYS_LANDLOCK_ADD_RULE,
                        ruleset,
                        LANDLOCK_RULE_PATH_BENEATH,
                        &rule as *const PathBeneath,
                        0u32,
                    ) != 0
                    {
                        // A directory whose type does not support every
                        // right (rare) is skipped rather than fatal.
                        continue;
                    }
                }
                if syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset, 0u32) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                close(ruleset);
                if let Some(prog) = filter.as_ref() {
                    let fprog = seccomp::SockFprog {
                        len: prog.len() as u16,
                        filter: prog.as_ptr(),
                    };
                    if prctl(
                        PR_SET_SECCOMP,
                        SECCOMP_MODE_FILTER,
                        &fprog as *const seccomp::SockFprog as c_ulong,
                        0,
                        0,
                    ) != 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                } else if strict {
                    return Err(std::io::Error::other(
                        "no seccomp filter for this architecture and WRIGHT_SANDBOX_STRICT=1",
                    ));
                }
                Ok(())
            });
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not start '{program}': {e}"))?;
        drop(cmd); // closes our copies of the pipe's write ends
        drop(rules);
        let pid = child.id() as c_int;

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
                Ok(None) if wright_runtime::killswitch::is_stopped() => {
                    // The kill switch: the command goes now, not at its clock.
                    timed_out = true;
                    // SAFETY: plain kill(2) of our own child's process group.
                    unsafe { kill(-pid, SIGKILL) };
                    break child.wait().map_err(|e| e.to_string())?;
                }
                Ok(None) if Instant::now() >= deadline => {
                    timed_out = true;
                    // SAFETY: plain kill(2) of our own child's process group.
                    unsafe { kill(-pid, SIGKILL) };
                    break child.wait().map_err(|e| e.to_string())?;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(15)),
                Err(e) => return Err(format!("waiting for '{program}': {e}")),
            }
        };
        // Kill stragglers the command may have left in its group.
        // SAFETY: as above; ESRCH (no such group) is harmless.
        unsafe { kill(-pid, SIGKILL) };
        let (output, truncated) = reader_thread.join().unwrap_or_default();
        let _ = status.signal();
        Ok(RunOutput {
            code: status.code(),
            output,
            timed_out,
            truncated,
        })
    }

    /// The seccomp-BPF program (WR-02), built in the parent before fork.
    pub(super) mod seccomp {
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct SockFilter {
            code: u16,
            jt: u8,
            jf: u8,
            k: u32,
        }
        #[repr(C)]
        pub struct SockFprog {
            pub len: u16,
            pub filter: *const SockFilter,
        }

        const LD_W_ABS: u16 = 0x20;
        const JEQ_K: u16 = 0x15;
        const JGE_K: u16 = 0x35;
        const JSET_K: u16 = 0x45;
        const RET_K: u16 = 0x06;
        const RET_KILL_PROCESS: u32 = 0x8000_0000;
        const RET_ERRNO: u32 = 0x0005_0000;
        const RET_ALLOW: u32 = 0x7fff_0000;
        const EPERM: u32 = 1;
        const ENOSYS: u32 = 38;
        /// CLONE_NEWNS | NEWCGROUP | NEWUTS | NEWIPC | NEWUSER | NEWPID | NEWNET
        pub const NAMESPACE_FLAGS: u32 = 0x7E02_0000;

        struct Arch {
            audit: u32,
            /// Syscalls >= this are a foreign ABI (x32) and kill the process.
            abi_limit: Option<u32>,
            clone: u32,
            clone3: u32,
            denied: &'static [u32],
        }

        #[cfg(target_arch = "x86_64")]
        const ARCH: Option<Arch> = Some(Arch {
            audit: 0xC000_003E,
            abi_limit: Some(0x4000_0000),
            clone: 56,
            clone3: 435,
            denied: &[
                101, // ptrace
                310, 311, // process_vm_readv/writev
                248, 249, 250, // add_key, request_key, keyctl
                298, // perf_event_open
                321, // bpf
                323, // userfaultfd
                246, 320, // kexec_load, kexec_file_load
                175, 313, 176, // init_module, finit_module, delete_module
                165, 166, 155, 161, // mount, umount2, pivot_root, chroot
                272, 308, // unshare, setns
                167, 168, 169, 163, 179, // swapon, swapoff, reboot, acct, quotactl
                303, 304, // name_to_handle_at, open_by_handle_at
                172, 173, // iopl, ioperm
                103, // syslog
                164, 227, 159, // settimeofday, clock_settime, adjtimex
                312, // kcmp
                425, 426, 427, // io_uring_*
                428, 429, 430, 431, 432, 442, // open_tree, move_mount, fs*, mount_setattr
                300, // fanotify_init
                212, // lookup_dcookie
            ],
        });

        #[cfg(target_arch = "aarch64")]
        const ARCH: Option<Arch> = Some(Arch {
            audit: 0xC000_00B7,
            abi_limit: None,
            clone: 220,
            clone3: 435,
            denied: &[
                117, // ptrace
                270, 271, // process_vm_readv/writev
                217, 218, 219, // add_key, request_key, keyctl
                241, // perf_event_open
                280, // bpf
                282, // userfaultfd
                104, 294, // kexec_load, kexec_file_load
                105, 273, 106, // init_module, finit_module, delete_module
                40, 39, 41, 51, // mount, umount2, pivot_root, chroot
                97, 268, // unshare, setns
                224, 225, 142, 89, 60, // swapon, swapoff, reboot, acct, quotactl
                264, 265, // name_to_handle_at, open_by_handle_at
                116, // syslog
                170, 112, 171, // settimeofday, clock_settime, adjtimex
                272, // kcmp
                425, 426, 427, // io_uring_*
                428, 429, 430, 431, 432, 442, // open_tree, move_mount, fs*, mount_setattr
                262, // fanotify_init
                18,  // lookup_dcookie
            ],
        });

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        const ARCH: Option<Arch> = None;

        fn stmt(code: u16, k: u32) -> SockFilter {
            SockFilter {
                code,
                jt: 0,
                jf: 0,
                k,
            }
        }
        fn jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
            SockFilter { code, jt, jf, k }
        }

        /// The program, or None on an architecture without a table.
        pub fn filter() -> Option<Vec<SockFilter>> {
            let a = ARCH?;
            let mut f = vec![
                stmt(LD_W_ABS, 4), // seccomp_data.arch
                jump(JEQ_K, a.audit, 1, 0),
                stmt(RET_K, RET_KILL_PROCESS),
                stmt(LD_W_ABS, 0), // seccomp_data.nr
            ];
            if let Some(limit) = a.abi_limit {
                f.push(jump(JGE_K, limit, 0, 1));
                f.push(stmt(RET_K, RET_KILL_PROCESS));
            }
            for &nr in a.denied {
                f.push(jump(JEQ_K, nr, 0, 1));
                f.push(stmt(RET_K, RET_ERRNO | EPERM));
            }
            // clone3 takes its flags through a pointer BPF cannot follow:
            // ENOSYS makes libc fall back to clone, whose flags it can.
            f.push(jump(JEQ_K, a.clone3, 0, 1));
            f.push(stmt(RET_K, RET_ERRNO | ENOSYS));
            f.push(jump(JEQ_K, a.clone, 0, 3));
            f.push(stmt(LD_W_ABS, 16)); // args[0], low 32 bits (little-endian)
            f.push(jump(JSET_K, NAMESPACE_FLAGS, 0, 1));
            f.push(stmt(RET_K, RET_ERRNO | EPERM));
            f.push(stmt(RET_K, RET_ALLOW));
            Some(f)
        }
    }

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    trait ProbeExt {
        fn pre_exec_unshare_probe(&mut self) -> &mut Self;
    }
    impl ProbeExt for Command {
        fn pre_exec_unshare_probe(&mut self) -> &mut Self {
            // SAFETY: a raw syscall in the child, nothing else.
            unsafe {
                self.pre_exec(|| {
                    if unshare(CLONE_NEWUSER | CLONE_NEWNET) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                })
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-sbx-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn sh(cmd: &str, policy: &Policy) -> RunOutput {
        run("/bin/sh", &["-c", cmd], policy).expect("sandbox available on the test machine")
    }

    #[test]
    fn commands_run_and_can_write_the_project() {
        let p = project("write");
        let out = sh("echo hello > made.txt && cat made.txt", &Policy::new(&p));
        assert!(out.success(), "{}", out.report());
        assert_eq!(out.output.trim(), "hello");
        assert!(p.join("made.txt").exists());
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

    #[test]
    fn home_secrets_are_invisible() {
        let p = project("home");
        let secret_dir =
            std::env::temp_dir().join(format!("wright-fake-home-{}", std::process::id()));
        std::fs::create_dir_all(&secret_dir).unwrap();
        std::fs::write(secret_dir.join("id_rsa"), "PRIVATE").unwrap();
        let out = sh(
            &format!("cat {}/id_rsa", secret_dir.display()),
            &Policy::new(&p),
        );
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
    fn network_is_off_unless_granted() {
        let p = project("net");
        // Connect to a local listener: blocked without permission.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let probe = format!("python3 -c \"import socket; socket.create_connection(('127.0.0.1', {port}), timeout=2); print('CON' + 'NECTED')\"");
        let denied = sh(&probe, &Policy::new(&p));
        assert!(!denied.output.contains("CONNECTED"), "{}", denied.report());
        let mut allowed_policy = Policy::new(&p);
        allowed_policy.allow_network = true;
        let allowed = sh(&probe, &allowed_policy);
        assert!(allowed.output.contains("CONNECTED"), "{}", allowed.report());
    }

    #[test]
    fn timeouts_kill_the_whole_process_group() {
        let p = project("timeout");
        let mut pol = Policy::new(&p);
        pol.timeout = Duration::from_millis(300);
        let started = std::time::Instant::now();
        let out = sh("sleep 30 & sleep 30; echo never", &pol);
        assert!(out.timed_out);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn path_parents_are_never_roots() {
        let home = Path::new("/home/ada");
        let roots = linux::path_roots(
            "/home/ada/.cargo/bin:/usr/bin:/:/home/ada:/home:relative/bin",
            Some(home),
        );
        assert_eq!(
            roots,
            vec![
                PathBuf::from("/home/ada/.cargo/bin"),
                PathBuf::from("/usr/bin")
            ]
        );
    }

    /// WR-01 regression: a rustup-style `~/.cargo/bin` on PATH used to make
    /// its parent — the whole home — a read-only root.
    #[test]
    fn home_paths_are_not_roots() {
        let p = project("home-path");
        let fake_home =
            std::env::temp_dir().join(format!("wright-fake-rustup-home-{}", std::process::id()));
        let bin = fake_home.join(".cargo").join("bin");
        std::fs::create_dir_all(fake_home.join(".ssh")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(fake_home.join(".ssh").join("id_rsa"), "PRIVATE").unwrap();
        let mut pol = Policy::new(&p);
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        // The sandbox reads the PATH it hands the child from the policy env too.
        pol.env.push(("PATH".into(), path.clone()));
        let roots = linux::path_roots(&path, Some(&fake_home));
        assert!(!roots.iter().any(|r| r == &fake_home), "{roots:?}");
        // Run with that PATH as the process PATH (prepending is harmless to
        // concurrent tests: every entry they rely on is still there).
        let _g = PathGuard::prepend(&bin);
        let out = sh(&format!("cat {}/.ssh/id_rsa", fake_home.display()), &pol);
        assert!(!out.output.contains("PRIVATE"), "{}", out.report());
    }

    struct PathGuard(String);
    impl PathGuard {
        fn prepend(d: &Path) -> PathGuard {
            let old = std::env::var("PATH").unwrap();
            std::env::set_var("PATH", format!("{}:{old}", d.display()));
            PathGuard(old)
        }
    }
    impl Drop for PathGuard {
        fn drop(&mut self) {
            std::env::set_var("PATH", &self.0);
        }
    }

    /// Another process's /proc entry (Wright's own environment holds API
    /// keys) is unreadable; the command's own is readable.
    #[test]
    fn other_processes_proc_is_hidden() {
        let p = project("proc");
        let me = std::process::id();
        let out = sh(
            &format!("cat /proc/{me}/environ >/dev/null 2>&1 && echo LEAK; cat /proc/{me}/status >/dev/null && echo STATUS_VISIBLE; cat /proc/self/status | head -1"),
            &Policy::new(&p),
        );
        assert!(!out.output.contains("LEAK"), "{}", out.report());
        assert!(out.output.contains("Name:"), "{}", out.report());
    }

    /// WR-02: a host process cannot be signalled from inside.
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
        if linux::pid_namespaces_available()
            && std::env::var_os("WRIGHT_SANDBOX_NO_PIDNS").is_none()
        {
            assert!(alive && !out.output.contains("KILLED"), "{}", out.report());
        } else {
            eprintln!("no unprivileged user namespaces here: PID isolation not tested");
        }
    }

    /// WR-02: the seccomp filter refuses the dangerous syscalls.
    #[test]
    fn dangerous_syscalls_are_refused() {
        let p = project("seccomp");
        #[cfg(target_arch = "x86_64")]
        let (keyctl, ptrace, unshare) = (250, 101, 272);
        #[cfg(target_arch = "aarch64")]
        let (keyctl, ptrace, unshare) = (219, 117, 97);
        let script = format!(
            "import ctypes, os\n\
             l = ctypes.CDLL(None, use_errno=True)\n\
             for name, nr, args in (('keyctl', {keyctl}, (0, -3, 0)), ('ptrace', {ptrace}, (0, 0, 0)), ('unshare', {unshare}, (0x10000000, 0, 0))):\n\
             \x20   r = l.syscall(nr, *args)\n\
             \x20   print(name, r, ctypes.get_errno())\n\
             try:\n\
             \x20   os.fork() or os._exit(0)\n\
             \x20   print('fork ok')\n\
             except OSError as e:\n\
             \x20   print('fork failed', e)\n"
        );
        std::fs::write(p.join("probe.py"), script).unwrap();
        let out = sh("python3 probe.py", &Policy::new(&p));
        for name in ["keyctl", "ptrace", "unshare"] {
            assert!(
                out.output.contains(&format!("{name} -1 1")),
                "{name} not refused with EPERM:\n{}",
                out.report()
            );
        }
        // Ordinary process creation still works (clone3 -> clone fallback).
        assert!(out.output.contains("fork ok"), "{}", out.report());
    }

    /// WR-10: the scratch HOME is private to the user.
    #[test]
    fn scratch_home_is_0700() {
        let p = project("scratch");
        let out = sh("stat -c %a \"$HOME\" \"$TMPDIR\"", &Policy::new(&p));
        assert_eq!(
            out.output.split_whitespace().collect::<Vec<_>>(),
            ["700", "700"],
            "{}",
            out.report()
        );
    }

    /// WR-16: the kill switch lives outside every writable root, so a
    /// sandboxed command cannot engage (or release) it.
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

    /// Timeouts still kill everything when the command runs as PID 1 of its
    /// own namespace behind a waiting parent.
    #[test]
    fn exit_codes_and_signals_pass_through() {
        let p = project("status");
        assert_eq!(sh("exit 7", &Policy::new(&p)).code, Some(7));
        // A process killed inside reports 128+9 through the shell.
        let killed = sh("sh -c 'kill -9 $$'; echo status=$?", &Policy::new(&p));
        assert!(killed.output.contains("status=137"), "{}", killed.report());
    }
}
