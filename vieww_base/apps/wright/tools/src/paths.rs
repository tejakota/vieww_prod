//! Path safety: the one function every file tool goes through.
//!
//! A tool-facing path is project-relative. [`resolve`] turns it into an
//! absolute path that is guaranteed to stay inside the project, or
//! refuses. The rules:
//!
//! * no absolute paths, no `..` components, no NUL;
//! * no symlinks anywhere along the way — not the file, not a parent.
//!   (v0.1 canonicalized only the parent, so `innocent.txt -> /etc/hostname`
//!   read straight out of the sandbox. Review found it; the regression
//!   test is `symlink_escape_is_refused`.)
//! * `.wright/` (Wright's own state) is never reachable by agents;
//!   `.git/` is reachable only through the `git` tool.
//!
//! Symlink checks walk the path component by component with
//! `symlink_metadata`, which does not follow links.

use std::path::{Component, Path, PathBuf};

/// Why a path was refused, in words for the Event River.
pub type PathError = String;

/// Resolve `rel` inside `project` (which must already be canonical).
pub fn resolve(project: &Path, rel: &str) -> Result<PathBuf, PathError> {
    if rel.contains('\0') {
        return Err("path contains a NUL byte".into());
    }
    let rel = rel.trim();
    let rel = if rel.is_empty() { "." } else { rel };
    let p = Path::new(rel);
    let mut out = project.to_path_buf();
    let mut first = true;
    for c in p.components() {
        match c {
            Component::CurDir => continue,
            Component::Normal(name) => {
                if first && (name == ".wright" || name == ".git") {
                    return Err(format!(
                        "'{rel}' is inside {} — not reachable by agents",
                        name.to_string_lossy()
                    ));
                }
                first = false;
                out.push(name);
                if let Ok(meta) = std::fs::symlink_metadata(&out) {
                    if meta.file_type().is_symlink() {
                        return Err(format!("'{rel}' goes through a symbolic link (refused: links can point outside the project)"));
                    }
                }
            }
            Component::ParentDir => {
                return Err(format!(
                    "'{rel}' uses '..' (paths must stay inside the project)"
                ))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "'{rel}' is absolute (paths are relative to the project)"
                ));
            }
        }
    }
    debug_assert!(out.starts_with(project));
    Ok(out)
}

/// Create missing parent directories of `target`, refusing to create
/// anything through a symlink (each level re-checked as it appears).
pub fn create_parents(project: &Path, target: &Path) -> Result<(), PathError> {
    let parent = target.parent().ok_or("no parent directory")?;
    let rel = parent
        .strip_prefix(project)
        .map_err(|_| "outside the project".to_string())?;
    let mut cur = project.to_path_buf();
    for c in rel.components() {
        cur.push(c);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err("a parent directory is a symbolic link (refused)".into())
            }
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(format!("'{}' exists and is not a directory", cur.display())),
            Err(_) => {
                std::fs::create_dir(&cur).map_err(|e| format!("create {}: {e}", cur.display()))?
            }
        }
    }
    Ok(())
}

/// Write `bytes` to `target` (inside `project`, already [`resolve`]d),
/// creating missing parents, with no window for a concurrent process to
/// swap a directory for a symlink between the check and the write (WR-10).
///
/// On Linux every directory is opened one component at a time with
/// `O_DIRECTORY | O_NOFOLLOW` beneath the previous, already-open one
/// (through its `/proc/self/fd` handle, which cannot be redirected), the
/// temp file is created `O_CREAT | O_EXCL | O_NOFOLLOW`, and the rename
/// happens inside that pinned directory. Elsewhere the walk falls back to
/// [`create_parents`] + a temp file created exclusively, then re-checks
/// that the result is still inside the project.
pub fn write_pinned(project: &Path, target: &Path, bytes: &[u8]) -> Result<(), PathError> {
    let rel = target
        .strip_prefix(project)
        .map_err(|_| "outside the project".to_string())?;
    let name = rel
        .file_name()
        .ok_or_else(|| "no file name".to_string())?
        .to_os_string();
    let parents: Vec<_> = rel
        .parent()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_os_string())
                .collect()
        })
        .unwrap_or_default();
    write_pinned_impl(project, &parents, &name, bytes)
}

#[cfg(target_os = "linux")]
fn write_pinned_impl(
    project: &Path,
    parents: &[std::ffi::OsString],
    name: &std::ffi::OsStr,
    bytes: &[u8],
) -> Result<(), PathError> {
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    const O_PATH: i32 = 0o10_000_000;
    const O_DIRECTORY: i32 = 0o200_000;
    const O_NOFOLLOW: i32 = 0o400_000;
    let open_dir = |p: &Path| -> std::io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(O_PATH | O_DIRECTORY | O_NOFOLLOW)
            .open(p)
    };
    let under = |dir: &File, n: &std::ffi::OsStr| -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd())).join(n)
    };
    let mut dir = open_dir(project).map_err(|e| format!("{}: {e}", project.display()))?;
    for comp in parents {
        let next = under(&dir, comp);
        dir = match open_dir(&next) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // mkdir never follows a symlink in its last component.
                std::fs::create_dir(&next)
                    .or_else(|e| {
                        if e.kind() == std::io::ErrorKind::AlreadyExists {
                            Ok(())
                        } else {
                            Err(e)
                        }
                    })
                    .map_err(|e| format!("create {}: {e}", comp.to_string_lossy()))?;
                open_dir(&next).map_err(|_| refused_link(comp))?
            }
            Err(_) => return Err(refused_link(comp)),
        };
    }
    let tmp_name = format!(
        ".{}.wright-tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    );
    let tmp = under(&dir, std::ffi::OsStr::new(&tmp_name));
    let _ = std::fs::remove_file(&tmp); // a stale temp (or a planted link) — unlink never follows
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(O_NOFOLLOW)
            .mode(0o644)
            .open(&tmp)
            .map_err(|e| format!("create {tmp_name}: {e}"))?;
        f.write_all(bytes).map_err(|e| format!("write: {e}"))?;
        f.sync_all().map_err(|e| format!("sync: {e}"))?;
    }
    // rename replaces a symlink at `name` rather than following it.
    std::fs::rename(&tmp, under(&dir, name)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename: {e}")
    })
}

