//! ST-13: what the studio *does* with the plugins it loaded.
//!
//! The ABI is [`crate::plugin`]'s; this file is the studio's side of the three
//! plug points — loading on command, running a palette command, and offering
//! the loaded rules to the lint pass. The export sheet's use of plugin formats
//! lives in `state/export.rs` with the rest of the export flow, because that
//! is one flow and this file is not a taxonomy.
//!
//! # Why loading is a command and not a launch step
//!
//! Three reasons, all of them the same reason: the directory is a person's
//! hand-curated list of code they want run inside this process, and acting on
//! it should be an act. Concretely — a studio launched by an integration test
//! would `dlopen` the developer's real `~/.local/share/vieww/plugins` and
//! inherit whatever was there; a broken plugin would break every launch
//! rather than one reload; and a person who has heard "plugins" and wants
//! nothing to do with them keeps a studio that runs none.

use super::*;

impl Studio {
    /// Read `<data_dir>/plugins/` and load what is there, replacing whatever
    /// was loaded before.
    ///
    /// Every outcome is a line in the Output panel: one per plugin that
    /// loaded (what it contributed), one per file that was refused (why, in
    /// the loader's own words), and a count either way. A reload that said
    /// nothing would have done something invisible, which is the opposite of
    /// what a command that runs somebody else's code owes the person who ran
    /// it.
    pub fn reload_plugins(&self) {
        let dir = crate::about::data_dir().join("plugins");
        let registry = crate::plugin::Registry::discover(&dir);

        let mut lines = Vec::new();
        if !dir.is_dir() {
            lines.push(format!(
                "no plugins directory at {} — create it and drop a plugin in",
                dir.display()
            ));
        }
        for plugin in &registry.plugins {
            lines.push(format!(
                "{}: {} lint rule(s), {} palette command(s), {} export format(s)",
                plugin.name(),
                plugin.lints().len(),
                plugin.commands().len(),
                plugin.formats().len()
            ));
        }
        for failure in &registry.failures {
            lines.push(failure.clone());
        }
        lines.push(format!("{} plugin(s) loaded", registry.plugins.len()));

        self.plugins.set(Rc::new(registry));
        // The lint pass is memoised against the buffer's text, and the text
        // has not changed — what changed is the rules. Without this the
        // panel would go on showing the pre-plugin pass until the next
        // keystroke, and a plugin whose rule never fires looks like a plugin
        // that loaded and did nothing.
        *self.lint_cache.borrow_mut() = None;
        self.append_output(lines);
        self.show_output();
    }

    /// Run a command a plugin contributed, as the palette asked for it.
    ///
    /// The pair `(plugin, title)` is the command's identity — see
    /// [`PaletteEntry::Plugin`](crate::state::PaletteEntry::Plugin) for why
    /// the title alone is not enough.
    pub fn run_plugin_command(&self, plugin: &str, title: &str) {
        // The registry's Rc is cloned out of the signal's read guard so the
        // borrowed plugin outlives it — the guard is a temporary, the plugin
        // must not be.
        let registry = self.plugins.get().clone();
        let Some(plugin) = registry.find(plugin) else {
            // A plugin that was unloaded between the palette reading it and
            // this running. Nothing is worse than quietly doing nothing, so
            // it is said — the alternative looks like a broken palette.
            self.notify("that plugin is not loaded — reload plugins and try again");
            return;
        };
        // The buffer's whole text, because that is the one editing verb the
        // ABI offers: see `plugin::Plugin::run_command`. No buffer, no text
        // to hand it, and the command is told so rather than run on nothing.
        let Some(buffer) = self.active() else {
            self.notify("that plugin command works on a buffer, and none is open");
            return;
        };
        let text = buffer.value.text.clone();
        match plugin.run_command(title, &text) {
            crate::plugin::Outcome::Replace(text) => {
                // Through `edit`, not a bare value swap: one undo step, the
                // collab seam, the dirty flag and the auto-render debounce —
                // everything every other whole-buffer rewrite goes through.
                //
                // The selection survives the rewrite clamped to the new
                // length, exactly as a canvas edit's does: a plugin that
                // shortens the file must not leave the caret describing a
                // position that no longer exists.
                let len = text.len();
                let clamp = |offset: usize| offset.min(len);
                let mut value = vieww_foundation::TextEditingValue::new(text);
                value.selection = vieww_foundation::TextSelection {
                    base: clamp(buffer.value.selection.base),
                    extent: clamp(buffer.value.selection.extent),
                    affinity: buffer.value.selection.affinity,
                };
                self.edit(value);
            }
            crate::plugin::Outcome::Notice(message) => {
                self.notify(&message);
            }
            crate::plugin::Outcome::None => {}
        }
    }
}
