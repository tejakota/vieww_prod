//! N5: turning a project into something you can hand to somebody.
//!
//! # What this module is, and what it deliberately is not
//!
//! It is a **planner**. Given the environment ([`toolchains::Env`]), a target
//! and a chosen [`Format`], it returns either a [`Plan`] — an ordered list of
//! child processes with an artefact expected at the end — or a [`Refusal`]
//! saying why there is nothing worth starting.
//!
//! It is not a runner. Running is [`jobs::Queue`](crate::jobs)'s job, which
//! already streams, cancels and reports. Keeping the plan separate from the
//! run is what makes every case here reachable from a test on a machine with
//! no Android SDK, no Xcode, and no phone plugged in — which is this container,
//! and every CI runner.
//!
//! # Refusal before start is the whole design
//!
//! The alternative — start `cargo ndk`, let it fail, print the error — costs a
//! process launch and produces a message about a linker when the real answer
//! is "you have not installed the NDK". [`toolchains`] already knows what is
//! missing and what command installs it. This asks it *first*, and the
//! refusal names the requirement and its fix.
//!
//! The one refusal that is not about a missing tool is iOS on a machine that
//! is not a Mac. That is a rule of Apple's, not a gap to work around, and it
//! is reported as such rather than as a missing dependency somebody might go
//! looking for.
//!
//! # Nothing here is biased toward a platform
//!
//! Every target is described the same way — a list of steps and an artefact —
//! and the host's own platform appears only where it genuinely decides
//! something: the executable's file name, and Apple's macOS rule. A target
//! this studio does not yet know how to package says so; it does not fall back
//! to the desktop's answer.

use std::path::{Path, PathBuf};

use crate::task::Spec;
use crate::toolchains::{self, Env, Host, Target};

/// What a target can be asked to produce.
///
/// Separate from [`Target`] because one target has more than one answer: an
/// iOS build is a simulator `.app` or a signed `.ipa`, and those differ in
/// what they need, not only in what they emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// An optimised binary for the machine the studio is running on, copied
    /// out of `target/release` into an export directory beside it.
    DesktopBinary,
    /// A macOS `.app` bundle for the user's own application (ST-07).
    ///
    /// The layout `packaging/package.sh` builds for the *studio* — `Contents/`
    /// with `Info.plist`, the binary under `MacOS/` — generalised to any
    /// project, from the `macos/` directory the scaffolder writes.
    ///
    /// macOS only, and for the same reason iOS is: the binary inside a `.app`
    /// is a Mach-O, and no toolchain this studio can orchestrate produces one
    /// from elsewhere.
    MacAppBundle,
    /// A `.deb` of the user's own application (ST-07).
    ///
    /// `dpkg-deb`'s layout — `DEBIAN/control`, the binary under
    /// `usr/local/bin`, a `.desktop` entry — from the `linux/` directory the
    /// scaffolder writes. Linux only: a `.deb` holds a Linux binary, and a
    /// studio running elsewhere has no way to put one inside.
    LinuxDeb,
    /// A Windows `.exe`, cross-compiled when the host is not Windows.
    ///
    /// The one export that produces something for a machine the user is very
    /// likely *not* sitting at, which is exactly why it is worth having: a
    /// person developing on Linux can hand a colleague a Windows build without
    /// owning a Windows machine to make it on.
    WindowsExe,
    /// A `wasm32` bundle and the page that mounts it (ST-07).
    ///
    /// The cheapest distribution there is — no store, no signing, no installer
    /// — and until this format existed it was also the one thing the studio
    /// could not produce at all, while the framework it previews ran on the
    /// web the whole time (`crates/vieww-platform-web`, certified byte for
    /// byte against the native rasteriser by `examples/test-web`).
    Web,
    /// A debug-signed `.apk`.
    ///
    /// Debug-signed, and named so in the UI: a release-signed APK needs a
    /// keystore, a key alias and two passwords, none of which the studio has
    /// anywhere to put and none of which should be typed into a text field
    /// that logs to an Output panel. A debug-signed APK installs on any phone
    /// with developer mode on, which is what "give this to a colleague"
    /// means.
    AndroidApk,
    /// A release-signed `.apk` (ST-07).
    ///
    /// The same Gradle project as [`AndroidApk`](Self::AndroidApk) with
    /// `assembleRelease`, signed by a signing configuration the generated
    /// `build.gradle` reads from the environment — `VIEWW_KEYSTORE`,
    /// `VIEWW_KEYSTORE_ALIAS` and `VIEWW_KEYSTORE_PASSWORD` — so the keystore's
    /// location and alias come from settings and the password comes from a
    /// one-shot prompt that hands it to the child process and then forgets it.
    /// None of the three is ever written to a file this studio owns; see
    /// [`Refusal`] and `settings::Settings::keystore_alias` for the policy.
    AndroidReleaseApk,
    /// An unsigned `.app` for the iOS simulator.
    IosSimulatorApp,
    /// A signed `.ipa` for a device.
    IosIpa,
}

impl Format {
    /// Every format, in the order the export sheet lists them.
    pub const ALL: [Self; 9] = [
        Self::DesktopBinary,
        Self::MacAppBundle,
        Self::LinuxDeb,
        Self::WindowsExe,
        Self::Web,
        Self::AndroidApk,
        Self::AndroidReleaseApk,
        Self::IosSimulatorApp,
        Self::IosIpa,
    ];

    #[must_use]
    pub const fn target(self) -> Target {
        match self {
            Self::DesktopBinary | Self::MacAppBundle | Self::LinuxDeb => Target::Desktop,
            Self::WindowsExe => Target::Windows,
            Self::Web => Target::Web,
            Self::AndroidApk | Self::AndroidReleaseApk => Target::Android,
            Self::IosSimulatorApp | Self::IosIpa => Target::Ios,
        }
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::DesktopBinary => "Desktop binary",
            Self::MacAppBundle => "macOS .app",
            Self::LinuxDeb => "Linux .deb",
            Self::WindowsExe => "Windows .exe",
            Self::Web => "Web",
            Self::AndroidApk => "Android APK (debug-signed)",
            Self::AndroidReleaseApk => "Android APK (release-signed)",
            Self::IosSimulatorApp => "iOS simulator app",
            Self::IosIpa => "iOS .ipa (signed)",
        }
    }

    /// The one-line explanation under the row.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::DesktopBinary => "an optimised build for this machine, copied into export/",
            Self::MacAppBundle => "a bundle with Info.plist, from the project's macos/ directory",
            Self::LinuxDeb => "a .deb with a .desktop entry, from the project's linux/ directory",
            Self::WindowsExe => "a 64-bit Windows build; cross-compiled when this is not Windows",
            Self::Web => "a wasm32 bundle and its index.html, to serve from anywhere",
            Self::AndroidApk => "installs on any phone with developer mode on",
            Self::AndroidReleaseApk => "signed with your keystore; the password is asked for once",
            Self::IosSimulatorApp => "runs in the simulator; not installable on a device",
            Self::IosIpa => "needs a signing identity, and macOS",
        }
    }
}

/// Why an export was not started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The target cannot be built on this host whatever is installed.
    Impossible(&'static str),
    /// Something is missing. Named, with the command that installs it.
    Missing {
        requirement: &'static str,
        install: &'static str,
    },
    /// There is no project open, or it is not a cargo project.
    NoProject,
    /// The studio does not know how to package this yet, and says so rather
    /// than producing something that is not what was asked for.
    NotImplemented(&'static str),
    /// A plugin's refusal, in the plugin's own words.
    ///
    /// ST-13: a plugin export format plans through this same split, and its
    /// refusals are its own sentences — the studio cannot name another
    /// project's missing tools for it. The variant exists so a plugin's
    /// "cannot" reads as a refusal and not as a plan that silently did
    /// nothing.
    External(String),
    /// A refusal composed at runtime, because what it names only exists then
    /// — a staging directory that could not be created (ST-14), a release
    /// that publishes no checksums. The studio's own words, unlike
    /// [`External`](Self::External), but written by a `format!` rather than
    /// whole.
    Computed(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Impossible(why) => write!(f, "{why}"),
            Self::Missing {
                requirement,
                install,
            } => write!(f, "{requirement} is missing — install it with: {install}"),
            Self::NoProject => write!(f, "no cargo project is open"),
            Self::NotImplemented(what) => write!(f, "{what}"),
            Self::External(why) => write!(f, "{why}"),
            Self::Computed(why) => write!(f, "{why}"),
        }
    }
}

/// One child process in a plan, with what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What the Tasks panel calls it — "Build (release)", "Package APK".
    pub spec: Spec,
    /// One line for the export sheet's preview, so a person can see what is
    /// about to run before it runs.
    ///
    /// Owned rather than `&'static str` since ST-13: a plugin's plan carries
    /// its own one-liners, and they are not constants this crate compiled.
    pub why: String,
}

/// Everything an export will do, and what it will leave behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// What the run is called in the Output panel — the export row's title,
    /// `"toolbelt: Source tarball"` for a plugin's, the format's own title
    /// for the built-ins.
    ///
    /// A name rather than a [`Format`] since ST-13: plugin formats have no
    /// variant in that enum, and a plan that could not say what it was would
    /// report its own result anonymously.
    pub title: String,
    /// Run in order. A step that fails stops the plan — a package built from
    /// a failed compile is worse than no package.
    pub steps: Vec<Step>,
    /// Where the result is expected. Reported at the end; the studio does not
    /// open it, because what to do with a build is the user's business.
    pub artefact: PathBuf,
}

/// The directory exports are written to.
///
/// Inside `target/`, not beside the project: it is generated output, it is
/// already in every `.gitignore` that exists, and putting an APK next to
/// `src/` is how a repository acquires a fifty-megabyte binary nobody meant to
/// commit.
#[must_use]
pub fn export_dir(root: &Path) -> PathBuf {
    root.join("target").join("export")
}

