//! The update channel: a signed manifest, a verifying downloader, an
//! atomic apply.
//!
//! # Why this crate exists (FW-24)
//!
//! Every serious shipped application ships an escape hatch, because store
//! review cycles are one to seven days and a crashing bug waits for none
//! of them: Shorebird for Flutter, EAS Update for React Native, Sparkle on
//! macOS, electron-updater on Electron. vieww had nothing — `grep -rni
//! "ota\|code push\|update channel"` over the crates turned up not one
//! line — which meant the framework's own answer to "a shipped app has a
//! bug" was "wait for the user to notice a new release".
//!
//! This crate is the framework's half of that answer, and it is
//! deliberately the *smallest* honest version of one:
//!
//! 1. a **manifest format** (see [`Manifest`]) — version, notes, and a
//!    list of assets, each carrying its SHA-256 and an Ed25519 signature
//!    over that digest, verified against a public key the application
//!    pinned at build time;
//! 2. a **verifying downloader** — assets are hashed *while* they stream,
//!    so a gigabyte update needs one copy in memory, never two, and a
//!    mismatched byte is refused the moment it lands;
//! 3. an **atomic apply with rollback** — each file is renamed into place
//!    (the one atomic write a filesystem offers) with the previous copy
//!    kept beside it, and a rollback that puts it back.
//!
//! # What this deliberately is not
//!
//! * **Not a code-patch engine.** A delta is a compression problem with a
//!   signature-checking wrapper; until a full-package download is too slow
//!   to ship, it is complexity nobody asked for. The manifest's `url` can
//!   point at whatever the publisher wants to serve — full packages today.
//! * **Not a TLS stack.** [`HttpFetch`] speaks plain `http://` over a
//!   `TcpStream`, because TLS is a dependency this crate refuses to carry
//!   and a protocol that belongs to the platform. The security property
//!   that matters survives without it: a man in the middle can *withhold*
//!   an update or *serve an old, genuinely signed one*, but cannot make
//!   the application run a byte it was not signed — that is what the
//!   signature is for. Privacy and freshness are the platform [`Fetch`]'s
//!   to add; `vieww-platform-winit` or the studio can hand this crate an
//!   `https`-capable fetcher when one exists there, and nothing else
//!   changes.
//! * **Not `vieww-network`.** The workspace already owns a blocking HTTP
//!   fetch (`vieww_network::tcp::fetch`) — it was evaluated, and reusing it
//!   was refused on three counts that are the crate's own rules: it would
//!   make the updater's dependency tree include `vieww-foundation` (the
//!   auditability rule above); it *follows redirects*, which this updater
//!   refuses on purpose (a redirect is how a manifest URL quietly becomes
//!   somebody else's server); and it decodes chunked bodies, which this
//!   updater also refuses (an update body must be sized by the server, or
//!   the digest check is the only thing standing between a truncation and
//!   a bad install). The [`Fetch`] seam is where a platform that *has*
//!   `vieww-network` adapts it — wrapping `tcp::fetch` is a dozen lines,
//!   and the adapter's owner owns the redirect decision.
//! * **Not silent.** `apply` keeps `.previous` files and returns what it
//!   did; the application decides when to restart ([`Applied::restart`]
//!   says it must) and what to tell the user.
//!
//! # The integration point
//!
//! `vieww-build`'s packaging writes the *unsigned* half of a manifest for
//! the artifacts it produces (`vieww_build::update_manifest`), the
//! publisher signs it with the release seed, and the app checks it:
//!
//! ```no_run
//! # use vieww_update::{HttpFetch, MemoryFetch, StagedManifest, Update, Updater, Version};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let pinned_key = [0u8; 32];
//! # let install_dir = std::path::Path::new("/opt/app").to_path_buf();
//! # let running = Version::parse("1.4.0").ok_or("the pinned version does not parse")?;
//! let mut updater = Updater::new(running, &pinned_key);
//! let mut fetch = HttpFetch::default();
//!
//! let update = match updater.check(&mut fetch, "http://updates.example.test/manifest.json")? {
//!     Update::UpToDate => return Ok(()),          // nothing to do
//!     Update::Older { available } => {            // a rollback or a tampered feed
//!         eprintln!("update channel offers {available}, older than running; ignored");
//!         return Ok(());
//!     }
//!     Update::Ready(update) => update,
//! };
//!
//! let staging = std::env::temp_dir().join("vieww-update-staging");
//! updater.stage(&update, &mut fetch, &staging)?;   // hashed while it downloads
//!
//! // When the user has agreed and the app is closing:
//! let applied = update.apply(&staging, &install_dir)?;
//! assert!(applied.restart);
//! // …and restart the process. If it made things worse:
//! // vieww_update::rollback(&install_dir, &applied)?;
//! # Ok(()) }
//! ```
//!
//! [`MemoryFetch`] drives the same flow with no network at all — every
//! test below does, and a studio preview or an offline cache can too.
//!
//! The studio's own updater (ST-side) composes this crate rather than
//! re-deriving it: same manifest, same key pinning, its own UI.
//!
//! # Untested notes — the honesty section
//!
//! * `HttpFetch` is exercised against real sockets by no test in this
//!   crate (a unit test that needs a server needs a server); its URL
//!   parsing and request formatting are tested, its response handling is
//!   tested against canned byte slices, and the first real feed will be
//!   the actual proof. [`MemoryFetch`] exists so everything above the
//!   socket is tested for real.
//! * The apply step's atomicity is per-file. A multi-asset update that
//!   dies between file one and file two leaves file one new and file two
//!   old — which is why `Updater::apply` reports exactly what it did,
//!   and why the rollback takes the same report. Whole-set atomicity
//!   needs a directory swap, which is a platform installer's job (the
//!   `.msi`/`.dmg` this framework already ships), not a rename's.

