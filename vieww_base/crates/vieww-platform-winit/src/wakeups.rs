//! How many times this process was woken, counted from `/proc`.
//!
//! # Why a wakeup counter belongs beside the CPU counter
//!
//! `crate::cpu_time` says what an idle window *costs*; this says *why*. The
//! audit's baseline measured **3.4% CPU with zero frames presented** on Linux
//! (PENDING §4.2, FW-15) — the process was doing something, and the only
//! honest description was "unexplained wakeups". A number nobody can attribute
//! is a number nobody can fix, and the first step to attributing it is counting
//! it run over run: an event loop asleep in `ControlFlow::Wait` wakes on real
//! input alone, a poller wakes on its own timer, and the two are told apart by
//! the rate while nothing else happens.
//!
//! # What is counted
//!
//! `voluntary_ctxt_switches + nonvoluntary_ctxt_switches` from
//! `/proc/self/status`, the whole process, every thread together. Every wakeup
//! that runs anything is preceded by a context switch back into the thread, so
//! this is an upper bound on wakeups — close enough to gate on, because the
//! gated quantity is *idle* wakeups (the delta over a stretch where the app
//! presents nothing and handles no input), where the only context switches
//! happening are the ones being counted for. A thread that loops on a 16 ms
//! poll reads as ~62 per second; a loop asleep in `Wait` on an untouched
//! machine reads as roughly zero.
//!
//! Linux and Android only; `None` elsewhere, the same contract as
//! [`crate::cpu_time`]. The `baseline` example reports it per idle stretch so
//! `ci/hardware/linux.sh` can gate on wakeups-per-second at zero frames
//! (FW-15), which is the assertion that keeps a poller from coming back.

/// How many times this process has been woken since it started, every thread
/// together. `None` where the platform does not say.
#[must_use]
pub fn process_wakeups() -> Option<u64> {
    imp::read()
}

/// `later - earlier`, saturating, when both are known — the one subtraction
/// every idle-wakeup report is, kept here so every caller computes it the same
/// way and a torn reading stays `None` instead of becoming a zero a gate would
/// pass on.
#[must_use]
pub fn wakeups_between(earlier: Option<u64>, later: Option<u64>) -> Option<u64> {
    Some(later?.saturating_sub(earlier?))
}

/// `voluntary_ctxt_switches + nonvoluntary_ctxt_switches` from the text of
/// `/proc/<pid>/status`.
///
/// Both lines are kernel-formatted as `Name:\t<number>` and both must be
/// present for a reading: a torn read where one line is missing is no reading
/// at all, which is the same all-or-nothing rule `cpu_time`'s parser keeps.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "android")),
    allow(
        dead_code,
        reason = "only /proc platforms read the file; the tests use it everywhere"
    )
)]
fn wakeups_from_status(status: &str) -> Option<u64> {
    let value = |key: &str| {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|field| field.parse::<u64>().ok())
    };
    Some(value("voluntary_ctxt_switches:")? + value("nonvoluntary_ctxt_switches:")?)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod imp {
    pub(super) fn read() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        super::wakeups_from_status(&status)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
mod imp {
    pub(super) const fn read() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real file, on the platforms that have one — the counter must at
    /// least read, and every thread the test harness itself owns guarantees a
    /// non-zero reading by the time any test runs.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn the_process_counter_reads_a_positive_number() {
        assert!(process_wakeups().is_some_and(|w| w > 0));
    }

    /// A counter that reads zero would make every idle gate pass vacuously:
    /// "zero wakeups" from a parser that matched nothing is exactly the failure
    /// mode a budget must not be able to hide behind.
    #[test]
    fn both_lines_are_required_for_a_reading() {
        assert_eq!(wakeups_from_status("voluntary_ctxt_switches:\t7\n"), None);
        assert_eq!(
            wakeups_from_status("nonvoluntary_ctxt_switches:\t3\n"),
            None
        );
        assert_eq!(wakeups_from_status(""), None);
    }

    /// The two counters add, which is the whole definition: a thread that
    /// sleeps on purpose and is preempted involuntarily both cost a wakeup.
    #[test]
    fn voluntary_and_involuntary_switches_add() {
        let status = "Name:\tvieww\nVmRSS:\t  150000 kB\n\
                      voluntary_ctxt_switches:\t1204\n\
                      nonvoluntary_ctxt_switches:\t37\n";
        assert_eq!(wakeups_from_status(status), Some(1241));
    }

    /// Other lines between and around the two must not matter — `/proc`
    /// reorders nothing, but a parser that depended on order would be one
    /// kernel config away from silently reading nothing.
    #[test]
    fn the_two_lines_may_have_neighbours_in_either_order() {
        let status = "Name:\tvieww\n\
                      nonvoluntary_ctxt_switches:\t2\n\
                      Threads:\t9\n\
                      voluntary_ctxt_switches:\t40\n";
        assert_eq!(wakeups_from_status(status), Some(42));
    }

    /// A negative, a word or a missing number is no reading rather than a
    /// panic or a zero — see `the_process_counter_reads_a_positive_number`
    /// for why zero is the dangerous one.
    #[test]
    fn garbage_is_no_reading() {
        assert_eq!(wakeups_from_status("Name:\t\n"), None);
        assert_eq!(
            wakeups_from_status("voluntary_ctxt_switches:\tmany\nnonvoluntary_ctxt_switches:\t1\n"),
            None
        );
    }

    /// The subtraction a report is made of: known minus known, `None` when
    /// either side is unknown, and never a panic on the back-to-back reads a
    /// live process is always doing.
    #[test]
    fn a_report_is_a_saturating_subtraction() {
        assert_eq!(wakeups_between(Some(7), Some(7)), Some(0));
        assert_eq!(wakeups_between(Some(10), Some(3)), Some(0));
        assert_eq!(wakeups_between(None, Some(3)), None);
        assert_eq!(wakeups_between(Some(3), None), None);
        // Back to back on a live process: whatever it is, it is not a panic.
        let _ = wakeups_between(process_wakeups(), process_wakeups());
    }
}