/// Plan an export, or say why there is none.
///
/// `name` is the package's name, which decides the artefact's file name. The
/// caller reads it from `Cargo.toml`; it is a parameter rather than something
/// this module parses, so every case below is reachable from a test.
pub fn plan(env: &Env, root: &Path, name: &str, format: Format) -> Result<Plan, Refusal> {
    if name.is_empty() {
        return Err(Refusal::NoProject);
    }

    // The checklist first, always. See the module docs: a refusal that names
    // the missing tool and its install command is worth more than the error
    // the tool would print about itself.
    let report = toolchains::report(env, format.target());
    if let Some(why) = report.impossible {
        return Err(Refusal::Impossible(why));
    }
    // A simulator build is signed ad hoc by Xcode itself ("Sign to Run
    // Locally"); only a device `.ipa` needs a real identity. Requiring one
    // here refused the simulator export to everybody without an Apple
    // developer account, which is the format that exists for exactly them.
    let missing = report
        .missing()
        .into_iter()
        .find(|r| !(format == Format::IosSimulatorApp && r.name == "Signing identity"));
    if let Some(missing) = missing {
        return Err(Refusal::Missing {
            requirement: missing.name,
            install: missing.install,
        });
    }

    let out = export_dir(root);
    match format {
        Format::DesktopBinary => Ok(desktop(env, root, name, &out)),
        Format::MacAppBundle => mac_app_bundle(env, root, name, &out),
        Format::LinuxDeb => linux_deb(env, root, name, &out),
        Format::WindowsExe => Ok(windows(env, root, name, &out)),
        Format::Web => web(env, root, name, &out),
        Format::AndroidApk => android(env.host, root, name, &out),
        Format::AndroidReleaseApk => android_release(env, root, name, &out),
        Format::IosSimulatorApp => ios_app(env, root, name, &out),
        Format::IosIpa => ios_ipa(env, root, name, &out),
    }
}

fn desktop(env: &Env, root: &Path, name: &str, out: &Path) -> Plan {
    // The host's own naming, which is the one place a desktop export is
    // allowed to care what it is running on.
    let file = match env.host {
        Host::Windows => format!("{name}.exe"),
        _ => name.to_string(),
    };
    let built = root.join("target").join("release").join(&file);
    let artefact = out.join(&file);

    let mut steps = vec![Step {
        spec: Spec::new("Build (release)", "cargo", root).args(["build", "--release"]),
        why: "an optimised build, which is what ships".into(),
    }];
    // Copied through the queue rather than with `std::fs::copy` on the UI
    // thread: it is one more step in the same queue, cancellable and logged
    // with everything else, and a copy of a hundred-megabyte binary is not
    // free.
    steps.extend(
        copy_steps(env.host, root, &built, &artefact)
            .into_iter()
            .map(|spec| Step {
                spec,
                why: "copied out of target/release, where the next build overwrites it".into(),
            }),
    );

    Plan {
        title: Format::DesktopBinary.title().to_string(),
        steps,
        artefact,
    }
}

/// A Windows `.exe`, from whichever host is asking.
///
/// On Windows this is [`desktop`] with a different name on the row — the host
/// toolchain already produces a `.exe`, and passing `--target` there would ask
/// for a cross-compile of a machine to itself, which needs a target that is not
/// installed by default and buys nothing.
///
/// Everywhere else it is an explicit `--target x86_64-pc-windows-gnu`, which
/// also moves where the binary lands: `target/<triple>/release/` rather than
/// `target/release/`. Getting that path wrong is the classic cross-compile bug
/// — the build succeeds, the copy silently takes the *host* binary sitting in
/// `target/release` from an earlier run, and the user is handed a Linux ELF
/// named `.exe`.
fn windows(env: &Env, root: &Path, name: &str, out: &Path) -> Plan {
    const TRIPLE: &str = "x86_64-pc-windows-gnu";
    let file = format!("{name}.exe");
    let cross = env.host != Host::Windows;

    let built = if cross {
        root.join("target").join(TRIPLE).join("release").join(&file)
    } else {
        root.join("target").join("release").join(&file)
    };
    let artefact = out.join(&file);

    let spec = if cross {
        Spec::new("Build (release, Windows)", "cargo", root).args([
            "build",
            "--release",
            "--target",
            TRIPLE,
        ])
    } else {
        Spec::new("Build (release, Windows)", "cargo", root).args(["build", "--release"])
    };

    let mut steps = vec![Step {
        spec,
        why: if cross {
            "cross-compiled for Windows; the MinGW linker produces the .exe"
        } else {
            "an optimised build, which is what ships"
        }
        .into(),
    }];
    steps.extend(
        copy_steps(env.host, root, &built, &artefact)
            .into_iter()
            .map(|spec| Step {
                spec,
                why: "copied out of the target directory, where the next build overwrites it"
                    .into(),
            }),
    );

    Plan {
        title: Format::WindowsExe.title().to_string(),
        steps,
        artefact,
    }
}

/// One `mkdir -p` for every directory a layout wants, then one `cp -R` per
/// file. Unix-only, on purpose.
///
/// [`copy_steps`] branches on the host because a desktop export has to copy on
/// whatever machine asked. These layouts do not: a `.app` exists only where
/// macOS already refused-not-to be ([`mac_app_bundle`]), and a `.deb` only
/// where Linux did ([`linux_deb`]) — so by the time either reaches a copy, the
/// host is a Unix, and a Windows branch here would be code no plan could ever
/// run, which is worse than none.
fn layout_steps(root: &Path, copies: &[(&Path, &Path)]) -> Vec<Spec> {
    let mut dirs: Vec<&Path> = Vec::new();
    for (_, to) in copies {
        if let Some(parent) = to.parent() {
            if !dirs.contains(&parent) {
                dirs.push(parent);
            }
        }
    }
    // One `mkdir -p` with every directory, rather than one per file: a layout
    // is assembled as a unit, and five identical "Create export directory"
    // lines in the Output panel say nothing five times.
    let mut mkdir = Spec::new("Create bundle directories", "mkdir", root).arg("-p");
    for dir in dirs {
        mkdir = mkdir.arg(dir.to_string_lossy().to_string());
    }
    let mut steps = vec![mkdir];
    for (from, to) in copies {
        steps.push(
            Spec::new("Copy into the bundle", "cp", root)
                .arg("-R")
                .arg(from.to_string_lossy().to_string())
                .arg(to.to_string_lossy().to_string()),
        );
    }
    steps
}

/// A `.app` bundle of the *user's* application (ST-07).
///
/// The layout is `packaging/package.sh`'s, made generic: `Contents/MacOS`
/// holds the release binary, `Contents/Info.plist` comes from the `macos/`
/// directory the scaffolder writes, and nothing is signed — for the same
/// stated reason the studio's own packaging does not sign: a signing identity
/// is a certificate that cannot live in a repository or a generated file, and a
/// plan that pretended to sign would be worse than one that says it does not.
/// Gatekeeper's right-click → Open is the honest path for an unsigned bundle.
fn mac_app_bundle(env: &Env, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    if env.host != Host::MacOs {
        return Err(Refusal::Impossible(
            "a .app holds a macOS binary, and only Apple's toolchain produces one — this is \
             Apple's rule rather than a gap somebody could install",
        ));
    }
    let plist = root.join("macos").join("Info.plist");
    if !plist.is_file() {
        return Err(Refusal::Missing {
            requirement: "macos/Info.plist",
            install: "copy `macos/` across from a freshly scaffolded project — this one \
                      predates the macOS export",
        });
    }
    let app = out.join(format!("{name}.app"));
    let built = root.join("target").join("release").join(name);
    let copies: [(&Path, &Path); 2] = [
        (&built, &app.join("Contents").join("MacOS").join(name)),
        (
            &root.join("macos").join("Info.plist"),
            &app.join("Contents").join("Info.plist"),
        ),
    ];
    let mut steps = vec![Step {
        spec: Spec::new("Build (release)", "cargo", root).args(["build", "--release"]),
        why: "an optimised build, which is what ships".into(),
    }];
    steps.extend(layout_steps(root, &copies).into_iter().map(|spec| Step {
        spec,
        why: "the bundle layout Finder and LaunchServices expect".into(),
    }));
    Ok(Plan {
        title: Format::MacAppBundle.title().to_string(),
        steps,
        artefact: app,
    })
}

/// A `.deb` of the *user's* application (ST-07).
///
/// `dpkg-deb --build` over a staging tree the plan assembles: `DEBIAN/control`
/// and the `.desktop` entry from the `linux/` directory the scaffolder writes,
/// the binary under `/usr/local/bin`. `--root-owner-group` because a build as
/// the current user would otherwise bake that user's uid and gid into every
/// file — a package that installs everything owned by `1000:1000` and breaks
/// for the next person.
///
/// # Why the control file's architecture is checked, not assumed
///
/// `dpkg -i` refuses a package whose `Architecture:` names another machine —
/// and it refuses `any`, the value a *source* package carries, with exactly
/// that error. The control file is the project's own source, so the plan reads
/// it and refuses before building when it does not name this machine: an
/// export that produced a `.deb` nobody could install would be an export that
/// worked and shipped nothing, which is the failure this module exists to
/// prevent.
fn linux_deb(env: &Env, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    if env.host != Host::Linux {
        return Err(Refusal::Impossible(
            "a .deb holds a Linux binary, and this machine is not the one to put it in — \
             build it from the Linux machine that will run it",
        ));
    }
    if env.find_on_path("dpkg-deb").is_none() {
        return Err(Refusal::Missing {
            requirement: "dpkg-deb",
            install: "`apt install dpkg` (Debian and Ubuntu) — other distributions package \
                      with rpm instead, and this format is theirs",
        });
    }
    let control = root.join("linux").join("control");
    if !control.is_file() {
        return Err(Refusal::Missing {
            requirement: "linux/control",
            install: "copy `linux/` across from a freshly scaffolded project — this one \
                      predates the Linux export",
        });
    }
    let here = crate::harness::debian_arch();
    let named = std::fs::read_to_string(&control)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("Architecture:")
                    .map(|value| value.trim().to_string())
            })
        })
        .unwrap_or_default();
    if named.is_empty() {
        return Err(Refusal::Missing {
            requirement: "an `Architecture:` line in linux/control",
            install: "`dpkg-deb` refuses a package with no architecture — set it to what \
                      `dpkg --print-architecture` prints",
        });
    }
    if named != here {
        return Err(Refusal::Impossible(
            "linux/control's `Architecture:` names a machine this is not, and `dpkg -i` \
             would refuse the package — set it to what `dpkg --print-architecture` prints",
        ));
    }
    let lib = crate::harness::lib_name(name);
    let stage = out.join("deb");
    let tree = stage.join(&lib);
    let built = root.join("target").join("release").join(name);
    let copies: [(&Path, &Path); 3] = [
        (
            &root.join("linux").join("control"),
            &tree.join("DEBIAN").join("control"),
        ),
        (
            &built,
            &tree.join("usr").join("local").join("bin").join(name),
        ),
        (
            &root.join("linux").join(format!("{name}.desktop")),
            &tree
                .join("usr")
                .join("share")
                .join("applications")
                .join(format!("{name}.desktop")),
        ),
    ];
    let artefact = out.join(format!("{name}.deb"));
    let mut steps = vec![Step {
        spec: Spec::new("Build (release)", "cargo", root).args(["build", "--release"]),
        why: "an optimised build, which is what ships".into(),
    }];
    steps.extend(layout_steps(root, &copies).into_iter().map(|spec| Step {
        spec,
        why: "the staging tree dpkg-deb turns into the package".into(),
    }));
    steps.push(Step {
        spec: Spec::new("Package .deb", "dpkg-deb", root)
            .arg("--build")
            .arg("--root-owner-group")
            .arg(tree.to_string_lossy().to_string())
            .arg(artefact.to_string_lossy().to_string()),
        why: "assembles the archive apt installs from".into(),
    });
    Ok(Plan {
        title: Format::LinuxDeb.title().to_string(),
        steps,
        artefact,
    })
}

