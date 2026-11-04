//! ST-14: the update check and the two downloads, end to end, against a fake
//! release feed.
//!
//! # What this file proves, and how
//!
//! The *pure* half — the feed parser, the version comparison, the checksum
//! comparison, the plans, the manifest verify — is `update`'s own test suite's
//! subject. What only the state layer can prove is the loop: that
//! `check_for_updates` drives its `curl` through the real jobs queue (a real
//! spawned process, real stdout, a real exit status), that the release it finds
//! is the one *Download the Update* then fetches, and that the whole download —
//! sums, archive, unpack, probe — walks its steps one at a time and ends in the
//! in-process SHA-256 check that decides whether anything is staged.
//!
//! The `curl` here is a shell script that answers the three questions a
//! download asks of the network — the feed (to stdout), the checksums, the
//! archive (each with `-o`) — and fails loudly on any other URL, so a download
//! that grows a fourth request fails here rather than quietly fetching
//! something new. The archive the fake serves for the SDK is a real `tar.gz`
//! built by this test with the real `tar`, so the unpack step is the real one
//! too, and the bundled-rustc probe runs a real executable.
//!
//! **Run with `--test-threads=1`.** The fake goes on `PATH` and the repository
//! override goes in the environment, both of which are process-global; the
//! file's tests take a mutex around the mutation, and the argument serializes
//! what the mutex cannot (the rest of the process's environment, which the
//! worker threads' children inherit at fork time).
//!
//! No network exists on the machine this runs on, and none is needed: the
//! assertions are about the studio's side of the wire, which is the only side
//! it owns.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use vieww_element::Runtime;
use viewwstudio::buffer::Workspace;
use viewwstudio::compile::Toolchain;
use viewwstudio::sdk::Manifest;
use viewwstudio::state::Studio;
use viewwstudio::update::{host_slug, sdk_asset_name, studio_asset_name};

/// Serializes this file's tests around the `PATH` and `VIEWW_REPO` mutations.
static NET: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A project with a manifest and a library — the shape every studio test opens,
/// because the update commands are studio state, not free functions.
fn project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("vieww-update-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("the project tree");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n\n[workspace]\n",
    )
    .expect("the manifest");
    std::fs::write(root.join("src").join("lib.rs"), "// the app\n").expect("the library");
    root
}

/// A studio with that project open, pointed at directories the test owns: the
/// staging directory under the project root rather than the real data
/// directory, and an SDK destination that is not beside this test's own
/// executable. A test that downloaded into the machine's real data directory,
/// or into the tree the binary runs from, would be a test that poisons every
/// later run.
fn studio(root: &Path, sdk: Option<&Path>) -> Studio {
    let runtime = Runtime::new();
    let mut studio = Studio::with_workspace(&runtime, Workspace::open(root))
        .with_host(vieww_foundation::TargetPlatform::Linux);
    studio.update_dir = root.join("staging");
    studio.sdk_destination = sdk.map(Path::to_path_buf);
    studio
}

/// A script on disk with its executable bit set.
fn write_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write the fake tool");
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// The release feed, as GitHub's `/releases/latest` answers it: a tag far in
/// the future, and the three assets a real release carries for this machine —
/// the studio archive, the split SDK, and the checksums over both.
fn feed_file(dir: &Path) -> PathBuf {
    let path = dir.join("feed.json");
    let studio_asset = studio_asset_name();
    let sdk_asset = sdk_asset_name();
    std::fs::write(
        &path,
        format!(
            r#"{{
  "tag_name": "v9.9.0",
  "assets": [
    {{"name": "{studio_asset}",
     "browser_download_url": "https://github.com/example/vieww/releases/download/v9.9.0/{studio_asset}"}},
    {{"name": "{sdk_asset}",
     "browser_download_url": "https://github.com/example/vieww/releases/download/v9.9.0/{sdk_asset}"}},
    {{"name": "SHA256SUMS",
     "browser_download_url": "https://github.com/example/vieww/releases/download/v9.9.0/SHA256SUMS"}}
  ]
}}"#
        ),
    )
    .expect("write the feed");
    path
}

/// A `SHA256SUMS` fixture: one GNU-shaped line per name, over the bytes the
/// fake will serve. `tamper` swaps the digest for another one, which is the
/// whole difference between a release and an attack.
///
/// The digest is computed by the platform's own `sha256sum` — **not** by the
/// studio's transcription — so this file's verdicts are against an
/// independent implementation, and a studio hasher that drifted from FIPS
/// 180-4 fails here rather than agreeing with itself. macOS's `shasum -a 256`
/// is the fallback, and writes the same line shape.
fn sums_file(dir: &Path, name: &str, bytes: &[u8], tamper: bool) -> PathBuf {
    let path = dir.join("SHA256SUMS");
    let blob = if tamper {
        b"something else entirely" as &[u8]
    } else {
        bytes
    };
    let digest = published_digest(dir, blob);
    std::fs::write(&path, format!("{digest}  {name}\n")).expect("write the sums");
    path
}

