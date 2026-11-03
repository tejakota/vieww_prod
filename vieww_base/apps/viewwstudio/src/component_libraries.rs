//! ST-05's third verb: components shared **across projects and people**,
//! through a git repository.
//!
//! The local registry (`crate::components`) is one machine's shelf. A
//! *component library* is the team's: a git repository laid out exactly like
//! the registry — one directory per component, `<Name>/component.say` — that
//! the studio clones under its data directory and lists in the Snippets view
//! beside the local components. Scaffolding from a library component is the
//! same act as scaffolding from a local one (the component is Say source,
//! compiled into the project that uses it, so there is no ABI between the
//! library and the project); publishing copies a local component into a
//! library, commits, and pushes.
//!
//! # Why git, and why the user's own
//!
//! Every team that would share a `Card` already has somewhere to put a
//! repository, credentials that reach it, and a review flow on top. A
//! registry server would duplicate all three badly. So the studio drives the
//! user's own `git` — the same binary, the same credential helper, the same
//! SSH keys a terminal would use — and says what git said when it fails. A
//! push that is refused (no rights, a branch protection) leaves the commit in
//! the clone and says so: nothing is lost, and the next sync carries it.
//!
//! # Layout on disk
//!
//! `<data_dir>/component-libraries/<slug>/` per library, where the slug is
//! derived from the URL, so adding the same URL twice is a sync rather than a
//! second clone. A library is any directory there with a `.git` inside; its
//! URL is the clone's `origin`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::components::{registry_components, write_registry_entry, Component};
use crate::state::Studio;

/// Where cloned libraries live.
#[must_use]
pub fn libraries_dir() -> PathBuf {
    crate::about::data_dir().join("component-libraries")
}

/// One cloned library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// What it was cloned from — the clone's `origin`.
    pub url: String,
    /// The clone.
    pub dir: PathBuf,
}

impl Library {
    /// A short name for the Snippets view: the URL's last path segment,
    /// without `.git`.
    #[must_use]
    pub fn name(&self) -> String {
        let tail = self
            .url
            .trim_end_matches('/')
            .rsplit(['/', ':'])
            .next()
            .unwrap_or(&self.url);
        tail.trim_end_matches(".git").to_owned()
    }

    /// The components this library holds, by the registry's own reading.
    #[must_use]
    pub fn components(&self) -> Vec<Component> {
        registry_components(&self.dir)
    }
}

/// Why `url` will not do as a library address, if it will not.
///
/// Deliberately loose about *what* git accepts (https, ssh, `git@host:path`,
/// a local path or `file://` — git is the judge), strict about what would
/// turn the argument into something else: whitespace, a leading `-` (an
/// option to git, not an address), and nothing at all.
#[must_use]
pub fn url_error(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return Some("A repository address is needed.".to_owned());
    }
    if url.starts_with('-') {
        return Some("That reads as an option to git, not an address.".to_owned());
    }
    if url.chars().any(char::is_whitespace) {
        return Some("An address cannot contain spaces.".to_owned());
    }
    None
}

/// The directory name a URL clones into: stable, filesystem-safe, and the
/// same for the same URL however it is spelled at the end (`/`, `.git`).
#[must_use]
pub fn slug(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let mut out: String = trimmed
        .chars()
        // Case kept: many git hosts tell `Team/UI` and `team/ui` apart, and
        // folding them would send one repository's sync into the other's clone.
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_owned()
}

/// Run git in `dir` (or nowhere in particular) and return its stdout, or a
/// sentence with what it said on stderr.
fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    // Never stop to ask: the studio has no terminal to answer a prompt in,
    // and a credential prompt that hangs the clone forever is the worst way
    // for "no rights to that repository" to be reported.
    command.env("GIT_TERMINAL_PROMPT", "0");
    let output = command
        .args(args)
        .output()
        .map_err(|error| format!("git could not be run ({error}) — is it installed?"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim();
        Err(if said.is_empty() {
            format!(
                "git {} failed ({})",
                args.first().unwrap_or(&""),
                output.status
            )
        } else {
            said.lines().last().unwrap_or(said).to_owned()
        })
    }
}

/// Every library cloned under `root`, sorted by name.
#[must_use]
pub fn libraries(root: &Path) -> Vec<Library> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Library> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.join(".git").exists())
        .filter_map(|dir| {
            let url = git(Some(&dir), &["remote", "get-url", "origin"]).ok()?;
            Some(Library { url, dir })
        })
        .collect();
    out.sort_by_key(Library::name);
    out
}

/// Clone `url` under `root`, or bring an existing clone of it up to date.
///
/// # Errors
///
/// A sentence: the address is unusable, or what git said.
pub fn add_library(root: &Path, url: &str) -> Result<Library, String> {
    if let Some(error) = url_error(url) {
        return Err(error);
    }
    let url = url.trim();
    let dir = root.join(slug(url));
    if dir.join(".git").exists() {
        sync_library(&dir)?;
    } else {
        std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
        let target = dir.to_string_lossy().into_owned();
        git(None, &["clone", "--quiet", "--", url, &target])?;
    }
    Ok(Library {
        url: url.to_owned(),
        dir,
    })
}

/// Pull a clone up to date — fast-forward only, so a library somebody
/// rewrote is reported rather than merged into a mess.
///
/// # Errors
///
/// What git said.
pub fn sync_library(dir: &Path) -> Result<(), String> {
    // An empty repository (just created, nothing pushed) has no upstream to
    // pull; that is not a failure, it is a library with no components yet.
    if git(Some(dir), &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        return Ok(());
    }
    git(Some(dir), &["pull", "--quiet", "--ff-only"]).map(|_| ())
}

