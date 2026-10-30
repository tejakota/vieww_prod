//! `u-memory` — everything U remembers, on the device it happened on.
//!
//! The plan's §8: distinct records for decisions, assessments, feedback,
//! outcomes and the versioned personalisation state, written to a store a
//! phone owns. The physical shape here is **JSON lines, one file per record
//! kind, in the app's private directory** — the same shape the app's event
//! log already uses, chosen for the properties that matter on Android:
//! append-only writes that survive a mid-write process death, a reader that
//! costs one line at a time, and no C library to cross-compile.
//!
//! `SQLite` is the plan's named candidate for when the query patterns earn
//! it; the method surface below is the seam it arrives through, so the
//! swap is a new implementation rather than a rewrite of everything that
//! remembers.
//!
//! # Recovery is a property, not a hope
//!
//! A corrupt or truncated line costs exactly that line — every line before
//! it is already parsed — and the store reports the count it recovered so
//! nobody mistakes a partial history for a whole one. A store whose schema
//! is newer than the build reading it is *refused* ([`Error::Schema`])
//! rather than best-effort parsed, because a half-understood history that
//! still looks like history is the worst thing a memory can do.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use u_core::{Error, Id};
use u_persona::Persona;

/// The store's schema version — the plan's `schema_migrations` table, as a
/// number on disk that must never go backwards silently.
pub const SCHEMA: u32 = 1;

/// A decision the person brought, exactly as they brought it (§8's
/// `decisions` + `decision_options`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    /// When it was recorded.
    pub id: Id,
    /// The free-form situation, normalised.
    pub situation: String,
    /// The options they gave, verbatim.
    pub options: Vec<String>,
    /// What they said matters.
    pub matters: Vec<String>,
    /// The constraints they named.
    pub constraints: Vec<String>,
    /// Any notes.
    pub notes: String,
    /// The inferred tags at record time — retrieval labels, nothing more.
    pub tags: Vec<String>,
}

/// One saved assessment (§8's `decision_assessments`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssessmentRecord {
    /// The decision this assessed.
    pub decision: Id,
    /// The assessment, whole, as its own schema-carrying JSON.
    pub assessment: serde_json::Value,
}

/// Whether an assessment helped — the person's word for it, nothing
/// inferred (§8's `decision_feedback`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Verdict {
    /// It helped.
    Helpful,
    /// It did not.
    NotHelpful,
}

/// One piece of feedback on a decision.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FeedbackRecord {
    /// The decision it is about.
    pub decision: Id,
    /// Their verdict.
    pub verdict: Verdict,
    /// What they cared to add, if anything.
    pub note: Option<String>,
    /// When.
    pub at: Id,
}

/// What actually happened — optional, and never proof the decision was good
/// (§2.6: "do not treat an outcome as proof").
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OutcomeRecord {
    /// The decision it belongs to.
    pub decision: Id,
    /// What happened, in their words.
    pub what: String,
    /// Whether it matched what they wanted — their call, not U's.
    pub satisfied: Option<bool>,
    /// When they wrote it.
    pub at: Id,
}