/// The digest of `bytes` according to the machine's own checksum utility.
fn published_digest(dir: &Path, bytes: &[u8]) -> String {
    let blob = dir.join("blob");
    std::fs::write(&blob, bytes).expect("write the blob");
    let output = if let Ok(out) = Command::new("sha256sum").arg(&blob).output() {
        out
    } else {
        Command::new("shasum")
            .arg("-a")
            .arg("256")
            .arg(&blob)
            .output()
            .expect("sha256sum or shasum")
    };
    assert!(output.status.success(), "the checksum utility ran");
    let line = String::from_utf8_lossy(&output.stdout);
    line.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The bin directory holding a fake `curl` that knows the three answers.
///
/// The URLs are matched by shape, not by host: the feed by `/releases/latest`,
/// the checksums by name, the archive by its stable asset name — so a plan that
/// pointed anywhere else fails loudly instead of quietly succeeding against a
/// fixture that was never asked for.
fn fake_bin(dir: &Path, feed: &Path, sums: &Path, archive: &Path, asset: &str) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("the bin directory");
    let script = format!(
        r#"#!/usr/bin/env bash
# The three answers a studio download asks of the network, and nothing else —
# see the module doc. The feed goes to stdout (the check captures it); the
# other two honour -o the way curl does.
set -u
url=""
dest=""
prev=""
for a in "$@"; do
        case "$a" in
        https://*) url="$a" ;;
        -o) prev="o" ;;
        *) [ "$prev" = "o" ] && {{ dest="$a"; prev=""; }} ;;
        esac
done
case "$url" in
        *"/releases/latest")
                cat "{feed}"
                exit 0 ;;
        *"SHA256SUMS")
                [ -n "$dest" ] || {{ echo "no -o for $url" >&2; exit 1; }}
                cp "{sums}" "$dest"
                exit 0 ;;
        *"/{asset}")
                [ -n "$dest" ] || {{ echo "no -o for $url" >&2; exit 1; }}
                cp "{archive}" "$dest"
                exit 0 ;;
esac
echo "fake curl: unsupported: $url" >&2
exit 1
"#,
        feed = feed.display(),
        sums = sums.display(),
        archive = archive.display(),
        asset = asset,
    );
    write_executable(&bin.join("curl"), &script);
    bin
}

/// A real SDK archive, the way `packaging/package.sh --split-sdk` writes one:
/// one top-level directory holding the manifest, the rlib it names, and a
/// `toolchain/bin/rustc` that reports the manifest's own version. Built with
/// the real `tar`, because the download's unpack step is the real `tar` too.
fn sdk_archive(dir: &Path, version: &str) -> PathBuf {
    let rustc = format!("rustc {version}");
    let top = dir.join("sdk-src").join("viewwstudio-sdk-9.9.0");
    std::fs::create_dir_all(top.join(viewwstudio::sdk::TOOLCHAIN_DIR).join("bin"))
        .expect("the toolchain tree");
    let manifest = Manifest {
        format: viewwstudio::sdk::FORMAT,
        vieww: "9.9.0".into(),
        rustc: rustc.clone(),
        host: viewwstudio::sdk::host_triple().into(),
        rlib: "libvieww-9f8e7d.rlib".into(),
    };
    std::fs::write(top.join(viewwstudio::sdk::MANIFEST), manifest.to_text())
        .expect("write the manifest");
    std::fs::write(top.join("libvieww-9f8e7d.rlib"), b"not really an rlib")
        .expect("write the rlib");
    write_executable(
        &top.join(viewwstudio::sdk::TOOLCHAIN_DIR)
            .join("bin")
            .join("rustc"),
        &format!("#!/bin/sh\necho '{rustc}'\n"),
    );

    let archive = dir.join(sdk_asset_name());
    let status = Command::new("tar")
        .args(["-C", &*dir.join("sdk-src").to_string_lossy()])
        .arg("-czf")
        .arg(&archive)
        .arg("viewwstudio-sdk-9.9.0")
        .status()
        .expect("run tar");
    assert!(status.success(), "the fixture archive must build");
    archive
}

/// Poll the studio until `done` holds, and fail loudly if it never does.
fn pump(studio: &Studio, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "the download never finished — output so far:\n{}",
            studio.output.get().join("\n")
        );
        if !studio.poll_jobs() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The joined Output panel, for assertions and failure messages alike.
fn output(studio: &Studio) -> String {
    studio.output.get().join("\n")
}

