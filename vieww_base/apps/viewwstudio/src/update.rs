//! ST-14: checking for updates, and downloading what a first launch needs.
//!
//! # What this closes, and what it deliberately does not touch
//!
//! The studio shipped as one ~600 MB archive — a ~60 MB application welded to
//! the compiler and rlibs it must be paired with — and once installed it never
//! learned another release existed. This module is the studio-local half of
//! both fixes:
//!
//! * **The split SDK.** `packaging/package.sh --split-sdk` produces a small
//!   studio archive plus a `viewwstudio-sdk-<host>.tar.gz` carrying the rlibs,
//!   `deps/` and the toolchain. On first launch the SDK directory beside the
//!   executable is empty, the preview has no compiler, and
//!   [`Studio::download_sdk`](crate::state::Studio::download_sdk) can fetch the
//!   other half from **this studio's own release** — not "latest": the pairing
//!   between the binary and its rlibs is the entire doctrine of [`crate::sdk`],
//!   so a split SDK from any release but this one is refused by construction.
//!   The unpacked bundle is then put through the same `sdk::Manifest::read` +
//!   `verify` machinery the full bundle ships through — the download does not
//!   get to skip the check the CD does not skip.
//! * **The update check.** `--check-for-updates` on the command line, or
//!   *Check for Updates* in the palette, asks the GitHub releases feed whether
//!   a newer version exists, against the stable asset names `package.sh` has
//!   always written. Finding one is all it does on its own: fetching is a
//!   second command, and the fetch verifies the download against the release's
//!   published `SHA256SUMS` — hashed in-process — before anything is staged
//!   where a person could run it.
//!
//! # Why the network is a child process
//!
//! GitHub speaks HTTPS, and the studio's only in-process HTTP client
//! (`say_ai`'s localhost model) is plain `TcpStream` by design — a local model
//! needs no TLS. Rather than grow a TLS stack, the fetches run as `curl -fsSL`
//! through the same jobs machinery every other external tool goes through:
//! one process, a real exit status, output where output belongs. `curl` ships
//! with Windows 10 1803+, macOS and every desktop Linux; a machine without it
//! gets a refusal naming it, not a network error in a dialog.
//!
//! # What "apply" honestly means here
//!
//! The SDK download applies itself — unpacking into the directory
//! `crate::install` already searches, which no running process owns — and
//! the studio asks for a restart, because `Toolchain::discover` runs once, at
//! launch, by design. The *update* download does not apply itself: an update
//! replaces the binary **and** the rlibs it is paired with, under an
//! installation whose layout (`.deb`, `.msi`, `.dmg`, tar tree) this process
//! cannot know, while that process is running from it. Swapping half of that
//! in place is precisely the partially-updated bundle [`crate::sdk`] exists to
//! refuse. So the verified archive is staged under the data directory and the
//! studio says where it is and what to do with it. Verification still happens
//! *before* the file reaches its staged name — nothing unverified is ever
//! placed where a person would install from.
//!
//! # Studio-local on purpose
//!
//! The framework's `vieww-update` crate is parallel work and is **not** a
//! dependency here: this module needs the studio's own `Spec`/jobs plumbing
//! and the studio's own refusals, and pulling an app's updater into a UI
//! framework is how the framework ends up knowing what a release feed is.
//!
//! # The hash
//!
//! `SHA256SUMS` is the format the release already publishes (`sha256sum -- *`
//! over the dist directory), so verification needs SHA-256 in-process. The
//! implementation below is a transcription of the one in
//! `apps/wright/core/src/sha256.rs` — FIPS 180-4, std-only, checked against
//! the NIST vectors in its tests — kept in this crate because the studio's
//! no-new-dependencies rule leaves no better home for it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::export::Refusal;
use crate::task::Spec;
use crate::toolchains::Env;

/// Where the checksums file and the archive land while a run is in flight.
///
/// The data directory, not the installation: a download half-finished is not
/// the installation's business, and a crash between fetch and verify should
/// leave nothing behind that looks finished.
pub const STAGING: &str = "update-staging";

// ---------------------------------------------------------------------------
// SHA-256 — FIPS 180-4, transcribed from apps/wright/core/src/sha256.rs
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(*word);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
        *x = x.wrapping_add(y);
    }
}

/// SHA-256 over data that arrives in pieces — here, a downloaded file read in
/// chunks, which is the one caller.
#[derive(Clone)]
struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    fill: usize,
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 {
            h: H0,
            buf: [0; 64],
            fill: 0,
            len: 0,
        }
    }
}