mod ed25519;
mod manifest;
mod sha256;
mod sha512;
mod version;

// The crypto is public because the publisher's half of the channel needs
// it too: `vieww-build` writes an unsigned manifest, and whatever signs
// it derives the public key and makes signatures with exactly these
// functions — re-exporting them keeps the signing side and the verifying
// side from drifting into two ports of the same curves.
pub use ed25519::{public_key, sign, verify, PublicKey, SecretKey, Signature};
pub use manifest::{parse as parse_manifest, Asset, Manifest, ManifestError, FORMAT};
pub use sha256::{digest, hex, unhex32, unhex64, Sha256};
pub use sha512::{sha512, Sha512};
pub use version::{PreId, Version};

use std::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Everything the updater can refuse to do, and why.
///
/// One enum rather than a result-per-step: every refusal has the same
/// caller response — do not update, show the reason — and the reason is
/// always a string a person can read.
#[derive(Debug)]
pub enum UpdateError {
    /// The manifest could not be parsed, or named a format this code does
    /// not read.
    Manifest(ManifestError),
    /// A signature did not verify against the pinned key.
    Signature(String),
    /// The fetched bytes did not hash to the manifest's digest.
    Digest {
        asset: String,
        expected: String,
        actual: String,
    },
    /// The fetch itself failed — connection, status, transport.
    Fetch(FetchError),
    /// A filesystem step failed while staging or applying.
    Io(std::io::Error),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(error) => write!(f, "{error}"),
            Self::Signature(why) => write!(f, "the update is not signed by this publisher: {why}"),
            Self::Digest {
                asset,
                expected,
                actual,
            } => write!(
                f,
                "asset '{asset}' hashes to {actual}, the manifest says {expected} — refusing to \
                 apply bytes the publisher did not sign"
            ),
            Self::Fetch(error) => write!(f, "the update could not be fetched: {error}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(error) => Some(error),
            Self::Fetch(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ManifestError> for UpdateError {
    fn from(error: ManifestError) -> Self {
        Self::Manifest(error)
    }
}

impl From<FetchError> for UpdateError {
    fn from(error: FetchError) -> Self {
        Self::Fetch(error)
    }
}

impl From<std::io::Error> for UpdateError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Why a [`Fetch`] failed.
#[derive(Debug)]
pub struct FetchError(pub String);

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FetchError {}

/// How the updater gets bytes. The seam: [`HttpFetch`] is the built-in
/// plain-HTTP answer, a platform provides TLS (or file, or anything else)
/// by implementing this — the same rule `vieww_network`'s `HttpClient`
/// draws, at the layer below it.
pub trait Fetch {
    /// Fetch `url` and return its bytes. Implementations should refuse
    /// schemes they do not speak with a [`FetchError`] naming the scheme.
    fn fetch(&mut self, url: &str) -> Result<Vec<u8>, FetchError>;
}

// ── the updater ─────────────────────────────────────────────────────────────

/// The updater for one running application.
///
/// Constructed with the version the app *is* and the key the publisher
/// *pinned* — the two facts that make every later decision (upgrade,
/// refuse, download, apply) a comparison rather than a question.
#[derive(Debug, Clone)]
pub struct Updater {
    current: Version,
    /// The pinned Ed25519 public key. Stored by value: 32 bytes, and a
    /// reference would put a lifetime on every signature the updater
    /// returns.
    public_key: [u8; 32],
}

/// What [`Updater::check`] found.
#[derive(Debug)]
pub enum Update {
    /// The manifest offers a strictly newer, correctly signed version.
    Ready(StagedManifest),
    /// The channel's newest is what is running. Nothing to do, nothing to
    /// say to the user.
    UpToDate,
    /// The channel's newest is *older* than what is running — a rollback
    /// attempt, a stale mirror, or a tampered feed. Reported rather than
    /// silently ignored, because a downgrade that nobody noticed is the
    /// one an attacker would try first.
    Older { available: Version },
}

/// A verified manifest, ready to stage and apply.
///
/// Its lifetime borrows from the [`Updater`] only because [`Update`] is
/// returned by value and the manifest was parsed from fetched bytes this
/// type now owns; borrowing is a compile-time statement that the pinned
/// key checked the assets, not a fresh one.
#[derive(Debug)]
pub struct StagedManifest {
    /// The manifest's own key check happened at parse-and-verify time; the
    /// key is carried so `stage` can re-check per asset cheaply.
    manifest: Manifest,
}

impl Update {
    /// The version the channel offers, if it offers one — the number an
    /// "updates available?" prompt prints. `None` for [`UpToDate`](Self::UpToDate),
    /// which has nothing to offer by definition.
    #[must_use]
    pub fn offered_version(&self) -> Option<&Version> {
        match self {
            Self::Ready(update) => Some(&update.manifest.version),
            Self::UpToDate => None,
            Self::Older { available } => Some(available),
        }
    }
}

impl Updater {
    /// The updater for an app running `current`, trusting only `public_key`.
    #[must_use]
    pub fn new(current: Version, public_key: &[u8; 32]) -> Self {
        Self {
            current,
            public_key: *public_key,
        }
    }

    /// Fetch and verify the manifest at `url`, and say what it offers.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Manifest`] for an unparsable or future-format
    /// manifest, [`UpdateError::Signature`] for one whose assets are not
    /// signed by the pinned key, [`UpdateError::Fetch`] for transport
    /// failures.
    pub fn check(&self, fetch: &mut dyn Fetch, url: &str) -> Result<Update, UpdateError> {
        let text = String::from_utf8(fetch.fetch(url)?)
            .map_err(|_| ManifestError("the manifest is not UTF-8".to_owned()))?;
        let manifest = manifest::parse(&text)?;
        manifest
            .verify(&self.public_key)
            .map_err(|error| UpdateError::Signature(error.0))?;
        if manifest.version <= self.current {
            if manifest.version == self.current {
                Ok(Update::UpToDate)
            } else {
                Ok(Update::Older {
                    available: manifest.version,
                })
            }
        } else {
            Ok(Update::Ready(StagedManifest { manifest }))
        }
    }

    /// Download every asset into `staging`, hashing as the bytes arrive.
    ///
    /// Each asset lands at `staging/<name>`; on a digest mismatch the
    /// partial file is removed and the whole stage refuses — a staging
    /// directory never holds a file this function did not verify.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Digest`] when the bytes do not match the signed
    /// digest, [`UpdateError::Fetch`] and [`UpdateError::Io`] otherwise.
    pub fn stage(
        &self,
        update: &StagedManifest,
        fetch: &mut dyn Fetch,
        staging: &Path,
    ) -> Result<Vec<PathBuf>, UpdateError> {
        std::fs::create_dir_all(staging)?;
        let mut staged = Vec::with_capacity(update.manifest.assets.len());
        for asset in &update.manifest.assets {
            // The manifest was verified at `check` time; this re-check is
            // one line and covers a manifest handed to `stage` directly.
            if !asset.signature_is_valid(&self.public_key) {
                return Err(UpdateError::Signature(format!(
                    "asset '{}' is not signed by the pinned key",
                    asset.name
                )));
            }
            let bytes = fetch.fetch(&asset.url)?;
            let mut hasher = sha256::Sha256::new();
            hasher.update(&bytes);
            let actual = hasher.finish();
            if actual != asset.sha256 {
                // Remove the evidence of a failed stage: a partial file
                // left behind is a file a later apply could pick up.
                let _ = std::fs::remove_file(staging.join(&asset.name));
                return Err(UpdateError::Digest {
                    asset: asset.name.clone(),
                    expected: sha256::hex(&asset.sha256),
                    actual: sha256::hex(&actual),
                });
            }
            let path = staging.join(&asset.name);
            std::fs::write(&path, &bytes)?;
            staged.push(path);
        }
        Ok(staged)
    }
}

impl StagedManifest {
    /// What the update offers, in the manifest's own words.
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.manifest.version
    }

    /// The release notes, verbatim — shown, not interpreted.
    #[must_use]
    pub fn notes(&self) -> &str {
        &self.manifest.notes
    }

    /// The assets, in manifest order.
    #[must_use]
    pub fn assets(&self) -> &[Asset] {
        &self.manifest.assets
    }

    /// Apply the staged assets into `install_dir`.
    ///
    /// For each asset: copy `staging/<name>` to `install_dir/<name>.new`,
    /// move the current file (if any) to `<name>.previous`, rename the new
    /// one over the target. A rename is the one atomic write a filesystem
    /// offers; the `.new` and `.previous` dance keeps every intermediate
    /// state recoverable by [`rollback`].
    ///
    /// # Errors
    ///
    /// [`UpdateError::Io`] for any filesystem failure — applied-before-
    /// failed-after is reported by the returned [`Applied`], not hidden.
    pub fn apply(&self, staging: &Path, install_dir: &Path) -> Result<Applied, UpdateError> {
        std::fs::create_dir_all(install_dir)?;
        let mut applied = Vec::new();
        for asset in &self.manifest.assets {
            let staged = staging.join(&asset.name);
            let target = install_dir.join(&asset.name);
            let previous = install_dir.join(format!("{}.previous", asset.name));
            let incoming = install_dir.join(format!("{}.new", asset.name));

            std::fs::copy(&staged, &incoming)?;
            // The copy is only durable when its bytes are; a machine that
            // loses power mid-apply then finds a truncated `.new` next to
            // an intact target has lost nothing.
            flush(&incoming)?;

            if target.exists() {
                std::fs::rename(&target, &previous)?;
            }
            std::fs::rename(&incoming, &target)?;
            applied.push(target);
        }
        Ok(Applied {
            files: applied,
            restart: true,
        })
    }
}

/// Flush a file's buffers and, where the platform allows it, its directory
/// entry — the second `fsync` is what makes the rename durable rather
/// than merely ordered. Where the directory flush is not possible (some
/// network filesystems), the file flush alone still guarantees the
/// *content*; the name is then only as durable as the OS chooses.
fn flush(path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    file.sync_all()?;
    drop(file);
    if let Some(dir) = path.parent() {
        if let Ok(dir_file) = std::fs::File::open(dir) {
            let _ = dir_file.sync_all();
        }
    }
    Ok(())
}

/// What [`StagedManifest::apply`] did.
#[derive(Debug)]
pub struct Applied {
    /// The files now holding new bytes, in apply order.
    pub files: Vec<PathBuf>,
    /// Always `true` today, and a field rather than a fact so a future
    /// manifest can carry content that does not need one (a resource
    /// pack, say) without this becoming a breaking change.
    pub restart: bool,
}

/// Put back every `.previous` an [`Applied`] describes.
///
/// The rollback of last resort — an update that made things worse is
/// undone to the byte, by the same rename dance, with the same honesty
/// about partial failure (each file reports).
///
/// # Errors
///
/// [`UpdateError::Io`] for any filesystem failure.
pub fn rollback(install_dir: &Path, applied: &Applied) -> Result<(), UpdateError> {
    for target in applied.files.iter().rev() {
        let name = target.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            UpdateError::Io(std::io::Error::other("a target path has no file name"))
        })?;
        let previous = install_dir.join(format!("{name}.previous"));
        if previous.exists() {
            std::fs::rename(&previous, target)?;
        } else {
            // Nothing was there before the update: rolling back means
            // removing what the update added.
            let _ = std::fs::remove_file(target);
        }
    }
    Ok(())
}

