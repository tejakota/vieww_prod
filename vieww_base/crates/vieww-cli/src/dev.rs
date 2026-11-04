//! `vieww dev` — the one-terminal hot-reload loop.
//!
//! # What this removes
//!
//! `vieww-reload`'s own module doc used to open with the two-terminal dance:
//! `cargo watch -w app/src -x 'build -p app'` in one terminal, the host in
//! another. The host half was always finished — it already polls the built
//! library, stages a copy, and loads it between frames. What was missing was
//! the *rebuild* half living somewhere a developer would actually find it.
//! This module is that half: watch the sources, rebuild the guest, keep the
//! host running, all from one command.
//!
//! ```text
//! vieww dev --guest reload-guest --host reload-host
//! ```
//!
//! # Why polling, and why the cadence is what it is
//!
//! The watcher is a `std::fs` modification-time sweep — the same answer
//! `vieww-reload`'s `watch.rs` gives for the library itself, and for the same
//! reasons: it runs a few times a second in a development loop, a filesystem
//! watcher crate would be a new dependency, and the honest statement is that a
//! save is noticed **within one interval, not instantly**. The default
//! interval is 250 ms, matching the cadence `Reloader::wake_on_change` uses on
//! the host side, so the two halves of the loop agree about how responsive a
//! reload promises to be. An inotify/kqueue backend would notice saves in
//! microseconds; this notices them in at most `--interval` milliseconds plus
//! one sweep, and costs nothing the workspace does not already depend on.
//!
//! A sweep walks the project directory and stats every `.rs` file and every
//! `Cargo.toml`, skipping `target/`, `.git/`, and hidden directories. On this
//! repository — a few thousand files — a sweep is single-digit milliseconds,
//! which is why the cadence is stated rather than hidden.
//!
//! # What is deliberately *not* done here
//!
//! - No output is captured or reinterpreted. Cargo's own compiler errors go
//!   to the terminal untouched (`Command::status`, not `::output()`), because
//!   a wrapper that re-prints what cargo already printed is a second place
//!   for the two to disagree — the same rule `cmd_cargo` states.
//! - A failed rebuild is reported and the loop carries on. The host is still
//!   running the last good library, which is the entire value of hot reload:
//!   the developer whose edit does not compile is precisely the one mid-edit.
//! - The host is not restarted when it exits. When it does — a crash, a
//!   `Ctrl-C` — `vieww dev` exits with the host's own exit code, because a
//!   tool that silently restarted a window the developer just closed is a
//!   tool that cannot be stopped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, SystemTime};

/// The polling source watcher behind `vieww dev`.
///
/// Holds one `mtime` per tracked file, which is the whole state: cheap to
/// poll, no thread of its own, and the caller owns the cadence — the same
/// shape as `vieww_reload::Watch`, for the same reasons.
#[derive(Debug)]
struct SourceWatch {
    root: PathBuf,
    seen: BTreeMap<PathBuf, Option<SystemTime>>,
}

impl SourceWatch {
    /// Watch `root` recursively, taking what is there now as the baseline.
    ///
    /// The first [`poll`](Self::poll) after construction is therefore not a
    /// rebuild: the files that exist at start-up are the ones the first build
    /// was built from.
    #[must_use]
    fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let seen = sweep(&root);
        Self { root, seen }
    }

    /// Look once. `true` when any tracked file was added, removed, or
    /// rewritten since the last look.
    ///
    /// Like `vieww_reload::Watch::poll`, a change is a timestamp that
    /// *differs* rather than one that is newer: a `git stash` or branch
    /// switch restores older files and is still an edit the developer wants
    /// rebuilt.
    fn poll(&mut self) -> bool {
        let now = sweep(&self.root);
        // Map equality covers all three of a rewrite, an addition and a
        // removal, which is why the value is an `Option<SystemTime>` rather
        // than `SystemTime`: a file with no readable mtime is still a file.
        let changed = now != self.seen;
        self.seen = now;
        changed
    }
}

