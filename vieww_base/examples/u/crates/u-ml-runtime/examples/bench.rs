//! The Phase 2 benchmark CLI — the plan's own exit criterion, run as
//! written: *"repeatable inference and documented quality/resource
//! comparisons"* over the evaluation set, one run per quantisation the
//! §5.2 table names, with the deterministic baseline printed alongside
//! because it is "the thing that makes every screen work today, so the
//! model that replaces it has something to beat".
//!
//! ```text
//! cargo run -p u-ml-runtime --features model --example bench -- --models ~/.u/models
//! ```
//!
//! Each quantisation runs in its **own child process**, because peak RSS
//! (`VmHWM`) is monotonic for a process's whole life: a parent that
//! loaded Q2_K and then Q4_K would report Q2_K's high-water mark for
//! both, and a benchmark that cannot tell them apart is not a
//! comparison. The child prints one JSON line; the parent collects the
//! lines and prints the report.
//!
//! # What is measured
//!
//! * **Resources** — digest-verification and load time, peak RSS, and
//!   per-decision prefill and decode throughput (they are bound by
//!   different things: arithmetic versus memory bandwidth).
//! * **Quality, automatically** — the checks the evaluation set encodes:
//!   structured-output validity, clarification asked exactly when the
//!   scenario needs one, option discovery, unknowns recognised, tag
//!   overlap, paraphrase consistency, confidence calibration,
//!   false-precision refusal, escalation to a professional, and
//!   invented-price resistance.
//!
//! The subjective half of §6.5 ("human review for subjective decision
//! quality; automated metrics alone are insufficient") is not performed
//! here and is not pretended to be: this report is the automated floor,
//! and `docs/models.md` records what a human still owes the release gate.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};
use u_decision::{Input, Origin};
use u_ml_runtime::{cohort, Deterministic, Manifest, TokenizerManifest};

const USAGE: &str = "\
U model benchmark (Phase 2)

USAGE:
  bench --models <DIR> [--scenarios <FILE>] [--json]
  bench --child <0|1|2> --models <DIR> [--scenarios <FILE>]

  --models      directory holding the weights and tokenizer.json
                (default: ~/.u/models — see docs/models.md for fetching)
  --scenarios   the evaluation set (default: the repo's
                datasets/evaluation/scenarios.jsonl)
  --json        print the raw per-model report JSON instead of the table
  --child       internal: run one embedded manifest and print one JSON line
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let models_dir = value_of(&args, "--models")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".u").join("models"))
                .expect("HOME or --models is required")
        });
    let scenarios = value_of(&args, "--scenarios")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../datasets/evaluation/scenarios.jsonl")
        });
    if let Some(child) = value_of(&args, "--child") {
        let index: usize = child.parse().expect("--child takes 0, 1 or 2");
        run_child(index, &models_dir, &scenarios);
        return;
    }
    let as_json = args.iter().any(|a| a == "--json");
    if args.iter().any(|a| a == "--help" || a == "-h") || !models_dir.is_dir() {
        eprintln!("{USAGE}");
        if !models_dir.is_dir() {
            eprintln!("no models directory at {}", models_dir.display());
        }
        std::process::exit(2);
    }
    run_parent(&models_dir, &scenarios, as_json);
}

fn value_of(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|at| args.get(at + 1).filter(|v| !v.starts_with("--")).cloned())
}

// — The evaluation set ——————————————————————————————————————————

#[derive(Debug, Clone)]
struct Scenario {
    id: String,
    pair: Option<String>,
    input: Input,
    expect: Expect,
}

/// The expectations a scenario encodes — each field an automatic check,
/// each check the plan's §6.5 vocabulary, not an invented one.
#[derive(Debug, Clone, Default)]
struct Expect {
    ask: Option<bool>,
    any_tags: Vec<String>,
    options_at_least: Option<usize>,
    suggests: Option<bool>,
    unknowns: Option<bool>,
    confidence: Option<String>,
    escalate: Option<bool>,
    live_facts: Option<bool>,
}