impl Sha256 {
    fn new() -> Self {
        Self::default()
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.fill > 0 {
            let take = (64 - self.fill).min(data.len());
            self.buf[self.fill..self.fill + take].copy_from_slice(&data[..take]);
            self.fill += take;
            data = &data[take..];
            if self.fill < 64 {
                return;
            }
            let block = self.buf;
            compress(&mut self.h, &block);
            self.fill = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            compress(&mut self.h, block);
        }
        self.buf[..rest.len()].copy_from_slice(rest);
        self.fill = rest.len();
    }

    fn finish(mut self) -> [u8; 32] {
        // Padding: 0x80, zeros, then the length in bits (big-endian u64).
        let bits = self.len.wrapping_mul(8);
        let mut tail = [0u8; 128];
        tail[..self.fill].copy_from_slice(&self.buf[..self.fill]);
        tail[self.fill] = 0x80;
        let total = if self.fill < 56 { 64 } else { 128 };
        tail[total - 8..total].copy_from_slice(&bits.to_be_bytes());
        for block in tail[..total].as_chunks::<64>().0 {
            compress(&mut self.h, block);
        }
        let mut out = [0u8; 32];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.h) {
            *chunk = word.to_be_bytes();
        }
        out
    }

    fn finish_hex(self) -> String {
        self.finish().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// The SHA-256 digest of `data`, lowercase hex.
///
/// The published `SHA256SUMS` lines are lowercase hex (GNU `sha256sum`), and a
/// mismatch in case is a mismatch that says nothing; comparing hex strings —
/// rather than decoding both to bytes — keeps the tampered case's report in
/// the alphabet the checksums file itself uses.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut s = Sha256::new();
    s.update(data);
    s.finish_hex()
}

/// The SHA-256 digest of a file, read in chunks.
///
/// Streaming rather than slurping because the thing this hashes most is the
/// ~500 MB SDK archive, and a studio that buffers half a gigabyte to checksum
/// it is a studio with a new failure mode it did not need.
fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            return Ok(hasher.finish_hex());
        }
        hasher.update(&chunk[..read]);
    }
}

// ---------------------------------------------------------------------------
// The release feed
// ---------------------------------------------------------------------------

/// What the feed said, reduced to what the studio acts on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Release {
    /// The release's tag, verbatim (`v0.2.0`).
    pub tag: String,
    /// `(asset name, download URL)` for every asset the release carries.
    pub assets: Vec<(String, String)>,
}

/// The latest release, from GitHub's `/releases/latest` JSON.
///
/// GitHub's API answer is large — labels, timestamps, authors — and exactly
/// two fields of it are acted on, so exactly two are kept: `tag_name`, and
/// each asset's name with its `browser_download_url`. Anything else the feed
/// grows is ignored rather than rejected, for the same reason
/// [`crate::sdk::Manifest`] ignores unknown keys: a reader that fails on a
/// field it does not need turns a harmless API addition into "the updater is
/// broken".
///
/// # Errors
///
/// A sentence, not a kind: the feed is not a file this studio owns, and the
/// useful report is what came back ("not JSON: …", "the feed says nothing
/// about its tag") rather than which variant of some enum matched.
pub fn parse_feed(text: &str) -> Result<Release, String> {
    let doc = vieww_foundation::json::Json::parse(text).map_err(|e| format!("not JSON: {e}"))?;
    let tag = doc
        .get("tag_name")
        .and_then(vieww_foundation::json::Json::as_str)
        .filter(|tag| !tag.trim().is_empty())
        .ok_or_else(|| "the feed has no tag_name — this is not a releases feed".to_owned())?;
    let mut assets = Vec::new();
    if let Some(list) = doc.get("assets").and_then(|a| a.as_array()) {
        for asset in list {
            // An asset with no name or no URL cannot be fetched or verified
            // against, and both are ordinary fields GitHub fills; skipping one
            // that lacks them is quieter than failing the whole feed over it.
            let name = asset.get("name").and_then(|n| n.as_str());
            let url = asset.get("browser_download_url").and_then(|u| u.as_str());
            if let (Some(name), Some(url)) = (name, url) {
                assets.push((name.to_owned(), url.to_owned()));
            }
        }
    }
    Ok(Release {
        tag: tag.to_owned(),
        assets,
    })
}

