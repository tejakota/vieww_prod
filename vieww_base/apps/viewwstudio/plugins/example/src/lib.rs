//! The reference plugin: a lint rule, a palette command and an export
//! format, over the smallest ABI the studio speaks.
//!
//! # Copy this file to write a plugin
//!
//! The studio's side of the contract is `apps/viewwstudio/src/plugin.rs`,
//! and its module docs are the ABI's specification. This file is the other
//! half: a whole plugin in one file, with every rule the ABI asks of a
//! well-behaved one followed, so "write a plugin" starts from something that
//! is known to load rather than from a list of requirements.
//!
//! The rules, as this file follows them:
//!
//! * **Every symbol the ABI names is exported, and nothing else is.** A
//!   plugin that exports more is promising something nobody checks.
//! * **No function unwinds.** Each body is wrapped in `catch_unwind`; a
//!   panic that crossed `extern "C"` is undefined behaviour, and the studio
//!   would rather show a plugin that answered nothing than die for it.
//! * **Strings the studio passed in are borrowed, never kept, never freed.**
//!   Strings this plugin returns are allocated here and released only by
//!   this plugin's `vieww_plugin_free`.
//! * **The fingerprint is computed, not configured.** It is the same
//!   question the compiled preview answers — what is `vieww`'s `Text`'s
//!   `TypeId`? — which is why this crate depends on `vieww` and why it is a
//!   workspace member: a plugin built from any other dependency graph
//!   answers differently and the studio refuses it, for the reasons
//!   `loaded.rs` documented first.
//!
//! # What this particular plugin does
//!
//! Nothing ambitious, on purpose. A lint rule that flags TODO and FIXME
//! comments — a rule the studio's own four deliberately leave to whoever
//! wants them. A palette command that counts the TODOs in the open buffer
//! and says the number in the notice bar. An export format that packages
//! the project's own source as a `.tar.gz` with the system `tar`, refusing
//! when `tar` is not installed rather than planning a step that would fail
//! two minutes into a run.
//!
//! Each of the three is a demonstration of one plug point; together they are
//! a round trip: manifest in, findings and plans and notices out, memory
//! both sides agree about.

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The ABI version this plugin speaks. One number, kept beside the symbols
/// it describes, so the two cannot drift and a mismatch is a one-line diff.
const ABI_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// The ABI's bookkeeping: version, fingerprint, manifest, free
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn vieww_plugin_abi() -> u32 {
    ABI_VERSION
}

/// The plugin's answer to the question the preview's guard asks.
///
/// The studio compares this with its own computation over the same type, and
/// refuses the plugin when they differ. See the crate docs for why a plugin
/// that crosses no vieww types still has to answer it honestly.
#[no_mangle]
pub extern "C" fn vieww_plugin_fingerprint() -> u64 {
    use std::any::TypeId;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    TypeId::of::<vieww::prelude::Text>().hash(&mut hasher);
    hasher.finish()
}

/// What this plugin offers, in the JSON the ABI specifies.
#[no_mangle]
pub extern "C" fn vieww_plugin_manifest() -> *mut c_char {
    guarded(|| {
        // The reference manifest: every field the studio's parser reads, and
        // none it does not.
        let manifest = r#"{
  "name": "toolbelt",
  "lints": [
    { "rule": "todo-comment", "description": "TODO/FIXME comments left in the code" }
  ],
  "commands": [
    { "title": "Count TODOs", "detail": "toolbelt" }
  ],
  "formats": [
    { "id": "source-tarball", "title": "Source tarball", "detail": "the project's own source, packaged by tar" }
  ]
}"#;
        copy_out(manifest)
    })
}

/// Release a string this plugin returned.
///
/// # Safety
///
/// `text` must be a pointer `vieww_plugin_manifest`, `vieww_plugin_lint`,
/// `vieww_plugin_plan` or `vieww_plugin_run_command` returned, and must not
/// already have been freed. Null is allowed and ignored — the studio passes
/// back what it was given, and "nothing" is a valid answer.
#[no_mangle]
pub unsafe extern "C" fn vieww_plugin_free(text: *mut c_char) {
    if !text.is_null() {
        drop(CString::from_raw(text));
    }
}

// ---------------------------------------------------------------------------
// The lint rule
// ---------------------------------------------------------------------------

