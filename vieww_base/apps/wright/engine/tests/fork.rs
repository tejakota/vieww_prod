//! WR-12 end to end: fire a two-agent project against the mock provider,
//! fork it right after the first agent's artifact was written, and check
//! the fork: events 1..=N byte-identical, lineage stamped, artifacts as of
//! N (the second agent's output absent), and a fresh firing of the fork
//! whose receipt verifies.

use wright_core::events::EventKind;
use wright_engine::approve::AutoApprove;
use wright_engine::mock::MockProvider;
use wright_engine::{fire, FireOptions};
use wright_runtime::config::Config;
use wright_runtime::store::Store;

fn options(mock: &MockProvider) -> FireOptions {
    let mut config = Config::parse(
        &format!(
            "provider \"mock\" kind=\"openai\" base=\"{}\" models=\"mock-*\"\n",
            mock.base_url()
        ),
        "test config",
    )
    .unwrap();
    for w in config.workers.iter_mut() {
        w.command = "/no/such/agent".into();
    }
    FireOptions {
        approver: Box::new(AutoApprove),
        config: Some(config),
        allow_private_network: true,
        ..FireOptions::default()
    }
}

#[test]
fn a_fork_at_an_event_carries_exactly_that_state_and_fires_on_its_own() {
    let base = std::env::temp_dir().join(format!("wright-wr12-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("home")).unwrap();
    std::env::set_var("WRIGHT_HOME", base.join("home"));
    let dir = base.join("notes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("blueprint.kdl"),
        r#"meta { name "WR-12 fork" }
intent { summary "Draft, then polish." confirmed "v1" }
project { language "python" }
agent Drafter {
  goal "Write a draft"
  out "draft.md"
}
agent Polisher {
  goal "Polish the draft"
  in "draft.md"
  out "final.md"
}
edge from="Drafter" to="Polisher"
guardrails { budget per-firing=500000 daily=3000000 wall-clock 300 max-turns 60 }
deliverable kind="file" { check "final.md exists." }
models { tier cheap prefer="mock-1" }
"#,
    )
    .unwrap();
    let mock = MockProvider::start(0).unwrap();
    let first = fire(&dir, options(&mock)).expect("the firing runs");
    let run = first.run;

    let store = Store::open(&dir).unwrap();
    let log = store.read_events(run).unwrap();
    let at = log
        .events()
        .iter()
        .find(|e| e.kind == EventKind::ArtifactWritten && e.note.contains("draft.md"))
        .expect("the draft was written")
        .seq;
    assert!(
        log.events()
            .iter()
            .any(|e| e.seq > at && e.note.contains("final.md")),
        "the polish comes after the fork point"
    );

    let into = base.join("notes-fork");
    let f = wright_engine::fork::fork(&dir, run, at, &into).expect("the fork");
    assert_eq!(f.events as u64, at);
    assert_eq!(f.lineage, format!("fork://WR-12 fork/run-{run}@{at}"));
    assert_eq!(f.restored, vec!["artifacts/draft.md".to_string()], "{f:?}");
    assert!(f.missing.is_empty(), "{f:?}");
    assert!(
        !into.join("artifacts/final.md").exists(),
        "a later artifact leaked in"
    );
    assert!(std::fs::read_to_string(into.join("blueprint.kdl"))
        .unwrap()
        .contains(&format!("lineage \"fork://WR-12 fork/run-{run}@{at}\"")));

    // Events 1..=N, byte for byte, as the fork's run 1.
    let src = std::fs::read_to_string(store.run_dir(run).join("events.jsonl")).unwrap();
    let new_store = Store::open(&into).unwrap();
    let copied = std::fs::read_to_string(new_store.run_dir(1).join("events.jsonl")).unwrap();
    let want: String = src
        .lines()
        .take(at as usize)
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(copied, want);

    // A fork never overwrites: the same target again is refused.
    assert!(wright_engine::fork::fork(&dir, run, at, &into).is_err());
    assert!(wright_engine::fork::fork(&dir, run, 0, &base.join("x")).is_err());

    // The fork fires on its own, and its receipt verifies.
    let second = fire(&into, options(&mock)).expect("the fork fires");
    assert_eq!(second.run, 2, "the carried-over record is run 1");
    let v = wright_engine::verify::project_run(&into, None).unwrap();
    assert!(
        v.ok,
        "{:?}",
        v.checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| &c.note)
            .collect::<Vec<_>>()
    );
}
