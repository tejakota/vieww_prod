//! Windows Job objects for native worker CLIs (WR-05, tier 1).
//!
//! A worker is started suspended, put in a Job object, then resumed, so
//! nothing it starts can escape the job before the limits apply:
//!
//! * `KILL_ON_JOB_CLOSE`: dropping the [`Job`] (timeout, kill switch, normal
//!   end) kills the worker *and every process it started* — before this, only
//!   the worker process itself was killed and its node/python children lived on;
//! * `DIE_ON_UNHANDLED_EXCEPTION`, no breakaway: children cannot leave;
//! * a committed-memory cap per job and an active-process cap.
//!
//! What a Job object does not do is narrow the filesystem: the throwaway
//! 0700 copy, the scrubbed environment and the all-or-nothing read-back
//! remain the filesystem boundary for native Windows workers, and
//! `wright doctor` says so. (Tier 2 — AppContainer — is tracked in WR-05.)
//!
//! One of the files AGENTS.md allows `unsafe` in. Every block carries a
//! SAFETY comment.

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;

type Handle = *mut c_void;

const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
const LIMIT_ACTIVE_PROCESS: u32 = 0x0000_0008;
const LIMIT_JOB_MEMORY: u32 = 0x0000_0200;
const LIMIT_DIE_ON_UNHANDLED_EXCEPTION: u32 = 0x0000_0400;
const LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

/// The limits native workers run under.
pub const WORKER_MAX_PROCESSES: u32 = 64;
/// 4 GiB of committed memory across the worker's whole process tree.
pub const WORKER_MAX_MEMORY: usize = 4 << 30;

/// `CREATE_SUSPENDED`, for `CommandExt::creation_flags`.
pub const CREATE_SUSPENDED: u32 = 0x0000_0004;

#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_ops: u64,
    write_ops: u64,
    other_ops: u64,
    read_bytes: u64,
    write_bytes: u64,
    other_bytes: u64,
}

#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
    fn QueryInformationJobObject(
        job: Handle,
        class: i32,
        info: *mut c_void,
        len: u32,
        returned: *mut u32,
    ) -> i32;
    fn CloseHandle(h: Handle) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtResumeProcess(process: Handle) -> i32;
}

/// A job holding one worker and everything it starts.
pub struct Job(Handle);

// SAFETY: a job handle is a kernel object reference, usable from any thread.
unsafe impl Send for Job {}