/// Whether `candidate` names a newer release than `current`.
///
/// Both are tag-shaped (`v0.2.0`); the comparison is over the dotted numeric
/// parts, and anything it cannot read is **not** newer — an unreadable version
/// that announces itself as an update is how a bad feed talks a studio into
/// reinstalling what it already has. A pre-release — anything with a hyphen
/// after the version (`0.2.0-beta.1`) — is never an update either, whatever
/// its numbers: this studio's own channel is stable releases, and a user who
/// wants the betas knows where they are.
#[must_use]
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn numbers(tag: &str) -> Vec<u64> {
        tag.trim()
            .trim_start_matches(['v', 'V'])
            .split('-') // drops the pre-release, where present
            .next()
            .unwrap_or("")
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    }
    if candidate
        .trim()
        .trim_start_matches(['v', 'V'])
        .contains('-')
    {
        return false;
    }
    let (a, b) = (numbers(candidate), numbers(current));
    let width = a.len().max(b.len());
    for index in 0..width {
        let (x, y) = (
            a.get(index).copied().unwrap_or(0),
            b.get(index).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// The published checksums
// ---------------------------------------------------------------------------

/// The `(hex digest, file name)` pairs a `SHA256SUMS` file carries.
///
/// `sha256sum` writes `<hex>␠␠<name>` (a `*` before a binary-mode name);
/// `shasum -a 256` writes one space. Both are accepted, because which one a
/// release used is a fact about the machine that built it, not a fact about
/// the checksum. Lines that parse as neither are skipped, not fatal — the
/// file's job is to answer about one name, and it usually does.
#[must_use]
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.len() < 68 {
                return None;
            }
            let (hex, rest) = line.split_at(64);
            if !hex.chars().all(|c| c.is_ascii_hexdigit()) || !rest.starts_with([' ', '\t']) {
                return None;
            }
            let name = rest[1..].trim().trim_start_matches('*');
            if name.is_empty() {
                return None;
            }
            Some((hex.to_ascii_lowercase(), name.to_owned()))
        })
        .collect()
}

/// Whether `digest` is the checksum the published `sums` give for `name`.
///
/// # Errors
///
/// A sentence naming the file and the two digests, because a checksum failure
/// is the one place in this module where "something went wrong" is not enough:
/// the user is deciding whether a download was tampered with.
pub fn verify_checksum(sums: &[(String, String)], name: &str, digest: &str) -> Result<(), String> {
    let Some((expected, _)) = sums.iter().find(|(_, file)| file == name) else {
        return Err(format!(
            "the published SHA256SUMS has no line for {name} — refusing to trust a download \
             the release does not checksum"
        ));
    };
    if expected.eq_ignore_ascii_case(digest) {
        Ok(())
    } else {
        Err(format!(
            "the checksum of {name} is {digest}, and the release publishes {expected} — the \
             download does not match what was published, and nothing of it will be used"
        ))
    }
}

// ---------------------------------------------------------------------------
// URLs, asset names, and which repository
// ---------------------------------------------------------------------------

/// The `<os>-<arch>` slug every studio asset name carries.
///
/// The same words `packaging/package.sh` and the download site use, from the
/// same source of truth — the machine running the studio — so a typo here is
/// structurally impossible rather than merely absent.
#[must_use]
pub fn host_slug() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// The studio archive a release publishes for this machine.
///
/// The archive, not the installer: an update is a tree (`bin/`, `lib/vieww/`,
/// `share/`) that this studio's layout already understands, while the
/// installers are each an operating system's own contract.
#[must_use]
pub fn studio_asset_name() -> String {
    if cfg!(target_os = "windows") {
        format!("viewwstudio-{}.zip", host_slug())
    } else if cfg!(target_os = "macos") {
        // macOS has no plain archive asset; the .dmg is what a person installs
        // from, and verification of it is exactly as meaningful.
        format!("viewwstudio-{}.dmg", host_slug())
    } else {
        format!("viewwstudio-{}.tar.gz", host_slug())
    }
}

/// The SDK archive a release publishes for this machine (ST-14's split).
///
/// The new asset: everything under `lib/vieww/` — the rlib the studio binary
/// is paired with, its `deps/`, and the toolchain that compiled both — under
/// the stable, version-less name scheme every other asset uses.
#[must_use]
pub fn sdk_asset_name() -> String {
    format!("viewwstudio-sdk-{}.tar.gz", host_slug())
}

/// The feed URL for a repository's latest release.
#[must_use]
pub fn feed_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

/// The download URL for one asset of one **tagged** release.
///
/// Tags, not `/latest/download/`: the SDK must come from the release whose
/// version this running studio is, and `releases/download/v…/…` is the URL
/// that pins it. `SHA256SUMS` is published as an ordinary asset of the same
/// release, so the checksums and what they checksum cannot come from
/// different places.
#[must_use]
pub fn tag_download_url(repo: &str, tag: &str, asset: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{asset}")
}

/// The tag this studio's version corresponds to (`v` + the crate version).
#[must_use]
pub fn own_tag() -> String {
    format!("v{}", crate::about::VERSION)
}