/// The whole store, as one directory of JSON-lines files.
///
/// Created against a directory the *caller* owns — on Android the
/// activity's private directory, on a desktop the platform's data dir —
/// because "where U's memory lives" is a decision the platform layer makes,
/// not the memory itself.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Open (or create) the store at `dir`.
    ///
    /// # Errors
    /// [`Error::Schema`] when the directory holds a schema this build does
    /// not read, and [`Error::Corrupt`] when the directory cannot be
    /// created at all.
    pub fn at(dir: impl AsRef<Path>) -> Result<Self, Error> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(|_| Error::Corrupt { recovered: 0 })?;
        let marker = dir.join("schema.json");
        let found: u32 = if marker.exists() {
            read_schema(&marker).unwrap_or(0)
        } else {
            // A brand-new store: write the marker now so the version is a
            // fact on disk rather than an implication.
            let _ = write_schema(&marker);
            SCHEMA
        };
        if found > SCHEMA {
            return Err(Error::Schema {
                found,
                reads: SCHEMA,
            });
        }
        Ok(Self { dir })
    }

    /// Append a record to its `kind`'s file — one line, flushed before
    /// returning, so a process death after the call leaves the record.
    ///
    /// # Errors
    /// The filesystem's own failure, as [`Error::Corrupt`].
    pub fn append<T: serde::Serialize>(&self, kind: &str, record: &T) -> Result<(), Error> {
        let path = self.dir.join(format!("{kind}.jsonl"));
        append_line(&path, record).map_err(|_| Error::Corrupt { recovered: 0 })
    }

    /// Read every record of a `kind`, skipping corrupt lines and reporting
    /// how many survived.
    ///
    /// A `kind` with no file yet is an empty memory, not a broken one — the
    /// first decision ever brought has not written `decisions.jsonl` into
    /// existence, and reading that as corruption would make a first run
    /// refuse the engine that is about to write it.
    ///
    /// # Errors
    /// [`Error::Corrupt`] only when the file cannot be opened for a reason
    /// other than not existing; a corrupt *line* is survived, never fatal.
    pub fn load<T: for<'de> serde::Deserialize<'de>>(
        &self,
        kind: &str,
    ) -> Result<(Vec<T>, usize), Error> {
        let path = self.dir.join(format!("{kind}.jsonl"));
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(not_there) if not_there.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), 0));
            }
            Err(_) => return Err(Error::Corrupt { recovered: 0 }),
        };
        let mut records = Vec::new();
        let mut recovered = 0;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str(line) {
                records.push(record);
                recovered += 1;
            }
        }
        Ok((records, recovered))
    }

    /// The most recent decision, if any.
    ///
    /// # Errors
    /// As [`Self::load`].
    pub fn latest_decision(&self) -> Result<Option<DecisionRecord>, Error> {
        let (mut decisions, _) = self.load::<DecisionRecord>("decisions")?;
        decisions.sort_by_key(|d| std::cmp::Reverse(d.id));
        Ok(decisions.into_iter().next())
    }

    /// Save the persona whole — the versioned personalisation state.
    ///
    /// One file, one object, written to a sibling and renamed over, so a
    /// crash mid-write leaves the previous state rather than half of the
    /// new one.
    ///
    /// # Errors
    /// The filesystem's own failure, as [`Error::Corrupt`].
    pub fn save_persona(&self, persona: &Persona) -> Result<(), Error> {
        let final_path = self.dir.join("persona.json");
        let stage = self.dir.join("persona.json.new");
        let json = serde_json::to_string(persona).map_err(|_| Error::Corrupt { recovered: 0 })?;
        fs::write(&stage, json).map_err(|_| Error::Corrupt { recovered: 0 })?;
        fs::rename(&stage, &final_path).map_err(|_| Error::Corrupt { recovered: 0 })?;
        Ok(())
    }

    /// Load the persona, or an empty one for a first run.
    ///
    /// # Errors
    /// [`Error::Corrupt`] when the file exists and cannot be parsed — the
    /// persona is too valuable to guess at in halves.
    pub fn load_persona(&self) -> Result<Persona, Error> {
        let path = self.dir.join("persona.json");
        if !path.exists() {
            return Ok(Persona::default());
        }
        let text = fs::read_to_string(&path).map_err(|_| Error::Corrupt { recovered: 0 })?;
        serde_json::from_str(&text).map_err(|_| Error::Corrupt { recovered: 0 })
    }
}

/// Read a schema marker's `"schema"` field.
fn read_schema(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("schema")?.as_u64().map(|v| v as u32)
}

/// Write the schema marker.
fn write_schema(path: &Path) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    writeln!(file, "{{\"schema\":{SCHEMA}}}")
}

/// Append one serialised record as a line, flushed.
fn append_line<T: serde::Serialize>(path: &Path, record: &T) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(record)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writeln!(file, "{line}")?;
    file.flush()
}

