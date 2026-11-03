//! ST-13: the studio's plugin ABI — the smallest honest one.
//!
//! # What a plugin is, and what it is allowed to be
//!
//! A `cdylib` in `<data_dir>/plugins/` that the studio `dlopen`s at the user's
//! request (the **Reload Studio Plugins** command) and then consults from
//! three places it already had:
//!
//! * [`LintRule`](crate::lint::LintRule) — extra findings in the Problems
//!   panel, through the same door the built-in rules use;
//! * an export [`Plan`](crate::export::Plan) builder — extra rows in the
//!   export sheet, through the same planner/refusal split every built-in
//!   format goes through;
//! * palette commands — extra rows in the command palette.
//!
//! Nothing else. A plugin cannot draw, cannot read the workspace beyond the
//! text it is handed, and cannot change settings. That is the point: the ABI
//! is small enough to still be an ABI in a year, not a platform.
//!
//! # The boundary is bytes, not objects
//!
//! Every value that crosses the edge is either a C string or a
//! `(pointer, length)` pair of bytes, and every compound value is JSON built
//! from those. No `Rc`, no `Vec`, no trait object ever crosses — those have no
//! layout stability between two compilations of even the same crate, which is
//! precisely the failure the preview's fingerprint guard exists for.
//!
//! Strings a plugin returns belong to the plugin; the studio hands the pointer
//! back to `vieww_plugin_free` and never touches it again. Strings the studio
//! passes are borrowed for the duration of the call and must be copied if the
//! plugin wants them.
//!
//! # The fingerprint guard, and why plugins get it too
//!
//! [`load`] refuses a library whose `vieww_plugin_fingerprint` does not equal
//! [`host_fingerprint`](crate::loaded::host_fingerprint) — the *same* number
//! the compiled preview is checked against, asking the *same* question: is
//! this the same compilation of `vieww` as the one this process links? The
//! plugin ABI above crosses no `vieww` types, so a mismatch would not corrupt
//! memory the way a mismatched preview does; the reason to check anyway is
//! that the fingerprint is the one signal a plugin was built from the
//! studio's own dependency graph, and a plugin built from any other graph is
//! one whose author the studio has no way to reach. The refusal tells them
//! how to fix it rather than failing inside a call three screens away.
//!
//! # Honest limits, stated here rather than discovered later
//!
//! * **Plugins run on the frame thread.** A plugin that stalls stalls the
//!   studio — same trust model as a lint rule compiled in, without the
//!   process boundary `rustc` gets. Not solved; stated.
//! * **A plugin must not unwind.** `extern "C"` functions that unwind are
//!   undefined behaviour; the example plugin in `plugins/example/` wraps
//!   every body in `catch_unwind`, and every plugin author should copy that.
//! * **Nothing is ever unloaded.** The `Library` handle is leaked for the
//!   life of the process, for the reason `crate::loaded` documents at length:
//!   `dlclose` under a live pointer is a use-after-free somewhere unrelated.
//!   Reloading loads afresh and leaks the old image; a session is not long
//!   enough for that to matter, and the preview made the same trade first.
//! * **Plugin strings are not translated.** The studio's own strings go
//!   through [`i18n`](crate::i18n); a plugin's words are the plugin's.
//!
//! [`i18n`]: crate::i18n

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use libloading::Library;

use crate::export::{Plan, Refusal, Step};
use crate::json::Json;
use crate::lint::Finding;
use crate::state::Severity;
use crate::task::Spec;

/// The ABI version this studio speaks.
///
/// A plugin's `vieww_plugin_abi` must return exactly this. One number, not a
/// range: there is no second version yet, and "compatible with 1 and 2" is a
/// claim nobody has had to test.
pub const ABI_VERSION: u32 = 1;

