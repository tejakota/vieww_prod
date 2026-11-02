//! Export: a project leaves Wright as something other tools take.
//!
//! ```text
//!  every project  -> <name>-<date>.zip      the source, without logs, caches, builds
//!  python         -> <name>-<v>-py3-none-any.whl   pip installs it
//!  rust           -> <name>-<v>.crate              cargo package
//!  react          -> <name>-<v>-web.zip            the built site (dist/), host anywhere
//! ```
//!
//! **Packages need proof.** A wheel, a crate or a web build is made only
//! from a project whose last run passed its checks and was approved by the
//! Reviewer — the same proof the result screen shows. The source zip is
//! always allowed: it is a copy, not a claim.
//!
//! **Exports never land inside the project.** They go to
//! `<projects folder>/Exports/<project>/` (or `--out`), so exporting does
//! not change the project, its cache keys, or the next export.
//!
//! The zip and the wheel are written here (std only, reproducible bytes);
//! the crate and the web build run the project's own tool chain (`cargo
//! package`, `npm run build`) on this machine, outside the sandbox, because
//! a person asked for them.
//!
//! **Every export is signed into a receipt** (WR-15): `<out>/receipt.json`,
//! format `wright.export-receipt/1`, records the artifact set — each file
//! with its SHA-256 (what a person checks) and its BLAKE3 (the store's
//! content-addressed copy, the durable witness) — plus the blueprint the
//! packagings were generated from and the Docker base-image digest when one
//! is pinned. It is signed over its exact bytes with the machine's Ed25519
//! key, exactly like a run receipt, and `wright verify <receipt> <project>`
//! checks it (ADR-046).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use wright_core::blueprint::{canon, Blueprint};
use wright_core::convention;
use wright_core::sha256;
use wright_core::wire::json::{self, Value};
use wright_core::zip::ZipWriter;
use wright_runtime::store::{write_atomic, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The project's source, as a zip.
    Zip,
    /// A Python wheel.
    Wheel,
    /// A Rust crate (`.crate`, what crates.io takes).
    Crate,
    /// A built web app (React), zipped.
    Web,
    /// A Dockerfile that runs the project with its convention's run
    /// command (WR-15).
    Docker,
    /// A systemd service + timer from the `scheduling` chip (WR-15).
    Systemd,
    /// A GitHub Actions workflow running the project's tests (WR-15).
    GhActions,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Zip,
        Kind::Wheel,
        Kind::Crate,
        Kind::Web,
        Kind::Docker,
        Kind::Systemd,
        Kind::GhActions,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Zip => "Project as .zip",
            Kind::Wheel => "Python wheel",
            Kind::Crate => "Rust crate",
            Kind::Web => "Web build",
            Kind::Docker => "Dockerfile",
            Kind::Systemd => "systemd units",
            Kind::GhActions => "GitHub Actions",
        }
    }

    pub fn flag(self) -> &'static str {
        match self {
            Kind::Zip => "--zip",
            Kind::Wheel => "--wheel",
            Kind::Crate => "--crate",
            Kind::Web => "--web",
            Kind::Docker => "--docker",
            Kind::Systemd => "--systemd",
            Kind::GhActions => "--gh-actions",
        }
    }

    /// The stable name recorded in the export receipt (and nothing else
    /// reads it, so it is the wire format: never rename one).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Zip => "zip",
            Kind::Wheel => "wheel",
            Kind::Crate => "crate",
            Kind::Web => "web",
            Kind::Docker => "docker",
            Kind::Systemd => "systemd",
            Kind::GhActions => "gh-actions",
        }
    }

    /// Whether this export is a package (needs a verified run).
    /// The deployment packagings are configuration generated from the
    /// blueprint; a Dockerfile packages code to run, so it waits for the
    /// proof like a package does.
    pub fn needs_proof(self) -> bool {
        !matches!(self, Kind::Zip | Kind::Systemd | Kind::GhActions)
    }
}

