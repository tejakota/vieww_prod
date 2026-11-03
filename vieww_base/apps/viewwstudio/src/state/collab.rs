//! ST-10: the buffer, shared — `vieww-collab` finally used by the studio.
//!
//! # What this is
//!
//! The active buffer's text is held in a [`collab::Doc`] — an RGA replicated
//! text under a Lamport clock — for as long as a session is running. Local
//! edits are applied to the doc immediately and **emitted as operations**;
//! operations and presence updates from other replicas are read back and
//! applied; because the CRDT converges from any interleaving, both sides end
//! with the same text without either waiting for the other.
//!
//! # Why the transport is an append-only file
//!
//! v0 has no server and needs none for the claim it makes: every replica
//! appends JSONL to `<workspace>/.vieww/collab.log` and tails it from its own
//! read offset. Two studios on one machine share through the page cache; two
//! machines share through whatever already syncs the project folder
//! (Syncthing, a network drive, a checked-in branch). The honest limits:
//!
//! * **No line-level locking.** Appends under one line's length are atomic on
//!   the platforms this ships on, so a reader never sees a torn line — it
//!   stops at the last `\n` — but two replicas starting *simultaneously* on an
//!   empty log can both seed the base text and double it. Start one first.
//! * **A crashed writer leaves a partial line**, which is never consumed and
//!   never corrected. Harmless: the line is skipped and the next complete one
//!   is read.
//! * **`Doc::apply` de-duplicates by stamp**, so re-reading a log — including
//!   one's own writes — is a no-op. That is what makes the whole tail-a-file
//!   design safe.
//!
//! # The seam this hangs on
//!
//! Every text change in the studio already funnels through two functions:
//! [`Studio::edit`](super::Studio::edit) (typing, paste, undo, format,
//! canvas edits — the lot) and [`Studio::put_buffers`](super::Studio::put_buffers)
//! (open, close, reload). The session hooks both: `edit` diffs old against
//! new and emits insert/delete operations; `put_buffers` reconciles, and any
//! change to the shared buffer that did **not** come through `edit` — a
//! watcher reload, a reopened tab, a conflict resolution — is pulled back to
//! the doc's converged text, because while the file is shared the doc *is*
//! the file. Local [`History`](crate::history::History) is untouched and
//! stays the local undo; remote edits are not pushed onto it.
//!
//! # Presence
//!
//! The caret's position is broadcast as an *anchor* — the id of the character
//! it stands behind — so it survives other people's edits, the same property
//! the crate's `Presence` type exists for. [`RemoteCaret`] is the line/column
//! form the editor's overlay draws, derived through the layout probe the way
//! inline diagnostics are.
//!
//! # v0 boundaries, said out loud
//!
//! One buffer per session (the one active when it started); edits made while
//! a *different* buffer is focused are not shared. Carets are drawn only with
//! word-wrap off, because the probe's line rects are visual rows and only
//! without wrapping does a source line equal one.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::rc::Rc;

use vieww_collab::{self as collab, Op};
use vieww_foundation::json::Json;

use super::Studio;
use crate::buffer::Buffer;

/// The session's own state, in the cell the `Studio` carries.
///
/// Not a signal: nothing subscribes to the *session* — the buffer text is
/// already a signal, and the carets the editor draws are published through
/// [`Studio::collab_carets`](super::Studio::collab_carets) when they change.
#[derive(Debug)]
pub(super) struct CollabSession {
    /// The shared text. Its `text` field is the RGA; the doc around it mints
    /// stamps and remembers what has been applied.
    doc: collab::Doc,
    /// `<workspace>/.vieww/collab.log`, the transport.
    log: PathBuf,
    /// The file whose buffer is shared, by path: tabs reorder, paths do not.
    buffer: PathBuf,
    /// Who we are, for other replicas' presence readouts.
    name: String,
    /// How many bytes of the log this replica has consumed. Appends past it
    /// are the next poll's work; a partial line at the end is left unread.
    read: u64,
    /// Everyone else's carets, newest-per-replica by sequence.
    presence: collab::Presence,
    /// Our presence sequence — bumped per broadcast, so stale lines lose.
    seq: u64,
    /// The last caret position broadcast, to avoid a log line per keystroke
    /// that only moved the caret within one character.
    last_caret: Option<(u32, u32)>,
}

/// A collaborator's caret, as the editor draws it.
///
/// One-based line and column, the same convention `Buffer::caret` uses, so
/// the status readout and the remote flag agree on what "line 3" means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCaret {
    pub replica: collab::ReplicaId,
    pub name: String,
    pub line: u32,
    pub col: u32,
}

