//! Undo for file operations (ST-11).
//!
//! Text has always had undo: every buffer keeps its own history. A file
//! created, renamed or moved to the trash from the Explorer had none,
//! because the only cross-file undo was git. The journal records each of
//! those operations together with how to reverse it. `Undo File Operation`
//! reverses the newest one, and `Redo File Operation` does it again.
//!
//! Every reversal is a move, never a deletion:
//!
//! * undoing a **create** moves the new file or folder into the workspace
//!   trash, where `Move to Trash` would have put it;
//! * undoing a **rename** renames it back;
//! * undoing a **trash** moves it back from the trash to where it was.
//!
//! An undo refuses rather than overwrite. If something else now occupies
//! the destination, the undo fails with a sentence and the entry stays on
//! the stack. The journal is kept per workspace, in memory; it is cleared
//! when another workspace opens.

use std::path::{Path, PathBuf};

/// One reversible file operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// `path` was created (a folder when `dir`).
    Created {
        /// What was created.
        path: PathBuf,
        /// A folder rather than a file.
        dir: bool,
        /// Where an undo moved it, so a redo can bring it back.
        trashed_at: Option<PathBuf>,
    },
    /// `from` was renamed to `to`.
    Renamed {
        /// The old path.
        from: PathBuf,
        /// The new path.
        to: PathBuf,
    },
    /// `original` was moved to the trash, at `trashed_at`.
    Trashed {
        /// Where it lived.
        original: PathBuf,
        /// Where it is in the trash.
        trashed_at: PathBuf,
    },
}

impl Entry {
    /// What undoing it does, in words.
    #[must_use]
    pub fn undo_description(&self) -> String {
        match self {
            Self::Created { path, .. } => format!("Undo creating {}", name(path)),
            Self::Renamed { from, to } => format!("Undo renaming {} to {}", name(from), name(to)),
            Self::Trashed { original, .. } => format!("Undo deleting {}", name(original)),
        }
    }
}

fn name(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn move_refusing_overwrite(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Err(format!(
            "{} is in the way — move it first, then undo again",
            to.display()
        ));
    }
    if !from.exists() {
        return Err(format!("{} is no longer there", from.display()));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

/// The workspace's file-operation history.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Journal {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

/// How many operations are kept.
const DEPTH: usize = 200;

impl Journal {
    /// Record an operation that just happened. Clears the redo stack, as a
    /// new edit does in text.
    pub fn record(&mut self, entry: Entry) {
        self.undo.push(entry);
        if self.undo.len() > DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Whether there is an operation to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is an operation to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The newest redoable operation.
    #[must_use]
    pub fn peek_redo(&self) -> Option<&Entry> {
        self.redo.last()
    }

    /// The newest undoable operation.
    #[must_use]
    pub fn peek_undo(&self) -> Option<&Entry> {
        self.undo.last()
    }

    /// Reverse the newest operation, inside `root` (whose trash undoing a
    /// create uses).
    ///
    /// # Errors
    /// The reversal would overwrite something, or its source is gone; the
    /// entry stays on the stack.
    pub fn undo(&mut self, root: &Path) -> Result<String, String> {
        let entry = self.undo.last().cloned().ok_or("Nothing to undo")?;
        let (redo, message) = match entry {
            Entry::Created { path, dir, .. } => {
                let trashed =
                    crate::file_tree::delete_to_trash(root, &path).map_err(|e| e.to_string())?;
                let message = format!("Moved {} to the workspace trash", name(&path));
                (
                    Entry::Created {
                        path,
                        dir,
                        trashed_at: Some(trashed),
                    },
                    message,
                )
            }
            Entry::Renamed { from, to } => {
                move_refusing_overwrite(&to, &from)?;
                let message = format!("Renamed {} back to {}", name(&to), name(&from));
                (Entry::Renamed { from, to }, message)
            }
            Entry::Trashed {
                original,
                trashed_at,
            } => {
                move_refusing_overwrite(&trashed_at, &original)?;
                let message = format!("Restored {}", name(&original));
                (
                    Entry::Trashed {
                        original,
                        trashed_at,
                    },
                    message,
                )
            }
        };
        self.undo.pop();
        self.redo.push(redo);
        Ok(message)
    }

    /// Do the newest undone operation again.
    ///
    /// # Errors
    /// As [`Self::undo`].
    pub fn redo(&mut self, root: &Path) -> Result<String, String> {
        let entry = self.redo.last().cloned().ok_or("Nothing to redo")?;
        let (again, message) = match entry {
            Entry::Created {
                path,
                dir,
                trashed_at,
            } => {
                let from = trashed_at.clone().ok_or("It was never trashed")?;
                move_refusing_overwrite(&from, &path)?;
                let message = format!("Recreated {}", name(&path));
                (
                    Entry::Created {
                        path,
                        dir,
                        trashed_at: None,
                    },
                    message,
                )
            }
            Entry::Renamed { from, to } => {
                move_refusing_overwrite(&from, &to)?;
                let message = format!("Renamed {} to {}", name(&from), name(&to));
                (Entry::Renamed { from, to }, message)
            }
            Entry::Trashed { original, .. } => {
                let trashed = crate::file_tree::delete_to_trash(root, &original)
                    .map_err(|e| e.to_string())?;
                let message = format!("Moved {} to the workspace trash", name(&original));
                (
                    Entry::Trashed {
                        original,
                        trashed_at: trashed,
                    },
                    message,
                )
            }
        };
        self.redo.pop();
        self.undo.push(again);
        Ok(message)
    }

    /// Forget everything (another workspace opened).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
