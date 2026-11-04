# ADR: `spawn_sandboxed` — plugin isolation out of process

- **Status:** Implemented (`vieww_plugin::sandbox`, the `vieww-plugin-host`
  binary, `tests/sandbox.rs`). The tail records what changed from the design
  and what is untested.
- **Context:** FW-17 (`docs/plugin-abi.md`, `vieww-plugin`). Recorded
  2026-10-21, framework worktree `wt-fw-b`.
- **Related:** [`plugin-abi.md`](./plugin-abi.md) (the in-process contract this
  builds *on top of*, not instead of), `vieww-reload` (a different, deliberately
  coupled ABI — see `vieww-plugin/src/abi.rs`'s module doc for the split).

## The decision

`PluginRegistry` grows one new entry point:

```rust,ignore
impl PluginRegistry {
    /// Load `path` in a supervisor child process instead of this one.
    pub fn spawn_sandboxed(
        &mut self,
        path: impl AsRef<Path>,
        policy: SandboxPolicy,
    ) -> Result<SandboxedPlugin, PluginError>;
}
```

The child is a small std-only binary (host side of the pipe) that **reuses the
existing in-process loader unchanged**: it `dlopen`s the plugin, runs the same
version gate and `init`, then serves vtable calls as messages. Everything
`plugin-abi.md` promises stays true inside the child; the parent never links
the plugin at all.

This is **fault and resource isolation, not a security sandbox**, and the API
name stops one word short of claiming more: a hostile native process can
always lie over a pipe. OS-level confinement (seccomp/Landlock on Linux,
App Sandbox on macOS, AppContainer on Windows) is a later, per-platform
layer with its own ADR if a threat model ever demands it. What this design
buys on day one: a plugin that panics with `panic = "abort"`, segfaults,
infinite-loops, or leaks without bound **cannot take the host's process down
with it** — the failure the in-process ABI can only mitigate.

## Why a child process, not the alternatives

- **In-process `catch_unwind` (status quo).** Already shipped as ABI 1.1's
  panic contract — and honestly bounded there: it cannot catch `abort`, a
  segfault in plugin code, or a hang. Everything the contract cannot do is the
  job description of this ADR.
- **WASM plugins.** Real isolation in-process, but a *different ABI and a
  different ecosystem*: a wasm plugin is not a `cdylib`, has no host `dlopen`,
  and needs a whole compilation/imports story. A future `AbiVersion 2` decision
  if plugin authors ask for it; recording it here as considered-and-deferred.
- **OS threads with signals.** A hung thread cannot be safely killed in Rust
  today; `pthread_cancel` is unsound with Rust's unwinding. Not a real option,
  listed so nobody re-investigates it.

## The protocol (length-prefixed, std-only)

One `std::process::Child` per sandboxed plugin, `stdin`/`stdout` as a
bidirectional pipe, `stderr` inherited (plugin `log` output reaches the
operator's console exactly as in-process plugins' does — same
`stderr_host_vtable` in the child). Every message is `u32
little-endian length + UTF-8 JSON body` reusing `vieww-foundation`'s
`json.rs` (the repo already refuses a serde dependency), shaped after the
studio's out-of-process preview IPC (FW-10-era `remote.rs`) — the *pattern*,
not the crate: `vieww-plugin` cannot depend on an app, and the app must not
grow framework plumbing it does not own.

Calls are synchronous request/response over the single pipe, matching the
in-process ABI's own sequencing rule (one call at a time, never re-entered):

| parent → child            | child does                            | child → parent            |
|---------------------------|---------------------------------------|---------------------------|
| `{"load": path}`          | `dlopen`, version gate, `init`        | `{"loaded": name, version, commands}` or `{"error": …}` |
| `{"invoke": id}`          | the same trampoline the host would run| `{"ok": true}` or `{"error": code}` |
| `{"shutdown": true}`      | `shutdown`, `drop`, exit 0            | *(process exit)*          |

`SandboxedPlugin::invoke` maps the child's exit-without-answer to
`PluginError::Panicked("<sandboxed: child died>")` — a new variant
(`SandboxDied`) is the one additive `HostVTable`-side change this design
foresees, and it lands as an ABI **minor** bump with a new status code,
exactly the additive path `plugin-abi.md` commits to.

## `SandboxPolicy`

```rust,ignore
pub struct SandboxPolicy {
    /// Kill the child after this many ms without a pipe answer (default 5 s).
    pub invoke_timeout: Duration,
    /// `RLIMIT_AS`-shaped cap on the child's address space, where the OS
    /// offers one (Unix: setrlimit pre-exec; Windows: job objects — the
    /// honest-untested note on the Windows leg is expected, not a surprise).
    pub memory_limit: Option<usize>,
    /// Kill the child if it writes more than this to the pipe (a plugin
    /// that answers a `u32`-length protocol with gigabytes is broken or
    /// hostile; either way the parent stops reading first).
    pub max_reply_bytes: usize,
}
```

The watchdog is a plain thread over `Child::try_wait` + pipe deadlines —
std-only, the same discipline as `vieww-reload`'s watcher (no new deps, a
documented polling cadence where a blocking read would otherwise own the
thread forever).