/// The exports that make sense for this project: the zip, plus the one
/// package its language has.
pub fn default_kinds(bp: &Blueprint) -> Vec<Kind> {
    let mut out = vec![Kind::Zip];
    match bp.project.language.as_str() {
        "python" => out.push(Kind::Wheel),
        "rust" => out.push(Kind::Crate),
        "react" => out.push(Kind::Web),
        _ => {}
    }
    out
}

/// Every export this project can make: [`default_kinds`] plus the
/// deployment packagings its blueprint supports (WR-15): a Dockerfile when
/// it has a run command, systemd units when it is scheduled, a GitHub
/// Actions workflow when it has a test command.
pub fn available(bp: &Blueprint) -> Vec<Kind> {
    let mut out = default_kinds(bp);
    if !convention::run_cmd(bp).is_empty() && base_image(&bp.project.language).is_some() {
        out.push(Kind::Docker);
    }
    if bp.chips.scheduling.is_some() && !convention::run_cmd(bp).is_empty() {
        out.push(Kind::Systemd);
    }
    if !convention::test_cmd(bp).is_empty() {
        out.push(Kind::GhActions);
    }
    out
}

/// `<parent of the project>/Exports/<project folder name>`.
pub fn default_out(project: &Path) -> PathBuf {
    let name = project
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "project".into());
    project
        .parent()
        .unwrap_or(project)
        .join("Exports")
        .join(name)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Exported {
    pub kind: Kind,
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
    /// Docker only: the base image the Dockerfile builds from (with its
    /// digest appended when one is pinned).
    pub base_image: Option<String>,
    /// Docker only: the digest pinning the base image, when the export
    /// config has one (`sha256:…`).
    pub digest: Option<String>,
}

/// Why a package cannot be made yet, or `Ok` when the last run is the proof.
pub fn verified(project: &Path) -> Result<(), String> {
    let store = Store::open(project)?;
    match last_run_state(&store) {
        None => {
            Err("this project has not run yet — run it, and export once its checks pass".into())
        }
        Some((run, None)) => Err(format!(
            "run {run} has no receipt yet (still running, or it stopped)"
        )),
        Some((_, Some(true))) => Ok(()),
        Some((run, Some(false))) => Err(format!(
            "the last run ({run}) did not pass its checks and review — fix that first, then export"
        )),
    }
}

/// The last run's number and its verdict (`None` = no receipt yet, so
/// still running or stopped). The proof a package export asked for, and
/// the context the export receipt records.
fn last_run_state(store: &Store) -> Option<(u64, Option<bool>)> {
    let run = store.runs().last().copied()?;
    let ok = std::fs::read_to_string(store.run_dir(run).join("receipt.json"))
        .ok()
        .and_then(|t| json::parse(&t).ok())
        .and_then(|v| v.get("ok").and_then(|b| b.as_bool()));
    Some((run, ok))
}

/// Make one export of `project` into `out`.
pub fn export(project: &Path, kind: Kind, out: &Path) -> Result<Exported, String> {
    let bp = crate::load_blueprint(project)?;
    if !available(&bp).contains(&kind) {
        return Err(format!(
            "a {} is not available for a {} project",
            kind.label().to_lowercase(),
            bp.project.language
        ));
    }
    if kind.needs_proof() {
        verified(project)?;
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let cfg = ExportConfig::load(project)?;
    let (name, bytes, image, digest) = match kind {
        Kind::Zip => two(source_zip(project, &bp)?),
        Kind::Wheel => two(wheel(project, &bp)?),
        Kind::Crate => two(crate_file(project)?),
        Kind::Web => two(web(project, &bp)?),
        Kind::Docker => dockerfile(&bp, &cfg)?,
        Kind::Systemd => two(systemd(&bp)?),
        Kind::GhActions => two(gh_actions(&bp)?),
    };
    let path = out.join(name);
    write_atomic(&path, &bytes)?;
    Ok(Exported {
        kind,
        bytes: bytes.len() as u64,
        sha256: sha256::hex(&bytes),
        path,
        base_image: image,
        digest,
    })
}

/// What one generator produced: the artifact's name, its bytes, and —
/// Docker only — the base image and the digest pinning it.
type Made = (String, Vec<u8>, Option<String>, Option<String>);

/// Widen a generator's `(name, bytes)` to [`Made`] — everything but
/// the Dockerfile pins nothing.
fn two(m: (String, Vec<u8>)) -> Made {
    (m.0, m.1, None, None)
}

// ── the export receipt (WR-15) ────────────────────────────────────

/// Where the export receipt landed, and whether it is signed. A machine
/// with no signing key still gets a receipt — it says so by carrying no
/// signature block, and `wright verify` reports that honestly.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportReceipt {
    pub path: PathBuf,
    pub signed: bool,
}

