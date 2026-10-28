//! `Copy diagnostics` (ST-17): everything a supporter asks for, in one
//! paste.
//!
//! The About box already copies version, build, target and data directory.
//! A support conversation needs more than that, and it used to be a series
//! of round trips. This module adds:
//!
//! * **Toolchains**: every target, whether it can build, and what is
//!   missing;
//! * **Problems**: the most recent diagnostics. Each one gives the file,
//!   line, severity, code and rustc's message, cut at 200 characters;
//! * **Session**: the workspace path, the open buffer *names*, and the last
//!   render time.
//!
//! **What it never contains: buffer text.** It takes no buffer contents,
//! only names, and `tests/diagnostics.rs` checks that a secret typed into
//! an open file is absent from the report. Nothing is sent anywhere: the
//! report goes to the clipboard, and the user decides where it goes next.
//! There is no telemetry. If there ever is, it will be opt-in, buffered
//! locally, and flushed to one documented endpoint.

use crate::state::{Severity, Studio};

/// The most recent problems a report carries.
pub const MAX_PROBLEMS: usize = 20;
/// A problem message is cut at this many characters.
const MAX_MESSAGE: usize = 200;

/// The diagnostics report for `studio`, as text.
#[must_use]
pub fn report(studio: &Studio) -> String {
    let mut out = crate::about::report();

    out.push_str("\nToolchains\n");
    for r in studio.toolchains.get().iter() {
        let state = if let Some(why) = r.impossible {
            format!("impossible here ({why})")
        } else if r.ready() {
            "ready".to_string()
        } else {
            let missing: Vec<&str> = r
                .requirements
                .iter()
                .filter(|q| !q.satisfied())
                .map(|q| q.name)
                .collect();
            format!("missing {}", missing.join(", "))
        };
        out.push_str(&format!("  {:<10} {state}\n", r.target.title()));
    }

    let problems = studio.diagnostics.get();
    out.push_str(&format!(
        "\nProblems ({} total, newest {} shown)\n",
        problems.len(),
        problems.len().min(MAX_PROBLEMS)
    ));
    if problems.is_empty() {
        out.push_str("  none\n");
    }
    for d in problems.iter().rev().take(MAX_PROBLEMS) {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let msg: String = d
            .message
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(MAX_MESSAGE)
            .collect();
        let code = if d.code.is_empty() {
            String::new()
        } else {
            format!("[{}] ", d.code)
        };
        out.push_str(&format!(
            "  {}:{}:{} {sev} {code}{msg}\n",
            d.file, d.line, d.column
        ));
    }

    out.push_str("\nSession\n");
    out.push_str(&format!(
        "  Workspace   {}\n",
        studio
            .root
            .get()
            .map_or_else(|| "none open".to_string(), |p| p.display().to_string())
    ));
    out.push_str(&format!(
        "  Last render {}\n",
        studio.last_render.get().map_or_else(
            || "none yet".to_string(),
            |d| format!("{} ms", d.as_millis())
        )
    ));
    let names: Vec<String> = studio
        .buffers
        .get()
        .iter()
        .map(|b| format!("{}{}", b.name, if b.dirty { " (unsaved)" } else { "" }))
        .collect();
    out.push_str(&format!(
        "  Open files  {}\n",
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(", ")
        }
    ));
    out
}