/// The symbols a plugin must export. Named here once so the loader and the
/// example plugin cannot drift apart, which is the whole of what an ABI is.
mod symbol {
    // All `pub(crate)`-visible through `Self::` uses inside this module's
    // parent only — the example plugin writes the same bytes by hand in its
    // own source, which is the ABI contract's own discipline (drift is what
    // the fingerprint check catches).
    pub(super) const ABI: &[u8] = b"vieww_plugin_abi";
    pub(super) const FINGERPRINT: &[u8] = b"vieww_plugin_fingerprint";
    pub(super) const MANIFEST: &[u8] = b"vieww_plugin_manifest";
    pub(super) const LINT: &[u8] = b"vieww_plugin_lint";
    pub(super) const PLAN: &[u8] = b"vieww_plugin_plan";
    pub(super) const RUN: &[u8] = b"vieww_plugin_run_command";
    pub(super) const FREE: &[u8] = b"vieww_plugin_free";
}

/// One lint rule a plugin offers, as its manifest describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintDescriptor {
    /// The name findings carry in the Problems panel's code column — shown so
    /// a rule can be recognised and argued with, exactly as the built-ins are.
    pub rule: String,
    /// One line saying what the rule is for, for the panel's rule list.
    pub description: String,
}

/// One palette command a plugin offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDescriptor {
    /// What the palette row says. Unique within the plugin; the studio keys
    /// the command by plugin name and title together.
    pub title: String,
    /// The dimmer line under the title.
    pub detail: String,
}

/// One export format a plugin offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatDescriptor {
    /// The stable id the studio passes back when planning — `"source-tarball"`,
    /// not the display title, so a plugin may reword a row without breaking
    /// saved settings.
    pub id: String,
    pub title: String,
    pub detail: String,
}

/// What a plugin's command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Replace the active buffer's whole text. Applied through
    /// [`Studio::edit`](crate::state::Studio::edit), so it is one undo step
    /// and drives the collab seam like any other edit.
    Replace(String),
    /// Say something in the notice bar.
    Notice(String),
    /// Nothing happened, and nothing should be said.
    None,
}

/// Why a library could not become a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// `dlopen` refused it.
    Dlopen(String),
    /// No `vieww_plugin_abi`, so there is no version to check.
    NoAbi,
    /// A version this studio does not speak.
    WrongAbi { studio: u32, plugin: u32 },
    /// No `vieww_plugin_fingerprint`, so there is nothing to check.
    NoFingerprint,
    /// The plugin was built against a different compilation of `vieww` than
    /// this studio links. Carries both numbers, for the same reason the
    /// preview's mismatch does: "still wrong" and "wrong in a new way" are
    /// different situations.
    AbiMismatch { studio: u64, plugin: u64 },
    /// No manifest symbol, so the studio cannot ask what the plugin offers.
    NoManifest,
    /// A manifest that is not the JSON this ABI specifies.
    BadManifest(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dlopen(why) => {
                write!(f, "could not load the plugin: {why}")
            }
            Self::NoAbi => write!(
                f,
                "the library has no `vieww_plugin_abi`, so it is not a studio plugin"
            ),
            Self::WrongAbi { studio, plugin } => write!(
                f,
                "the plugin speaks ABI version {plugin}; this studio speaks {studio}. \
                 Rebuild the plugin against this checkout."
            ),
            Self::NoFingerprint => write!(
                f,
                "the plugin has no `vieww_plugin_fingerprint`, so the studio cannot tell \
                 whether it was built from the same dependency graph"
            ),
            Self::AbiMismatch { studio, plugin } => write!(
                f,
                "the plugin was built against a different build of vieww than this studio \
                 links (studio {studio:#018x}, plugin {plugin:#018x}). Fix: build the plugin \
                 from the same workspace and the same feature set as the studio — \
                 `cargo build -p viewwstudio-plugin-example` from this checkout is the \
                 shape that works."
            ),
            Self::NoManifest => write!(
                f,
                "the plugin exports no `vieww_plugin_manifest`, so the studio cannot ask \
                 what it offers"
            ),
            Self::BadManifest(why) => write!(f, "the plugin's manifest is not valid: {why}"),
        }
    }
}