/// Write `<out>/receipt.json` for one export invocation: the artifact
/// set, signed over its exact bytes with the machine key (ADR-046).
///
/// Each artifact is recorded twice over: its SHA-256 (what a person
/// checks against the file) and its BLAKE3 (the copy the project's store
/// just took, content-addressed — the durable witness that outlives the
/// export folder being moved or deleted). The blueprint the packagings
/// were generated from is committed to the store's history, so the
/// receipt names the plan it came from the way a run receipt does. The
/// receipt is overwritten by the next export of the same project; the
/// artifacts it names stay in the store either way.
pub fn write_receipt(
    project: &Path,
    out: &Path,
    made: &[Exported],
) -> Result<ExportReceipt, String> {
    if made.is_empty() {
        return Err("nothing was made; there is no export to record".into());
    }
    let bp = crate::load_blueprint(project)?;
    let store = Store::open(project)?;
    let commit = store.commit_blueprint(&canon::canonical(&bp), "exported")?;
    let (run, run_ok) = match last_run_state(&store) {
        Some((r, ok)) => (
            Value::int(r as i64),
            ok.map(Value::Bool).unwrap_or(Value::Null),
        ),
        None => (Value::Null, Value::Null),
    };
    let mut rows: Vec<Value> = Vec::new();
    for e in made {
        let bytes = std::fs::read(&e.path).map_err(|err| format!("{}: {err}", e.path.display()))?;
        let blake3 = store.put_object(&bytes)?;
        rows.push(Value::obj(vec![
            ("kind", Value::str(e.kind.name())),
            (
                "name",
                Value::str(
                    e.path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                ),
            ),
            ("path", Value::str(e.path.display().to_string())),
            ("bytes", Value::int(e.bytes as i64)),
            ("sha256", Value::str(e.sha256.clone())),
            ("blake3", Value::str(blake3)),
            (
                "base_image",
                e.base_image.clone().map(Value::str).unwrap_or(Value::Null),
            ),
            (
                "digest",
                e.digest.clone().map(Value::str).unwrap_or(Value::Null),
            ),
        ]));
    }
    let mut receipt = Value::obj(vec![
        ("format", Value::str("wright.export-receipt/1")),
        ("project", Value::str(bp.meta.name.clone())),
        ("project_dir", Value::str(project.display().to_string())),
        ("blueprint", Value::str(commit)),
        ("run", run),
        ("run_ok", run_ok),
        ("made_ms", Value::int(wright_core::events::now_ms() as i64)),
        ("artifacts", Value::Arr(rows)),
        ("wright", Value::str(crate::firing::APP_VERSION)),
    ]);
    // The same rule as a run receipt: sign the exact bytes as they stand
    // (no signature field yet), then append the signature block — the
    // writer is insertion-ordered, so a verifier reproduces the bytes.
    let mut signed = false;
    if let Ok(key) = wright_runtime::signing::SigningKey::resolve() {
        let bytes = json::to_string(&receipt);
        let sig = key.sign(bytes.as_bytes());
        let hex_sig: String = sig.iter().map(|b| format!("{b:02x}")).collect();
        receipt.set(
            "signature",
            Value::obj(vec![
                ("public_key", Value::str(key.public_hex())),
                ("provenance", Value::str(key.provenance_str())),
                ("value", Value::str(hex_sig)),
            ]),
        );
        signed = true;
    }
    let path = out.join("receipt.json");
    write_atomic(&path, json::to_string(&receipt).as_bytes())?;
    Ok(ExportReceipt { path, signed })
}