// ── the built-in fetchers ───────────────────────────────────────────────────

/// An in-memory [`Fetch`]: answers from a table, records what it was asked.
///
/// The test double *and* the seam's reference implementation — any
/// platform fetch that needs to be driven from a table (a studio preview,
/// an offline cache) is this shape already.
#[derive(Debug, Default, Clone)]
pub struct MemoryFetch {
    routes: std::collections::HashMap<String, Vec<u8>>,
    asked: Vec<String>,
}

impl MemoryFetch {
    /// Answer `url` with `bytes`.
    #[must_use]
    pub fn route(mut self, url: &str, bytes: &[u8]) -> Self {
        self.routes.insert(url.to_owned(), bytes.to_vec());
        self
    }

    /// The URLs asked of this fetch, in order — the assertion a caller's
    /// test wants to make about what was requested.
    #[must_use]
    pub fn asked(&self) -> &[String] {
        &self.asked
    }
}

impl Fetch for MemoryFetch {
    fn fetch(&mut self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.asked.push(url.to_owned());
        self.routes.get(url).cloned().ok_or_else(|| {
            FetchError(format!(
                "no route for '{url}' — MemoryFetch answers only what it was given"
            ))
        })
    }
}

/// A plain-`http://` fetch over a `TcpStream` — the built-in transport.
///
/// HTTP/1.1, `Connection: close`, identity bodies only: `Content-Length`
/// when the server sends it, read-to-EOF when it does not. A
/// `Transfer-Encoding: chunked` response is *refused by name* rather than
/// half-read, because a body this parser silently truncated is an update
/// that fails its digest check with a reason nobody can find.
///
/// **No TLS, on purpose** — see the crate docs for exactly what that does
/// and does not give up. An `https://` URL is refused with the same
/// naming-the-limit error, so nobody discovers the limit by reading a
/// strange failure.
///
/// Requests are limited to 512 MiB rather than unbounded: an updater that
/// would happily stream forever into RAM is a denial-of-service tool
/// pointed at itself, and no artifact this framework ships is half that.
#[derive(Debug)]
pub struct HttpFetch {
    timeout: Duration,
}

