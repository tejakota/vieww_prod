//! FW-17: `examples/plugin-minimal` — the ecosystem-facing plugin — built by
//! a real `cargo` subprocess and loaded through the same registry any host
//! uses, so the *shipped example a third party copies* is the thing proven to
//! build and load, not just the author-owned fixture in `tests/fixtures/`
//! (`example_plugin.rs` covers that one, including its error paths).
//!
//! The example is its own workspace root (see its `Cargo.toml`), reached by
//! path dependency only — building it here is the path-dep half of the
//! compatibility matrix; `ci/check/plugin-compat.sh` (the `plugin-compat` leg
//! of Framework checks) additionally runs it against the host runner built
//! from HEAD and, once `vieww-plugin` has a published release, compiles the
//! example against that release too. Same skip discipline as
//! `example_plugin.rs`: loud, on stderr, and only when this machine genuinely
//! cannot run the check.

use std::path::{Path, PathBuf};
use std::process::Command;

use vieww_plugin::{stderr_host_vtable, PluginError, PluginRegistry};

fn example_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugin-minimal/Cargo.toml")
}

fn have_cargo() -> bool {
    Command::new("cargo")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// A scratch `--target-dir` this test owns, removed on drop — never inside
/// the framework workspace's own `target/`, so the nested `cargo build` can
/// never contend on the parent cargo's lock (the same reason
/// `example_plugin.rs` uses one).
struct ScratchTargetDir(PathBuf);

impl ScratchTargetDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vieww-plugin-minimal-example-target-{label}"));
        std::fs::remove_dir_all(&dir).ok();
        Self(dir)
    }
}

impl Drop for ScratchTargetDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Builds `examples/plugin-minimal` with a real `cargo build` and returns the
/// shared library it produced, whichever extension this platform uses.
fn build_example(target_dir: &Path) -> Option<PathBuf> {
    let output = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(example_manifest())
        .arg("--target-dir")
        .arg(target_dir)
        .output()
        .expect("cargo should run at all");

    if !output.status.success() {
        eprintln!(
            "plugin-minimal build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    let dir = target_dir.join("debug");
    [
        "libvieww_plugin_minimal.so",
        "libvieww_plugin_minimal.dylib",
        "vieww_plugin_minimal.dll",
    ]
    .into_iter()
    .map(|name| dir.join(name))
    .find(|path| path.exists())
}

#[test]
fn the_shipped_minimal_example_builds_outside_the_workspace_and_loads() {
    if !have_cargo() {
        eprintln!("skipping: no cargo on PATH");
        return;
    }

    let scratch = ScratchTargetDir::new("lifecycle");
    let Some(library_path) = build_example(&scratch.0) else {
        panic!("plugin-minimal did not produce a shared library at any expected path");
    };

    let mut registry = PluginRegistry::new();
    let host = stderr_host_vtable();

    // SAFETY: `library_path` was just built from
    // `examples/plugin-minimal`, a real `vieww` plugin this same test
    // controls the source of.
    let loaded = unsafe { registry.load(&library_path, &host) }
        .expect("the shipped example should load through the real registry");

    assert_eq!(loaded.name(), "vieww-plugin-minimal");
    assert_eq!(loaded.version(), "0.1.0");
    assert_eq!(loaded.commands().len(), 2);
    assert!(loaded
        .commands()
        .iter()
        .any(|c| c.id == "ping" && c.title == "Answer through the host's log"));
    assert!(loaded.commands().iter().any(|c| c.id == "toggle"));

    let plugin = &mut registry.plugins_mut()[0];

    // Every command the example ships must succeed through the real ABI —
    // including the one that reports back through the host's `log` slot,
    // which is the host↔guest direction of the boundary in miniature.
    for id in ["ping", "toggle", "ping", "toggle", "toggle"] {
        assert!(
            plugin.invoke(id).is_ok(),
            "invoke({id}) on the shipped example should succeed"
        );
    }

    match plugin.invoke("does-not-exist") {
        Err(PluginError::UnknownCommand(id)) => assert_eq!(id, "does-not-exist"),
        other => panic!("expected UnknownCommand, got {other:?}"),
    }

    // The full documented lifecycle, including the shutdown/drop sequence
    // through the loaded library — see `LoadedPlugin`'s own `Drop` impl.
    drop(registry);
}
