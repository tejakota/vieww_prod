//! Team mode without a server (WR-18, ADR-045).
//!
//! The finding: every rival's team story is a server — accounts,
//! tenancy, a protocol to attack. Wright already owns the audit story
//! teams actually need (verifiable receipts, per-task identities,
//! content-addressed artifacts), so the team story here is the
//! **shared store**: one directory — a network mount or a synced
//! folder — used as this project's store by every member machine.
//! No server process, no new outbound protocol, nothing to attack
//! that a file server is not already carrying.
//!
//! How a team works:
//!
//! * **join** — `wright team join <shared-dir>` checks the directory
//!   is suitable ([`suitability`]: outside the project, writable,
//!   fsync-durable), appends this machine's `did:key` to the shared
//!   `team/roster.jsonl` (trust on first use: the roster is the set of
//!   keys pinned AT JOIN TIME, honestly weaker than checking each
//!   other's keys in person — `wright trust` remains the stronger
//!   path), and writes a pointer next to this machine's default store.
//!   The pointer lives in Wright's home, OUTSIDE the project: a
//!   sandboxed agent can write the project tree, never the redirect.
//!   `WRIGHT_STORE` overrides the pointer for one shell (CI, a
//!   one-off) — same trust domain, the user's own environment.
//! * **the spine** — runs are append-only `events.jsonl` files under
//!   `runs/`, every other file is written atomically
//!   (temp-then-rename), objects are content-addressed (two machines
//!   writing the same artifact write the same bytes to the same path —
//!   benign), and the firing lock serializes interleaved firings
//!   across machines, which also makes run-id allocation safe.
//! * **pull** — `wright team pull` verifies every receipt in the
//!   shared store against the ROSTER's pinned keys (a receipt from
//!   machine A checks on machine B through the same seven checks;
//!   the identity delegation check is signer-relative, so it crosses
//!   machines unchanged), and says which runs are new.
//! * **convergence rules** (documented, tested): the STORE converges
//!   through the shared spine; the WORKING TREES do not — each machine
//!   keeps its own checkout and syncs it with the VCS as always. A
//!   receipt verified on a machine whose tree has not synced the
//!   artifacts yet fails its artifacts check, honestly. The task cache
//!   is shared: keys are pure functions of task+model+inputs+tool
//!   versions+permissions (machine-independent by construction), so a
//!   peer's cache entry is usable exactly when a local one would be —
//!   and never across differing permissions.
//!
//! Honest limits: no conflict resolution exists because none is
//! needed (nothing is ever edited in place — append or atomic
//! replace); a synced folder that lags means pull sees what has
//! arrived, no more; the roster is TOFU, and a mount that silently
//! drops fsync or locks will be caught by the suitability probe at
//! join and by verify at pull, not prevented. No server, no ports, no
//! daemons: if the folder is reachable, the team is working.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use wright_core::events::now_ms;
use wright_core::jcs::{did_key, did_key_public};
use wright_core::wire::json::{self, Value};

use crate::signing::{to_hex, SigningKey};

/// One line of the shared roster: a machine that joined this store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The machine's receipt-signing key as a `did:key` (the same
    /// vocabulary the identity system uses).
    pub did: String,
    /// What the machine calls itself (defaults to the hostname).
    pub name: String,
    /// The hostname at join time (a name is chosen, a host is a fact).
    pub host: String,
    pub joined_ms: u64,
}

impl Member {
    fn to_value(&self) -> Value {
        Value::obj(vec![
            ("type", Value::str("wright.team-member/1")),
            ("did", Value::str(self.did.clone())),
            ("name", Value::str(self.name.clone())),
            ("host", Value::str(self.host.clone())),
            ("joined_ms", Value::int(self.joined_ms as i64)),
        ])
    }

    fn from_value(v: &Value) -> Option<Member> {
        Some(Member {
            did: v.str_of("did")?.to_string(),
            name: v.str_of("name").unwrap_or("?").to_string(),
            host: v.str_of("host").unwrap_or("?").to_string(),
            joined_ms: v.get("joined_ms").and_then(|j| j.as_u64()).unwrap_or(0),
        })
    }