/// The PATH-with-fake + repository setup every networked test needs, held for
/// as long as the run is forking children — the same rule `device_mirror`
/// learned the hard way.
struct Network {
    old_path: String,
    old_repo: Option<String>,
}

impl Network {
    fn install(bin: &Path) -> Self {
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{old_path}", bin.display()));
        let old_repo = std::env::var("VIEWW_REPO").ok();
        std::env::set_var("VIEWW_REPO", "example/vieww");
        Self { old_path, old_repo }
    }

    fn restore(self) {
        std::env::set_var("PATH", &self.old_path);
        match self.old_repo {
            Some(repo) => std::env::set_var("VIEWW_REPO", repo),
            None => std::env::remove_var("VIEWW_REPO"),
        }
    }
}

#[test]
fn a_check_finds_the_newer_release_and_the_download_verifies_and_stages_it() {
    let _guard = NET.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = project("end-to-end");
    let asset = studio_asset_name();
    let bytes = b"the new studio, as released";
    std::fs::write(root.join("archive"), bytes).expect("the archive fixture");
    let feed = feed_file(&root);
    let sums = sums_file(&root, &asset, bytes, false);
    let bin = fake_bin(&root, &feed, &sums, &root.join("archive"), &asset);

    let net = Network::install(&bin);
    let studio = studio(&root, None);

    // The check: one curl, the feed in its stdout, the verdict in the bar.
    studio.check_for_updates();
    pump(&studio, || {
        studio
            .jobs
            .borrow()
            .jobs()
            .iter()
            .all(|job| !job.is_cancellable())
    });
    let release = studio.update_release.get().expect("a release was found");
    assert_eq!(release.tag, "v9.9.0");
    assert!(
        output(&studio).contains("vieww Studio v9.9.0 is available; this is"),
        "the verdict names both versions:\n{}",
        output(&studio)
    );

    // The download: the sums, the archive, then the in-process check.
    studio.download_update();
    assert!(
        studio.update_run.borrow().is_some(),
        "the download started — instead the Output panel says:\n{}",
        output(&studio)
    );
    pump(&studio, || studio.update_run.borrow().is_none());
    net.restore();

    let logged = output(&studio);
    assert!(
        logged.contains("The update for v9.9.0 matches the published SHA256SUMS"),
        "the checksum verdict:\n{logged}"
    );
    // Staged, not applied: the archive sits in the staging directory the test
    // owns, exactly where the notice says, with the bytes that arrived.
    let staged = root.join("staging").join(&asset);
    assert_eq!(
        std::fs::read(&staged).expect("the staged archive"),
        bytes,
        "the verified archive is staged under the data directory"
    );
    let notice = studio.notice.get().unwrap_or_default();
    assert!(
        notice.contains("install it as this one was installed, then restart"),
        "the notice says what applying honestly is:\n{notice}"
    );
    assert!(
        notice.contains(staged.display().to_string().as_str()),
        "the notice names where the archive is:\n{notice}"
    );
}

#[test]
fn a_tampered_download_fails_verification_and_nothing_pretends_to_be_verified() {
    let _guard = NET.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = project("tampered");
    let asset = studio_asset_name();
    let bytes = b"the new studio, as released";
    std::fs::write(root.join("archive"), bytes).expect("the archive fixture");
    let feed = feed_file(&root);
    // The one-byte difference that is the whole attack: the sums name a digest
    // the archive does not have.
    let sums = sums_file(&root, &asset, bytes, true);
    let bin = fake_bin(&root, &feed, &sums, &root.join("archive"), &asset);

    let net = Network::install(&bin);
    let studio = studio(&root, None);
    studio.check_for_updates();
    pump(&studio, || {
        studio
            .jobs
            .borrow()
            .jobs()
            .iter()
            .all(|job| !job.is_cancellable())
    });
    studio.download_update();
    pump(&studio, || studio.update_run.borrow().is_none());
    net.restore();

    let logged = output(&studio);
    assert!(
        logged.contains("failed verification"),
        "the failure, in the studio's own words:\n{logged}"
    );
    assert!(
        logged.contains("the download does not match what was published"),
        "both digests, for the decision the user is making:\n{logged}"
    );
    let notice = studio.notice.get().unwrap_or_default();
    assert!(
        notice.contains(
            "the update's checksum did not match the published SHA256SUMS — nothing was installed"
        ),
        "the status bar says what did not happen:\n{notice}"
    );
    // The bytes are left where they landed, named as they arrived — for a
    // person to inspect, not moved anywhere they could be mistaken for safe.
    assert!(root.join("staging").join(&asset).is_file());
}

