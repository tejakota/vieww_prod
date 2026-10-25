//! Secrets: references in, values out at the last possible moment.
//!
//! A blueprint never holds a secret; it holds `keychain:<path>`. This
//! module turns a reference into a value right before use, and the
//! value's type ([`SecretValue`]) makes it hard to leak: it prints as
//! `<secret>`, and its bytes are overwritten when dropped.
//!
//! Where values come from, in order:
//!
//! 1. `WRIGHT_SECRET_<PATH>` environment variable — for CI and servers,
//!    where there is no desktop keychain. `<PATH>` is the reference path
//!    upper-cased with every non-alphanumeric character turned into `_`
//!    (`keychain:wright/hn-digest/smtp` -> `WRIGHT_SECRET_WRIGHT_HN_DIGEST_SMTP`).
//! 2. The OS keychain: macOS `security` and Linux `secret-tool`
//!    (libsecret / GNOME Keyring / KWallet), each fed on stdin; Windows
//!    Credential Manager in-process through `wincred.rs` (WR-06). No
//!    library, no crate.
//!
//! The event spine records THAT a secret was used, never its value.

use std::process::{Command, Stdio};

/// A resolved secret. Debug/Display never show the value.
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    pub fn new(v: impl Into<Vec<u8>>) -> Self {
        SecretValue(v.into())
    }

    /// The value, for exactly one use (a header, a stdin pipe).
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("")
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<secret>")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            // Volatile write so the optimizer cannot skip the wipe.
            // SAFETY-free alternative: std::hint::black_box after writing.
            *b = 0;
        }
        std::hint::black_box(&self.0);
    }
}

/// The path part of a `keychain:` reference, validated.
pub fn path_of(reference: &str) -> Result<&str, String> {
    let path = reference
        .strip_prefix("keychain:")
        .ok_or_else(|| format!("'{reference}' is not a keychain: reference"))?;
    if path.is_empty()
        || path.len() > 200
        || !path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c))
    {
        return Err(format!(
            "'{reference}' has an invalid path (letters, digits, / - _ . only)"
        ));
    }
    Ok(path)
}

/// The environment variable that can hold this secret.
pub fn env_name(reference: &str) -> Result<String, String> {
    let path = path_of(reference)?;
    Ok(format!(
        "WRIGHT_SECRET_{}",
        path.chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            })
            .collect::<String>()
    ))
}

/// Resolve a reference to its value.
pub fn resolve(reference: &str) -> Result<SecretValue, String> {
    let var = env_name(reference)?;
    if let Ok(v) = std::env::var(&var) {
        if !v.is_empty() {
            return Ok(SecretValue::new(v));
        }
    }
    let path = path_of(reference)?;
    // Windows: Credential Manager, in-process (WR-06).
    #[cfg(windows)]
    {
        return match crate::wincred::read(path)? {
            Some(v) if !v.is_empty() => Ok(SecretValue::new(v)),
            _ => Err(format!("{reference} is not in Credential Manager yet: run `wright secret set {reference}` (or set {var})")),
        };
    }
    #[allow(unreachable_code)]
    let out = if cfg!(target_os = "macos") {
        Command::new("security")
            .args(["find-generic-password", "-s", "wright", "-a", path, "-w"])
            .stderr(Stdio::null())
            .output()
    } else if cfg!(target_os = "linux") {
        Command::new("secret-tool")
            .args(["lookup", "service", "wright", "key", path])
            .stderr(Stdio::null())
            .output()
    } else {
        return Err(format!("{reference} is not set: set the {var} environment variable (no OS keychain on this platform)"));
    };
    match out {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => {
            let mut v = o.stdout;
            while v.last() == Some(&b'\n') {
                v.pop();
            }
            Ok(SecretValue::new(v))
        }
        Ok(_) => Err(format!("{reference} is not in your keychain yet: run `wright secret set {reference}` (or set {var})")),
        Err(_) => Err(format!(
            "{reference} cannot be read: the keychain tool ({}) is not installed — install it, or set {var}",
            if cfg!(target_os = "macos") { "security" } else { "secret-tool" }
        )),
    }
}