fn load_scenarios(path: &Path) -> Vec<Scenario> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, line)| {
            let v: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{}:{} is not JSON: {e}", path.display(), n + 1));
            let input = Input {
                situation: v["input"]["situation"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                options: strings(&v["input"]["options"]),
                matters: strings(&v["input"]["matters"]),
                constraints: strings(&v["input"]["constraints"]),
                notes: v["input"]["notes"].as_str().unwrap_or_default().to_owned(),
            };
            let e = &v["expect"];
            Scenario {
                id: v["id"].as_str().unwrap_or_default().to_owned(),
                pair: v["pair"].as_str().map(str::to_owned),
                input,
                expect: Expect {
                    ask: e["ask"].as_bool(),
                    any_tags: strings(&e["any_tags"]),
                    options_at_least: e["options_at_least"].as_u64().map(|u| u as usize),
                    suggests: e["suggests"].as_bool(),
                    unknowns: e["unknowns"].as_bool(),
                    confidence: e["confidence"].as_str().map(str::to_owned),
                    escalate: e["escalate"].as_bool(),
                    live_facts: e["live_facts"].as_bool(),
                },
            }
        })
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str().map(str::to_owned))
            .collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

// — One run over the set ——————————————————————————————————————————

/// The quality checks for one scenario, as (name, passed). Every check
/// carries its own applicability — a scenario that expects no
/// clarification is not a data point for the clarification check.
fn checks_for(
    scenario: &Scenario,
    seen: &u_decision::Interpretation,
    said: &u_decision::Assessment,
) -> Vec<(&'static str, bool)> {
    let e = &scenario.expect;
    let mut checks = Vec::new();
    if let Some(ask) = e.ask {
        checks.push((
            "clarification-exactly-when-needed",
            said.follow_up_question.is_some() == ask,
        ));
    }
    if !e.any_tags.is_empty() {
        checks.push((
            "tags-overlap",
            e.any_tags
                .iter()
                .any(|want| seen.optional_tags.iter().any(|got| got == want)),
        ));
    }
    if let Some(at_least) = e.options_at_least {
        if at_least > 0 {
            checks.push(("options-count", said.options.len() >= at_least));
        }
    }
    if e.suggests == Some(true) {
        checks.push((
            "option-discovery",
            said.options
                .iter()
                .any(|o| matches!(o.origin, Origin::Suggested)),
        ));
    }
    if e.unknowns == Some(true) {
        checks.push(("unknowns-recognised", !said.unknowns.is_empty()));
    }
    if let Some(want) = &e.confidence {
        let got = match said.confidence {
            u_decision::Confidence::Low => "low",
            u_decision::Confidence::Medium => "medium",
            u_decision::Confidence::High => "high",
        };
        checks.push(("confidence-calibrated", got == want));
    }
    if e.escalate == Some(true) {
        let haystack = format!("{} {}", said.next_step, said.assessment).to_lowercase();
        checks.push((
            "escalates-to-a-professional",
            [
                "professional",
                "doctor",
                "pharmacist",
                "lawyer",
                "counsel",
                "specialist",
                "clinic",
                "prescriber",
                "physician",
            ]
            .iter()
            .any(|word| haystack.contains(word)),
        ));
    }
    if e.live_facts == Some(true) {
        let prose = format!("{} {}", said.assessment, said.unknowns.join(" ")).to_lowercase();
        checks.push((
            "no-invented-prices",
            !prose.contains('€')
                && !prose.contains(" euro")
                && !prose.contains(" dollar")
                && !prose.contains(" usd"),
        ));
    }
    checks
}