/// The wasm32 bundle and its page (ST-07).
///
/// # What the steps are, and why the page is copied rather than written
///
/// `cargo build --lib --target wasm32-unknown-unknown` — `--lib`, because the
/// `[[bin]]` target cannot exist on wasm (its `main` has no process to own)
/// and the library is where the `cdylib` the browser loads comes from. Then
/// `wasm-bindgen --target web` turns cargo's `.wasm` into the `.js` +
/// `_bg.wasm` pair a page imports, straight into the export directory. Then
/// the page itself — `web/index.html`, which the *scaffolder* generates, for
/// the same reason it generates `AndroidManifest.xml` and `Info.plist`: a
/// page is a source file of the project (its canvas, its fallback text, its
/// own title), not something an export should be inventing per run, and
/// owning it means the user can edit it and the export still works.
///
/// # Why the wasm is named the *library's* name
///
/// A `cdylib` on wasm32 lands at `target/wasm32-unknown-unknown/release/`
/// under the library's name with no `lib` prefix — `my-app` is `my_app.wasm`
/// — which is the same rename the Android manifest's `android.app.lib_name`
/// performs for the same reason: the loader asks for the library by name.
fn web(_env: &Env, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    // `env` is not read here, and that is the point: everything the web
    // toolchain needs was already asked about through `toolchains::report`
    // before this ran — the wasm target, `wasm-bindgen` — because those are
    // checklist questions. The files below are project questions.
    let page = root.join("web").join("index.html");
    if !page.is_file() {
        return Err(Refusal::Missing {
            requirement: "web/index.html",
            install: "copy `web/` across from a freshly scaffolded project — this one \
                      predates the web export",
        });
    }
    let lib = crate::harness::lib_name(name);
    let triple = "wasm32-unknown-unknown";
    let bundle = out.join("web");
    let wasm = root
        .join("target")
        .join(triple)
        .join("release")
        .join(format!("{lib}.wasm"));

    let mut steps = vec![
        Step {
            spec: Spec::new("Build (Web)", "cargo", root).args([
                "build",
                "--release",
                "--lib",
                "--target",
                triple,
            ]),
            why: "the library as a wasm module; the binary target has no browser to run in".into(),
        },
        Step {
            spec: Spec::new("JS glue", "wasm-bindgen", root)
                .arg("--target")
                .arg("web")
                .arg("--no-typescript")
                .arg("--out-dir")
                .arg(bundle.to_string_lossy().to_string())
                .arg(wasm.to_string_lossy().to_string()),
            why: "the .js loader and the .wasm it fetches, in the export directory".into(),
        },
    ];
    let copies: [(&Path, &Path); 1] = [(&page, &bundle.join("index.html"))];
    steps.extend(layout_steps(root, &copies).into_iter().map(|spec| Step {
        spec,
        why: "the page the project owns, beside the bundle it mounts".into(),
    }));
    Ok(Plan {
        title: Format::Web.title().to_string(),
        steps,
        artefact: bundle.join("index.html"),
    })
}

/// `./gradlew` when the project has a wrapper, and `gradle` when it does not.
///
/// # Why this is a choice rather than a constant
///
/// The wrapper is the right thing to run — it pins the Gradle version, so a
/// build does not change under you when Homebrew updates. But a wrapper is a
/// shell script *and a binary JAR*, and [`scaffold`](crate::scaffold) writes
/// source files; emitting a JAR from a scaffolder is not something to start
/// doing for one build step.
///
/// So a scaffolded project ships `gradle-wrapper.properties` and no JAR, this
/// runs the system `gradle` until somebody runs `gradle wrapper` once, and from
/// then on it runs the wrapper — including for every project that arrived with
/// one already, which is most projects that are not new.
///
/// Checked on disk rather than assumed, because the answer changes during the
/// life of a project and an export that hard-coded either one would be wrong
/// for half of it.
fn gradle_command(root: &Path) -> &'static str {
    let wrapper = if cfg!(windows) {
        root.join("android").join("gradlew.bat")
    } else {
        root.join("android").join("gradlew")
    };
    if wrapper.is_file() {
        if cfg!(windows) {
            "gradlew.bat"
        } else {
            "./gradlew"
        }
    } else {
        "gradle"
    }
}

fn android(host: Host, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    let artefact = out.join(format!("{name}.apk"));
    let gradle_output = root
        .join("android")
        .join("app")
        .join("build")
        .join("outputs")
        .join("apk")
        .join("debug")
        .join("app-debug.apk");
    let mut steps = vec![
        Step {
            // `cargo ndk` rather than `cargo build --target`: the NDK's
            // linker has to be on the path with the right sysroot for the
            // API level, and `cargo-ndk` exists to do exactly that. Doing
            // it by hand means reproducing its environment setup here and
            // getting it wrong on the next NDK release.
            spec: Spec::new("Build (Android)", "cargo", root).args([
                "ndk",
                "-t",
                "arm64-v8a",
                "-o",
                "target/android/jniLibs",
                "build",
                "--release",
            ]),
            why: "cross-compiles the Rust for arm64 with the NDK's linker".into(),
        },
        Step {
            spec: Spec::new("Package APK", gradle_command(root), root.join("android"))
                .args(["assembleDebug"]),
            why: "packages and debug-signs the apk".into(),
        },
    ];
    steps.extend(
        copy_steps(host, root, &gradle_output, &artefact)
            .into_iter()
            .map(|spec| Step {
                spec,
                why: "copied out of the gradle output tree under its own name".into(),
            }),
    );
    Ok(Plan {
        title: Format::AndroidApk.title().to_string(),
        steps,
        artefact,
    })
}

/// The environment variable that names the release keystore, and the one that
/// names the key inside it. The password is deliberately absent from this list.
///
/// # Why these are the planner's business and the password is not
///
/// A keystore's *path* and the *alias* of the key in it are facts about the
/// machine, like `ANDROID_HOME` — they already live in the settings file, they
/// are what a checklist can check (does the file exist?), and planning around
/// them is what makes the row's refusal name the thing to fix. The *password*
/// is none of those: it is a secret, it is deliberately in no file this studio
/// writes (see `settings::Settings::keystore_alias`), and so it never becomes a
/// requirement. [`plan`] is pure data about a project; a password is not.
///
/// The caller injects `VIEWW_KEYSTORE_PASSWORD` into the packaging step's
/// environment at run time, from a one-shot prompt or from the studio's own
/// process environment — never from here.
pub const KEYSTORE_VAR: &str = "VIEWW_KEYSTORE";
pub const KEYSTORE_ALIAS_VAR: &str = "VIEWW_KEYSTORE_ALIAS";
pub const KEYSTORE_PASSWORD_VAR: &str = "VIEWW_KEYSTORE_PASSWORD";

/// A release-signed `.apk` (ST-07).
///
/// [`android`] with `assembleRelease` and a signing configuration the generated
/// `build.gradle` assembles from [`KEYSTORE_VAR`] and [`KEYSTORE_ALIAS_VAR`] —
/// Gradle reads them out of the child's environment, which is the only place a
/// signing key's location belongs. The password reaches the same environment by
/// another route ([`KEYSTORE_PASSWORD_VAR`]), added by the caller between this
/// plan being printed and the packaging step starting, because a plan is data
/// the Output panel prints and a password is not.
fn android_release(env: &Env, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    // The keystore and its alias are requirements like any other: named, with
    // the fix beside them, checked *before* a build that would otherwise fail
    // inside Gradle with a message about a store file. The fix is a settings
    // screen rather than a shell command because the studio is where the value
    // has to be typed — see the Toolchains view.
    let keystore = env
        .vars
        .get(KEYSTORE_VAR)
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| path.is_file());
    if keystore.is_none() {
        return Err(Refusal::Missing {
            requirement: "release keystore",
            install: "set the keystore path in Settings \u{2192} Toolchains (the file has to \
                      exist; create one with `keytool -genkeypair`)",
        });
    }
    let alias = env
        .vars
        .get(KEYSTORE_ALIAS_VAR)
        .map(|value| value.trim())
        .filter(|alias| !alias.is_empty());
    if alias.is_none() {
        return Err(Refusal::Missing {
            requirement: "key alias",
            install: "set the key alias in Settings \u{2192} Toolchains — `keytool -list` \
                      names the keys in the keystore",
        });
    }

    let artefact = out.join(format!("{name}-release.apk"));
    let gradle_output = root
        .join("android")
        .join("app")
        .join("build")
        .join("outputs")
        .join("apk")
        .join("release")
        .join("app-release.apk");
    let mut steps = vec![
        Step {
            spec: Spec::new("Build (Android)", "cargo", root).args([
                "ndk",
                "-t",
                "arm64-v8a",
                "-o",
                "target/android/jniLibs",
                "build",
                "--release",
            ]),
            why: "cross-compiles the Rust for arm64 with the NDK's linker".into(),
        },
        Step {
            // The signing configuration in the generated `build.gradle` is
            // assembled from exactly these two variables — the keystore's path
            // and the alias of the key in it. The password is added to this
            // step's environment by the caller, from a prompt or the process
            // environment, and is never in the plan.
            spec: Spec::new(
                "Package release APK",
                gradle_command(root),
                root.join("android"),
            )
            .args(["assembleRelease"])
            .env(KEYSTORE_VAR, env.vars[KEYSTORE_VAR].trim())
            .env(KEYSTORE_ALIAS_VAR, env.vars[KEYSTORE_ALIAS_VAR].trim()),
            why: "packages the apk and signs it with the keystore".into(),
        },
    ];
    steps.extend(
        copy_steps(env.host, root, &gradle_output, &artefact)
            .into_iter()
            .map(|spec| Step {
                spec,
                why: "copied out of the gradle output tree under its own name".into(),
            }),
    );
    Ok(Plan {
        title: Format::AndroidReleaseApk.title().to_string(),
        steps,
        artefact,
    })
}

