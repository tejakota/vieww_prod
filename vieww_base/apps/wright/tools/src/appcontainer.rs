//! Windows AppContainer for native worker CLIs (WR-05, tier 2).
//!
//! A worker `.exe` runs inside the `wright.worker` AppContainer profile:
//!
//! * the throwaway copy is the only place it can write — AppContainer
//!   tokens are denied by default everywhere an ACE does not name the
//!   container, so the user's profile (`~/.ssh`, startup folders, the rest
//!   of `%USERPROFILE%`) is neither readable nor writable;
//! * the container's own package folder
//!   (`%LOCALAPPDATA%\Packages\wright.worker\AC`) is writable too, as
//!   for every AppContainer; it holds nothing of the user's;
//! * the program's own install folders are granted read + execute for the
//!   length of the run and revoked afterwards (the system folders are
//!   already readable by every AppContainer);
//! * network is the `internetClient` capability — a worker is a model
//!   client and must reach its vendor; nothing else (no LAN, no loopback
//!   servers: `privateNetworkClientServer` is not granted);
//! * it starts suspended and is resumed only once inside the worker's Job
//!   object ([`crate::winjob`]), so the process tree, memory and process
//!   count stay bounded and everything dies with the job.
//!
//! Batch shims (`.cmd`/`.bat`, how npm installs CLIs) are not launched
//! here: starting them means handing the prompt to `cmd.exe`'s parser, and
//! the standard library's careful escaping for that case is the safer
//! path. Those workers keep tier 1 and the report says so.
//!
//! One of the files AGENTS.md allows `unsafe` in. Every block carries a
//! SAFETY comment; nothing else here is unsafe.

use std::ffi::{c_void, OsStr};
use std::fs::File;
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::winjob::{Handle, Job};

const PROFILE: &str = "wright.worker";
const INTERNET_CLIENT_SID: &str = "S-1-15-3-1";

const SE_FILE_OBJECT: i32 = 1;
const DACL_SECURITY_INFORMATION: u32 = 0x4;
const GRANT_ACCESS: i32 = 1;
const REVOKE_ACCESS: i32 = 4;
const SUB_CONTAINERS_AND_OBJECTS_INHERIT: u32 = 0x3;
const TRUSTEE_IS_SID: i32 = 0;
const TRUSTEE_IS_UNKNOWN: i32 = 0;
/// FILE_ALL_ACCESS.
pub const ACCESS_READ_WRITE: u32 = 0x001F_01FF;
/// FILE_GENERIC_READ | FILE_GENERIC_EXECUTE.
pub const ACCESS_READ_EXECUTE: u32 = 0x0012_00A9;

const SE_GROUP_ENABLED: u32 = 0x4;
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
const PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES: usize = 0x0002_0009;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
const HANDLE_FLAG_INHERIT: u32 = 0x1;
const GENERIC_READ: u32 = 0x8000_0000;
const FILE_SHARE_READ_WRITE: u32 = 0x3;
const OPEN_EXISTING: u32 = 3;
const WAIT_OBJECT_0: u32 = 0;
const ERROR_ALREADY_EXISTS_HRESULT: i32 = 0x8007_00B7_u32 as i32;

#[repr(C)]
struct Trustee {
    multiple_trustee: *mut c_void,
    multiple_trustee_operation: i32,
    form: i32,
    kind: i32,
    name: *mut c_void,
}

#[repr(C)]
struct ExplicitAccess {
    permissions: u32,
    mode: i32,
    inheritance: u32,
    trustee: Trustee,
}

#[repr(C)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: u32,
}

#[repr(C)]
struct SecurityCapabilities {
    app_container_sid: *mut c_void,
    capabilities: *mut SidAndAttributes,
    capability_count: u32,
    reserved: u32,
}

#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut c_void,
    inherit: i32,
}

#[repr(C)]
struct StartupInfo {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    reserved2_len: u16,
    reserved2: *mut u8,
    std_input: Handle,
    std_output: Handle,
    std_error: Handle,
}

#[repr(C)]
struct StartupInfoEx {
    startup: StartupInfo,
    attributes: *mut c_void,
}