/// Collect the mtime of every watchable file under `root`.
///
/// Watchable is `.rs` and `Cargo.toml`; skipped is `target/`, `.git/`, and
/// any hidden entry. `target/` must be skipped or every build would look
/// like an edit — the artifacts a rebuild writes are the majority of files
/// under a project directory, and they all change on every build.
fn sweep(root: &Path) -> BTreeMap<PathBuf, Option<SystemTime>> {
    let mut seen = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // An unreadable directory is ordinary mid-edit (a temp dir being
            // rotated) rather than an error worth stopping a dev loop for.
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "target" {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                stack.push(path);
            } else if name.ends_with(".rs") || name == "Cargo.toml" {
                seen.insert(path, metadata.modified().ok());
            }
        }
    }
    seen
}

/// `vieww dev --guest <pkg> [--host <pkg>] [dir]`.
///
/// Builds the guest once, starts the host if one was named, and then loops:
/// sleep one interval, sweep the sources, rebuild the guest when they moved.
/// Exits when the host exits (with its exit code), or immediately when the
/// *first* build fails — there is nothing to load yet, and pretending
/// otherwise would print a loop of errors where one honest failure belongs.
pub(super) fn run_dev(guest: &str, host: Option<&str>, dir: &Path, interval: Duration) -> ExitCode {
    println!(
        "vieww dev: watching {} every {}ms (a polling sweep, not inotify — a save is noticed within one interval)",
        dir.display(),
        interval.as_millis()
    );

    // The first build is fatal, unlike every one after it: with no library on
    // disk there is no host to run and no previous build to keep.
    if !build(guest, dir) {
        return ExitCode::FAILURE;
    }

    // The host runs as a child with inherited stdio, so its window and its
    // own prints behave exactly as if the developer had run `cargo run -p`
    // themselves — including receiving `Ctrl-C` from the same terminal.
    let mut child = match host {
        Some(host) => match Command::new("cargo")
            .args(["run", "-p", host])
            .current_dir(dir)
            .stdin(Stdio::null())
            .spawn()
        {
            Ok(child) => Some(child),
            Err(error) => {
                eprintln!("vieww dev: could not run the host: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    let mut watch = SourceWatch::new(dir);
    loop {
        std::thread::sleep(interval);

        // The host exiting ends the loop with its own exit code; losing the
        // child (it was killed out from under us) is reported once and the
        // loop degrades to the rebuild-only half, which is still useful.
        if let Some(spawned) = child.as_mut() {
            match spawned.try_wait() {
                Ok(Some(status)) => return super::exit_code_of(Ok(status)),
                Ok(None) => {}
                Err(_) => {
                    eprintln!("vieww dev: lost the host process; still rebuilding");
                    child = None;
                }
            }
        }

        if watch.poll() {
            let started = std::time::Instant::now();
            if build(guest, dir) {
                println!(
                    "vieww dev: rebuilt {guest} in {:.1}s",
                    started.elapsed().as_secs_f64()
                );
            } else {
                // Cargo already printed the error. What this adds is the one
                // fact a developer needs: the running build is untouched.
                eprintln!("vieww dev: build failed — the host keeps the running build");
            }
        }
    }
}

/// One `cargo build -p <guest>`, with cargo's own output passed through.
fn build(guest: &str, dir: &Path) -> bool {
    Command::new("cargo")
        .args(["build", "-p", guest])
        .current_dir(dir)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A scratch project that cleans up after itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("vieww-cli-dev-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("src")).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_first_poll_after_start_up_is_not_a_rebuild() {
        let project = Scratch::new("baseline");
        project.write("src/main.rs", "fn main() {}");
        let mut watch = SourceWatch::new(&project.0);
        assert!(
            !watch.poll(),
            "the files that are there built the first build"
        );
    }

    #[test]
    fn a_rewritten_source_is_seen_once_and_then_settles() {
        let project = Scratch::new("rewrite");
        project.write("src/main.rs", "v1");
        let mut watch = SourceWatch::new(&project.0);

        // A different tick, because filesystem timestamps are coarse on some
        // platforms and this must test the comparison rather than the clock.
        std::thread::sleep(Duration::from_millis(20));
        project.write("src/main.rs", "v2");

        assert!(watch.poll(), "the edit is the whole point");
        assert!(!watch.poll(), "and it is reported once, not forever after");
    }

    #[test]
    fn a_deeply_nested_source_is_watched() {
        let project = Scratch::new("nested");
        project.write("src/main.rs", "fn main() {}");
        let mut watch = SourceWatch::new(&project.0);

        std::thread::sleep(Duration::from_millis(20));
        project.write("src/screens/settings.rs", "pub const EDIT: u32 = 1;");

        assert!(watch.poll(), "a real edit is rarely in the top-level src");
    }

    #[test]
    fn a_new_file_and_a_deleted_file_are_both_changes() {
        let project = Scratch::new("add-remove");
        project.write("src/main.rs", "fn main() {}");
        let mut watch = SourceWatch::new(&project.0);

        std::thread::sleep(Duration::from_millis(20));
        project.write("src/new.rs", "pub const A: u32 = 1;");
        assert!(watch.poll(), "adding a file is an edit");

        std::thread::sleep(Duration::from_millis(20));
        fs::remove_file(project.0.join("src/new.rs")).unwrap();
        assert!(watch.poll(), "so is removing one — the module list changed");
    }

    #[test]
    fn cargo_toml_is_watched_because_dependencies_are_edits() {
        let project = Scratch::new("manifest");
        project.write("src/main.rs", "fn main() {}");
        project.write("Cargo.toml", "[package]\nname = \"x\"\n");
        let mut watch = SourceWatch::new(&project.0);

        std::thread::sleep(Duration::from_millis(20));
        project.write("Cargo.toml", "[package]\nname = \"y\"\n");

        assert!(
            watch.poll(),
            "a dependency change is as much an edit as code"
        );
    }

    #[test]
    fn artifacts_in_target_do_not_look_like_edits() {
        // The one rule the sweep cannot get wrong: `target/` holds the
        // artifacts every rebuild rewrites, and treating those as edits would
        // turn one save into an infinite build loop.
        let project = Scratch::new("target-skip");
        project.write("src/main.rs", "fn main() {}");
        let mut watch = SourceWatch::new(&project.0);

        std::thread::sleep(Duration::from_millis(20));
        project.write("target/debug/libguest.so", "binary-ish bytes");
        project.write("target/debug/deps/thing.rmeta", "more bytes");

        assert!(
            !watch.poll(),
            "a build's own outputs must not retrigger the build"
        );
    }

    #[test]
    fn hidden_directories_are_skipped() {
        let project = Scratch::new("hidden-skip");
        project.write("src/main.rs", "fn main() {}");
        let mut watch = SourceWatch::new(&project.0);

        std::thread::sleep(Duration::from_millis(20));
        // `.git` and editor state (`.vscode`, `.idea`) churn constantly
        // without a single source file moving.
        project.write(".git/index", "git bookkeeping");
        project.write(".vscode/settings.json", "{}");

        assert!(!watch.poll(), "nothing a developer did not edit changed");
    }

    #[test]
    fn a_first_build_failure_is_fatal_rather_than_looped() {
        // An empty directory with no Cargo.toml makes `cargo build` fail
        // immediately, which proves the failure path exits rather than
        // entering the watch loop with nothing to load.
        let dir = std::env::temp_dir().join("vieww-cli-dev-fail");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let code = run_dev("no-such-guest", None, &dir, Duration::from_millis(10));
        fs::remove_dir_all(&dir).unwrap();
        assert_ne!(format!("{code:?}"), format!("{:?}", ExitCode::SUCCESS));
    }
}
