//! The one-shot keystore password prompt behind a release-signed APK export
//! (ST-07).
//!
//! # What this file proves, and what it deliberately does not
//!
//! The *plan* for a release APK — its steps, its refusal when the keystore or
//! the alias is unset, the password's absence from the plan's own data — is
//! `export`'s own test suite's subject, and `tests/scaffold_exports.rs` proves
//! the Gradle project it runs against reads the same environment variables.
//! What only the state layer can prove is the *lifecycle of the password*:
//!
//! * asking for it only when the export was actually requested and the
//!   studio's own environment does not already have one (the CI path);
//! * handing it to the packaging step's child process environment — and
//!   nowhere else, least of all the Output panel, which is a log;
//! * clearing it the moment the prompt closes, whichever way it closed.
//!
//! No Gradle, no keystore and no Android SDK exist on the machine this runs
//! on, and none are needed: the assertions are about where a secret goes and
//! when it stops existing, not about a signed APK. A machine that *did* have
//! the whole chain would exercise exactly the same code paths.

use std::path::{Path, PathBuf};

use vieww_element::Runtime;
use viewwstudio::export::Format;
use viewwstudio::state::Studio;

/// A minimal project with a manifest, so the export has a package name to
/// plan for. The platform files a real scaffold writes are not needed: the
/// release plan reads none of them.
fn project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "vieww-release-signing-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("the project tree");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n\n[workspace]\n",
    )
    .expect("the manifest");
    std::fs::write(root.join("src").join("lib.rs"), "// the app\n").expect("the library");
    root
}

/// A studio with that project open, the way a launch would have it.
fn studio(root: &Path) -> Studio {
    let runtime = Runtime::new();
    Studio::with_workspace(&runtime, viewwstudio::buffer::Workspace::open(root))
        .with_host(vieww_foundation::TargetPlatform::Linux)
}

#[test]
fn a_release_export_asks_for_the_password_before_touching_the_toolchain() {
    let root = project("prompt");
    let studio = studio(&root);
    assert!(
        std::env::var(viewwstudio::export::KEYSTORE_PASSWORD_VAR).is_err(),
        "this test's premise is a studio with no password in its environment — \
         VIEWW_KEYSTORE_PASSWORD is set where it runs"
    );

    studio.start_export(Format::AndroidReleaseApk);
    assert!(
        studio.keystore_prompt.get(),
        "the export pauses for the one-shot prompt"
    );
    // Nothing was typed yet, and nothing is running: the question comes before
    // a three-minute build whose last step would have been this same prompt.
    assert_eq!(studio.keystore_password.get(), "");
    assert!(!studio.exporting());
}

#[test]
fn confirming_with_nothing_typed_starts_nothing_and_says_why() {
    let root = project("empty");
    let studio = studio(&root);
    studio.start_export(Format::AndroidReleaseApk);

    studio.confirm_keystore_prompt();
    assert!(!studio.keystore_prompt.get(), "the prompt closed");
    assert_eq!(studio.keystore_password.get(), "");
    // The user is told why nothing happened, in one line, rather than the
    // export silently not starting.
    let notice = studio.notice.get().unwrap_or_default();
    assert!(
        notice.contains("no password typed"),
        "the notice explains itself: {notice}"
    );
}

#[test]
fn cancelling_clears_what_was_typed_rather_than_keeping_it() {
    let root = project("cancel");
    let studio = studio(&root);
    studio.start_export(Format::AndroidReleaseApk);
    studio.set_keystore_password("half-typed".into());

    studio.cancel_keystore_prompt();
    assert!(!studio.keystore_prompt.get());
    // The point of the clear: a prompt that keeps the typed half of a password
    // in a signal is a prompt that remembered one.
    assert_eq!(studio.keystore_password.get(), "");
}

#[test]
fn the_password_reaches_the_child_and_nothing_else() {
    let root = project("handoff");
    let studio = studio(&root);
    studio.start_export(Format::AndroidReleaseApk);
    studio.set_keystore_password("sekrit-value".into());

    studio.confirm_keystore_prompt();

    // The prompt is gone and the signal is empty: the studio's only copy was
    // the local one on its way to the packaging step's environment.
    assert!(!studio.keystore_prompt.get());
    assert_eq!(
        studio.keystore_password.get(),
        "",
        "confirming spends the password; it does not keep it"
    );

    // Whether the plan was refused (no Android toolchain here, the ordinary
    // case) or started, everything the Output panel ever shows is command
    // lines and refusals — `Spec::command_line` prints program and arguments,
    // never environment. This is the regression line for that property: the
    // day someone "helpfully" prints the child's environment to make a
    // signing failure easier to debug, this goes red.
    let output = studio.output.get();
    let joined = output.join("\n");
    assert!(
        !joined.contains("sekrit-value"),
        "the Output panel is a log and must never see the password:\n{joined}"
    );
}

#[test]
fn the_prompt_belongs_to_the_release_format_alone() {
    // The debug-signed APK never asks: it is signed with the key every Android
    // SDK ships, which is the whole reason it exists. A prompt that appeared
    // for it too would be asking for a password nothing was going to use.
    let root = project("debug-too");
    let studio = studio(&root);
    studio.start_export(Format::AndroidApk);
    assert!(
        !studio.keystore_prompt.get(),
        "no password was needed, so none was asked for"
    );
    assert_eq!(studio.keystore_password.get(), "");
}
