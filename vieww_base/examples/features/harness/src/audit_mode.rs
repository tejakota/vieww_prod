//! `VIEWW_AUDIT=1` — judge the example instead of showing it.
//!
//! ```console
//! VIEWW_AUDIT=1 cargo run -p feature-button-pressable
//! ```
//!
//! # Why the gallery is audited, not just looked at
//!
//! Every example under `examples/features/` is a picture of the widget
//! catalogue, and the repository's own rule is that a picture is how an
//! example is *checked*. But a screen reader never sees the picture: it sees
//! the semantics tree. `VIEWW_SHOT` proves an example draws; `VIEWW_AUDIT`
//! proves it *says* something — every control labelled, every text run it
//! painted judged for WCAG AA contrast against the backdrop it was actually
//! painted over ([`vieww_accessibility::audit`] for the tree,
//! [`vieww_accessibility::driver_text_contrast`] for the scene). A gallery
//! that ships an unlabelled button is teaching every reader of the example
//! to ship one too, which is why the finding fails the run rather than
//! scrolling past.
//!
//! # What a failure means, and what it does not
//!
//! The two halves are the same ones `docs/guide/accessibility.md` tells an
//! application author to run; nothing here is example-specific. Skipped text
//! runs (gradients, layers, images behind text) are reported but never fail
//! anything — they are "a renderer could tell you", not "this example is
//! wrong", and the count is printed so a skip that quietly swallows the
//! whole gallery would at least be visible in the log.
//!
//! # Determinism
//!
//! The audit happens at a fixed point in a fixed clock: the frame at zero,
//! then — if the tree is still animating — sixty frames a second up to one
//! second, then every step the example declared, at its own timestamp. No
//! wall clock, no display, no input: the same tree every run, on any
//! machine, which is what makes this a gate rather than a flake.

use std::time::Duration;

use vieww_accessibility::{audit, driver_text_contrast, Severity};
use vieww_foundation::Size;
use vieww_render::FrameDriver;

use crate::{run_starts, Steps};

/// The point in the example's clock the audited frame is taken at.
const AUDIT_MS: f64 = 1000.0;

/// Whether this run was started as an audit (`VIEWW_AUDIT` in the
/// environment).
#[must_use]
pub(crate) fn requested() -> bool {
    std::env::var_os("VIEWW_AUDIT").is_some()
}

/// Run the audits and return the verdict as the example's own `main` result:
/// `Ok` when the example is clean, an `Err` whose text is the findings.
///
/// The frame clock is advanced exactly as [`run`](self)'s doc describes, so
/// the audited tree is the same on every machine.
pub(crate) fn run<F>(title: &str, size: Size, build: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(&mut FrameDriver) -> Steps,
{
    let mut driver = FrameDriver::new(size);
    let steps = build(&mut driver);
    run_starts(Duration::ZERO);

    driver.draw_frame_at(Duration::ZERO);
    if driver.is_animating() {
        const STEP_MS: f64 = 1000.0 / 60.0;
        let mut at = 0.0;
        while at < AUDIT_MS {
            at = (at + STEP_MS).min(AUDIT_MS);
            driver.draw_frame_at(Duration::from_secs_f64(at / 1000.0));
        }
    }
    // The example's declared steps, at the timestamps they asked for: the
    // state a control was *set* to is part of what the tree should be
    // judged in, not something the audit should skip.
    let mut clock = 0.0_f64;
    for (_, at, step) in &steps {
        #[allow(clippy::cast_precision_loss)]
        let due = f64::from(*at as u32);
        while clock < due {
            clock = (clock + 1000.0 / 60.0).min(due);
            driver.draw_frame_at(Duration::from_secs_f64(clock / 1000.0));
        }
        step(&mut driver);
        driver.draw_frame_at(Duration::from_secs_f64(clock / 1000.0));
    }

    let findings = audit(&driver.semantics());
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    let warnings = findings.len() - errors;
    for finding in &findings {
        let tag = if finding.severity == Severity::Error {
            "ERROR"
        } else {
            "warning"
        };
        println!(
            "audit: {title}: [{tag}] node {}: {}",
            finding.node_id, finding.message
        );
    }

    let contrast = driver_text_contrast(&driver);
    for failure in &contrast.failures {
        println!(
            "audit: {title}: [ERROR] text {:.1}:1 (needs {:.1}:1) at {:.0},{:.0}..{:.0},{:.0} — \
             #{:02x}{:02x}{:02x} on #{:02x}{:02x}{:02x}",
            failure.ratio,
            failure.required,
            failure.bounds.left,
            failure.bounds.top,
            failure.bounds.right,
            failure.bounds.bottom,
            failure.text.r,
            failure.text.g,
            failure.text.b,
            failure.background.r,
            failure.background.g,
            failure.background.b,
        );
    }

    println!(
        "audit: {title}: {} error(s), {warnings} warning(s); text contrast: {} failure(s), \
         {} judged, {} skipped",
        errors + contrast.failures.len(),
        contrast.failures.len(),
        contrast.checked,
        contrast.skipped,
    );

    if errors > 0 || !contrast.failures.is_empty() {
        return Err(format!(
            "accessibility audit failed: {errors} tree error(s), {} contrast failure(s)",
            contrast.failures.len()
        )
        .into());
    }
    Ok(())
}
