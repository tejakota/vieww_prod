//! The smallest honest host: ~70 lines that load one plugin `cdylib`, print
//! everything the ABI gives a host, invoke every command it offers, and tear
//! it down cleanly. `ci/check/plugin-compat.sh` (the `plugin-compat` leg of
//! Framework checks) drives it with `examples/plugin-minimal` — including,
//! once `vieww-plugin` has a published release, a plugin binary compiled
//! against that *older* release, which is the compatibility promise in
//! motion. A plugin author can drive it with their own build just the same:
//!
//! ```sh
//! cargo run -p vieww-plugin --example load_plugin -- path/to/plugin.so
//! ```
//!
//! Exit codes: `0` the full lifecycle ran clean; `1` the load or a command
//! failed (the error is on stderr); `2` usage. Deliberately not a
//! `[[bin]]` of the crate: an *example* says what this is — something to
//! read before writing a real embedding, not a tool the framework promises
//! as an interface.

use std::path::PathBuf;
use std::process::ExitCode;

use vieww_plugin::{stderr_host_vtable, PluginRegistry};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        eprintln!("usage: load_plugin <path-to-plugin-cdylib>");
        return ExitCode::from(2);
    }
    let path = PathBuf::from(&args[0]);

    let mut registry = PluginRegistry::new();
    // The default host: stderr logging, current ABI version. A real
    // application writes its own `HostVTable` literal to route `log`
    // somewhere it wants — see `vieww_plugin::host`'s module doc.
    let host = stderr_host_vtable();

    // SAFETY: the caller named the path; loading and running a native
    // plugin is inherently unsafe in the way every `dlopen` is, and the
    // version gate inside `load` is the first line of defence, not the
    // last. This runner is for plugins you chose to trust.
    let loaded = match unsafe { registry.load(&path, &host) } {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("load failed: {error:?}");
            return ExitCode::from(1);
        }
    };

    println!(
        "loaded {} {} from {}",
        loaded.name(),
        loaded.version(),
        loaded.path().display()
    );

    // Copy out the command list before taking the `&mut` — `commands()` is
    // the post-`init` snapshot the ABI promises, stable for the plugin's
    // lifetime.
    let commands: Vec<(String, String)> = loaded
        .commands()
        .iter()
        .map(|command| (command.id.clone(), command.title.clone()))
        .collect();
    for (id, title) in &commands {
        println!("  command {id:?}: {title}");
    }
    if commands.is_empty() {
        println!("  (no commands)");
    }

    let mut status = 0u8;
    for (id, _) in &commands {
        match registry.plugins_mut()[0].invoke(id) {
            Ok(()) => println!("invoke {id:?}: ok"),
            Err(error) => {
                eprintln!("invoke {id:?} failed: {error:?}");
                status = 1;
            }
        }
    }

    // Drop runs the documented shutdown sequence — `shutdown`, then the
    // plugin's own `drop` trampoline, then the library unload.
    drop(registry);
    println!("unloaded cleanly");

    ExitCode::from(status)
}
