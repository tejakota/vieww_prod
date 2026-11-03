//! Reload metrics: how many reloads a session accepted, and how much state
//! survived them — the two numbers a hot-reload HUD owes the developer who
//! is depending on the feature.
//!
//! # What this counts, and why those numbers
//!
//! A host's [`Reloaded`](vieww_reload::Reloaded) match already divides every
//! poll into the outcomes that matter:
//!
//! - `Root` — the tree survived. The **state-preservation rate** is the
//!   share of accepted reloads that landed here, and it is the number that
//!   says whether hot reload is doing its job: a session at 100% is the
//!   promise the crate's README makes, and every percentage point below it
//!   is a `Taps` counter that went back to zero for someone.
//! - `Restart` — the tree was rebuilt on purpose, because a state type
//!   changed shape. Not a failure, but not *preserved* either; counted
//!   separately so the rate stays honest.
//! - `Failed` — the build didn't load and the old one kept running. Not a
//!   reload at all (nothing was accepted), so it is excluded from the rate's
//!   denominator rather than silently dragging it down: a session with 40
//!   broken builds and 10 clean reloads that all kept state has a
//!   preservation rate of 100%, not 20%.
//!
//! # Why the ledger is fed by the caller rather than reading `Reloader`
//!
//! This crate does not depend on `vieww-reload` — the same choice
//! [`crate::quality_overlay`] makes for the platform probes, for the same
//! reason: the devtools crate reports, and the layer that owns the data
//! hands it over. A host maps its poll results in one line:
//!
//! ```ignore
//! match reloader.poll() {
//!     Reloaded::Nothing => {}
//!     Reloaded::Root(_) => ledger.record(ReloadOutcome::TreeKept),
//!     Reloaded::Restart { .. } => ledger.record(ReloadOutcome::TreeRebuilt),
//!     Reloaded::Failed(_) => ledger.record(ReloadOutcome::BuildFailed),
//! }
//! ```
//!
//! [`ReloadReport`] is a snapshot of the ledger, so a HUD reads it without
//! borrowing the ledger across the frame, and the JSON export can hand the
//! same numbers to a browser devtools UI.

/// One reload outcome, as the host's `Reloaded` match reports it.
///
/// The variants mirror `vieww_reload::Reloaded`'s three interesting arms —
/// `Nothing` is absent because nothing happened and there is nothing to
/// count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// `Reloaded::Root`: a new build, the tree survived.
    TreeKept,
    /// `Reloaded::Restart`: a new build whose state changed shape, so the
    /// tree was rebuilt on purpose.
    TreeRebuilt,
    /// `Reloaded::Failed`: the build did not load; the old one is still
    /// running. Not a reload, and excluded from the preservation rate.
    BuildFailed,
}

/// The running tally a host keeps of its reloads.
///
/// Three counters and nothing else — no history, because a HUD wants the
/// session totals and a ring buffer of past reloads is a second thing to
/// bound for no reader.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReloadLedger {
    kept: u64,
    rebuilt: u64,
    failed: u64,
}

impl ReloadLedger {
    /// An empty ledger, before the first poll.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            kept: 0,
            rebuilt: 0,
            failed: 0,
        }
    }

    /// Count one outcome.
    pub fn record(&mut self, outcome: ReloadOutcome) {
        match outcome {
            ReloadOutcome::TreeKept => self.kept += 1,
            ReloadOutcome::TreeRebuilt => self.rebuilt += 1,
            ReloadOutcome::BuildFailed => self.failed += 1,
        }
    }

    /// The session so far, as a snapshot a HUD or the JSON export reads
    /// without borrowing the ledger.
    #[must_use]
    pub const fn report(&self) -> ReloadReport {
        ReloadReport {
            kept: self.kept,
            rebuilt: self.rebuilt,
            failed: self.failed,
        }
    }
}

/// How many reloads a session accepted, and how much state survived them.
///
/// Not `Eq`: [`ReloadReport::state_preservation_rate`] is a derived float
/// and the same reasoning [`crate::gpu_inspector::GpuReport`] gives applies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReloadReport {
    /// Reloads whose tree survived (`Reloaded::Root`).
    pub kept: u64,
    /// Reloads that rebuilt the tree because state changed shape
    /// (`Reloaded::Restart`) — correct behaviour, counted so the rate stays
    /// honest.
    pub rebuilt: u64,
    /// Builds that did not load (`Reloaded::Failed`). The old build kept
    /// running, so these are not reloads.
    pub failed: u64,
}

impl ReloadReport {
    /// Reloads the session accepted — kept plus rebuilt.
    ///
    /// Failed builds are excluded: nothing was loaded, and a rate whose
    /// denominator counted them would call a session of nothing but broken
    /// builds "perfectly preserving".
    #[must_use]
    pub const fn reloads(&self) -> u64 {
        self.kept + self.rebuilt
    }

    /// The share of accepted reloads that kept the tree: `kept / reloads`.
    ///
    /// `None` before the first reload — an empty session has no rate, and a
    /// zero would read as "nothing ever survived", which is the opposite of
    /// the truth.
    #[must_use]
    pub fn state_preservation_rate(&self) -> Option<f32> {
        let reloads = self.reloads();
        (reloads > 0).then(|| self.kept as f32 / reloads as f32)
    }