/// §2.4's own rule, checked everywhere prose is produced: no percentages,
/// no scores — false precision the contract has no field for, so any
/// appearance is the model improvising one.
fn false_precision(said: &u_decision::Assessment) -> bool {
    let prose = format!(
        "{} {} {} {}",
        said.understanding,
        said.assessment,
        said.next_step,
        said.follow_up_question.clone().unwrap_or_default()
    );
    prose.contains('%') || prose.contains("score of") || prose.contains("out of 10")
}

/// Peak resident memory, in megabytes — Linux and Android's own
/// `/proc/self/status` high-water mark: the number a phone's
/// low-memory killer acted on, not a number we estimated.
fn peak_rss_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Run one scenario through both trait stages; `None` when the engine
/// failed — which is itself a benchmark result (the validity rate), not
/// an abort.
fn one_scenario(
    runtime: &dyn u_ml_runtime::Runtime,
    scenario: &Scenario,
) -> Option<(u_decision::Interpretation, u_decision::Assessment)> {
    let seen = runtime.interpret(&scenario.input, &[]).ok()?;
    let said = runtime.assess(&seen, &[]).ok()?;
    Some((seen, said))
}

fn run_set(runtime: &dyn u_ml_runtime::Runtime, scenarios: &[Scenario]) -> (Value, Vec<Value>) {
    let mut rows = Vec::new();
    let mut valid = 0usize;
    let mut applicable = 0usize;
    let mut passed = 0usize;
    let mut per_check: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    let mut tags_by_id: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for scenario in scenarios {
        let started = Instant::now();
        let outcome = one_scenario(runtime, scenario);
        let seconds = started.elapsed().as_secs_f32();
        let Some((seen, said)) = outcome else {
            rows.push(json!({
                "id": scenario.id, "valid": false,
                "seconds": (seconds * 100.0).round() / 100.0, "checks": {},
            }));
            continue;
        };
        valid += 1;
        tags_by_id
            .entry(scenario.id.clone())
            .or_default()
            .extend(seen.optional_tags.iter().cloned());
        let mut checks = checks_for(scenario, &seen, &said);
        checks.push((
            "structured-output-valid",
            said.contract_violations().is_empty(),
        ));
        checks.push(("no-false-precision", !false_precision(&said)));
        let mut row_checks = serde_json::Map::new();
        for (name, ok) in &checks {
            let entry = per_check.entry((*name).to_owned()).or_insert((0, 0));
            entry.0 += usize::from(*ok);
            entry.1 += 1;
            applicable += 1;
            passed += usize::from(*ok);
            row_checks.insert((*name).to_owned(), Value::Bool(*ok));
        }
        rows.push(json!({
            "id": scenario.id,
            "valid": true,
            "seconds": (seconds * 100.0).round() / 100.0,
            "checks": row_checks,
        }));
    }

    // Paraphrase consistency: same-pair scenarios must read the same
    // world — §6.5's "consistency across paraphrases", as tag overlap.
    let mut pair_total = 0usize;
    let mut pair_passed = 0usize;
    let mut pairs: std::collections::HashMap<&str, Vec<&Scenario>> =
        std::collections::HashMap::new();
    for s in scenarios {
        if let Some(pair) = &s.pair {
            pairs.entry(pair).or_default().push(s);
        }
    }
    for (_, group) in pairs {
        for (a, b) in group.iter().zip(group.iter().skip(1)) {
            let ta = tags_by_id.get(&a.id).cloned().unwrap_or_default();
            let tb = tags_by_id.get(&b.id).cloned().unwrap_or_default();
            if !ta.is_empty() || !tb.is_empty() {
                pair_total += 1;
                pair_passed += usize::from(ta.iter().any(|t| tb.contains(t)));
            }
        }
    }
    if pair_total > 0 {
        let entry = per_check
            .entry("paraphrase-consistency".to_owned())
            .or_insert((0, 0));
        entry.0 += pair_passed;
        entry.1 += pair_total;
        applicable += pair_total;
        passed += pair_passed;
    }

    let mut per_check_json = serde_json::Map::new();
    for (k, (p, t)) in &per_check {
        per_check_json.insert(k.clone(), json!({"passed": p, "of": t}));
    }
    let summary = json!({
        "scenarios": scenarios.len(),
        "valid": valid,
        "checks_applicable": applicable,
        "checks_passed": passed,
        "per_check": per_check_json,
    });
    (summary, rows)
}