#[test]
fn the_sdk_download_unpacks_and_verifies_through_the_manifest() {
    let _guard = NET.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = project("sdk");
    let asset = sdk_asset_name();
    let archive = sdk_archive(&root, "1.98.1 (000000000 2026-01-01)");
    let bytes = std::fs::read(&archive).expect("the archive bytes");
    let feed = feed_file(&root);
    let sums = sums_file(&root, &asset, &bytes, false);
    let bin = fake_bin(&root, &feed, &sums, &archive, &asset);
    let destination = root.join("sdk");

    let net = Network::install(&bin);
    // A first launch: no working toolchain (the studio's own default in these
    // tests), and an SDK destination the test owns rather than the directory
    // beside this binary.
    let studio = studio(&root, Some(&destination));
    studio.download_sdk();
    assert!(
        studio.update_run.borrow().is_some(),
        "the SDK download started — instead the Output panel says:\n{}",
        output(&studio)
    );
    let logged = output(&studio);
    assert!(
        logged.contains("4 step(s)"),
        "the plan, before it ran:\n{logged}"
    );
    pump(&studio, || studio.update_run.borrow().is_none());
    net.restore();

    let logged = output(&studio);
    assert!(
        logged.contains("The SDK for v") && logged.contains("matches the published SHA256SUMS"),
        "the checksum verdict:\n{logged}"
    );
    assert!(
        logged.contains("The SDK is unpacked into") && logged.contains("restart the studio"),
        "the honest finish — unpacked, verified, restart:\n{logged}"
    );
    // The tree the restart will find: the manifest where `Manifest::read`
    // looks, and the compiler where `sdk::rustc_path` looks.
    assert!(destination.join(viewwstudio::sdk::MANIFEST).is_file());
    assert!(destination
        .join(viewwstudio::sdk::TOOLCHAIN_DIR)
        .join("bin")
        .join("rustc")
        .is_file());
    let notice = studio.notice.get().unwrap_or_default();
    assert!(
        notice.contains("the vieww SDK is installed and verified"),
        "the status bar's version of the finish:\n{notice}"
    );
}

#[test]
fn the_downloads_refuse_before_anything_runs() {
    let _guard = NET.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = project("refused");

    // No PATH mutation, no fake tools, no repository: every refusal here is
    // decided before anything is spawned, which is the point of refusal-first.
    std::env::remove_var("VIEWW_REPO");
    let studio = studio(&root, Some(&root.join("sdk")));

    // A build that does not know its repository does not get to guess one.
    studio.check_for_updates();
    let logged = output(&studio);
    assert!(
        logged.contains("does not know which repository it was published to"),
        "the unknown repository, named:\n{logged}"
    );

    // An update without a check first: there is nothing to fetch.
    std::env::set_var("VIEWW_REPO", "example/vieww");
    studio.download_update();
    let logged = output(&studio);
    assert!(
        logged.contains("no release has been found yet — Check for Updates first"),
        "the missing check, named:\n{logged}"
    );

    // And an SDK download over a toolchain that already works: the download
    // has nothing to add, and says so instead of running.
    let studio = studio.with_toolchain(
        Ok(Toolchain {
            rustc: root.join("rustc"),
            vieww_rlib: root.join("libvieww-fake.rlib"),
            candidates: vec![],
            deps: root.join("deps"),
            sysroot_lib: None,
            version: "rustc 1.98.1".into(),
        }),
        None,
    );
    studio.download_sdk();
    let logged = output(&studio);
    assert!(
        logged.contains("the preview toolchain already works"),
        "the working toolchain, named:\n{logged}"
    );
    assert!(
        studio.update_run.borrow().is_none() && studio.update_release.get().is_none(),
        "nothing was started by any of the refusals"
    );
    std::env::remove_var("VIEWW_REPO");
}

#[test]
fn the_names_a_release_actually_publishes_are_the_names_the_updater_asks_for() {
    // What a script writes against `--check-for-updates` is the URL shapes and
    // the asset names: the feed is `/releases/latest`, the checksums and the
    // SDK are pinned to a **tag**, and the assets carry the stable, version-
    // less scheme `package.sh` writes — including the split SDK this round
    // added. A name that drifted between the packer and the asker is a download
    // that 404s after a check that succeeded, so the two are pinned together
    // here, against the one source of truth both sides share: this machine's
    // own `os-arch` slug.
    let slug = host_slug();
    assert_eq!(
        viewwstudio::update::feed_url("example/vieww"),
        "https://api.github.com/repos/example/vieww/releases/latest"
    );
    assert_eq!(
        viewwstudio::update::tag_download_url("example/vieww", "v9.9.0", "SHA256SUMS"),
        "https://github.com/example/vieww/releases/download/v9.9.0/SHA256SUMS"
    );
    assert!(studio_asset_name().starts_with("viewwstudio-"));
    assert!(studio_asset_name().contains(&slug));
    assert_eq!(sdk_asset_name(), format!("viewwstudio-sdk-{slug}.tar.gz"));
}
