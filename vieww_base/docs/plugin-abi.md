# The vieww plugin ABI, version 1

This is the contract a third-party plugin is built against, stated as a
promise rather than as code. `vieww-plugin`'s own doc comments are the
line-by-line authority; this page is what a plugin author needs to believe
before shipping a binary, and what the vieww maintainers have committed to
not break while the major number is 1.

The short version: **a plugin is a `cdylib` exporting one symbol,
`vieww_plugin_entry`, which returns a `#[repr(C)]` vtable of function
pointers. Nothing else crosses the boundary — no Rust trait object, no
`String`, no `Box`, no allocator.** Everything below is that sentence with
its consequences paid.

## Versioning: what `AbiVersion` promises

Every vtable's first field is an `AbiVersion { major, minor }`, and the host
reads *only that field* before anything else — in every ABI version, past
and future, that field is first by invariant, which is what makes the check
itself version-safe.

| change | bump | effect on a plugin built against the older one |
|---|---|---|
| new vtable field appended, new status code, new documented behaviour that old code already satisfies | minor | loads; a 1.2 host runs a 1.0 plugin |
| field removed, reordered, retyped, or its *meaning* changes | major | **refuses to load**, loudly, before any other call is made |

The rule the host enforces is `plugin.major == host.major &&
plugin.minor <= host.minor`. A plugin built against a *newer* minor than the
host is also refused, even though the change was additive: that plugin may
call a host vtable field or rely on a vtable slot the older host's struct
does not have, and calling through a function pointer at the wrong offset is
memory corruption, not a debuggable crash.

Within major 1, additions are **additive only**. Appending a field to the
end of `PluginVTable` or `HostVTable` is the only sanctioned shape change,
because a host built before the field reads a prefix it still understands
and a plugin built before it never touches what it cannot see.

Current version: **1.1** (minor 1 added the `STATUS_PANICKED` code; a 1.0
host reads it as a generic failure, which is exactly the degradation the
additive rule requires).

### The ABI version is not the crate's semver

`vieww-plugin` the *crate* has its own version line, and while it is 0.x it
may change its Rust API (`trait Plugin`'s methods, `PluginCommand`'s
fields) freely. What 0.x cannot do is bump `AbiVersion`'s major: a compiled
plugin `cdylib` from any toolchain must keep loading into any host whose
ABI major matches and whose minor is at least the plugin's. Binary
compatibility is keyed on `AbiVersion` alone — the Rust API is
source-compatibility, which is the plugin author's build-time problem, and
the compatibility-matrix CI job exists to keep both honest.

## The wire format

```rust,ignore
// The one export. Found by dlsym; returns a leaked *mut PluginVTable.
unsafe extern "C" fn vieww_plugin_entry() -> *mut PluginVTable;

#[repr(C)]
pub struct PluginVTable {
    pub abi_version: AbiVersion,          // always first (the invariant above)
    pub instance: *mut c_void,            // plugin-owned, opaque to the host
    pub name:        unsafe extern "C" fn(*mut c_void) -> AbiStr,
    pub version:     unsafe extern "C" fn(*mut c_void) -> AbiStr,
    pub init:        unsafe extern "C" fn(*mut c_void, *const HostVTable) -> i32,
    pub shutdown:    unsafe extern "C" fn(*mut c_void),
    pub command_count: unsafe extern "C" fn(*mut c_void) -> usize,
    pub command_at:  unsafe extern "C" fn(*mut c_void, usize) -> AbiCommand,
    pub invoke:      unsafe extern "C" fn(*mut c_void, AbiStr) -> i32,
    pub drop:        unsafe extern "C" fn(*mut c_void),
}
```

Strings cross as `AbiStr` — `(ptr, len)`, borrowed, never owned. Commands
cross as `AbiCommand { id: AbiStr, title: AbiStr }`. Status codes are
`i32`: `0` ok, `-1` error, `-2` unknown command, `-3` panicked (1.1).

`#[vieww_plugin]` on a type implementing `trait Plugin` (which requires
`Default`) generates this export; a plugin author who writes it by hand
calls `vieww_plugin::abi::leak_vtable` themselves and takes on that
responsibility. Everything else in the crate is safe code.

## Lifetimes at the boundary

These are the rules that make the borrowed-pointer design sound, and every
one of them is also in the doc comment it belongs to:

- **`instance` is plugin-owned.** The host treats it as opaque. `drop` is
  the last call the host ever makes through the vtable, and after it the
  pointer is dead.
- **A returned `AbiStr` is valid until the next call into the same
  instance** — the `readdir` convention. A host that needs a name, version,
  or command list to outlive the next call copies it into a `String`
  immediately. `vieww-plugin`'s own `LoadedPlugin` does exactly that, once,
  at load time.
- **The `HostVTable` pointer is valid for the duration of the `init` call
  only.** A plugin that wants to log later keeps the `log` function pointer
  itself (plain and `Copy`), never the struct pointer.