    /// The report as one line — the form a suite log carries, and the form
    /// `examples/reload-host` prints per reload so a developer watching the
    /// terminal sees the session's score alongside the reload it just got.
    ///
    /// Whole percentages: a HUD that said "99.4% preserved" would be
    /// implying precision the count of ten reloads cannot support.
    #[must_use]
    pub fn summary_line(&self) -> String {
        let reloads = self.reloads();
        if reloads == 0 && self.failed == 0 {
            return "reloads 0 — no reload yet".to_owned();
        }
        let rate = self.state_preservation_rate().map_or_else(
            || "—".to_owned(),
            |rate| format!("{}%", (rate * 100.0).round() as u32),
        );
        let mut line = format!("reloads {reloads} · state kept {rate}");
        if self.rebuilt > 0 {
            line += &format!(" ({} rebuilt)", self.rebuilt);
        }
        if self.failed > 0 {
            line += &format!(" · {} failed", self.failed);
        }
        line
    }

    /// The report as the lines a HUD would draw, alongside
    /// [`crate::QualityOverlay`]'s: the count, then the rate with its
    /// numerator visible — "11 of 12" says more than "92%" when the
    /// developer is deciding whether to trust the feature.
    #[must_use]
    pub fn hud_lines(&self) -> Vec<String> {
        let reloads = self.reloads();
        if reloads == 0 && self.failed == 0 {
            return vec!["reload: none yet".to_owned()];
        }
        let mut lines = vec![format!("reload: {reloads} this session")];
        if reloads > 0 {
            let percent = (self.state_preservation_rate().unwrap_or(0.0) * 100.0).round() as u32;
            lines.push(format!(
                "state kept: {} of {} reloads ({}%)",
                self.kept, reloads, percent
            ));
        } else {
            // Only failures so far: the rate is undefined, and saying so is
            // better than a 0% that reads as "everything was lost" when in
            // fact nothing was ever accepted.
            lines.push("state kept: no reload accepted yet".to_owned());
        }
        if self.rebuilt > 0 {
            lines.push(format!("rebuilt for shape: {}", self.rebuilt));
        }
        if self.failed > 0 {
            lines.push(format!("failed builds: {}", self.failed));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::{ReloadLedger, ReloadOutcome, ReloadReport};

    /// The session the module docs describe: 10 clean reloads that all kept
    /// state, 40 broken builds. The rate is 100%, not 20%.
    fn mixed_session() -> ReloadLedger {
        let mut ledger = ReloadLedger::new();
        for _ in 0..10 {
            ledger.record(ReloadOutcome::TreeKept);
        }
        for _ in 0..40 {
            ledger.record(ReloadOutcome::BuildFailed);
        }
        ledger
    }

    #[test]
    fn an_empty_session_has_no_rate_and_says_so() {
        let report = ReloadLedger::new().report();
        assert_eq!(report.state_preservation_rate(), None);
        assert_eq!(report.summary_line(), "reloads 0 — no reload yet");
        assert_eq!(report.hud_lines(), vec!["reload: none yet".to_owned()]);
    }

    #[test]
    fn failed_builds_do_not_drag_the_rate_down() {
        let report = mixed_session().report();
        // 10 kept, 0 rebuilt, 40 failed: the denominator is the 10 accepted
        // reloads, so the rate is perfect, not 10/50.
        assert_eq!(report.reloads(), 10);
        assert_eq!(report.state_preservation_rate(), Some(1.0));
        let line = report.summary_line();
        assert!(line.contains("reloads 10"), "{line}");
        assert!(line.contains("100%"), "{line}");
        assert!(line.contains("40 failed"), "{line}");
    }

    #[test]
    fn a_shape_change_is_counted_and_lowers_the_rate() {
        let mut ledger = ReloadLedger::new();
        for _ in 0..11 {
            ledger.record(ReloadOutcome::TreeKept);
        }
        ledger.record(ReloadOutcome::TreeRebuilt);
        let report = ledger.report();
        assert_eq!(report.reloads(), 12);
        // 11/12 ≈ 0.9167 — the honest score for one shape change.
        let rate = report.state_preservation_rate().unwrap();
        assert!((rate - 11.0 / 12.0).abs() < 1e-6, "{rate}");
        let line = report.summary_line();
        assert!(line.contains("92%"), "{line}");
        assert!(line.contains("1 rebuilt"), "{line}");
        // And the HUD line shows the numerator, because "11 of 12" is what
        // lets a developer decide whether to trust the feature.
        assert!(
            report
                .hud_lines()
                .iter()
                .any(|line| line.contains("11 of 12")),
            "{:?}",
            report.hud_lines()
        );
    }

    #[test]
    fn a_session_of_failures_only_reports_no_accepted_reload() {
        let mut ledger = ReloadLedger::new();
        ledger.record(ReloadOutcome::BuildFailed);
        let report = ledger.report();
        assert_eq!(report.reloads(), 0);
        assert_eq!(report.state_preservation_rate(), None);
        assert!(
            report
                .hud_lines()
                .iter()
                .any(|line| line.contains("no reload accepted yet")),
            "a 0% rate would read as everything-lost when nothing was lost: {:?}",
            report.hud_lines()
        );
    }

    #[test]
    fn the_report_is_a_snapshot_rather_than_a_borrow() {
        // The ledger keeps counting after a report is taken; the report's
        // numbers are the ones from when it was taken.
        let mut ledger = ReloadLedger::new();
        ledger.record(ReloadOutcome::TreeKept);
        let report = ledger.report();
        ledger.record(ReloadOutcome::TreeKept);
        assert_eq!(report.kept, 1);
        assert_eq!(ledger.report().kept, 2);
    }
}