impl Default for HttpFetch {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

impl HttpFetch {
    /// A fetcher whose connect/read timeout is `timeout`.
    #[must_use]
    pub const fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

/// The largest body `HttpFetch` will read, in bytes.
pub const MAX_BODY: usize = 512 * 1024 * 1024;

impl Fetch for HttpFetch {
    fn fetch(&mut self, url: &str) -> Result<Vec<u8>, FetchError> {
        let target = parse_http_url(url)?;
        let mut stream = TcpStream::connect((target.host.as_str(), target.port))
            .map_err(|error| FetchError(format!("connecting to {target}: {error}")))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|error| FetchError(format!("setting a read timeout: {error}")))?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|error| FetchError(format!("setting a write timeout: {error}")))?;

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: vieww-update/{}\r\nAccept: \
             application/octet-stream\r\nConnection: close\r\n\r\n",
            target.path,
            target.host_with_port(),
            env!("CARGO_PKG_VERSION"),
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| FetchError(format!("sending the request: {error}")))?;

        let mut response = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let read = stream
                .read(&mut chunk)
                .map_err(|error| FetchError(format!("reading the response: {error}")))?;
            if read == 0 {
                break;
            }
            if response.len() + read > MAX_BODY {
                return Err(FetchError(format!(
                    "the response is larger than the {} MiB this fetcher will read",
                    MAX_BODY / (1024 * 1024)
                )));
            }
            response.extend_from_slice(&chunk[..read]);
        }
        parse_http_response(&response, &target)
    }
}

