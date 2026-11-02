//! Fifty reload cycles in one process, with memory held to a bound.
//!
//! # Why this file exists
//!
//! The end-to-end test proves one reload and one restart. A session at a
//! desk is not one reload: it is an afternoon, and the crate's own module
//! docs say what that costs — **every reload leaks a library, on purpose**
//! (see `vieww_reload::Guest`), because closing one would dangle every
//! vtable pointer in the surviving tree. That is a designed, bounded cost
//! per reload. What nothing checked is that fifty reloads cost *fifty times
//! the designed cost and no more* — that the loop does not also leak
//! something accidental per cycle: a staging copy read into memory, a
//! per-reload allocation the host never frees, a watcher that grows.
//!
//! So this builds the real guest, reloads it fifty times, and asserts the
//! process's resident set grew by no more than a bound that the deliberate
//! leak explains. The bound is stated in the test, with the arithmetic that
//! justifies it, rather than chosen as "whatever it measured".
//!
//! # It is slow, and it is `#[ignore]`d for it
//!
//! One cold build of the guest (same cost as one end-to-end test) plus
//! fifty rebuilds of the guest crate alone. Run it the same way:
//!
//! ```console
//! cargo test -p vieww-reload --test soak -- --ignored --test-threads=1
//! ```
//!
//! `--test-threads=1` for the same reason `end_to_end.rs` needs it: both
//! shell out to cargo and would race for the build lock. This test uses a
//! scratch target directory of its own, so it never touches the workspace's
//! `target/` either.
//!
//! # What the RSS bound means, and what it does not
//!
//! This is not "hot reload leaks nothing" — it leaks on purpose, and the
//! assertion would be false if it said so. It is "the leak is the library
//! and nothing else": fifty cycles may cost fifty libraries' worth of
//! resident pages, and not a multiple of that. A regression that leaked,
//! say, the staged copy's bytes as well would double the slope and fail
//! here long before it failed anyone's afternoon.

use std::path::{Path, PathBuf};
use std::process::Command;

use vieww_reload::{Reloaded, Reloader};

/// How many rebuild-and-reload cycles the soak runs.
///
/// Fifty, because a number a person would actually reach in a session is
/// the point — ten would prove the loop and not the afternoon.
const CYCLES: usize = 50;

/// What fifty reloads may add to the process's resident set, in bytes.
///
/// The arithmetic, so a reader can check it rather than trust it:
///
/// - Every reload maps a fresh copy of the guest library and never unmaps
///   it — that is the deliberate leak this crate documents on `Guest`.
///   Only the pages the loader touches become resident: the relocation
///   records, the initialisers, and the code the root builder actually ran.
///   Measured, that is single-digit megabytes per cycle on this build, so
///   fifty cycles is comfortably under 400 MB.
/// - Everything else — the tree from each reload, the state-type table,
///   the staged file copy — is freed or was never resident, and contributes
///   nothing that grows with the cycle count.
///
/// A run that finishes near this bound has a per-cycle cost the designed
/// leak does not explain, and the number to lower is this one *after* the
/// cause is found — not the test's tolerance for it.
const RSS_BOUND_BYTES: u64 = 400 * 1024 * 1024;

/// The workspace root, from this crate's manifest directory.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<name>/ is two below the root")
        .to_path_buf()
}

/// Build `reload-guest` into `target_dir`, and return the `cdylib`.
///
/// The same helper `end_to_end.rs` has, for the same reasons it gives: a
/// target directory of this test's own, and the inherited `CARGO` so a CI
/// image without a default toolchain resolves the same compiler. Duplicated
/// rather than shared through `tests/common/` because the two slow test
/// binaries are each self-explanatory that way, and a helper module shared
/// between them would be the one thing in this crate imported only by tests
/// that shell out to cargo.
fn build_guest(target_dir: &Path) -> PathBuf {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(workspace())
        .env("CARGO_TARGET_DIR", target_dir)
        .args(["build", "-p", "reload-guest"])
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "building the guest failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let library = target_dir
        .join("debug")
        .join(if cfg!(target_os = "windows") {
            "reload_guest.dll"
        } else if cfg!(target_os = "macos") {
            "libreload_guest.dylib"
        } else {
            "libreload_guest.so"
        });
    assert!(
        library.exists(),
        "cargo reported success but there is no cdylib at {}",
        library.display()
    );
    library
}

/// The guest's source, rewritten per cycle and restored however the test
/// ends. The same `\n` normalisation `end_to_end.rs` explains: a Windows
/// checkout can hold this file with `\r\n`, and the landmark below has to
/// be found either way. `original` keeps the bytes as found, so `Drop`
/// restores the checkout exactly.
struct GuestSource {
    path: PathBuf,
    original: String,
}

impl GuestSource {
    fn open() -> Self {
        let path = workspace()
            .join("examples")
            .join("reload-guest")
            .join("src")
            .join("lib.rs");
        let original = std::fs::read_to_string(&path).expect("the guest example is checked in");
        Self { path, original }
    }

