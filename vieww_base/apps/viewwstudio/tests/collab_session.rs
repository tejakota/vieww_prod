//! ST-10 end to end: two studios, one buffer, one log file.
//!
//! # What this file proves
//!
//! The unit tests in `state/collab.rs` prove the pieces — the diff, the
//! line/column arithmetic, the replica ids. This file proves the wires, with
//! two whole `Studio` instances in one process pointed at the same folder:
//!
//! * local edits are applied to the RGA **and emitted**, and both sides
//!   converge on interleaved edits — the CRDT's whole claim, through the
//!   studio's real seams (`Studio::edit`, `Studio::poll_collab`) rather than
//!   an in-memory channel;
//! * the transport really is the file: the second instance builds its
//!   document from `<workspace>/.vieww/collab.log` alone, and a latecomer
//!   that starts *after* several edits adopts the converged text rather than
//!   seeding its own;
//! * presence travels the same log and lands as line/column carets, and the
//!   anchor keeps a remote caret where its owner left it through the other
//!   side's edits;
//! * the second seam holds: a buffer changed out from under the session —
//!   the shape a watcher reload has — is pulled back to the shared text.
//!
//! No rustc, no GPU: this is the editing layer, and it says so.

use std::path::PathBuf;

use vieww_element::Runtime;
use viewwstudio::Studio;

/// A workspace with one `.say` screen in it, cleaned up afterwards.
struct Dir {
    path: PathBuf,
}

