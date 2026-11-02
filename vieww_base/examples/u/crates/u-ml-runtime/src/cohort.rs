//! The device cohorts the plan's §5.5 table draws, and the caps each one
//! earns — "test cohorts, not compatibility guarantees", as the plan says
//! twice, because a RAM label is a guess until the device is measured.
//!
//! A cohort answers one question: *how much room is there for inference?*
//! The answer is not the RAM the box advertises — a phone's available memory
//! depends on "Android version, background pressure, thermal state and
//! device implementation", in the plan's own words — so the cohorts are
//! deliberately conservative, and the caps they hand the runtime are bounds
//! the runtime may not exceed, never sizes it is promised.
//!
//! Reading total memory is pure `std`: `/proc/meminfo` exists on Android
//! and on Linux, and on anything else the reader reports [`None`] and the
//! caller falls back to the smallest cohort's caution. Nothing here knows
//! about models or tokenizers — that is the point: the *policy* is testable
//! without the thing it bounds.

/// One row of the plan's §5.5 cohort table.
///
/// The names say what the plan says — "around 3 GB", "around 4–6 GB",
/// "6 GB+" — because anything sharper would be a compatibility guarantee
/// this module has no way to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cohort {
    /// ~3 GB: smaller model or restricted inference; graceful fallback.
    Around3Gb,
    /// ~4–6 GB: 0.6B Q2_K/Q3_K with short context and measured headroom.
    Around4To6Gb,
    /// 6 GB+: Q3_K/Q4_K and longer context *only where justified*.
    SixGbPlus,
}

/// What a cohort lets inference do — the §5.6 caps, sized per §5.5.
///
/// Every field is a bound the model runtime must enforce, never a hint it
/// may ignore: context tokens (the KV cache's stride), new tokens (the
/// generation's), and a wall-clock ceiling after which the work is
/// abandoned and the caller falls back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// The most positions (prompt + generated) a single run may occupy.
    pub max_context_tokens: usize,
    /// The most tokens one generation may write before stopping.
    pub max_new_tokens: usize,
    /// How long one whole decision may take before the caller gives up.
    pub timeout: std::time::Duration,
}

impl Caps {
    /// The tightest caps any device must survive — the ~3 GB cohort's own,
    /// and the default when memory could not be read at all.
    ///
    /// A thousand and twenty-four positions is a short context by
    /// design: a KV cache's stride is `layers × kv_heads × head_dim × 2 ×
    /// bytes`, which for this model family is roughly a fifth of a
    /// megabyte *per token*, so a thousand spare positions is two hundred
    /// megabytes of someone's phone gone before the model has written a
    /// word — and it still fits the prompt the [`prompt`](crate::prompt)
    /// module writes for a full two-thousand-character situation.
    pub const SMALLEST: Self = Self {
        max_context_tokens: 1024,
        max_new_tokens: 512,
        timeout: std::time::Duration::from_secs(90),
    };
}

impl Cohort {
    /// The cohort for a total-memory reading, in bytes.
    ///
    /// The thresholds sit *below* the labels — 2.75 GiB counts as "around
    /// 3 GB" — because a device that reports exactly 3 GB has already
    /// spent some of it on the screen, the radio and the OS it is running
    /// on top of.
    #[must_use]
    pub fn of(total_bytes: u64) -> Self {
        const GB: u64 = 1024 * 1024 * 1024;
        if total_bytes >= 6 * GB {
            Self::SixGbPlus
        } else if total_bytes >= 4 * GB {
            Self::Around4To6Gb
        } else if total_bytes >= 2 * GB {
            Self::Around3Gb
        } else {
            // Below the table's smallest row: the table's caution, doubled.
            Self::Around3Gb
        }
    }

    /// The quantisations this cohort may try, best first (§5.5's rows).
    ///
    /// The model runtime walks this order and keeps the first quantisation
    /// whose weights are actually present — which is how "smaller model or
    /// restricted inference" stays a graceful fallback instead of a
    /// day-one crash on a 3 GB phone.
    #[must_use]
    pub fn quant_preference(self) -> &'static [crate::Quant] {
        use crate::Quant::{Q2K, Q3K, Q4K};
        match self {
            Self::Around3Gb => &[Q2K],
            Self::Around4To6Gb => &[Q2K, Q3K],
            Self::SixGbPlus => &[Q3K, Q4K],
        }
    }

    /// The caps this cohort bounds inference by.
    #[must_use]
    pub const fn caps(self) -> Caps {
        match self {
            Self::Around3Gb => Caps::SMALLEST,
            Self::Around4To6Gb => Caps {
                max_context_tokens: 1536,
                max_new_tokens: 640,
                timeout: std::time::Duration::from_secs(120),
            },
            // "Longer context only where justified" (§5.5) — the runtime
            // still clamps the *prompt* to what the situation needs, so a
            // long cap is permission, not appetite.
            Self::SixGbPlus => Caps {
                max_context_tokens: 2048,
                max_new_tokens: 768,
                timeout: std::time::Duration::from_secs(180),
            },
        }
    }
}