#[repr(C)]
struct ProcessInformation {
    process: Handle,
    thread: Handle,
    process_id: u32,
    thread_id: u32,
}

#[link(name = "userenv")]
extern "system" {
    fn CreateAppContainerProfile(
        name: *const u16,
        display_name: *const u16,
        description: *const u16,
        capabilities: *const SidAndAttributes,
        capability_count: u32,
        sid: *mut *mut c_void,
    ) -> i32;
    fn DeriveAppContainerSidFromAppContainerName(name: *const u16, sid: *mut *mut c_void) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut c_void) -> i32;
    fn FreeSid(sid: *mut c_void) -> *mut c_void;
    fn GetNamedSecurityInfoW(
        name: *const u16,
        object_type: i32,
        info: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn SetEntriesInAclW(
        count: u32,
        entries: *const ExplicitAccess,
        old: *mut c_void,
        new: *mut *mut c_void,
    ) -> u32;
    fn SetNamedSecurityInfoW(
        name: *const u16,
        object_type: i32,
        info: u32,
        owner: *mut c_void,
        group: *mut c_void,
        dacl: *mut c_void,
        sacl: *mut c_void,
    ) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(mem: *mut c_void) -> *mut c_void;
    fn CreatePipe(
        read: *mut Handle,
        write: *mut Handle,
        attributes: *const SecurityAttributes,
        size: u32,
    ) -> i32;
    fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        attributes: *const SecurityAttributes,
        disposition: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *const c_void,
        size: usize,
        previous: *mut c_void,
        returned: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateProcessW(
        application: *const u16,
        command_line: *mut u16,
        process_attributes: *const c_void,
        thread_attributes: *const c_void,
        inherit_handles: i32,
        flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup: *const StartupInfoEx,
        info: *mut ProcessInformation,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, ms: u32) -> u32;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
    fn TerminateProcess(process: Handle, code: u32) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn last_error(what: &str) -> std::io::Error {
    let e = std::io::Error::last_os_error();
    std::io::Error::new(e.kind(), format!("{what}: {e}"))
}

fn win32(what: &str, code: u32) -> std::io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::Error::from_raw_os_error(code as i32).kind(),
            format!("{what}: {}", std::io::Error::from_raw_os_error(code as i32)),
        ))
    }
}

/// An owned handle, closed on drop.
struct Owned(Handle);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 as isize != -1 {
            // SAFETY: we own the handle and close it once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// The `wright.worker` container's SID.
struct ContainerSid(*mut c_void);

impl ContainerSid {
    fn get() -> std::io::Result<ContainerSid> {
        let name = wide(OsStr::new(PROFILE));
        let display = wide(OsStr::new("wright worker"));
        let mut sid = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated strings, no capabilities, an out
        // pointer for the SID (freed with FreeSid in Drop).
        let hr = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                display.as_ptr(),
                display.as_ptr(),
                std::ptr::null(),
                0,
                &mut sid,
            )
        };
        if hr == ERROR_ALREADY_EXISTS_HRESULT {
            // SAFETY: as above.
            let hr = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
            if hr < 0 {
                return Err(std::io::Error::other(format!(
                    "DeriveAppContainerSidFromAppContainerName failed (0x{hr:08x})"
                )));
            }
        } else if hr < 0 {
            return Err(std::io::Error::other(format!(
                "CreateAppContainerProfile failed (0x{hr:08x})"
            )));
        }
        Ok(ContainerSid(sid))
    }
}

impl Drop for ContainerSid {
    fn drop(&mut self) {
        // SAFETY: a SID returned by the profile APIs, freed once.
        unsafe { FreeSid(self.0) };
    }
}