impl Job {
    /// A job with the worker limits.
    /// `max_memory` caps the job's committed memory; `None` leaves it
    /// uncapped (the process count and tree kill still apply).
    pub fn new(max_processes: u32, max_memory: Option<usize>) -> std::io::Result<Job> {
        // SAFETY: null attributes and name create an anonymous job.
        let h = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if h.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Job(h);
        let info = ExtendedLimits {
            basic: BasicLimits {
                limit_flags: LIMIT_KILL_ON_JOB_CLOSE
                    | LIMIT_DIE_ON_UNHANDLED_EXCEPTION
                    | LIMIT_ACTIVE_PROCESS
                    | if max_memory.is_some() {
                        LIMIT_JOB_MEMORY
                    } else {
                        0
                    },
                active_process_limit: max_processes,
                ..BasicLimits::default()
            },
            job_memory_limit: max_memory.unwrap_or(0),
            ..ExtendedLimits::default()
        };
        // SAFETY: `info` is a correctly laid-out
        // JOBOBJECT_EXTENDED_LIMIT_INFORMATION alive for the call.
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                (&info as *const ExtendedLimits).cast(),
                std::mem::size_of::<ExtendedLimits>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Put a child started with [`CREATE_SUSPENDED`] in the job, then let
    /// it run. On failure the child is killed — it never runs outside.
    pub fn adopt(&self, child: &mut std::process::Child) -> std::io::Result<()> {
        let process = child.as_raw_handle() as Handle;
        // SAFETY: both handles are valid for the duration of the call.
        if unsafe { AssignProcessToJobObject(self.0, process) } == 0 {
            let e = std::io::Error::last_os_error();
            let _ = child.kill();
            return Err(e);
        }
        // SAFETY: a valid process handle; NtResumeProcess resumes every
        // thread of a process created suspended.
        let status = unsafe { NtResumeProcess(process) };
        if status < 0 {
            let _ = child.kill();
            return Err(std::io::Error::other(format!(
                "NtResumeProcess failed (0x{status:08x})"
            )));
        }
        Ok(())
    }

    /// Kill everything in the job now.
    pub fn kill_all(&self) {
        // SAFETY: a valid job handle.
        unsafe { TerminateJobObject(self.0, 1) };
    }
}

#[repr(C)]
#[derive(Default)]
struct BasicAccounting {
    total_user_time: i64,
    total_kernel_time: i64,
    this_period_user_time: i64,
    this_period_kernel_time: i64,
    page_faults: u32,
    total_processes: u32,
    active_processes: u32,
    terminated_processes: u32,
}

#[repr(C)]
#[derive(Default)]
struct LimitViolation {
    limit_flags: u32,
    violation_limit_flags: u32,
    io_read_bytes: u64,
    io_read_bytes_limit: u64,
    io_write_bytes: u64,
    io_write_bytes_limit: u64,
    per_job_user_time: i64,
    per_job_user_time_limit: i64,
    job_memory: u64,
    job_memory_limit: u64,
    rate_tolerance: u32,
    rate_tolerance_limit: u32,
}

impl Job {
    /// What the job has seen and which of its limits were hit, in words —
    /// for an error report when a worker fails inside its job.
    pub fn report(&self) -> String {
        let mut acct = BasicAccounting::default();
        let mut viol = LimitViolation::default();
        // SAFETY: both structs are correctly laid out for their classes
        // (1: BasicAccountingInformation, 13: LimitViolationInformation)
        // and alive for the calls.
        let (a, v) = unsafe {
            (
                QueryInformationJobObject(
                    self.0,
                    1,
                    (&mut acct as *mut BasicAccounting).cast(),
                    std::mem::size_of::<BasicAccounting>() as u32,
                    std::ptr::null_mut(),
                ),
                QueryInformationJobObject(
                    self.0,
                    13,
                    (&mut viol as *mut LimitViolation).cast(),
                    std::mem::size_of::<LimitViolation>() as u32,
                    std::ptr::null_mut(),
                ),
            )
        };
        let mut out = String::new();
        if a != 0 {
            out.push_str(&format!(
                "processes: {} started, {} active, {} ended",
                acct.total_processes, acct.active_processes, acct.terminated_processes
            ));
        }
        if v != 0 && viol.violation_limit_flags != 0 {
            let mut hit = Vec::new();
            if viol.violation_limit_flags & LIMIT_JOB_MEMORY != 0 {
                hit.push(format!(
                    "job memory ({} of {} bytes)",
                    viol.job_memory, viol.job_memory_limit
                ));
            }
            if viol.violation_limit_flags & LIMIT_ACTIVE_PROCESS != 0 {
                hit.push("active process count".to_string());
            }
            if hit.is_empty() {
                hit.push(format!("flags 0x{:x}", viol.violation_limit_flags));
            }
            out.push_str(&format!("; limit hit: {}", hit.join(", ")));
        }
        out
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle; closing it with KILL_ON_JOB_CLOSE ends
        // whatever is still running in the job.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::time::{Duration, Instant};

    /// The worker's grandchildren die with the job (the gap: before this,
    /// killing a native worker left its children running).
    #[test]
    fn closing_the_job_kills_the_whole_tree() {
        let job = Job::new(16, None).unwrap();
        // cmd starts a ping that would run ~30 s, as a grandchild.
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "ping -n 30 127.0.0.1 >NUL"])
            .creation_flags(CREATE_SUSPENDED | 0x0800_0000)
            .spawn()
            .unwrap();
        job.adopt(&mut child).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            child.try_wait().unwrap().is_none(),
            "cmd ended before the job was closed (its ping never started?): {}",
            job.report()
        );
        let started = Instant::now();
        drop(job);
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(started.elapsed() < Duration::from_secs(5));
        let still = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", child.id())])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&still.stdout).contains(&child.id().to_string()));
    }

    #[test]
    fn the_process_cap_holds() {
        let job = Job::new(1, None).unwrap();
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "cmd /c echo inner"])
            .creation_flags(CREATE_SUSPENDED | 0x0800_0000)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        job.adopt(&mut child).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("inner"),
            "a second process started"
        );
    }

    /// The limits real workers run under must still let a worker start its
    /// own children (node, git, python): before this test, a quota refusal
    /// made the tree-kill test pass for the wrong reason.
    #[test]
    fn workers_can_start_children_under_the_production_limits() {
        let job = Job::new(WORKER_MAX_PROCESSES, Some(WORKER_MAX_MEMORY)).unwrap();
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "ping -n 1 127.0.0.1"])
            .creation_flags(CREATE_SUSPENDED | 0x0800_0000)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        job.adopt(&mut child).unwrap();
        let out = child.wait_with_output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.status.success() && text.to_ascii_lowercase().contains("ttl"),
            "a child could not start inside the worker job: {text}\njob: {}",
            job.report()
        );
    }
}
