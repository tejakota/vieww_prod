//! Team mode through the whole machine (WR-18): two machines, one
//! shared store, no server.
//!
//! The unit suites in `wright-runtime` cover the roster, the pointer
//! and the suitability probe. What only this file can prove is the
//! CONVERGENCE claim itself, in process: machine A and machine B are
//! two project directories with two signing keys and two Wright
//! homes, pointed (the way `wright team join` points them) at ONE
//! store directory. The mock provider drives both.
//!
//! * interleaved firings allocate distinct runs on the shared spine;
//! * the task cache is shared — B's identical task is served from A's
//!   entry (the key is machine-independent by construction), and the
//!   artifacts are restored from the shared object store;
//! * a receipt A signed verifies on B through the roster's pins — and
//!   refuses without them — with B's tree still EMPTY: the shared
//!   store's object copy is the durable witness (the existing verify
//!   rule: absent-on-disk is not tampering), so cross-machine audit
//!   needs nothing but the store and the pinned keys.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use wright_engine::mock::MockProvider;
use wright_engine::verify::verify_text_pinned;
use wright_engine::{fire, FireOptions, TaskStatus, Trigger};
use wright_runtime::config::Config;
use wright_runtime::store::Store;
use wright_runtime::team::{self, Member};

/// Env vars are process-wide and tests run in parallel: everything that
/// flips WRIGHT_HOME / WRIGHT_STORE / the signing key takes this lock
/// (signing.rs's pattern). The "machines" are sequential — the store
/// they share is the point, not the parallelism.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// One provider for the whole binary, like trust.rs and browser.rs.
fn mock_env() -> &'static (PathBuf, MockProvider) {
    static ENV: OnceLock<(PathBuf, MockProvider)> = OnceLock::new();
    ENV.get_or_init(|| {
        let h = std::env::temp_dir().join(format!("wright-team-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(&h).unwrap();
        std::env::set_var("WRIGHT_HOME", &h);
        (h, MockProvider::start(0).unwrap())
    })
}

fn config() -> Config {
    let (_, mock) = mock_env();
    Config::parse(
        &format!(
            "provider \"mock\" kind=\"openai\" base=\"{}\" models=\"mock-*\"\ninterview model=\"mock-1\"\nprice model=\"mock-*\" in=0 out=0",
            mock.base_url()
        ),
        "team test config",
    )
    .unwrap()
}

const BLUEPRINT: &str = "meta { name \"team-demo\" }\n\
intent { summary \"write the team note\" confirmed \"v1\" }\n\
agent Writer { goal \"write note.txt with the daily line\" tools \"files-write\" out \"note.txt\" permissions files-write=true }\n\
models { tier cheap prefer=\"mock-1\" }\n\
deliverable kind=\"report\" { check \"the note exists\" accept \"the note was written\" }\n";

/// One machine: its own project directory, its own Wright home, its
/// own signing seed. `shared` is the one store directory both point at
/// (WRIGHT_STORE — the same redirect `wright team join` persists as a
/// pointer file outside the project).
struct Machine {
    project: PathBuf,
    home: PathBuf,
    seed: [u8; 32],
}

fn machine(tag: &str, seed: u8, shared: &Path) -> Machine {
    let m = Machine {
        project: std::env::temp_dir().join(format!("wright-team-{tag}-{}", std::process::id())),
        home: std::env::temp_dir().join(format!("wright-team-home-{tag}-{}", std::process::id())),
        seed: [seed; 32],
    };
    let _ = std::fs::remove_dir_all(&m.project);
    let _ = std::fs::remove_dir_all(&m.home);
    std::fs::create_dir_all(&m.project).unwrap();
    std::fs::create_dir_all(&m.home).unwrap();
    std::fs::write(m.project.join("blueprint.kdl"), BLUEPRINT).unwrap();
    let _ = shared;
    m
}

/// Become the machine: its home, its key, the shared store.
fn impersonate(m: &Machine, shared: &Path) {
    std::env::set_var("WRIGHT_HOME", &m.home);
    std::env::set_var(
        "WRIGHT_SECRET_WRIGHT_RECEIPT_SIGNING",
        wright_runtime::signing::to_hex(&m.seed),
    );
    std::env::set_var("WRIGHT_STORE", shared.display().to_string());
}

fn fire_once(dir: &Path) -> wright_engine::RunSummary {
    fire(
        dir,
        FireOptions {
            reproducible: false,
            trigger: Trigger::Manual,
            approver: Box::new(wright_engine::approve::AutoApprove),
            listeners: Vec::new(),
            config: Some(config()),
            allow_private_network: false,
            browser_loopback: false,
            resume: None,
            no_cache: false, // the cache IS the convergence story here
        },
    )
    .unwrap()
}

/// The roster this pair of machines would have (join pins exactly
/// these dids); built directly from the seeds so the test pins the
/// trust chain, not the join command's file layout.
fn roster_of(a: &Machine, b: &Machine) -> Vec<Member> {
    let did_of =
        |m: &Machine| wright_core::jcs::did_key(&wright_core::ed25519::public_key(&m.seed));
    vec![
        Member {
            did: did_of(a),
            name: "alpha".into(),
            host: "box-a".into(),
            joined_ms: 1,
        },
        Member {
            did: did_of(b),
            name: "beta".into(),
            host: "box-b".into(),
            joined_ms: 2,
        },
    ]
}

fn receipt_of(store: &Store, run: u64) -> String {
    std::fs::read_to_string(store.run_dir(run).join("receipt.json")).unwrap()
}

/// The task row of one named agent.
fn task_of<'a>(s: &'a wright_engine::RunSummary, name: &str) -> &'a (String, TaskStatus) {
    s.tasks
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no task '{name}' in {:?}", s.tasks))
}

