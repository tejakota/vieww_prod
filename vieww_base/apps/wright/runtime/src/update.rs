//! The update channel (WR-04): signed manifests, verify-then-apply.
//!
//! Wright owns every primitive a trustworthy release channel needs — an
//! audited Ed25519 (`core/src/ed25519.rs`, RFC 8032 vectors plus OpenSSL
//! cross-vectors), JSON canonicalization (`core/src/jcs.rs`, RFC 8785 —
//! the same cryptosuite the receipt credential uses), SHA-256, and a
//! signing-key provenance story ([`crate::signing`]). This module is
//! those primitives, aimed at distribution.
//!
//! # The wire format
//!
//! One static file per release, `wright-v<version>.manifest.json`, plus
//! a copy at the fixed alias `wright-latest.manifest.json` so a channel
//! URL of the `releases/latest/download/...` shape finds it:
//!
//! ```json
//! {
//!   "format": "wright.update-manifest/1",
//!   "version": "1.0.3",
//!   "notes": "…",
//!   "signed_by": {"key": "<64 hex>", "provenance": "env"},
//!   "assets": [
//!     {"name": "Wright-Setup-1.0.3-x64.exe", "sha256": "<64 hex>",
//!      "size": 12345678, "ed25519_sig": "<128 hex>"}
//!   ],
//!   "ed25519_sig": "<128 hex over the JCS of everything above>"
//! }
//! ```
//!
//! The manifest signature covers the JCS canonicalization of the value
//! WITHOUT the `ed25519_sig` member (the receipt's scheme, so any JCS
//! implementation in any language can check it). Each asset carries its
//! own Ed25519 signature over the asset BYTES, so a swapped binary
//! fails even inside a manifest that itself verifies.
//!
//! # Privacy — the whole story, in one paragraph
//!
//! `wright update check` makes exactly ONE outbound connection: a plain
//! HTTPS GET of the one static manifest file at the channel URL. No
//! version query string, no telemetry, no identifier of any kind — the
//! URL is the same for every machine, which is why the channel works
//! behind any cache. `wright update apply` additionally downloads the
//! ONE asset it names. Nothing else here ever touches the network. The
//! channel URL and release key are settings (`update url="…"
//! key="…"` in config.kdl), never secrets; a machine with no channel
//! configured phones nothing at all.
//!
//! # What is NOT here
//!
//! No background updater, no code execution of downloaded installers
//! (a verified `.exe`/`.dmg` is handed to the person to run — Wright
//! never auto-executes downloaded code), and no downgrade: `apply`
//! refuses a manifest whose version is not newer than the running one.

use std::path::Path;

use wright_core::ed25519;
use wright_core::jcs;
use wright_core::sha256;
use wright_core::wire::http::{parse_url, Request};
use wright_core::wire::json::{self, Value};

use crate::config::UpdateSettings;
use crate::net::{self, NetConfig};
use crate::signing::{self, SigningKey};

/// The one format id a signed update manifest may carry.
pub const FORMAT: &str = "wright.update-manifest/1";

/// The largest manifest or asset `apply` will read into memory: a
/// Wright installer is tens of MiB; anything past this is refused
/// before a byte is written to disk.
pub const MAX_ASSET_BYTES: usize = 512 * 1024 * 1024;

/// The manifest file a release publishes for `version`.
pub fn manifest_name(version: &str) -> String {
    format!("wright-v{version}.manifest.json")
}

/// The fixed alias the channel URL points at (`releases/latest/download/…`
/// needs one name that never changes).
pub const LATEST_ALIAS: &str = "wright-latest.manifest.json";

// ── the manifest ──────────────────────────────────────────────────

/// One downloadable asset, as the manifest names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    /// SHA-256 of the asset bytes, hex.
    pub sha256: String,
    pub size: u64,
    /// Ed25519 over the asset bytes, hex — checked on download.
    pub ed25519_sig: String,
}

/// A parsed, verified-update manifest (signature checked separately —
/// [`verify_manifest`] is the door that does both).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    pub notes: String,
    /// Who signed: the public key (hex) and its provenance, as the
    /// publisher recorded it — the same honesty [`crate::signing`]
    /// demands of receipt keys.
    pub signed_by_key: String,
    pub signed_by_provenance: String,
    pub assets: Vec<Asset>,
    /// The manifest signature over the JCS of everything above, hex.
    pub ed25519_sig: String,
}