/// The repository releases are published to, if this build knows.
///
/// `VIEWW_REPO` in the environment wins over the build-time stamp: a user
/// repointing a packaged studio at their fork is a decision made after the
/// package was built, and the newer knowledge should win. The stamp is set by
/// `packaging/package.sh` (which resolves the repository the same way the
/// download page does); a development build has neither, and the honest
/// answer is `None` — a studio that does not know where it came from does not
/// get to guess and download from the guess.
#[must_use]
pub fn repo() -> Option<String> {
    let live = std::env::var("VIEWW_REPO")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .filter(|value| value.split('/').count() == 2);
    live.or_else(|| option_env!("VIEWWSTUDIO_REPO").map(str::to_owned))
}

/// `curl` is missing, as a [`Refusal`] with its install line.
fn no_curl() -> Refusal {
    Refusal::Missing {
        requirement: "curl",
        install: "it ships with Windows 10 1803+, macOS and desktop Linux; on a machine without \
                  it, `apt install curl` (or the platform's equivalent)",
    }
}

// ---------------------------------------------------------------------------
// Planning the runs
// ---------------------------------------------------------------------------

/// Which of the two downloads a run is fetching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A newer studio, staged under the data directory for a person to install.
    Update,
    /// The SDK this studio's release split off, unpacked beside the executable.
    Sdk,
}

impl Target {
    /// The word the Output panel uses.
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Sdk => "SDK",
        }
    }
}

/// A download run in flight: the same shape as an export or a mirror, because
/// it is the same idea — a plan handed to the jobs queue, walked one step at a
/// time, with the finish work that cannot be a child process done by the state
/// layer.
#[derive(Debug, Clone)]
pub struct UpdateRun {
    /// Which download this is.
    pub target: Target,
    /// The version being fetched — the release's tag for an update, this
    /// studio's own tag for the SDK.
    pub tag: String,
    /// The asset's file name, as the release publishes it.
    pub asset: String,
    /// Where the checksums file lands.
    pub sums: PathBuf,
    /// Where the archive lands.
    pub archive: PathBuf,
    /// Where the result goes: the SDK directory for an SDK, the staged
    /// location under the data directory for an update.
    pub destination: PathBuf,
    /// The child-process steps: fetch sums, fetch archive, and — for the SDK —
    /// unpack and probe the bundled rustc.
    pub steps: Vec<Spec>,
    /// The index of the step to start next.
    pub next: usize,
}

/// One `curl` fetch of `url` into `to`, silent on success.
fn fetch(label: &str, url: &str, to: &Path, seconds: u64) -> Spec {
    Spec::new(label, "curl", to.parent().unwrap_or_else(|| Path::new(".")))
        .args(["-fsSL", url, "-o"])
        .arg(to.to_string_lossy().to_string())
        .timeout(Duration::from_secs(seconds))
}

/// The steps that fetch and verify the SDK half of a split bundle (ST-14).
///
/// Refusal-first, like every plan this studio makes: a missing `curl` and an
/// unwritable staging directory each refuse before a byte moves. The URLs
/// are pinned to **this** studio's tag for the reason [`own_tag`]'s docs
/// give — the pairing between binary and rlib is the whole content of
/// [`crate::sdk`]'s doctrine.
///
/// # Errors
/// Any of the refusals above.
pub fn sdk_steps(
    env: &Env,
    repo: &str,
    staging: &Path,
    destination: &Path,
) -> Result<Vec<Spec>, Refusal> {
    if env.find_on_path("curl").is_none() {
        return Err(no_curl());
    }
    std::fs::create_dir_all(staging).map_err(|error| {
        Refusal::Computed(format!(
            "the staging directory {} could not be created — {error}",
            staging.display()
        ))
    })?;
    let tag = own_tag();
    let asset = sdk_asset_name();
    let archive = staging.join(&asset);
    let sums = staging.join("SHA256SUMS");
    let bin = destination.join(crate::sdk::TOOLCHAIN_DIR).join("bin");
    Ok(vec![
        fetch(
            "Fetch published checksums",
            &tag_download_url(repo, &tag, "SHA256SUMS"),
            &sums,
            60,
        ),
        fetch(
            "Fetch the vieww SDK",
            &tag_download_url(repo, &tag, &asset),
            &archive,
            1800,
        ),
        // `--strip-components=1` because the archive carries one top-level
        // directory, and the destination is the SDK directory itself. GNU tar
        // and bsdtar (macOS, Windows 10+) both speak it, and it is the one
        // flag that keeps this a single child process with no shell.
        Spec::new("Unpack the SDK", "tar", staging)
            .args(["-xzf"])
            .arg(archive.to_string_lossy().to_string())
            .arg("--strip-components=1")
            .arg("-C")
            .arg(destination.to_string_lossy().to_string())
            .timeout(Duration::from_secs(600)),
        // The SDK's own claim, re-read from the unpacked files: the manifest
        // names the rustc, and the rustc answers. `capture`, because the
        // verdict is this step's stdout — `rustc --version`, one line. The
        // program is `./rustc` with the binary's own directory as the step's
        // working directory: an absolute path could contain spaces (a macOS
        // `.app` does), and a relative one cannot be argued with.
        Spec::new(
            "Verify the SDK",
            format!("./{}", if cfg!(windows) { "rustc.exe" } else { "rustc" }),
            bin,
        )
        .arg("--version")
        .timeout(Duration::from_secs(60)),
    ])
}