impl Studio {
    // ----- starting and stopping ------------------------------------------

    /// Share the active buffer, or stop sharing it.
    ///
    /// The command behind it toggles, because "share" is a state the user
    /// needs to be able to leave without finding a second menu item.
    pub fn toggle_collab(&self) {
        if self.collab.peek() {
            self.stop_collab();
        } else {
            self.start_collab();
        }
    }

    /// Begin a session on the active buffer.
    ///
    /// On a workspace with no log yet this replica *seeds* it — the whole
    /// base text as operations, which is what lets a later arrival rebuild
    /// the document from the file alone. On a workspace that already has one,
    /// the session starts empty and [`Studio::poll_collab`] catches it up;
    /// the doc's converged text then wins over whatever this buffer happened
    /// to hold, which is the meaning of joining a shared file.
    pub fn start_collab(&self) {
        let Some(root) = self.root.get() else {
            self.notify("Open a project folder first — collaboration shares one of its files.");
            return;
        };
        let Some(buffer) = self.active() else {
            return;
        };
        let Some(path) = buffer.path.clone() else {
            self.notify("A scratch buffer cannot be shared — save it into the project first.");
            return;
        };
        let dir = root.join(".vieww");
        if std::fs::create_dir_all(&dir).is_err() {
            self.notify(
                "The .vieww folder could not be created — collaboration keeps its log there.",
            );
            return;
        }
        let log = dir.join("collab.log");
        // Whether this is joining an existing session or starting the first
        // one — decided before `log` moves into the session.
        let joining = log.is_file();

        let replica = next_replica();
        let name = format!("{} {}", user_name(), replica);
        let mut session = CollabSession {
            doc: collab::Doc::new(replica),
            log,
            buffer: path,
            name: name.clone(),
            read: 0,
            presence: collab::Presence::default(),
            seq: 0,
            last_caret: None,
        };

        if joining {
            // A session exists: adopt it wholesale. `poll_collab` reads from
            // zero, applies the seed and everything after it, and reconciles
            // the buffer to the converged text.
            *self.collab_session.borrow_mut() = Some(session);
            self.collab.set(true);
            self.poll_collab();
        } else {
            // First replica: the buffer's text is the base state, and it goes
            // into the log as operations so a later arrival can rebuild it.
            let text = buffer.value.text.clone();
            let ops = session.doc.insert(0, &text);
            let (anchor, caret) = session.caret_broadcast(&buffer.value);
            session.broadcast_ops(&ops);
            session.broadcast_presence(anchor);
            session.last_caret = Some(caret);
            // Skip re-reading the seed we just wrote: applying it again would
            // be a no-op (stamps de-duplicate), but reading it is wasted work.
            session.read = std::fs::metadata(&session.log).map_or(0, |m| m.len());
            *self.collab_session.borrow_mut() = Some(session);
            self.collab.set(true);
            self.collab_refresh_carets();
        }
        self.notify(&crate::i18n::tf(
            "Sharing {} — edits sync through .vieww/collab.log.",
            &[&name],
        ));
    }

    /// End the session. The buffer keeps its text; the log keeps its history,
    /// so a restarted session still catches up to everything in it.
    pub fn stop_collab(&self) {
        if !self.collab.peek() {
            return;
        }
        let name = self
            .collab_session
            .borrow()
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        self.collab.set(false);
        *self.collab_session.borrow_mut() = None;
        self.collab_carets.set(Rc::new(Vec::new()));
        self.notify(&crate::i18n::tf("Stopped sharing {}.", &[&name]));
    }

    // ----- the two seams ---------------------------------------------------

    /// The `edit` seam: diff the edit against the doc and emit it.
    ///
    /// Called by [`Studio::edit`](super::Studio::edit) *before* the buffer is
    /// mutated, with the buffer's old value and the value it is about to
    /// take. Caret-only moves broadcast presence, nothing else.
    pub(super) fn collab_note_edit(
        &self,
        buffer: &Buffer,
        next: &vieww_foundation::TextEditingValue,
    ) {
        if !self.collab.peek() {
            return;
        }
        let mut session = self.collab_session.borrow_mut();
        let Some(session) = session.as_mut() else {
            return;
        };
        if buffer.path.as_deref() != Some(session.buffer.as_path()) {
            // A different file is focused; its edits are not this session's.
            return;
        }
        if buffer.value.text != next.text {
            let (pos, deleted, inserted) = text_diff(&buffer.value.text, &next.text);
            let mut ops = Vec::new();
            if deleted > 0 {
                ops.extend(session.doc.delete(pos, deleted));
            }
            if !inserted.is_empty() {
                ops.extend(session.doc.insert(pos, inserted));
            }
            session.broadcast_ops(&ops);
        }
        let (anchor, caret) = session.caret_broadcast(next);
        if session.last_caret != Some(caret) {
            session.broadcast_presence(anchor);
            session.last_caret = Some(caret);
        }
    }