/// One of a plugin's lint rules, as the [`LintRule`] the lint pass merges
/// with its own.
///
/// A newtype rather than calling `Plugin::lint` straight from the pass, so
/// the trait's `rule` and `description` come from the manifest the loader
/// already parsed — the plugin side of the ABI has no way to answer "what
/// are you?" per call, and asking every frame would be a protocol the ABI
/// does not have.
#[derive(Debug, Clone, Copy)]
pub struct PluginLint<'a> {
    plugin: &'a Plugin,
    descriptor: &'a LintDescriptor,
}

impl crate::lint::LintRule for PluginLint<'_> {
    fn rule(&self) -> &str {
        &self.descriptor.rule
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn check(&self, source: &str) -> Vec<Finding> {
        self.plugin.lint(&self.descriptor.rule, source)
    }
}

/// A loaded plugin.
///
/// Constructed only by [`load`], which is the thing that checks the guards.
#[derive(Debug)]
pub struct Plugin {
    /// Leaked on purpose — see the module docs. Held, not dropped: the
    /// function pointers below are only sound while this handle is alive, and
    /// the handle is what never goes away.
    library: Library,
    /// What the manifest said the plugin is called.
    name: String,
    lints: Vec<LintDescriptor>,
    commands: Vec<CommandDescriptor>,
    formats: Vec<FormatDescriptor>,
}

impl Plugin {
    /// The plugin's own name, as its manifest declares it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The lint rules this plugin offers.
    #[must_use]
    pub fn lints(&self) -> &[LintDescriptor] {
        &self.lints
    }

    /// The palette commands this plugin offers.
    #[must_use]
    pub fn commands(&self) -> &[CommandDescriptor] {
        &self.commands
    }

    /// The export formats this plugin offers.
    #[must_use]
    pub fn formats(&self) -> &[FormatDescriptor] {
        &self.formats
    }

    /// Run one of the plugin's lint rules over `source`.
    ///
    /// An unknown `rule` name produces no findings rather than an error: the
    /// manifest and the rule set are one plugin's two halves, and a mismatch
    /// between them is the plugin's bug to read about, not the user's to
    /// click through.
    #[must_use]
    pub fn lint(&self, rule: &str, source: &str) -> Vec<Finding> {
        let Ok(rule) = CString::new(rule) else {
            return Vec::new();
        };
        // SAFETY: the pointer pair is `source`'s own bytes, borrowed for the
        // call; the plugin is forbidden from keeping or freeing them, and a
        // plugin that does is the plugin's defect, stated in the ABI.
        let raw = unsafe {
            type LintFn = unsafe extern "C" fn(*const c_char, *const c_char, usize) -> *mut c_char;
            let lint = self
                .library
                .get::<LintFn>(symbol::LINT)
                .expect("load checked every symbol");
            lint(rule.as_ptr(), source.as_ptr().cast(), source.len())
        };
        let findings = parse_findings(&borrowed(raw));
        // SAFETY: the pointer came from the plugin's allocator and the
        // plugin's `free` is the only thing that may release it.
        unsafe { free(&self.library, raw) };
        findings
    }