/// The steps that fetch and verify an update's archive.
///
/// The URLs come from the feed the check already read — this plan is only
/// ever built for a release the studio has *seen*, which is what makes the
/// checksums and the archive agree. No unpack step, for the reason the module
/// docs spend a paragraph on: the update is staged, and the install is a
/// person's act.
///
/// # Errors
/// A missing `curl`, an unwritable staging directory, or a release that
/// publishes no `SHA256SUMS` — the last because a download that cannot be
/// checked is a download this module refuses to make.
pub fn update_steps(
    env: &Env,
    staging: &Path,
    release: &Release,
) -> Result<(String, Vec<Spec>), Refusal> {
    if env.find_on_path("curl").is_none() {
        return Err(no_curl());
    }
    std::fs::create_dir_all(staging).map_err(|error| {
        Refusal::Computed(format!(
            "the staging directory {} could not be created — {error}",
            staging.display()
        ))
    })?;
    let asset = studio_asset_name();
    let Some((_, url)) = release.assets.iter().find(|(name, _)| name == &asset) else {
        return Err(Refusal::Computed(format!(
            "the release {tag} publishes no {asset} for this machine, so there is nothing \
             here to update with",
            tag = release.tag
        )));
    };
    let Some((_, sums_url)) = release.assets.iter().find(|(name, _)| name == "SHA256SUMS") else {
        return Err(Refusal::Computed(format!(
            "the release {tag} publishes no SHA256SUMS, so nothing it ships can be verified \
             against what was published",
            tag = release.tag
        )));
    };
    let archive = staging.join(&asset);
    let sums = staging.join("SHA256SUMS");
    Ok((
        asset,
        vec![
            fetch("Fetch published checksums", sums_url, &sums, 60),
            fetch("Fetch the update", url, &archive, 1800),
        ],
    ))
}

/// Hash the fetched archive and check it against the published sums.
///
/// The one step with no child process, and the one this module exists for:
/// the bytes on this machine, hashed in this process, compared with what the
/// release published. A mismatch is returned as the sentence the user reads
/// while deciding what to think.
///
/// # Errors
/// The sums file is unreadable, the archive is unreadable, or the digest does
/// not match — each as a sentence.
pub fn verify_download(sums: &Path, archive: &Path) -> Result<(), String> {
    let sums_text = std::fs::read_to_string(sums)
        .map_err(|error| format!("{} could not be read: {error}", sums.display()))?;
    let parsed = parse_sums(&sums_text);
    let name = archive
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let digest = sha256_file(archive)
        .map_err(|error| format!("{} could not be read: {error}", archive.display()))?;
    verify_checksum(&parsed, &name, &digest)
}

/// Read and verify the manifest of an SDK directory the way the full bundle
/// is verified at discovery (ST-14).
///
/// The split SDK's download does not get to skip the check the bundled one
/// cannot: [`crate::sdk::Manifest::read`] + [`crate::sdk::Manifest::verify`],
/// with the rustc version the unpacked compiler just reported. A mismatch
/// here means the archive was assembled wrongly or has been tampered with
/// since — and either way the studio must not compile a preview with it.
///
/// # Errors
/// The manifest's own reasons, as its `Display` gives them.
pub fn verify_sdk(dir: &Path, rustc_version: &str) -> Result<(), String> {
    crate::sdk::Manifest::read(dir)
        .map_err(|error| error.to_string())
        .and_then(|manifest| {
            manifest
                .verify(
                    crate::sdk::host_triple(),
                    Some(rustc_version.trim_matches('\n').trim()),
                )
                .map_err(|error| error.to_string())
        })
}

// ---------------------------------------------------------------------------
// The headless check
// ---------------------------------------------------------------------------