// ── deployment packagings (WR-15) ─────────────────────────────────
//
// Each is generated from the blueprint alone: the same blueprint gives the
// same bytes, so an export can be diffed and reviewed like code. None of
// them imports anything a model wrote.

// ── the export config (WR-15: digest pinning) ────────────────────

/// The operator's export decisions, read from `export.kdl` in the project
/// root. The blueprint is the *plan* (generated from the person's words);
/// this file is the operator's supply-chain calls — the facts Wright
/// cannot know and must not guess. Today one fact matters: which Docker
/// base images are pinned by digest.
///
/// ```kdl
/// export {
///   docker {
///     digest "python:3.12-slim" value="sha256:…"   // `docker buildx
///     //                                imagetools inspect python:3.12-slim`
///     require_digest true      // refuse --docker while a digest is missing
///   }
/// }
/// ```
///
/// **Why the digest is yours to type.** Wright makes no outbound traffic
/// but the update channel (AGENTS.md), so it cannot read a registry to
/// learn a digest itself — and a guessed digest would be worse than a
/// moving tag. The file is optional: without it, the Dockerfile builds
/// from the tag and says so; with `require_digest`, an unpinned export is
/// REFUSED rather than quietly reproducible-in-theory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportConfig {
    /// Base image (as the Dockerfile spells it) → `sha256:…` digest.
    digests: BTreeMap<String, String>,
    /// Refuse a Docker export whose base image has no digest.
    require_digest: bool,
}

impl ExportConfig {
    /// The project's export config: `export.kdl` beside the blueprint, or
    /// empty when the file does not exist. Anything it cannot parse is an
    /// error, not a guess — a mistyped digest must never become a tag.
    pub fn load(project: &Path) -> Result<ExportConfig, String> {
        let path = project.join("export.kdl");
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Ok(ExportConfig::default()),
        };
        Self::parse(&src, &path.display().to_string())
    }

    /// Parse `export.kdl`'s text. `source` names the file in errors.
    pub fn parse(src: &str, source: &str) -> Result<ExportConfig, String> {
        use wright_core::blueprint::parse::parse_doc;
        let doc = parse_doc(src).map_err(|e| format!("{source}: {e}"))?;
        let mut cfg = ExportConfig::default();
        let export = doc
            .iter()
            .find(|n| n.name == "export")
            .ok_or_else(|| format!("{source}: expected one `export {{ … }}` node"))?;
        for node in &export.children {
            match node.name.as_str() {
                "docker" => cfg.read_docker(node, source)?,
                other => {
                    return Err(format!(
                        "{source}: line {}: unknown export setting '{other}' (docker)",
                        node.line
                    ))
                }
            }
        }
        Ok(cfg)
    }

    fn read_docker(
        &mut self,
        docker: &wright_core::blueprint::parse::KdlNode,
        source: &str,
    ) -> Result<(), String> {
        for node in &docker.children {
            match node.name.as_str() {
                "digest" => {
                    let image = node.arg_str().ok_or_else(|| {
                        format!("{source}: line {}: digest names the image (digest \"python:3.12-slim\" value=…)", node.line)
                    })?;
                    let value = node.prop_str("value").ok_or_else(|| {
                        format!(
                            "{source}: line {}: digest '{image}' needs value=\"sha256:…\"",
                            node.line
                        )
                    })?;
                    check_digest(value, image)
                        .map_err(|e| format!("{source}: line {}: {e}", node.line))?;
                    self.digests.insert(image.to_string(), value.to_lowercase());
                }
                "require_digest" => {
                    let on = node
                        .arg_bool()
                        .or_else(|| node.prop_bool("value"))
                        .ok_or_else(|| {
                            format!(
                                "{source}: line {}: require_digest is true or false",
                                node.line
                            )
                        })?;
                    self.require_digest = on;
                }
                other => {
                    return Err(format!(
                    "{source}: line {}: unknown docker setting '{other}' (digest | require_digest)",
                    node.line
                ))
                }
            }
        }
        Ok(())
    }

    /// The digest pinned for `image`, if the config has one.
    pub fn digest_for(&self, image: &str) -> Option<&str> {
        self.digests.get(image).map(String::as_str)
    }

    /// Whether the operator asked for refusal over an unpinned base image.
    pub fn requires_digest(&self) -> bool {
        self.require_digest
    }
}