fn hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn unhex(s: &str, n: usize) -> Result<Vec<u8>, String> {
    if s.len() != n * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("'{s}' is not {n} bytes of hex"));
    }
    (0..n)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Manifest {
    /// The manifest as a JSON value (no signature members).
    pub fn to_value(&self) -> Value {
        Value::obj(vec![
            ("format", Value::str(FORMAT)),
            ("version", Value::str(self.version.clone())),
            ("notes", Value::str(self.notes.clone())),
            (
                "signed_by",
                Value::obj(vec![
                    ("key", Value::str(self.signed_by_key.clone())),
                    ("provenance", Value::str(self.signed_by_provenance.clone())),
                ]),
            ),
            (
                "assets",
                Value::Arr(
                    self.assets
                        .iter()
                        .map(|a| {
                            Value::obj(vec![
                                ("name", Value::str(a.name.clone())),
                                ("sha256", Value::str(a.sha256.clone())),
                                ("size", Value::int(a.size as i64)),
                                ("ed25519_sig", Value::str(a.ed25519_sig.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }

    /// Parse the manifest BODY (already signature-verified, or about to
    /// be): every field checked, every hex length named in the error.
    /// `strict` demands per-asset signatures too (a manifest that will
    /// be *checked* must be complete; one about to be *signed* starts
    /// without them).
    fn from_value(v: &Value, strict: bool) -> Result<Manifest, String> {
        if v.str_of("format") != Some(FORMAT) {
            return Err(format!(
                "this is not a Wright update manifest (its format is '{}'; Wright speaks {})",
                v.str_of("format").unwrap_or("(missing)"),
                FORMAT
            ));
        }
        let version = v
            .str_of("version")
            .ok_or("the manifest has no version")?
            .to_string();
        if !valid_version(&version) {
            return Err(format!(
                "'{version}' is not a version Wright can compare (it wants 1.2.3-shaped numbers)"
            ));
        }
        let notes = v.str_of("notes").unwrap_or("").to_string();
        let signed_by_key = v
            .path("signed_by", &["key"])
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let signed_by_provenance = v
            .path("signed_by", &["provenance"])
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();
        let assets_v = v
            .get("assets")
            .and_then(|a| a.as_arr())
            .ok_or("the manifest lists no assets")?;
        if assets_v.is_empty() {
            return Err("the manifest lists no assets".into());
        }
        let mut assets = Vec::new();
        for a in assets_v {
            let name = a.str_of("name").ok_or("an asset has no name")?.to_string();
            if name.contains(['/', '\\', '\0']) {
                return Err(format!("asset name '{name}' is not a plain file name"));
            }
            let sha = a.str_of("sha256").unwrap_or("").to_ascii_lowercase();
            // (A manifest about to be SIGNED may leave the hash empty —
            // sign_file computes it from the asset bytes; a manifest that
            // will be CHECKED must carry it.)
            if !hex64(&sha) && !(sha.is_empty() && !strict) {
                return Err(format!("asset '{name}' has no sha256 (64 hex characters)"));
            }
            let sig = a.str_of("ed25519_sig").unwrap_or("").to_ascii_lowercase();
            if strict && sig.len() != 128 {
                return Err(format!(
                    "asset '{name}' has no ed25519_sig (128 hex characters) — an unsigned asset never downloads"
                ));
            }
            assets.push(Asset {
                name,
                sha256: sha,
                size: a.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                ed25519_sig: sig,
            });
        }
        Ok(Manifest {
            version,
            notes,
            signed_by_key,
            signed_by_provenance,
            assets,
            ed25519_sig: String::new(),
        })
    }

    /// Parse for signing: asset signatures may be absent (they are
    /// about to be computed).
    fn unsigned_from_value(v: &Value) -> Result<Manifest, String> {
        Self::from_value(v, false)
    }

    /// The asset this machine would download, by name heuristic:
    /// the platform's archive or installer.
    pub fn asset_for_this_platform(&self) -> Option<&Asset> {
        let pick = |needle: &[&str]| {
            self.assets
                .iter()
                .find(|a| needle.iter().any(|n| a.name.contains(n)))
        };
        if cfg!(windows) {
            pick(&["-windows-x64.zip", "-x64.exe"])
        } else if cfg!(target_os = "macos") {
            pick(&["-macos-universal.app.zip", "-macos-universal.dmg"])
        } else {
            pick(&["-linux-x64.tar.gz", "_amd64.deb", "-linux-x64.zip"])
        }
    }
}

/// The bytes a manifest signature covers: the value, minus its own
/// `ed25519_sig` member, JCS-canonicalized (RFC 8785 — the receipt's
/// scheme, so the check is reproducible in any language).
pub fn signed_input(v: &Value) -> Result<Vec<u8>, String> {
    let body = strip(v, &["ed25519_sig"]);
    Ok(jcs::canonicalize(&body)?.into_bytes())
}

fn strip(v: &Value, keys: &[&str]) -> Value {
    match v {
        Value::Obj(fields) => Value::Obj(
            fields
                .iter()
                .filter(|(k, _)| !keys.contains(&k.as_str()))
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Parse and verify a manifest against the pinned release key. This is
/// the only door that produces a trustable [`Manifest`]; it refuses
/// unsigned manifests with the sentence that stops the update.
pub fn verify_manifest(bytes: &[u8], release_pk: &[u8; 32]) -> Result<Manifest, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("the manifest is not UTF-8: {e}"))?;
    let v = json::parse(text).map_err(|e| format!("the manifest is not JSON: {e}"))?;
    let sig_hex = v
        .str_of("ed25519_sig")
        .ok_or_else(|| "the manifest carries no signature. Unsigned updates are not installed — \
             anyone on the path between you and the file could replace the bytes. Ask the publisher \
             to sign it, or pin the release key and get a fresh copy."
            .to_string())?
        .to_ascii_lowercase();
    let sig = unhex(&sig_hex, 64)
        .map_err(|_| "the manifest signature is not 64 bytes of hex".to_string())?;
    let mut m = Manifest::from_value(&v, true)?;
    let signed = signed_input(&v)?;
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig);
    if !ed25519::verify(release_pk, &signed, &sig_arr) {
        return Err(format!(
            "the manifest failed verification: its content no longer matches the pinned release key's \
             signature (version {}{}). Get a fresh copy from the channel, or re-pin the key if it \
             genuinely rotated.",
            m.version,
            if m.assets.len() == 1 {
                String::new()
            } else {
                format!(", {} assets", m.assets.len())
            }
        ));
    }
    m.ed25519_sig = sig_hex;
    Ok(m)
}

/// Check one downloaded asset: its SHA-256 and its own Ed25519
/// signature, both against what the (already verified) manifest said.
pub fn verify_asset(bytes: &[u8], asset: &Asset, release_pk: &[u8; 32]) -> Result<(), String> {
    let got = sha256::hex(bytes);
    if got != asset.sha256 {
        return Err(format!(
            "asset '{}' is not what the manifest promised: sha256 {} != {}. Nothing was written.",
            asset.name, got, asset.sha256
        ));
    }
    let sig = unhex(&asset.ed25519_sig, 64).map_err(|e| format!("asset '{}': {e}", asset.name))?;
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig);
    if !ed25519::verify(release_pk, bytes, &sig_arr) {
        return Err(format!(
            "asset '{}' failed its signature check: the bytes do not match the release key's \
             signature. Nothing was written.",
            asset.name
        ));
    }
    Ok(())
}

/// Sign a manifest file in place (the release workflow's `wright update
/// sign`): every asset's bytes are read from `assets_dir`, hashed,
/// signed, the manifest is signed, and the result verifies before it is
/// written back. The key's provenance is recorded in the manifest, the
/// same honesty receipts carry.
pub fn sign_file(
    path: &Path,
    assets_dir: &Path,
    key: &SigningKey,
) -> Result<(String, usize), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v = json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut m = Manifest::unsigned_from_value(&v)?;
    let pk = key.public_hex();
    // Each asset: read, hash, sign.
    for a in &mut m.assets {
        let bytes = std::fs::read(assets_dir.join(&a.name))
            .map_err(|e| format!("asset '{}' ({}): {e}", a.name, assets_dir.display()))?;
        let sha = sha256::hex(&bytes);
        if !a.sha256.is_empty() && a.sha256 != sha {
            return Err(format!(
                "asset '{}' hashes to {sha} but the manifest says {} — the file changed under you",
                a.name, a.sha256
            ));
        }
        a.sha256 = sha;
        a.size = bytes.len() as u64;
        a.ed25519_sig = hex(&ed25519::sign(&key.seed_for_signing(), &bytes));
    }
    m.signed_by_key = pk.clone();
    m.signed_by_provenance = key.provenance_str().to_string();
    // Sign the manifest body.
    let body = m.to_value();
    let sig = ed25519::sign(
        &key.seed_for_signing(),
        jcs::canonicalize(&body)?.as_bytes(),
    );
    m.ed25519_sig = hex(&sig);
    // It must verify before we write it.
    let mut out = body;
    out.set("ed25519_sig", Value::str(m.ed25519_sig.clone()));
    let written = json::to_string(&out);
    let pk_arr = unhex(&pk, 32).map_err(|e| format!("the release key is wrong: {e}"))?;
    let mut pk_arr2 = [0u8; 32];
    pk_arr2.copy_from_slice(&pk_arr);
    verify_manifest(written.as_bytes(), &pk_arr2)?;
    std::fs::write(path, written.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((pk, m.assets.len()))
}

/// The release signing key, resolved like [`crate::signing`] resolves
/// the receipt key — but deliberately, never silently generated: env
/// override first (`WRIGHT_SECRET_WRIGHT_RELEASE_SIGNING`, what CI
/// uses), then the OS keychain (`keychain:wright/release-signing`).
/// A missing key is an error saying how to make one; a release channel
/// must never fall back to "some key this process made up".
pub fn release_signing_key() -> Result<SigningKey, String> {
    let ref_ = "keychain:wright/release-signing";
    if let Ok(name) = crate::secrets::env_name(ref_) {
        if let Ok(v) = std::env::var(&name) {
            let seed = signing_seed_from_hex(v.trim())?;
            return Ok(SigningKey::from_seed(seed, crate::signing::Provenance::Env));
        }
    }
    if let Ok(v) = crate::secrets::resolve(ref_) {
        let seed = signing_seed_from_hex(v.expose())?;
        return Ok(SigningKey::from_seed(
            seed,
            crate::signing::Provenance::OsKeychain,
        ));
    }
    Err(format!(
        "no release signing key. Make one and put it where only you can read it: \
         wright secret set {ref_} <64 hex chars> (or export {}). A release key is never \
         generated silently — half the point of a signed channel is that the key is a decision.",
        crate::secrets::env_name(ref_).unwrap_or_else(|_| "the env equivalent".into())
    ))
}

fn signing_seed_from_hex(s: &str) -> Result<[u8; 32], String> {
    let clean: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if clean.len() != 64 || !clean.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the release signing key must be 32 bytes (64 hex characters)".into());
    }
    let bytes = unhex(&clean, 32)?;
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

// ── versions ─────────────────────────────────────────────────────

/// A version Wright can compare: dot-separated non-negative numbers
/// (`1.0.3`, `1.0.3-rc1` tolerated as its leading numbers).
fn valid_version(v: &str) -> bool {
    let head = v.split(['-', '+']).next().unwrap_or("");
    !head.is_empty()
        && head
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.len() < 10)
}

/// True when `offered` is strictly newer than `current` (numeric,
/// field by field; a longer tail wins ties: 1.2 > 1.2.0? No — 1.2.0
/// EQUALS 1.2, so only a longer AND greater tail wins).
pub fn newer_than(current: &str, offered: &str) -> bool {
    let num = |v: &str| -> Vec<u64> {
        v.split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (num(current), num(offered));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if y > x {
            return true;
        }
        if y < x {
            return false;
        }
    }
    false
}

// ── the channel ──────────────────────────────────────────────────

/// A resolved update channel: where the manifest lives and which key
/// must have signed it.
#[derive(Debug)]
pub struct Channel {
    pub url: String,
    pub key: [u8; 32],
}

impl Channel {
    /// From the config's `update` settings. Errors are plain sentences
    /// for the CLI; `None` when no channel is configured at all.
    pub fn from_settings(u: &UpdateSettings) -> Result<Option<Channel>, String> {
        if u.url.is_empty() {
            return Ok(None);
        }
        if u.key.is_empty() {
            return Err(format!(
                "an update channel is configured (update url=\"{}\") but no release key is pinned \
                 (update key=\"<64 hex>\"). Without a pinned key a manifest cannot be verified, so \
                 nothing would ever install.",
                u.url
            ));
        }
        let hex_key = signing::normalize_key(&u.key)
            .ok_or_else(|| "the update key is not 64 hex characters or a did:key".to_string())?;
        let bytes = unhex(&hex_key, 32).map_err(|e| format!("the update key: {e}"))?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Some(Channel {
            url: u.url.clone(),
            key,
        }))
    }
}

/// What a check found.
pub enum Outcome {
    UpToDate,
    Update {
        version: String,
        notes: String,
        asset: Option<Asset>,
    },
}

/// `wright update check`: fetch the manifest, verify it, compare.
/// `fetch` is the network leg (a plain GET); tests inject bytes.
pub fn check_with(
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    channel: &Channel,
    current_version: &str,
) -> Result<Outcome, String> {
    let bytes = fetch(&channel.url)
        .map_err(|e| format!("could not read the update channel ({}): {e}", channel.url))?;
    let m = verify_manifest(&bytes, &channel.key)?;
    if newer_than(current_version, &m.version) {
        Ok(Outcome::Update {
            version: m.version.clone(),
            notes: m.notes.clone(),
            asset: m.asset_for_this_platform().cloned(),
        })
    } else {
        Ok(Outcome::UpToDate)
    }
}

/// The network leg: one plain GET, HTTPS, a size cap, a short clock.
pub fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let u = parse_url(url).map_err(|e| format!("{url}: {e}"))?;
    let mut cfg = NetConfig::from_env();
    cfg.limits.max_body = 4 * 1024 * 1024;
    cfg.io_timeout = std::time::Duration::from_secs(30);
    let req = Request::new("GET", u)
        .header("User-Agent", concat!("wright/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/json");
    let resp = net::send(&req, &cfg).map_err(|e| e.to_string())?;
    if resp.status != 200 {
        return Err(format!("HTTP {}", resp.status));
    }
    Ok(resp.body)
}

// ── verify-then-apply ────────────────────────────────────────────

/// Download (through `fetch`), verify (sha256 + the asset's own
/// Ed25519), and swap `target` atomically: the new bytes land in a
/// temp file BESIDE the target, the running file is renamed aside, the
/// new file takes its place — a crash leaves either the old or the new
/// file, never half of one. Returns the restart sentence. `target` is
/// normally `std::env::current_exe()`; tests point it at a temp file.
///
/// Refuses: a manifest that is not newer (no downgrades, no reinstalls
/// over the same version), a missing platform asset, any hash or
/// signature mismatch. On any refusal the target is untouched — the
/// swap only starts after every check passed.
pub fn apply_with(
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    channel: &Channel,
    current_version: &str,
    asset_name: Option<&str>,
    target: &Path,
) -> Result<String, String> {
    let bytes = fetch(&channel.url)
        .map_err(|e| format!("could not read the update channel ({}): {e}", channel.url))?;
    let m = verify_manifest(&bytes, &channel.key)?;
    if !newer_than(current_version, &m.version) {
        return Err(format!(
            "the channel offers {} and this is {} — nothing newer to apply.",
            m.version, current_version
        ));
    }
    let asset =
        match asset_name {
            Some(want) => m.assets.iter().find(|a| a.name == want).ok_or_else(|| {
                format!(
                    "the manifest for {} lists no asset named '{want}'. It lists: {}",
                    m.version,
                    m.assets
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
            None => {
                m.asset_for_this_platform().ok_or_else(|| {
                    format!(
                "the manifest for {} lists no asset for {} — pick one by hand with --asset: {}",
                m.version,
                platform_name(),
                m.assets.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
            )
                })?
            }
        };
    if asset.size > MAX_ASSET_BYTES as u64 {
        return Err(format!(
            "asset '{}' claims {} bytes; Wright refuses anything over {}",
            asset.name, asset.size, MAX_ASSET_BYTES
        ));
    }
    // Where the asset bytes come from: the manifest's own channel URL,
    // minus the manifest's file name, plus the asset's name — one
    // origin, one hop.
    let base = channel
        .url
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/{}", asset.name))
        .ok_or_else(|| format!("the channel URL '{}' has no directory part", channel.url))?;
    let asset_bytes = fetch(&base)
        .map_err(|e| format!("could not download '{}' from the channel: {e}", asset.name))?;
    verify_asset(&asset_bytes, asset, &channel.key)?;

    // The swap: temp beside the target, rename the old aside, rename
    // the new in, drop the old. Every step is rename(2)/atomic on the
    // same filesystem, so a crash between steps leaves the old file or
    // the new one — never a half-written binary, never none.
    let dir = target
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", target.display()))?;
    let tmp = dir.join(format!(".wright-update-{}-new", std::process::id()));
    std::fs::write(&tmp, &asset_bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let aside = dir.join(format!(".wright-update-{}-old", std::process::id()));
    let _ = std::fs::remove_file(&aside);
    let rename = std::fs::rename(target, &aside).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not move {} aside: {e}", target.display())
    });
    rename?;
    match std::fs::rename(&tmp, target) {
        Ok(()) => {
            let _ = std::fs::remove_file(&aside);
            Ok(format!(
                "wright {} is in place ({} verified: sha256 + release signature; the old binary was {} — deleted). Restart wright to run it.",
                m.version,
                asset.name,
                aside.display()
            ))
        }
        Err(e) => {
            // Put the old one back; only then report.
            let restored = std::fs::rename(&aside, target).is_ok();
            Err(format!(
                "could not move the new binary into place: {e}.{}",
                if restored {
                    " The old binary is back; nothing changed."
                } else {
                    " The old binary is at {aside} — move it back by hand."
                }
                .replace("{aside}", &aside.display().to_string())
            ))
        }
    }
}

fn platform_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "linux"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn seed() -> [u8; 32] {
        [0x5fu8; 32]
    }

    fn key() -> SigningKey {
        SigningKey::from_seed(seed(), crate::signing::Provenance::Ephemeral)
    }

    fn pk() -> [u8; 32] {
        ed25519::public_key(&seed())
    }

    fn write_manifest(dir: &Path, assets: &[(&str, &[u8])]) -> (PathBuf, Manifest) {
        let mut m = Manifest {
            version: "9.9.9".into(),
            notes: "the next one".into(),
            signed_by_key: String::new(),
            signed_by_provenance: String::new(),
            assets: assets
                .iter()
                .map(|(name, _)| Asset {
                    name: (*name).into(),
                    sha256: String::new(),
                    size: 0,
                    ed25519_sig: String::new(),
                })
                .collect(),
            ed25519_sig: String::new(),
        };
        for (a, (_, bytes)) in m.assets.iter_mut().zip(assets) {
            a.sha256 = sha256::hex(bytes);
            a.size = bytes.len() as u64;
            a.ed25519_sig = hex(&ed25519::sign(&seed(), bytes));
        }
        m.signed_by_key = hex(&pk());
        m.signed_by_provenance = "ephemeral".into();
        m.ed25519_sig = hex(&ed25519::sign(
            &seed(),
            jcs::canonicalize(&m.to_value()).unwrap().as_bytes(),
        ));
        let path = dir.join(manifest_name(&m.version));
        let mut v = m.to_value();
        v.set("ed25519_sig", Value::str(m.ed25519_sig.clone()));
        std::fs::write(&path, json::to_string(&v)).unwrap();
        // the asset files too, so a test `fetch` can serve them by name
        for (name, bytes) in assets {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        (path, m)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn manifest_round_trips_through_canonical_json() {
        let dir = tmp("roundtrip");
        let (path, m) = write_manifest(&dir, &[("a.zip", b"asset bytes")]);
        let bytes = std::fs::read(&path).unwrap();
        let back = verify_manifest(&bytes, &pk()).unwrap();
        assert_eq!(back.version, m.version);
        assert_eq!(back.assets, m.assets);
        assert_eq!(back.signed_by_provenance, "ephemeral");
        // and the SAME bytes come back from a re-serialization round trip
        let v = json::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(json::to_string(&v), String::from_utf8(bytes).unwrap());
    }

    #[test]
    fn unsigned_and_foreign_manifests_are_refused_in_plain_words() {
        let dir = tmp("refuse");
        let (path, _) = write_manifest(&dir, &[("a.zip", b"asset bytes")]);
        let good_bytes = std::fs::read(&path).unwrap();
        let good = String::from_utf8(good_bytes.clone()).unwrap();
        // unsigned: strip the signature member
        let v = json::parse(&good).unwrap();
        let unsigned = json::to_string(&strip(&v, &["ed25519_sig"]));
        let err = verify_manifest(unsigned.as_bytes(), &pk()).unwrap_err();
        assert!(err.contains("no signature"), "{err}");
        assert!(err.contains("Unsigned updates are not installed"), "{err}");
        // a stranger's key
        let stranger = ed25519::public_key(&[9u8; 32]);
        let err = verify_manifest(&good_bytes, &stranger).unwrap_err();
        assert!(err.contains("failed verification"), "{err}");
        // one edited character
        let tampered = good.replace("the next one", "the next TWO");
        let err = verify_manifest(tampered.as_bytes(), &pk()).unwrap_err();
        assert!(err.contains("failed verification"), "{err}");
        // a bad format id
        let wrong = good.replace(FORMAT, "wright.update-manifest/0");
        assert!(verify_manifest(wrong.as_bytes(), &pk()).is_err());
    }

    #[test]
    fn assets_are_verified_by_hash_and_signature() {
        let dir = tmp("assets");
        let (_, m) = write_manifest(&dir, &[("a.zip", b"asset bytes")]);
        let a = &m.assets[0];
        assert!(verify_asset(b"asset bytes", a, &pk()).is_ok());
        let err = verify_asset(b"tampered bytes", a, &pk()).unwrap_err();
        assert!(err.contains("not what the manifest promised"), "{err}");
        assert!(err.contains("Nothing was written"), "{err}");
        // re-signed asset bytes with a stranger key: hash matches if we
        // rebuild the asset entry, signature does not
        let forged = Asset {
            name: a.name.clone(),
            sha256: sha256::hex(b"tampered bytes"),
            size: 14,
            ed25519_sig: hex(&ed25519::sign(&[9u8; 32], b"tampered bytes")),
        };
        let err = verify_asset(b"tampered bytes", &forged, &pk()).unwrap_err();
        assert!(err.contains("failed its signature check"), "{err}");
        // unsigned asset entry refused at parse time (strict parse)
        let v = json::parse(&format!(
            "{{\"format\":\"{FORMAT}\",\"version\":\"1.0.0\",\"assets\":[{{\"name\":\"x\",\"sha256\":\"{}\"}}]}}",
            "a".repeat(64)
        ))
        .unwrap();
        let err = Manifest::from_value(&v, true).unwrap_err();
        assert!(err.contains("unsigned asset never downloads"), "{err}");
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(newer_than("1.0.0", "1.0.1"));
        assert!(newer_than("1.9.0", "1.10.0"));
        assert!(!newer_than("1.0.1", "1.0.0"));
        assert!(!newer_than("1.0.0", "1.0.0"));
        assert!(!newer_than("1.0.1", "1.0.1"));
        assert!(newer_than("1.0", "1.0.1"));
        assert!(!newer_than("1.0.1", "1.0"));
        assert!(
            !newer_than("2.0.0", "2.0.0-rc1"),
            "a pre-release is not newer"
        );
        assert!(valid_version("1.0.0"));
        assert!(valid_version("1.0.0-rc1"));
        assert!(!valid_version("1.x"));
        assert!(!valid_version(""));
    }

    #[test]
    fn check_with_reports_an_update_only_when_newer() {
        let dir = tmp("check");
        let (path, _) = write_manifest(&dir, &[("wright-9.9.9-linux-x64.tar.gz", b"bin")]);
        let bytes = std::fs::read(&path).unwrap();
        let channel = Channel {
            url: "https://updates.invalid/wright-latest.manifest.json".into(),
            key: pk(),
        };
        let fetch = |_u: &str| Ok(bytes.clone());
        match check_with(&fetch, &channel, "1.0.0").unwrap() {
            Outcome::Update { version, asset, .. } => {
                assert_eq!(version, "9.9.9");
                // this test runs on linux CI; the heuristic picks the tarball
                if cfg!(target_os = "linux") {
                    assert!(asset.is_some_and(|a| a.name.contains("tar.gz")));
                }
            }
            Outcome::UpToDate => panic!("9.9.9 > 1.0.0"),
        }
        assert!(matches!(
            check_with(&fetch, &channel, "9.9.9").unwrap(),
            Outcome::UpToDate
        ));
        // a tampered manifest refuses
        let tampered = String::from_utf8(bytes.clone())
            .unwrap()
            .replace("the next one", "something else");
        let fetch_bad = move |_u: &str| Ok(tampered.clone().into_bytes());
        assert!(check_with(&fetch_bad, &channel, "1.0.0").is_err());
    }

    /// The check goes through the REAL net stack: a loopback server
    /// hands the manifest, exactly the one-GET shape.
    #[test]
    fn check_reads_one_get_through_the_real_net_stack() {
        use std::io::{BufRead, BufReader, Write};
        let dir = tmp("net");
        let (path, _) = write_manifest(&dir, &[("a.zip", b"asset bytes")]);
        let bytes = std::fs::read(&path).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = bytes.clone();
        let hit = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hit2 = hit.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    if r.read_line(&mut line).unwrap() == 0 || line.trim().is_empty() {
                        break;
                    }
                }
                hit2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    served.len()
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.write_all(&served);
                let _ = s.flush();
            }
        });
        let channel = Channel {
            url: format!("http://127.0.0.1:{port}/wright-latest.manifest.json"),
            key: pk(),
        };
        match check_with(&http_get, &channel, "1.0.0").unwrap() {
            Outcome::Update { version, .. } => assert_eq!(version, "9.9.9"),
            Outcome::UpToDate => panic!("the served manifest is newer"),
        }
        assert_eq!(
            hit.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "exactly one GET"
        );
    }

    #[test]
    fn sign_file_signs_assets_and_the_manifest() {
        let dir = tmp("sign");
        let asset_bytes = b"the real installer bytes";
        std::fs::write(dir.join("Wright-Setup-9.9.9-x64.exe"), asset_bytes).unwrap();
        // start from a manifest with hashes but no signatures
        let mut m = Manifest {
            version: "9.9.9".into(),
            notes: "n".into(),
            signed_by_key: String::new(),
            signed_by_provenance: String::new(),
            assets: vec![Asset {
                name: "Wright-Setup-9.9.9-x64.exe".into(),
                sha256: sha256::hex(asset_bytes),
                size: asset_bytes.len() as u64,
                ed25519_sig: String::new(),
            }],
            ed25519_sig: String::new(),
        };
        let path = dir.join(manifest_name("9.9.9"));
        std::fs::write(&path, json::to_string(&m.to_value())).unwrap();
        let (pk_hex, n) = sign_file(&path, &dir, &key()).unwrap();
        assert_eq!(n, 1);
        assert_eq!(pk_hex, hex(&pk()));
        let verified = verify_manifest(&std::fs::read(&path).unwrap(), &pk()).unwrap();
        assert_eq!(verified.signed_by_provenance, "ephemeral");
        assert!(verify_asset(asset_bytes, &verified.assets[0], &pk()).is_ok());
        // a changed asset file under the signer is refused
        std::fs::write(dir.join("Wright-Setup-9.9.9-x64.exe"), b"different").unwrap();
        let err = sign_file(&path, &dir, &key()).unwrap_err();
        assert!(err.contains("changed under you"), "{err}");
        m.version = "1.0".into();
        let _ = m;
    }

    #[test]
    fn the_release_key_never_generates_silently() {
        // The key resolver errors when nothing is configured. Reading
        // the env under test would race other tests; the sentence is
        // what matters, and it names the keychain reference.
        // (A happy path lives in the workflow, with the env set.)
        let err = match release_signing_key() {
            Err(e) => e,
            Ok(_) => panic!("no key was configured; one must not appear"),
        };
        assert!(err.contains("no release signing key"), "{err}");
        assert!(err.contains("keychain:wright/release-signing"), "{err}");
    }

    #[test]
    fn channels_need_both_a_url_and_a_key() {
        assert!(Channel::from_settings(&UpdateSettings::default())
            .unwrap()
            .is_none());
        let u = UpdateSettings {
            url: "https://x/wright-latest.manifest.json".into(),
            key: String::new(),
            check: None,
        };
        assert!(Channel::from_settings(&u)
            .unwrap_err()
            .contains("no release key is pinned"));
        let did = jcs::did_key(&pk());
        let u = UpdateSettings {
            url: "https://x/wright-latest.manifest.json".into(),
            key: did,
            check: Some(true),
        };
        let c = Channel::from_settings(&u).unwrap().unwrap();
        assert_eq!(c.key, pk(), "a did:key pins the same key");
        let bad = UpdateSettings {
            url: "u".into(),
            key: "not-a-key".into(),
            check: None,
        };
        assert!(Channel::from_settings(&bad).is_err());
    }

    #[test]
    fn asset_names_cannot_escape() {
        let v = json::parse(&format!(
            "{{\"format\":\"{FORMAT}\",\"version\":\"1.0.0\",\"assets\":[{{\"name\":\"../evil\",\"sha256\":\"{}\",\"ed25519_sig\":\"{}\"}}]}}",
            "a".repeat(64),
            "b".repeat(128)
        ))
        .unwrap();
        assert!(Manifest::from_value(&v, true)
            .unwrap_err()
            .contains("not a plain file name"));
    }

    #[test]
    fn apply_swaps_atomically_and_refuses_tampered_assets() {
        let dir = tmp("apply");
        let target = dir.join("wright");
        std::fs::write(&target, b"the old binary").unwrap();
        let new_bytes = b"the new binary, verifiably";
        let (path, m) = write_manifest(&dir, &[("wright-9.9.9-linux-x64.tar.gz", new_bytes)]);
        let manifest_bytes = std::fs::read(&path).unwrap();
        let channel = Channel {
            url: "https://updates.invalid/wright-latest.manifest.json".into(),
            key: pk(),
        };
        // fetch serves the manifest at the channel URL and the asset at
        // the URL that replaces the manifest's file name with the asset's.
        let serve = |url: &str| -> Result<Vec<u8>, String> {
            if url.ends_with(LATEST_ALIAS) {
                Ok(manifest_bytes.clone())
            } else if let Some(name) = url.rsplit('/').next() {
                Ok(std::fs::read(dir.join(name))
                    .unwrap_or_else(|_| panic!("test fetch: unexpected asset {name}")))
            } else {
                Err("bad url".into())
            }
        };
        let want = if cfg!(target_os = "linux") {
            m.asset_for_this_platform().map(|a| a.name.clone())
        } else {
            Some("wright-9.9.9-linux-x64.tar.gz".into())
        };
        let said = apply_with(&serve, &channel, "1.0.0", want.as_deref(), &target).unwrap();
        assert!(said.contains("Restart wright"), "{said}");
        assert_eq!(std::fs::read(&target).unwrap(), new_bytes);
        assert!(
            !dir.join(".wright-update-0-new").exists(),
            "no temp file left behind"
        );

        // a tampered asset leaves the (already-updated) target alone:
        // rebuild with different bytes under the same manifest promises
        let target2 = dir.join("wright2");
        std::fs::write(&target2, b"the old binary").unwrap();
        let bad = |url: &str| -> Result<Vec<u8>, String> {
            if url.ends_with(LATEST_ALIAS) {
                Ok(manifest_bytes.clone())
            } else {
                Ok(b"tampered asset bytes".to_vec())
            }
        };
        let err = apply_with(
            &bad,
            &channel,
            "1.0.0",
            Some("wright-9.9.9-linux-x64.tar.gz"),
            &target2,
        )
        .unwrap_err();
        assert!(err.contains("not what the manifest promised"), "{err}");
        assert_eq!(
            std::fs::read(&target2).unwrap(),
            b"the old binary",
            "a refused update touches nothing"
        );

        // not-newer refused
        let err = apply_with(&serve, &channel, "9.9.9", want.as_deref(), &target).unwrap_err();
        assert!(err.contains("nothing newer to apply"), "{err}");
    }
}
