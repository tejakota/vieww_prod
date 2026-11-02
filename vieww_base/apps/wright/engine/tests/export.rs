//! Exports, checked by the tools that will read them: Python's `zipfile`
//! and pip for zips and wheels, cargo for crates, a real Vite build for
//! the web (only with `WRIGHT_TEST_NPM=1`; it downloads packages).

use std::path::{Path, PathBuf};
use std::process::Command;

use wright_core::blueprint::parse;
use wright_core::convention;
use wright_core::wire::json::{self, Value};
use wright_engine::export::{self, Kind};
use wright_engine::verify;
use wright_runtime::store::Store;

/// The tests here set WRIGHT_HOME (process-wide), so they take turns.
static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A fresh project of `lang`: its skeleton and blueprint, under a private
/// WRIGHT_HOME. `verified` records a passing run (what the result screen
/// would show), so packages are allowed.
fn project(lang: &str, verified: bool) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "wright-export-{lang}-{verified}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::env::set_var("WRIGHT_HOME", base.join("home"));
    let dir = base.join("projects").join(format!("{lang}-thing"));
    std::fs::create_dir_all(&dir).unwrap();
    let text = format!(
        "meta {{ name \"Export Check {lang}\" }}\nintent {{ summary \"Count words, export cleanly.\" confirmed \"v1\" }}\nproject {{ language \"{lang}\" }}\n\
agent Writer {{ goal \"write the code\" tools \"files-write\" permissions files-write=true }}\ndeliverable kind=\"code\" {{ check \"tests pass\" }}\n"
    );
    std::fs::write(dir.join("blueprint.kdl"), &text).unwrap();
    for f in convention::skeleton(&parse::blueprint(&text).unwrap()) {
        let p = dir.join(&f.path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, &f.contents).unwrap();
    }
    // Things an export must leave out.
    std::fs::write(dir.join("logs/main.log"), "{\"level\":\"info\"}\n").unwrap();
    std::fs::create_dir_all(dir.join("node_modules/left-pad")).unwrap();
    std::fs::write(dir.join("node_modules/left-pad/index.js"), "x").unwrap();
    if verified {
        let store = Store::open(&dir).unwrap();
        let run = store.new_run().unwrap().run();
        std::fs::write(
            store.run_dir(run).join("receipt.json"),
            "{\"ok\":true,\"headline\":\"done\"}",
        )
        .unwrap();
    }
    dir
}

fn python(code: &str) -> String {
    let out = Command::new("python3")
        .args(["-I", "-c", code])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "python failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn names(zip: &Path) -> Vec<String> {
    python(&format!(
        "import zipfile; print('\\n'.join(zipfile.ZipFile({:?}).namelist()))",
        zip.display().to_string()
    ))
    .lines()
    .map(String::from)
    .collect()
}