/// Run the rule called `rule` over `source`, answering findings as the ABI's
/// JSON array.
///
/// # Safety
///
/// `rule` is a NUL-terminated string the studio owns; `source` and
/// `source_len` are the buffer's bytes, borrowed for this call. Neither is
/// kept, freed, or written.
#[no_mangle]
pub unsafe extern "C" fn vieww_plugin_lint(
    rule: *const c_char,
    source: *const c_char,
    source_len: usize,
) -> *mut c_char {
    guarded(|| {
        let Some(rule) = borrow_str(rule) else {
            return copy_out("[]");
        };
        let Some(source) = borrow_bytes(source, source_len) else {
            return copy_out("[]");
        };
        match rule.as_str() {
            "todo-comment" => copy_out(&lint_todos(&source)),
            // A rule this plugin never advertised: the manifest and this
            // match are the two halves of one promise, and an unknown name
            // means they disagree. No findings rather than an error, because
            // the studio has no way to show a plugin's internal bug as
            // anything other than a broken buffer.
            _ => copy_out("[]"),
        }
    })
}

/// The findings, as the ABI's JSON.
///
/// A line's column is where the marker starts, because a finding that points
/// somewhere a person can put a caret is worth more than one that points at
/// the line's beginning.
fn lint_todos(source: &str) -> String {
    let mut findings = String::from("[");
    let mut first = true;
    for (index, line) in source.lines().enumerate() {
        let Some(marker) = todo_marker(line) else {
            continue;
        };
        if !first {
            findings.push(',');
        }
        first = false;
        let kind = if line.contains("TODO") { "TODO" } else { "FIXME" };
        findings.push_str(&format!(
            "{{\"rule\":\"todo-comment\",\"severity\":\"warning\",\
             \"message\":\"a {kind} marker was left behind — this is work somebody \
             meant to come back to\",\
             \"line\":{},\"column\":{}}}",
            index + 1,
            marker + 1
        ));
    }
    findings.push(']');
    findings
}

/// Where the TODO or FIXME marker starts on `line`, if there is one that
/// sits in a comment.
fn todo_marker(line: &str) -> Option<usize> {
    let comment = line.find("//")?;
    let after = &line[comment + 2..];
    let at = after.find("TODO").or_else(|| after.find("FIXME"))?;
    Some(comment + 2 + at)
}

// ---------------------------------------------------------------------------
// The palette command
// ---------------------------------------------------------------------------

/// Run the command called `command` over `source`, answering the ABI's JSON.
///
/// # Safety
///
/// As `vieww_plugin_lint`.
#[no_mangle]
pub unsafe extern "C" fn vieww_plugin_run_command(
    command: *const c_char,
    source: *const c_char,
    source_len: usize,
) -> *mut c_char {
    guarded(|| {
        let Some(command) = borrow_str(command) else {
            return null_answer();
        };
        let Some(source) = borrow_bytes(source, source_len) else {
            return null_answer();
        };
        match command.as_str() {
            "Count TODOs" => {
                let count = source.lines().filter(|l| todo_marker(l).is_some()).count();
                copy_out(&format!(
                    "{{\"notice\":\"toolbelt: {count} TODO/FIXME marker(s) in this buffer\"}}"
                ))
            }
            _ => null_answer(),
        }
    })
}

// ---------------------------------------------------------------------------
// The export format
// ---------------------------------------------------------------------------

/// Plan the format called `format` for the project at `root` named `name`,
/// answering the ABI's JSON — a plan, or a refusal in this plugin's words.
///
/// # Safety
///
/// `format` and `name` are NUL-terminated strings the studio owns; `root`
/// and `root_len` are the project directory's bytes, borrowed. As everywhere
/// in this ABI: nothing is kept, freed, or written.
#[no_mangle]
pub unsafe extern "C" fn vieww_plugin_plan(
    format: *const c_char,
    root: *const c_char,
    root_len: usize,
    name: *const c_char,
) -> *mut c_char {
    guarded(|| {
        let Some(format) = borrow_str(format) else {
            return null_answer();
        };
        let Some(root) = borrow_bytes(root, root_len) else {
            return null_answer();
        };
        let Some(name) = borrow_str(name) else {
            return null_answer();
        };
        match format.as_str() {
            "source-tarball" => copy_out(&plan_tarball(&root, &name)),
            _ => copy_out(&format!(
                "{{\"refusal\":\"{}\"}}",
                escape(&format!("toolbelt has no format called {format:?}"))
            )),
        }
    })
}

