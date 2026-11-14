//! `screen`, `widget`, `assets` — the project commands.
//!
//! # What these commands are
//!
//! The studio's sidebar, minus the sidebar:
//!
//! * `screen`/`widget` are **New Screen from Template** and **New Widget
//!   from Template**, the same [`scaffold::scaffold`] templates, written
//!   the same way the dialog writes them — file under `src/screens/`, the
//!   module line appended to `src/screens/mod.rs` (a Rust screen that
//!   exists on disk and not in `mod.rs` is a file the project does not
//!   have, which is the failure this command would otherwise create every
//!   single time).
//! * `assets` is the **Assets sidebar** as a listing: the project's
//!   `assets/` directory, which is exactly what [`WorkspaceBundle`]
//!   resolves `an image "…"` against, with sizes and a note about where
//!   the resolution order looks next.
//!
//! [`WorkspaceBundle`]: crate::assets::WorkspaceBundle

use std::path::{Path, PathBuf};

use crate::scaffold::{self, Template};

use super::project_at;

/// `screen`: a new screen file from the template, into the project around
/// the current directory (or the one named, when the command grows a path
/// argument — today the studio's dialog has a workspace open, and the CLI's
/// equivalent is standing in one).
///
/// # Errors
///
/// No project around the current directory, a name the module system would
/// refuse, or the file already exists — the template never overwrites, the
/// same rule the dialog follows.
pub fn run_screen(name: &str) -> Result<String, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot tell where you are standing — {error}"))?;
    let root = project_at(&cwd).ok_or_else(|| {
        "a screen goes into a project — stand inside one (the directory with the Cargo.toml)"
            .to_owned()
    })?;
    run_template_in(&root, Template::Screen, name)
}

/// [`run_screen`], with the project named rather than found — the spelling
/// a script wants when the project is not the directory it stands in.
///
/// # Errors
///
/// As [`run_screen`].
pub fn run_screen_from(root: &Path, name: &str) -> Result<String, String> {
    run_template_in(root, Template::Screen, name)
}

/// `widget`: a new widget file from the template.
///
/// # Errors
///
/// As [`run_screen`].
pub fn run_widget(name: &str) -> Result<String, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot tell where you are standing — {error}"))?;
    let root = project_at(&cwd).ok_or_else(|| {
        "a widget goes into a project — stand inside one (the directory with the Cargo.toml)"
            .to_owned()
    })?;
    run_template_in(&root, Template::Widget, name)
}

/// [`run_widget`], with the project named rather than found.
///
/// # Errors
///
/// As [`run_widget`].
pub fn run_widget_from(root: &Path, name: &str) -> Result<String, String> {
    run_template_in(root, Template::Widget, name)
}