/// Store a value in the OS keychain (`wright secret set`). The value never
/// appears in any process's argv (WR-10): it is piped on stdin to
/// `secret-tool` on Linux, on macOS to `security -i` (which reads the
/// `add-generic-password` command line from stdin instead of argv), and on
/// Windows it goes to Credential Manager in-process (WR-06).
pub fn store(reference: &str, value: &str) -> Result<(), String> {
    let path = path_of(reference)?;
    // Windows: Credential Manager, in-process — no child process, no argv
    // (WR-06).
    #[cfg(windows)]
    {
        return crate::wincred::write(path, value.as_bytes());
    }
    #[allow(unreachable_code)]
    let status = if cfg!(target_os = "macos") {
        use std::io::Write;
        if value.contains('\n') || value.contains('\r') {
            return Err("the value contains a line break; keychain values must be one line".into());
        }
        let mut child = Command::new("security")
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not run the keychain tool: {e}"))?;
        let line = macos_add_command(path, value);
        child
            .stdin
            .take()
            .map(|mut s| s.write_all(line.as_bytes()))
            .transpose()
            .map_err(|e| e.to_string())?;
        child.wait()
    } else if cfg!(target_os = "linux") {
        use std::io::Write;
        let mut child = Command::new("secret-tool")
            .args(["store", "--label", &format!("Wright: {path}"), "service", "wright", "key", path])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|_| "secret-tool is not installed (install libsecret-tools), or use the WRIGHT_SECRET_ environment variable".to_string())?;
        child
            .stdin
            .take()
            .map(|mut s| s.write_all(value.as_bytes()))
            .transpose()
            .map_err(|e| e.to_string())?;
        child.wait()
    } else {
        return Err(
            "no OS keychain on this platform; use the WRIGHT_SECRET_ environment variable".into(),
        );
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("the keychain tool failed ({s})")),
        Err(e) => Err(format!("could not run the keychain tool: {e}")),
    }
}

/// The `security -i` command that stores `value` under `path`. Arguments
/// are double-quoted with `\` and `"` escaped, which is how `security`'s
/// interactive parser reads them.
fn macos_add_command(path: &str, value: &str) -> String {
    fn q(s: &str) -> String {
        let mut o = String::with_capacity(s.len() + 2);
        o.push('"');
        for c in s.chars() {
            if c == '"' || c == '\\' {
                o.push('\\');
            }
            o.push(c);
        }
        o.push('"');
        o
    }
    format!(
        "add-generic-password -U -s wright -a {} -w {}\n",
        q(path),
        q(value)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_command_quotes_the_value() {
        assert_eq!(
            macos_add_command("anthropic", r#"a"b\c d"#),
            "add-generic-password -U -s wright -a \"anthropic\" -w \"a\\\"b\\\\c d\"\n"
        );
    }

    #[test]
    fn env_names_are_derived_from_paths() {
        assert_eq!(
            env_name("keychain:wright/hn-digest/smtp").unwrap(),
            "WRIGHT_SECRET_WRIGHT_HN_DIGEST_SMTP"
        );
        assert!(env_name("plain").is_err());
        assert!(env_name("keychain:a b").is_err());
        assert!(
            env_name("keychain:$(rm -rf)").is_err(),
            "no shell metacharacters reach a command line"
        );
    }

    #[test]
    fn env_override_resolves_and_never_prints() {
        std::env::set_var("WRIGHT_SECRET_TEST_ONLY_KEY", "s3cret-value");
        let v = resolve("keychain:test/only-key").unwrap();
        assert_eq!(v.expose(), "s3cret-value");
        assert_eq!(format!("{v:?}"), "<secret>");
        std::env::remove_var("WRIGHT_SECRET_TEST_ONLY_KEY");
    }

    #[test]
    fn missing_secrets_say_how_to_fix_it() {
        let err = resolve("keychain:definitely/not/set/anywhere").unwrap_err();
        assert!(
            err.contains("WRIGHT_SECRET_DEFINITELY_NOT_SET_ANYWHERE"),
            "{err}"
        );
    }
}