    /// The did as a hex public key — the form `verify` pins.
    pub fn public_hex(&self) -> Result<String, String> {
        Ok(to_hex(&did_key_public(&self.did)?))
    }
}

/// The roster file inside a shared store.
fn roster_path(root: &Path) -> PathBuf {
    root.join("team").join("roster.jsonl")
}

/// The pointer file `wright team join` writes: a sibling of this
/// machine's default store directory (never inside the project).
fn pointer_file(default_root: &Path) -> PathBuf {
    PathBuf::from(format!("{}.team", default_root.display()))
}

/// Where this project's store lives today: `WRIGHT_STORE` when set
/// (one shell, CI), else the pointer file, else none (the local
/// single-machine store). This is the one redirect [`crate::store`]
/// consults — every command that opens a store follows it.
pub fn active(default_root: &Path) -> Option<PathBuf> {
    if let Ok(v) = std::env::var("WRIGHT_STORE") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    let text = fs::read_to_string(pointer_file(default_root)).ok()?;
    let t = text.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(PathBuf::from(t))
    }
}

/// The shared store this project uses, when it uses one (resolves the
/// default root first — `wright where` prints exactly this).
pub fn shared_store(project_dir: &Path) -> Option<PathBuf> {
    active(&crate::store::Store::default_root(project_dir))
}

/// The roster, deduped by did: a machine that re-joins keeps one line
/// in the file (append-only) and the LATEST line wins on read.
pub fn roster(root: &Path) -> Result<Vec<Member>, String> {
    let p = roster_path(root);
    let text = match fs::read_to_string(&p) {
        Ok(t) => t,
        Err(_) => return Ok(Vec::new()), // no roster yet: an empty team
    };
    let mut members: Vec<Member> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v = json::parse(line).map_err(|e| format!("{} line {}: {e}", p.display(), n + 1))?;
        let m = Member::from_value(&v)
            .ok_or_else(|| format!("{} line {}: not a team-member record", p.display(), n + 1))?;
        members.retain(|x| x.did != m.did);
        members.push(m);
    }
    members.sort_by_key(|m| m.joined_ms);
    Ok(members)
}

/// The roster's dids as hex public keys — the pin list for `pull`.
pub fn pins(members: &[Member]) -> Vec<String> {
    members.iter().filter_map(|m| m.public_hex().ok()).collect()
}

/// Append one roster line, fsynced: the roster is part of the spine
/// (a join must survive a crash, like an event).
fn append_member(root: &Path, m: &Member) -> Result<(), String> {
    let p = roster_path(root);
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| format!("create {}: {e}", d.display()))?;
    }
    let line = format!("{}\n", json::to_string(&m.to_value()));
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(|e| format!("open {}: {e}", p.display()))?;
    f.write_all(line.as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| format!("append {}: {e}", p.display()))
}

/// `wright team join`: suitability, roster line, pointer. Returns the
/// member as joined. Idempotent — re-joining appends a fresh line for
/// the same did (name/host refresh) and rewrites the pointer.
pub fn join(project_dir: &Path, shared: &Path, name: Option<&str>) -> Result<Member, String> {
    let note = suitability(shared, project_dir)?;
    let key = SigningKey::resolve()?;
    let did = did_key(&wright_core::ed25519::public_key(&key.seed_for_signing()));
    let host = hostname();
    let member = Member {
        did,
        name: name.map(str::to_string).unwrap_or_else(|| host.clone()),
        host,
        joined_ms: now_ms(),
    };
    append_member(shared, &member)?;
    let default = crate::store::Store::default_root(project_dir);
    if let Some(d) = default.parent() {
        fs::create_dir_all(d).map_err(|e| format!("create {}: {e}", d.display()))?;
    }
    crate::store::write_atomic(
        &pointer_file(&default),
        shared.display().to_string().as_bytes(),
    )?;
    // The pointer is state, not data: keep it out of any listing eyes.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(pointer_file(&default), fs::Permissions::from_mode(0o600));
    }
    let _ = note;
    Ok(member)
}

/// `wright team leave`: remove this machine's pointer. The shared
/// store and the roster line stay (a left machine's receipts remain
/// verifiable; remove its line with an editor if the team decides to).
pub fn leave(project_dir: &Path) -> Result<bool, String> {
    let p = pointer_file(&crate::store::Store::default_root(project_dir));
    match fs::remove_file(&p) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("remove {}: {e}", p.display())),
    }
}