    /// The `put_buffers` seam: reconcile the shared buffer with the doc.
    ///
    /// Every buffer write that did not come through `collab_note_edit` — a
    /// reload from disk, a reopened tab, a conflict resolution — lands here,
    /// and while the file is shared the doc's converged text is what the
    /// buffer holds. The rewrite is in place, before the signal is published,
    /// so there is one write and no recursion.
    pub(super) fn collab_reconcile(&self, buffers: &mut [Buffer]) {
        if !self.collab.peek() {
            return;
        }
        let mut session = self.collab_session.borrow_mut();
        let Some(session) = session.as_mut() else {
            return;
        };
        let Some(index) = buffers
            .iter()
            .position(|b| b.path.as_deref() == Some(session.buffer.as_path()))
        else {
            return;
        };
        let shared = session.doc.text.value();
        if buffers[index].value.text == shared {
            return;
        }
        let buffer = &mut buffers[index];
        let mut value = buffer.value.clone();
        value.text = shared;
        let caret = clamp_to_boundary(&value.text, value.selection.extent);
        value.selection = vieww_foundation::TextSelection::collapsed(caret);
        value.secondary.clear();
        value.composing = None;
        buffer.value = value;
        buffer.dirty = true;
        self.notify(
            "This buffer is shared — restored the session's text over a change from outside it.",
        );
    }

    // ----- the poll --------------------------------------------------------

    /// Tail the log: apply new operations and presence, publish remote carets.
    ///
    /// Called every frame from the platform hook (a byte comparison on the
    /// common frame where nothing was appended) and directly by tests. Stops
    /// at the last complete line so a writer mid-append is never misread.
    pub fn poll_collab(&self) {
        if !self.collab.peek() {
            return;
        }
        let fresh = {
            let mut session = self.collab_session.borrow_mut();
            let Some(session) = session.as_mut() else {
                return;
            };
            let Ok(mut file) = std::fs::File::open(&session.log) else {
                return;
            };
            let Ok(len) = file.metadata().map(|m| m.len()) else {
                return;
            };
            if len <= session.read {
                return;
            }
            if file.seek(SeekFrom::Start(session.read)).is_err() {
                return;
            }
            let mut fresh = String::new();
            if file.read_to_string(&mut fresh).is_err() {
                // A torn append is the usual cause; try again next frame from
                // the same offset rather than skipping what might be good.
                return;
            }
            let Some(end) = fresh.rfind('\n') else {
                return;
            };
            session.read += end as u64 + 1;
            let mut applied = false;
            for line in fresh[..=end].lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(j) = Json::parse(line) else {
                    // A corrupt line cannot be un-written; skipping it keeps
                    // the session alive, which is the better failure.
                    continue;
                };
                if j.get("kind").and_then(Json::as_str) == Some("presence") {
                    if let Some(update) = presence_from(&j) {
                        // Our own presence line, read back: the log is shared
                        // and this replica reads all of it. Applying it would
                        // put our caret in the *remote* carets list.
                        if update.replica != session.doc.replica {
                            session.presence.update(
                                update.replica,
                                update.seq,
                                &update.name,
                                update.anchor,
                            );
                        }
                    }
                } else if let Ok(op) = Op::from_json(&j) {
                    session.doc.apply(&op);
                    applied = true;
                }
            }
            applied
        };
        if fresh {
            self.collab_adopt_text();
        }
        self.collab_refresh_carets();
    }

    /// Push the doc's text into the shared buffer, if they differ.
    ///
    /// The remote-edit path: not through `edit`, so nothing is emitted back
    /// (that would be an echo loop) and the local history is not stepped —
    /// other people's typing is not this side's undo.
    fn collab_adopt_text(&self) {
        let shared = {
            let session = self.collab_session.borrow();
            session
                .as_ref()
                .map(|s| (s.buffer.clone(), s.doc.text.value()))
        };
        let Some((path, text)) = shared else {
            return;
        };
        let buffers = (*self.buffers.get()).clone();
        let Some(index) = buffers
            .iter()
            .position(|b| b.path.as_deref() == Some(path.as_path()))
        else {
            return;
        };
        let mut buffers = buffers;
        if buffers[index].value.text == text {
            return;
        }
        let buffer = &mut buffers[index];
        let mut value = buffer.value.clone();
        // Keep the local caret where it can still stand: clamped down to a
        // boundary. An anchor-aware mapping is what `Presence` exists for and
        // is the upgrade path; clamping is the honest v0.
        let caret = clamp_to_boundary(&text, value.selection.extent);
        value.text = text;
        value.selection = vieww_foundation::TextSelection::collapsed(caret);
        value.secondary.clear();
        value.composing = None;
        buffer.value = value;
        buffer.dirty = true;
        let caret = buffer.caret();
        self.put_buffers(buffers);
        self.caret.set(caret);
        self.mark_dirty();
        self.buffer_changed();
    }

    /// This session's replica id, for tests and diagnostics.
    #[must_use]
    pub fn collab_replica(&self) -> Option<collab::ReplicaId> {
        self.collab_session
            .borrow()
            .as_ref()
            .map(|session| session.doc.replica)
    }

    /// Publish everyone else's carets as line/column, for the overlay.
    fn collab_refresh_carets(&self) {
        let carets = {
            let session = self.collab_session.borrow();
            let Some(session) = session.as_ref() else {
                return;
            };
            let text = session.doc.text.value();
            session
                .presence
                .positions(&session.doc.text)
                .into_iter()
                .map(|(replica, name, pos)| {
                    // `positions` hands back the caret's own character index
                    // (0-based, doc-wide): `anchor_at(p)` anchors to the char
                    // at `p-1` and `position_of` counts back to it, so the +1
                    // the crate adds returns `p` itself.
                    let (line, col) = line_col(&text, pos);
                    RemoteCaret {
                        replica,
                        name,
                        line,
                        col,
                    }
                })
                .collect::<Vec<_>>()
        };
        self.collab_carets.set(Rc::new(carets));
    }
}

