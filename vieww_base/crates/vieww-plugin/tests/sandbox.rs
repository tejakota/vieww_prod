//! `spawn_sandboxed` against a real, badly-behaved plugin `cdylib`
//! (`tests/fixtures/sandbox-plugin`) and the real `vieww-plugin-host`
//! binary — the ADR's test plan.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use vieww_plugin::{PluginError, PluginRegistry, SandboxError, SandboxPolicy};

fn build_fixture() -> Option<PathBuf> {
    let manifest =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sandbox-plugin/Cargo.toml");
    let target = std::env::temp_dir().join("vieww-sandbox-test-plugin-target");
    let out = Command::new("cargo")
        .args(["build", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target)
        .output()
        .ok()?;
    assert!(
        out.status.success(),
        "fixture build: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let file = format!(
        "{}vieww_sandbox_test_plugin{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    Some(target.join("debug").join(file))
}

fn policy() -> SandboxPolicy {
    SandboxPolicy {
        invoke_timeout: Duration::from_secs(3),
        host_binary: Some(PathBuf::from(env!("CARGO_BIN_EXE_vieww-plugin-host"))),
        ..SandboxPolicy::default()
    }
}

/// One fixture build for the whole file (cargo serialises on the target
/// directory anyway); tests run their sandboxes in parallel.
fn fixture() -> PathBuf {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| build_fixture().expect("cargo is needed to build the fixture"))
        .clone()
}

#[test]
fn a_sandboxed_plugin_answers_like_an_in_process_one() {
    let path = fixture();
    let registry = PluginRegistry::new();
    let mut plugin = registry.spawn_sandboxed(&path, policy()).expect("spawned");
    assert_eq!(plugin.name(), "vieww-sandbox-test-plugin");
    assert_eq!(plugin.version(), "0.0.1");
    let ids: Vec<&str> = plugin.commands().iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        ["greet", "fail", "panic", "abort", "hang", "print", "alloc"]
    );

    // Sequence parity: the same answers the in-process loader gives.
    let mut local = PluginRegistry::new();
    let host = vieww_plugin::stderr_host_vtable();
    let loaded = unsafe { local.load(&path, &host) }.expect("in-process load");
    assert_eq!(loaded.name(), plugin.name());
    assert_eq!(loaded.commands(), plugin.commands());
    let local = local.plugins_mut().first_mut().unwrap();
    for id in ["greet", "fail", "panic", "nope"] {
        let here = local.invoke(id).map_err(|e| e.to_string());
        let there = plugin.invoke(id).map_err(|e| e.to_string());
        assert_eq!(here, there, "{id}: in-process vs sandboxed");
    }
    assert!(matches!(
        plugin.invoke("fail"),
        Err(SandboxError::Plugin(PluginError::InvokeFailed(_)))
    ));
    assert!(plugin.is_alive(), "a caught panic does not end the child");
}

#[test]
fn a_plugin_that_aborts_takes_only_its_child_down() {
    let path = fixture();
    let registry = PluginRegistry::new();
    let mut plugin = registry.spawn_sandboxed(&path, policy()).expect("spawned");
    plugin.invoke("greet").expect("alive first");
    assert!(matches!(plugin.invoke("abort"), Err(SandboxError::Died(_))));
    assert!(!plugin.is_alive());
    assert!(
        matches!(plugin.invoke("greet"), Err(SandboxError::Died(_))),
        "dead stays dead"
    );
    // This process is still here — and a fresh child of the same library works.
    let mut again = registry
        .spawn_sandboxed(&path, policy())
        .expect("respawned");
    again.invoke("greet").expect("a fresh instance serves");
}

#[test]
fn a_plugin_that_hangs_is_killed_at_the_deadline() {
    let path = fixture();
    let mut plugin = PluginRegistry::new()
        .spawn_sandboxed(
            &path,
            SandboxPolicy {
                invoke_timeout: Duration::from_millis(500),
                ..policy()
            },
        )
        .expect("spawned");
    let started = Instant::now();
    assert!(matches!(
        plugin.invoke("hang"),
        Err(SandboxError::TimedOut(_))
    ));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the deadline held"
    );
    assert!(!plugin.is_alive());
}

#[test]
fn a_plugin_printing_to_stdout_does_not_corrupt_the_pipe() {
    let mut plugin = PluginRegistry::new()
        .spawn_sandboxed(fixture(), policy())
        .expect("spawned");
    plugin
        .invoke("print")
        .expect("printing is not a protocol error");
    plugin
        .invoke("greet")
        .expect("and the pipe still carries frames");
}

// Unix: the child's environment switch goes through a `/bin/sh` wrapper.
#[cfg(unix)]
#[test]
fn an_oversized_reply_is_cut_off_not_read() {
    // The child inherits this, and the fixture then lists 50 000 commands.
    let mut command_env = policy();
    command_env.max_reply_bytes = 64 * 1024;
    // The switch goes to the child only, through a wrapper script, so this
    // process's environment is never mutated under parallel tests.
    let host = command_env.host_binary.clone().unwrap();
    let wrapper = std::env::temp_dir().join(format!("vieww-sandbox-huge-{}", std::process::id()));
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nVIEWW_SANDBOX_TEST_HUGE=1 exec '{}' \"$@\"\n",
                host.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        command_env.host_binary = Some(wrapper.clone());
        let result = PluginRegistry::new().spawn_sandboxed(fixture(), command_env);
        assert!(
            matches!(result, Err(SandboxError::Protocol(_))),
            "{result:?}"
        );
        std::fs::remove_file(&wrapper).ok();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_memory_limit_ends_an_unbounded_allocation_in_the_child() {
    let mut plugin = PluginRegistry::new()
        .spawn_sandboxed(
            fixture(),
            SandboxPolicy {
                memory_limit: Some(512 << 20),
                invoke_timeout: Duration::from_secs(20),
                ..policy()
            },
        )
        .expect("spawned under a 512 MiB cap");
    // The allocator fails at the cap and Rust aborts the child — the host,
    // with no cap of its own, carries on.
    assert!(matches!(plugin.invoke("alloc"), Err(SandboxError::Died(_))));
}

#[test]
fn no_host_binary_is_a_spawn_error_not_a_panic() {
    let missing = SandboxPolicy {
        host_binary: Some(PathBuf::from("/nonexistent/vieww-plugin-host")),
        ..policy()
    };
    assert!(matches!(
        PluginRegistry::new().spawn_sandboxed(fixture(), missing),
        Err(SandboxError::Spawn(_))
    ));
}