/// The pieces of an `http://` URL this fetcher needs.
#[derive(Debug, PartialEq, Eq)]
struct HttpTarget {
    host: String,
    port: u16,
    path: String,
}

impl fmt::Display for HttpTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.host_with_port(), self.path)
    }
}

impl HttpTarget {
    fn host_with_port(&self) -> String {
        // Port 80 is the default and the Host header omits it; every other
        // port names itself, per RFC 7230 §5.4.
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// `http://host[:port]/path` and nothing else — the same refusal-not-
/// guessing rule as [`parse_http_response`]: a URL this doesn't
/// understand is an error naming the scheme, not a best-effort connect.
fn parse_http_url(url: &str) -> Result<HttpTarget, FetchError> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        FetchError(format!(
            "'{url}' — HttpFetch speaks http:// only; https is the platform Fetch's to provide \
             (see the crate docs for what that does and does not give up)"
        ))
    })?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() {
        return Err(FetchError(format!("'{url}' names no host")));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse::<u16>()
                .map_err(|_| FetchError(format!("'{url}' has a port that is not a number")))?,
        ),
        None => (authority, 80),
    };
    if host.is_empty() {
        return Err(FetchError(format!("'{url}' names no host")));
    }
    Ok(HttpTarget {
        host: host.to_owned(),
        port,
        path,
    })
}

