# plugin-minimal — the ABI's compatibility example

The smallest real `vieww` plugin: one struct, two commands, one log line
through the host. It exists to be **copied** — this directory is deliberately
its own workspace (the empty `[workspace]` table in `Cargo.toml`), so it
builds exactly the way a third party's plugin does: the framework reached
only through the `vieww-plugin` dependency, never through workspace
inheritance.

## Build it, load it

```sh
# from this directory (or: cargo build --manifest-path Cargo.toml)
cargo build

# from the framework checkout — the smallest honest host, built from HEAD
cargo run -p vieww-plugin --example load_plugin -- \
  target/debug/libvieww_plugin_minimal.so   # .dylib on macOS, .dll on Windows
```

The host prints the plugin's name, version and commands, invokes each
command once, and shuts it down cleanly — the full documented lifecycle of
[`docs/plugin-abi.md`](../../docs/plugin-abi.md), which is the contract to
read before writing anything bigger than this.

## Why this example exists

`vieww-plugin`'s own tests load a fixture plugin that lives *inside* that
crate — written by the ABI's author, it can only ever prove the author's
code agrees with itself. This crate is the honest version of that proof:

1. `vieww-plugin`'s test suite builds **this** crate with a real `cargo`
   subprocess and `dlopen`s the result through the same registry any host
   uses (`crates/vieww-plugin/tests/minimal_example.rs`).
2. CI's compatibility leg (`vieww_base/ci/check/plugin-compat.sh`, the
   `plugin-compat` job of *Framework checks*) compiles it against the
   framework by **path** on every run, and — once `vieww-plugin` has a
   published release (FW-01) — additionally compiles a copy against the
   **last published** version, then runs that binary against the **current**
   host. That second half is what "additive-only within a major" actually
   means, demonstrated rather than asserted.

Until the first publish ships, the published half of that leg skips loudly
with its reason instead of quietly testing nothing.

## Notes for the copier

- Implement [`Plugin`] for a `Default` type, put `#[vieww_plugin]` on it,
  done — the macro generates the one `vieww_plugin_entry` export the host
  looks up. Everything else in `vieww-plugin` is safe code.
- Strings cross borrowed: a returned `&str` is valid until the next call;
  host-provided pointers are valid only inside the call that handed them
  over. `plugin-abi.md`'s "Lifetimes at the boundary" section is the whole
  list.
- If your plugin starts threads, synchronizing them is entirely your
  business; the host will call you sequentially and never re-enter.