// One test, in order: the process environment (WRIGHT_HOME) is shared.
#[test]
fn exports_are_what_other_tools_expect() {
    let _env = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !have("python3") {
        eprintln!("python3 not installed; skipping");
        return;
    }

    // ── the source zip: always allowed, never the junk, same bytes twice ──
    let dir = project("python", false);
    let out = export::default_out(&dir);
    assert!(out.ends_with("Exports/python-thing"), "{}", out.display());
    let a = export::export(&dir, Kind::Zip, &out).unwrap();
    let b = export::export(&dir, Kind::Zip, &out).unwrap();
    assert_eq!(
        a.sha256, b.sha256,
        "the same project exports the same bytes"
    );
    let listed = names(&a.path);
    assert!(
        listed.contains(&"export-check-python/blueprint.kdl".to_string()),
        "{listed:?}"
    );
    assert!(listed.contains(&"export-check-python/src/export_check_python/__main__.py".to_string()));
    assert!(listed.contains(&"export-check-python/logs/.keep".to_string()));
    assert!(
        !listed
            .iter()
            .any(|n| n.contains("main.log") || n.contains("node_modules")),
        "logs and installs stay out: {listed:?}"
    );
    assert!(
        !dir.join("Exports").exists(),
        "nothing was written inside the project"
    );

    // ── packages need proof ──
    let e = export::export(&dir, Kind::Wheel, &out).unwrap_err();
    assert!(e.contains("not run yet"), "{e}");
    assert!(export::export(&dir, Kind::Crate, &out)
        .unwrap_err()
        .contains("not available"));

    // ── the wheel: pip installs it, and its command runs ──
    let dir = project("python", true);
    let wheel = export::export(&dir, Kind::Wheel, &export::default_out(&dir)).unwrap();
    assert!(
        wheel
            .path
            .ends_with("export_check_python-0.1.0-py3-none-any.whl"),
        "{}",
        wheel.path.display()
    );
    // RECORD's hashes are the files' hashes.
    python(&format!(
        "import zipfile,hashlib,base64\nz=zipfile.ZipFile({:?})\nfor line in z.read('export_check_python-0.1.0.dist-info/RECORD').decode().splitlines():\n    p,h,n=line.split(',')\n    if not h: continue\n    d=z.read(p)\n    assert h=='sha256='+base64.urlsafe_b64encode(hashlib.sha256(d).digest()).rstrip(b'=').decode(), p\n    assert int(n)==len(d), p\n",
        wheel.path.display().to_string()
    ));
    let pip_ok = Command::new("python3")
        .args(["-m", "pip", "--version"])
        .output()
        .is_ok_and(|o| o.status.success());
    if pip_ok {
        let target = dir.parent().unwrap().join("site");
        let out = Command::new("python3")
            .args([
                "-m",
                "pip",
                "install",
                "--quiet",
                "--no-deps",
                "--no-index",
                "--target",
            ])
            .arg(&target)
            .arg(&wheel.path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "pip refused the wheel:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let logs = dir.parent().unwrap().join("logs");
        let ran = Command::new("python3")
            .args(["-m", "export_check_python"])
            .env("PYTHONPATH", &target)
            .env("WRIGHT_LOG_DIR", &logs)
            .output()
            .unwrap();
        assert!(
            ran.status.success(),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        assert!(
            logs.join("main.log").exists(),
            "the installed package ran and logged"
        );
        assert!(
            target.join("bin/export-check-python").exists() || cfg!(windows),
            "the console script was installed"
        );
    }

    // ── the crate: cargo packages it ──
    if have("cargo") {
        let dir = project("rust", true);
        let c = export::export(&dir, Kind::Crate, &export::default_out(&dir)).unwrap();
        assert!(
            c.path.ends_with("export-check-rust-0.1.0.crate"),
            "{}",
            c.path.display()
        );
        assert!(c.bytes > 100);
        assert!(
            !dir.join("target/package").exists(),
            "the project's own target/ was not used"
        );
    }

    // ── the web build: only when asked (downloads packages) ──
    if std::env::var("WRIGHT_TEST_NPM").is_ok() && have("npm") {
        let dir = project("react", true);
        let w = export::export(&dir, Kind::Web, &export::default_out(&dir)).unwrap();
        let listed = names(&w.path);
        assert!(
            listed.contains(&"export-check-react/index.html".to_string()),
            "{listed:?}"
        );
        assert!(
            listed
                .iter()
                .any(|n| n.starts_with("export-check-react/assets/") && n.ends_with(".js")),
            "{listed:?}"
        );
    }
}

/// WR-15: the deployment packagings are generated from the blueprint,
/// byte-identical every time, and say what the blueprint says.
#[test]
fn deployment_packagings_come_from_the_blueprint() {
    let _env = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = project("python", true);
    let text = std::fs::read_to_string(dir.join("blueprint.kdl"))
        .unwrap()
        .replace(
            "deliverable kind=",
            "chips { scheduling cron=\"30 7 * * 1-5\" tz=\"Asia/Kolkata\" }\ndeliverable kind=",
        );
    std::fs::write(dir.join("blueprint.kdl"), &text).unwrap();
    let bp = parse::blueprint(&text).unwrap();
    let kinds = export::available(&bp);
    for k in [Kind::Docker, Kind::Systemd, Kind::GhActions] {
        assert!(kinds.contains(&k), "{k:?} not offered: {kinds:?}");
    }
    assert_eq!(
        export::default_kinds(&bp),
        vec![Kind::Zip, Kind::Wheel],
        "defaults unchanged"
    );
    let out = dir.parent().unwrap().join("deploy-out");

    let docker = export::export(&dir, Kind::Docker, &out).unwrap();
    let d = std::fs::read_to_string(&docker.path).unwrap();
    assert!(
        d.starts_with("# Generated by `wright export --docker`"),
        "{d}"
    );
    assert!(d.contains("FROM python:3.12-slim"), "{d}");
    assert!(
        d.contains(&convention::test_cmd(&bp)),
        "tests run at build time: {d}"
    );
    assert!(d.contains("USER 10001"), "not root: {d}");
    assert!(
        d.trim_end().ends_with(']') && d.contains("CMD [\"sh\", \"-c\","),
        "{d}"
    );
    let again = export::export(&dir, Kind::Docker, &out).unwrap();
    assert_eq!(docker.sha256, again.sha256, "deterministic");

    let units = export::export(&dir, Kind::Systemd, &out).unwrap();
    let names = names(&units.path);
    assert!(
        names.iter().any(|n| n.ends_with(".service"))
            && names.iter().any(|n| n.ends_with(".timer")),
        "{names:?}"
    );
    let timer = python(&format!(
        "import zipfile; z=zipfile.ZipFile({:?}); print(z.read([n for n in z.namelist() if n.endswith('.timer')][0]).decode())",
        units.path.display().to_string()
    ));
    assert!(
        timer.contains("OnCalendar=Mon..Fri *-*-* 07:30:00 Asia/Kolkata"),
        "{timer}"
    );

    let ci = export::export(&dir, Kind::GhActions, &out).unwrap();
    let y = std::fs::read_to_string(&ci.path).unwrap();
    assert!(
        y.contains("actions/setup-python@v5") && y.contains("unittest"),
        "{y}"
    );

    // Without a verified run the Dockerfile waits; the config files do not.
    let fresh = project("node", false);
    assert!(export::export(&fresh, Kind::Docker, &out).is_err());
    assert!(export::export(&fresh, Kind::GhActions, &out).is_ok());
}

/// WR-15: base images are pinned by digest from `export.kdl` — the
/// operator's fact to supply, because Wright cannot read a registry
/// offline. A required-but-missing digest refuses; a malformed one
/// refuses; a good one lands in the Dockerfile's `FROM` line and in the
/// export receipt.
#[test]
fn docker_digests_come_from_the_export_config_or_the_export_refuses() {
    let _env = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = project("python", true);
    let out = dir.parent().unwrap().join("digest-out");
    let good = format!("sha256:{}", "a".repeat(64));

    // ── no config: the honest moving tag, and a header that says so ──
    let e = export::export(&dir, Kind::Docker, &out).unwrap();
    assert_eq!(e.base_image.as_deref(), Some("python:3.12-slim"));
    assert_eq!(e.digest, None, "unpinned without a config");
    let d = std::fs::read_to_string(&e.path).unwrap();
    assert!(d.contains("FROM python:3.12-slim\n"), "{d}");
    assert!(d.contains("NOT pinned"), "the header admits the tag: {d}");

    // ── a pinned config: the FROM line carries the digest ──
    let cfg = format!(
        "export {{\n  docker {{\n    digest \"python:3.12-slim\" value=\"{good}\"\n  }}\n}}\n"
    );
    std::fs::write(dir.join("export.kdl"), &cfg).unwrap();
    let e = export::export(&dir, Kind::Docker, &out).unwrap();
    assert_eq!(e.digest.as_deref(), Some(good.as_str()));
    let d = std::fs::read_to_string(&e.path).unwrap();
    assert!(d.contains(&format!("FROM python:3.12-slim@{good}")), "{d}");
    let again = export::export(&dir, Kind::Docker, &out).unwrap();
    assert_eq!(e.sha256, again.sha256, "still deterministic");

    // ── required but absent: refusal, with the way to get a digest ──
    let cfg = "export {\n  docker {\n    require_digest true\n  }\n}\n";
    std::fs::write(dir.join("export.kdl"), cfg).unwrap();
    let err = export::export(&dir, Kind::Docker, &out).unwrap_err();
    assert!(
        err.contains("requires a digest") && err.contains("imagetools inspect"),
        "{err}"
    );
    let d = std::fs::read_to_string(out.join("Dockerfile")).unwrap();
    assert!(
        d.contains(&format!("FROM python:3.12-slim@{good}")),
        "the refusal left the last good Dockerfile alone: {d}"
    );

    // ── a malformed digest is an error, not a quiet tag build ──
    let short = format!("sha256:{}", "a".repeat(63));
    std::fs::write(
        dir.join("export.kdl"),
        format!("export {{ docker {{ digest \"python:3.12-slim\" value=\"{short}\" }} }}"),
    )
    .unwrap();
    let err = export::export(&dir, Kind::Docker, &out).unwrap_err();
    assert!(err.contains("64 hex"), "{err}");

    // ── an unknown setting refuses rather than being ignored ──
    std::fs::write(dir.join("export.kdl"), "export { submarines true }").unwrap();
    let err = export::export(&dir, Kind::Docker, &out).unwrap_err();
    assert!(err.contains("unknown export setting"), "{err}");

    // ── a digest for an image this project does not use pins nothing ──
    std::fs::write(
        dir.join("export.kdl"),
        format!(
            "export {{ docker {{ digest \"node:22-slim\" value=\"{good}\" require_digest true }} }}"
        ),
    )
    .unwrap();
    let err = export::export(&dir, Kind::Docker, &out).unwrap_err();
    assert!(err.contains("python:3.12-slim"), "{err}");
}

/// WR-15: every export is signed into a receipt — the artifact set under
/// the machine's Ed25519 key, with BLAKE3 copies in the store as durable
/// witnesses — and `wright verify`'s export checks catch every way it
/// can be made to lie.
#[test]
fn the_export_receipt_is_signed_evidence() {
    let _env = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = project("python", true);
    let digest = format!("sha256:{}", "b".repeat(64));
    std::fs::write(
        dir.join("export.kdl"),
        format!("export {{ docker {{ digest \"python:3.12-slim\" value=\"{digest}\" }} }}"),
    )
    .unwrap();
    let out = dir.parent().unwrap().join("receipt-out");
    let mut made = vec![
        export::export(&dir, Kind::Zip, &out).unwrap(),
        export::export(&dir, Kind::Docker, &out).unwrap(),
    ];
    let r = export::write_receipt(&dir, &out, &made).unwrap();
    assert!(r.signed, "a machine key resolved and signed");

    // What the receipt says.
    let text = std::fs::read_to_string(&r.path).unwrap();
    let v = json::parse(&text).unwrap();
    assert_eq!(v.str_of("format"), Some("wright.export-receipt/1"));
    assert!(v.get("run").and_then(|n| n.as_u64()) == Some(1), "{text}");
    assert!(
        v.get("run_ok").and_then(|b| b.as_bool()) == Some(true),
        "{text}"
    );
    let rows = v
        .get("artifacts")
        .and_then(|a| a.as_arr())
        .unwrap()
        .to_vec();
    assert_eq!(rows.len(), 2, "{text}");
    let docker = rows
        .iter()
        .find(|x| x.str_of("kind") == Some("docker"))
        .unwrap();
    assert_eq!(docker.str_of("digest"), Some(digest.as_str()));
    assert_eq!(docker.str_of("base_image"), Some("python:3.12-slim"));
    assert!(docker.str_of("blake3").is_some_and(|h| h.len() == 64));
    assert!(v.get("signature").is_some(), "{text}");

    // ── it verifies, green ──
    let ver = verify::export_receipt_file(&dir, &r.path);
    assert!(ver.ok, "{}", ver.report());
    let names: Vec<&str> = ver.checks.iter().map(|c| c.name).collect();
    assert_eq!(names, ["signature", "artifacts", "blueprint", "base image"]);
    assert!(
        ver.checks[3].note.contains("pinned by digest"),
        "{}",
        ver.report()
    );

    // ── an artifact edited after signing fails, with both hashes ──
    let original = std::fs::read(out.join("Dockerfile")).unwrap();
    std::fs::write(out.join("Dockerfile"), "# tampered\n").unwrap();
    let ver = verify::export_receipt_file(&dir, &r.path);
    assert!(!ver.ok, "an edited artifact must fail");
    assert!(
        ver.report()
            .contains("changed after the receipt was signed"),
        "{}",
        ver.report()
    );

    // ── the export folder can be moved or emptied: the store's copy is
    //    the durable witness, and verification still passes ──
    std::fs::write(out.join("Dockerfile"), &original).unwrap();
    let moved = dir.parent().unwrap().join("receipt-moved");
    let _ = std::fs::remove_dir_all(&moved);
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::copy(&r.path, moved.join("receipt.json")).unwrap();
    let ver = verify::export_receipt_file(&dir, &moved.join("receipt.json"));
    assert!(ver.ok, "{}", ver.report());
    assert!(
        ver.report().contains("only in the store"),
        "{}",
        ver.report()
    );

    // ── the receipt edited after signing fails the signature ──
    let tampered = text.replace("Export Check python", "Export Check TAMPERED");
    std::fs::write(&r.path, &tampered).unwrap();
    let ver = verify::export_receipt_file(&dir, &r.path);
    assert!(!ver.ok);
    assert!(ver.report().contains("does NOT match"), "{}", ver.report());

    // ── a stranger's valid signature is still not this machine's word ──
    std::fs::write(&r.path, &text).unwrap();
    let forged = resigned(&text, &[9u8; 32]);
    let stranger_path = out.join("receipt-stranger.json");
    std::fs::write(&stranger_path, json::to_string(&forged).as_bytes()).unwrap();
    let ver = verify::export_receipt_file(&dir, &stranger_path);
    assert!(!ver.ok, "a stranger's signature is not proof");
    assert!(ver.report().contains("but the key"), "{}", ver.report());

    // ── a receipt that claims a pin its artifact lacks fails ──
    // (Rebuild the set unpinned, then re-sign over a claimed digest that
    // was never in the Dockerfile: the FROM line is read back.)
    std::fs::remove_file(dir.join("export.kdl")).unwrap();
    made.clear();
    made.push(export::export(&dir, Kind::Docker, &out).unwrap());
    let r2 = export::write_receipt(&dir, &out, &made).unwrap();
    let text2 = std::fs::read_to_string(&r2.path).unwrap();
    let claimed = text2.replace("\"digest\":null", &format!("\"digest\":\"{digest}\""));
    assert_ne!(text2, claimed);
    // Re-sign the lie with this machine's own key: the signature is good,
    // the artifact is not.
    let own = wright_runtime::signing::SigningKey::resolve()
        .unwrap()
        .seed_for_signing();
    let lied = resigned(&claimed, &own);
    std::fs::write(&r2.path, json::to_string(&lied).as_bytes()).unwrap();
    let ver = verify::export_receipt_file(&dir, &r2.path);
    assert!(!ver.ok, "a claimed pin the bytes lack must fail");
    assert!(
        ver.report().contains("no such FROM line"),
        "{}",
        ver.report()
    );
}

/// A receipt's JSON with its signature stripped and re-signed by the key
/// grown from `seed` — the shape a forger (or an honest re-signer)
/// produces. The core primitives only, so the test signs exactly the way
/// the runtime does.
fn resigned(text: &str, seed: &[u8; 32]) -> Value {
    let v = json::parse(text).unwrap();
    let mut fields_out = Vec::new();
    if let Value::Obj(fields) = &v {
        for (k, val) in fields {
            if k != "signature" {
                fields_out.push((k.clone(), val.clone()));
            }
        }
    }
    let stripped = Value::Obj(fields_out);
    let bytes = json::to_string(&stripped);
    let pub_hex: String = wright_core::ed25519::public_key(seed)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let sig: String = wright_core::ed25519::sign(seed, bytes.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut out = stripped;
    out.set(
        "signature",
        Value::obj(vec![
            ("public_key", Value::str(pub_hex)),
            ("provenance", Value::str("ephemeral")),
            ("value", Value::str(sig)),
        ]),
    );
    out
}