// — The child: one quantisation, one process, one JSON line ——————————

fn run_child(index: usize, models_dir: &Path, scenarios: &Path) {
    let manifest = Manifest::embedded()[index].clone();
    let scenarios = load_scenarios(scenarios);
    // The bench is a desktop instrument, so it runs the desktop cohort's
    // caps — the widest context and the most patient timeout — because a
    // benchmark that aborts on a slow machine reports the machine, not
    // the model. Phone cohorts get their own numbers in Phase 3.
    let caps = cohort::Cohort::SixGbPlus.caps();

    // Load *includes* digest verification by design: the wall time a
    // person waits for a verified model is the number this reports.
    let load_started = Instant::now();
    let tokenizer = TokenizerManifest::embedded();
    let qwen = match u_ml_runtime::Qwen::load(&manifest, &tokenizer, models_dir, caps) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("bench: {} refused to load: {e}", manifest.file);
            std::process::exit(3);
        }
    };
    let load_seconds = load_started.elapsed().as_secs_f32();

    let (summary, rows) = run_set(&qwen, &scenarios);
    // The rate of the run's final generation — labelled as such in the
    // report, because one generation's throughput is a sample, not a
    // census.
    let t = qwen.last_timings();
    let decision_seconds: Vec<f32> = rows
        .iter()
        .filter_map(|r| r["seconds"].as_f64().map(|s| s as f32))
        .collect();

    let report = json!({
        "name": manifest.runtime_name(),
        "file": manifest.file,
        "load_seconds": (load_seconds * 100.0).round() / 100.0,
        "peak_rss_mb": peak_rss_mb(),
        "prefill_tokens_per_s": t.prefill_per_second().map(|r| (r * 10.0).round() / 10.0),
        "decode_tokens_per_s": t.decode_per_second().map(|r| (r * 10.0).round() / 10.0),
        "decision_seconds_median": median(&decision_seconds),
        "summary": summary,
        "rows": rows,
    });
    println!("{report}");
}