#[cfg(not(target_os = "linux"))]
fn write_pinned_impl(
    project: &Path,
    parents: &[std::ffi::OsString],
    name: &std::ffi::OsStr,
    bytes: &[u8],
) -> Result<(), PathError> {
    use std::io::Write;
    let mut dir = project.to_path_buf();
    dir.extend(parents);
    let target = dir.join(name);
    create_parents(project, &target)?;
    let tmp = dir.join(format!(
        ".{}.wright-tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_EXCL: never through a planted link
            .open(&tmp)
            .map_err(|e| format!("create: {e}"))?;
        f.write_all(bytes).map_err(|e| format!("write: {e}"))?;
        f.sync_all().map_err(|e| format!("sync: {e}"))?;
    }
    std::fs::rename(&tmp, &target).map_err(|e| format!("rename: {e}"))?;
    match target.parent().and_then(|p| p.canonicalize().ok()) {
        Some(real) if real.starts_with(project) => Ok(()),
        _ => {
            let _ = std::fs::remove_file(&target);
            Err("a parent directory was replaced while writing (refused)".into())
        }
    }
}

#[cfg(target_os = "linux")]
fn refused_link(comp: &std::ffi::OsStr) -> PathError {
    format!(
        "'{}' is a symbolic link or not a directory (refused: links can point outside the project)",
        comp.to_string_lossy()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wright-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn plain_paths_resolve_inside() {
        let p = project("plain");
        assert_eq!(resolve(&p, "src/a.py").unwrap(), p.join("src/a.py"));
        assert_eq!(resolve(&p, "./x").unwrap(), p.join("x"));
        assert_eq!(resolve(&p, "").unwrap(), p);
    }

    #[test]
    fn escapes_are_refused() {
        let p = project("escape");
        for bad in [
            "../x",
            "a/../../x",
            "/etc/passwd",
            ".wright/cache/k.json",
            ".git/config",
            "a\0b",
        ] {
            assert!(resolve(&p, bad).is_err(), "{bad} must be refused");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_refused() {
        let p = project("symlink");
        std::os::unix::fs::symlink("/etc/hostname", p.join("innocent.txt")).unwrap();
        std::os::unix::fs::symlink("/etc", p.join("cfg")).unwrap();
        assert!(resolve(&p, "innocent.txt")
            .unwrap_err()
            .contains("symbolic link"));
        assert!(resolve(&p, "cfg/passwd")
            .unwrap_err()
            .contains("symbolic link"));
    }

    #[cfg(unix)]
    #[test]
    fn parents_are_created_but_never_through_links() {
        let p = project("parents");
        create_parents(&p, &p.join("a/b/c.txt")).unwrap();
        assert!(p.join("a/b").is_dir());
        std::os::unix::fs::symlink("/tmp", p.join("lnk")).unwrap();
        assert!(create_parents(&p, &p.join("lnk/x/y.txt")).is_err());
    }

    #[test]
    fn pinned_writes_create_parents_and_replace_atomically() {
        let p = project("pinned");
        let t = p.join("a/b/c.txt");
        write_pinned(&p, &t, b"one").unwrap();
        write_pinned(&p, &t, b"two").unwrap();
        assert_eq!(std::fs::read(&t).unwrap(), b"two");
        let leftovers: Vec<_> = std::fs::read_dir(p.join("a/b")).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "temp file left behind");
    }

    /// The race resolve() cannot close: a directory swapped for a symlink
    /// after the check. Simulated by swapping before the write.
    #[cfg(unix)]
    #[test]
    fn pinned_writes_never_go_through_a_swapped_link() {
        let p = project("swap");
        let outside = project("swap-outside");
        let t = resolve(&p, "dir/x.txt").unwrap(); // checked while dir/ is absent
        std::os::unix::fs::symlink(&outside, p.join("dir")).unwrap();
        assert!(write_pinned(&p, &t, b"pwned").is_err());
        assert!(
            !outside.join("x.txt").exists(),
            "write escaped through the link"
        );
    }

    /// A planted symlink at the temp name must not redirect the write.
    #[cfg(target_os = "linux")]
    #[test]
    fn planted_temp_link_is_not_followed() {
        let p = project("tmp-link");
        let outside = project("tmp-link-outside");
        let victim = outside.join("victim");
        std::fs::write(&victim, "orig").unwrap();
        let tmp = p.join(format!(".f.txt.wright-tmp-{}", std::process::id()));
        std::os::unix::fs::symlink(&victim, &tmp).unwrap();
        write_pinned(&p, &p.join("f.txt"), b"data").unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "orig");
        assert_eq!(std::fs::read_to_string(p.join("f.txt")).unwrap(), "data");
    }

    /// A symlink at the final name is replaced, never written through.
    #[cfg(unix)]
    #[test]
    fn final_link_is_replaced_not_followed() {
        let p = project("final-link");
        let outside = project("final-link-outside");
        std::fs::write(outside.join("v"), "orig").unwrap();
        std::os::unix::fs::symlink(outside.join("v"), p.join("f")).unwrap();
        write_pinned(&p, &p.join("f"), b"new").unwrap();
        assert_eq!(std::fs::read_to_string(outside.join("v")).unwrap(), "orig");
        assert!(!std::fs::symlink_metadata(p.join("f"))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