    /// Plan an export in format `id`, or refuse it.
    ///
    /// `root` is the project's directory and `name` its package name — the
    /// same two facts the built-in planner hands its own formats, and all a
    /// plugin is told about the project.
    pub fn plan(&self, id: &str, root: &Path, name: &str) -> Result<Plan, Refusal> {
        let Ok(id) = CString::new(id) else {
            return Err(Refusal::External(
                "the format id is not a valid C string".into(),
            ));
        };
        let Ok(name) = CString::new(name) else {
            return Err(Refusal::External(
                "the package name is not a valid C string".into(),
            ));
        };
        let root = root.to_string_lossy();
        // SAFETY: as `lint` — borrowed in, plugin-owned out.
        let raw = unsafe {
            type PlanFn = unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                usize,
                *const c_char,
            ) -> *mut c_char;
            let plan = self
                .library
                .get::<PlanFn>(symbol::PLAN)
                .expect("load checked every symbol");
            plan(id.as_ptr(), root.as_ptr().cast(), root.len(), name.as_ptr())
        };
        let name = name.to_string_lossy().into_owned();
        let plan = parse_plan(&borrowed(raw), root.as_ref(), &name);
        // SAFETY: as `lint`.
        unsafe { free(&self.library, raw) };
        // The plan's title is the row's title, not the package's name: it is
        // what the output panel attributes the run to, so it carries the
        // plugin's name and the format's, the way the built-ins carry theirs.
        let title = self
            .formats
            .iter()
            .find(|format| format.id == id.to_string_lossy())
            .map_or_else(|| self.name.clone(), |format| format.title.clone());
        plan.map(|mut plan| {
            plan.title = format!("{}: {}", self.name, title);
            plan
        })
    }

    /// Run one of the plugin's commands, handing it the text of the active
    /// buffer.
    ///
    /// The buffer's whole text, not a selection or a line: the ABI has no
    /// way to speak about spans, and "here is the document, give me back a
    /// document" is the one editing verb that cannot corrupt anything the
    /// studio keeps about the old one.
    #[must_use]
    pub fn run_command(&self, title: &str, source: &str) -> Outcome {
        let Ok(title) = CString::new(title) else {
            return Outcome::None;
        };
        // SAFETY: as `lint`.
        let raw = unsafe {
            type RunFn = unsafe extern "C" fn(*const c_char, *const c_char, usize) -> *mut c_char;
            let run = self
                .library
                .get::<RunFn>(symbol::RUN)
                .expect("load checked every symbol");
            run(title.as_ptr(), source.as_ptr().cast(), source.len())
        };
        let outcome = parse_outcome(&borrowed(raw));
        // SAFETY: as `lint`.
        unsafe { free(&self.library, raw) };
        outcome
    }
}

/// Load `library` as a plugin, checking every guard before anything is used.
///
/// # Errors
///
/// Every way a file in `plugins/` can fail to be a plugin this studio can
/// talk to — see [`LoadError`]. A failure here is reported per file and skips
/// that one plugin; it never takes down the others, because the directory is
/// a list somebody curates by hand and one bad file is one bad file.
pub fn load(library: &Path) -> Result<Plugin, LoadError> {
    // SAFETY: loading a library runs its initialisers. A plugin's
    // initialisers are its own to keep honest; nothing here calls anything
    // until the guards below have passed.
    let handle =
        unsafe { Library::new(library) }.map_err(|error| LoadError::Dlopen(error.to_string()))?;

    // The version first: nothing else about a foreign ABI is safe to ask.
    // SAFETY: returns a u32; it cannot unwind.
    let abi = unsafe {
        handle
            .get::<unsafe extern "C" fn() -> u32>(symbol::ABI)
            .map_err(|_| LoadError::NoAbi)?
    };
    // SAFETY: as above.
    let plugin_abi = unsafe { abi() };
    if plugin_abi != ABI_VERSION {
        return Err(LoadError::WrongAbi {
            studio: ABI_VERSION,
            plugin: plugin_abi,
        });
    }

    // Then the fingerprint — the preview's guard, asked of a plugin. Checked
    // before any manifest is read or any rule runs: see the module docs.
    // SAFETY: returns a u64; it cannot unwind.
    let fingerprint = unsafe {
        handle
            .get::<unsafe extern "C" fn() -> u64>(symbol::FINGERPRINT)
            .map_err(|_| LoadError::NoFingerprint)?
    };
    // SAFETY: as above.
    let plugin_fingerprint = unsafe { fingerprint() };
    let studio = crate::loaded::host_fingerprint();
    if plugin_fingerprint != studio {
        return Err(LoadError::AbiMismatch {
            studio,
            plugin: plugin_fingerprint,
        });
    }

    // Only now, with both guards passed, the manifest.
    // SAFETY: returns a plugin-owned pointer or null; freed below through the
    // plugin's own `free`, on the failure path and the success path alike.
    let manifest = unsafe {
        handle
            .get::<unsafe extern "C" fn() -> *mut c_char>(symbol::MANIFEST)
            .map_err(|_| LoadError::NoManifest)?
    };
    // SAFETY: as above.
    let raw = unsafe { manifest() };
    let text = borrowed(raw);
    let parsed = parse_manifest(&text);
    // SAFETY: the pointer is the plugin's and its `free` is the only release.
    unsafe { free(&handle, raw) };
    let (name, lints, commands, formats) = parsed.map_err(LoadError::BadManifest)?;

    // Every symbol the studio will ever call is resolved here, so a plugin
    // that is missing one fails at load with the name it lacks rather than
    // at first use with a panic — the panic would be ours, and the bug is
    // the plugin author's.
    for name in [symbol::LINT, symbol::PLAN, symbol::RUN, symbol::FREE] {
        // SAFETY: resolution only; nothing is called.
        unsafe {
            handle.get::<unsafe extern "C" fn()>(name).map_err(|_| {
                LoadError::BadManifest(format!("exports no `{}`", String::from_utf8_lossy(name)))
            })?;
        }
    }

    Ok(Plugin {
        library: handle,
        name,
        lints,
        commands,
        formats,
    })
}

