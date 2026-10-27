//! `wright fork <run> --at N` (WR-12): a new project branched from a run
//! at one event.
//!
//! Replay shows a run's state at event N without changing anything. A fork
//! makes that state a project of its own, to change and fire again. The
//! new project gets:
//!
//! * the project's files, apart from builds, caches, logs and `artifacts/`;
//! * the blueprint the run fired, from the store's commit history. Its
//!   `meta.lineage` is set to `fork://<run's project>/run-<run>@<N>`, and it
//!   is committed to the new project's own history;
//! * `artifacts/` as they stood at event N. For each artifact the last
//!   `ArtifactWritten` at or before N names a hash. The bytes come from the
//!   object store, or from the current file when that hash still matches.
//!   An artifact whose bytes are in neither place is listed as not
//!   restored. The fork says so, rather than shipping a later version;
//! * the source run's events 1..=N, byte for byte, as the new project's
//!   run 1. That record shows where it came from. Its first fresh firing
//!   is run 2.

use std::path::{Path, PathBuf};

use wright_core::blueprint::{canon, parse};
use wright_core::events::{EventKind, RunId};
use wright_core::hash;
use wright_runtime::store::{write_atomic, Store};

/// What a fork made.
#[derive(Debug)]
pub struct Forked {
    /// The new project.
    pub dir: PathBuf,
    /// Its blueprint's lineage line.
    pub lineage: String,
    /// Artifacts restored as of the event, by name.
    pub restored: Vec<String>,
    /// Artifacts written by then whose bytes were no longer anywhere.
    pub missing: Vec<String>,
    /// Events carried over (= N).
    pub events: usize,
}

/// Folders never copied (what `export` leaves out, plus `artifacts/`,
/// which is rebuilt as of the event).
const SKIP: &[&str] = &[
    ".git",
    ".wright",
    "target",
    "node_modules",
    "dist",
    "__pycache__",
    ".venv",
    "venv",
    "logs",
    "artifacts",
];

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name();
        let meta = std::fs::symlink_metadata(e.path()).map_err(|err| err.to_string())?;
        if meta.is_dir() {
            if SKIP.iter().any(|s| name == *s) {
                continue;
            }
            copy_tree(&e.path(), &to.join(&name))?;
        } else if meta.is_file() {
            std::fs::copy(e.path(), to.join(&name))
                .map_err(|err| format!("{}: {err}", e.path().display()))?;
        }
        // Symbolic links are not followed: a fork copies the project, not
        // what it points at.
    }
    Ok(())
}

/// Fork `project`'s run `run` at event `at` into `into` (which must not
/// exist, or be empty).
///
/// # Errors
/// No such run or event, the target is not empty, the run's blueprint
/// commit is missing, or a file operation fails.
pub fn fork(project: &Path, run: RunId, at: u64, into: &Path) -> Result<Forked, String> {
    let store = Store::open(project)?;
    let log = store.read_events(run)?;
    if at == 0 || at > log.len() as u64 {
        return Err(format!(
            "run {run} has events 1..={}; there is no event {at}",
            log.len()
        ));
    }
    if into.exists()
        && std::fs::read_dir(into)
            .map_err(|e| format!("{}: {e}", into.display()))?
            .next()
            .is_some()
    {
        return Err(format!("{} exists and is not empty", into.display()));
    }
    let prefix: Vec<_> = log.events().iter().filter(|e| e.seq <= at).collect();

    // The blueprint the run fired.
    let commit = prefix
        .iter()
        .find(|e| e.kind == EventKind::RunStarted)
        .and_then(|e| e.data_value())
        .and_then(|v| v.str_of("blueprint").map(str::to_owned))
        .ok_or_else(|| format!("run {run}'s start event names no blueprint commit"))?;
    let text = store.blueprint_at(&commit)?;
    let mut bp = parse::blueprint(&text).map_err(|e| format!("the run's blueprint: {e}"))?;
    let lineage = format!("fork://{}/run-{run}@{at}", bp.meta.name);
    bp.meta.lineage = lineage.clone();
    let canonical = canon::canonical(&bp);

    copy_tree(project, into)?;
    write_atomic(&into.join("blueprint.kdl"), canonical.as_bytes())?;

    // Artifacts as of the event: the last write of each at or before it.
    let mut latest: std::collections::BTreeMap<String, String> = Default::default();
    for e in &prefix {
        if e.kind != EventKind::ArtifactWritten {
            continue;
        }
        if let Some(v) = e.data_value() {
            if let (Some(p), Some(h)) = (v.str_of("path"), v.str_of("hash")) {
                latest.insert(p.to_owned(), h.to_owned());
            }
        }
    }
    let (mut restored, mut missing) = (Vec::new(), Vec::new());
    for (path, h) in latest {
        // Paths come from the log: only plain relative paths are written.
        let rel = Path::new(&path);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            missing.push(path);
            continue;
        }
        let bytes = store.get_object(&h).ok().or_else(|| {
            std::fs::read(project.join(rel))
                .ok()
                .filter(|b| hash::blake3_hex(b) == h)
        });
        match bytes {
            Some(b) => {
                let dst = into.join(rel);
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                write_atomic(&dst, &b)?;
                restored.push(path);
            }
            None => missing.push(path),
        }
    }
    std::fs::create_dir_all(into.join("artifacts")).map_err(|e| e.to_string())?;

    // The new project's store: the blueprint committed, the source run's
    // first N events as run 1.
    let new_store = Store::open(into)?;
    new_store.commit_blueprint(&canonical, &format!("fork of run {run} at event {at}"))?;
    let writer = new_store.new_run()?;
    let events_path = writer.path().to_path_buf();
    drop(writer);
    let lines: String = prefix.iter().map(|e| e.to_json_line() + "\n").collect();
    write_atomic(&events_path, lines.as_bytes())?;

    Ok(Forked {
        dir: into.to_path_buf(),
        lineage,
        restored,
        missing,
        events: prefix.len(),
    })
}