impl Dir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("viewwstudio-collab-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src/screens")).expect("a temp workspace");
        std::fs::write(
            path.join("src/screens/flow.say"),
            "-- say-language: 1\nscreen \"home\":\n    a label \"hello\"\n",
        )
        .expect("the shared .say file");
        Self { path }
    }

    fn log(&self) -> PathBuf {
        self.path.join(".vieww/collab.log")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A studio with the workspace open and the shared buffer focused.
fn studio_in(dir: &Dir) -> Studio {
    let runtime = Runtime::new();
    let studio = Studio::new(&runtime);
    studio.open_workspace(&dir.path);
    // The shared file, opened on demand the way a click in the Explorer
    // opens it — `entry_points` may or may not have reached into
    // `src/screens/`, and the test should not depend on which.
    studio.open_path(dir.path.join("src/screens/flow.say"));
    studio
}

/// Type `text` at the caret, the way the editor field does.
fn type_at(studio: &Studio, text: &str) {
    let mut value = studio.active().expect("the shared buffer").value;
    value.insert(text);
    studio.edit(value);
}

/// Move the caret to a byte offset, the way a click does.
fn move_caret(studio: &Studio, byte: usize) {
    let mut value = studio.active().expect("the shared buffer").value;
    value.selection = vieww_foundation::TextSelection::collapsed(byte);
    studio.edit(value);
}

/// The active buffer's text.
fn text_of(studio: &Studio) -> String {
    studio.active().expect("the shared buffer").value.text
}

#[test]
fn two_instances_converge_on_interleaved_edits() {
    let dir = Dir::new("converge");
    let a = studio_in(&dir);
    let b = studio_in(&dir);

    // A shares first — it seeds the log with the base text.
    a.start_collab();
    assert!(a.collab.get(), "A is sharing");
    assert!(dir.log().is_file(), "the transport exists");

    // B joins: same log, no server, no handshake beyond reading it.
    b.start_collab();
    assert!(b.collab.get(), "B is sharing");
    b.poll_collab();
    assert_eq!(text_of(&b), text_of(&a), "B caught up to the seed");

    // Interleaved edits, both sides typing before either polls: A appends,
    // B prepends — the concurrent-insert case the RGA exists for.
    move_caret(&a, text_of(&a).len());
    type_at(&a, " and more");
    move_caret(&b, 0);
    type_at(&b, "-- two editors\n");
    a.poll_collab();
    b.poll_collab();

    let converged = text_of(&a);
    assert_eq!(
        text_of(&b),
        converged,
        "both sides hold the same text after one round"
    );
    assert!(converged.starts_with("-- two editors\n"), "{converged}");
    assert!(converged.ends_with(" and more"), "{converged}");

    // And a second round on top: insert on A, insert on B, poll in the
    // opposite order to the first round.
    let hello = text_of(&a).find("hello").expect("the word");
    let mut value = a.active().expect("the shared buffer").value;
    value.selection = vieww_foundation::TextSelection::collapsed(hello + "hello".len());
    value.insert("!");
    a.edit(value);
    move_caret(&b, "-- two editors\n".len());
    type_at(&b, "# a comment\n");
    b.poll_collab();
    a.poll_collab();
    assert_eq!(
        text_of(&a),
        text_of(&b),
        "converged again, polling in the other order"
    );
    assert!(text_of(&a).contains("hello!"), "{}", text_of(&a));
    assert!(text_of(&a).contains("# a comment"), "{}", text_of(&a));

    // Re-polling is a no-op: the log is only consumed once, and applying a
    // duplicate operation is refused by its stamp.
    let settled = text_of(&a);
    a.poll_collab();
    b.poll_collab();
    assert_eq!(text_of(&a), settled);
    assert_eq!(text_of(&b), settled);
}

#[test]
fn a_latecomer_adopts_the_shared_text() {
    let dir = Dir::new("latecomer");
    let a = studio_in(&dir);
    a.start_collab();
    // Several rounds of edits before B even exists.
    type_at(&a, " one");
    a.poll_collab();
    type_at(&a, " two");
    a.poll_collab();

    let b = studio_in(&dir);
    b.start_collab();
    b.poll_collab();
    assert_eq!(
        text_of(&b),
        text_of(&a),
        "the joiner rebuilds from the log rather than seeding a second base"
    );
    assert!(text_of(&b).contains(" one"));
    assert!(text_of(&b).contains(" two"));

    // And from here on it is an ordinary participant: its own insert lands
    // in the converged text and reaches A.
    type_at(&b, " three");
    a.poll_collab();
    assert_eq!(text_of(&a), text_of(&b));
    assert!(text_of(&a).contains(" three"));
}

#[test]
fn presence_positions_arrive_and_survive_remote_edits() {
    let dir = Dir::new("presence");
    let a = studio_in(&dir);
    let b = studio_in(&dir);
    a.start_collab();
    b.start_collab();
    b.poll_collab();

    // A parks its caret after the label's text, on line 3.
    let at = text_of(&a).find("hello").expect("the label") + "hello".len();
    move_caret(&a, at);
    b.poll_collab();

    let carets = b.collab_carets.get();
    assert_eq!(carets.len(), 1, "one remote caret: {carets:?}");
    assert_eq!(
        (carets[0].line, carets[0].col),
        (3, 19),
        "line 3, column after \"hello\": {carets:?}"
    );
    let seen = carets[0].clone();

    // A's own list never contains A: the signal holds *remote* carets only.
    let a_replica = a.collab_replica().expect("A is sharing");
    assert!(
        a.collab_carets.get().iter().all(|c| c.replica != a_replica),
        "the sender does not appear to itself"
    );

    // B's own edit inserts a line above; A's caret — anchored to its
    // character, not an offset — moves with the text.
    move_caret(&b, 0);
    type_at(&b, "-- presence\n");
    b.poll_collab();
    a.poll_collab();

    let carets = b.collab_carets.get();
    assert_eq!(
        carets.len(),
        1,
        "still one caret, and still A's: {carets:?}"
    );
    assert_eq!(
        (carets[0].line, carets[0].col),
        (4, 19),
        "the anchor carried the caret down a line"
    );
    assert_eq!(carets[0].replica, seen.replica);
    assert_eq!(carets[0].name, seen.name);

    // And stopping the session clears them — on the side that stopped.
    // (A going away silently leaves its last caret on B's side: there is no
    // leave message in v0, the limit `state/collab.rs` states.)
    a.stop_collab();
    b.stop_collab();
    assert!(b.collab_carets.get().is_empty(), "no session, no carets");
}

#[test]
fn an_out_of_band_buffer_change_is_pulled_back_to_the_shared_text() {
    let dir = Dir::new("reconcile");
    let a = studio_in(&dir);
    let b = studio_in(&dir);
    a.start_collab();
    b.start_collab();
    b.poll_collab();
    type_at(&a, " and more");
    b.poll_collab();
    let shared = text_of(&a);
    assert_eq!(text_of(&b), shared);

    // The real out-of-band path: something rewrites the file on disk, and B
    // reverts to it — a buffer change that never went through `edit`.
    std::fs::write(
        dir.path.join("src/screens/flow.say"),
        "-- a stale copy from disk\n",
    )
    .expect("overwrite the shared file");
    b.revert_active();

    assert_eq!(
        text_of(&b),
        shared,
        "the session's text wins over a change from outside it"
    );
    assert!(
        b.notice.get().is_some_and(|n| n.contains("shared")),
        "the pull-back was said out loud: {:?}",
        b.notice.get()
    );

    // And the session still works afterwards — the reconcile did not leave
    // the doc and the buffer disagreeing.
    type_at(&b, " still");
    a.poll_collab();
    assert_eq!(text_of(&a), text_of(&b));
    assert!(text_of(&a).contains("still"));
}

#[test]
fn sharing_needs_a_real_file_in_a_real_folder() {
    // No workspace: the studio opens its scratch buffer, and there is
    // nothing to share.
    let runtime = Runtime::new();
    let studio = Studio::new(&runtime);
    studio.start_collab();
    assert!(!studio.collab.get(), "nothing is shared without a folder");
    assert!(
        studio.notice.get().is_some_and(|n| n.contains("folder")),
        "the refusal says why: {:?}",
        studio.notice.get()
    );
    assert!(studio.collab_replica().is_none());

    // The command toggles: a started session stops on the second run.
    let dir = Dir::new("toggle");
    let studio = studio_in(&dir);
    studio.start_collab();
    assert!(studio.collab.get());
    studio.toggle_collab();
    assert!(!studio.collab.get(), "the second run stops the session");
    assert!(studio.collab_carets.get().is_empty());
}