/// The plugins loaded into this studio, plus the failures that did not load.
///
/// A type and not a `Vec` because the two halves are one fact: what the user
/// now has, and what the studio told them they did not get. The palette, the
/// Problems panel and the export sheet all read the first half; the output
/// panel is handed the second.
#[derive(Debug, Default)]
pub struct Registry {
    /// Loaded, in the order the directory listed them.
    pub plugins: Vec<Plugin>,
    /// One line per file that failed, already in the voice of the refusals.
    pub failures: Vec<String>,
}

impl Registry {
    /// No plugins and no failures — what the studio starts with, before
    /// anybody runs the reload command.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// The plugin called `name`, if it loaded.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|plugin| plugin.name == name)
    }

    /// Every palette command, as `(plugin name, command)` pairs.
    #[must_use]
    pub fn palette_commands(&self) -> Vec<(&str, &CommandDescriptor)> {
        self.plugins
            .iter()
            .flat_map(|plugin| {
                plugin
                    .commands
                    .iter()
                    .map(|command| (plugin.name.as_str(), command))
            })
            .collect()
    }

    /// Every lint rule, as the [`LintRule`]s the lint pass merges with its
    /// own.
    #[must_use]
    pub fn lint_rules(&self) -> Vec<PluginLint<'_>> {
        self.plugins
            .iter()
            .flat_map(|plugin| {
                plugin
                    .lints
                    .iter()
                    .map(|descriptor| PluginLint { plugin, descriptor })
            })
            .collect()
    }

    /// Load every plugin in `dir` — the studio passes
    /// `<data_dir>/plugins`.
    ///
    /// Every file with a shared-library suffix is tried; the three suffixes
    /// are all accepted because which one a person's build produced is their
    /// platform's habit, not a claim about their plugin, and the loader — not
    /// the studio — is the thing that knows whether the bytes are loadable.
    ///
    /// A directory that does not exist is not a failure: it is a studio where
    /// nobody has installed a plugin yet, which is most of them.
    #[must_use]
    pub fn discover(dir: &Path) -> Self {
        let mut registry = Self::default();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return registry;
        };
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("so") | Some("dylib") | Some("dll")
                )
            })
            .collect();
        // Directory order, not filesystem order, so the output panel lists
        // the same plugins in the same order on every machine.
        paths.sort();
        for path in paths {
            match load(&path) {
                Ok(plugin) => registry.plugins.push(plugin),
                Err(error) => registry.failures.push(format!(
                    "{}: {error}",
                    path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned()
                    )
                )),
            }
        }
        registry
    }
}

// ---------------------------------------------------------------------------
// The JSON the plugin speaks, parsed into the studio's own types
// ---------------------------------------------------------------------------