/// Is this directory usable as a shared store? The honest probe
/// `wright team join` and `wright doctor` both run: it must exist (or
/// be creatable), be outside the project AND not contain it (a store
/// an agent could write is a store an agent could forge), take an
/// fsync'd write (events are fsynced before they count), and read it
/// back. Returns the report (warnings included) or the refusal.
pub fn suitability(shared: &Path, project_dir: &Path) -> Result<String, String> {
    fs::create_dir_all(shared).map_err(|e| format!("cannot create {}: {e}", shared.display()))?;
    let shared_c = shared
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", shared.display()))?;
    let project_c = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    if shared_c.starts_with(&project_c) {
        return Err(format!(
            "{} is inside the project: sandboxed agents can write the project tree, so a store there could be forged. Put the shared store outside every member's checkout",
            shared_c.display()
        ));
    }
    if project_c.starts_with(&shared_c) {
        return Err(format!(
            "the project is inside {}: the store directory must hold only Wright's state, not a checkout",
            shared_c.display()
        ));
    }
    // The durability probe: an event is fsynced before it is considered
    // done; a mount that drops fsync would silently lose the spine.
    let probe = shared_c.join(format!(".wright-suitability-{}", std::process::id()));
    fs::write(&probe, b"probe").map_err(|e| format!("cannot write {}: {e}", probe.display()))?;
    {
        let f = fs::OpenOptions::new()
            .write(true)
            .open(&probe)
            .map_err(|e| format!("reopen {}: {e}", probe.display()))?;
        f.sync_all().map_err(|e| {
            format!(
                "{} does not take an fsync — event durability cannot be promised: {e}",
                shared_c.display()
            )
        })?;
    }
    let read_back = fs::read_to_string(&probe).unwrap_or_default();
    let _ = fs::remove_file(&probe);
    if read_back != "probe" {
        return Err(format!(
            "{} did not read back what was written to it",
            shared_c.display()
        ));
    }
    let mut notes = vec![format!(
        "outside the project, writable, fsync-durable ({})",
        shared_c.display()
    )];
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(md) = fs::metadata(&shared_c) {
            let mode = md.permissions().mode();
            if mode & 0o022 != 0 {
                notes.push(format!(
                    "WARNING: the directory is group/other-writable (mode {:o}) — everyone the file server already trusts can write the store; that is the team's boundary, Wright cannot narrow it",
                    mode & 0o777
                ));
            }
        }
    }
    Ok(notes.join("; "))
}

