//! Version comparison for the update channel.
//!
//! # Why not `semver` the crate
//!
//! Two reasons, one practical and one of scope. Practical: this crate has
//! no dependencies (see the crate docs for why that is a hard rule here),
//! and version comparison is a hundred lines of parsing and integer
//! comparison. Scope: an update channel does not need the whole semver
//! spec — build metadata is ignored by precedence *by the spec itself*
//! and `+build` tags on a shipped app's version would make an updater
//! refuse its own update, so what this parses is deliberately the subset
//! an app's `CARGO_PKG_VERSION` can actually contain:
//!
//! ```text
//! major "." minor "." patch [ "-" pre-release ]
//! ```
//!
//! The pre-release is compared by the semver rule — a version *with* a
//! pre-release is older than the same version without one, and
//! dot-separated identifiers compare numerically when both are numeric
//! and lexically otherwise — because that rule is what makes
//! `1.4.0-rc.2` upgrade to `1.4.0`, which is precisely the upgrade an
//! update channel exists to deliver.

use std::cmp::Ordering;
use std::fmt;

/// A version, parsed. Constructed by [`parse`](Self::parse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The `‑`-separated identifiers after the patch, empty when absent.
    pub pre: Vec<PreId>,
}

/// One dot-separated identifier of a pre-release tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreId {
    /// Numeric identifiers compare by value, not lexically (`rc.9` < `rc.10`).
    Number(u64),
    /// Alphabetic ones lexically, and always *lower* than a numeric one at
    /// the same position (`1.0.0-alpha` < `1.0.0-1`), which is the rule
    /// that makes `alpha < beta < 1`.
    Text(String),
}

impl Version {
    /// Parse `major.minor.patch[-pre]`. `None` for anything else — an
    /// unparsable version in a manifest is a refusal, never a guess, and
    /// never `0.0.0` silently.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = core.split('.');
        let mut next = || numbers.next()?.parse::<u64>().ok();
        let major = next()?;
        let minor = next()?;
        let patch = next()?;
        if numbers.next().is_some() {
            return None;
        }
        // `unwrap_or_default`, not `?`: an *absent* pre-release is the
        // common case, and `?` would turn "no tag" into "no version".
        let pre = pre
            .map(|pre| {
                pre.split('.')
                    .map(|id| match id.parse::<u64>() {
                        Ok(n) => PreId::Number(n),
                        Err(_) => PreId::Text(id.to_owned()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// `true` when this is the same release without a pre-release tag —
    /// `1.4.0-rc.1` upgrading to `1.4.0`, the one comparison an updater
    /// makes that equality alone would miss.
    #[must_use]
    pub fn is_prerelease_of(&self, release: &Self) -> bool {
        self.major == release.major
            && self.minor == release.minor
            && self.patch == release.patch
            && !self.pre.is_empty()
            && release.pre.is_empty()
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // Build metadata is not part of precedence and not parsed, so
        // there is nothing to strip here.
        let core = (self.major, self.minor, self.patch, other_pre_rank(self));
        let other_core = (other.major, other.minor, other.patch, other_pre_rank(other));
        core.cmp(&other_core)
            .then_with(|| cmp_pre(&self.pre, &other.pre))
    }
}

/// An empty pre-release sorts *after* any non-empty one (semver §11): the
/// release is newer than its own candidates. Everything else is decided by
/// `cmp_pre`, so the rank only has to distinguish "has a tag" from "does not".
fn other_pre_rank(v: &Version) -> u8 {
    u8::from(v.pre.is_empty())
}

fn cmp_pre(a: &[PreId], b: &[PreId]) -> Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        let here = match (x, y) {
            (PreId::Number(m), PreId::Number(n)) => m.cmp(n),
            // Numeric beats alphabetic at the same position.
            (PreId::Number(_), PreId::Text(_)) => Ordering::Greater,
            (PreId::Text(_), PreId::Number(_)) => Ordering::Less,
            (PreId::Text(m), PreId::Text(n)) => m.cmp(n),
        };
        if here != Ordering::Equal {
            return here;
        }
    }
    // Shorter pre-release is smaller, when one is a prefix of the other.
    a.len().cmp(&b.len())
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            f.write_str("-")?;
            for (i, id) in self.pre.iter().enumerate() {
                if i > 0 {
                    f.write_str(".")?;
                }
                match id {
                    PreId::Number(n) => write!(f, "{n}")?,
                    PreId::Text(t) => f.write_str(t)?,
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("'{text}' should parse"))
    }

    #[test]
    fn numeric_fields_compare_numerically_not_lexically() {
        // The classic lexicographic trap: "9" vs "10".
        assert!(v("1.2.9") < v("1.2.10"));
        assert!(v("1.9.0") < v("1.10.0"));
        assert!(v("2.0.0") > v("1.99.99"));
        assert_eq!(v("1.2.3"), v("1.2.3"));
    }

    #[test]
    fn pre_releases_are_older_than_their_release() {
        assert!(v("1.4.0-rc.1") < v("1.4.0"));
        assert!(v("1.4.0-rc.2") < v("1.4.0-rc.10"));
        assert!(v("1.4.0-alpha") < v("1.4.0-beta"));
        assert!(v("1.4.0-alpha.1") < v("1.4.0-alpha.2"));
        // A prefix of another pre-release is the smaller one.
        assert!(v("1.4.0-rc") < v("1.4.0-rc.1"));
        // And numeric identifiers beat alphabetic ones at the same slot.
        assert!(v("1.0.0-alpha") < v("1.0.0-1"));
        assert!(v("1.4.0") > v("1.4.0-anything"));
    }

    #[test]
    fn a_release_candidate_is_a_prerelease_of_its_release() {
        let rc = v("1.4.0-rc.1");
        let release = v("1.4.0");
        assert!(rc.is_prerelease_of(&release));
        assert!(!release.is_prerelease_of(&rc));
        assert!(
            !v("1.4.1-rc.1").is_prerelease_of(&release),
            "different patch"
        );
    }

    #[test]
    fn round_trips_through_display() {
        for text in ["0.1.0", "1.4.0", "1.4.0-rc.1", "2.0.0-beta.11"] {
            assert_eq!(v(text).to_string(), text, "{text}");
        }
    }

    #[test]
    fn anything_else_is_none_not_a_guess() {
        for text in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "1.2.x",
            "1.2.-3",
            "v1.2.3",
            "1.2.3+build",
        ] {
            assert!(Version::parse(text).is_none(), "'{text}' parsed");
        }
    }
}
