//! Windows Credential Manager (WR-06): `CredWriteW` / `CredReadW` /
//! `CredDeleteW` from advapi32, so a secret is stored in the user's
//! credential vault (DPAPI-protected, per user) instead of an environment
//! variable, and never appears in any process's argv.
//!
//! One of the files AGENTS.md allows `unsafe` in: these three calls are the
//! OS boundary. Every block carries a SAFETY comment; nothing else here is
//! unsafe.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

const CRED_TYPE_GENERIC: u32 = 1;
const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
const ERROR_NOT_FOUND: i32 = 1168;

#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
struct CredentialW {
    flags: u32,
    kind: u32,
    target_name: *mut u16,
    comment: *mut u16,
    last_written: FileTime,
    blob_size: u32,
    blob: *mut u8,
    persist: u32,
    attribute_count: u32,
    attributes: *mut c_void,
    target_alias: *mut u16,
    user_name: *mut u16,
}

#[link(name = "advapi32")]
extern "system" {
    fn CredWriteW(credential: *const CredentialW, flags: u32) -> i32;
    fn CredReadW(target: *const u16, kind: u32, flags: u32, out: *mut *mut CredentialW) -> i32;
    fn CredDeleteW(target: *const u16, kind: u32, flags: u32) -> i32;
    fn CredFree(buffer: *mut c_void);
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

/// The vault entry name for a secret path: `wright:<path>`.
pub fn target(path: &str) -> String {
    format!("wright:{path}")
}

/// Store `value` under `target(path)`, replacing any previous value.
pub fn write(path: &str, value: &[u8]) -> Result<(), String> {
    if value.len() > 5 * 512 {
        return Err("the value is longer than Credential Manager allows (2560 bytes)".into());
    }
    let mut name = wide(&target(path));
    let mut user = wide("wright");
    let mut blob = value.to_vec();
    let cred = CredentialW {
        flags: 0,
        kind: CRED_TYPE_GENERIC,
        target_name: name.as_mut_ptr(),
        comment: std::ptr::null_mut(),
        last_written: FileTime { low: 0, high: 0 },
        blob_size: blob.len() as u32,
        blob: blob.as_mut_ptr(),
        persist: CRED_PERSIST_LOCAL_MACHINE,
        attribute_count: 0,
        attributes: std::ptr::null_mut(),
        target_alias: std::ptr::null_mut(),
        user_name: user.as_mut_ptr(),
    };
    // SAFETY: every pointer in `cred` points into a buffer owned by this
    // frame (`name`, `user`, `blob`), alive for the duration of the call;
    // CredWriteW copies what it keeps.
    let ok = unsafe { CredWriteW(&cred, 0) };
    blob.iter_mut().for_each(|b| *b = 0);
    if ok == 0 {
        return Err(format!(
            "Credential Manager refused to store the secret ({})",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// The value under `target(path)`, `Ok(None)` when there is none.
pub fn read(path: &str) -> Result<Option<Vec<u8>>, String> {
    let name = wide(&target(path));
    let mut out: *mut CredentialW = std::ptr::null_mut();
    // SAFETY: `name` is NUL-terminated UTF-16 alive for the call; `out`
    // receives a buffer the OS allocates, released below with CredFree.
    let ok = unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut out) };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(ERROR_NOT_FOUND) {
            return Ok(None);
        }
        return Err(format!(
            "Credential Manager could not read the secret ({err})"
        ));
    }
    // SAFETY: CredReadW succeeded, so `out` points at a valid CREDENTIALW
    // whose blob is `blob_size` bytes; both are copied before CredFree.
    let value = unsafe {
        let c = &*out;
        let v = if c.blob.is_null() || c.blob_size == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(c.blob, c.blob_size as usize).to_vec()
        };
        CredFree(out.cast());
        v
    };
    Ok(Some(value))
}

/// Remove `target(path)`. Removing what is not there is fine.
pub fn delete(path: &str) -> Result<(), String> {
    let name = wide(&target(path));
    // SAFETY: `name` is NUL-terminated UTF-16 alive for the call.
    let ok = unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(ERROR_NOT_FOUND) {
            return Err(format!(
                "Credential Manager could not delete the secret ({err})"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real round trip through the vault (GitHub's windows runner and the
    /// self-hosted Windows machine run this).
    #[test]
    fn round_trip_through_credential_manager() {
        let path = format!("test/wright-wincred-{}", std::process::id());
        assert_eq!(read(&path).unwrap(), None);
        write(&path, b"sk-test-value").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(&b"sk-test-value"[..]));
        write(&path, b"replaced").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(&b"replaced"[..]));
        delete(&path).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        delete(&path).unwrap();
    }
}