/// A Docker digest is `sha256:` + 64 hex digits, nothing else — the same
/// shape every registry prints, checked here so a typo cannot travel into
/// a Dockerfile and become a pull error on another machine.
fn check_digest(digest: &str, image: &str) -> Result<(), String> {
    let body = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("the digest for '{image}' must start with \"sha256:\""))?;
    let ok = body.len() == 64 && body.bytes().all(|b| b.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(format!(
            "the digest for '{image}' is not sha256 + 64 hex digits — `docker buildx imagetools inspect {image}` prints the real one"
        ))
    }
}

/// The base image a language's convention runs on. Tags, not digests: a
/// digest is the operator's to pin in `export.kdl` (Wright cannot read a
/// registry offline — see [`ExportConfig`]).
fn base_image(language: &str) -> Option<&'static str> {
    match language {
        "python" => Some("python:3.12-slim"),
        "node" | "react" => Some("node:22-slim"),
        "rust" => Some("rust:1-slim"),
        "shell" => Some("debian:bookworm-slim"),
        _ => None,
    }
}

/// JSON-quoted, for Dockerfile exec form and YAML.
fn quoted(s: &str) -> String {
    json::to_string(&json::Value::str(s))
}

/// How the Dockerfile's `FROM` line and its header comment explain the
/// pin: the digest when there is one, the honest tag when there is not.
fn from_line(image: &str, digest: Option<&str>) -> String {
    match digest {
        Some(d) => format!(
            "# Base image pinned by digest (export.kdl): {image}@{d}\nFROM {image}@{d}\n"
        ),
        None => format!(
            "# Base image NOT pinned: '{image}' is a moving tag. Pin it in export.kdl\n# (digest \"{image}\" value=\"sha256:…\") for reproducible builds.\nFROM {image}\n"
        ),
    }
}

fn dockerfile(bp: &Blueprint, cfg: &ExportConfig) -> Result<Made, String> {
    let language = bp.project.language.as_str();
    let image = base_image(language)
        .ok_or_else(|| format!("no container recipe for a {language} project"))?;
    let digest = match cfg.digest_for(image) {
        Some(d) => Some(d.to_string()),
        None if cfg.requires_digest() => {
            // Refusal, not a quiet fallback: the operator said unpinned
            // images are not acceptable, and an export that silently
            // ignored that would be the worse bug.
            return Err(format!(
                "export.kdl requires a digest for {image}, but none is set. Wright cannot read a registry offline, so the digest is yours to pin: `docker buildx imagetools inspect {image}` prints it. Add `digest \"{image}\" value=\"sha256:…\"` under `export {{ docker {{ … }} }}`, or drop `require_digest`."
            ));
        }
        None => None,
    };
    let run = convention::run_cmd(bp);
    let test = convention::test_cmd(bp);
    let mut d = format!(
        "# Generated by `wright export --docker` from the blueprint of\n# \"{}\". Regenerate rather than edit.\n{}WORKDIR /app\n",
        bp.meta.name.replace('\n', " "),
        from_line(image, digest.as_deref())
    );
    if language == "react" {
        d.push_str("COPY package*.json ./\nRUN npm install --no-audit --no-fund\n");
    }
    d.push_str("COPY . .\n");
    if language == "rust" {
        d.push_str("RUN cargo build --release --quiet\n");
    }
    if !test.is_empty() {
        d.push_str(&format!(
            "# The project's own tests run at build time: an image that fails them is not built.\nRUN [\"sh\", \"-c\", {}]\n",
            quoted(&test)
        ));
    }
    d.push_str(
        "RUN useradd --system --uid 10001 app 2>/dev/null || adduser -S -u 10001 app\nUSER 10001\n",
    );
    d.push_str(&format!("CMD [\"sh\", \"-c\", {}]\n", quoted(&run)));
    Ok((
        "Dockerfile".into(),
        d.into_bytes(),
        Some(image.to_string()),
        digest,
    ))
}

