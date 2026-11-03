//! A project the studio scaffolds is a project the studio can export.
//!
//! # The gap this closes
//!
//! `export` and `scaffold` were each correct and had never been introduced.
//! The export plans ran their second step in `<project>/android` and
//! `<project>/ios`; the scaffolder wrote neither directory. Nothing failed,
//! because nothing checked: `export`'s own tests build plans against a
//! temporary directory they populate by hand, and `scaffold`'s tests read the
//! files it writes. Between the two modules there was no test at all, and the
//! symptom was a user pressing Export APK on a new project and getting a
//! message from Gradle about a directory that did not exist.
//!
//! So this file scaffolds a real project on disk and then asks `export` for a
//! real plan, with no hand-placed files in between. Every directory a step runs
//! in, and every file a step names, has to be one the scaffolder wrote.
//!
//! # What it does not do
//!
//! Run any of it. There is no Android SDK, no Gradle and no Xcode here — and
//! more to the point, an export takes minutes and needs a phone. `export` is a
//! *planner* precisely so that its output can be checked without any of that,
//! and this checks the plan against the filesystem the plan will run on.

use std::path::{Path, PathBuf};

use viewwstudio::export::{self, Format};
use viewwstudio::scaffold::{self, Dependency};
use viewwstudio::toolchains::{Env, Host, Probed};

/// A scaffolded project in a temporary directory that cleans itself up.
struct Project {
    /// The temporary directory holding both the project and the stub tools.
    base: PathBuf,
    root: PathBuf,
    name: String,
}

impl Project {
    fn new(name: &str) -> Self {
        // **A counter as well as the pid.** `cargo test` runs these in
        // parallel threads of one process, and three of them scaffold a project
        // called `my-app`; without this they share a directory, and the second
        // one to start deletes the first one's files mid-assertion. That failure
        // is intermittent and reads like a bug in the scaffolder.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "vieww-scaffold-export-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join(name);
        scaffold::create(
            &root,
            name,
            &Dependency::Path(PathBuf::from("/somewhere/vieww")),
        )
        .expect("scaffolding a project");
        Self {
            base,
            root,
            name: name.to_owned(),
        }
    }

    /// Where the stub tools live — beside the project, never inside it, because
    /// `scaffold::create` refuses a directory that already holds anything.
    fn bin(&self) -> PathBuf {
        self.base.join("bin")
    }