/// Add (or remove) an inheritable ACE for `sid` on `path`.
fn edit_acl(path: &Path, sid: *mut c_void, access: u32, mode: i32) -> std::io::Result<()> {
    let name = wide(path.as_os_str());
    let mut old_dacl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: a NUL-terminated path; out pointers for the DACL and the
    // descriptor that owns it (freed with LocalFree below).
    win32("GetNamedSecurityInfoW", unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old_dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    })?;
    let entry = ExplicitAccess {
        permissions: access,
        mode,
        inheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        trustee: Trustee {
            multiple_trustee: std::ptr::null_mut(),
            multiple_trustee_operation: 0,
            form: TRUSTEE_IS_SID,
            kind: TRUSTEE_IS_UNKNOWN,
            name: sid,
        },
    };
    let mut new_dacl = std::ptr::null_mut();
    // SAFETY: one valid entry; `old_dacl` points into `descriptor`, alive
    // until LocalFree; `new_dacl` is allocated by the call.
    let set = unsafe { SetEntriesInAclW(1, &entry, old_dacl, &mut new_dacl) };
    let result = win32("SetEntriesInAclW", set).and_then(|()| {
        // SAFETY: a NUL-terminated path and the DACL just built.
        win32("SetNamedSecurityInfoW", unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                new_dacl,
                std::ptr::null_mut(),
            )
        })
    });
    // SAFETY: both were allocated by the calls above (null is fine).
    unsafe {
        LocalFree(new_dacl);
        LocalFree(descriptor);
    }
    result
}

/// Folders opened to the container for one run; revoked on drop (the
/// writable copy is deleted by its owner, so only read grants revert).
struct Grants {
    sid: ContainerSid,
    revoke: Vec<PathBuf>,
}

impl Grants {
    fn new(writable: &Path, readable: &[PathBuf]) -> std::io::Result<Grants> {
        let sid = ContainerSid::get()?;
        edit_acl(writable, sid.0, ACCESS_READ_WRITE, GRANT_ACCESS)?;
        let mut grants = Grants {
            sid,
            revoke: Vec::new(),
        };
        for dir in readable {
            if is_system_dir(dir) || !dir.is_dir() {
                continue;
            }
            edit_acl(dir, grants.sid.0, ACCESS_READ_EXECUTE, GRANT_ACCESS)?;
            grants.revoke.push(dir.clone());
        }
        Ok(grants)
    }
}

impl Drop for Grants {
    fn drop(&mut self) {
        for dir in &self.revoke {
            let _ = edit_acl(dir, self.sid.0, 0, REVOKE_ACCESS);
        }
    }
}

/// Under `%SystemRoot%` or Program Files: every AppContainer reads these
/// already, and their ACLs are not ours to edit.
fn is_system_dir(dir: &Path) -> bool {
    [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
    ]
    .iter()
    .filter_map(std::env::var_os)
    .any(|root| dir.starts_with(Path::new(&root)))
}

/// Whether this program can run in the container (a PE executable, not a
/// batch shim).
pub fn can_contain(program: &Path) -> bool {
    program
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("com"))
}

/// Quote one argument the way `CommandLineToArgvW` / the MSVC runtime
/// parse it back.
fn quote_arg(arg: &str, out: &mut String) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
}

fn command_line(program: &Path, args: &[&str]) -> Vec<u16> {
    let mut line = String::new();
    quote_arg(&program.to_string_lossy(), &mut line);
    for a in args {
        line.push(' ');
        quote_arg(a, &mut line);
    }
    wide(OsStr::new(&line))
}