/// A 5-field cron line as systemd `OnCalendar=` (numbers, lists, ranges
/// and `*/n` steps; weekday numbers become names).
fn on_calendar(cron: &str, tz: &str) -> Result<String, String> {
    let f: Vec<&str> = cron.split_whitespace().collect();
    if f.len() != 5 {
        return Err(format!("'{cron}' is not a 5-field cron line"));
    }
    let field = |v: &str| -> String {
        if let Some(step) = v.strip_prefix("*/") {
            format!("0/{step}")
        } else {
            v.replace('-', "..")
        }
    };
    const DAYS: [&str; 8] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let dow = if f[4] == "*" {
        String::new()
    } else {
        let names: Result<Vec<String>, String> = f[4]
            .split(',')
            .map(|part| {
                let name = |n: &str| {
                    n.parse::<usize>()
                        .ok()
                        .and_then(|i| DAYS.get(i))
                        .map(|d| (*d).to_string())
                        .ok_or_else(|| format!("weekday '{n}' in '{cron}'"))
                };
                match part.split_once('-') {
                    Some((a, b)) => Ok(format!("{}..{}", name(a)?, name(b)?)),
                    None => name(part),
                }
            })
            .collect();
        format!("{} ", names?.join(","))
    };
    let pad = |v: String| if v.len() == 1 { format!("0{v}") } else { v };
    let tz = if tz.is_empty() {
        String::new()
    } else {
        format!(" {tz}")
    };
    Ok(format!(
        "{dow}*-{}-{} {}:{}:00{tz}",
        field(f[3]),
        field(f[2]),
        pad(field(f[1])),
        pad(field(f[0]))
    ))
}

fn systemd(bp: &Blueprint) -> Result<(String, Vec<u8>), String> {
    let sched = bp
        .chips
        .scheduling
        .as_ref()
        .ok_or("the blueprint has no scheduling chip")?;
    let slug = convention::slug(&bp.meta.name).replace('_', "-");
    let run = convention::run_cmd(bp);
    let service = format!(
        "# Generated by `wright export --systemd`. Install under ~/.config/systemd/user/,\n# set WorkingDirectory to the project, then: systemctl --user enable --now {slug}.timer\n[Unit]\nDescription={}\n\n[Service]\nType=oneshot\nWorkingDirectory=%h/{slug}\nExecStart=/bin/sh -c {}\nNoNewPrivileges=yes\nPrivateTmp=yes\n",
        bp.meta.name.replace('\n', " "),
        quoted(&run)
    );
    let timer = format!(
        "[Unit]\nDescription={} on its schedule ({})\n\n[Timer]\nOnCalendar={}\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n",
        bp.meta.name.replace('\n', " "),
        sched.cron,
        on_calendar(&sched.cron, &sched.tz)?
    );
    let mut z = ZipWriter::new();
    z.add(&format!("{slug}.service"), service.as_bytes())?;
    z.add(&format!("{slug}.timer"), timer.as_bytes())?;
    Ok((format!("{slug}-systemd.zip"), z.finish()?))
}

fn gh_actions(bp: &Blueprint) -> Result<(String, Vec<u8>), String> {
    let test = convention::test_cmd(bp);
    if test.is_empty() {
        return Err("the project has no test command".into());
    }
    let slug = convention::slug(&bp.meta.name).replace('_', "-");
    let setup = match bp.project.language.as_str() {
        "python" => "      - uses: actions/setup-python@v5\n        with:\n          python-version: \"3.12\"\n",
        "node" | "react" => "      - uses: actions/setup-node@v4\n        with:\n          node-version: \"22\"\n",
        _ => "",
    };
    let install = if bp.project.language == "react" {
        "      - run: npm install --no-audit --no-fund\n"
    } else {
        ""
    };
    let y = format!(
        "# Generated by `wright export --gh-actions`: the project's own tests.\n# Save as .github/workflows/{slug}.yml.\nname: {}\non:\n  push:\n  pull_request:\n  workflow_dispatch:\npermissions:\n  contents: read\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    steps:\n      - uses: actions/checkout@v4\n{setup}{install}      - run: {}\n",
        quoted(&bp.meta.name),
        quoted(&test)
    );
    Ok((format!("{slug}.yml"), y.into_bytes()))
}