- **`command_count`/`command_at` read a snapshot** taken once, immediately
  after a successful `init`. A plugin whose command list could change at
  runtime is out of scope for ABI 1 — see "What 1.x does not promise".

## Memory: there is no shared allocator, on purpose

Nothing is allocated by one side and freed by the other. Every `AbiStr` is
borrowed for a documented window and ownership never crosses; the plugin's
instance is allocated by the plugin's allocator (inside `leak_vtable`) and
freed by the plugin's `drop` trampoline (same allocator, same module); the
host's `HostVTable` is stack or host-owned memory a plugin never stores.

That is why `HostVTable` in 1.x carries no `malloc`/`free` function
pointers, which is the shape most C plugin systems need: those exist to let
the *guest* allocate memory the *host* must later free (or vice versa), and
this ABI has no such flow. If a future minor version adds one (an
out-parameter buffer for error text is the most plausible reason), it will
be appended to `HostVTable` per the additive rule, and older plugins will
simply not use it.

Each `leak_vtable` call leaks exactly two small allocations for the life of
the process — the vtable and the entry box — because a `dlopen`ed plugin
has no moment before process exit at which a host is *required* to be done
with it. The plugin's own resources are not leaked: `drop` frees the
instance for real when the host calls it.

## The panic contract

A panic may never unwind out of an `extern "C"` function; Rust aborts the
process when one tries. So every trampoline in `vieww-plugin` runs plugin
code under `catch_unwind` **when the plugin was built with
`panic = "unwind"`**, reports the message through the host's `log` at
error level, and returns `STATUS_PANICKED` (ABI 1.1) — the host sees a
`PluginError::Panicked`, the process lives.

A plugin built with `panic = "abort"` cannot be caught in-process at all;
the panic ends the process after the host's crash reporter records it.
In-process isolation is best-effort by nature. **For code the host does not
trust, the answer is a separate process** — that decision, and the
`spawn_sandboxed` design, is recorded in
[`docs/plugin-sandbox-adr.md`](./plugin-sandbox-adr.md).

## Threads: sequential, never re-entered

Calls into one plugin instance must be **sequential and never re-entered**:
the same instance is not called from two threads at once, and no vtable
function of a plugin is called from inside that plugin's own call. This is
the contract the trampolines' `unsafe` blocks are written against (a `&mut`
to the instance exists for the duration of every call), and it is the
plugin author's side of the bargain that `trait Plugin`'s methods may
assume they are the only code touching `self`.

Consequently:

- A plugin's `Plugin` impl needs no internal locking for its own state. If
  a plugin *itself* starts threads, synchronizing them is entirely the
  plugin's business — the ABI gives it no help and takes no responsibility.
- `LoadedPlugin` is `Send` (a host may move a plugin to another thread) but
  deliberately **not** `Sync` (two threads cannot hold it at once), which
  is the type-system spelling of the same rule.
- `HostVTable::log` must be safe to call from whatever thread the plugin
  calls it on; the default host's implementation is `eprintln!`, which is.

## What 1.x does not promise

Stated so nobody is surprised later, each with the reason it was deferred:

- **Error text across the boundary.** `invoke` failures return a bare
  `-1`. Carrying text needs an out-parameter buffer with a capacity
  protocol — the kind of interface a first version should not guess the
  shape of. The plugin's own `Err` strings go to the host *log* meanwhile.
- **A mutable command list.** The post-`init` snapshot is the whole
  protocol. Dynamic commands need invalidation events, which need host
  callbacks the vtable does not have yet.
- **Hot-reload of a plugin.** `dlopen`/`dlclose` cycles of the same library
  are a known trap (thread-locals, once-flags) that `vieww-reload` exists
  to solve *for the application's own code*, at a different coupling level
  (same-rustc guest, same dependency graph). A third-party plugin is
  loaded once per process and never unloaded-but-reloaded.
- **Process isolation.** In-process, per the panic contract above; see the
  ADR for the out-of-process design.

## How the promise is tested

- `vieww-plugin`'s unit tests cover the loader, the version gate (both
  refusal directions), the panic trampoline and the lifetime rules.
- `examples/plugin-minimal` is a real `cdylib` **outside the workspace**,
  compiled against `vieww-plugin` by path and loaded through the registry by
  `vieww-plugin`'s own test suite (`tests/minimal_example.rs`) — the
  smallest possible proof that the documented contract is buildable by
  somebody who is not the ABI's author, exercised on every `cargo test` of
  the crate, not only in CI.
- The compatibility-matrix CI job (framework-checks, `plugin-compat` leg)
  compiles that same example against the **last published** `vieww-plugin`
  and runs the compiled `cdylib` against the **current** host — which is
  what "additive only within a major" actually means, demonstrated rather
  than asserted. Until FW-01's first publish ships, that leg skips loudly
  with the reason, rather than quietly testing nothing.

## Changelog

| ABI | change |
|---|---|
| 1.0 | `PluginVTable`, `HostVTable`, `AbiStr`, `AbiCommand`, entry symbol, version gate |
| 1.1 | `STATUS_PANICKED` + the panic-reporting trampoline contract |