/// A plugin-owned pointer as text, empty when there was no pointer at all.
fn borrowed(raw: *const c_char) -> String {
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: the pointer is a valid NUL-terminated string the plugin built,
    // read only here and released by `free` in the caller.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

/// Hand a plugin-owned pointer back to the plugin's own `free`.
///
/// # Safety
///
/// `raw` must have been returned by a function of `library` and not already
/// freed. Null is allowed and ignored.
unsafe fn free(library: &Library, raw: *mut c_char) {
    if raw.is_null() {
        return;
    }
    // SAFETY: the contract above is the whole of the requirement — the
    // plugin's allocator is the only one that may release its memory.
    if let Ok(free) = library.get::<unsafe extern "C" fn(*mut c_char)>(symbol::FREE) {
        // SAFETY: as above.
        unsafe { free(raw) };
    }
}

/// What a manifest contributes: the plugin's name plus its three surfaces.
type Manifest = (
    String,
    Vec<LintDescriptor>,
    Vec<CommandDescriptor>,
    Vec<FormatDescriptor>,
);

/// The manifest: `{"name":…, "lints":[…], "commands":[…], "formats":[…]}`.
fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let json = Json::parse(text).map_err(|error| error.to_string())?;
    let name = json
        .str_field("name")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "no `name` string".to_string())?
        .to_string();

    let mut lints = Vec::new();
    if let Some(rules) = json.get("lints").and_then(Json::as_array) {
        for rule in rules {
            lints.push(LintDescriptor {
                rule: rule
                    .str_field("rule")
                    .filter(|r| !r.is_empty())
                    .ok_or_else(|| "a lint with no `rule`".to_string())?
                    .to_string(),
                description: rule
                    .str_field("description")
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }

    let mut commands = Vec::new();
    if let Some(entries) = json.get("commands").and_then(Json::as_array) {
        for command in entries {
            commands.push(CommandDescriptor {
                title: command
                    .str_field("title")
                    .filter(|t| !t.is_empty())
                    .ok_or_else(|| "a command with no `title`".to_string())?
                    .to_string(),
                detail: command.str_field("detail").unwrap_or_default().to_string(),
            });
        }
    }

    let mut formats = Vec::new();
    if let Some(entries) = json.get("formats").and_then(Json::as_array) {
        for format in entries {
            formats.push(FormatDescriptor {
                id: format
                    .str_field("id")
                    .filter(|i| !i.is_empty())
                    .ok_or_else(|| "a format with no `id`".to_string())?
                    .to_string(),
                title: format
                    .str_field("title")
                    .filter(|t| !t.is_empty())
                    .ok_or_else(|| "a format with no `title`".to_string())?
                    .to_string(),
                detail: format.str_field("detail").unwrap_or_default().to_string(),
            });
        }
    }

    Ok((name, lints, commands, formats))
}

/// The lint findings: `[{"rule":…, "severity":…, "message":…, "line":…,
/// "column":…}]`.
///
/// Unparseable output is no findings rather than an error, for the same
/// reason an unparseable file is no diagnostics in `lint::check`: two
/// producers saying the same thing in different words is how a Problems panel
/// becomes noise, and a plugin that answers nonsense has already been told.
fn parse_findings(text: &str) -> Vec<Finding> {
    let Ok(json) = Json::parse(text) else {
        return Vec::new();
    };
    let Some(array) = json.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for finding in array {
        let Some(rule) = finding.str_field("rule") else {
            continue;
        };
        out.push(Finding {
            rule: rule.to_string(),
            severity: match finding.str_field("severity") {
                Some("error") => Severity::Error,
                _ => Severity::Warning,
            },
            message: finding.str_field("message").unwrap_or_default().to_string(),
            // The ABI says a number (the reference plugin emits numbers),
            // but a string that parses is accepted too — hand-written JSON
            // from a plugin is the one place the studio benefits from
            // leniency that cannot corrupt anything.
            line: finding
                .get("line")
                .and_then(Json::as_u32)
                .or_else(|| finding.str_field("line").and_then(|l| l.parse().ok()))
                .unwrap_or(1),
            column: finding
                .get("column")
                .and_then(Json::as_u32)
                .or_else(|| finding.str_field("column").and_then(|c| c.parse().ok()))
                .unwrap_or(1),
        });
    }
    out
}

/// The plan answer: `{"refusal": "…"}` or `{"steps": […], "artefact": "…"}`.
///
/// A step is `{"label":…, "program":…, "args":[…], "cwd":…, "why":…}` where
/// `cwd` is relative to the project root and `why` is the one line the export
/// sheet previews. Absolute step paths come from the plugin as they are.
fn parse_plan(text: &str, root: &str, _name: &str) -> Result<Plan, Refusal> {
    let json = match Json::parse(text) {
        Ok(json) => json,
        Err(_) => {
            return Err(Refusal::External(
                "the plugin's plan was not valid JSON".into(),
            ))
        }
    };
    if let Some(refusal) = json.str_field("refusal") {
        return Err(Refusal::External(refusal.to_string()));
    }
    let Some(steps) = json.get("steps").and_then(Json::as_array) else {
        return Err(Refusal::External("the plugin's plan has no steps".into()));
    };
    let root = Path::new(root);
    let mut parsed = Vec::new();
    for step in steps {
        let Some(label) = step.str_field("label") else {
            continue;
        };
        let Some(program) = step.str_field("program") else {
            continue;
        };
        // `cwd` is relative to the root the studio handed the plugin; absent
        // means the root itself, which is what every built-in step uses.
        let cwd = step
            .str_field("cwd")
            .map_or_else(|| root.to_path_buf(), |cwd| root.join(cwd));
        let mut spec = Spec::new(label, program, cwd);
        if let Some(args) = step.get("args").and_then(Json::as_array) {
            for arg in args {
                if let Some(arg) = arg.as_str() {
                    spec = spec.arg(arg);
                }
            }
        }
        parsed.push(Step {
            spec,
            why: step.str_field("why").unwrap_or_default().to_string(),
        });
    }
    if parsed.is_empty() {
        return Err(Refusal::External("the plugin's plan has no steps".into()));
    }
    // The artefact is what the run reports at the end; a plan that cannot
    // name its own output would "succeed" and point at nothing.
    let Some(artefact) = json.str_field("artefact") else {
        return Err(Refusal::External(
            "the plugin's plan has no artefact".into(),
        ));
    };
    Ok(Plan {
        title: String::new(),
        steps: parsed,
        artefact: PathBuf::from(artefact),
    })
}

/// The command answer: `{"replace": "…"}`, `{"notice": "…"}` or nothing.
fn parse_outcome(text: &str) -> Outcome {
    let Ok(json) = Json::parse(text) else {
        return Outcome::None;
    };
    if let Some(replace) = json.str_field("replace") {
        return Outcome::Replace(replace.to_string());
    }
    if let Some(notice) = json.str_field("notice") {
        return Outcome::Notice(notice.to_string());
    }
    Outcome::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_names_the_plugin_and_lists_what_it_offers() {
        let manifest = r#"{
            "name": "toolbelt",
            "lints": [{"rule": "todo-comment", "description": "TODOs left behind"}],
            "commands": [{"title": "Count TODOs", "detail": "toolbelt"}],
            "formats": [{"id": "source-tarball", "title": "Source tarball", "detail": "tar"}]
        }"#;
        let (name, lints, commands, formats) =
            parse_manifest(manifest).expect("the reference manifest");
        assert_eq!(name, "toolbelt");
        assert_eq!(lints.len(), 1);
        assert_eq!(lints[0].rule, "todo-comment");
        assert_eq!(commands[0].title, "Count TODOs");
        assert_eq!(formats[0].id, "source-tarball");
    }

    #[test]
    fn a_manifest_without_a_name_is_refused() {
        // The name is what the palette, the Problems panel and the output
        // panel all attribute the plugin's work by; "anonymous plugin" is a
        // line nobody can act on.
        assert!(parse_manifest("{}").is_err());
        assert!(parse_manifest(r#"{"name": ""}"#).is_err());
        assert!(parse_manifest("not json").is_err());
    }

    #[test]
    fn findings_carry_their_rule_and_position() {
        let findings = parse_findings(
            r#"[{"rule":"todo-comment","severity":"warning","message":"unfinished","line":3,"column":5}]"#,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "todo-comment");
        assert_eq!(findings[0].line, 3);
        assert_eq!(findings[0].column, 5);
    }

    #[test]
    fn an_error_severity_is_an_error_and_everything_else_is_a_warning() {
        let findings = parse_findings(
            r#"[{"rule":"a","severity":"error","message":"m","line":1,"column":1},
                {"rule":"b","severity":"nonsense","message":"m","line":1,"column":1}]"#,
        );
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[1].severity, Severity::Warning);
    }

    #[test]
    fn nonsense_from_a_plugin_is_no_findings_rather_than_an_error() {
        assert!(parse_findings("").is_empty());
        assert!(parse_findings("null").is_empty());
        assert!(parse_findings(r#"[{"no":"rule"}]"#).is_empty());
    }

    #[test]
    fn a_plan_becomes_steps_with_the_root_as_their_cwd() {
        let plan = parse_plan(
            r#"{"steps":[
                {"label":"Archive","program":"tar","args":["-czf","out.tgz","src"],"why":"zips the source"},
                {"label":"Nested","program":"ls","cwd":"sub"}
            ],"artefact":"/tmp/out.tgz"}"#,
            "/project",
            "app",
        )
        .expect("a plan");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].spec.program, "tar");
        assert_eq!(plan.steps[0].spec.args, vec!["-czf", "out.tgz", "src"]);
        assert_eq!(plan.steps[0].spec.dir, PathBuf::from("/project"));
        assert_eq!(plan.steps[1].spec.dir, PathBuf::from("/project/sub"));
        assert_eq!(plan.artefact, PathBuf::from("/tmp/out.tgz"));
        assert_eq!(plan.steps[0].why, "zips the source");
    }

    #[test]
    fn a_refusal_is_returned_in_the_plugins_own_words() {
        let refusal = parse_plan(r#"{"refusal":"tar is not installed"}"#, "/p", "app")
            .expect_err("the plugin refused");
        assert_eq!(
            refusal,
            Refusal::External("tar is not installed".to_string())
        );
    }

    #[test]
    fn a_plan_with_no_steps_or_no_artefact_is_refused_rather_than_run() {
        // A plan that does nothing would report an artefact that was never
        // produced — the export that "succeeded" and made nothing.
        assert!(parse_plan(r#"{"steps":[]}"#, "/p", "app").is_err());
        assert!(parse_plan("{}", "/p", "app").is_err());
        // Steps but no artefact: the run would finish with nothing to show.
        assert!(parse_plan(r#"{"steps":[{"label":"a","program":"b"}]}"#, "/p", "app").is_err());
    }

    #[test]
    fn a_command_answer_is_a_replace_a_notice_or_nothing() {
        assert_eq!(
            parse_outcome(r#"{"replace":"new text"}"#),
            Outcome::Replace("new text".to_string())
        );
        assert_eq!(
            parse_outcome(r#"{"notice":"3 TODOs"}"#),
            Outcome::Notice("3 TODOs".to_string())
        );
        assert_eq!(parse_outcome("null"), Outcome::None);
        assert_eq!(parse_outcome("nonsense"), Outcome::None);
    }

    #[test]
    fn a_missing_plugins_directory_is_no_plugins_and_no_failures() {
        // Most studios: nobody has written a plugin yet. Not an error, and
        // especially not an error every frame.
        let registry = Registry::discover(Path::new("/does/not/exist/plugins"));
        assert!(registry.plugins.is_empty());
        assert!(registry.failures.is_empty());
    }
}