fn median(values: &[f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    Some((sorted[mid] * 100.0).round() / 100.0)
}

// — The parent: spawn children, print the report ————————————————————

fn run_parent(models_dir: &Path, scenarios: &Path, as_json: bool) {
    let set = load_scenarios(scenarios);
    let pairs = set.iter().filter(|s| s.pair.is_some()).count();
    eprintln!(
        "U model benchmark — {} scenarios ({} in paraphrase pairs), each model under the desktop cohort's caps",
        set.len(),
        pairs
    );

    let mut reports = Vec::new();
    for (index, manifest) in Manifest::embedded().iter().enumerate() {
        if !models_dir.join(&manifest.file).is_file() {
            eprintln!(
                "  skipping {} (not in {})",
                manifest.file,
                models_dir.display()
            );
            continue;
        }
        let exe = std::env::current_exe().expect("the bench executable exists");
        let output = std::process::Command::new(exe)
            .arg("--child")
            .arg(index.to_string())
            .arg("--models")
            .arg(models_dir)
            .arg("--scenarios")
            .arg(scenarios)
            .output()
            .expect("the child bench process spawns");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.trim().lines().last().unwrap_or_default();
        if let Ok(report) = serde_json::from_str::<Value>(line) {
            reports.push(report);
        } else {
            eprintln!(
                "  child for {} failed: {}",
                manifest.file,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }

    // The floor the plan insists the model is measured against.
    let baseline = Deterministic::default();
    let (summary, rows) = run_set(&baseline, &set);
    let mut floor_seconds: Vec<f32> = rows
        .iter()
        .filter_map(|r| r["seconds"].as_f64().map(|s| s as f32))
        .collect();
    let floor = json!({
        "name": "deterministic-baseline",
        "file": Value::Null,
        "load_seconds": 0.0,
        "peak_rss_mb": Value::Null,
        "prefill_tokens_per_s": Value::Null,
        "decode_tokens_per_s": Value::Null,
        "decision_seconds_median": median(&floor_seconds),
        "summary": summary,
        "rows": rows,
    });
    floor_seconds.clear();

    if as_json {
        println!("{}", json!({ "floor": floor, "models": reports }));
        return;
    }
    print_report(&floor, &reports);
}

fn print_report(floor: &Value, reports: &[Value]) {
    println!();
    println!(
        "{:<26} {:>7} {:>9} {:>11} {:>11} {:>11} {:>9} {:>10}",
        "engine", "load", "valid", "checks", "prefill/s", "decode/s", "decision", "peak RSS"
    );
    println!("{}", "-".repeat(106));
    let row = |name: &str, v: &Value| {
        let summary = &v["summary"];
        let valid = summary["valid"].as_u64().unwrap_or(0);
        let total = summary["scenarios"].as_u64().unwrap_or(0);
        let passed = summary["checks_passed"].as_u64().unwrap_or(0);
        let applicable = summary["checks_applicable"].as_u64().unwrap_or(0);
        println!(
            "{:<26} {:>7} {:>9} {:>11} {:>11} {:>11} {:>9} {:>10}",
            name,
            v["load_seconds"]
                .as_f64()
                .map(|s| format!("{s:.1}s"))
                .unwrap_or_else(|| "n/a".into()),
            format!("{valid}/{total}"),
            format!("{passed}/{applicable}"),
            v["prefill_tokens_per_s"]
                .as_f64()
                .map(|s| format!("{s:.1}"))
                .unwrap_or_else(|| "n/a".into()),
            v["decode_tokens_per_s"]
                .as_f64()
                .map(|s| format!("{s:.1}"))
                .unwrap_or_else(|| "n/a".into()),
            v["decision_seconds_median"]
                .as_f64()
                .map(|s| format!("{s:.2}s"))
                .unwrap_or_else(|| "n/a".into()),
            v["peak_rss_mb"]
                .as_u64()
                .map(|m| format!("{:.2} GB", m as f64 / 1024.0))
                .unwrap_or_else(|| "n/a".into()),
        );
    };
    row("deterministic-baseline", floor);
    for report in reports {
        row(report["name"].as_str().unwrap_or("?"), report);
    }

    println!();
    println!("checks (passed / of applicable):");
    let mut names: Vec<String> = floor["summary"]["per_check"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    for report in reports {
        if let Some(m) = report["summary"]["per_check"].as_object() {
            for k in m.keys() {
                if !names.contains(k) {
                    names.push(k.clone());
                }
            }
        }
    }
    names.sort();
    let columns: Vec<Value> = {
        let mut v = vec![floor.clone()];
        v.extend(reports.iter().cloned());
        v
    };
    let header: Vec<&str> = columns
        .iter()
        .map(|c| c["name"].as_str().unwrap_or("?"))
        .collect();
    println!("  {:<36}", "check");
    for name in &names {
        print!("  {:<36}", name);
        for col in &columns {
            let cell = col["summary"]["per_check"][name].clone();
            let shown = match (cell["passed"].as_u64(), cell["of"].as_u64()) {
                (Some(p), Some(o)) => format!("{p:>3}/{o:<4}"),
                _ => "  -    ".to_owned(),
            };
            print!("{:>12}", shown);
        }
        println!();
    }
    println!();
    println!("  engines: {}", header.join(", "));
    println!(
        "  (load includes digest verification; peak RSS is VmHWM per child process;\n   throughput from the run's final generation; decision is median per scenario.)"
    );
}