impl CollabSession {
    /// Append `ops` to the log, one JSON object per line.
    ///
    /// An append failure drops the ops for the transport but not from the
    /// doc: this replica's text is still right, and the next successful
    /// append carries whatever it can. v0 says nothing about it — a shared
    /// folder that cannot be written to is a problem the user can see.
    fn broadcast_ops(&mut self, ops: &[Op]) {
        let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log)
        else {
            return;
        };
        for op in ops {
            let _ = writeln!(file, "{}", op.to_json().to_string());
        }
    }

    /// Append one presence line for `anchor`.
    fn broadcast_presence(&mut self, anchor: Option<collab::Stamp>) {
        self.seq += 1;
        let line = Json::object([
            ("kind", Json::from("presence")),
            ("r", Json::from(f64::from(self.doc.replica))),
            ("q", Json::from(self.seq as f64)),
            ("n", Json::from(self.name.as_str())),
            (
                "a",
                anchor.map_or(Json::Null, |s| {
                    Json::numbers([s.counter as f64, f64::from(s.replica)])
                }),
            ),
        ])
        .to_string();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log)
        {
            let _ = writeln!(file, "{line}");
        }
    }

    /// The anchor and line/column a value's caret should broadcast.
    ///
    /// The anchor is the character *before* the caret, the crate's
    /// `anchor_at` convention; the line/column pair is only used to keep
    /// consecutive broadcasts of the same position from appending a line each.
    fn caret_broadcast(
        &self,
        value: &vieww_foundation::TextEditingValue,
    ) -> (Option<collab::Stamp>, (u32, u32)) {
        let pos = char_index(&value.text, value.selection.extent);
        let (line, col) = line_col(&value.text, pos);
        (collab::anchor_at(&self.doc.text, pos), (line, col))
    }
}

/// A presence line, parsed back.
struct PresenceUpdate {
    replica: collab::ReplicaId,
    seq: u64,
    name: String,
    anchor: Option<collab::Stamp>,
}

fn presence_from(j: &Json) -> Option<PresenceUpdate> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let replica = j.get("r").and_then(Json::as_f64)? as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let seq = j.get("q").and_then(Json::as_f64)? as u64;
    let name = j
        .get("n")
        .and_then(Json::as_str)
        .unwrap_or("editor")
        .to_owned();
    let anchor = j.get("a").and_then(Json::as_array).and_then(|a| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let counter = a.first().and_then(Json::as_f64)? as u64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let replica = a.get(1).and_then(Json::as_f64)? as u32;
        Some(collab::Stamp { counter, replica })
    });
    Some(PresenceUpdate {
        replica,
        seq,
        name,
        anchor,
    })
}