/// The half of New Screen/New Widget that writes: the template, the file,
/// and the module line — `state/editing.rs::new_from_template`'s sequence,
/// with a refusal where the dialog has a notification. `root` is the
/// project (a parameter so a test can pass a scratch one; the commands
/// pass the project around the current directory).
fn run_template_in(root: &Path, template: Template, name: &str) -> Result<String, String> {
    let made = scaffold::scaffold(template, name).map_err(|error| error.to_string())?;
    let path = root.join(&made.path);
    if path.exists() {
        return Err(format!(
            "{} already exists — a template never overwrites a file somebody may have edited",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {} — {error}", parent.display()))?;
    }
    std::fs::write(&path, &made.contents)
        .map_err(|error| format!("cannot write {} — {error}", path.display()))?;

    // **And the module list, or the file compiles into nothing.** A Say
    // project's `build.rs` walks the directory and needs no line; a Rust
    // project's `mod.rs` is a list, and the line goes at its end, once.
    let mut noted = String::new();
    if let Some(line) = made.module_line.as_ref() {
        let mod_rs = root.join("src/screens/mod.rs");
        match std::fs::read_to_string(&mod_rs) {
            Ok(existing) if existing.contains(line.as_str()) => {}
            Ok(existing) => {
                let joined = format!("{}\n{line}\n", existing.trim_end());
                std::fs::write(&mod_rs, joined)
                    .map_err(|error| format!("wrote the file, but not mod.rs: {error}"))?;
            }
            Err(_) => {
                noted = format!(
                    " — and add `{line}` wherever this project lists its modules (there is \
                     no src/screens/mod.rs)",
                );
            }
        }
    }

    Ok(format!(
        "created {} (rename it before you build on it){noted}",
        made.path.display()
    ))
}

/// `assets`: the project's asset bundle as a listing — every file under
/// `assets/`, recursive, with sizes, which is the same directory
/// [`WorkspaceBundle`](crate::assets::WorkspaceBundle) resolves
/// `an image "…"` against first (the studio's
/// own bundle second — noted, because that is the answer to "why doesn't my
/// fallback image load").
///
/// # Errors
///
/// No project around the current directory.
pub fn run_assets(project: Option<&Path>) -> Result<String, String> {
    let root = match project {
        Some(path) => project_at(path).ok_or_else(|| {
            format!(
                "{} is not inside a project — assets belongs to one (the directory with \
                 the Cargo.toml)",
                path.display()
            )
        })?,
        None => {
            let cwd = std::env::current_dir()
                .map_err(|error| format!("cannot tell where you are standing — {error}"))?;
            project_at(&cwd).ok_or_else(|| {
                "assets belongs to a project — pass one, or stand inside it (the directory with the Cargo.toml)".to_owned()
            })?
        }
    };
    let bundle = root.join("assets");
    if !bundle.is_dir() {
        return Err(format!(
            "{} has no assets/ directory — create it and put images, fonts and data files \
             there; `an image \"…\"` reads from it",
            root.display()
        ));
    }

    let mut files = Vec::new();
    collect_assets(&bundle, &bundle, &mut files, 0)
        .map_err(|error| format!("cannot walk {} — {error}", bundle.display()))?;
    files.sort();
    if files.is_empty() {
        return Ok(format!(
            "{}'s asset bundle is empty (assets/ holds nothing but .gitkeep) — the scaffold \
             writes the directory and nothing in it",
            root.display()
        ));
    }

    let mut out = format!("the asset bundle at {}:\n", bundle.display());
    let mut total = 0_u64;
    for (relative, size) in &files {
        out.push_str(&format!(
            "  {:>9}  {}\n",
            human_bytes(*size),
            relative.display()
        ));
        total += size;
    }
    out.push_str(&format!(
        "  {} file{}, {} total — resolved project-first, then the studio's own bundle",
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        human_bytes(total)
    ));
    Ok(out)
}

/// Walk `dir`, collecting every file's path relative to `root` and its
/// length. Depth-capped, because a symlink loop is a walk that never ends
/// and a refusal is better than a hang. Dotfiles are skipped — `.gitkeep`
/// is the scaffolder keeping an empty directory in git, not an asset, and a
/// listing that shows it is a listing with one lie in it.
fn collect_assets(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, u64)>,
    depth: usize,
) -> std::io::Result<()> {
    if depth > 16 {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let hidden = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_assets(root, &path, out, depth + 1)?;
        } else if metadata.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push((relative, metadata.len()));
        }
    }
    Ok(())
}

/// A byte count as a person reads one — `1.2 MB`, not `1234567`.
fn human_bytes(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    #[expect(clippy::cast_precision_loss, reason = "a byte count is exact in f64")]
    let size = size as f64;
    if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.0} KB", size / KB)
    } else {
        format!("{size:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_writes_the_file_and_the_module_line() {
        let dir = std::env::temp_dir().join("vieww-cli-template");
        std::fs::remove_dir_all(&dir).ok();
        let screens = dir.join("src").join("screens");
        std::fs::create_dir_all(&screens).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"d\"\n").unwrap();
        std::fs::write(
            screens.join("mod.rs"),
            "//! One module per screen.\n\npub mod home;\n",
        )
        .unwrap();

        let summary = run_template_in(&dir, Template::Screen, "login").unwrap();
        assert!(summary.contains("login.rs"), "{summary}");
        assert!(screens.join("login.rs").is_file(), "the template's file");
        let mod_rs = std::fs::read_to_string(screens.join("mod.rs")).unwrap();
        assert!(
            mod_rs.contains("pub mod login;"),
            "the module line: {mod_rs}"
        );

        // Twice is a refusal: never overwrite.
        let error = run_template_in(&dir, Template::Screen, "login").unwrap_err();
        assert!(error.contains("already exists"), "{error}");
    }

    #[test]
    fn a_bad_module_name_is_refused_with_nothing_written() {
        let dir = std::env::temp_dir().join("vieww-cli-template-name");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("src").join("screens")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"d\"\n").unwrap();

        assert!(run_template_in(&dir, Template::Widget, "Not A Name").is_err());
        assert!(
            !dir.join("src").join("screens").join("login.rs").exists(),
            "nothing was written for a refused name"
        );
    }

    #[test]
    fn byte_counts_read_like_people_read_them() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2 KB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MB");
    }
}
