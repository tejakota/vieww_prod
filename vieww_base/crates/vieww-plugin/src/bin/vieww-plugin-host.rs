//! The child half of `PluginRegistry::spawn_sandboxed` (FW-17): load one
//! plugin with the ordinary in-process loader and serve its calls over
//! stdin/stdout. See `vieww_plugin::sandbox` for the wire and the ADR for
//! why. Std-only, like the crate.
//!
//! # The protocol owns stdout; the plugin does not
//!
//! A plugin that `println!`s would otherwise write into the reply stream and
//! corrupt it. So before anything is loaded the protocol moves to a
//! duplicate of stdout, and the process's stdout is pointed at stderr: a
//! plugin's prints reach the operator's console, the parent's pipe carries
//! only frames.

use std::io::Write;

use vieww_plugin::sandbox::{encode_error, read_frame, write_frame, FIELD, ITEM};
use vieww_plugin::{stderr_host_vtable, PluginRegistry};

/// Requests are tiny (a path, a command id); anything bigger is not ours.
const MAX_REQUEST: usize = 64 * 1024;

fn main() {
    apply_memory_limit();
    let mut replies = take_stdout();
    let mut requests = std::io::stdin().lock();
    let host = stderr_host_vtable();
    let mut registry = PluginRegistry::new();
    loop {
        let Ok(request) = read_frame(&mut requests, MAX_REQUEST) else {
            // The parent went away (or sent garbage): nothing left to serve.
            return;
        };
        let mut fields = request.split(FIELD);
        let reply = match fields.next() {
            Some("load") if registry.is_empty() => {
                let path = fields.next().unwrap_or_default();
                // SAFETY: the same contract as an in-process load — the
                // parent asked for this library by path, and loading it in
                // this disposable process is the point of the sandbox.
                match unsafe { registry.load(path, &host) } {
                    Ok(plugin) => {
                        let mut out =
                            format!("loaded{FIELD}{}{FIELD}{}", plugin.name(), plugin.version());
                        for command in plugin.commands() {
                            out.push(FIELD);
                            out.push_str(&command.id);
                            out.push(ITEM);
                            out.push_str(&command.title);
                        }
                        out
                    }
                    Err(error) => encode_error(&error),
                }
            }
            Some("invoke") => {
                let id = fields.next().unwrap_or_default();
                match registry.plugins_mut().first_mut() {
                    Some(plugin) => match plugin.invoke(id) {
                        Ok(()) => "ok".to_owned(),
                        Err(error) => encode_error(&error),
                    },
                    None => format!("err{FIELD}load{FIELD}nothing is loaded"),
                }
            }
            Some("shutdown") => {
                // Dropping the registry runs the plugin's own shutdown.
                drop(registry);
                return;
            }
            _ => format!("err{FIELD}load{FIELD}an unknown request"),
        };
        if write_frame(&mut replies, &reply).is_err() {
            return;
        }
    }
}

/// The protocol's own handle on the parent's pipe, with fd/handle 1 pointed
/// at stderr from here on.
fn take_stdout() -> Box<dyn Write> {
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd as _;
        unsafe extern "C" {
            fn dup(fd: i32) -> i32;
            fn dup2(old: i32, new: i32) -> i32;
        }
        // SAFETY: plain descriptor calls on the process's own standard
        // descriptors; `protocol` is a fresh descriptor this function owns.
        let protocol = unsafe { dup(1) };
        if protocol >= 0 && unsafe { dup2(2, 1) } >= 0 {
            // SAFETY: `protocol` is open and owned by nothing else.
            return Box::new(unsafe { std::fs::File::from_raw_fd(protocol) });
        }
        Box::new(std::io::stdout())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
        const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        unsafe extern "system" {
            fn SetStdHandle(which: u32, handle: *mut std::ffi::c_void) -> i32;
        }
        let pipe = std::io::stdout().as_raw_handle();
        let err = std::io::stderr().as_raw_handle();
        // SAFETY: the protocol keeps the original stdout handle (owned from
        // here on, never closed by std's stdout again once the standard
        // handle is redirected); the standard handle now names stderr.
        unsafe {
            if SetStdHandle(STD_OUTPUT_HANDLE, err) != 0 {
                return Box::new(std::fs::File::from_raw_handle(pipe));
            }
        }
        Box::new(std::io::stdout())
    }
    #[cfg(not(any(unix, windows)))]
    {
        Box::new(std::io::stdout())
    }
}

/// `VIEWW_PLUGIN_MEMORY_LIMIT` bytes as this process's address-space cap,
/// set before the plugin loads (so the plugin cannot raise what it was
/// given above the hard limit). Unix only; elsewhere it is not applied.
fn apply_memory_limit() {
    let Some(limit) = std::env::var("VIEWW_PLUGIN_MEMORY_LIMIT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    else {
        return;
    };
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    {
        #[repr(C)]
        struct Rlimit {
            cur: u64,
            max: u64,
        }
        unsafe extern "C" {
            fn setrlimit(resource: i32, limit: *const Rlimit) -> i32;
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        const RLIMIT_AS: i32 = 9;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        const RLIMIT_AS: i32 = 5;
        let cap = Rlimit {
            cur: limit,
            max: limit,
        };
        // SAFETY: a valid struct for the duration of the call.
        if unsafe { setrlimit(RLIMIT_AS, &raw const cap) } != 0 {
            let _ = writeln!(
                std::io::stderr(),
                "vieww-plugin-host: could not apply the {limit}-byte memory limit"
            );
        }
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    {
        let _ = writeln!(
            std::io::stderr(),
            "vieww-plugin-host: memory limits are not enforced on this OS ({limit} bytes asked)"
        );
    }
}