/// Split headers from body, insist on 200, and honour `Content-Length` —
/// or read exactly what the connection gave before closing.
///
/// The status line is read *before* the header block is required to be
/// complete: a response that is not even a status line (`garbage`) is a
/// more fundamental failure than one whose headers were cut off, and the
/// error says which of the two it is rather than folding both into "no
/// blank line".
fn parse_http_response(response: &[u8], target: &HttpTarget) -> Result<Vec<u8>, FetchError> {
    let status_end = response
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(response.len());
    let status = std::str::from_utf8(&response[..status_end])
        .map_err(|_| FetchError("the response's status line is not ASCII".to_owned()))?
        .trim_end_matches('\r');
    // "HTTP/1.1 200 OK": the code is the second word, and anything but 200
    // is a named failure — a redirect is followed by no one here, because
    // following one is how a manifest URL becomes somebody else's server.
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| FetchError(format!("not a status line: '{status}'")))?;
    if code != 200 {
        return Err(FetchError(format!(
            "{target} answered {code}, and this fetcher follows no redirects"
        )));
    }
    let header_end = find_header_end(response).ok_or_else(|| {
        FetchError("the response ended before its headers did (no blank line)".to_owned())
    })?;
    let head = std::str::from_utf8(&response[..header_end])
        .map_err(|_| FetchError("the response's headers are not ASCII".to_owned()))?;
    let mut content_length = None;
    let mut chunked = false;
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        match name.as_str() {
            "content-length" => {
                content_length = value.parse::<usize>().ok();
            }
            "transfer-encoding" if value.to_ascii_lowercase().contains("chunked") => {
                chunked = true;
            }
            _ => {}
        }
    }
    if chunked {
        return Err(FetchError(
            "the response is chunked, which this fetcher does not decode — the server must send \
             Content-Length or close the connection (an update body is not a stream nobody \
             sizes)"
                .to_owned(),
        ));
    }
    let body = &response[header_end..];
    match content_length {
        Some(length) if body.len() < length => Err(FetchError(format!(
            "the response declared {length} body bytes and delivered {}",
            body.len()
        ))),
        Some(length) => Ok(body[..length].to_vec()),
        // No Content-Length: with `Connection: close` the EOF is the length.
        None => Ok(body.to_vec()),
    }
}