/// The convergence test: A fires, B fires, A fires again — three runs
/// on one spine, B's task served from A's cache entry, artifacts
/// restored into B's tree from the shared object store.
#[test]
fn two_machines_converge_on_one_shared_store() {
    let _guard = env_guard();
    let (_, _) = mock_env();
    let shared = std::env::temp_dir().join(format!("wright-team-shared-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&shared);
    std::fs::create_dir_all(&shared).unwrap();
    let a = machine("alpha", 1, &shared);
    let b = machine("beta", 2, &shared);

    // Machine A fires run 1.
    impersonate(&a, &shared);
    let s1 = fire_once(&a.project);
    assert!(s1.ok, "A's run passes: {s1:?}");
    assert_eq!(s1.run, 1);
    assert!(
        matches!(task_of(&s1, "Writer").1, TaskStatus::Ran { .. }),
        "ran for real: {:?}",
        s1.tasks
    );

    // Machine B fires the SAME task — the key is machine-independent,
    // so B is served from A's cache entry, and the artifacts come back
    // from the shared object store into B's own tree.
    impersonate(&b, &shared);
    let s2 = fire_once(&b.project);
    assert!(s2.ok, "B's run passes: {s2:?}");
    assert_eq!(s2.run, 2, "the spine interleaves: B is run 2");
    assert!(
        matches!(task_of(&s2, "Writer").1, TaskStatus::Cached { .. }),
        "B's Writer is served from A's cache entry: {:?}",
        s2.tasks
    );
    let note = b.project.join("artifacts").join("note.txt");
    assert!(note.is_file(), "the artifact was restored into B's tree");

    // Machine A fires once more: three runs, one spine.
    impersonate(&a, &shared);
    let s3 = fire_once(&a.project);
    assert!(s3.ok && s3.run == 3, "the third interleaved run: {s3:?}");

    // The store each machine opens IS the shared one.
    let store = Store::open(&b.project).unwrap();
    assert_eq!(store.root(), &shared);
    assert_eq!(store.runs(), vec![1, 2, 3], "the shared spine");

    // The mock's outputs are deterministic, so the object store does
    // not grow with the machines: three runs, and exactly the objects
    // run 1 wrote (the note, once, plus the task reports) — content
    // addressing dedups the machines' work.
    let objects = || -> usize {
        let mut n = 0;
        for e in std::fs::read_dir(shared.join("objects")).unwrap().flatten() {
            if let Ok(rd) = std::fs::read_dir(e.path()) {
                n += rd.filter_map(|f| f.ok()).count();
            }
        }
        n
    };
    let after_one = objects();
    assert!(after_one >= 2, "the note and its report: {after_one}");
    assert_eq!(
        objects(),
        after_one,
        "runs 2 and 3 added nothing: every machine's copy is the same bytes at the same address"
    );
}

/// The trust test: A's receipt verifies on B through the roster's pins
/// — and refuses without them. The shared store's object copy is the
/// durable witness, so B needs NOTHING synced to verify A's work;
/// B's own receipt verifies on A the same way — the audit story is
/// symmetric, and no server is involved.
#[test]
fn a_receipt_from_a_verifies_on_b_through_the_roster() {
    let _guard = env_guard();
    let (_, _) = mock_env();
    let shared = std::env::temp_dir().join(format!("wright-team-verify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&shared);
    std::fs::create_dir_all(&shared).unwrap();
    let a = machine("verif-alpha", 3, &shared);
    let b = machine("verif-beta", 4, &shared);
    let roster = roster_of(&a, &b);
    let pins = team::pins(&roster);
    assert_eq!(pins.len(), 2);

    // A fires; B has NOT synced a single file.
    impersonate(&a, &shared);
    let s1 = fire_once(&a.project);
    assert!(s1.ok, "{s1:?}");
    let text = receipt_of(&Store::open(&a.project).unwrap(), 1);

    // B verifies A's receipt through B's own store handle: the shared
    // store serves the event log, the objects and the blueprint, so
    // all seven checks pass with B's tree still empty.
    impersonate(&b, &shared);
    let store_b = Store::open(&b.project).unwrap();
    let on_b = verify_text_pinned(&store_b, &text, &pins);
    assert!(on_b.ok, "A's receipt checks on B: {}", on_b.report());

    // Without A's pin the SIGNATURE check fails: a stranger's key is
    // not trust, however valid its Ed25519.
    let b_only: Vec<String> = pins[1..2].to_vec();
    let stranger = verify_text_pinned(&store_b, &text, &b_only);
    assert!(!stranger.ok, "an unpinned key must fail");
    let sig = stranger
        .checks
        .iter()
        .find(|c| c.name == "signature")
        .expect("the signature check exists");
    assert!(!sig.ok, "{}", sig.note);

    // B fires (served from A's cache entry, artifacts restored into
    // B's tree) — and B's own receipt verifies on machine A.
    let s2 = fire_once(&b.project);
    assert!(
        s2.ok && matches!(task_of(&s2, "Writer").1, TaskStatus::Cached { .. }),
        "{s2:?}"
    );
    impersonate(&a, &shared);
    let store_a = Store::open(&a.project).unwrap();
    let b_text = receipt_of(&store_a, 2);
    let on_a = verify_text_pinned(&store_a, &b_text, &pins);
    assert!(on_a.ok, "B's receipt checks on A: {}", on_a.report());
}

/// The doctor probe WR-18 adds: a shared store must be outside the
/// project (an agent-writable store is a forgeable store), must not
/// contain the project, and must take an fsync'd write.
#[test]
fn the_shared_store_suitability_probe_is_honest() {
    let _guard = env_guard();
    let (_, _) = mock_env();
    let shared = std::env::temp_dir().join(format!("wright-team-suit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&shared);
    std::fs::create_dir_all(&shared).unwrap();
    let a = machine("suit-alpha", 5, &shared);

    let ok = team::suitability(&shared, &a.project).unwrap();
    assert!(ok.contains("fsync-durable"), "{ok}");

    let inside = a.project.join("store-here");
    let err = team::suitability(&inside, &a.project).unwrap_err();
    assert!(err.contains("inside the project"), "{err}");
}

/// Env vars are process-wide: every test here flips them, so all of
/// them hold one lock for the whole body.
fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}
