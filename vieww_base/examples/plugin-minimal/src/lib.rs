//! The smallest real `vieww` plugin: one struct, two commands, one log line
//! through the host — and nothing else.
//!
//! This crate exists to be *copied*. It is deliberately not a workspace
//! member (see its `Cargo.toml` for the fence) so that building it exercises
//! exactly the path a third party's plugin takes: `vieww-plugin` reached as a
//! dependency, the `#[vieww_plugin]` macro generating the one
//! `vieww_plugin_entry` export, and every interaction with the host crossing
//! the documented ABI — never a shared Rust type, allocation, or trait
//! object. `vieww-plugin`'s own test suite loads the binary this crate
//! produces through the real `dlopen` path; CI additionally compiles it
//! against the last *published* `vieww-plugin` and runs that binary against
//! the current host, which is the compatibility promise demonstrated rather
//! than asserted. The contract itself is
//! [`docs/plugin-abi.md`](../../../docs/plugin-abi.md) in the framework tree.
//!
//! # Unimplemented notes, honestly
//!
//! Nothing here reads host state or draws to the screen: ABI 1 carries
//! commands and logging only (see `plugin-abi.md`'s "What 1.x does not
//! promise" for why a first version is this small on purpose).

use std::cell::Cell;

use vieww_plugin::abi::{AbiStr, HostVTable, LogFn, LogLevel};
use vieww_plugin::{vieww_plugin, Plugin, PluginCommand};

/// A one-bit note the plugin keeps to itself — plugin-owned state the host
/// never sees except through command results, which is the whole ownership
/// story of the ABI in one field.
///
/// The `log` slot demonstrates the one piece of the host a plugin may keep
/// past `init`: the plain function pointer (documented `Copy`), never the
/// `&HostVTable` it arrived in, whose validity window is that call only.
#[vieww_plugin]
pub struct NotesPlugin {
    note_on: Cell<bool>,
    /// Copied out of the `HostVTable` during `init`, per the lifetime rule
    /// above. `extern "C" fn` pointers are plain values: `Copy`, `Send`,
    /// `Sync` — nothing unsafe about holding one.
    log: Option<LogFn>,
}

impl Default for NotesPlugin {
    fn default() -> Self {
        Self {
            note_on: Cell::new(false),
            log: None,
        }
    }
}

impl NotesPlugin {
    /// Report through whichever host `log` this plugin was initialized
    /// under — `eprintln!` would *look* equivalent in development and then
    /// silently bypass a host that routes plugin logs elsewhere.
    fn report(&self, level: LogLevel, message: &str) {
        if let Some(log) = self.log {
            log(level, AbiStr::borrowed(message));
        }
    }
}

impl Plugin for NotesPlugin {
    fn name(&self) -> &str {
        "vieww-plugin-minimal"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(&mut self, host: &HostVTable) -> Result<(), String> {
        // Keep the function pointer, forget the struct — see the field doc.
        self.log = Some(host.log);
        self.report(LogLevel::Info, "minimal plugin initialized");
        Ok(())
    }

    fn shutdown(&mut self) {
        self.report(LogLevel::Info, "minimal plugin shutting down");
    }

    fn commands(&self) -> Vec<PluginCommand> {
        vec![
            PluginCommand::new("ping", "Answer through the host's log"),
            PluginCommand::new("toggle", "Flip this plugin's own note"),
        ]
    }

    fn invoke(&mut self, id: &str) -> Result<(), String> {
        match id {
            "ping" => {
                // Crossing back into the host synchronously, inside this
                // very call, with memory this stack frame owns for its
                // duration — exactly the validity window the borrowed
                // `AbiStr` design requires.
                self.report(LogLevel::Info, "pong");
                Ok(())
            }
            "toggle" => {
                let now = !self.note_on.get();
                self.note_on.set(now);
                self.report(LogLevel::Debug, if now { "note on" } else { "note off" });
                Ok(())
            }
            // Unreachable through the real ABI: the trampoline checks every
            // id against the `commands` snapshot before `invoke` runs.
            _ => Err(format!("unknown command: {id}")),
        }
    }
}