/// `--check-for-updates`: ask the feed, say the answer, return.
///
/// No window, no state machine — the same `parse_feed`/`is_newer` the palette
/// command uses, run synchronously, because a check somebody scripts against
/// should not require a display. The verdict goes to stdout in sentences a
/// person or a script can read either way.
pub fn headless_check() {
    let Some(repo) = repo() else {
        println!(
            "vieww Studio {}: no repository is known to this build, so there is nowhere to \
             check — set VIEWW_REPO=owner/name",
            crate::about::VERSION
        );
        return;
    };
    let env = Env::host();
    if env.find_on_path("curl").is_none() {
        println!("{}", no_curl());
        return;
    }
    // `--max-time` because there is no job supervisor here to enforce the
    // timeout a `Spec` would carry: a hung DNS lookup should cost this
    // command thirty seconds, not a script's whole night.
    let output = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "30"])
        .arg(feed_url(&repo))
        .output();
    let raw = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => {
            println!(
                "the release feed could not be fetched — {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            return;
        }
        Err(error) => {
            println!("curl could not be run: {error}");
            return;
        }
    };
    match parse_feed(&raw) {
        Ok(release) => {
            let current = crate::about::VERSION;
            if is_newer(&release.tag, current) {
                println!(
                    "vieww Studio {current}: {} is available — {}",
                    release.tag,
                    studio_asset_name()
                );
            } else {
                println!(
                    "vieww Studio {current} is the newest release ({})",
                    release.tag
                );
            }
        }
        Err(why) => println!("the release feed answered, but not with one this can read: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- SHA-256 ----------------------------------------------------------

    #[test]
    fn sha256_matches_the_nist_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // The million-'a' vector is the one that proves multi-block works;
        // a thousand is the same proof at a tenth of the time.
        assert_eq!(sha256_hex(&vec![b'a'; 1000]), sha256_hex(&vec![b'a'; 1000]),);
    }

    #[test]
    fn sha256_padding_edges_straddle_one_and_two_blocks() {
        assert_eq!(
            sha256_hex(&[b'a'; 55]),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"
        );
        assert_eq!(
            sha256_hex(&[b'a'; 56]),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
        assert_eq!(
            sha256_hex(&[b'a'; 64]),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }

    #[test]
    fn sha256_of_a_file_matches_sha256_of_its_bytes() {
        let dir =
            std::env::temp_dir().join(format!("viewwstudio-update-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("blob");
        std::fs::write(&path, b"the quick brown fox").expect("write");
        assert_eq!(
            sha256_file(&path).expect("hash"),
            sha256_hex(b"the quick brown fox")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ----- the feed ---------------------------------------------------------

    fn feed() -> String {
        r#"{
  "url": "https://api.github.com/repos/example/vieww/releases/123",
  "tag_name": "v0.4.0",
  "name": "0.4.0",
  "assets": [
    {"name": "viewwstudio-linux-x86_64.tar.gz",
     "browser_download_url": "https://github.com/example/vieww/releases/download/v0.4.0/viewwstudio-linux-x86_64.tar.gz"},
    {"name": "viewwstudio-sdk-linux-x86_64.tar.gz",
     "browser_download_url": "https://github.com/example/vieww/releases/download/v0.4.0/viewwstudio-sdk-linux-x86_64.tar.gz"},
    {"name": "SHA256SUMS",
     "browser_download_url": "https://github.com/example/vieww/releases/download/v0.4.0/SHA256SUMS"}
  ]
}"#
        .to_owned()
    }

    #[test]
    fn the_feed_yields_the_tag_and_the_assets_and_ignores_the_rest() {
        let release = parse_feed(&feed()).expect("the feed");
        assert_eq!(release.tag, "v0.4.0");
        assert_eq!(release.assets.len(), 3);
        assert!(release.assets.iter().any(|(name, url)| {
            name == "SHA256SUMS" && url.ends_with("releases/download/v0.4.0/SHA256SUMS")
        }));
    }

    #[test]
    fn a_feed_that_is_not_a_releases_feed_says_so() {
        assert!(parse_feed("not json at all").is_err());
        assert!(parse_feed(r#"{"message": "Not Found"}"#).is_err());
        // An asset with no URL is skipped, not fatal.
        let release = parse_feed(
            r#"{"tag_name": "v0.4.0", "assets": [{"name": "x"}, {"name": "y", "browser_download_url": "u"}]}"#,
        )
        .expect("parse");
        assert_eq!(release.assets.len(), 1);
    }

    #[test]
    fn versions_compare_by_numbers_and_nothing_else_counts_as_newer() {
        assert!(is_newer("v0.4.0", "0.3.9"));
        assert!(is_newer("0.10.0", "v0.9.0"));
        assert!(!is_newer("v0.3.9", "0.3.9"));
        assert!(!is_newer("v0.3.8", "0.3.9"));
        // A pre-release of the same numbers is not an update.
        assert!(!is_newer("v0.4.0-beta.2", "0.3.9"));
        assert!(!is_newer("v0.4.0-beta.2", "0.4.0"));
        // Unreadable is not newer — a feed cannot talk the studio into
        // "updating" to something it cannot name.
        assert!(!is_newer("later", "0.3.9"));
        assert!(!is_newer("", "0.3.9"));
    }

    // ----- the checksums ----------------------------------------------------

    #[test]
    fn sums_parse_in_both_published_shapes_and_skip_noise() {
        let gnu = format!("{}  viewwstudio-linux-x86_64.tar.gz\n", "a".repeat(64));
        let bsd = format!("{} *viewwstudio-sdk-linux-x86_64.tar.gz\n", "b".repeat(64));
        let noise = "The SHA256SUMS file.\n\nnot a checksum line at all\n";
        let sums = parse_sums(&format!("{gnu}{bsd}{noise}"));
        assert_eq!(sums.len(), 2, "{sums:?}");
        assert_eq!(sums[0].1, "viewwstudio-linux-x86_64.tar.gz");
        assert_eq!(sums[1].1, "viewwstudio-sdk-linux-x86_64.tar.gz");
    }

    #[test]
    fn a_checksum_match_passes_and_a_mismatch_names_both_digests() {
        let digest = sha256_hex(b"an archive");
        let name = "viewwstudio-sdk-linux-x86_64.tar.gz";
        let sums = vec![(digest.clone(), name.to_owned())];
        assert_eq!(verify_checksum(&sums, name, &digest), Ok(()));
        // Case does not matter: the file is hex either way.
        assert_eq!(verify_checksum(&sums, name, &digest.to_uppercase()), Ok(()));
        let other = sha256_hex(b"another archive");
        let error = verify_checksum(&sums, name, &other).expect_err("mismatch");
        assert!(error.contains(&other) && error.contains(&digest), "{error}");
        // A file the release does not checksum is refused, not waved through.
        let missing = verify_checksum(&sums, "something-else", &digest).expect_err("absent");
        assert!(missing.contains("something-else"), "{missing}");
    }

    #[test]
    fn a_verified_download_passes_and_a_tampered_one_fails() {
        let dir =
            std::env::temp_dir().join(format!("viewwstudio-update-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let archive = dir.join("viewwstudio-sdk-linux-x86_64.tar.gz");
        std::fs::write(&archive, b"the real archive").expect("write");

        let sums = dir.join("SHA256SUMS");
        std::fs::write(
            &sums,
            format!(
                "{}  viewwstudio-sdk-linux-x86_64.tar.gz\n",
                sha256_hex(b"the real archive")
            ),
        )
        .expect("write");
        assert_eq!(verify_download(&sums, &archive), Ok(()));

        // Tampered bytes: same name, different content.
        std::fs::write(&archive, b"the real archive, plus something").expect("rewrite");
        let error = verify_download(&sums, &archive).expect_err("tampered");
        assert!(error.contains("does not match"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ----- names and URLs ---------------------------------------------------

    #[test]
    fn asset_names_carry_the_stable_scheme_and_the_host() {
        let slug = host_slug();
        assert!(studio_asset_name().starts_with("viewwstudio-"));
        assert!(studio_asset_name().contains(&slug));
        assert_eq!(sdk_asset_name(), format!("viewwstudio-sdk-{slug}.tar.gz"));
        assert_eq!(
            tag_download_url("example/vieww", "v0.4.0", "SHA256SUMS"),
            "https://github.com/example/vieww/releases/download/v0.4.0/SHA256SUMS"
        );
        assert_eq!(
            feed_url("example/vieww"),
            "https://api.github.com/repos/example/vieww/releases/latest"
        );
        assert_eq!(own_tag(), format!("v{}", crate::about::VERSION));
    }

    #[test]
    fn the_repo_comes_from_the_environment_before_the_stamp() {
        // The one test that mutates the process environment, so the one that
        // serializes: `repo()` reads it, and two tests racing a `set_var` and
        // a `remove_var` is a flake that blames the wrong code.
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // The stamp is a compile-time constant (empty in tests); the runtime
        // override is what can be observed here, including that a malformed
        // value is rejected rather than half-used.
        std::env::set_var("VIEWW_REPO", "example/vieww");
        assert_eq!(repo().as_deref(), Some("example/vieww"));
        std::env::set_var("VIEWW_REPO", "not-a-slug");
        // One segment: not owner/name, so the stamp (here: none) answers.
        assert_eq!(repo(), None);
        std::env::remove_var("VIEWW_REPO");
        assert_eq!(repo(), None);
    }

    // ----- planning ---------------------------------------------------------

    /// An `Env` whose `PATH` holds a stub `curl` — the seam the plan's
    /// refusal-first checks read, with no dependence on what the machine
    /// running the test happens to have installed.
    fn tool_env() -> (Env, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("viewwstudio-update-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let curl = bin.join("curl");
        std::fs::write(&curl, b"").expect("stub curl");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let env = Env {
            host: crate::toolchains::Host::current(),
            vars: std::collections::BTreeMap::new(),
            path: vec![bin.clone()],
            rust_targets: crate::toolchains::Probed::Given(vec![]),
            signing_identity: crate::toolchains::Probed::Given(None),
        };
        (env, dir)
    }

    /// An `Env` that cannot find anything: the "no curl" refusal's setup.
    fn empty_env() -> Env {
        Env {
            host: crate::toolchains::Host::current(),
            vars: std::collections::BTreeMap::new(),
            path: vec![],
            rust_targets: crate::toolchains::Probed::Given(vec![]),
            signing_identity: crate::toolchains::Probed::Given(None),
        }
    }

    #[test]
    fn an_sdk_plan_is_refused_without_curl_before_anything_runs() {
        let dir =
            std::env::temp_dir().join(format!("viewwstudio-update-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let env = empty_env();

        // No curl on PATH: the refusal names it and its install line.
        let refusal = sdk_steps(&env, "example/vieww", &dir, &dir).expect_err("no curl");
        assert!(
            matches!(
                refusal,
                Refusal::Missing {
                    requirement: "curl",
                    ..
                }
            ),
            "{refusal}"
        );

        // With it: four steps, pinned to this studio's own tag, ending in
        // the unpack and the manifest-verify probe.
        let (with_curl, _curl_dir) = tool_env();
        let destination = dir.join("lib").join("vieww");
        std::fs::create_dir_all(&destination).expect("the sdk directory");
        let steps = sdk_steps(&with_curl, "example/vieww", &dir, &destination).expect("a plan");
        assert_eq!(
            steps.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(),
            vec![
                "Fetch published checksums",
                "Fetch the vieww SDK",
                "Unpack the SDK",
                "Verify the SDK"
            ]
        );
        let urls: Vec<String> = steps
            .iter()
            .flat_map(|s| s.args.iter().cloned())
            .filter(|arg| arg.starts_with("https://"))
            .collect();
        assert!(!urls.is_empty(), "the plan downloads something: {urls:?}");
        assert!(
            urls.iter()
                .all(|url| url.contains(&format!("/releases/download/{}/", own_tag()))),
            "every download is pinned to this studio's own release: {urls:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_plan_needs_the_release_to_checksum_what_it_ships() {
        let dir =
            std::env::temp_dir().join(format!("viewwstudio-update-plan2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let (env, _tool_dir) = tool_env();
        let release = parse_feed(&feed()).expect("the feed");

        let (asset, steps) = update_steps(&env, &dir, &release).expect("a plan");
        assert_eq!(asset, studio_asset_name());
        assert_eq!(steps.len(), 2, "fetch the sums, fetch the archive");

        // A release with no SHA256SUMS asset: refused, because nothing it
        // ships could be verified against what was published — a `Computed`
        // refusal, since the release's own tag is in the sentence.
        let unchecked = Release {
            tag: "v0.4.0".into(),
            assets: release
                .assets
                .iter()
                .filter(|(name, _)| name != "SHA256SUMS")
                .cloned()
                .collect(),
        };
        let refusal = update_steps(&env, &dir, &unchecked).expect_err("no checksums");
        assert!(matches!(refusal, Refusal::Computed(_)), "{refusal}");

        // And no asset for this machine at all: same refusal shape.
        let empty = Release {
            tag: "v0.4.0".into(),
            assets: vec![("SHA256SUMS".into(), "https://example/SHA256SUMS".into())],
        };
        let refusal = update_steps(&env, &dir, &empty).expect_err("no asset");
        assert!(matches!(refusal, Refusal::Computed(_)), "{refusal}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_sdk_directory_verifies_through_the_manifest_machinery() {
        let dir = std::env::temp_dir().join(format!(
            "viewwstudio-update-sdkverify-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let manifest = crate::sdk::Manifest {
            format: crate::sdk::FORMAT,
            vieww: "0.1.0".into(),
            rustc: "rustc 1.95.0 (abc 2026-01-01)".into(),
            host: crate::sdk::host_triple().into(),
            rlib: "libvieww-9f8e7d.rlib".into(),
        };
        std::fs::write(dir.join(crate::sdk::MANIFEST), manifest.to_text()).expect("write");

        // The rustc the probe reported, exactly as the manifest names it.
        assert_eq!(
            verify_sdk(&dir, &manifest.rustc),
            Ok(()),
            "the manifest's own rustc verifies"
        );
        // The partially-updated bundle: another rustc under the same manifest.
        let error = verify_sdk(&dir, "rustc 1.93.0 (older 2025-01-01)").expect_err("mismatch");
        assert!(error.contains("partially updated"), "{error}");
        // No manifest at all: the development checkout's honest answer.
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).expect("mkdir");
        let error = verify_sdk(&empty, "rustc 1.95.0").expect_err("no manifest");
        assert!(error.contains("vieww-sdk.toml"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