// ── the files of a project ───────────────────────────────────────

/// Folders that are never part of what a project *is*.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".wright",
    "target",
    "node_modules",
    "dist",
    "__pycache__",
    ".venv",
    "venv",
    ".pytest_cache",
    ".mypy_cache",
];

/// Every regular file under `dir`, as (path relative to `dir` with `/`,
/// bytes), sorted so the same tree makes the same archive. Symbolic links
/// are skipped: an export copies the project, not what it points at.
fn files(dir: &Path, skip: &dyn Fn(&str, bool) -> bool) -> Result<Vec<(String, Vec<u8>)>, String> {
    fn walk(
        root: &Path,
        dir: &Path,
        skip: &dyn Fn(&str, bool) -> bool,
        out: &mut Vec<(String, Vec<u8>)>,
    ) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let meta = std::fs::symlink_metadata(e.path()).map_err(|err| err.to_string())?;
            let rel = e
                .path()
                .strip_prefix(root)
                .map_err(|err| err.to_string())?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            if meta.file_type().is_symlink() || skip(&rel, meta.is_dir()) {
                continue;
            }
            if meta.is_dir() {
                walk(root, &e.path(), skip, out)?;
            } else if meta.is_file() {
                out.push((
                    rel,
                    std::fs::read(e.path())
                        .map_err(|err| format!("{}: {err}", e.path().display()))?,
                ));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, skip, &mut out)?;
    Ok(out)
}

fn zip_of(prefix: &str, entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut z = ZipWriter::new();
    for (path, data) in entries {
        z.add(&format!("{prefix}/{path}"), data)?;
    }
    z.finish()
}

fn today() -> String {
    crate::firing::today_utc()
}

// ── the four exports ─────────────────────────────────────────────

fn source_zip(project: &Path, bp: &Blueprint) -> Result<(String, Vec<u8>), String> {
    let skip = |rel: &str, is_dir: bool| {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        (is_dir && SKIP_DIRS.contains(&name))
            || (rel.starts_with("logs/") && name != ".keep")
            || name.ends_with(".pyc")
    };
    let entries = files(project, &skip)?;
    let slug = convention::slug(&bp.meta.name).replace('_', "-");
    Ok((format!("{slug}-{}.zip", today()), zip_of(&slug, &entries)?))
}