fn ios_app(env: &Env, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    if env.host != Host::MacOs {
        return Err(Refusal::Impossible(
            "Apple's toolchain runs only on macOS, so no iOS build is possible on this machine",
        ));
    }
    let artefact = out.join(format!("{name}.app"));
    Ok(Plan {
        title: Format::IosSimulatorApp.title().to_string(),
        steps: vec![
            Step {
                spec: Spec::new("Build (iOS simulator)", "cargo", root).args([
                    "build",
                    "--release",
                    "--target",
                    "aarch64-apple-ios-sim",
                ]),
                why: "the simulator runs arm64 on an Apple-silicon Mac".into(),
            },
            Step {
                // `CONFIGURATION_BUILD_DIR`: without it the `.app` lands in
                // Xcode's DerivedData, nothing copied it out, and the plan's
                // artefact path never existed — an export that "succeeded"
                // and produced nothing.
                spec: Spec::new("Package .app", "xcodebuild", root.join("ios"))
                    .args([
                        "-scheme",
                        name,
                        "-sdk",
                        "iphonesimulator",
                        "-configuration",
                        "Release",
                        "build",
                    ])
                    .arg(format!("CONFIGURATION_BUILD_DIR={}", out.to_string_lossy())),
                why: "assembles the bundle Xcode's project describes".into(),
            },
        ],
        artefact,
    })
}

fn ios_ipa(env: &Env, root: &Path, name: &str, out: &Path) -> Result<Plan, Refusal> {
    if env.host != Host::MacOs {
        return Err(Refusal::Impossible(
            "a signed .ipa can only be produced on macOS — an Apple rule, not a limitation to \
             work around",
        ));
    }
    let artefact = out.join(format!("{name}.ipa"));
    Ok(Plan {
        title: Format::IosIpa.title().to_string(),
        steps: vec![
            Step {
                spec: Spec::new("Build (iOS device)", "cargo", root).args([
                    "build",
                    "--release",
                    "--target",
                    "aarch64-apple-ios",
                ]),
                why: "every device since the iPhone 5s is arm64".into(),
            },
            Step {
                spec: Spec::new("Archive", "xcodebuild", root.join("ios")).args([
                    "-scheme",
                    name,
                    "-sdk",
                    "iphoneos",
                    "-configuration",
                    "Release",
                    "archive",
                    "-archivePath",
                    "build/app.xcarchive",
                ]),
                why: "signs with whichever identity the project is configured for".into(),
            },
            Step {
                spec: Spec::new("Export .ipa", "xcodebuild", root.join("ios"))
                    .args(["-exportArchive", "-archivePath", "build/app.xcarchive"])
                    .arg("-exportPath")
                    .arg(out.to_string_lossy().to_string())
                    .args(["-exportOptionsPlist", "ExportOptions.plist"]),
                why: "turns the signed archive into an installable .ipa".into(),
            },
        ],
        artefact,
    })
}

/// A copy step, expressed as a command so it joins the same queue.
///
/// # Why this branches on the host
///
/// It used to be one `sh -c "mkdir -p … && cp -R …"` for every host, on the
/// reasoning that `cp` is what "the two shells this runs under both have".
/// **There is no `sh` on a stock Windows machine** — no `cp` and no `mkdir -p`
/// either — so a Windows desktop export failed before it copied anything, with
/// an error naming a shell the user had never heard of and had no reason to
/// install. A developer who installed the studio and nothing else hit it on
/// their first export.
///
/// So the shell is gone on both sides rather than swapped for a different one:
///
/// * **Unix** runs `cp -R` directly. No `sh`, so no quoting question at all —
///   the arguments are passed as arguments, which is stronger than any shell
///   quoting could be.
/// * **Windows** runs `cmd /C` with `xcopy`. `xcopy` is chosen over `copy`
///   because it creates the destination directory itself (answering `/I`) and
///   handles a directory source, which is what the `-R` on the Unix side is
///   for. The `mkdir` step disappears with it.
///
/// The directory is still created explicitly on Unix, as a separate step, so
/// that a failure to create it is reported as its own line in the Output panel
/// rather than as a confusing "cp: no such file or directory".
fn copy_steps(host: Host, root: &Path, from: &Path, to: &Path) -> Vec<Spec> {
    let dest_dir = to
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    if host == Host::Windows {
        // `xcopy src dest* /Y /I /E /Q`. The trailing `*` on a file
        // destination is how `xcopy` is told the target is a file and not a
        // directory it should prompt about — the prompt is what would hang a
        // headless queue forever.
        let destination = format!("{}*", to.to_string_lossy());
        vec![Spec::new("Copy artefact", "cmd", root)
            .arg("/C")
            .arg("xcopy")
            .arg(from.to_string_lossy().to_string())
            .arg(destination)
            .args(["/Y", "/I", "/E", "/Q"])]
    } else {
        vec![
            Spec::new("Create export directory", "mkdir", root)
                .arg("-p")
                .arg(dest_dir.to_string_lossy().to_string()),
            Spec::new("Copy artefact", "cp", root)
                .arg("-R")
                .arg(from.to_string_lossy().to_string())
                .arg(to.to_string_lossy().to_string()),
        ]
    }
}

/// The `name` under `[package]` in a `Cargo.toml`.
///
/// # Why this is a scan and not a TOML parse
///
/// The studio has no TOML dependency and the question is small: the first
/// `name = "..."` inside the `[package]` table. Sections are tracked so a
/// `name` under `[[bin]]` or `[dependencies.serde]` is not mistaken for the
/// package's — which is the whole reason a plain search for `name =` is wrong.
///
/// A manifest this cannot read returns `None`, and the caller refuses with
/// [`Refusal::NoProject`] rather than guessing from the directory's name: a
/// folder and its package differ often enough that guessing is a coin flip,
/// and the artefact's name is what somebody double-clicks.
#[must_use]
pub fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(value) = line.strip_prefix("name") else {
            continue;
        };
        let value = value.trim_start();
        let Some(value) = value.strip_prefix('=') else {
            continue;
        };
        // Comments and quotes, in that order: `name = "app" # the binary`.
        let value = value.split('#').next().unwrap_or_default().trim();
        let name = value.trim_matches(|c| c == '"' || c == '\'');
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Connected devices
// ---------------------------------------------------------------------------

/// A device an export could be installed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// What `adb -s` takes, or what `simctl` calls a UDID.
    pub id: String,
    /// A name a person recognises. Falls back to the id.
    pub name: String,
    pub kind: Target,
    pub state: DeviceState,
}

/// Whether a device can be installed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Ready,
    /// Plugged in, but the phone has not accepted this computer's key. The
    /// commonest state for a phone somebody has just connected, and the one
    /// worth naming: "offline" would send them looking at the cable.
    Unauthorised,
    /// Listed but not usable — booting, offline, or in recovery.
    Unavailable(String),
}

impl DeviceState {
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Ready => "ready",
            Self::Unauthorised => "unauthorised — accept the prompt on the device",
            Self::Unavailable(why) => why,
        }
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// The command that lists Android devices.
#[must_use]
pub fn adb_devices(root: &Path) -> Spec {
    Spec::new("Scan devices", "adb", root)
        .args(["devices", "-l"])
        .timeout(std::time::Duration::from_secs(15))
}

/// Parse `adb devices -l`.
///
/// # The format, and why it is parsed rather than asked for as JSON
///
/// `adb` has no machine-readable mode. The output is a header line, then one
/// line per device: an id, a state, and space-separated `key:value` pairs of
/// which `model:` and `device:` are the ones worth showing. Anything that does
/// not match that shape is skipped rather than guessed at — a warning adb
/// prints on startup ("daemon not running") would otherwise become a device
/// named `*`.
#[must_use]
pub fn parse_adb_devices(output: &str) -> Vec<Device> {
    let mut devices = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("List of devices")
            || line.starts_with('*')
            || line.starts_with("adb server")
            || line.starts_with("daemon")
        {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(id), Some(state)) = (parts.next(), parts.next()) else {
            continue;
        };
        let mut model = None;
        for pair in parts {
            if let Some(value) = pair.strip_prefix("model:") {
                model = Some(value.replace('_', " "));
            }
        }
        devices.push(Device {
            name: model.unwrap_or_else(|| id.to_string()),
            id: id.to_string(),
            kind: Target::Android,
            state: match state {
                "device" => DeviceState::Ready,
                "unauthorized" | "unauthorised" => DeviceState::Unauthorised,
                other => DeviceState::Unavailable(other.to_string()),
            },
        });
    }
    devices
}

/// Install an artefact on a device.
///
/// Only Android today, and the refusal for anything else says so rather than
/// running something that would half-work: installing to an iOS device is
/// `xcrun devicectl`, which needs a signed `.ipa` and a paired device, and
/// pretending otherwise would fail three steps later with an Xcode error.
pub fn install(root: &Path, device: &Device, artefact: &Path) -> Result<Spec, Refusal> {
    match device.kind {
        Target::Android => Ok(Spec::new("Install", "adb", root)
            .args(["-s", &device.id, "install", "-r"])
            .arg(artefact.to_string_lossy().to_string())
            .timeout(std::time::Duration::from_secs(300))),
        Target::Ios => Err(Refusal::NotImplemented(
            "installing to an iOS device needs `xcrun devicectl` and a paired device; the \
             simulator is reachable with `xcrun simctl install booted`",
        )),
        Target::Desktop | Target::Windows => Err(Refusal::NotImplemented(
            "a desktop binary is run, not installed — it is in target/export",
        )),
        Target::Web => Err(Refusal::NotImplemented(
            "a web bundle is served, not installed — the directory it is in is the app",
        )),
    }
}

// ---------------------------------------------------------------------------
// Mirroring the preview on a connected device (ST-08)
// ---------------------------------------------------------------------------

/// Where a mirror's screenshot lives on the device, and where it is pulled to.
///
/// A fixed remote name rather than a generated one: the phone cleans up after
/// itself (the last step deletes it), and a unique name per mirror would be a
/// unique file left behind whenever one of those steps failed.
pub const MIRROR_REMOTE_PNG: &str = "/sdcard/vieww-mirror.png";

/// The component `am start` is given, `<applicationId>/<activity>`.
///
/// `android.app.NativeActivity` because that is what the generated
/// `AndroidManifest.xml` declares — the manifest a scaffold writes and this
/// launches are the same file's two halves, and this function is where they
/// meet. A manifest that changes its activity changes this, and
/// `tests/scaffold_exports.rs` reading both keeps them honest.
#[must_use]
pub fn launch_component(package: &str) -> String {
    format!("{package}/android.app.NativeActivity")
}

