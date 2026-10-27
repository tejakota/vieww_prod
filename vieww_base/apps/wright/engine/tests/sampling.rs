//! WR-09 end to end: sampling pinned in the blueprint (or asked for with
//! `--reproducible`) reaches the provider's request bytes, is recorded on
//! every model call, and is signed into the receipt, which `verify`
//! checks against the event log.

mod common;

use common::recording_proxy;
use wright_core::wire::json::{self, Value};
use wright_engine::approve::AutoApprove;
use wright_engine::mock::MockProvider;
use wright_engine::{fire, FireOptions};
use wright_runtime::config::Config;

fn home() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let home = std::env::temp_dir().join(format!("wright-wr09-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("WRIGHT_HOME", &home);
    });
}

fn project(name: &str, models: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-wr09-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("blueprint.kdl"),
        format!(
            r#"meta {{ name "WR-09 {name}" }}
intent {{ summary "Write a note." confirmed "v1" }}
project {{ language "python" }}
agent Writer {{
  goal "Write a short note"
  out "note.md"
}}
guardrails {{ budget per-firing=500000 daily=3000000 wall-clock 300 max-turns 60 }}
deliverable kind="file" {{ check "note.md exists." }}
{models}
"#
        ),
    )
    .unwrap();
    dir
}

/// Fire `dir` through a recording proxy; the recorded request bodies.
fn fire_recorded(dir: &std::path::Path, reproducible: bool) -> (Vec<Value>, std::path::PathBuf) {
    let mock = MockProvider::start(0).unwrap();
    let (proxy, seen) = recording_proxy(mock.port);
    let mut config = Config::parse(
        &format!(
            "provider \"mock\" kind=\"openai\" base=\"http://127.0.0.1:{proxy}/v1\" models=\"mock-*\"\n"
        ),
        "test config",
    )
    .unwrap();
    for w in config.workers.iter_mut() {
        w.command = "/no/such/agent".into();
    }
    let summary = fire(
        dir,
        FireOptions {
            approver: Box::new(AutoApprove),
            config: Some(config),
            allow_private_network: true,
            reproducible,
            ..FireOptions::default()
        },
    )
    .expect("the firing runs");
    let bodies = seen
        .lock()
        .unwrap()
        .iter()
        .filter_map(|b| json::parse(b).ok())
        .filter(|v| v.get("messages").is_some())
        .collect();
    (bodies, summary.receipt)
}

fn writer_requests(bodies: &[Value]) -> Vec<&Value> {
    bodies
        .iter()
        .filter(|b| {
            b.get("messages")
                .and_then(Value::as_arr)
                .and_then(|m| m.first())
                .and_then(|m| m.str_of("content"))
                .is_some_and(|c| c.contains("Writer"))
        })
        .collect()
}

#[test]
fn pinned_sampling_reaches_the_provider_and_the_signed_receipt() {
    home();
    let dir = project(
        "pinned",
        r#"models { tier cheap prefer="mock-1" temp=0.2 top-p=0.9 seed=7 }"#,
    );
    let (bodies, receipt) = fire_recorded(&dir, false);
    let writer = writer_requests(&bodies);
    assert!(!writer.is_empty(), "no Writer request was recorded");
    for body in &writer {
        assert_eq!(
            body.get("temperature").and_then(Value::as_f64),
            Some(0.2),
            "{}",
            json::to_string(body)
        );
        assert_eq!(body.get("top_p").and_then(Value::as_f64), Some(0.9));
        assert_eq!(body.get("seed").and_then(Value::as_u64), Some(7));
    }

    let text = std::fs::read_to_string(&receipt).unwrap();
    let r = json::parse(&text).unwrap();
    assert_eq!(r.str_of("format"), Some("wright.run-receipt/3"));
    let rows = r
        .get("sampling")
        .and_then(Value::as_arr)
        .expect("sampling rows");
    assert!(
        rows.iter().any(|row| row.str_of("agent") == Some("Writer")
            && row.get("temperature").and_then(Value::as_f64) == Some(0.2)
            && row.get("seed").and_then(Value::as_u64) == Some(7)),
        "{text}"
    );

    let v = wright_engine::verify::project_run(&dir, None).unwrap();
    let sampling = v
        .checks
        .iter()
        .find(|c| c.name == "sampling")
        .expect("a sampling check");
    assert!(sampling.ok, "{}", sampling.note);
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

#[test]
fn unpinned_sampling_sends_nothing_and_reproducible_pins_zero() {
    home();
    let dir = project("defaults", r#"models { tier cheap prefer="mock-1" }"#);
    let (bodies, receipt) = fire_recorded(&dir, false);
    for body in writer_requests(&bodies) {
        for k in ["temperature", "top_p", "seed"] {
            assert!(
                body.get(k).is_none(),
                "{k} was sent though nothing pinned it"
            );
        }
    }
    let r = json::parse(&std::fs::read_to_string(&receipt).unwrap()).unwrap();
    assert_eq!(r.get("reproducible").and_then(Value::as_bool), Some(false));

    let dir = project("reproducible", r#"models { tier cheap prefer="mock-1" }"#);
    let (bodies, receipt) = fire_recorded(&dir, true);
    let writer = writer_requests(&bodies);
    assert!(!writer.is_empty());
    for body in writer {
        assert_eq!(body.get("temperature").and_then(Value::as_f64), Some(0.0));
        assert_eq!(body.get("seed").and_then(Value::as_u64), Some(0));
        assert!(body.get("top_p").is_none());
    }
    let r = json::parse(&std::fs::read_to_string(&receipt).unwrap()).unwrap();
    assert_eq!(r.get("reproducible").and_then(Value::as_bool), Some(true));
}