## What stays in process, and who chooses

Trusted plugins keep the zero-copy `load` path; `spawn_sandboxed` is opt-in
per plugin. That is the whole policy mechanism, and it is the right default:
the in-process ABI is the fast path for command-shaped plugins, and ABI 1 is
command-shaped — a few invocations per user action, where one pipe
round-trip (~tens of microseconds) is noise. The design would be wrong the
day ABI grows per-frame work; that day is also a major-version day, which is
the versioning system doing its job.

## Test plan (when implemented)

- **Child dies mid-`invoke`** (a plugin that calls `std::process::abort()` in
  `invoke`): host observes `SandboxDied`, keeps serving other plugins, can
  `spawn_sandboxed` a fresh instance of the same library afterwards.
- **Timeout**: a plugin that sleeps forever in `invoke`; the policy's deadline
  fires, child reaped, error surfaced.
- **Reply-bytes cap**: a plugin that writes a huge reply is cut off, not OOM.
- **Sequence parity**: the same `examples/plugin-minimal` binary loaded both
  ways answers identically (name, version, commands, invoke results) — the
  sandbox changes *where* the plugin runs, never *what* the contract says.

## As implemented (2026-11-03)

- `PluginRegistry::spawn_sandboxed(&self, path, SandboxPolicy)` returns a
  `SandboxedPlugin` (`name`, `version`, `commands`, `invoke`, `is_alive`);
  dropping it sends `shutdown` and reaps the child, killing it past the
  policy's deadline.
- **Errors are their own type**, `SandboxError`, not new `PluginError`
  variants: `PluginError` is not `#[non_exhaustive]`, and a host matching it
  exhaustively would break on a minor bump. A plugin's own refusals cross the
  pipe as the same `PluginError` (`SandboxError::Plugin`), so a caller
  handles them identically; the child's own failures are `Died`, `TimedOut`,
  `Protocol` and `Spawn`.
- **The wire is std-only framing, not JSON**: `u32` LE length + UTF-8 body,
  fields split by U+001F and command id/title by U+001E. This crate depends
  on `libloading` alone, and five messages do not justify a JSON dependency.
- **stdout belongs to the protocol.** The host binary moves its replies to a
  duplicate of fd 1 and points fd 1 at stderr (Windows: `SetStdHandle`)
  before anything loads, so a plugin's `println!` reaches the console, not
  the pipe — `a_plugin_printing_to_stdout_does_not_corrupt_the_pipe`.
- **Memory limit:** the child applies `RLIMIT_AS` to itself before loading
  (Linux, Android, macOS, iOS, via `setrlimit`); on Windows the limit is
  reported as not enforced — a job object is the mechanism, not built yet.
- **Finding the host:** `SandboxPolicy::host_binary`, else
  `VIEWW_PLUGIN_HOST`, else beside the running executable (or one directory
  up, for a test binary in `deps/`). An application that sandboxes plugins
  ships `vieww-plugin-host` next to itself.
- **Tested** (Linux, `tests/sandbox.rs`, a real misbehaving `cdylib`):
  sequence parity with the in-process loader, `abort` ending only the child
  and a fresh child serving afterwards, a hang killed at a 500 ms deadline,
  stdout writes not corrupting the pipe, a 50 000-command reply refused at a
  64 KiB cap before it is read, a 512 MiB cap ending an unbounded
  allocation, and a missing host binary as `Spawn`. **Untested:** the
  Windows stdout redirection and the macOS `RLIMIT_AS` value (type-checked
  only).