/// What a publish did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Published {
    /// Committed and pushed.
    Pushed,
    /// Committed in the clone; the push was refused, with git's reason. The
    /// next successful sync or publish carries the commit.
    CommittedOnly(String),
    /// The library already held exactly this component; nothing to commit.
    Unchanged,
}

/// Copy `component` into `library`, commit, and push.
///
/// # Errors
///
/// A sentence, when the component could not be written or committed. A
/// refused *push* is not an error — see [`Published::CommittedOnly`].
pub fn publish(component: &Component, library: &Library) -> Result<Published, String> {
    // Up to date first, so the commit lands on top of what the team has.
    sync_library(&library.dir)?;
    write_registry_entry(
        &library.dir,
        &component.name,
        &component.from,
        &component.source,
    )
    .map_err(|error| error.to_string())?;
    let path = format!("{}/component.say", component.name);
    git(Some(&library.dir), &["add", "--", &path])?;
    let staged = git(Some(&library.dir), &["diff", "--cached", "--name-only"])?;
    if staged.is_empty() {
        return Ok(Published::Unchanged);
    }
    let message = format!("Add component {} (from {})", component.name, component.from);
    git(
        Some(&library.dir),
        &[
            // A clone made by the studio may sit on a machine with no git
            // identity configured; a commit refused for that would be a
            // confusing way to fail a publish. The user's own identity wins
            // when there is one (these only fill gaps).
            "-c",
            "user.useConfigOnly=false",
            "commit",
            "--quiet",
            "-m",
            &message,
        ],
    )
    .or_else(|_| {
        git(
            Some(&library.dir),
            &[
                "-c",
                "user.name=vieww Studio",
                "-c",
                "user.email=studio@vieww.invalid",
                "commit",
                "--quiet",
                "-m",
                &message,
            ],
        )
    })?;
    match git(Some(&library.dir), &["push", "--quiet", "origin", "HEAD"]) {
        Ok(_) => Ok(Published::Pushed),
        Err(why) => Ok(Published::CommittedOnly(why)),
    }
}

impl Studio {
    /// The cloned libraries, for the Snippets view.
    #[must_use]
    pub fn component_libraries(&self) -> Vec<Library> {
        libraries(&libraries_dir())
    }

    /// Open the prompt that asks for a library's address.
    pub fn ask_for_component_library(&self) {
        self.ask_for_name(crate::state::NameKind::ComponentLibrary, &libraries_dir());
    }

    /// Clone (or sync) the library at `url`, and say what happened.
    ///
    /// # Errors
    ///
    /// The sentence for the prompt's footnote.
    pub fn add_component_library(&self, url: &str) -> Result<String, String> {
        let library = add_library(&libraries_dir(), url)?;
        let count = library.components().len();
        Ok(format!(
            "Added the {} component library — {count} component{}",
            library.name(),
            if count == 1 { "" } else { "s" }
        ))
    }

    /// Pull every library up to date.
    pub fn sync_component_libraries(&self) {
        let libraries = self.component_libraries();
        if libraries.is_empty() {
            self.notify("No component libraries yet — add one by its git address first.");
            return;
        }
        let failed: Vec<String> = libraries
            .iter()
            .filter_map(|library| {
                sync_library(&library.dir)
                    .err()
                    .map(|why| format!("{}: {why}", library.name()))
            })
            .collect();
        if failed.is_empty() {
            self.notify(&format!(
                "Component libraries up to date ({})",
                libraries.len()
            ));
        } else {
            self.notify(&format!("Could not sync {}", failed.join("; ")));
        }
    }

    /// Publish a component into a library, and say what happened.
    pub fn publish_component(&self, component: &Component, library: &Library) {
        let message = match publish(component, library) {
            Ok(Published::Pushed) => format!(
                "Published <{}> to {}",
                component.name,
                library.name()
            ),
            Ok(Published::CommittedOnly(why)) => format!(
                "Committed <{}> to {}, but the push was refused ({why}) — it goes up with the next sync",
                component.name,
                library.name()
            ),
            Ok(Published::Unchanged) => format!(
                "{} already has this <{}>",
                library.name(),
                component.name
            ),
            Err(why) => format!("Could not publish <{}>: {why}", component.name),
        };
        self.notify(&message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_judged_by_git_except_for_what_would_change_the_command() {
        assert!(url_error("").is_some());
        assert!(url_error("--upload-pack=evil").is_some());
        assert!(url_error("https://x/y z").is_some());
        assert!(url_error("https://github.com/team/components.git").is_none());
        assert!(url_error("git@github.com:team/components.git").is_none());
        assert!(url_error("/srv/git/components").is_none());
    }

    #[test]
    fn the_same_address_is_the_same_clone() {
        assert_eq!(
            slug("https://github.com/team/components.git"),
            slug("https://github.com/team/components/")
        );
        assert_ne!(slug("https://h/Team/ui"), slug("https://h/team/ui"));
        assert_eq!(slug("git@github.com:team/ui.git"), "git-github-com-team-ui");
        let library = Library {
            url: "git@github.com:team/ui-kit.git".to_owned(),
            dir: PathBuf::new(),
        };
        assert_eq!(library.name(), "ui-kit");
    }
}
