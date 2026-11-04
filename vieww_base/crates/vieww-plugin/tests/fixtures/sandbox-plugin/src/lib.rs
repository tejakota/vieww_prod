//! The sandbox's adversary — see `Cargo.toml`.

use std::cell::Cell;

use vieww_plugin::{vieww_plugin, Plugin, PluginCommand};

#[vieww_plugin]
pub struct SandboxTestPlugin {
    greets: Cell<u32>,
}

impl Default for SandboxTestPlugin {
    fn default() -> Self {
        Self { greets: Cell::new(0) }
    }
}

impl Plugin for SandboxTestPlugin {
    fn name(&self) -> &str {
        "vieww-sandbox-test-plugin"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    fn commands(&self) -> Vec<PluginCommand> {
        let mut out = vec![
            PluginCommand::new("greet", "Count a greeting"),
            PluginCommand::new("fail", "Return an error"),
            PluginCommand::new("panic", "Panic (caught by the trampoline)"),
            PluginCommand::new("abort", "Abort the process"),
            PluginCommand::new("hang", "Never return"),
            PluginCommand::new("print", "Write to stdout"),
            PluginCommand::new("alloc", "Allocate without bound"),
        ];
        // A command list too big for the sandbox's reply cap.
        if std::env::var_os("VIEWW_SANDBOX_TEST_HUGE").is_some() {
            out.extend((0..50_000).map(|i| PluginCommand::new(format!("c{i}"), "x".repeat(40))));
        }
        out
    }

    fn invoke(&mut self, id: &str) -> Result<(), String> {
        match id {
            "greet" => {
                self.greets.set(self.greets.get() + 1);
                Ok(())
            }
            "fail" => Err("on purpose".to_owned()),
            "panic" => panic!("on purpose"),
            "abort" => std::process::abort(),
            "hang" => loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
            },
            "print" => {
                // Into what would be the protocol pipe, without the host's
                // redirection: a frame-length's worth of garbage and more.
                for _ in 0..1000 {
                    println!("hello from the plugin's stdout");
                }
                Ok(())
            }
            "alloc" => {
                let mut hoard: Vec<Vec<u8>> = Vec::new();
                loop {
                    let mut block = vec![0u8; 64 << 20];
                    block[0] = 1;
                    hoard.push(block);
                }
            }
            _ => unreachable!(),
        }
    }
}