fn hostname() -> String {
    if let Ok(h) = fs::read_to_string("/etc/hostname") {
        let h = h.trim();
        if !h.is_empty() {
            return h.to_string();
        }
    }
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "this machine".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env vars are process-wide and tests run in parallel: everything
    /// here that touches WRIGHT_STORE or the signing-key env takes this
    /// lock — shared crate-wide, so signing's WRIGHT_HOME flips and
    /// these tests serialize against each other too.
    use crate::TEST_ENV_LOCK as ENV_LOCK;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-team-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// The roster dedups by did, last line wins, ordered by join time.
    #[test]
    fn the_roster_dedups_by_did() {
        let root = tmp("roster");
        let a = Member {
            did: "did:key:z6MkA".into(),
            name: "alice".into(),
            host: "box-a".into(),
            joined_ms: 100,
        };
        let a2 = Member {
            did: "did:key:z6MkA".into(),
            name: "alice-laptop".into(),
            host: "box-a".into(),
            joined_ms: 300,
        };
        let b = Member {
            did: "did:key:z6MkB".into(),
            name: "bob".into(),
            host: "box-b".into(),
            joined_ms: 200,
        };
        append_member(&root, &a).unwrap();
        append_member(&root, &b).unwrap();
        append_member(&root, &a2).unwrap();
        let members = roster(&root).unwrap();
        assert_eq!(members.len(), 2, "{members:?}");
        assert_eq!(members[0].name, "bob", "join order");
        assert_eq!(members[1].name, "alice-laptop", "the latest line wins");
        // A missing roster is an empty team, not an error.
        assert!(roster(&tmp("empty")).unwrap().is_empty());
    }

    /// The pins are the roster's dids as hex keys.
    #[test]
    fn pins_are_hex_public_keys() {
        // did_key_public round-trips: build a real did from a key.
        let seed = [7u8; 32];
        let pk = wright_core::ed25519::public_key(&seed);
        let did = did_key(&pk);
        let m = Member {
            did,
            name: "a".into(),
            host: "h".into(),
            joined_ms: 1,
        };
        let pinned = pins(&[m]);
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].len(), 64, "{}", pinned[0]);
        assert_eq!(pinned[0], to_hex(&pk));
        // A did that is not a key is skipped, not fatal.
        let bad = Member {
            did: "did:key:not-base58!!".into(),
            name: "b".into(),
            host: "h".into(),
            joined_ms: 2,
        };
        assert!(pins(&[bad]).is_empty());
    }

    /// Suitability refuses a store inside the project, and a project
    /// inside the store, and reports a good directory.
    #[test]
    fn suitability_refuses_overlap_with_the_project() {
        let project = tmp("proj");
        let inside = project.join("shared");
        let err = suitability(&inside, &project).unwrap_err();
        assert!(err.contains("inside the project"), "{err}");

        let outer = tmp("outer");
        let project_in_store = outer.join("checkout");
        fs::create_dir_all(&project_in_store).unwrap();
        let err = suitability(&outer, &project_in_store).unwrap_err();
        assert!(err.contains("must hold only Wright's state"), "{err}");

        let good = tmp("good");
        let note = suitability(&good, &project).unwrap();
        assert!(note.contains("fsync-durable"), "{note}");
        // The probe cleans up after itself.
        assert!(!good
            .join(format!(".wright-suitability-{}", std::process::id()))
            .exists());
    }

    /// The pointer: join writes it, active follows it, leave removes it.
    /// (WRIGHT_STORE is unset here — the unit tests never depend on the
    /// ambient environment; the env override has its own test below.)
    #[test]
    fn the_pointer_directs_the_store() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("WRIGHT_STORE");
        let project = tmp("pointer");
        let shared = tmp("pointer-shared");
        let default = crate::store::Store::default_root(&project);
        assert_eq!(active(&default), None, "no pointer yet");

        // A deterministic key, so the roster line is checkable.
        std::env::set_var("WRIGHT_SECRET_WRIGHT_RECEIPT_SIGNING", to_hex(&[9u8; 32]));
        let m = join(&project, &shared, Some("test-box")).unwrap();
        assert!(m.did.starts_with("did:key:z6Mk"), "{}", m.did);
        assert_eq!(m.name, "test-box");
        assert_eq!(active(&default), Some(shared.clone()));
        assert!(shared.join("team/roster.jsonl").is_file());
        let members = roster(&shared).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "test-box");

        assert!(leave(&project).unwrap(), "the pointer existed");
        assert_eq!(active(&default), None);
        assert!(!leave(&project).unwrap(), "already gone");
        // The roster line stays: a left machine's receipts verify.
        assert_eq!(roster(&shared).unwrap().len(), 1);
        std::env::remove_var("WRIGHT_SECRET_WRIGHT_RECEIPT_SIGNING");
    }

    /// WRIGHT_STORE overrides the pointer for one shell — and the
    /// environment always outranks the file, both ways.
    #[test]
    fn the_env_overrides_the_pointer() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let project = tmp("env");
        let default = crate::store::Store::default_root(&project);
        let shared = tmp("env-shared");
        let other = tmp("env-other");
        if let Some(d) = default.parent() {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(pointer_file(&default), other.display().to_string()).unwrap();
        std::env::set_var("WRIGHT_STORE", shared.display().to_string());
        assert_eq!(active(&default), Some(shared));
        std::env::remove_var("WRIGHT_STORE");
        assert_eq!(active(&default), Some(other.clone()));
        std::env::set_var("WRIGHT_STORE", "  ");
        assert_eq!(active(&default), Some(other), "blank is unset");
        std::env::remove_var("WRIGHT_STORE");
        let _ = fs::remove_file(pointer_file(&default));
    }
}