/// A sorted, double-NUL-terminated UTF-16 environment block.
fn environment_block(env: &[(String, String)]) -> Vec<u16> {
    let mut vars: Vec<&(String, String)> = env.iter().collect();
    vars.sort_by_key(|(k, _)| k.to_ascii_uppercase());
    let mut block = Vec::new();
    for (k, v) in vars {
        block.extend(OsStr::new(&format!("{k}={v}")).encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// One pipe end the child inherits, and our end as a `File`.
fn pipe() -> std::io::Result<(File, Owned)> {
    let sa = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        descriptor: std::ptr::null_mut(),
        inherit: 1,
    };
    let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: out pointers for two new handles.
    if unsafe { CreatePipe(&mut read, &mut write, &sa, 0) } == 0 {
        return Err(last_error("CreatePipe"));
    }
    // SAFETY: `read` is ours alone (not inherited) from here on.
    unsafe { SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0) };
    // SAFETY: a fresh handle we own, handed to File which closes it.
    let file = unsafe { File::from_raw_handle(read) };
    Ok((file, Owned(write)))
}

/// What a contained run produced.
pub struct Outcome {
    pub success: bool,
    pub exit_code: u32,
    pub output: String,
    pub timed_out: bool,
    pub stopped: bool,
}

/// Run `program` in the container with `writable` as its only writable
/// folder, inside `job`, until it exits, `timeout` passes, or `stop()`
/// says so. `env` is the whole environment (nothing is inherited).
#[allow(clippy::too_many_arguments)]
pub fn run(
    program: &Path,
    args: &[&str],
    writable: &Path,
    readable: &[PathBuf],
    env: &[(String, String)],
    job: &Job,
    timeout: Duration,
    stop: &dyn Fn() -> bool,
) -> std::io::Result<Outcome> {
    let grants = Grants::new(writable, readable)?;

    let internet_text = wide(OsStr::new(INTERNET_CLIENT_SID));
    let mut internet = std::ptr::null_mut();
    // SAFETY: a NUL-terminated SID string; the SID is LocalFree'd below.
    if unsafe { ConvertStringSidToSidW(internet_text.as_ptr(), &mut internet) } == 0 {
        return Err(last_error("ConvertStringSidToSidW"));
    }
    struct LocalSid(*mut c_void);
    impl Drop for LocalSid {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSidToSidW.
            unsafe { LocalFree(self.0) };
        }
    }
    let internet = LocalSid(internet);
    let mut capability = SidAndAttributes {
        sid: internet.0,
        attributes: SE_GROUP_ENABLED,
    };
    let capabilities = SecurityCapabilities {
        app_container_sid: grants.sid.0,
        capabilities: &mut capability,
        capability_count: 1,
        reserved: 0,
    };

    let (stdout, out_w) = pipe()?;
    let (stderr, err_w) = pipe()?;
    let nul = wide(OsStr::new("NUL"));
    let sa = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        descriptor: std::ptr::null_mut(),
        inherit: 1,
    };
    // SAFETY: opening the NUL device read-only, inheritable.
    let stdin = Owned(unsafe {
        CreateFileW(
            nul.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ_WRITE,
            &sa,
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    });
    if stdin.0 as isize == -1 {
        return Err(last_error("open NUL"));
    }
    let inherited = [stdin.0, out_w.0, err_w.0];

    // The attribute list: security capabilities + exactly the three std
    // handles to inherit (no other inheritable handle of ours leaks in).
    let mut size = 0usize;
    // SAFETY: a size query with a null list, as documented.
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut size) };
    let mut list_buf = vec![0u64; size.div_ceil(8)];
    let list = list_buf.as_mut_ptr().cast::<c_void>();
    // SAFETY: a buffer of at least `size` bytes, 8-aligned.
    if unsafe { InitializeProcThreadAttributeList(list, 2, 0, &mut size) } == 0 {
        return Err(last_error("InitializeProcThreadAttributeList"));
    }
    struct List(*mut c_void);
    impl Drop for List {
        fn drop(&mut self) {
            // SAFETY: initialised above, deleted once.
            unsafe { DeleteProcThreadAttributeList(self.0) };
        }
    }
    let list_guard = List(list);
    // SAFETY: `capabilities` and `inherited` outlive CreateProcessW below,
    // which is the only reader of the list.
    unsafe {
        if UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            (&capabilities as *const SecurityCapabilities).cast(),
            std::mem::size_of::<SecurityCapabilities>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(last_error("security capabilities attribute"));
        }
        if UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            inherited.as_ptr().cast(),
            std::mem::size_of_val(&inherited),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(last_error("handle list attribute"));
        }
    }

    let startup = StartupInfoEx {
        startup: StartupInfo {
            cb: std::mem::size_of::<StartupInfoEx>() as u32,
            reserved: std::ptr::null_mut(),
            desktop: std::ptr::null_mut(),
            title: std::ptr::null_mut(),
            x: 0,
            y: 0,
            x_size: 0,
            y_size: 0,
            x_count_chars: 0,
            y_count_chars: 0,
            fill_attribute: 0,
            flags: STARTF_USESTDHANDLES,
            show_window: 0,
            reserved2_len: 0,
            reserved2: std::ptr::null_mut(),
            std_input: stdin.0,
            std_output: out_w.0,
            std_error: err_w.0,
        },
        attributes: list,
    };
    let application = wide(program.as_os_str());
    let mut line = command_line(program, args);
    // CreateProcessW builds an AppContainer's package folders from the
    // environment it is handed and fails with ERROR_ENVVAR_NOT_FOUND (203)
    // when LOCALAPPDATA is missing from it, so the real one is supplied
    // when the caller did not choose its own.
    let mut env = env.to_vec();
    if !env
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("LOCALAPPDATA"))
    {
        if let Ok(v) = std::env::var("LOCALAPPDATA") {
            env.push(("LOCALAPPDATA".into(), v));
        }
    }
    let block = environment_block(&env);
    let cwd = wide(writable.as_os_str());
    let mut info = ProcessInformation {
        process: std::ptr::null_mut(),
        thread: std::ptr::null_mut(),
        process_id: 0,
        thread_id: 0,
    };
    // SAFETY: every pointer is to a NUL-terminated buffer or struct owned
    // by this frame and alive for the call; the command line is mutable as
    // CreateProcessW requires.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW,
            block.as_ptr().cast(),
            cwd.as_ptr(),
            &startup,
            &mut info,
        )
    };
    // Read the error before anything else can overwrite it.
    let create_error = (created == 0).then(|| last_error("CreateProcessW in the AppContainer"));
    drop(list_guard);
    if let Some(e) = create_error {
        return Err(e);
    }
    let process = Owned(info.process);
    drop(Owned(info.thread));
    // Our copies of the child's ends: closed now, so the pipes end when
    // the child (and everything it started) is gone.
    drop((stdin, out_w, err_w));

    if let Err(e) = job.adopt_raw(process.0) {
        // SAFETY: a valid process handle we own.
        unsafe { TerminateProcess(process.0, 1) };
        return Err(e);
    }

    let drain = |mut f: File| {
        std::thread::spawn(move || {
            let mut keep = Vec::new();
            let mut buf = [0u8; 8192];
            while let Ok(n) = f.read(&mut buf) {
                if n == 0 {
                    break;
                }
                keep.extend_from_slice(&buf[..n]);
                if keep.len() > 256 * 1024 {
                    keep.drain(..keep.len() - 128 * 1024);
                }
            }
            String::from_utf8_lossy(&keep).into_owned()
        })
    };
    let out_thread = drain(stdout);
    let err_thread = drain(stderr);

    let start = Instant::now();
    let (mut timed_out, mut stopped) = (false, false);
    loop {
        // SAFETY: a valid process handle.
        if unsafe { WaitForSingleObject(process.0, 20) } == WAIT_OBJECT_0 {
            break;
        }
        if stop() {
            stopped = true;
        } else if start.elapsed() > timeout {
            timed_out = true;
        } else {
            continue;
        }
        job.kill_all();
        // SAFETY: as above; bounded wait for the kill to land.
        unsafe { WaitForSingleObject(process.0, 5000) };
        break;
    }
    // Whatever the worker left running goes before its files are read.
    job.kill_all();
    let mut code = 1u32;
    // SAFETY: a valid process handle and an out pointer.
    unsafe { GetExitCodeProcess(process.0, &mut code) };
    let output = format!(
        "{}{}",
        out_thread.join().unwrap_or_default(),
        err_thread.join().unwrap_or_default()
    );
    drop(grants);
    Ok(Outcome {
        success: code == 0 && !timed_out && !stopped,
        exit_code: code,
        output,
        timed_out,
        stopped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-ac-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn base_env(copy: &Path) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = ["SystemRoot", "PATHEXT", "COMSPEC", "PATH"]
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect();
        for k in [
            "TEMP",
            "TMP",
            "USERPROFILE",
            "HOME",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            env.push((k.into(), copy.display().to_string()));
        }
        env
    }

    fn cmd(script: &str, copy: &Path, env: &[(String, String)]) -> Outcome {
        let comspec = PathBuf::from(std::env::var("COMSPEC").unwrap());
        let job = Job::new(16, None).unwrap();
        run(
            &comspec,
            &["/d", "/c", script],
            copy,
            &[],
            env,
            &job,
            Duration::from_secs(30),
            &|| false,
        )
        .expect("the container starts")
    }

    /// The worker escape suite's filesystem half: inside the copy works;
    /// outside it, neither writing nor reading the user's files does.
    #[test]
    fn the_copy_is_the_only_place_a_contained_worker_can_write_or_read() {
        let copy = temp("copy");
        let outside = temp("outside");
        std::fs::write(outside.join("private.txt"), "PRIVATE-CONTENT").unwrap();
        let env = base_env(&copy);

        let wrote = cmd("echo inside> inside.txt", &copy, &env);
        assert!(
            wrote.success,
            "writing inside the copy failed: {}",
            wrote.output
        );
        assert!(copy.join("inside.txt").is_file());

        let escaped = cmd(
            &format!(
                "echo escaped> \"{}\"",
                outside.join("planted.txt").display()
            ),
            &copy,
            &env,
        );
        assert!(
            !outside.join("planted.txt").exists(),
            "the worker wrote outside its copy: {}",
            escaped.output
        );

        let profile = std::env::var("USERPROFILE").unwrap();
        let startup = Path::new(&profile)
            .join(r"AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup");
        let planted = startup.join(format!("wright-ac-{}.cmd", std::process::id()));
        let _ = cmd(&format!("echo x> \"{}\"", planted.display()), &copy, &env);
        assert!(!planted.exists(), "the worker planted a startup item");

        let read = cmd(
            &format!("type \"{}\"", outside.join("private.txt").display()),
            &copy,
            &env,
        );
        assert!(
            !read.output.contains("PRIVATE-CONTENT"),
            "the worker read a file outside its copy: {}",
            read.output
        );

        let _ = std::fs::remove_dir_all(&copy);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// The environment is exactly what was passed: no wright secrets, no
    /// keychain references, nothing inherited.
    #[test]
    fn a_contained_worker_sees_only_the_environment_it_was_given() {
        let copy = temp("env");
        std::env::set_var("WRIGHT_SECRET_SHOULD_NOT_LEAK", "keychain:leak");
        let mut env = base_env(&copy);
        env.push(("WORKER_KEY".into(), "given".into()));
        let out = cmd("set", &copy, &env);
        assert!(out.success, "{}", out.output);
        assert!(out.output.contains("WORKER_KEY=given"), "{}", out.output);
        assert!(
            !out.output.contains("WRIGHT_SECRET_SHOULD_NOT_LEAK"),
            "an inherited variable reached the worker: {}",
            out.output
        );
        let _ = std::fs::remove_dir_all(&copy);
    }

    /// A worker that outlives its clock is killed with its tree.
    #[test]
    fn a_contained_worker_past_its_timeout_is_killed() {
        let copy = temp("timeout");
        let env = base_env(&copy);
        let comspec = PathBuf::from(std::env::var("COMSPEC").unwrap());
        let job = Job::new(16, None).unwrap();
        let started = Instant::now();
        let out = run(
            &comspec,
            // An endless loop in cmd itself: ping (the Job test's sleeper)
            // exits at once inside a container, which has no loopback.
            &["/d", "/c", "for /l %i in (1,0,2) do @rem"],
            &copy,
            &[],
            &env,
            &job,
            Duration::from_secs(2),
            &|| false,
        )
        .unwrap();
        assert!(
            out.timed_out && !out.success,
            "the worker ended on its own (exit {}): {}",
            out.exit_code,
            out.output
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&copy);
    }

    #[test]
    fn arguments_survive_quoting() {
        let mut s = String::new();
        quote_arg(r#"a "quoted" \path\ with spaces\"#, &mut s);
        assert_eq!(s, r#""a \"quoted\" \path\ with spaces\\""#);
        let mut s = String::new();
        quote_arg("plain", &mut s);
        assert_eq!(s, "plain");
        let mut s = String::new();
        quote_arg("", &mut s);
        assert_eq!(s, r#""""#);
    }
}
