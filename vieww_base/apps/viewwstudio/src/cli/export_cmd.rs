//! `export` — the export sheet, run for real.
//!
//! The studio's export is a *planner* and a *runner*: [`export::plan`]
//! turns a format into a list of [`Spec`]s (or refuses, naming the missing
//! requirement), and the studio's jobs queue runs them. This module is the
//! same two halves with a terminal where the queue was — `run_spec` runs
//! each step in the foreground with its output inherited, because a person
//! watching a `cargo build` wants its progress, not a spinner.
//!
//! Every format the sheet has is a format this command has:
//!
//! * `desktop` — an optimised binary for this machine.
//! * `mac-app` — a `.app` bundle (macOS only, like the sheet's).
//! * `deb` — a `.deb` package (Linux only).
//! * `windows` — a `.exe`, cross-compiled when the host is not Windows.
//! * `web` — the interactive wasm32 bundle. `viewwstudio-cli web` is this
//!   one, kept as its own spelling.
//! * `apk` / `apk-release` — the debug-signed and release-signed Android
//!   packages; the release one reads its keystore from the same
//!   `VIEWW_KEYSTORE*` environment variables the sheet's prompt feeds.
//! * `ios-simulator` / `ipa` — the simulator `.app` and the device `.ipa`.
//!
//! `--all` runs every format the machine can *plan*, reporting each refusal
//! as a skip rather than a failure — a Linux box without Xcode skipping
//! `ipa` is the expected answer, not an error. `--list` builds nothing: it
//! prints each format with the first refusal its own plan would make here,
//! which is the export sheet's pre-flight, as text.

use std::path::Path;

use crate::export;
use crate::toolchains::Env;

use super::{package_name, project_at, run_spec};

/// A format's command-line name — the spellings `--format` takes, mapping
/// onto the studio's own [`export::Format`] one for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportName {
    Desktop,
    MacApp,
    Deb,
    Windows,
    Web,
    Apk,
    ApkRelease,
    IosSimulator,
    Ipa,
}