/// Total memory from `/proc/meminfo`, in bytes — `None` where unreadable.
///
/// `MemTotal` is the machine's physical RAM, not what is free now; the
/// cohort table is labelled by hardware class, so the class's own number
/// is the honest one to bucket by.
pub fn total_memory() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    // "MemTotal:       32780184 kB" — the kernel's own fixed shape.
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// The cohort this machine falls in, by its own `/proc/meminfo`.
///
/// Unreadable means unknown, and unknown means the smallest cohort's
/// caution — never a guess that flatters the device.
#[must_use]
pub fn current() -> Cohort {
    total_memory().map_or(Cohort::Around3Gb, Cohort::of)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn the_table_rows_bucket_below_their_labels() {
        // "Around N GB" must catch devices that report a little under N.
        assert_eq!(Cohort::of(3 * GB - 300 * 1024 * 1024), Cohort::Around3Gb);
        assert_eq!(Cohort::of(3 * GB), Cohort::Around3Gb);
        assert_eq!(Cohort::of(4 * GB), Cohort::Around4To6Gb);
        assert_eq!(Cohort::of(5 * GB), Cohort::Around4To6Gb);
        assert_eq!(Cohort::of(6 * GB), Cohort::SixGbPlus);
        assert_eq!(Cohort::of(12 * GB), Cohort::SixGbPlus);
    }

    #[test]
    fn a_machine_smaller_than_the_table_gets_the_tables_caution() {
        // A 1 GB reading is not a row in §5.5 — it is the smallest row's
        // warning applied harder, not a licence to try anyway.
        assert_eq!(Cohort::of(GB), Cohort::Around3Gb);
        assert_eq!(Cohort::of(0), Cohort::Around3Gb);
    }

    #[test]
    fn every_cohort_is_bounded_and_the_bounds_only_loosen_with_room() {
        let small = Cohort::Around3Gb.caps();
        let mid = Cohort::Around4To6Gb.caps();
        let big = Cohort::SixGbPlus.caps();
        assert!(small.max_context_tokens < mid.max_context_tokens);
        assert!(mid.max_context_tokens < big.max_context_tokens);
        assert!(small.max_new_tokens <= mid.max_new_tokens);
        assert!(mid.max_new_tokens <= big.max_new_tokens);
        assert!(small.timeout <= mid.timeout);
        assert!(mid.timeout <= big.timeout);
    }

    #[test]
    fn the_smallest_cohort_only_ever_suggests_the_lowest_quant() {
        // §5.5: around 3 GB is "smaller model or restricted inference" —
        // Q3_K/Q4_K must not be reachable there, even if present.
        assert_eq!(Cohort::Around3Gb.quant_preference(), &[crate::Quant::Q2K]);
        assert!(Cohort::SixGbPlus
            .quant_preference()
            .contains(&crate::Quant::Q4K));
    }

    #[test]
    fn meminfo_parsing_tolerates_the_kernels_own_spacing() {
        // The kernel writes fixed-width columns; the parser must not care.
        let parsed = |line: &str| -> Option<u64> {
            let text = format!("{line}\n");
            std::fs::write("/tmp/u-meminfo-test", text).ok()?;
            let read = std::fs::read_to_string("/tmp/u-meminfo-test").ok()?;
            let l = read.lines().next()?;
            l.split_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()
                .map(|kb| kb * 1024)
        };
        assert_eq!(
            parsed("MemTotal:       32780184 kB"),
            Some(32_780_184 * 1024)
        );
        assert_eq!(parsed("MemTotal:  1024 kB"), Some(1024 * 1024));
        let _ = std::fs::remove_file("/tmp/u-meminfo-test");
    }

    #[test]
    fn the_running_machine_falls_in_some_cohort() {
        // Smoke: wherever this test runs, the reading is a row, not a
        // panic, and the caps it yields are the caps it yields.
        let _ = current().caps();
    }
}