    /// Write the guest with `BAND` set to `rgb(r, g, b)`.
    ///
    /// A colour constant is the cosmetic edit the example's own docs
    /// suggest — same state types, same sizes, so every cycle here is a
    /// `Reloaded::Root`: the outcome whose whole cost is the deliberate
    /// leak. A shape-changing edit would exercise `Restart` instead, and
    /// that is `end_to_end.rs`'s job, not the soak's.
    fn write_band(&self, rgb: (u8, u8, u8)) {
        let normalised = self.original.replace("\r\n", "\n");
        let landmark = "const BAND: Color = Color::rgb(186, 217, 212);";
        assert!(
            normalised.contains(landmark),
            "the guest example no longer contains `{landmark}` — this test \
             rewrites it to force rebuilds, and needs a landmark that is there"
        );
        let (r, g, b) = rgb;
        std::fs::write(
            &self.path,
            normalised.replace(
                landmark,
                &format!("const BAND: Color = Color::rgb({r}, {g}, {b});"),
            ),
        )
        .expect("writing the guest");
    }
}

impl Drop for GuestSource {
    fn drop(&mut self) {
        // The original bytes, not a normalised copy, so the working tree is
        // exactly as it was found even on a CRLF checkout.
        let _ = std::fs::write(&self.path, &self.original);
    }
}

/// The process's resident set, in bytes, or `None` off Linux.
///
/// `/proc/self/status` is read rather than any allocator counter, because
/// the question is what the *process* holds — the mapped library pages the
/// deliberate leak costs are invisible to a malloc tally and are exactly
/// what this test exists to bound. Linux-only, honestly: the other targets
/// this workspace builds for have no stable same-process answer, and a
/// test that guessed one would be a bound nobody could check.
fn resident_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    // "VmRSS:    1234 kB" — the kernel's own unit is kB (kibibytes).
    let kib = line
        .trim_start_matches("VmRSS:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(kib * 1024)
}

/// **Fifty reloads, one process, one bound.**
///
/// Every cycle rewrites the guest's colour constant — the edit a developer
/// actually makes all afternoon — rebuilds, and polls. All fifty must come
/// back `Reloaded::Root`: a soak that restarted the tree would be measuring
/// the wrong leak, because a restart drops the tree that anchors the old
/// libraries' pages. The generation counter must reach exactly fifty, which
/// is also the check that no cycle was silently missed.
#[test]
#[ignore = "builds the guest crate fifty-one times; run with --ignored"]
fn fifty_reloads_stay_inside_the_designed_leak() {
    let scratch = tempdir("reload-soak");
    let source = GuestSource::open();

    // The cold build, with the checked-in colour.
    let library = build_guest(&scratch);
    let mut reloader = Reloader::new(&library).expect("the first load");
    assert!(
        matches!(reloader.poll(), Reloaded::Nothing),
        "a guest nobody has touched must not report a reload"
    );

    // Baseline taken after the first load: the first library's resident
    // pages are the starting condition, not a per-cycle cost.
    let baseline = resident_bytes();

    let mut kept = 0usize;
    for cycle in 1..=CYCLES {
        // Two colours, alternating: every build differs from the last, so
        // cargo rebuilds and the watcher has something to see, and the state
        // types never move, so the verdict is always "keep the tree".
        let warm = cycle % 2 == 1;
        source.write_band(if warm {
            (240, 120, 60)
        } else {
            (120, 200, 240)
        });
        build_guest(&scratch);

        match reloader.poll() {
            Reloaded::Root(root) => {
                // The tree is dropped here as the host would drop the old
                // one on the swap; keeping all fifty would be this test
                // inventing a leak of its own.
                drop(root);
                kept += 1;
            }
            Reloaded::Restart { .. } => panic!(
                "cycle {cycle} restarted the tree — a colour constant does not \
                 move a state type, so the fingerprint is reporting a change \
                 that did not happen, and the soak is measuring the wrong leak"
            ),
            Reloaded::Failed(error) => panic!("cycle {cycle} failed to load: {error}"),
            Reloaded::Nothing => panic!(
                "cycle {cycle} was not seen — the watcher missed a rebuild, and \
                 the generation count below is about to say so"
            ),
        }
        assert_eq!(
            reloader.generation(),
            cycle as u32,
            "the generation must move exactly once per cycle"
        );

        if cycle % 10 == 0 {
            eprintln!(
                "soak: {cycle}/{CYCLES} reloads kept, resident {} MB",
                resident_bytes().map_or(0, |bytes| bytes / (1024 * 1024))
            );
        }
    }

    assert_eq!(kept, CYCLES, "every cycle has to have kept the tree");

    match (baseline, resident_bytes()) {
        (Some(before), Some(after)) => {
            let growth = after - before;
            eprintln!(
                "soak: {CYCLES} reloads grew the process by {} MB (bound {} MB)",
                growth / (1024 * 1024),
                RSS_BOUND_BYTES / (1024 * 1024)
            );
            assert!(
                growth <= RSS_BOUND_BYTES,
                "{CYCLES} reloads grew the process by {} MB, past the {} MB \
                 bound the deliberate library leak explains — something \
                 accidental is leaking per cycle too",
                growth / (1024 * 1024),
                RSS_BOUND_BYTES / (1024 * 1024)
            );
        }
        // Off Linux there is no `/proc/self/status`; the loop above still
        // ran, so the soak of the *loop* happened, and only the memory
        // assertion is absent rather than quietly faked with a number that
        // means nothing off this platform.
        _ => eprintln!("soak: no /proc/self/status on this platform; RSS bound not checked"),
    }
}

/// A scratch directory that is this test's alone, removed when the test
/// ends — the same contract `end_to_end.rs`'s `tempdir` documents.
fn tempdir(name: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("vieww-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    Scratch(dir)
}

/// Removes its directory on drop, including when a test panics.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
