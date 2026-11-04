//! ST-05: component libraries — components shared across projects through
//! git, driven end to end against real repositories on disk (no network:
//! the "remote" is a bare repository in a temp directory, which git treats
//! exactly as it treats one on a server).

use std::path::{Path, PathBuf};
use std::process::Command;

use viewwstudio::component_libraries::{
    add_library, libraries, publish, sync_library, url_error, Library, Published,
};
use viewwstudio::components::Component;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vieww-complib-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A bare "server" repository seeded with one component, `Badge`.
fn seeded_remote(base: &Path) -> PathBuf {
    let remote = base.join("team-ui.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "--quiet", "--bare"]);
    let work = base.join("seed");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "--quiet"]);
    std::fs::create_dir_all(work.join("Badge")).unwrap();
    std::fs::write(
        work.join("Badge/component.say"),
        "-- extracted from home.say by vieww Studio's Extract Widget\n\
         a card, padded 8:\n    a text \"new\"\n",
    )
    .unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "--quiet", "-m", "seed"]);
    git(
        &work,
        &["push", "--quiet", remote.to_str().unwrap(), "HEAD:main"],
    );
    git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    remote
}

fn note() -> Component {
    Component {
        name: "Note".to_owned(),
        source: "a card, padded 12:\n    a text \"hi\"\n".to_owned(),
        from: "settings.say".to_owned(),
    }
}

#[test]
fn a_library_is_cloned_listed_and_published_to() {
    let base = temp("publish");
    let remote = seeded_remote(&base);
    let url = remote.to_string_lossy().into_owned();
    let root = base.join("libraries");

    // Added: cloned, and its component listed like a local one.
    let library = add_library(&root, &url).expect("the clone");
    assert_eq!(library.name(), "team-ui");
    let names: Vec<String> = library.components().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Badge"]);
    assert_eq!(
        libraries(&root),
        vec![library.clone()],
        "found again by scanning"
    );

    // Adding the same address again is a sync, not a second clone.
    let again = add_library(&root, &format!("{url}/")).expect("re-add");
    assert_eq!(again.dir, library.dir);
    assert_eq!(libraries(&root).len(), 1);

    // Published: committed and pushed — the server has it.
    assert_eq!(publish(&note(), &library), Ok(Published::Pushed));
    let on_server = git(&remote, &["show", "main:Note/component.say"]);
    assert!(on_server.contains("a text \"hi\""), "{on_server}");
    assert!(on_server.contains("from settings.say"), "{on_server}");

    // A teammate's clone sees it after a sync.
    let other_root = base.join("teammate");
    let theirs = add_library(&other_root, &url).expect("their clone");
    let names: Vec<String> = theirs.components().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Badge", "Note"]);

    // Publishing the same component again changes nothing.
    assert_eq!(publish(&note(), &library), Ok(Published::Unchanged));

    // A newer version from the teammate reaches the first clone by sync.
    let newer = Component {
        source: "a card, padded 16:\n    a text \"hello\"\n".to_owned(),
        ..note()
    };
    assert_eq!(publish(&newer, &theirs), Ok(Published::Pushed));
    sync_library(&library.dir).expect("sync");
    let mine = library
        .components()
        .into_iter()
        .find(|c| c.name == "Note")
        .expect("Note");
    assert!(mine.source.contains("hello"), "{}", mine.source);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_refused_push_keeps_the_commit_and_says_why() {
    let base = temp("refused");
    let remote = seeded_remote(&base);
    let root = base.join("libraries");
    let library = add_library(&root, remote.to_str().unwrap()).expect("clone");
    // The server stops accepting pushes (a protected branch, no rights).
    let hook = remote.join("hooks/pre-receive");
    std::fs::write(&hook, "#!/bin/sh\necho 'protected branch' >&2\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    match publish(&note(), &library) {
        Ok(Published::CommittedOnly(why)) => assert!(!why.is_empty()),
        other => panic!("expected a kept commit, got {other:?}"),
    }
    // The commit is in the clone, waiting.
    let log = git(&library.dir, &["log", "--oneline", "-1"]);
    assert!(log.contains("Add component Note"), "{log}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn bad_addresses_never_reach_git() {
    let root = temp("bad");
    assert!(add_library(&root, "--upload-pack=touch /tmp/x").is_err());
    assert!(add_library(&root, "").is_err());
    assert!(url_error("https://example.com/a b").is_some());
    assert!(libraries(&root).is_empty());
    // A clone that fails says what git said, and leaves nothing behind.
    let missing = root.join("no-such-repo.git");
    let err = add_library(&root, missing.to_str().unwrap()).expect_err("no repo");
    assert!(!err.is_empty());
    assert!(libraries(&root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
    let _: Option<Library> = None;
}