/// A pure-Python wheel (PEP 427) of `src/<slug>/`, written directly: a
/// wheel is a zip with three metadata files, so no `pip install build`.
fn wheel(project: &Path, bp: &Blueprint) -> Result<(String, Vec<u8>), String> {
    let slug = convention::slug(&bp.meta.name);
    let pkg = project.join("src").join(&slug);
    if !pkg.is_dir() {
        return Err(format!(
            "no Python package at src/{slug}/ — the convention puts the code there"
        ));
    }
    let init = std::fs::read_to_string(pkg.join("__init__.py")).unwrap_or_default();
    let version = init
        .lines()
        .find_map(|l| {
            l.trim().strip_prefix("__version__").map(|r| {
                r.trim()
                    .trim_start_matches('=')
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string()
            })
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "0.1.0".into());
    let dist = format!("{slug}-{version}.dist-info");
    let skip = |rel: &str, is_dir: bool| {
        (is_dir && rel.rsplit('/').next() == Some("__pycache__")) || rel.ends_with(".pyc")
    };
    let mut entries: Vec<(String, Vec<u8>)> = files(&pkg, &skip)?
        .into_iter()
        .map(|(p, d)| (format!("{slug}/{p}"), d))
        .collect();

    let summary = bp
        .intent
        .summary
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let readme = std::fs::read_to_string(project.join("README.md")).unwrap_or_default();
    let metadata = format!(
        "Metadata-Version: 2.1\nName: {name}\nVersion: {version}\nSummary: {summary}\nRequires-Python: >=3.9\nDescription-Content-Type: text/markdown\n\n{readme}",
        name = slug.replace('_', "-"),
    );
    entries.push((format!("{dist}/METADATA"), metadata.into_bytes()));
    entries.push((
        format!("{dist}/WHEEL"),
        format!(
            "Wheel-Version: 1.0\nGenerator: wright {}\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
            env!("CARGO_PKG_VERSION")
        )
        .into_bytes(),
    ));
    let main = std::fs::read_to_string(pkg.join("__main__.py")).unwrap_or_default();
    if main.contains("def main(") {
        entries.push((
            format!("{dist}/entry_points.txt"),
            format!(
                "[console_scripts]\n{} = {slug}.__main__:main\n",
                slug.replace('_', "-")
            )
            .into_bytes(),
        ));
    }
    // RECORD: every file with its SHA-256 and size; RECORD itself unhashed.
    let mut record = String::new();
    for (path, data) in &entries {
        record.push_str(&format!(
            "{path},sha256={},{}\n",
            sha256::base64_url(&sha256::digest(data)),
            data.len()
        ));
    }
    record.push_str(&format!("{dist}/RECORD,,\n"));
    entries.push((format!("{dist}/RECORD"), record.into_bytes()));

    let mut z = ZipWriter::new();
    for (path, data) in &entries {
        z.add(path, data)?;
    }
    Ok((format!("{slug}-{version}-py3-none-any.whl"), z.finish()?))
}

/// A command from the project's tool chain. On Windows, npm and friends
/// are `.cmd` scripts that only `cmd` can start.
fn tool(name: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", name]);
        c
    } else {
        Command::new(name)
    }
}

fn run(mut cmd: Command, dir: &Path, what: &str) -> Result<(), String> {
    let out = cmd
        .current_dir(dir)
        .output()
        .map_err(|e| format!("could not start {what}: {e} — is it installed and on PATH?"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = err
        .lines()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Err(format!("{what} failed:\n{}", tail.join("\n")))
}

/// `cargo package`, into a scratch target folder (the project's own
/// `target/` is left alone).
fn crate_file(project: &Path) -> Result<(String, Vec<u8>), String> {
    let scratch = std::env::temp_dir().join(format!("wright-crate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let mut cmd = tool("cargo");
    cmd.args([
        "package",
        "--allow-dirty",
        "--no-verify",
        "--quiet",
        "--target-dir",
    ])
    .arg(&scratch);
    run(cmd, project, "cargo package")?;
    let found = std::fs::read_dir(scratch.join("package"))
        .map_err(|e| format!("cargo package wrote nothing: {e}"))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "crate"))
        .ok_or("cargo package made no .crate file")?;
    let name = found
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let bytes = std::fs::read(&found).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&scratch);
    Ok((name, bytes))
}

/// `npm install` and `npm run build`, then `dist/` as a zip.
///
/// Install runs every time: it takes about a second when nothing changed,
/// and a `node_modules/` that merely exists can be partial or stale (an
/// interrupted install, a dependency an agent just added).
fn web(project: &Path, bp: &Blueprint) -> Result<(String, Vec<u8>), String> {
    let mut cmd = tool("npm");
    cmd.args(["install", "--no-audit", "--no-fund"]);
    run(cmd, project, "npm install")?;
    let mut cmd = tool("npm");
    cmd.args(["run", "build"]);
    run(cmd, project, "npm run build")?;
    let dist = project.join("dist");
    if !dist.join("index.html").is_file() {
        return Err("the build finished but dist/index.html is missing".into());
    }
    let version = std::fs::read_to_string(project.join("package.json"))
        .ok()
        .and_then(|t| json::parse(&t).ok())
        .and_then(|v| v.str_of("version").map(String::from))
        .unwrap_or_else(|| "0.1.0".into());
    let slug = convention::slug(&bp.meta.name).replace('_', "-");
    let entries = files(&dist, &|_, _| false)?;
    Ok((
        format!("{slug}-{version}-web.zip"),
        zip_of(&slug, &entries)?,
    ))
}