// ----- small text helpers -------------------------------------------------

/// The edit that turns `old` into `new`, as `(position, deleted, inserted)`.
///
/// Position and length are in `char`s — the doc's addressing; `inserted` is
/// the replacement text. A common prefix and suffix are left alone, so a
/// one-character typing change is one operation, not a whole-file rewrite —
/// which is the difference between a shared log that grows by a line per
/// keystroke and one that grows by the file.
fn text_diff<'a>(old: &str, new: &'a str) -> (usize, usize, &'a str) {
    let old_chars = old.chars().count();
    let new_chars = new.chars().count();
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old
        .chars()
        .rev()
        .zip(new.chars().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old_chars - prefix)
        .min(new_chars - prefix);
    let deleted = old_chars - prefix - suffix;
    let start = char_byte(new, prefix);
    let end = char_byte(new, new_chars - suffix);
    (prefix, deleted, &new[start..end])
}

/// How many whole characters precede byte offset `byte`.
///
/// A `byte` that lands inside a multi-byte character is moved down to the
/// boundary first: a selection extent is bytes and can be stale by half a
/// character after a remote edit, and a panic here would take the frame with
/// it.
fn char_index(text: &str, byte: usize) -> usize {
    let byte = clamp_to_boundary(text, byte);
    text[..byte].chars().count()
}

/// The byte offset of character index `at`.
fn char_byte(text: &str, at: usize) -> usize {
    text.char_indices()
        .nth(at)
        .map_or(text.len(), |(byte, _)| byte)
}

/// `byte`, moved down to the nearest character boundary.
fn clamp_to_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while byte > 0 && !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

/// The one-based line and column of character index `at` (zero-based).
fn line_col(text: &str, at: usize) -> (u32, u32) {
    let (mut line, mut col) = (1u32, 1u32);
    let mut seen = 0usize;
    for ch in text.chars() {
        if seen >= at {
            break;
        }
        seen += 1;
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// A replica id that is unique within this process and unlikely to meet its
/// twin in another one: the process id mixed with a per-session counter.
fn next_replica() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::process::id() ^ n.wrapping_mul(0x9E37_79B9_u32)
}

/// What other replicas call us. The username, or "editor" where there is
/// none; the replica id beside it (in `start_collab`) is what keeps two
/// studios run by one user distinguishable.
fn user_name() -> String {
    for key in ["USER", "USERNAME", "LOGNAME"] {
        if let Ok(name) = std::env::var(key) {
            if !name.trim().is_empty() {
                return name;
            }
        }
    }
    "editor".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typing_change_is_one_small_edit() {
        let (pos, deleted, inserted) = text_diff("hello world", "hello worlds");
        assert_eq!((pos, deleted, inserted), (11, 0, "s"));
        let (pos, deleted, inserted) = text_diff("hello", "hallo");
        assert_eq!((pos, deleted, inserted), (1, 1, "a"));
        let (pos, deleted, inserted) = text_diff("shello", "hello");
        assert_eq!((pos, deleted, inserted), (0, 1, ""));
        // A whole-file replacement is still expressed correctly.
        let (pos, deleted, inserted) = text_diff("abc", "xyz");
        assert_eq!((pos, deleted, inserted), (0, 3, "xyz"));
    }

    #[test]
    fn line_and_column_are_one_based_and_multibyte_safe() {
        let text = "ab\ncdé\nf";
        assert_eq!(line_col(text, 0), (1, 1));
        assert_eq!(line_col(text, 2), (1, 3));
        assert_eq!(line_col(text, 3), (2, 1));
        // é is one char, two bytes: columns count characters.
        assert_eq!(line_col(text, 6), (2, 4));
        assert_eq!(line_col(text, 7), (3, 1));
        assert_eq!(line_col(text, 8), (3, 2), "past the end is the last caret");
        assert_eq!(char_index(text, 5), 5);
        assert_eq!(char_index(text, 6), 5, "a byte inside é clamps to é");
        assert_eq!(char_index(text, 7), 6, "the newline after é");
        assert_eq!(clamp_to_boundary("dé", 2), 1);
    }

    #[test]
    fn replica_ids_do_not_repeat_within_a_process() {
        let a = next_replica();
        let b = next_replica();
        assert_ne!(a, b);
    }
}