/// A throwaway directory under the system temp dir, unique per call — the
/// place a scratch store lives.
///
/// Exposed as a path rather than only as a [`Store`] because a caller with
/// more than a store — an engine, say — wants the same throwaway place for
/// everything it keeps, not a second one beside the first.
#[must_use]
pub fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("u-memory-{}-{}", std::process::id(), now_ns()))
}

/// A store for tests and headless renders: a throwaway directory under the
/// system temp dir, unique per call.
///
/// Not used by the app itself — the screenshot tool wants renders that
/// never depend on the machine drawing them, which starts with a memory
/// that keeps nothing.
#[must_use]
pub fn scratch() -> Store {
    Store::at(scratch_dir()).expect("a fresh temp directory is always openable")
}

fn now_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use u_persona::Dimension;

    fn decision(id: Id, situation: &str) -> DecisionRecord {
        DecisionRecord {
            id,
            situation: situation.to_owned(),
            options: vec!["one".into()],
            matters: vec!["growth".into()],
            constraints: Vec::new(),
            notes: String::new(),
            tags: vec!["career".into()],
        }
    }

    #[test]
    fn records_round_trip_through_the_store() {
        let store = scratch();
        store.append("decisions", &decision(1, "switch teams")).unwrap();
        store.append("decisions", &decision(2, "move cities")).unwrap();
        let (loaded, recovered) = store.load::<DecisionRecord>("decisions").unwrap();
        assert_eq!(recovered, 2);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[1].situation, "move cities");
    }

    #[test]
    fn a_corrupt_line_costs_only_itself() {
        let store = scratch();
        store.append("decisions", &decision(1, "good")).unwrap();
        // Damage: half a JSON object, as a truncated write would leave.
        let path = store.dir.join("decisions.jsonl");
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("{\"situation\": \"half of so");
        fs::write(&path, text).unwrap();

        let (loaded, recovered) = store.load::<DecisionRecord>("decisions").unwrap();
        assert_eq!(recovered, 1, "the good line survived");
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn the_latest_decision_wins() {
        let store = scratch();
        store.append("decisions", &decision(10, "older")).unwrap();
        store.append("decisions", &decision(20, "newer")).unwrap();
        assert_eq!(store.latest_decision().unwrap().map(|d| d.id), Some(20));
    }

    #[test]
    fn the_persona_round_trips_whole() {
        let store = scratch();
        let mut persona = Persona::default();
        persona.observe(Dimension::Stability, 0.7, 0.8, u_persona::Source::Stated, None, 5);
        store.save_persona(&persona).unwrap();
        let back = store.load_persona().unwrap();
        assert_eq!(back, persona);
    }

    #[test]
    fn a_fresh_store_has_no_decisions_and_says_so() {
        // The first run of the app reads a memory whose files do not exist
        // yet — that is emptiness, not corruption, and confusing the two
        // would refuse the engine on the very launch that would create them.
        let store = scratch();
        assert_eq!(store.latest_decision().unwrap(), None);
        let (records, recovered) = store.load::<DecisionRecord>("decisions").unwrap();
        assert!(records.is_empty());
        assert_eq!(recovered, 0);
    }

    #[test]
    fn a_fresh_store_has_an_empty_persona() {
        let store = scratch();
        assert_eq!(store.load_persona().unwrap(), Persona::default());
    }

    #[test]
    fn a_future_schema_is_refused_not_guessed_at() {
        let store = scratch();
        let marker = store.dir.join("schema.json");
        fs::write(&marker, format!("{{\"schema\": {}}}", SCHEMA + 1)).unwrap();
        assert!(matches!(
            Store::at(&store.dir),
            Err(Error::Schema { found, .. }) if found == SCHEMA + 1
        ));
    }

    #[test]
    fn a_fresh_store_writes_its_schema_down() {
        let store = scratch();
        let marker = store.dir.join("schema.json");
        assert!(marker.exists());
        assert_eq!(read_schema(&marker), Some(SCHEMA));
    }
}