    fn has(&self, relative: &str) -> bool {
        self.root.join(relative).exists()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// An environment with every requirement satisfied, so the plan is produced
/// rather than refused — the refusal paths are `export`'s own tests' subject.
///
/// Built by hand rather than probed: `Env`'s fields are public for exactly this
/// reason, so a checklist can be rendered on a machine with no Android SDK, no
/// Xcode and no keychain. The stub tools are real empty files marked executable,
/// because `find_on_path` looks at the mode.
fn ready(host: Host, bin: &Path) -> Env {
    use std::collections::BTreeMap;
    std::fs::create_dir_all(bin).expect("mkdir");
    for tool in [
        "cargo",
        "cargo-ndk",
        "gradle",
        "javac",
        "xcodebuild",
        "xcrun",
        "wasm-bindgen",
        "dpkg-deb",
    ] {
        let path = bin.join(tool);
        std::fs::write(&path, "").expect("write stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }

    let mut vars = BTreeMap::new();
    for (key, folder) in [
        ("ANDROID_HOME", "sdk"),
        ("ANDROID_NDK_HOME", "ndk"),
        ("JAVA_HOME", "jdk"),
    ] {
        let path = bin.join(folder);
        std::fs::create_dir_all(&path).expect("mkdir");
        vars.insert(key.to_string(), path.to_string_lossy().into_owned());
    }

    Env {
        host,
        vars,
        path: vec![bin.to_path_buf()],
        rust_targets: Probed::Given(vec![
            "aarch64-linux-android".into(),
            "aarch64-apple-ios".into(),
            "aarch64-apple-ios-sim".into(),
            "wasm32-unknown-unknown".into(),
        ]),
        signing_identity: Probed::Given(Some("Apple Development: Test (TEAMID)".into())),
    }
}

#[test]
fn every_directory_an_export_step_runs_in_is_one_the_scaffolder_wrote() {
    // The bug this is about, stated directly: a `Spec`'s working directory is
    // where the process is spawned, and spawning in a directory that does not
    // exist fails before the command is even looked up.
    let project = Project::new("my-app");
    for (format, host) in [
        (Format::DesktopBinary, Host::Linux),
        (Format::MacAppBundle, Host::MacOs),
        (Format::LinuxDeb, Host::Linux),
        (Format::Web, Host::Linux),
        (Format::AndroidApk, Host::Linux),
        (Format::IosSimulatorApp, Host::MacOs),
        (Format::IosIpa, Host::MacOs),
    ] {
        let plan = export::plan(
            &ready(host, &project.bin()),
            &project.root,
            &project.name,
            format,
        )
        .unwrap_or_else(|refusal| panic!("{format:?} refused: {refusal}"));
        for step in &plan.steps {
            let dir = &step.spec.dir;
            // The export directory is created by the plan's own first copy
            // step, so it is allowed not to exist yet.
            if dir.starts_with(project.root.join("export")) {
                continue;
            }
            assert!(
                dir.is_dir(),
                "{format:?} step {:?} runs in {}, which nothing writes",
                step.spec.label,
                dir.display()
            );
        }
    }
}

#[test]
fn the_gradle_project_is_where_the_android_plan_expects_it() {
    let project = Project::new("my-app");
    assert!(project.has("android/settings.gradle"));
    assert!(project.has("android/app/build.gradle"));
    assert!(project.has("android/app/src/main/AndroidManifest.xml"));

    let plan = export::plan(
        &ready(Host::Linux, &project.bin()),
        &project.root,
        &project.name,
        Format::AndroidApk,
    )
    .expect("a plan");

    // Two steps in two directories: the Rust at the project root, the packaging
    // in `android/`. Getting these the wrong way round produces a Gradle error
    // about a missing settings file and a cargo error about a missing manifest,
    // neither of which says "wrong directory".
    assert_eq!(plan.steps[0].spec.dir, project.root);
    assert_eq!(plan.steps[1].spec.dir, project.root.join("android"));
}

#[test]
fn a_project_with_no_wrapper_is_built_with_gradle_and_one_with_a_wrapper_is_not() {
    // The scaffolder writes `gradle-wrapper.properties` and no JAR, so a new
    // project has no `gradlew` to run. Hard-coding either answer is wrong for
    // half a project's life, so `export` looks.
    let project = Project::new("wrapperless");
    assert!(
        !project.has("android/gradlew"),
        "a scaffolder does not emit a binary JAR, so there is no wrapper yet"
    );
    let plan = export::plan(
        &ready(Host::Linux, &project.bin()),
        &project.root,
        &project.name,
        Format::AndroidApk,
    )
    .expect("a plan");
    assert_eq!(plan.steps[1].spec.program, "gradle");

    // Now somebody has run `gradle wrapper` once. The wrapper `gradle` writes
    // is `gradlew` plus `gradlew.bat`, and `export::gradle_command` looks for
    // the one this host would run — so the test writes that one, rather than
    // the Unix name on every platform, which left Windows still planning
    // `gradle`.
    let wrapper = if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    };
    std::fs::write(project.root.join("android").join(wrapper), "#!/bin/sh\n").expect("write");
    let plan = export::plan(
        &ready(Host::Linux, &project.bin()),
        &project.root,
        &project.name,
        Format::AndroidApk,
    )
    .expect("a plan");
    assert!(
        plan.steps[1].spec.program.contains("gradlew"),
        "the pinned version wins once it is available: {}",
        plan.steps[1].spec.program
    );
}

#[test]
fn the_xcode_project_carries_the_scheme_the_ios_plans_name() {
    // `xcodebuild -scheme <name>` fails with "scheme not found" when the only
    // scheme is the per-user one Xcode generates on first open — which a build
    // run by the studio never triggers. The scaffolder writes a shared one.
    let project = Project::new("my-app");
    let scheme = format!(
        "ios/my-app.xcodeproj/xcshareddata/xcschemes/{}.xcscheme",
        project.name
    );
    assert!(project.has(&scheme), "missing {scheme}");
    assert!(
        project.has("ios/ExportOptions.plist"),
        "the .ipa export reads this"
    );

    for format in [Format::IosSimulatorApp, Format::IosIpa] {
        let plan = export::plan(
            &ready(Host::MacOs, &project.bin()),
            &project.root,
            &project.name,
            format,
        )
        .expect("a plan");
        let xcode = plan
            .steps
            .iter()
            .find(|step| step.spec.program == "xcodebuild")
            .expect("an xcodebuild step");
        let scheme_arg = xcode
            .spec
            .args
            .iter()
            .position(|arg| arg == "-scheme")
            .map(|at| &xcode.spec.args[at + 1])
            .expect("-scheme");
        assert_eq!(scheme_arg, &project.name, "{format:?}");
    }
}

#[test]
fn the_ipa_export_names_an_options_file_that_exists() {
    // `-exportOptionsPlist ExportOptions.plist` is resolved relative to the
    // step's working directory, so this is a two-part claim: the plan says
    // `ios/`, and the file is in `ios/`.
    let project = Project::new("my-app");
    let plan = export::plan(
        &ready(Host::MacOs, &project.bin()),
        &project.root,
        &project.name,
        Format::IosIpa,
    )
    .expect("a plan");

    let step = plan
        .steps
        .iter()
        .find(|step| {
            step.spec
                .args
                .iter()
                .any(|arg| arg == "-exportOptionsPlist")
        })
        .expect("an export step");
    let at = step
        .spec
        .args
        .iter()
        .position(|arg| arg == "-exportOptionsPlist")
        .expect("the flag");
    let named = &step.spec.args[at + 1];
    assert!(
        step.spec.dir.join(named).is_file(),
        "{} is not in {}",
        named,
        step.spec.dir.display()
    );
}

#[test]
fn the_generated_xml_and_property_lists_parse() {
    // Not a schema check — a well-formedness check, which is the class of
    // mistake a generated file actually makes: an unescaped `&` in a project
    // name, a tag closed in the wrong order after an edit. Xcode's and AAPT's
    // messages for those name a byte offset.
    let project = Project::new("my-app");
    for relative in [
        "android/app/src/main/AndroidManifest.xml",
        "ios/App/Info.plist",
        "ios/ExportOptions.plist",
        "ios/my-app.xcodeproj/xcshareddata/xcschemes/my-app.xcscheme",
        "macos/Info.plist",
    ] {
        let text = std::fs::read_to_string(project.root.join(relative))
            .unwrap_or_else(|_| panic!("reading {relative}"));
        well_formed(&text, relative);
    }
}

#[test]
fn the_project_file_is_balanced_and_every_reference_resolves() {
    // A `.pbxproj` is a graph keyed by 24-character identifiers, and Xcode's
    // error for a dangling one is "The project cannot be opened because it is
    // in a future Xcode format" — which is wrong, and sends people looking at
    // the wrong thing entirely. So: every identifier that is *used* has to be
    // one that is *defined*.
    let project = Project::new("my-app");
    let text = std::fs::read_to_string(project.root.join("ios/my-app.xcodeproj/project.pbxproj"))
        .expect("reading the project");

    assert_eq!(
        text.matches('{').count(),
        text.matches('}').count(),
        "unbalanced braces"
    );
    assert_eq!(
        text.matches('(').count(),
        text.matches(')').count(),
        "unbalanced parentheses"
    );

    let ids: Vec<&str> = text
        .split(|c: char| !c.is_ascii_hexdigit())
        .filter(|word| word.len() == 24 && word.chars().all(|c| c.is_ascii_hexdigit()))
        .collect();
    assert!(ids.len() > 10, "this file is made of these: {}", ids.len());

    // Defined = appears at the head of an object, which in this format is a
    // line whose first token is the identifier and which assigns an object
    // (`{id} = {` or `{id} /* … */ = {`). Matched on the trimmed line rather
    // than a fixed indent: the file's indentation is generated, and a check
    // that read it as structure would break the day it changed without the
    // graph changing at all.
    for id in &ids {
        let defined = text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with(id) && line.contains("= {")
        });
        assert!(defined, "{id} is referenced but never defined");
    }

    assert!(text.starts_with("// !$*UTF8*$!"), "the format's own header");
    assert!(text.contains("rootObject = "), "and a root to start from");
}

#[test]
fn the_library_the_manifest_loads_is_the_library_cargo_builds() {
    // Three files have to agree on one name, and none of them checks the other
    // two: `Cargo.toml`'s `[lib] name`, the Android manifest's
    // `android.app.lib_name`, and the Xcode project's `-l` flag. When they
    // disagree, everything builds and the app dies at launch.
    let project = Project::new("my-app");
    let read = |relative: &str| {
        std::fs::read_to_string(project.root.join(relative))
            .unwrap_or_else(|_| panic!("reading {relative}"))
    };

    let manifest = read("Cargo.toml");
    assert!(manifest.contains(r#"name = "my_app""#), "{manifest}");
    assert!(
        manifest.contains(r#"crate-type = ["rlib", "cdylib", "staticlib"]"#),
        "the three shapes the three platforms need: {manifest}"
    );

    let android = read("android/app/src/main/AndroidManifest.xml");
    assert!(android.contains(r#"android:value="my_app""#), "{android}");

    let xcode = read("ios/my-app.xcodeproj/project.pbxproj");
    assert!(xcode.contains(r#""-lmy_app""#), "{xcode}");
}

#[test]
fn the_ios_entry_point_the_objective_c_calls_is_one_the_rust_exports() {
    // Two files, one symbol, and a linker error that names a mangled string if
    // they drift.
    let project = Project::new("my-app");
    let main_m = std::fs::read_to_string(project.root.join("ios/App/main.m")).expect("main.m");
    let lib = std::fs::read_to_string(project.root.join("src/lib.rs")).expect("lib.rs");

    assert!(main_m.contains("extern void vieww_ios_main(void);"));
    assert!(main_m.contains("vieww_ios_main();"));
    assert!(
        lib.contains(r#"pub extern "C" fn vieww_ios_main()"#),
        "{lib}"
    );
    assert!(
        lib.contains("#[no_mangle]"),
        "an unmangled symbol is the whole point"
    );
}

/// A deliberately small well-formedness check: tags balance and nest.
///
/// A full parser is a dependency this crate does not have and does not need —
/// what is being checked is that a *generated* file is not malformed, and the
/// ways a generated file goes wrong are an unclosed tag and a mis-nested one.
fn well_formed(text: &str, what: &str) {
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else {
            panic!("{what}: a `<` with no `>`");
        };
        let tag = &rest[..close];
        rest = &rest[close + 1..];

        // Declarations, doctypes, comments and self-closing tags open nothing.
        if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            let opened = stack
                .pop()
                .unwrap_or_else(|| panic!("{what}: </{name}> with nothing open"));
            assert_eq!(opened, name.trim(), "{what}: mis-nested tags");
        } else {
            let name = tag.split_whitespace().next().unwrap_or(tag);
            stack.push(name);
        }
    }
    assert!(stack.is_empty(), "{what}: unclosed {stack:?}");
}

#[test]
fn the_web_plan_builds_what_the_scaffolded_page_mounts() {
    // Three files have to agree on one module name, and none of them checks
    // the others: the wasm cargo will emit (`my_app.wasm`, the *library's*
    // name), the `.js` the page imports, and the `wasm-bindgen` invocation the
    // plan runs. When they disagree the page boots forever on "booting…".
    let project = Project::new("my-app");
    let plan = export::plan(
        &ready(Host::Linux, &project.bin()),
        &project.root,
        &project.name,
        Format::Web,
    )
    .expect("a plan");

    let glue = plan
        .steps
        .iter()
        .find(|s| s.spec.program == "wasm-bindgen")
        .expect("the bindgen step");
    let wasm = glue
        .spec
        .args
        .iter()
        .find(|arg| arg.ends_with(".wasm"))
        .expect("the .wasm it binds");
    assert!(wasm.ends_with("my_app.wasm"), "{wasm}");

    let page = std::fs::read_to_string(project.root.join("web/index.html")).expect("the page");
    assert!(
        page.contains("./my_app.js"),
        "the page imports what bindgen will emit from my_app.wasm: {page}"
    );
    assert!(
        page.contains(r#"id="vieww-canvas""#),
        "the page carries the canvas the library's `start` mounts on"
    );
    // And the library really does mount on that id — the two ends of the web
    // export, checked in one place so they cannot drift.
    let lib = std::fs::read_to_string(project.root.join("src/lib.rs")).expect("the library");
    assert!(
        lib.contains(r#"WebSurface::by_id("vieww-canvas")"#),
        "the library mounts the canvas the page carries: {lib}"
    );
}

#[test]
fn the_mac_bundle_lays_out_the_plist_the_scaffolder_wrote() {
    let project = Project::new("my-app");
    let plan = export::plan(
        &ready(Host::MacOs, &project.bin()),
        &project.root,
        &project.name,
        Format::MacAppBundle,
    )
    .expect("a plan");

    let plist = std::fs::read_to_string(project.root.join("macos/Info.plist")).expect("the plist");
    // The executable the bundle runs is the one the plan copies into
    // `Contents/MacOS/`, and the identifier is the reserved one nobody can
    // publish by accident.
    assert!(plist.contains("<string>my-app</string>"), "{plist}");
    assert!(
        plist.contains("<string>com.example.my_app</string>"),
        "{plist}"
    );
    let copy = plan
        .steps
        .iter()
        .map(|s| s.spec.command_line())
        .find(|line| line.contains("Info.plist"))
        .expect("the plist copy");
    assert!(copy.contains("Contents/Info.plist"), "{copy}");
}

#[test]
fn the_deb_stages_the_control_and_desktop_the_scaffolder_wrote() {
    let project = Project::new("my-app");
    let plan = export::plan(
        &ready(Host::Linux, &project.bin()),
        &project.root,
        &project.name,
        Format::LinuxDeb,
    )
    .expect("a plan");

    // The control file names this machine's architecture — `dpkg -i` refuses
    // anything else, which is why the plan reads it before building.
    let control = std::fs::read_to_string(project.root.join("linux/control")).expect("control");
    let arch = viewwstudio::harness::debian_arch();
    assert!(
        control.contains(&format!("Architecture: {arch}")),
        "the scaffolded control names this machine: {control}"
    );
    let package = control
        .lines()
        .find_map(|line| line.strip_prefix("Package: "))
        .expect("a package name");
    assert_eq!(package, "my_app", "the package is the library's name");

    // Both staged files are the scaffolder's, under the paths dpkg-deb and a
    // desktop expect respectively.
    let lines: Vec<String> = plan.steps.iter().map(|s| s.spec.command_line()).collect();
    assert!(
        lines.iter().any(|l| l.contains("linux/control")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("linux/my-app.desktop") && l.contains("applications")),
        "the .desktop entry lands where launchers look: {lines:?}"
    );
    assert!(
        project.root.join("linux/my-app.desktop").is_file(),
        "the .desktop entry exists to be staged"
    );
}

#[test]
fn a_release_apk_signs_with_the_environment_the_scaffolded_gradle_reads() {
    // Two ends of release signing, in one place: the plan hands Gradle
    // VIEWW_KEYSTORE and VIEWW_KEYSTORE_ALIAS in its environment, and the
    // scaffolder's build.gradle assembles its signingConfig from exactly those
    // names. Neither file knows the other exists.
    let project = Project::new("my-app");
    let keystore = project.base.join("release.keystore");
    std::fs::write(&keystore, "a keystore").expect("write keystore");
    let mut env = ready(Host::Linux, &project.bin());
    env.vars.insert(
        viewwstudio::export::KEYSTORE_VAR.to_string(),
        keystore.to_string_lossy().into_owned(),
    );
    env.vars.insert(
        viewwstudio::export::KEYSTORE_ALIAS_VAR.to_string(),
        "upload".into(),
    );

    let plan = export::plan(
        &env,
        &project.root,
        &project.name,
        Format::AndroidReleaseApk,
    )
    .expect("a plan");
    let packaging = plan
        .steps
        .iter()
        .find(|s| s.spec.label == "Package release APK")
        .expect("the signing step");
    assert!(
        packaging
            .spec
            .args
            .iter()
            .any(|arg| arg == "assembleRelease"),
        "the release build type is what gets assembled"
    );
    for var in [
        viewwstudio::export::KEYSTORE_VAR,
        viewwstudio::export::KEYSTORE_ALIAS_VAR,
    ] {
        assert!(
            packaging.spec.env.iter().any(|(key, _)| key == var),
            "{var} reaches the child Gradle reads"
        );
    }

    let gradle =
        std::fs::read_to_string(project.root.join("android/app/build.gradle")).expect("gradle");
    for line in [
        r#"System.getenv("VIEWW_KEYSTORE")"#,
        r#"System.getenv("VIEWW_KEYSTORE_ALIAS")"#,
        r#"System.getenv("VIEWW_KEYSTORE_PASSWORD")"#,
    ] {
        assert!(
            gradle.contains(line),
            "the generated Gradle reads the same variable the plan hands it: {line}"
        );
    }
    // And the scaffolded library really does have a wasm entry for the page to
    // start — the Web export's other half, present in the same project.
    let lib = std::fs::read_to_string(project.root.join("src/lib.rs")).expect("the library");
    assert!(
        lib.contains("#[wasm_bindgen::prelude::wasm_bindgen(start)]"),
        "the library has a browser entry point: {lib}"
    );
}

#[test]
fn the_scaffolded_cargo_manifest_gates_winit_off_the_web_target() {
    // The wasm32 build fails inside winit unless the platform dependency is
    // target-gated, and a manifest that looks right but is not is exactly the
    // `export_check --only web` compile-level failure this guards against in
    // the meantime.
    let project = Project::new("my-app");
    let manifest = std::fs::read_to_string(project.root.join("Cargo.toml")).expect("the manifest");
    // The universal table: from the `[dependencies]` header to the next table
    // header, with `#` comment lines dropped — a comment that *names* the
    // gated dependency (as the one explaining the gating does) is not a
    // dependency, and counting it as one would make this test assert nothing.
    let dependencies = manifest
        .split("[dependencies]\n")
        .nth(1)
        .unwrap_or_default()
        .lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(
        !dependencies.contains("vieww-platform-winit"),
        "the winit platform is not in the universal table:\n{dependencies}"
    );
    assert!(
        manifest.contains("[target.'cfg(not(target_arch = \"wasm32\"))'.dependencies]"),
        "it is gated to the non-web targets:\n{manifest}"
    );
    assert!(
        manifest.contains("[target.'cfg(target_arch = \"wasm32\")'.dependencies]"),
        "the web platform has its own table:\n{manifest}"
    );
}

/// Every path under `android/`, `ios/`, `web/`, `macos/` and `linux/` belongs
/// to `harness`, so a stray file written by something else would show up here.
#[test]
fn the_harness_module_owns_every_platform_file() {
    let project = Project::new("my-app");
    let generated: Vec<PathBuf> = viewwstudio::harness::files("my-app")
        .into_iter()
        .map(|(path, _)| project.root.join(path))
        .collect();

    for dir in ["android", "ios", "web", "macos", "linux"] {
        walk(&project.root.join(dir), &mut |path| {
            assert!(
                generated.contains(&path.to_path_buf()),
                "{} is on disk but `harness::files` does not produce it",
                path.display()
            );
        });
    }
}

fn walk(dir: &Path, seen: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, seen);
        } else {
            seen(&path);
        }
    }
}