impl ExportName {
    /// The spelling, as `--format` takes it and `--list` shows it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::MacApp => "mac-app",
            Self::Deb => "deb",
            Self::Windows => "windows",
            Self::Web => "web",
            Self::Apk => "apk",
            Self::ApkRelease => "apk-release",
            Self::IosSimulator => "ios-simulator",
            Self::Ipa => "ipa",
        }
    }

    /// The studio's own format, which is where the plan comes from.
    #[must_use]
    pub fn format(self) -> export::Format {
        match self {
            Self::Desktop => export::Format::DesktopBinary,
            Self::MacApp => export::Format::MacAppBundle,
            Self::Deb => export::Format::LinuxDeb,
            Self::Windows => export::Format::WindowsExe,
            Self::Web => export::Format::Web,
            Self::Apk => export::Format::AndroidApk,
            Self::ApkRelease => export::Format::AndroidReleaseApk,
            Self::IosSimulator => export::Format::IosSimulatorApp,
            Self::Ipa => export::Format::IosIpa,
        }
    }

    /// Parse a `--format` value, refusing with the whole list — the same
    /// "error as the fix" rule the device names follow.
    ///
    /// # Errors
    ///
    /// The name is no format's.
    pub fn parse(text: &str) -> Result<Self, String> {
        let all = [
            Self::Desktop,
            Self::MacApp,
            Self::Deb,
            Self::Windows,
            Self::Web,
            Self::Apk,
            Self::ApkRelease,
            Self::IosSimulator,
            Self::Ipa,
        ];
        all.iter()
            .find(|format| format.as_str() == text.trim().to_lowercase())
            .copied()
            .ok_or_else(|| {
                format!(
                    "no export format called \"{text}\" — the formats are: {}",
                    all.iter()
                        .map(|format| format.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    /// Every format, in the order the export sheet lists them.
    pub const ALL: [Self; 9] = [
        Self::Desktop,
        Self::MacApp,
        Self::Deb,
        Self::Windows,
        Self::Web,
        Self::Apk,
        Self::ApkRelease,
        Self::IosSimulator,
        Self::Ipa,
    ];
}

/// `export`: plan one format and run it, run them all, or list what each
/// would need.
///
/// # Errors
///
/// A refusal (named, with the install line), or a step that failed — for
/// `--all`, only when *nothing* succeeded; the skips are the report.
pub fn run_export(
    project: Option<&Path>,
    format: Option<ExportName>,
    all: bool,
    list: bool,
) -> Result<String, String> {
    let root = resolve_project(project).ok_or_else(|| {
        "export needs a project — pass one, or stand inside it (the directory with the Cargo.toml)".to_owned()
    })?;
    let name = package_name(&root).ok_or_else(|| {
        format!(
            "{} has no Cargo.toml with a package name in it — export exports a project, not a file",
            root.display()
        )
    })?;

    if list {
        return Ok(listing(&root, &name));
    }

    if all {
        let env = Env::host();
        let mut lines = Vec::new();
        let mut exported = 0;
        for format in ExportName::ALL {
            match export::plan(&env, &root, &name, format.format()) {
                Ok(plan) => {
                    for step in &plan.steps {
                        run_spec(&step.spec).map_err(|why| {
                            format!("{} — {why} ({})", step.spec.label, format.as_str())
                        })?;
                    }
                    lines.push(format!("{} — {}", format.as_str(), plan.artefact.display()));
                    exported += 1;
                }
                Err(refusal) => {
                    lines.push(format!("skipped {} — {refusal}", format.as_str()));
                }
            }
        }
        return if exported == 0 {
            Err(format!(
                "nothing to export — every format refused:\n{}",
                lines.join("\n")
            ))
        } else {
            Ok(format!(
                "exported {exported} of {} formats:\n{}",
                ExportName::ALL.len(),
                lines.join("\n")
            ))
        };
    }

    // One format, named — the `web` spelling of this same path.
    let format = format.expect("parse guarantees a format without --all/--list");
    run_one(&root, &name, format)
}

/// `web`: the one format that is also a page, kept as its own command
/// because "give me the interactive web app" is a sentence people say
/// without the word "format" in it.
///
/// # Errors
///
/// A refusal (named, with the install lines), or a step that failed.
pub fn run_web(project: &Path) -> Result<String, String> {
    let name = package_name(project).ok_or_else(|| {
        format!(
            "{} has no Cargo.toml with a package name in it — web exports a project, not a file",
            project.display()
        )
    })?;
    run_one(project, &name, ExportName::Web)
}

/// Plan one format and run its steps, the way the export sheet previews
/// them and then runs them.
fn run_one(root: &Path, name: &str, format: ExportName) -> Result<String, String> {
    let env = Env::host();
    let plan = export::plan(&env, root, name, format.format())
        .map_err(|refusal| format!("{} — {refusal}", format.as_str()))?;
    println!("{} — {} steps", plan.title, plan.steps.len());
    for step in &plan.steps {
        println!("  {} — {}", step.spec.label, step.spec.command_line());
    }
    for step in &plan.steps {
        run_spec(&step.spec).map_err(|why| format!("{} — {why}", step.spec.label))?;
    }
    Ok(format!(
        "finished ({}) — the artefact is {}",
        format.as_str(),
        plan.artefact.display()
    ))
}

/// The export sheet's pre-flight, as text: every format, what it makes, and
/// the first refusal its own plan would make on this machine — which is the
/// honest question, not "is the toolchain present": `mac-app` on Linux has
/// every requirement the toolchain report asks for and is still impossible,
/// and only the plan says so. Nothing builds; a plan is a planner.
fn listing(root: &Path, name: &str) -> String {
    let env = Env::host();
    let mut out = format!("what {} could export, from this machine:\n", root.display());
    for format in ExportName::ALL {
        let line = match export::plan(&env, root, name, format.format()) {
            Ok(plan) => format!("ready ({} steps)", plan.steps.len()),
            Err(refusal) => format!("{refusal}"),
        };
        out.push_str(&format!(
            "  {:<14} {} — {}\n",
            format.as_str(),
            format.format().detail(),
            line
        ));
    }
    out.push_str("  --format <name> runs one; --all runs every one that is ready.\n");
    out
}

/// The project to export: the one named, or the one around the current
/// directory — walked up to the `Cargo.toml` either way, because "the
/// project" is the folder cargo owns, and both a person standing in
/// `src/screens/` and one naming the root mean the same project.
fn resolve_project(explicit: Option<&Path>) -> Option<std::path::PathBuf> {
    match explicit {
        Some(path) => project_at(path),
        None => std::env::current_dir().ok().as_deref().and_then(project_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_name_round_trips() {
        for format in ExportName::ALL {
            assert_eq!(ExportName::parse(format.as_str()).unwrap(), format);
        }
    }

    #[test]
    fn format_names_are_the_spellings_the_usage_promises() {
        for (name, _) in [
            ("desktop", ExportName::Desktop),
            ("mac-app", ExportName::MacApp),
            ("deb", ExportName::Deb),
            ("windows", ExportName::Windows),
            ("web", ExportName::Web),
            ("apk", ExportName::Apk),
            ("apk-release", ExportName::ApkRelease),
            ("ios-simulator", ExportName::IosSimulator),
            ("ipa", ExportName::Ipa),
        ] {
            assert_eq!(ExportName::parse(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn an_unknown_format_lists_the_known_ones() {
        let error = ExportName::parse("floppy").unwrap_err();
        assert!(error.contains("floppy"), "{error}");
        assert!(error.contains("apk-release"), "{error}");
        assert!(error.contains("ios-simulator"), "{error}");
    }

    #[test]
    fn case_does_not_matter() {
        assert_eq!(ExportName::parse(" Web ").unwrap(), ExportName::Web);
    }
}
