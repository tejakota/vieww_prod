//! ST-11: Explorer file operations undo and redo, in reverse order, back to
//! a byte-identical tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use vieww_element::Runtime;
use viewwstudio::command::Command;
use viewwstudio::state::{NameKind, Studio};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("viewwstudio-journal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(std::fs::canonicalize(&path).unwrap_or(path))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every file under `root` (the trash excluded) with its bytes, and every
/// directory.
fn tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.starts_with(".viewwstudio-trash") {
                continue;
            }
            if p.is_dir() {
                out.insert(format!("{rel}/"), None);
                walk(root, &p, out);
            } else {
                out.insert(rel, Some(std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn name(studio: &Studio, kind: NameKind, at: &Path, text: &str) {
    studio.ask_for_name(kind, at);
    studio.set_prompt_name(text.to_owned());
    studio.confirm_name_prompt();
}

#[test]
fn file_operations_undo_and_redo_to_identical_trees() {
    let dir = Dir::new("ops");
    std::fs::write(dir.0.join("keep.rs"), "fn keep() {}\n").unwrap();
    std::fs::write(dir.0.join("old.rs"), "fn old() {}\n").unwrap();
    std::fs::create_dir(dir.0.join("doomed")).unwrap();
    std::fs::write(dir.0.join("doomed/inner.txt"), "contents").unwrap();
    let runtime = Runtime::new();
    let studio = Studio::new(&runtime);
    studio.open_workspace(&dir.0);
    assert!(
        !studio.can_run(Command::UndoFileOperation),
        "a fresh workspace has nothing to undo"
    );

    let mut states = vec![tree(&dir.0)];
    name(&studio, NameKind::NewFile, &dir.0, "fresh.rs");
    states.push(tree(&dir.0));
    name(&studio, NameKind::NewFolder, &dir.0, "screens");
    states.push(tree(&dir.0));
    name(
        &studio,
        NameKind::Rename,
        &dir.0.join("old.rs"),
        "renamed.rs",
    );
    states.push(tree(&dir.0));
    studio.ask_to_delete(dir.0.join("doomed"));
    studio.confirm_delete();
    states.push(tree(&dir.0));

    assert!(states[1].contains_key("fresh.rs"));
    assert!(states[2].contains_key("screens/"));
    assert!(states[3].contains_key("renamed.rs") && !states[3].contains_key("old.rs"));
    assert!(!states[4].contains_key("doomed/"));

    // Undo all four, newest first: each step lands on the state before it.
    for want in states.iter().rev().skip(1) {
        assert!(studio.can_run(Command::UndoFileOperation));
        studio.run(Command::UndoFileOperation);
        assert_eq!(
            &tree(&dir.0),
            want,
            "an undo did not restore the earlier tree"
        );
    }
    assert!(!studio.can_run(Command::UndoFileOperation));

    // And redo all four.
    for want in states.iter().skip(1) {
        studio.run(Command::RedoFileOperation);
        assert_eq!(
            &tree(&dir.0),
            want,
            "a redo did not reproduce the later tree"
        );
    }
    assert!(!studio.can_run(Command::RedoFileOperation));
}

#[test]
fn an_undo_never_overwrites() {
    let dir = Dir::new("collide");
    std::fs::write(dir.0.join("a.rs"), "original").unwrap();
    let runtime = Runtime::new();
    let studio = Studio::new(&runtime);
    studio.open_workspace(&dir.0);
    name(&studio, NameKind::Rename, &dir.0.join("a.rs"), "b.rs");
    // Something new now sits where the undo would put the file back.
    std::fs::write(dir.0.join("a.rs"), "someone else's").unwrap();
    studio.run(Command::UndoFileOperation);
    assert_eq!(
        std::fs::read_to_string(dir.0.join("a.rs")).unwrap(),
        "someone else's"
    );
    assert_eq!(
        std::fs::read_to_string(dir.0.join("b.rs")).unwrap(),
        "original"
    );
    assert!(
        studio.can_run(Command::UndoFileOperation),
        "the refused undo stays on the stack"
    );
}