/// The applicationId the project's Gradle file declares — the package the
/// phone knows the app by, which is not the cargo package's name.
///
/// Read from `android/app/build.gradle` because that is the file that decides
/// it on the device; `com.example.<lib>` (the scaffolder's value, from
/// [`harness::bundle_id`](crate::harness::bundle_id)) is the fallback for a
/// project whose Gradle file is missing or edited past recognition. The
/// android/README tells the user to change the applicationId, and a launcher
/// that guessed the old one would report the app as missing when it is
/// installed and fine.
#[must_use]
pub fn launch_package(root: &Path, name: &str) -> String {
    for candidate in [
        root.join("android").join("app").join("build.gradle"),
        root.join("android").join("app").join("build.gradle.kts"),
    ] {
        let Ok(text) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("applicationId") else {
                continue;
            };
            if let Some(id) = rest
                .trim()
                .strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
            {
                if !id.is_empty() {
                    return id.to_string();
                }
            }
        }
    }
    crate::harness::bundle_id(&crate::harness::lib_name(name))
}

/// Where a mirror's screenshot is pulled to, under the workspace's private
/// `.vieww` directory (the one `collab.log` already lives in — studio-owned,
/// git-ignored by convention, and never the user's source tree).
#[must_use]
pub fn mirror_png(root: &Path, device: &Device) -> PathBuf {
    let safe: String = device
        .id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    root.join(".vieww").join(format!("mirror-{safe}.png"))
}

/// The steps that put the app on a device and bring its screen back (ST-08).
///
/// # What this is, and what it is honest about
///
/// Install the exported debug APK, launch it, screenshot the phone, pull the
/// PNG where the studio can decode it, delete the copy on the phone. Five
/// `adb` invocations, no shell — which is why `screencap` writes to a file on
/// the device and `pull` brings it over rather than `exec-out` piping bytes:
/// the jobs machinery captures *text*, and a PNG through a text pipe is a PNG
/// with its newlines rewritten.
///
/// # Why a screenshot and not a comparison
///
/// The research report's longer arc is preview↔device pixel comparison. That
/// needs the device's density and inset facts to be known before the studio's
/// own render is comparable, and a first step that claimed it while guessing
/// either would be a red number about nothing. This lands the honest first
/// half: the phone's actual pixels, in the preview pane, beside the preview —
/// the comparison is the eye's, which is where "it looked right in the
/// preview" goes to die.
pub fn mirror_steps(
    env: &Env,
    root: &Path,
    device: &Device,
    artefact: &Path,
    package: &str,
) -> Result<Vec<Spec>, Refusal> {
    if device.kind != Target::Android {
        return Err(Refusal::NotImplemented(
            "mirroring works through `adb`, which speaks Android — an iOS device is reached \
             with `xcrun devicectl` and a Mac",
        ));
    }
    if !device.state.is_ready() {
        return Err(Refusal::Impossible(
            "the device is not ready — a mirror needs the device's own consent, not just its \
             cable, and the state is printed on its row",
        ));
    }
    if env.find_on_path("adb").is_none() {
        return Err(Refusal::Missing {
            requirement: "adb",
            install: "`brew install android-platform-tools` (macOS) or install \
                       `google-platform-tools` (Linux) — it is the `adb` half of the SDK, on its own",
        });
    }
    if !artefact.is_file() {
        return Err(Refusal::Missing {
            requirement: "an exported APK to mirror",
            install: "export the Android APK (debug-signed) first — the mirror installs what \
                      the export built",
        });
    }
    let local = mirror_png(root, device);
    let launch = launch_component(package);
    Ok(vec![
        install(root, device, artefact).expect("Android, checked above"),
        Spec::new("Launch on device", "adb", root)
            .args(["-s", &device.id, "shell", "am", "start", "-W", "-n"])
            .arg(launch)
            .timeout(std::time::Duration::from_secs(60)),
        Spec::new("Screenshot on device", "adb", root)
            .args(["-s", &device.id, "shell", "screencap", "-p"])
            .arg(MIRROR_REMOTE_PNG)
            .timeout(std::time::Duration::from_secs(30)),
        Spec::new("Pull screenshot", "adb", root)
            .args(["-s", &device.id, "pull"])
            .arg(MIRROR_REMOTE_PNG.to_string())
            .arg(local.to_string_lossy().to_string())
            .timeout(std::time::Duration::from_secs(60)),
        Spec::new("Clean up on device", "adb", root)
            .args(["-s", &device.id, "shell", "rm"])
            .arg(MIRROR_REMOTE_PNG.to_string())
            .timeout(std::time::Duration::from_secs(30)),
    ])
}

/// Whether `am start -W` says the activity came up.
///
/// `am start -W` ends with a `Status:` line — `ok` for a launch, an error
/// string for everything else (`Error: Activity class … does not exist`, the
/// one a renamed package produces). One line of adb's output is the whole
/// verdict, so one line is all this reads.
#[must_use]
pub fn parse_am_start(output: &str) -> bool {
    output
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("Status:"))
        .is_some_and(|status| status.trim() == "ok")
}

/// Whether a mirror step's captured stdout contradicts its exit code (ST-08).
///
/// Only the launch step has such output, and it has it in two shapes: a
/// `Status:` line that is not `ok`, and the classic `Error: Activity class …
/// does not exist` printed with no `Status:` line at all — the failure that
/// some shells and adb wrappers hand back with exit code 0, which is the
/// case this function exists for. The other four steps print nothing
/// am-shaped (`Success`, `1 file pulled`, or silence), so for them this is
/// always `false` and the exit code stays the only verdict.
#[must_use]
pub fn mirror_step_output_failed(output: &str) -> bool {
    if output
        .lines()
        .any(|line| line.trim().starts_with("Status:"))
    {
        // am's own protocol is present, so it is authoritative — including
        // over an `Error:` line printed above a later `Status: ok`.
        !parse_am_start(output)
    } else {
        // am's failure without its protocol, still in its own words.
        output.lines().any(|line| line.trim().starts_with("Error:"))
    }
}