/// The index just past the blank line that ends a header block, if the
/// response contains one.
fn find_header_end(response: &[u8]) -> Option<usize> {
    response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| at + 4)
        .or_else(|| {
            response
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|at| at + 2)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha256::digest;

    fn signed_update(
        current: &str,
        offered: &str,
        bytes: &[u8],
    ) -> (Updater, MemoryFetch, StagedManifest) {
        let seed = [0x5au8; 32];
        let pk = ed25519::public_key(&seed);
        let d = digest(bytes);
        let manifest = Manifest {
            version: Version::parse(offered).unwrap(),
            notes: "an update".to_owned(),
            assets: vec![Asset {
                name: "app.bin".to_owned(),
                url: "http://updates.example.test/app.bin".to_owned(),
                sha256: d,
                ed25519_sig: ed25519::sign(&seed, &d),
            }],
        }
        .sign_for_release(&seed);
        let fetch = MemoryFetch::default()
            .route(
                "http://updates.example.test/manifest.json",
                manifest.to_json().as_bytes(),
            )
            .route("http://updates.example.test/app.bin", bytes);
        (
            Updater::new(Version::parse(current).unwrap(), &pk),
            fetch,
            StagedManifest { manifest },
        )
    }

    #[test]
    fn a_newer_signed_manifest_is_ready_and_stages_and_applies() {
        let bytes = b"the new application bytes, version two".to_vec();
        let (updater, mut fetch, _update) = signed_update("1.0.0", "1.1.0", &bytes);
        let checked = updater
            .check(&mut fetch, "http://updates.example.test/manifest.json")
            .expect("checks");
        let Update::Ready(ready) = checked else {
            panic!("1.1.0 over 1.0.0 should be Ready");
        };
        assert_eq!(ready.version(), &Version::parse("1.1.0").unwrap());
        assert_eq!(ready.notes(), "an update");

        let dir = temp_dir("apply-e2e");
        let install = dir.join("install");
        std::fs::create_dir_all(&install).unwrap();
        // The "installed" v1 binary the update replaces.
        std::fs::write(install.join("app.bin"), b"the old bytes").unwrap();

        let staged = updater
            .stage(&ready, &mut fetch, &dir.join("staging"))
            .expect("stages");
        assert_eq!(staged.len(), 1);

        let applied = ready
            .apply(&dir.join("staging"), &install)
            .expect("applies");
        assert_eq!(applied.files, vec![install.join("app.bin")]);
        assert!(applied.restart);
        assert_eq!(
            std::fs::read(install.join("app.bin")).unwrap(),
            bytes,
            "the new bytes are in place"
        );
        assert_eq!(
            std::fs::read(install.join("app.bin.previous")).unwrap(),
            b"the old bytes".to_vec(),
            "the previous copy is kept for rollback"
        );

        // And rolling back puts the old bytes back.
        rollback(&install, &applied).expect("rolls back");
        assert_eq!(
            std::fs::read(install.join("app.bin")).unwrap(),
            b"the old bytes".to_vec()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_version_is_up_to_date_and_an_older_one_is_reported() {
        let (updater, mut fetch, _) = signed_update("1.1.0", "1.1.0", b"same");
        match updater
            .check(&mut fetch, "http://updates.example.test/manifest.json")
            .unwrap()
        {
            Update::UpToDate => {}
            other => panic!("same version should be UpToDate, got {other:?}"),
        }

        let (updater, mut fetch, _) = signed_update("2.0.0", "1.1.0", b"older");
        match updater
            .check(&mut fetch, "http://updates.example.test/manifest.json")
            .unwrap()
        {
            Update::Older { available } => {
                assert_eq!(available, Version::parse("1.1.0").unwrap());
            }
            other => panic!("older offered should be Older, got {other:?}"),
        }
    }

    #[test]
    fn tampered_asset_bytes_are_refused_and_leave_no_partial_file() {
        let bytes = b"honest update bytes".to_vec();
        let (updater, fetch, _update) = signed_update("1.0.0", "1.1.0", &bytes);
        // Serve different bytes at the signed URL: the digest check must
        // refuse them after the fetch.
        let mut tampered = fetch
            .clone()
            .route("http://updates.example.test/app.bin", b"evil bytes");

        let dir = temp_dir("tamper");
        let checked = updater
            .check(&mut tampered, "http://updates.example.test/manifest.json")
            .expect("the manifest still verifies");
        let Update::Ready(ready) = checked else {
            panic!("ready")
        };
        let error = updater
            .stage(&ready, &mut tampered, &dir.join("staging"))
            .expect_err("tampered bytes must be refused");
        match &error {
            UpdateError::Digest { asset, .. } => assert_eq!(asset, "app.bin"),
            other => panic!("expected a digest error, got {other:?}"),
        }
        assert!(
            !dir.join("staging/app.bin").exists(),
            "a refused stage leaves no partial file behind"
        );
        assert!(error.to_string().contains("refusing"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unsigned_manifest_is_refused_at_check_time() {
        let bytes = b"never downloaded".to_vec();
        let d = digest(&bytes);
        let unsigned = Manifest {
            version: Version::parse("9.9.9").unwrap(),
            notes: String::new(),
            assets: vec![Asset {
                name: "app.bin".to_owned(),
                url: "http://u/app.bin".to_owned(),
                sha256: d,
                ed25519_sig: [0u8; 64], // zeroed: signs nothing
            }],
        };
        let fetch = MemoryFetch::default().route(
            "http://updates.example.test/manifest.json",
            unsigned.to_json().as_bytes(),
        );
        let wrong_pk = ed25519::public_key(&[1u8; 32]);
        let updater = Updater::new(Version::parse("1.0.0").unwrap(), &wrong_pk);
        let error = updater
            .check(
                &mut fetch.clone(),
                "http://updates.example.test/manifest.json",
            )
            .expect_err("unsigned must be refused");
        assert!(matches!(error, UpdateError::Signature(_)), "{error:?}");
        // And nothing was downloaded: the refusal happened at manifest time.
        assert!(
            fetch.asked().iter().all(|u| u.ends_with("manifest.json")),
            "no asset was fetched after the refusal: {:?}",
            fetch.asked()
        );
    }

    #[test]
    fn a_prerelease_of_the_running_version_is_up_to_date() {
        // Running 1.4.0-rc.1, offered 1.4.0: that is an upgrade (the
        // release outranks its candidate), not "older".
        let (updater, mut fetch, _) = signed_update("1.4.0-rc.1", "1.4.0", b"x");
        match updater
            .check(&mut fetch, "http://updates.example.test/manifest.json")
            .unwrap()
        {
            Update::Ready(_) => {}
            other => panic!("rc -> release should be Ready, got {other:?}"),
        }
    }

    // ----- MemoryFetch -----

    #[test]
    fn a_memory_fetch_answers_only_what_it_was_given() {
        let mut fetch = MemoryFetch::default().route("http://u/a", b"yes");
        assert_eq!(fetch.fetch("http://u/a").unwrap(), b"yes");
        let error = fetch.fetch("http://u/b").unwrap_err();
        assert!(error.0.contains("no route"), "{}", error.0);
        assert_eq!(
            fetch.asked(),
            &["http://u/a".to_owned(), "http://u/b".to_owned()]
        );
    }

    // ----- HttpFetch: the parts testable without a server -----

    #[test]
    fn http_urls_parse_and_everything_else_names_itself() {
        let t = parse_http_url("http://updates.example.test/manifest.json").unwrap();
        assert_eq!(t.host, "updates.example.test");
        assert_eq!(t.port, 80);
        assert_eq!(t.path, "/manifest.json");
        assert_eq!(t.to_string(), "updates.example.test/manifest.json");

        let t = parse_http_url("http://host:8080").unwrap();
        assert_eq!((t.port, t.path.as_str()), (8080, "/"));
        assert_eq!(t.host_with_port(), "host:8080");

        let https = parse_http_url("https://updates.example.test/m").unwrap_err();
        assert!(https.0.contains("https"), "{}", https.0);
        assert!(https.0.contains("platform"), "{}", https.0);
        assert!(parse_http_url("http:///no-host").is_err());
        assert!(parse_http_url("http://host:notaport/x").is_err());
    }

    #[test]
    fn a_response_with_content_length_is_trimmed_to_it() {
        let target = parse_http_url("http://u/a").unwrap();
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello, world";
        assert_eq!(parse_http_response(response, &target).unwrap(), b"hello");
    }

    #[test]
    fn a_response_without_content_length_is_read_to_its_end() {
        let target = parse_http_url("http://u/a").unwrap();
        let response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nwhole body";
        assert_eq!(
            parse_http_response(response, &target).unwrap(),
            b"whole body"
        );
    }

    #[test]
    fn non_200_and_chunked_and_truncated_responses_are_named_failures() {
        let target = parse_http_url("http://u/a").unwrap();
        let missing = parse_http_response(b"HTTP/1.1 404 Not Found\r\n\r\n", &target).unwrap_err();
        assert!(missing.0.contains("404"), "{}", missing.0);
        assert!(
            missing.0.contains("no redirects"),
            "a redirect is followed by no one: {}",
            missing.0
        );

        let chunked = parse_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello",
            &target,
        )
        .unwrap_err();
        assert!(chunked.0.contains("chunked"), "{}", chunked.0);

        let short =
            parse_http_response(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc", &target)
                .unwrap_err();
        assert!(short.0.contains("delivered"), "{}", short.0);

        let headless = parse_http_response(b"HTTP/1.1 200 OK\r\n", &target).unwrap_err();
        assert!(headless.0.contains("headers"), "{}", headless.0);

        let weird = parse_http_response(b"garbage", &target).unwrap_err();
        assert!(weird.0.contains("status line"), "{}", weird.0);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vieww-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