/// The tarball plan — or its refusal.
///
/// The refusal comes first, for the same reason the studio's own planner
/// refuses first: a step that fails two minutes into an export because `tar`
/// was never installed is a worse answer than a sentence that says so now.
/// This plugin cannot ask the studio's toolchain checklist — the ABI hands
/// it the project, not the environment — so it checks the one tool it needs
/// the way any shell script would.
fn plan_tarball(root: &str, name: &str) -> String {
    if !on_path("tar") {
        return format!(
            "{{\"refusal\":\"{}\"}}",
            escape("tar is not installed — the source tarball needs it. On Debian and \
                    Ubuntu: apt install tar; on macOS it ships with the system.")
        );
    }
    let export_dir = format!("{root}/target/export");
    let artefact = format!("{export_dir}/{name}-source.tar.gz");
    format!(
        "{{\"steps\":[\
           {{\"label\":\"Create export directory\",\"program\":\"mkdir\",\
             \"args\":[\"-p\",{}],\"why\":\"the tarball lands beside the studio's own exports\"}},\
           {{\"label\":\"Archive source\",\"program\":\"tar\",\
             \"args\":[\"-czf\",{},\"-C\",{},\"src\",\"Cargo.toml\"],\
             \"why\":\"the project's own source, packaged by the system tar\"}},\
         ],\"artefact\":{}}}",
        escape(&export_dir),
        escape(&artefact),
        escape(root),
        escape(&artefact)
    )
}

// ---------------------------------------------------------------------------
// The plumbing every plugin needs, kept at the bottom of the file
// ---------------------------------------------------------------------------

/// Run `body`, turning a panic into a null answer.
///
/// The one hard rule of the ABI's memory protocol: a pointer this plugin
/// promised and a panic instead means the studio is handed null, which every
/// reader treats as "no answer", rather than an unwind across `extern "C"`,
/// which is undefined behaviour.
fn guarded(body: impl FnOnce() -> *mut c_char) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| std::ptr::null_mut())
}

/// Copy `text` into memory this plugin owns, as the ABI's answer shape.
///
/// A caller that cannot represent its answer (a NUL byte inside it) answers
/// null rather than a truncated string — the studio reads null as "no
/// answer", and a silent half-answer is how protocol bugs look like data.
fn copy_out(text: &str) -> *mut c_char {
    CString::new(text)
        .map(|owned| owned.into_raw())
        .unwrap_or_else(|_| std::ptr::null_mut())
}

/// A null answer: no findings, no plan, no notice.
fn null_answer() -> *mut c_char {
    std::ptr::null_mut()
}

/// A NUL-terminated string the studio lent us, copied into memory this
/// plugin owns.
///
/// Copied rather than borrowed-and-kept: the ABI lends the pointer for the
/// call only, and a copy of a dozen bytes costs less than the lifetime it
/// would take to be honest about keeping it.
///
/// # Safety
///
/// `text` must be a valid NUL-terminated string for the duration of this
/// call — the ABI's contract, and the one thing this plugin asks of the
/// studio's side.
unsafe fn borrow_str(text: *const c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: the caller upholds the contract above.
    unsafe { CStr::from_ptr(text) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// A `(pointer, length)` pair the studio lent us, copied into memory this
/// plugin owns.
///
/// # Safety
///
/// `bytes` must be readable for `len` bytes for the duration of this call.
unsafe fn borrow_bytes(bytes: *const c_char, len: usize) -> Option<String> {
    if bytes.is_null() {
        return None;
    }
    // SAFETY: the caller upholds the contract above.
    let slice = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), len) };
    std::str::from_utf8(slice).ok().map(str::to_owned)
}

/// Escape `text` for embedding in a JSON string literal.
///
/// Hand-rolled for the same reason the studio's own `json` module is: two
/// characters need it, the plugin has no dependencies to add, and a JSON
/// writer is not one either.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Control characters JSON refuses to carry raw, spelled out
            // rather than dropped — a path with a bell in it is a path the
            // studio would otherwise read wrong.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether `program` is on `PATH`, the way a shell would find it.
///
/// The plugin's own refusal-first answer to "can this plan run": the ABI
/// gives it no environment report, so it looks, once, at plan time.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
    })
}
