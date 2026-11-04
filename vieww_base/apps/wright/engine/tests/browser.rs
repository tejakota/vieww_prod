//! The browser tool through the whole machine (WR-11).
//!
//! The unit suites in `wright-tools` cover the loopback server, the
//! golden diff and the refusals. What only this file can prove is the
//! WIRING: the palette entry (caps, legs, effect) → the compiled task
//! → the gate → the agent loop → the tool result → the receipt. The
//! mock provider drives one `render` call, exactly the way a real model
//! would.
//!
//! Determinism: this machine (and CI) has no browser, but a developer's
//! machine might — every test here pins `WRIGHT_BROWSER_BIN` at a
//! nonexistent path (under a lock, the house pattern for process-wide
//! env) so the outcome is the honest refusal, never a real render.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use wright_core::events::EventKind;
use wright_engine::mock::MockProvider;
use wright_engine::{fire, FireOptions, Trigger};
use wright_runtime::config::Config;
use wright_runtime::store::Store;

/// Env vars are process-wide and tests run in parallel (signing.rs's
/// pattern): everything that flips WRIGHT_BROWSER_BIN takes this lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// One provider for the whole binary, like trust.rs.
fn mock_env() -> &'static (PathBuf, MockProvider) {
    static ENV: OnceLock<(PathBuf, MockProvider)> = OnceLock::new();
    ENV.get_or_init(|| {
        let h = std::env::temp_dir().join(format!("wright-browser-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&h);
        std::fs::create_dir_all(&h).unwrap();
        std::env::set_var("WRIGHT_HOME", &h);
        std::env::set_var("WRIGHT_EPHEMERAL_SIGNING", "1");
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
        "browser test config",
    )
    .unwrap()
}

fn project(tag: &str, blueprint: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wright-browser-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("blueprint.kdl"), blueprint).unwrap();
    d
}

fn fire_once(dir: &std::path::Path) -> wright_engine::RunSummary {
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
            no_cache: true,
        },
    )
    .unwrap()
}

/// Pin the browser hunt at a nonexistent path: the refusal is then a
/// property of the config, not of the machine the test runs on.
fn no_browser_here() -> MutexGuard<'static, ()> {
    let g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    std::env::set_var("WRIGHT_BROWSER_BIN", "/no/such/browser/chrome");
    g
}

#[test]
fn a_render_call_flows_to_the_refusal_and_the_receipt_records_it() {
    let _guard = no_browser_here();
    let (_, _) = mock_env();
    // A public IP literal, so the SSRF check passes without DNS and the
    // refusal that follows is the browser's, not the gate's.
    let dir = project(
        "wired",
        "meta { name \"look-demo\" }\n\
intent { summary \"look at a page\" confirmed \"v1\" }\n\
agent Lookout { goal \"render http://93.184.216.34/ and describe what is there\" tools \"browser\" out \"look.json\" permissions network=true }\n\
chips { tests on }\n\
models { tier cheap prefer=\"mock-1\" }\n\
deliverable kind=\"report\" { check \"a look exists\" accept \"the look was made\" }\n",
    );
    let s = fire_once(&dir);
    assert!(
        s.ok,
        "the run passes; a refused render is a tool result, not a failed run: {s:?}"
    );

    // The event river saw the call and its refusal.
    let store = Store::open(&dir).unwrap();
    let log = store.read_events(s.run).unwrap();
    let call = log
        .events()
        .iter()
        .find(|e| e.kind == EventKind::ToolCalled && e.data.contains("\"browser\""))
        .expect("a browser tool call was emitted");
    assert!(
        call.note.contains("error"),
        "the refusal is visible: {note}",
        note = call.note
    );
    assert!(
        call.data.contains("\"ok\":false"),
        "the structured record says the call did not succeed: {data}",
        data = call.data
    );

    // The receipt attributes the call to the agent, failures counted.
    let receipt = std::fs::read_to_string(&s.receipt).unwrap();
    assert!(receipt.contains("look-demo"), "{receipt}");
    let parsed = wright_core::wire::json::parse(&receipt).unwrap();
    let tools = parsed
        .get("spend")
        .and_then(|s| s.get("agents"))
        .and_then(|a| a.as_arr().and_then(|x| x.first().cloned()))
        .and_then(|a| a.get("tools").and_then(|t| t.as_arr().map(|x| x.to_vec())))
        .expect("spend.agents[0].tools");
    let browser = tools
        .iter()
        .find(|t| t.str_of("tool") == Some("browser"))
        .expect("the browser call is attributed");
    assert_eq!(
        browser.get("calls").and_then(|c| c.as_u64()),
        Some(1),
        "{receipt}"
    );
    assert_eq!(
        browser.get("failed").and_then(|f| f.as_u64()),
        Some(1),
        "the refusal is counted as a failed call: {receipt}"
    );
    std::env::remove_var("WRIGHT_BROWSER_BIN");
}

#[test]
fn the_validator_refuses_browser_without_the_network_permission() {
    // The palette says the browser needs the Network capability, and the
    // blueprint validator enforces it BEFORE any firing: a card that
    // lists `browser` without `permissions network=true` never runs.
    // (Defense in depth: the gate re-checks at call time; this is the
    // earlier wall.)
    let bp = wright_core::blueprint::parse::blueprint(
        "meta { name \"look-nocap\" }\n\
intent { summary \"look without permission\" confirmed \"v1\" }\n\
agent Lookout { goal \"render http://93.184.216.34/ anyway\" tools \"browser\" out \"look.json\" }\n",
    )
    .unwrap();
    let errs = wright_core::blueprint::validate::validate(&bp).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.contains("browser") && e.contains("network")),
        "the validator names the tool and the missing capability: {errs:?}"
    );
}