/// Dispatch the repo's device farm workflow on `count` phones (ST-08).
///
/// # What the farm runs, honestly
///
/// The workflow this starts (`.github/workflows/device-farm.yml`) builds and
/// runs the **framework's** conformance app on real phones — low, mid and
/// high end — not the user's project. That is not a smaller version of the
/// thing the button's neighbourhood implies; it is the thing that makes the
/// studio's own claim checkable: the renderer the preview certifies itself
/// against is the one on the phone, and this is where the phone says so.
/// The user's *app* on real hardware is the local mirror; the farm is the
/// *framework* on real hardware. The notice the studio prints after
/// dispatching says exactly this.
///
/// # Why `--ref main` is spelled out
///
/// The farm's AWS trust policy accepts main's OIDC subject only, so a dispatch
/// from any other ref is refused by the workflow's own first job — five
/// minutes of build for a refusal the studio can name at the start.
pub fn farm_dispatch(env: &Env, root: &Path, platform: &str, count: u32) -> Result<Spec, Refusal> {
    if !matches!(platform, "android" | "ios" | "both") {
        return Err(Refusal::Impossible(
            "the farm workflow knows android, ios and both — nothing else",
        ));
    }
    if env.find_on_path("gh").is_none() {
        return Err(Refusal::Missing {
            requirement: "GitHub CLI (`gh`)",
            install: "`brew install gh` (macOS) or follow github.com/cli/cli — then `gh auth \
                      login`, because a dispatch is an API call as you",
        });
    }
    Ok(Spec::new("Dispatch device farm", "gh", root)
        .args(["workflow", "run", "device-farm.yml", "--ref", "main"])
        .arg("-f")
        .arg(format!("platform={platform}"))
        .arg("-f")
        .arg(format!("count={count}"))
        .arg("-f")
        .arg("profile=dist")
        .timeout(std::time::Duration::from_secs(60)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A fake environment with real files behind it.
    ///
    /// `toolchains` deliberately looks at the filesystem — a tool is a file on
    /// `PATH` with the executable bit, an SDK is a directory that exists — so
    /// a test that wants "everything installed" has to make it so rather than
    /// assert it. Everything lands under one temp directory that is removed
    /// with the test.
    fn complete(host: Host, tools: &[&str]) -> (Env, tempdir::Dir) {
        let dir = tempdir::Dir::new("viewwstudio-export");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("create bin");
        for tool in tools {
            let names: Vec<String> = if host == Host::Windows {
                vec![format!("{tool}.exe")]
            } else {
                vec![(*tool).to_string()]
            };
            for name in names {
                let path = bin.join(name);
                std::fs::write(&path, "").expect("write stub");
                executable(&path);
            }
        }

        let mut vars = BTreeMap::new();
        for (key, folder) in [
            ("ANDROID_HOME", "sdk"),
            ("ANDROID_NDK_HOME", "ndk"),
            ("JAVA_HOME", "jdk"),
        ] {
            let path = dir.path().join(folder);
            std::fs::create_dir_all(&path).expect("create sdk dir");
            vars.insert(key.to_string(), path.to_string_lossy().into_owned());
        }

        let env = Env {
            host,
            vars,
            path: vec![bin],
            rust_targets: toolchains::Probed::Given(vec![
                "aarch64-linux-android".into(),
                "aarch64-apple-ios".into(),
                "aarch64-apple-ios-sim".into(),
                "wasm32-unknown-unknown".into(),
            ]),
            signing_identity: toolchains::Probed::Given(Some(
                "Apple Development: Test (TEAMID)".into(),
            )),
        };
        (env, dir)
    }

    /// Give `path` the executable bit, where the platform has one.
    fn executable(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
    }

    /// A minimal scoped temp directory, so these tests do not need a
    /// dependency — the crate is `std`-only on purpose.
    mod tempdir {
        use std::path::{Path, PathBuf};

        #[derive(Debug)]
        pub(super) struct Dir(PathBuf);

        impl Dir {
            pub(super) fn new(prefix: &str) -> Self {
                let path = std::env::temp_dir().join(format!(
                    "{prefix}-{}-{:?}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("after the epoch")
                        .as_nanos()
                ));
                std::fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            pub(super) fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_missing_tool_is_refused_before_a_process_is_started() {
        // The whole design. An empty PATH means no cargo, and the refusal
        // names it and how to get it rather than letting the shell say
        // "command not found".
        let (mut env, _dir) = complete(Host::Linux, &[]);
        // An empty PATH: nothing is installed.
        env.path.clear();
        let refusal = plan(&env, Path::new("/project"), "app", Format::DesktopBinary)
            .expect_err("no cargo on an empty PATH");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "cargo",
                install: "https://rustup.rs",
            }
        );
        assert!(refusal.to_string().contains("rustup.rs"));
    }

    #[test]
    fn ios_on_a_machine_that_is_not_a_mac_is_impossible_rather_than_missing() {
        // Not a dependency somebody could go and install, so it must not read
        // like one.
        let (env, _dir) = complete(Host::Linux, &["cargo"]);
        for format in [Format::IosSimulatorApp, Format::IosIpa] {
            let refusal = plan(&env, Path::new("/project"), "app", format).expect_err("not a Mac");
            assert!(
                matches!(refusal, Refusal::Impossible(_)),
                "{format:?} gave {refusal:?}"
            );
            assert!(refusal.to_string().contains("macOS"));
        }
    }

    #[test]
    fn a_desktop_export_builds_release_and_copies_the_binary_out() {
        let (env, _dir) = complete(Host::Linux, &["cargo"]);
        let plan = plan(&env, Path::new("/project"), "app", Format::DesktopBinary)
            .expect("a complete desktop toolchain");
        // Build, create the directory, copy. The directory is its own step so
        // a failure to create it reads as itself rather than as a confusing
        // "cp: no such file or directory".
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].spec.args, vec!["build", "--release"]);
        assert_eq!(plan.artefact, PathBuf::from("/project/target/export/app"));
        // Copied out, because the next release build overwrites the original.
        assert!(plan.steps[2].spec.command_line().contains("cp -R"));
    }

    #[test]
    fn a_windows_desktop_export_names_the_executable_the_way_windows_does() {
        // The one place a desktop export is allowed to care about the host.
        let (env, _dir) = complete(Host::Windows, &["cargo"]);
        let plan = plan(&env, Path::new("/project"), "app", Format::DesktopBinary)
            .expect("a complete desktop toolchain");
        assert_eq!(
            plan.artefact,
            PathBuf::from("/project/target/export/app.exe")
        );
    }

    #[test]
    fn an_android_export_cross_compiles_then_packages_then_copies() {
        let (env, _dir) = complete(Host::Linux, &["cargo", "cargo-ndk", "gradle", "javac"]);
        let plan = plan(&env, Path::new("/project"), "app", Format::AndroidApk)
            .expect("a complete Android toolchain");
        let labels: Vec<&str> = plan.steps.iter().map(|s| s.spec.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Build (Android)",
                "Package APK",
                "Create export directory",
                "Copy artefact"
            ]
        );
        assert!(plan.steps[0].spec.args.contains(&"ndk".to_string()));
        assert_eq!(
            plan.artefact,
            PathBuf::from("/project/target/export/app.apk")
        );
    }

    // ----- ST-07: web, macOS, Linux, release signing ------------------------

    /// A scratch project with the platform files a scaffolded project has,
    /// so the plans below plan against something real rather than `/project`
    /// — which since ST-07 they read (the page, the plist, the control).
    ///
    /// Written from `harness::files` rather than by hand, so a template that
    /// stopped matching the plan fails here as well as in
    /// `tests/scaffold_exports.rs`.
    fn scratch_project(name: &str) -> (PathBuf, tempdir::Dir) {
        let dir = tempdir::Dir::new("viewwstudio-export-platform");
        let root = dir.path().join(name);
        for (relative, text) in crate::harness::files(name) {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
            std::fs::write(&path, text).expect("write platform file");
        }
        (root, dir)
    }

    #[test]
    fn a_web_export_builds_the_module_binds_it_and_copies_the_page() {
        let (env, _dir) = complete(Host::Linux, &["cargo", "wasm-bindgen"]);
        let (root, _project) = scratch_project("my-app");
        let plan = plan(&env, &root, "my-app", Format::Web).expect("a complete web toolchain");
        let labels: Vec<&str> = plan.steps.iter().map(|s| s.spec.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Build (Web)",
                "JS glue",
                "Create bundle directories",
                "Copy into the bundle"
            ]
        );
        // `--lib`: the `[[bin]]` target has no browser to run in, and asking
        // for it is a compile error rather than a smaller bundle.
        assert_eq!(
            plan.steps[0].spec.args,
            vec![
                "build",
                "--release",
                "--lib",
                "--target",
                "wasm32-unknown-unknown"
            ]
        );
        // The wasm cargo emits is named for the *library* — `my-app` is
        // `my_app.wasm` — and bindgen writes into the export directory so
        // nothing has to find it afterwards.
        let glue = plan.steps[1].spec.command_line();
        assert!(glue.contains("--target web"), "{glue}");
        assert!(glue.contains("my_app.wasm"), "{glue}");
        assert!(glue.contains("target/export/web"), "{glue}");
        // The page travels beside the bundle it mounts, from the directory the
        // scaffolder owns.
        let copy = plan.steps[3].spec.command_line();
        assert!(copy.contains("web/index.html"), "{copy}");
        assert_eq!(plan.artefact, root.join("target/export/web/index.html"));
    }

    #[test]
    fn a_web_export_without_the_target_names_the_command_that_adds_it() {
        // The refusal is the fix: `rustup target add wasm32-unknown-unknown`,
        // not a rustc error about a missing target, twenty minutes from now.
        let (mut env, _dir) = complete(Host::Linux, &["cargo", "wasm-bindgen"]);
        env.rust_targets = toolchains::Probed::Given(Vec::new());
        let refusal = plan(&env, Path::new("/project"), "app", Format::Web)
            .expect_err("the wasm target is not installed");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "wasm32-unknown-unknown",
                install: "`rustup target add wasm32-unknown-unknown`",
            }
        );
        assert!(
            refusal
                .to_string()
                .contains("rustup target add wasm32-unknown-unknown"),
            "{refusal}"
        );
    }

    #[test]
    fn a_web_export_without_wasm_bindgen_is_refused_before_building() {
        // Everything else can be present and the bundle still cannot exist:
        // the CLI is what turns cargo's wasm into the pair a page loads.
        let (env, _dir) = complete(Host::Linux, &["cargo"]);
        let refusal = plan(&env, Path::new("/project"), "app", Format::Web)
            .expect_err("no wasm-bindgen on PATH");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "wasm-bindgen",
                install: "`cargo install wasm-bindgen-cli --version <the one in Cargo.lock>`",
            }
        );
    }

    #[test]
    fn a_web_export_without_a_page_is_refused_rather_than_planned() {
        // A project scaffolded before the web export existed has no page, and
        // a plan whose copy step fails at run time is a refusal that waited.
        let (env, _dir) = complete(Host::Linux, &["cargo", "wasm-bindgen"]);
        let refusal =
            plan(&env, Path::new("/project"), "app", Format::Web).expect_err("no web/index.html");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "web/index.html",
                install: "copy `web/` across from a freshly scaffolded project — this one \
                      predates the web export",
            }
        );
    }

    #[test]
    fn a_mac_app_bundle_off_macos_is_apples_rule_not_a_missing_tool() {
        let (env, _dir) = complete(Host::Linux, &["cargo"]);
        let refusal =
            plan(&env, Path::new("/project"), "app", Format::MacAppBundle).expect_err("not a Mac");
        assert!(matches!(refusal, Refusal::Impossible(_)), "{refusal:?}");
        assert!(refusal.to_string().contains("macOS"), "{refusal}");
    }

    #[test]
    fn a_mac_app_bundle_lays_out_contents_from_the_scaffolded_plist() {
        let (env, _dir) = complete(Host::MacOs, &["cargo"]);
        let (root, _project) = scratch_project("app");
        let plan = plan(&env, &root, "app", Format::MacAppBundle).expect("a Mac with cargo");
        let app = root.join("target/export/app.app");
        assert_eq!(plan.artefact, app);
        // Build, one mkdir for the whole layout, then the binary and the plist.
        assert_eq!(plan.steps.len(), 4);
        let mkdir = plan.steps[1].spec.command_line();
        assert!(
            mkdir.contains("Contents/MacOS") && mkdir.contains("Contents"),
            "one mkdir covers every directory: {mkdir}"
        );
        assert!(plan.steps[2].spec.command_line().ends_with("MacOS/app"));
        assert!(plan.steps[3]
            .spec
            .command_line()
            .contains("macos/Info.plist"));
        assert!(plan.steps[3]
            .spec
            .command_line()
            .contains("Contents/Info.plist"));
        // Nothing signs: the studio's own packaging policy, stated in the plan
        // rather than contradicted by a step that pretends to.
        assert!(
            !plan.steps.iter().any(|s| s.spec.program == "codesign"),
            "an unsigned bundle is the honest bundle"
        );
    }

    #[test]
    fn a_linux_deb_off_linux_says_why_rather_than_guessing() {
        let (env, _dir) = complete(Host::MacOs, &["cargo", "dpkg-deb"]);
        let refusal =
            plan(&env, Path::new("/project"), "app", Format::LinuxDeb).expect_err("not Linux");
        assert!(matches!(refusal, Refusal::Impossible(_)), "{refusal:?}");
    }

    #[test]
    fn a_linux_deb_without_dpkg_deb_names_it_and_its_installer() {
        let (env, _dir) = complete(Host::Linux, &["cargo"]);
        let refusal = plan(&env, Path::new("/project"), "app", Format::LinuxDeb)
            .expect_err("no dpkg-deb on PATH");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "dpkg-deb",
                install: "`apt install dpkg` (Debian and Ubuntu) — other distributions package \
                          with rpm instead, and this format is theirs",
            }
        );
    }

    #[test]
    fn a_linux_deb_stages_the_tree_then_builds_it_with_root_owner_group() {
        let (env, _dir) = complete(Host::Linux, &["cargo", "dpkg-deb"]);
        let (root, _project) = scratch_project("my-app");
        let plan = plan(&env, &root, "my-app", Format::LinuxDeb).expect("a Linux with dpkg-deb");
        assert_eq!(plan.artefact, root.join("target/export/my-app.deb"));
        // control, binary, .desktop — every path the package needs is one the
        // scaffolder's `linux/` directory owns.
        let lines: Vec<String> = plan.steps.iter().map(|s| s.spec.command_line()).collect();
        let copy = lines
            .iter()
            .find(|l| l.contains("linux/control"))
            .expect("the control file is copied");
        assert!(copy.contains("DEBIAN/control"), "{copy}");
        assert!(
            lines.iter().any(|l| l.contains("usr/local/bin/my-app")),
            "the binary lands on the PATH the package installs to"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("share/applications/my-app.desktop")),
            "the .desktop entry lands where desktops look"
        );
        let build = lines.last().expect("dpkg-deb closes the plan");
        assert!(
            build.contains("--root-owner-group"),
            "no uid/gid baked in: {build}"
        );
        assert!(build.contains("target/export/deb"), "{build}");
    }

    #[test]
    fn a_linux_deb_refuses_a_control_file_from_another_architecture() {
        // `dpkg -i` refuses a package whose Architecture names another machine
        // — verified by building one and trying — so the plan refuses first,
        // naming the file to edit rather than producing a package nobody can
        // install.
        let (env, _dir) = complete(Host::Linux, &["cargo", "dpkg-deb"]);
        let (root, _project) = scratch_project("app");
        let other = if crate::harness::debian_arch() == "amd64" {
            "arm64"
        } else {
            "amd64"
        };
        let control = root.join("linux").join("control");
        std::fs::write(
            &control,
            format!("Package: app\nVersion: 0.1.0\nArchitecture: {other}\n"),
        )
        .expect("write the wrong arch");
        let refusal = plan(&env, &root, "app", Format::LinuxDeb)
            .expect_err("the control file names another machine");
        assert!(matches!(refusal, Refusal::Impossible(_)), "{refusal:?}");
        assert!(
            refusal.to_string().contains("dpkg --print-architecture"),
            "the fix is in the message: {refusal}"
        );
    }

    #[test]
    fn a_linux_deb_without_the_linux_directory_names_what_to_copy() {
        // A project scaffolded before ST-07 has no `linux/` directory at all.
        let (env, _dir) = complete(Host::Linux, &["cargo", "dpkg-deb"]);
        let refusal = plan(&env, Path::new("/project"), "app", Format::LinuxDeb)
            .expect_err("no linux/control");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "linux/control",
                install: "copy `linux/` across from a freshly scaffolded project — this one \
                      predates the Linux export",
            }
        );
    }

    #[test]
    fn a_release_apk_is_refused_without_a_keystore_before_gradle_hears_about_it() {
        let (env, _dir) = complete(Host::Linux, &["cargo", "cargo-ndk", "gradle", "javac"]);
        let refusal = plan(
            &env,
            Path::new("/project"),
            "app",
            Format::AndroidReleaseApk,
        )
        .expect_err("no keystore configured");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "release keystore",
                install: "set the keystore path in Settings \u{2192} Toolchains (the file has \
                          to exist; create one with `keytool -genkeypair`)",
            }
        );
        // The refusal reads as a settings problem, not a download problem.
        assert!(refusal.to_string().contains("Settings"), "{refusal}");
    }

    #[test]
    fn a_release_apk_carries_the_keystore_in_the_child_env_and_nowhere_else() {
        let (mut env, dir) = complete(Host::Linux, &["cargo", "cargo-ndk", "gradle", "javac"]);
        // A real file, because "found" has always meant "present at a plausible
        // path" — a keystore path that does not exist is a missing keystore.
        let keystore = dir.path().join("release.keystore");
        std::fs::write(&keystore, "a keystore").expect("write keystore");
        env.vars.insert(
            KEYSTORE_VAR.to_string(),
            keystore.to_string_lossy().into_owned(),
        );
        env.vars
            .insert(KEYSTORE_ALIAS_VAR.to_string(), "upload".into());
        // What a leaked password would look like, to prove the plan never
        // carries one even when the studio's own environment has it.
        env.vars
            .insert(KEYSTORE_PASSWORD_VAR.to_string(), "hunter2".into());

        let plan = plan(
            &env,
            Path::new("/project"),
            "app",
            Format::AndroidReleaseApk,
        )
        .expect("a keystore, an alias and the Android chain");
        let packaging = plan
            .steps
            .iter()
            .find(|s| s.spec.label == "Package release APK")
            .expect("the signing step");
        // The environment the child sees — the one Gradle's signingConfig reads.
        let env_keys: Vec<&str> = packaging.spec.env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(env_keys.contains(&KEYSTORE_VAR), "{env_keys:?}");
        assert!(env_keys.contains(&KEYSTORE_ALIAS_VAR), "{env_keys:?}");
        // The command line — the one the Output panel prints — says nothing
        // about the keystore at all, and no step anywhere carries a password.
        assert!(
            !packaging.spec.command_line().contains("keystore"),
            "{}",
            packaging.spec.command_line()
        );
        for step in &plan.steps {
            assert!(
                !step
                    .spec
                    .env
                    .iter()
                    .any(|(k, _)| k == KEYSTORE_PASSWORD_VAR),
                "the password is the caller's to inject, never the plan's: {:?}",
                step.spec.env
            );
        }
        assert_eq!(
            plan.artefact,
            PathBuf::from("/project/target/export/app-release.apk")
        );
    }

    #[test]
    fn an_export_lands_in_target_rather_than_beside_the_source() {
        // A fifty-megabyte APK next to `src/` is how a repository acquires a
        // binary nobody meant to commit.
        assert_eq!(
            export_dir(Path::new("/a/b")),
            PathBuf::from("/a/b/target/export")
        );
    }

    #[test]
    fn a_project_with_no_name_is_refused_as_no_project() {
        let (env, _dir) = complete(Host::Linux, &["cargo"]);
        assert_eq!(
            plan(&env, Path::new("/project"), "", Format::DesktopBinary),
            Err(Refusal::NoProject)
        );
    }

    #[test]
    fn a_path_with_a_space_or_a_quote_in_it_is_one_argument() {
        // The normal case on two of the three desktops, and the one that used
        // to turn a copy step into two broken arguments. There is no shell to
        // quote for any more: the path is passed as an argument, which is why
        // it survives untouched.
        let awkward = Path::new("/Users/a b/it's/app");
        let steps = copy_steps(Host::MacOs, Path::new("/p"), awkward, Path::new("/out/app"));
        let copy = steps.last().expect("a copy step");
        assert_eq!(copy.program, "cp");
        assert!(
            copy.args.contains(&awkward.to_string_lossy().to_string()),
            "the path is one argument, verbatim: {:?}",
            copy.args
        );
    }

    #[test]
    fn a_windows_export_does_not_reach_for_a_unix_shell() {
        // The blocker this replaced: `sh` is not on a stock Windows machine,
        // and neither are `cp` and `mkdir`. An export that needs them fails
        // before it copies anything, naming a shell the user never installed.
        let (env, _dir) = complete(Host::Windows, &["cargo"]);
        let plan = plan(&env, Path::new("/project"), "app", Format::DesktopBinary)
            .expect("a complete desktop toolchain");
        for step in &plan.steps {
            assert!(
                !matches!(step.spec.program.as_str(), "sh" | "cp" | "mkdir"),
                "a Windows export must not run {}: {:?}",
                step.spec.program,
                step.spec
            );
        }
        let copy = plan
            .steps
            .iter()
            .find(|s| s.spec.label == "Copy artefact")
            .expect("a copy step");
        assert_eq!(copy.spec.program, "cmd");
        assert!(
            copy.spec.args.iter().any(|a| a == "xcopy"),
            "{:?}",
            copy.spec.args
        );
    }

    #[test]
    fn a_unix_export_copies_without_a_shell_wrapper() {
        let (env, _dir) = complete(Host::Linux, &["cargo"]);
        let plan = plan(&env, Path::new("/project"), "app", Format::DesktopBinary)
            .expect("a complete desktop toolchain");
        let copy = plan
            .steps
            .iter()
            .find(|s| s.spec.label == "Copy artefact")
            .expect("a copy step");
        assert_eq!(copy.spec.program, "cp");
        assert!(
            plan.steps.iter().any(|s| s.spec.program == "mkdir"),
            "the export directory is created as its own step"
        );
    }

    #[test]
    fn adb_output_becomes_devices_and_its_noise_does_not() {
        let output = "\
* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
emulator-5554          device product:sdk_gphone64 model:Pixel_8 device:emu64a
FA7B2X01               unauthorized usb:1-3
R58M12ABCDE            offline

";
        let devices = parse_adb_devices(output);
        assert_eq!(devices.len(), 3, "the two daemon lines are not devices");

        assert_eq!(devices[0].id, "emulator-5554");
        assert_eq!(
            devices[0].name, "Pixel 8",
            "underscores are not in the name"
        );
        assert_eq!(devices[0].state, DeviceState::Ready);

        assert_eq!(devices[1].state, DeviceState::Unauthorised);
        assert!(
            devices[1].state.label().contains("accept the prompt"),
            "the commonest state says what to do about it"
        );
        // No `model:` on that line, so the id is the name rather than a blank.
        assert_eq!(devices[1].name, "FA7B2X01");

        assert_eq!(
            devices[2].state,
            DeviceState::Unavailable("offline".to_string())
        );
        assert!(!devices[2].state.is_ready());
    }

    #[test]
    fn empty_adb_output_is_no_devices_rather_than_an_error() {
        assert!(parse_adb_devices("List of devices attached\n\n").is_empty());
        assert!(parse_adb_devices("").is_empty());
    }

    #[test]
    fn installing_names_the_device_it_was_asked_about() {
        let device = Device {
            id: "emulator-5554".into(),
            name: "Pixel 8".into(),
            kind: Target::Android,
            state: DeviceState::Ready,
        };
        let spec = install(
            Path::new("/project"),
            &device,
            Path::new("/project/target/export/app.apk"),
        )
        .expect("android installs");
        // `-s <id>`, because a second phone plugged in mid-export would
        // otherwise make `adb install` ambiguous and it would refuse.
        assert!(spec.command_line().contains("-s emulator-5554"));
        assert!(spec.command_line().ends_with("app.apk"));
    }

    #[test]
    fn installing_to_a_target_with_no_path_says_so_rather_than_half_working() {
        for kind in [Target::Ios, Target::Desktop] {
            let device = Device {
                id: "x".into(),
                name: "x".into(),
                kind,
                state: DeviceState::Ready,
            };
            let refusal = install(Path::new("/p"), &device, Path::new("/p/app"))
                .expect_err("not implemented");
            assert!(matches!(refusal, Refusal::NotImplemented(_)));
        }
    }

    #[test]
    fn every_format_names_the_target_it_belongs_to() {
        // The sheet is built from `Format::ALL`, so a format with the wrong
        // target would run the wrong checklist and refuse for the wrong
        // reason.
        assert_eq!(Format::DesktopBinary.target(), Target::Desktop);
        assert_eq!(Format::MacAppBundle.target(), Target::Desktop);
        assert_eq!(Format::LinuxDeb.target(), Target::Desktop);
        assert_eq!(Format::WindowsExe.target(), Target::Windows);
        assert_eq!(Format::Web.target(), Target::Web);
        assert_eq!(Format::AndroidApk.target(), Target::Android);
        assert_eq!(Format::AndroidReleaseApk.target(), Target::Android);
        assert_eq!(Format::IosSimulatorApp.target(), Target::Ios);
        assert_eq!(Format::IosIpa.target(), Target::Ios);
        for format in Format::ALL {
            assert!(!format.title().is_empty());
            assert!(!format.detail().is_empty());
        }
    }
    #[test]
    fn the_package_name_comes_from_the_package_table_and_nowhere_else() {
        let manifest = "\
[package]
name = \"photo-studio\"
version = \"0.1.0\"

[[bin]]
name = \"something-else\"

[dependencies.serde]
name = \"not-this-either\"
";
        assert_eq!(
            package_name(manifest),
            Some("photo-studio".to_string()),
            "a `name` under [[bin]] is not the package's"
        );
    }

    #[test]
    fn a_name_with_a_trailing_comment_is_still_the_name() {
        assert_eq!(
            package_name("[package]\nname = \"app\" # the binary\n"),
            Some("app".to_string())
        );
    }

    #[test]
    fn a_manifest_with_no_package_table_has_no_name() {
        // A workspace root, which is a real thing to have open and not a
        // thing to export.
        assert_eq!(package_name("[workspace]\nmembers = [\"a\"]\n"), None);
        assert_eq!(package_name(""), None);
    }

    // ----- ST-08: the device mirror and the farm dispatch ---------------------

    /// A Ready Android device, and an artefact that exists.
    fn phone(dir: &tempdir::Dir) -> (Device, PathBuf) {
        let device = Device {
            id: "R58M12ABCDE".into(),
            name: "Pixel 8".into(),
            kind: Target::Android,
            state: DeviceState::Ready,
        };
        let apk = dir.path().join("app.apk");
        std::fs::write(&apk, "an apk").expect("the artefact");
        (device, apk)
    }

    #[test]
    fn a_mirror_installs_launches_screenshots_pulls_and_cleans_up_in_that_order() {
        let (env, dir) = complete(Host::Linux, &["adb"]);
        let (device, apk) = phone(&dir);
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join(".vieww")).expect("the private directory");

        let steps =
            mirror_steps(&env, &root, &device, &apk, "com.example.my_app").expect("a mirror");
        let labels: Vec<&str> = steps.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Install",
                "Launch on device",
                "Screenshot on device",
                "Pull screenshot",
                "Clean up on device"
            ],
            "each step is one adb invocation, and the phone is cleaned up last"
        );
        // Every step addresses the device by its serial, because a second
        // phone plugged in mid-mirror must not receive the screenshot request.
        for step in &steps {
            assert!(
                step.args.iter().any(|arg| arg == "R58M12ABCDE"),
                "{} does not address the device: {:?}",
                step.label,
                step.args
            );
        }
        // The launch names the component the generated manifest declares —
        // `am start` is the one place the package and the activity meet.
        let launch = &steps[1];
        assert!(
            launch
                .args
                .iter()
                .any(|arg| arg == "com.example.my_app/android.app.NativeActivity"),
            "the component the manifest's own activity resolves to: {:?}",
            launch.args
        );
        // The screenshot is written on the device and pulled as a file, never
        // piped: the jobs machinery captures text, and a PNG through a text
        // pipe arrives with its newlines rewritten.
        let screenshot = &steps[2];
        assert!(
            screenshot
                .args
                .windows(2)
                .any(|pair| pair == ["-p", MIRROR_REMOTE_PNG]),
            "screencap writes a file on the device — `-p`, then the path: {:?}",
            screenshot.args
        );
        let pull = &steps[3];
        let pulled_to = pull.args.last().expect("the local destination");
        assert!(
            pulled_to.contains(".vieww") && pulled_to.contains("mirror-R58M12ABCDE.png"),
            "the PNG lands under the workspace's private directory, named for the device: \
             {pulled_to}"
        );
        assert_eq!(pull.args[pull.args.len() - 2], MIRROR_REMOTE_PNG);
    }

    #[test]
    fn a_mirror_is_refused_for_every_reason_it_cannot_work_before_anything_runs() {
        let (env, dir) = complete(Host::Linux, &["adb"]);
        let (device, apk) = phone(&dir);
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).expect("the project");

        // An unready device: plugged in is not permission.
        let unauthorised = Device {
            state: DeviceState::Unauthorised,
            ..device.clone()
        };
        let refusal = mirror_steps(&env, &root, &unauthorised, &apk, "com.example.my_app")
            .expect_err("the phone has not accepted this computer");
        assert!(matches!(refusal, Refusal::Impossible(_)), "{refusal:?}");

        // A non-Android device has no adb path at all.
        let iphone = Device {
            kind: Target::Ios,
            ..device.clone()
        };
        let refusal = mirror_steps(&env, &root, &iphone, &apk, "com.example.my_app")
            .expect_err("iOS is not adb's to reach");
        assert!(matches!(refusal, Refusal::NotImplemented(_)), "{refusal:?}");

        // No adb on PATH.
        let (no_adb, _dir) = complete(Host::Linux, &[]);
        let refusal =
            mirror_steps(&no_adb, &root, &device, &apk, "com.example.my_app").expect_err("no adb");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "adb",
                install: "`brew install android-platform-tools` (macOS) or install \
                           `google-platform-tools` (Linux) — it is the `adb` half of the SDK, on its own",
            }
        );

        // No artefact: a mirror installs what an export built.
        let missing = root.join("never-exported.apk");
        let refusal = mirror_steps(&env, &root, &device, &missing, "com.example.my_app")
            .expect_err("nothing to install");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "an exported APK to mirror",
                install: "export the Android APK (debug-signed) first — the mirror installs what \
                          the export built",
            }
        );
    }

    #[test]
    fn am_start_is_answered_by_its_status_line_and_its_noise_is_not_the_answer() {
        assert!(parse_am_start("Starting: Intent\nStatus: ok\n"));
        // `am start -W` prints launch statistics after the status; a parser
        // that read the first matching-looking line would misread the lot.
        assert!(parse_am_start(
            "Starting: Intent\nStatus: ok\nLaunchState: COLD\nActivityTime: 812ms\nTotalTime: \
             812ms\nWaitTime: 836ms\n"
        ));
        assert!(!parse_am_start("Starting: Intent\nStatus: error\n"));
        assert!(!parse_am_start(""));
        // The one a renamed package produces — the mirror stops, the Output
        // panel has adb's own words for it.
        assert!(!parse_am_start(
            "Starting: Intent\nError: Activity class {com.example.my_app/com.example.my_app.\
             android.app.NativeActivity} does not exist.\n"
        ));
    }

    #[test]
    fn a_mirror_steps_output_can_fail_it_where_its_exit_code_did_not() {
        // am's own protocol, on the failure side of it.
        assert!(mirror_step_output_failed(
            "Starting: Intent\nStatus: error\n"
        ));
        // The classic renamed-package error with no Status line at all — the
        // shape that arrives with exit code 0 through some shells, which is
        // the whole reason the function exists.
        assert!(mirror_step_output_failed(
            "Starting: Intent\nError: Activity class {com.example.my_app/com.example.my_app.\
             android.app.NativeActivity} does not exist.\n"
        ));
        // Success, in am's own words, with its statistics after.
        assert!(!mirror_step_output_failed(
            "Starting: Intent\nStatus: ok\nLaunchState: COLD\nTotalTime: 812ms\n"
        ));
        // The other four steps' outputs carry no am verdict — `Success`,
        // `1 file pulled`, silence — so their exit codes stay the only ones.
        assert!(!mirror_step_output_failed(
            "Performing Streamed Install\nSuccess\n"
        ));
        assert!(!mirror_step_output_failed("1 file pulled"));
        assert!(!mirror_step_output_failed(""));
        // A warning is not a failure: `am start` prints exactly this when the
        // activity is already on top, and the launch did happen.
        assert!(!mirror_step_output_failed(
            "Warning: Activity not started, intent has been delivered to currently running \
             top-most instance.\nStatus: ok\n"
        ));
    }

    #[test]
    fn a_farm_dispatch_is_gh_workflow_run_with_the_inputs_the_workflow_defines() {
        let (env, dir) = complete(Host::Linux, &["gh"]);
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).expect("the project");

        let spec = farm_dispatch(&env, &root, "android", 3).expect("gh is installed");
        assert_eq!(spec.program, "gh");
        // The workflow file by name, on main because the farm's AWS trust
        // accepts main's OIDC subject only — a dispatch from anywhere else is
        // five minutes of build for a refusal the workflow's own first job
        // prints.
        assert!(
            spec.args.starts_with(
                &["workflow", "run", "device-farm.yml", "--ref", "main"].map(String::from)
            ),
            "{:?}",
            spec.args
        );
        // The inputs the workflow's `workflow_dispatch` block actually
        // declares: platform, count, profile. A typo here is a dispatch that
        // GitHub silently runs with the input at its default.
        let joined = spec.args.join(" ");
        assert!(joined.contains("platform=android"), "{joined}");
        assert!(joined.contains("count=3"), "{joined}");
        assert!(joined.contains("profile=dist"), "{joined}");
    }

    #[test]
    fn a_farm_dispatch_without_gh_names_the_cli_and_its_login_not_a_gh_error() {
        let (env, dir) = complete(Host::Linux, &[]);
        let root = dir.path();
        let refusal = farm_dispatch(&env, root, "android", 3).expect_err("no gh");
        assert_eq!(
            refusal,
            Refusal::Missing {
                requirement: "GitHub CLI (`gh`)",
                install: "`brew install gh` (macOS) or follow github.com/cli/cli — then `gh auth \
                      login`, because a dispatch is an API call as you",
            }
        );
    }

    #[test]
    fn a_farm_dispatch_refuses_a_platform_the_workflow_does_not_define() {
        let (env, dir) = complete(Host::Linux, &["gh"]);
        let refusal = farm_dispatch(&env, dir.path(), "webos", 3).expect_err("no such input");
        assert!(matches!(refusal, Refusal::Impossible(_)), "{refusal:?}");
    }

    #[test]
    fn the_launch_package_is_the_gradle_applicationid_not_the_cargo_name() {
        let dir = tempdir::Dir::new("viewwstudio-export-launch");
        let gradle = dir.path().join("android").join("app");
        std::fs::create_dir_all(&gradle).expect("the gradle directory");
        std::fs::write(
            gradle.join("build.gradle"),
            "android {\n    namespace \"com.example.my_app\"\n    defaultConfig {\n        \
             applicationId \"uk.co.real.shipper\"\n    }\n}\n",
        )
        .expect("the gradle file");

        // The phone knows the app by this, not by `my-app` — a renamed
        // applicationId is the first thing the README tells a user to do.
        assert_eq!(launch_package(dir.path(), "my-app"), "uk.co.real.shipper");
        // No Gradle file at all: the scaffolder's reserved id, which is what
        // the file the scaffolder would have written says.
        assert_eq!(
            launch_package(Path::new("/nowhere"), "my-app"),
            "com.example.my_app"
        );
        // A Gradle file whose applicationId line was edited past recognition:
        // the same fallback, rather than a guess about what it used to say.
        std::fs::write(
            gradle.join("build.gradle"),
            "android {\n    defaultConfig {\n        applicationId \"\"\n    }\n}\n",
        )
        .expect("rewrite");
        assert_eq!(launch_package(dir.path(), "my-app"), "com.example.my_app");
    }
}
