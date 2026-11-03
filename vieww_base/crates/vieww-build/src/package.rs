//! Turning a built vieww app into something a platform can install.
//!
//! # What this is not
//!
//! It is not `packaging/package.sh`. That script is `viewwstudio`'s own
//! release pipeline — it stages the studio's SDK (a bundled `rustc`, the
//! rlibs a preview compiles against, `vieww-sdk.toml`), which is a concern
//! specific to a code editor that `dlopen`s previews into its own process.
//! Nothing here needs any of that: [`package`] takes the directory of *any*
//! cargo binary project (in practice, the output of [`crate::scaffold`], but
//! nothing below checks for that specifically) and produces the plain
//! platform artefact a normal application ships — a `.desktop`-bearing tree
//! on Linux, an `.app` bundle on macOS, an `.msi` on Windows, an APK, an
//! `.ipa`. `packaging/package.sh` remains the right tool for packaging the
//! studio itself; this is the equivalent for a project `vieww new` made.
//!
//! # "Don't reimplement WiX or APK signing from scratch" — followed literally
//!
//! `package_windows` does not construct an MSI's binary format; it writes a
//! small `.wxs` (WiX's own XML authoring format, the same language
//! `packaging/viewwstudio.wxs` is written in) and calls the real `wix`
//! toolset to build it. `package_android` does not hand-sign a JAR; on a
//! machine that has the SDK, it shells out to `cargo apk`, which drives the
//! real Android build tools including `apksigner`. Both are named as
//! required tools rather than assumed present — see [`PackageError::MissingTool`]
//! — and both are refused with a named reason rather than attempted badly
//! when the tool is not there.
//!
//! # Which of these five targets is real *on this machine*
//!
//! This module is written and compiled unconditionally on every host — there
//! is no `#[cfg(target_os = ...)]` anywhere in it, because a `Command` you do
//! not run costs nothing to have compiled and a reader on any platform should
//! be able to read every code path. What differs by host is which of the
//! dispatch functions ever gets *past its own gate*:
//!
//! * **Linux** (`package_linux`) — fully real and fully exercised by this
//!   crate's own tests on every machine that runs them, this sandbox
//!   included: it runs `cargo build --release` for real and assembles a real
//!   `bin/` + `share/applications/*.desktop` tree.
//! * **macOS** (`package_macos`) — real code, calling real `iconutil` and
//!   assembling a real `Contents/{MacOS,Resources}` layout, exactly the shape
//!   `packaging/package.sh` uses for the studio. It is gated on
//!   [`PackageError::WrongHost`] first, because the binary
//!   inside a macOS `.app` has to actually be a macOS binary — Cargo cannot
//!   produce one on Linux without a cross toolchain this environment does not
//!   have, the same limitation `crates/vieww-hal/src/metal.rs` documents for
//!   the render backend itself. **Not exercised on this Linux sandbox**, and
//!   the host gate is exactly what a test here can assert without a Mac.
//! * **Windows** (`package_windows`) — real code calling the real `wix`
//!   toolset, gated the same way and for the same reason: an `.exe` built for
//!   Windows cannot come out of a Linux `cargo build` here either. **Not
//!   exercised on this Linux sandbox.**
//! * **Android** (`package_android`) — gated on tool/SDK *presence* rather
//!   than host, since an APK can genuinely be built from Linux, macOS or
//!   Windows given the SDK, the NDK and `cargo apk` (`ci/mobile/apk.sh` and
//!   `ci/mobile/android-env.sh` are this workspace's own proof of that). This
//!   sandbox has none of the three, so the real gate is what a test here
//!   exercises: a [`PackageError::MissingTool`], not a panic and not a
//!   silent no-op.
//! * **Android App Bundle** (`package_aab`, target `aab`) — the `.aab` Play
//!   actually accepts, via the project's own `gradlew bundleRelease` when it
//!   has one, else `cargo apk` + `unzip`/`zip` + `bundletool build-bundle`
//!   reshaping the APK into a base module. Same host-anywhere, tool-gated
//!   shape as the APK; the route decision ([`aab_plan`]) is a pure function
//!   of which tools a `PATH` names, so this sandbox tests it with fake tools
//!   on a fake `PATH` — the running of the real ones is the part left to
//!   machines that have an Android toolchain.
//! * **Multi-ABI APK** (`package_android_multi_abi`, target
//!   `android-multi-abi`) — one APK with `lib/arm64-v8a/` and `lib/x86_64/`
//!   side by side, for the machine that is a phone today and an emulator
//!   tomorrow: per-target `cargo apk` builds, `unzip`ed into one tree, packed
//!   stored, and placed by the SDK's own `zipalign`. Refusal-first like the
//!   AAB, and its merge plan is as pure as the AAB's route, so this sandbox
//!   tests the whole plan without an SDK; the merged-and-aligned APK itself
//!   is left to the machines that have one.
//! * **iOS** (`package_ios`) — gated on host *and* on `xcrun`, mirroring
//!   `ci/mobile/ios-app.sh`'s own opening check verbatim. **Not exercised on this
//!   Linux sandbox** for the same reason macOS is not.
//!
//! Every gate below is a plain function of a string (`host_os()`'s value) or
//! a boolean (`tool_on_path`'s answer), which is what lets every one of them
//! be unit-tested on whatever machine happens to be running this suite,
//! without that machine needing to *be* a Mac or a Windows box to prove the
//! refusal is correct — see the `tests` module for exactly that.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{host_os, is_probably_binary, tool_in_dirs, tool_on_path};

/// A platform artefact [`package`] knows how to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageTarget {
    /// A `bin/` + `share/applications/*.desktop` tree.
    Linux,
    /// A `.app` bundle.
    MacOs,
    /// An `.msi`, via the WiX toolset.
    Windows,
    /// An APK, via `cargo apk`.
    Android,
    /// One APK carrying every ABI the framework supports (`arm64-v8a` and
    /// `x86_64`), via per-target `cargo apk` builds merged with the SDK's own
    /// `zipalign` — the emulator-installable counterpart of `android`'s
    /// single-ABI APK.
    AndroidMultiAbi,
    /// An `.aab`, via the project's `gradlew bundleRelease` or
    /// `bundletool build-bundle`.
    Aab,
    /// An `.ipa`-shaped bundle, via `xcrun`.
    Ios,
}

impl PackageTarget {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::Android => "android",
            Self::AndroidMultiAbi => "android-multi-abi",
            Self::Aab => "aab",
            Self::Ios => "ios",
        }
    }
}

impl fmt::Display for PackageTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why [`package`] could not produce an artefact.
#[derive(Debug)]
pub enum PackageError {
    /// `target` needs a build for that platform, and this host cannot
    /// produce one — either because it is not that platform (an `.app`'s
    /// binary has to actually be a macOS binary) or because no cross
    /// toolchain for it is installed here.
    WrongHost {
        target: PackageTarget,
        host: &'static str,
    },
    /// A tool this target's packaging step needs is not on `PATH` — owned
    /// strings, because the AAB's refusal names *every* missing tool with its
    /// install command, composed at run time rather than enumerable up front.
    MissingTool {
        target: PackageTarget,
        tool: String,
        how_to_get_it: String,
    },
    /// `app_dir`'s `Cargo.toml` could not be found or has no `[package]
    /// name`.
    Manifest(PathBuf),
    /// `cargo build --release` (or one of the packaging steps that wrap it —
    /// `cargo apk`, `gradlew bundleRelease`, `unzip`/`zip`/`bundletool`) ran and
    /// exited non-zero. Carries the failing command's stderr, prefixed with
    /// the command itself by the caller where that is not `cargo build`.
    Build(String),
    /// The build reported success but the binary it should have produced is
    /// not where this expected it — a build that used a different
    /// `--target-dir`, cross-compiled to a different triple, or otherwise
    /// did not produce a plain `target/release/<name>`.
    MissingArtifact(PathBuf),
    Io(std::io::Error),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongHost { target, host } => write!(
                f,
                "cannot package for {target} on a {host} host — a {target} artefact needs a \
                 binary actually built for {target}, and this machine has no way to produce one"
            ),
            Self::MissingTool {
                target,
                tool,
                how_to_get_it,
            } => write!(
                f,
                "cannot package for {target}: no `{tool}` on PATH — {how_to_get_it}"
            ),
            Self::Manifest(path) => write!(
                f,
                "{} has no readable [package] name — is this a cargo project?",
                path.display()
            ),
            Self::Build(stderr) => write!(f, "a build step failed:\n{stderr}"),
            Self::MissingArtifact(path) => write!(
                f,
                "the build reported success but {} does not exist",
                path.display()
            ),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// What a packaging run produced.
#[derive(Debug, Clone)]
pub struct PackageReport {
    pub target: PackageTarget,
    /// The directory everything below was written into.
    pub output_dir: PathBuf,
    /// Every artefact written, in the order it was written.
    pub artifacts: Vec<PathBuf>,
    /// Things worth telling a person that are not failures — "shipping
    /// without an icon", the same tone `packaging/package.sh` uses for its
    /// own non-fatal notes.
    pub notes: Vec<String>,
}

/// Package the cargo project at `app_dir` for `target`.
///
/// # Errors
///
/// See [`PackageError`]'s variants. In every case where a required tool or
/// host is missing, this returns before touching `app_dir` or running a
/// build — a refusal never leaves a half-built artefact behind.
pub fn package(target: PackageTarget, app_dir: &Path) -> Result<PackageReport, PackageError> {
    check_host(target, host_os())?;
    match target {
        PackageTarget::Linux => package_linux(app_dir),
        PackageTarget::MacOs => package_macos(app_dir),
        PackageTarget::Windows => package_windows(app_dir),
        PackageTarget::Android => package_android(app_dir),
        PackageTarget::AndroidMultiAbi => package_android_multi_abi(app_dir),
        PackageTarget::Aab => package_aab(app_dir),
        PackageTarget::Ios => package_ios(app_dir),
    }
}

/// Whether `host` can even attempt to build for `target` — the part of
/// [`package`]'s gate that depends only on which operating system is running
/// it, pulled out so it is testable with a fake host string on every machine
/// this suite runs on.
///
/// Android is the one target with no host restriction at all: the real gate
/// for it is tool presence ([`android_tooling`]), checked separately inside
/// [`package_android`], because the SDK/NDK/`cargo apk` triple can genuinely
/// be missing or present on any of the three desktop hosts. The AAB is the
/// second such target, for the same reason — its gates are [`aab_plan`]'s
/// tool checks, and the multi-ABI APK's are
/// [`package_android_multi_abi`]'s own.
fn check_host(target: PackageTarget, host: &str) -> Result<(), PackageError> {
    let ok = match target {
        PackageTarget::Linux => host == "linux",
        PackageTarget::MacOs | PackageTarget::Ios => host == "macos",
        PackageTarget::Windows => host == "windows",
        PackageTarget::Android | PackageTarget::Aab | PackageTarget::AndroidMultiAbi => true,
    };
    if ok {
        Ok(())
    } else {
        Err(PackageError::WrongHost {
            target,
            host: leak_or_known(host),
        })
    }
}

/// `check_host`'s error carries `&'static str`, and the real value always is
/// one of `std::env::consts::OS`'s compile-time constants — this only exists
/// so a *test* can hand in an arbitrary owned string and still get a
/// `PackageError` to inspect, without `PackageError` itself needing a
/// lifetime parameter for a case that never happens outside a test.
fn leak_or_known(host: &str) -> &'static str {
    match host {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => "an unrecognised host",
    }
}

/// Read `[package] name` out of `app_dir/Cargo.toml`.
///
/// A small hand-rolled TOML walk rather than a dependency on a TOML parser —
/// this crate already keeps its dependency list to `vieww-foundation` and
/// `vieww-hardware`, and the one thing this needs is exactly what
/// `packaging/package.sh` extracts the same way, with `sed`: the value of one
/// key inside one section, in a file this crate itself may have just written.
fn read_package_name(app_dir: &Path) -> Result<String, PackageError> {
    let manifest_path = app_dir.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|_| PackageError::Manifest(manifest_path.clone()))?;

    let mut in_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("name") else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
    }
    Err(PackageError::Manifest(manifest_path))
}

/// `cargo build --release` in `app_dir`, real and blocking.
fn cargo_build_release(app_dir: &Path) -> Result<(), PackageError> {
    let output = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(app_dir)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(PackageError::Build(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

/// A minimal `.desktop` entry. Generic — unlike
/// `packaging/viewwstudio.desktop`, which names the studio's own MIME types
/// and `%F` file-drop handling, this names nothing about what the app does,
/// because a `vieww new` project has not said yet.
fn desktop_entry(name: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Exec={name}\n\
         Terminal=false\n\
         Categories=Utility;\n"
    )
}

/// Build for real and lay out `bin/` + `share/applications/*.desktop`.
///
/// Real end to end, and the target [`crate`]'s own tests exercise fully — see
/// `tests` below.
fn package_linux(app_dir: &Path) -> Result<PackageReport, PackageError> {
    let name = read_package_name(app_dir)?;
    cargo_build_release(app_dir)?;

    let binary = app_dir.join("target/release").join(&name);
    if !is_probably_binary(&binary) {
        return Err(PackageError::MissingArtifact(binary));
    }

    let output_dir = app_dir.join("target/package").join(format!("{name}-linux"));
    std::fs::create_dir_all(output_dir.join("bin"))?;
    std::fs::create_dir_all(output_dir.join("share/applications"))?;

    let bin_dest = output_dir.join("bin").join(&name);
    std::fs::copy(&binary, &bin_dest)?;

    let desktop_dest = output_dir
        .join("share/applications")
        .join(format!("{name}.desktop"));
    std::fs::write(&desktop_dest, desktop_entry(&name))?;

    Ok(PackageReport {
        target: PackageTarget::Linux,
        output_dir,
        artifacts: vec![bin_dest, desktop_dest],
        notes: Vec::new(),
    })
}

/// Whether the project asks to keep playing with the screen locked
/// (AU-04): `background-audio = true` under `[package.metadata.vieww]`.
///
/// iOS suspends an app the moment it leaves the foreground unless its
/// `Info.plist` lists `audio` in `UIBackgroundModes`. App Review rejects
/// the key on an app that plays nothing in the background, so it is opt-in
/// rather than written into every bundle.
fn wants_background_audio(app_dir: &Path) -> bool {
    let Ok(manifest) = std::fs::read_to_string(app_dir.join("Cargo.toml")) else {
        return false;
    };
    let mut in_section = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == "[package.metadata.vieww]";
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            if key.trim() == "background-audio" {
                return value.split('#').next().is_some_and(|v| v.trim() == "true");
            }
        }
    }
    false
}

/// A generic `Info.plist`. Modelled on `packaging/Info.plist`'s shape (the
/// same keys, `CFBundleIdentifier` built the same way) but with no document
/// types and no fixed name — those are the studio's own, not a scaffolded
/// project's.
fn info_plist(name: &str, background_audio: bool) -> String {
    let background = if background_audio {
        "\t<key>UIBackgroundModes</key>\n\t<array>\n\t\t<string>audio</string>\n\t</array>\n"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
        <key>CFBundleName</key>
        <string>{name}</string>
        <key>CFBundleIdentifier</key>
        <string>dev.vieww.{name}</string>
        <key>CFBundleExecutable</key>
        <string>{name}</string>
        <key>CFBundlePackageType</key>
        <string>APPL</string>
        <key>CFBundleShortVersionString</key>
        <string>0.1.0</string>
        <key>NSHighResolutionCapable</key>
        <true/>
{background}</dict>
</plist>
"#
    )
}

/// Build for macOS and lay out a real `.app`.
///
/// Reached only when [`check_host`] has already confirmed the host is macOS —
/// `cargo build --release` here genuinely produces a Mach-O binary, and
/// `iconutil`, when present, genuinely produces an `.icns` exactly as
/// `packaging/package.sh`'s own macOS branch does. Everything past the gate
/// in [`package`] is real Apple tooling; what makes it untestable on this
/// Linux sandbox is exactly and only that gate, which the `tests` module
/// checks directly with a synthetic host string instead.
fn package_macos(app_dir: &Path) -> Result<PackageReport, PackageError> {
    let name = read_package_name(app_dir)?;
    cargo_build_release(app_dir)?;

    let binary = app_dir.join("target/release").join(&name);
    if !is_probably_binary(&binary) {
        return Err(PackageError::MissingArtifact(binary));
    }

    let app = app_dir.join("target/package").join(format!("{name}.app"));
    let macos_dir = app.join("Contents/MacOS");
    let resources_dir = app.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&resources_dir)?;

    let bin_dest = macos_dir.join(&name);
    std::fs::copy(&binary, &bin_dest)?;

    let plist_dest = app.join("Contents/Info.plist");
    std::fs::write(&plist_dest, info_plist(&name, false))?;

    let mut artifacts = vec![bin_dest, plist_dest];
    let mut notes = if tool_on_path("iconutil") {
        vec![
            "iconutil is present but this generic packager ships no source icon to convert; \
             see packaging/package.sh's macOS branch for the real .iconset -> .icns pipeline"
                .to_owned(),
        ]
    } else {
        vec!["no iconutil on PATH — shipping without an .icns".to_owned()]
    };

    // FW-04: MoltenVK into `Contents/Frameworks`, the first place
    // `vieww_hal::vulkan::load_entry` looks after the system loader. A note
    // rather than an error when no source exists, because this packager serves
    // any vieww app and one that deliberately targets the Metal backend needs
    // no Vulkan portability layer at all — the studio's own release pipeline
    // (`packaging/package.sh --bundle-moltenvk`, made fatal there) is where a
    // missing MoltenVK stops a release.
    match bundle_moltenvk(&app, app_dir) {
        MoltenVkOutcome::Bundled { files } => artifacts.extend(files),
        MoltenVkOutcome::Absent { searched } => notes.push(format!(
            "no MoltenVK found ({}), so the bundle cannot open the GPU on a machine \
             without a Vulkan SDK or Homebrew — install the SDK, `brew install molten-vk`, \
             or drop a copy in a `moltenvk/` directory beside Cargo.toml; \
             vieww_hal::vulkan::load_entry searches Contents/Frameworks first",
            searched.join(", ")
        )),
    }

    Ok(PackageReport {
        target: PackageTarget::MacOs,
        output_dir: app,
        artifacts,
        notes,
    })
}

// ── MoltenVK (FW-04) ────────────────────────────────────────────────────────

/// A found MoltenVK: the dylib itself and, when the source shipped one, the
/// ICD json that tells a Vulkan loader where the dylib is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MoltenVk {
    dylib: PathBuf,
    icd: Option<PathBuf>,
}

/// What attempting to bundle MoltenVK into an `.app` came to.
#[derive(Debug)]
enum MoltenVkOutcome {
    /// The files now inside the bundle (dylib, and the ICD json when written).
    Bundled { files: Vec<PathBuf> },
    /// Nothing was bundled; `searched` names every location that was tried,
    /// because "not found" with no list is a riddle, not an error.
    Absent { searched: Vec<String> },
}

/// MoltenVK under an SDK- or Homebrew-shaped root: `lib/libMoltenVK.dylib`
/// with the ICD json under `share/vulkan/icd.d` (newer layouts) or
/// `etc/vulkan/icd.d` (older ones). Pure over its input, so the recognition
/// itself is testable on any host — the macOS-only part is only the *call*.
fn moltenvk_under(root: &Path) -> Option<MoltenVk> {
    let dylib = root.join("lib/libMoltenVK.dylib");
    if !dylib.is_file() {
        return None;
    }
    let icd = ["share/vulkan/icd.d", "etc/vulkan/icd.d"]
        .iter()
        .map(|sub| root.join(sub).join("MoltenVK_icd.json"))
        .find(|path| path.is_file());
    Some(MoltenVk { dylib, icd })
}

/// Where a MoltenVK can come from, in the order `vieww_hal::vulkan::load_entry`
/// prefers the sources themselves: `$VULKAN_SDK` (a deliberate install), then
/// Homebrew (Apple silicon prefix, then Intel), then a copy vendored beside
/// the project's own manifest — `moltenvk/libMoltenVK.dylib` + optionally
/// `moltenvk/MoltenVK_icd.json`, a flat pair rather than an SDK tree, because
/// a hand-dropped pair is two files and a fake SDK root is a directory tree.
///
/// Pure over its inputs for the same reason every other gate here is: the
/// order is the behaviour, and the order is testable on any machine.
fn find_moltenvk(
    sdk: Option<&str>,
    homebrew_prefixes: &[&str],
    vendored: &Path,
) -> Result<MoltenVk, Vec<String>> {
    let mut searched = Vec::new();
    if let Some(sdk) = sdk.map(str::trim).filter(|s| !s.is_empty()) {
        let root = Path::new(sdk);
        searched.push(root.join("lib").display().to_string());
        if let Some(found) = moltenvk_under(root) {
            return Ok(found);
        }
    } else {
        searched.push("$VULKAN_SDK (unset)".to_owned());
    }
    for prefix in homebrew_prefixes {
        let root = Path::new(prefix);
        searched.push(root.join("lib").display().to_string());
        if let Some(found) = moltenvk_under(root) {
            return Ok(found);
        }
    }
    searched.push(vendored.display().to_string());
    let dylib = vendored.join("libMoltenVK.dylib");
    if dylib.is_file() {
        let icd = vendored.join("MoltenVK_icd.json");
        return Ok(MoltenVk {
            dylib,
            icd: icd.is_file().then_some(icd),
        });
    }
    Err(searched)
}

/// Rewrite an ICD json's `library_path` to the dylib's place inside the
/// bundle: the json lands in `Contents/Resources/vulkan/icd.d/`, so
/// `../../Frameworks/libMoltenVK.dylib` is the copy two directories up and
/// over. A json that still pointed at `/opt/homebrew/lib/...` would describe
/// a library the bundle does not contain — the exact silent gap a bundled
/// ICD exists to close.
///
/// Hand-rolled, no regex and no JSON parser, for the same reason the manifest
/// reader in `vieww-update` is: the value is one quoted string after one
/// known key, and a parser that can read more is a parser that can misread
/// this. The key is matched *with* both its quotes, so `"library_path_extra"`
/// is not a hit, and a value that is not a string (or a string that never
/// closes) leaves the text untouched rather than mangled. Unchanged text
/// passes through byte-for-byte. (A path containing an escaped `\"` is not a
/// path; the scan does not try to honour escapes.)
fn rewrite_icd_library_path(source: &str) -> String {
    const KEY: &str = "\"library_path\"";
    const REPLACE: &str = "\"library_path\" : \"../../Frameworks/libMoltenVK.dylib\"";
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut at = 0;
    while let Some(found) = source[at..].find(KEY) {
        let start = at + found;
        out.push_str(&source[at..start]);
        // The value: from the byte after the key, skip the separator
        // (spaces and the colon), then replace one *closed* quoted string.
        let mut end = start + KEY.len();
        while matches!(bytes.get(end), Some(b' ' | b'\t' | b':')) {
            end += 1;
        }
        let replaced = if bytes.get(end) == Some(&b'"') {
            // `to_close` is the offset of the value's closing quote; a value
            // that never closes is not this rewriter's to touch.
            match source[end + 1..].find('"') {
                Some(to_close) => {
                    out.push_str(REPLACE);
                    at = end + 1 + to_close + 1;
                    true
                }
                None => false,
            }
        } else {
            false
        };
        if !replaced {
            // Not the shape this rewrites: keep the key and continue after it.
            out.push_str(KEY);
            at = start + KEY.len();
        }
    }
    out.push_str(&source[at..]);
    out
}

/// Put MoltenVK into `app/Contents/Frameworks` (and the ICD json into
/// `Contents/Resources/vulkan/icd.d`), from wherever it can be found — the
/// FW-04 stage of [`package_macos`].
fn bundle_moltenvk(app: &Path, app_dir: &Path) -> MoltenVkOutcome {
    let sdk = std::env::var("VULKAN_SDK").ok();
    let vendored = app_dir.join("moltenvk");
    let found = match find_moltenvk(sdk.as_deref(), &["/opt/homebrew", "/usr/local"], &vendored) {
        Ok(found) => found,
        Err(searched) => return MoltenVkOutcome::Absent { searched },
    };

    let mut files = Vec::with_capacity(2);
    let frameworks = app.join("Contents/Frameworks");
    let dylib_dest = frameworks.join("libMoltenVK.dylib");
    if std::fs::create_dir_all(&frameworks).is_err()
        || std::fs::copy(&found.dylib, &dylib_dest).is_err()
    {
        return MoltenVkOutcome::Absent {
            searched: vec![format!(
                "{} (found, but could not be copied into the bundle)",
                found.dylib.display()
            )],
        };
    }
    files.push(dylib_dest);

    if let Some(icd) = found.icd {
        let icd_dir = app.join("Contents/Resources/vulkan/icd.d");
        let icd_dest = icd_dir.join("MoltenVK_icd.json");
        if let Ok(text) = std::fs::read_to_string(&icd) {
            if std::fs::create_dir_all(&icd_dir).is_ok()
                && std::fs::write(&icd_dest, rewrite_icd_library_path(&text)).is_ok()
            {
                files.push(icd_dest);
            }
        }
    }
    MoltenVkOutcome::Bundled { files }
}

/// The WiX 4 authoring this writes and builds — a generic, single-executable
/// version of `packaging/viewwstudio.wxs`'s `<Files Include="...\**"/>`
/// pattern, with the version and payload directory substituted the same way
/// `packaging/package.sh` passes `-d Version=... -d Payload=...`.
fn wxs_source(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
        <Package
                Name="{name}"
                Manufacturer="vieww"
                Version="$(Version)"
                UpgradeCode="8f2a6d31-4a1e-4f6b-9c2b-1d0c7b5a9e45"
                Scope="perMachine"
                Compressed="yes">
                <MajorUpgrade AllowSameVersionUpgrades="yes" />
                <MediaTemplate EmbedCab="yes" />
                <StandardDirectory Id="ProgramFiles64Folder">
                        <Directory Id="INSTALLFOLDER" Name="{name}">
                                <Files Include="$(Payload)\**" />
                        </Directory>
                </StandardDirectory>
        </Package>
</Wix>
"#
    )
}

/// Build for Windows and produce an `.msi` via the real `wix` toolset.
///
/// Reached only once [`check_host`] has confirmed a Windows host, so
/// `cargo build --release` here genuinely produces a PE `.exe` for `wix` to
/// wrap — nothing about generating the MSI's binary format is reimplemented;
/// `wix build` does that, exactly as [`super`]'s module docs say. Not
/// exercised on this Linux sandbox for the same reason [`package_macos`]
/// is not.
fn package_windows(app_dir: &Path) -> Result<PackageReport, PackageError> {
    if !tool_on_path("wix") {
        return Err(PackageError::MissingTool {
            target: PackageTarget::Windows,
            tool: "wix".to_owned(),
            how_to_get_it: "dotnet tool install --global wix".to_owned(),
        });
    }

    let name = read_package_name(app_dir)?;
    cargo_build_release(app_dir)?;

    let binary = app_dir.join("target/release").join(format!("{name}.exe"));
    if !is_probably_binary(&binary) {
        return Err(PackageError::MissingArtifact(binary));
    }

    let payload = app_dir
        .join("target/package")
        .join(format!("{name}-windows"));
    std::fs::create_dir_all(&payload)?;
    std::fs::copy(&binary, payload.join(format!("{name}.exe")))?;

    let wxs_path = app_dir.join("target/package").join(format!("{name}.wxs"));
    std::fs::write(&wxs_path, wxs_source(&name))?;

    let version = std::fs::read_to_string(app_dir.join("Cargo.toml"))
        .ok()
        .and_then(|manifest| {
            manifest.lines().find_map(|line| {
                let trimmed = line.trim();
                trimmed
                    .strip_prefix("version")?
                    .trim_start()
                    .strip_prefix('=')
                    .map(|v| v.trim().trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| "0.1.0".to_owned());

    let msi_path = app_dir.join("target/package").join(format!("{name}.msi"));

    let output = Command::new("wix")
        .arg("build")
        .arg("-arch")
        .arg("x64")
        .arg("-d")
        .arg(format!("Version={version}"))
        .arg("-d")
        .arg(format!("Payload={}", payload.display()))
        .arg("-o")
        .arg(&msi_path)
        .arg(&wxs_path)
        .output()?;

    if !output.status.success() {
        return Err(PackageError::Build(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    Ok(PackageReport {
        target: PackageTarget::Windows,
        output_dir: app_dir.join("target/package"),
        artifacts: vec![msi_path],
        notes: vec![
            "unsigned: Authenticode signing needs a certificate this crate does not have"
                .to_owned(),
        ],
    })
}

/// What `package_android` needs on `PATH`/in the environment to attempt a
/// build, checked as a pure function of the inputs so it is testable without
/// an SDK — the same split `vieww_hardware::android_from_listing` uses for
/// its own `adb` parsing.
fn android_tooling(android_home: Option<&str>, has_cargo_apk: bool) -> Result<(), String> {
    let mut missing = Vec::new();
    if android_home.map(str::trim).unwrap_or_default().is_empty() {
        missing.push("ANDROID_HOME (or ANDROID_SDK_ROOT) is not set");
    }
    if !has_cargo_apk {
        missing.push("no `cargo-apk` subcommand on PATH");
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing.join("; "))
    }
}

/// Whether `cargo apk --version` runs — the real presence check
/// [`package_android`] uses; a plain [`tool_on_path`] lookup would not do,
/// since `cargo-apk` is invoked as a cargo subcommand rather than by its
/// binary name being typed directly.
fn have_cargo_apk() -> bool {
    Command::new("cargo")
        .args(["apk", "--version"])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Build an APK via `cargo apk`, the real Android build tool this workspace
/// already depends on for `ci/mobile/apk.sh`.
///
/// The APK this produces is **single-ABI** — whatever `cargo apk` builds
/// without a `--target`, which is arm64. One file that installs on both a
/// phone and an emulator is [`PackageTarget::AndroidMultiAbi`]'s to build;
/// this route stays single because a device-picked build is half the size and
/// twice as fast to link.
///
/// **Expects an app already set up for Android** — a `[package.metadata.android]`
/// table and a `cdylib` crate type, the shape
/// `crates/vieww-platform-winit/Cargo.toml`'s own `[package.metadata.android]`
/// documents. A project fresh out of [`crate::scaffold`] does not have either
/// yet, because the minimal desktop scaffold is a plain binary — this target
/// is for a project a developer has since extended for mobile, the same way
/// `apps/viewwstudio`'s own Android harness is written by its `scaffold`
/// module rather than assumed. `cargo apk build` will say so on its own if
/// the metadata is missing; this function does not duplicate that check.
fn package_android(app_dir: &Path) -> Result<PackageReport, PackageError> {
    let android_home = std::env::var("ANDROID_HOME")
        .ok()
        .or_else(|| std::env::var("ANDROID_SDK_ROOT").ok());
    let sdk_missing = android_home
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty();
    if android_tooling(android_home.as_deref(), have_cargo_apk()).is_err() {
        let how_to_get_it = if sdk_missing {
            "set ANDROID_HOME (or ANDROID_SDK_ROOT) to an installed Android SDK, then \
             cargo install cargo-apk if that is missing too"
        } else {
            "cargo install cargo-apk"
        }
        .to_owned();
        return Err(PackageError::MissingTool {
            target: PackageTarget::Android,
            tool: "cargo apk / Android SDK".to_owned(),
            how_to_get_it,
        });
    }

    let output = Command::new("cargo")
        .args(["apk", "build", "--release"])
        .current_dir(app_dir)
        .output()?;
    if !output.status.success() {
        return Err(PackageError::Build(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    // `cargo apk` writes the APK under target/<profile>/apk/ itself; this
    // reports the directory rather than guessing a filename, since the name
    // comes from `[package.metadata.android] apk_name`, which this module
    // has no reason to duplicate reading.
    let output_dir = app_dir.join("target/release/apk");
    Ok(PackageReport {
        target: PackageTarget::Android,
        output_dir: output_dir.clone(),
        artifacts: vec![output_dir],
        notes: vec![
            "built by cargo apk; see cargo apk's own output above for the exact .apk path"
                .to_owned(),
        ],
    })
}

// ── the Android App Bundle (FW-06) ──────────────────────────────────────────
//
// Play accepts an `.aab`, not an `.apk`: the store builds and signs every
// device-specific split from the bundle itself, which is also why the bundle
// this module hands over is deliberately *not* release-signed — under Play
// App Signing the only key this pipeline ever needs is the upload key
// (`packaging/sign.sh apk <file.aab>`, jarsigner, when the keystore secrets
// exist), and Google holds the real one. Two machines can produce a bundle
// from a cargo project, and nothing here pretends at a third:
//
// * **`gradlew bundleRelease`** — the project's own Android build. A project
//   that carries a Gradle wrapper (`apps/viewwstudio`'s scaffold writes one)
//   has already said how its bundle is built; second-guessing it with
//   `bundletool` would drop whatever that build configures.
// * **`bundletool build-bundle`** — for the plain cargo project `vieww new`
//   makes, which has no Gradle harness at all: `cargo apk` builds the APK,
//   `unzip` lays its (already binary-compiled) manifest and native libraries
//   out as a base module — signatures excluded, since a module must not carry
//   the APK's — `zip` packs the module, and `bundletool` turns it into the
//   bundle. `java`, `zip` and `unzip` are that route's real dependencies:
//   `bundletool` is a jar, and this crate has no zip implementation to be a
//   fourth.
//
// The route decision is [`aab_plan`], a pure function of [`AndroidTools`] — which
// tools a `PATH` names and whether the project has a wrapper — so the planner
// is fully testable with a directory of fake tools and no Android SDK at all
// (the same split `android_tooling`/`tool_in_dirs` exist for). What is *not*
// exercised on this Linux sandbox is running the real tools: no machine here
// has `bundletool`, `gradle` or `cargo apk`, so the end-to-end test below
// asserts the refusal instead, and the first real bundle is left to the
// release-gate's Android legs.

/// One tool a packaging route lacks, with how to install it — the refusal's
/// unit, so a test can assert the *fix* is named, not just the problem.
///
/// Named for what it is rather than for the route that first needed it: both
/// the AAB routes and the multi-ABI APK merge build their refusals from these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ToolRefusal {
    tool: &'static str,
    how: &'static str,
}

impl ToolRefusal {
    /// "`bundletool` — download bundletool.jar …", the shape every refusal
    /// line takes.
    fn line(self) -> String {
        format!("`{}` — {}", self.tool, self.how)
    }
}

/// What the Android packaging routes need from the machine and the project, as
/// presence booleans. Detected from an explicit list of `PATH` directories (not
/// the process's own `PATH`, which a test must not mutate — see
/// [`crate::util::tool_on_path`]'s own doc for why), so the detection and the
/// decision built on it are testable apart from each other.
///
/// Named for the platform rather than for one of its routes: the multi-ABI
/// APK merge below detects its `zip`/`unzip`/`cargo apk` needs through the
/// same struct rather than through a second copy of the `PATH` walk.
#[derive(Debug, Clone, Default)]
struct AndroidTools {
    /// `bundletool` on `PATH` (Google's jar wrapper script).
    bundletool: bool,
    /// `java` on `PATH` — `bundletool` and `gradlew` are Java programs.
    java: bool,
    /// `zip` on `PATH` — the bundletool route and the multi-ABI merge pack
    /// with it.
    zip: bool,
    /// `unzip` on `PATH` — the same routes unpack with it.
    unzip: bool,
    /// The project carries its own Gradle wrapper (`gradlew` / `gradlew.bat`).
    gradlew: bool,
    /// `gradle` on `PATH`, for a project with no wrapper of its own.
    gradle: bool,
    /// `cargo apk` runs — decided by the caller ([`have_cargo_apk`]), since it
    /// is a cargo subcommand rather than a binary a `PATH` names.
    cargo_apk: bool,
}

/// The wrapper's file name on this host — `gradlew.bat` on Windows, where a
/// bare `gradlew` shell script would not run.
fn wrapper_name() -> &'static str {
    if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    }
}

impl AndroidTools {
    /// Detect every tool on the `PATH` directories given, and the wrapper in
    /// `app_dir`. Detection only: every *which route* decision is
    /// [`aab_plan`]'s, so a fake `PATH` tests one and fake booleans the other.
    fn detect(dirs: &[PathBuf], app_dir: &Path, cargo_apk: bool) -> Self {
        let on = |tool: &str| tool_in_dirs(tool, dirs.iter().cloned());
        Self {
            bundletool: on("bundletool"),
            java: on("java"),
            zip: on("zip"),
            unzip: on("unzip"),
            gradlew: app_dir.join(wrapper_name()).is_file(),
            gradle: on("gradle"),
            cargo_apk,
        }
    }

    /// Everything the bundletool route lacks, each with its install command.
    fn missing_bundletool(&self) -> Vec<ToolRefusal> {
        let mut missing = Vec::new();
        if !self.cargo_apk {
            missing.push(ToolRefusal {
                tool: "cargo apk",
                how: "cargo install cargo-apk (the route's APK half)",
            });
        }
        if !self.bundletool {
            missing.push(ToolRefusal {
                tool: "bundletool",
                how: "download bundletool.jar from \
                      https://github.com/google/bundletool/releases and put a `bundletool` \
                      wrapper script on PATH",
            });
        }
        if !self.java {
            missing.push(ToolRefusal {
                tool: "java",
                how: "install a JDK (apt install openjdk-17-jdk / brew install openjdk@17) — \
                      bundletool is a jar",
            });
        }
        if !self.zip {
            missing.push(ToolRefusal {
                tool: "zip",
                how: "apt install zip / brew install zip",
            });
        }
        if !self.unzip {
            missing.push(ToolRefusal {
                tool: "unzip",
                how: "apt install unzip / brew install unzip",
            });
        }
        missing
    }

    /// Everything the gradle route lacks. `java` is on both routes' lists,
    /// because a Gradle wrapper is a Java program too; the merge in
    /// [`aab_plan`] keeps it named once.
    fn missing_gradle(&self) -> Vec<ToolRefusal> {
        let mut missing = Vec::new();
        if !self.java {
            missing.push(ToolRefusal {
                tool: "java",
                how: "install a JDK (apt install openjdk-17-jdk / brew install openjdk@17) — \
                      gradlew is a Java program",
            });
        }
        if !self.gradlew && !self.gradle {
            missing.push(ToolRefusal {
                tool: "gradle",
                how: "brew install gradle / sdkman.io, or add a wrapper to the project \
                      (gradle wrapper) — gradlew bundleRelease builds the bundle the project's \
                      own way",
            });
        }
        missing
    }
}

/// What [`package_aab`] will run, decided before anything is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AabRoute {
    /// `bundleRelease` through the project's own wrapper, or a host `gradle`
    /// when the project has no wrapper (`program` says which).
    Gradle { program: PathBuf },
    /// `cargo apk` → `unzip` → `zip` → `bundletool build-bundle`, with every
    /// intermediate living in `target/package`.
    Bundletool {
        module_dir: PathBuf,
        module_zip: PathBuf,
        config: PathBuf,
        bundle: PathBuf,
    },
}

/// Decide the route from the tools alone. The project's own Gradle wrapper
/// wins when it can run — a project that says how it is built is not
/// second-guessed — then `bundletool`, then a host `gradle`. When no route is
/// possible, every missing tool from *both* routes is returned with its
/// install command, named once each.
fn aab_plan(
    tools: &AndroidTools,
    app_dir: &Path,
    name: &str,
) -> Result<AabRoute, Vec<ToolRefusal>> {
    if tools.gradlew {
        if tools.java {
            return Ok(AabRoute::Gradle {
                program: app_dir.join(wrapper_name()),
            });
        }
        // The wrapper is there but cannot run; refusing with just this is
        // more honest than falling through to a route that ignores the
        // project's own build.
        return Err(vec![ToolRefusal {
            tool: "java",
            how: "install a JDK (apt install openjdk-17-jdk / brew install openjdk@17) — \
                  the project's gradlew cannot run without one",
        }]);
    }

    let bundletool_missing = tools.missing_bundletool();
    if bundletool_missing.is_empty() {
        let package_dir = app_dir.join("target/package");
        return Ok(AabRoute::Bundletool {
            module_dir: package_dir.join("aab-module"),
            module_zip: package_dir.join("aab-base.zip"),
            config: package_dir.join("BundleConfig.json"),
            bundle: package_dir.join(format!("{name}.aab")),
        });
    }

    if tools.missing_gradle().is_empty() {
        return Ok(AabRoute::Gradle {
            program: PathBuf::from("gradle"),
        });
    }

    // Neither route is possible: everything either one lacks, each tool
    // named once (java is on both lists above).
    let mut missing = bundletool_missing;
    for entry in tools.missing_gradle() {
        if !missing.iter().any(|seen| seen.tool == entry.tool) {
            missing.push(entry);
        }
    }
    Err(missing)
}

/// The `BundleConfig.json` the bundletool route writes: nothing but the
/// compression policy — native libraries stay uncompressed, so Play's split
/// delivery can mmap them straight out of the installed module instead of
/// decompressing at install time (the same trade `cargo apk`'s own APKs
/// make). Hand-written rather than a dependency on a JSON writer; it is one
/// object with one array with one glob.
fn bundle_config() -> String {
    "{\"compression\":{\"uncompressedGlob\":[\"lib/**/*.so\"]}}\n".to_owned()
}

/// One command a route runs, as argv plus a working directory — the whole
/// plan in a form a test can read without a single tool being real.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolStep {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
}

impl ToolStep {
    /// Run it for real, capturing the failure's stderr verbatim. The build
    /// step's own message shape is kept: `cargo apk` failing here is a build
    /// failure exactly like `cargo build --release` failing in
    /// [`cargo_build_release`].
    fn run(&self) -> Result<String, PackageError> {
        let output = Command::new(&self.program)
            .args(&self.args)
            .current_dir(&self.cwd)
            .output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(PackageError::Build(format!(
                "{} {} failed:\n{}",
                self.program,
                self.args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }
}

/// The commands each route runs *after* its build step — the build itself
/// (cargo apk / gradle bundleRelease) is [`package_aab`]'s to schedule,
/// because the bundletool route needs to discover the APK it just built
/// before it can name the commands that consume it.
///
/// `apk` is the built APK's path, required only by the bundletool route.
fn aab_steps(route: &AabRoute, app_dir: &Path, apk: Option<&Path>) -> Vec<ToolStep> {
    match route {
        AabRoute::Gradle { program } => vec![ToolStep {
            program: program.to_string_lossy().into_owned(),
            args: vec!["bundleRelease".to_owned()],
            cwd: app_dir.to_owned(),
        }],
        AabRoute::Bundletool {
            module_dir,
            module_zip,
            config,
            bundle,
        } => {
            let apk = apk.expect("the bundletool route runs after the APK was built");
            let arg = |path: &Path| path.to_string_lossy().into_owned();
            vec![
                // Unpack the APK as a base module, minus the APK's own
                // signatures: a signed module is refused by bundletool, and
                // Play re-signs every split anyway.
                ToolStep {
                    program: "unzip".to_owned(),
                    args: vec![
                        "-o".to_owned(),
                        arg(apk),
                        "-d".to_owned(),
                        arg(module_dir),
                        "-x".to_owned(),
                        "META-INF/*".to_owned(),
                    ],
                    cwd: app_dir.to_owned(),
                },
                ToolStep {
                    program: "zip".to_owned(),
                    args: vec![
                        "-X".to_owned(),
                        "-r".to_owned(),
                        arg(module_zip),
                        ".".to_owned(),
                    ],
                    cwd: module_dir.clone(),
                },
                ToolStep {
                    program: "bundletool".to_owned(),
                    args: vec![
                        "build-bundle".to_owned(),
                        "--modules".to_owned(),
                        arg(module_zip),
                        "--output".to_owned(),
                        arg(bundle),
                        "--config".to_owned(),
                        arg(config),
                    ],
                    cwd: app_dir.to_owned(),
                },
            ]
        }
    }
}

/// Find the first file with `extension` under `dir`, depth-limited — how the
/// bundletool route finds the APK `cargo apk` just wrote (`target/release/apk`,
/// a small directory) and the gradle route finds the bundle Gradle just wrote
/// (`app/build/outputs/bundle/release` for the standard module name).
///
/// Directories named in `avoid` are not entered: the gradle fallback searches
/// the whole project, and a stale bundle from an earlier `target/package` run
/// must not win over the one Gradle just wrote. Symlinked directories are not
/// followed either — `DirEntry::file_type` is `lstat` semantics, and a
/// packaging walk that wanders out of the project through a link is a bug,
/// not a feature to support.
fn find_artifact(dir: &Path, extension: &str, avoid: &[&str], depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().ok().is_some_and(|t| t.is_dir()) {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| avoid.contains(&name))
            {
                continue;
            }
            subdirs.push(path);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            return Some(path);
        }
    }
    subdirs
        .into_iter()
        .find_map(|sub| find_artifact(&sub, extension, avoid, depth - 1))
}

/// Build an Android App Bundle (`.aab`) — see the section docs above for
/// which route and why. Refuses before running anything when no route is
/// possible, naming every missing tool with its install command; a refusal
/// leaves nothing behind.
fn package_aab(app_dir: &Path) -> Result<PackageReport, PackageError> {
    let name = read_package_name(app_dir)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let tools = AndroidTools::detect(&dirs, app_dir, have_cargo_apk());
    let route = aab_plan(&tools, app_dir, &name).map_err(|missing| {
        let joined = missing
            .iter()
            .map(|entry| entry.line())
            .collect::<Vec<_>>()
            .join("; ");
        PackageError::MissingTool {
            target: PackageTarget::Aab,
            tool: "gradlew/gradle bundleRelease or bundletool (+ cargo apk, java, zip, unzip)"
                .to_owned(),
            how_to_get_it: joined,
        }
    })?;

    match &route {
        AabRoute::Gradle { program } => {
            ToolStep {
                program: program.to_string_lossy().into_owned(),
                args: vec!["bundleRelease".to_owned()],
                cwd: app_dir.to_owned(),
            }
            .run()?;
            // Gradle writes <module>/build/outputs/bundle/release/<…>.aab for
            // whatever modules the project declares, so the standard location
            // is searched first and the whole project second — bounded, and
            // never inside `target`, where a stale earlier bundle would win.
            let produced = find_artifact(
                &app_dir.join("app/build/outputs/bundle/release"),
                "aab",
                &[],
                2,
            )
            .or_else(|| find_artifact(app_dir, "aab", &["target", ".git"], 5));
            let Some(produced) = produced else {
                return Err(PackageError::MissingArtifact(
                    app_dir
                        .join("app/build/outputs/bundle/release")
                        .join("<module>-release.aab"),
                ));
            };
            let out_dir = app_dir.join("target/package");
            std::fs::create_dir_all(&out_dir)?;
            let bundle = out_dir.join(format!("{name}.aab"));
            std::fs::copy(&produced, &bundle)?;
            Ok(PackageReport {
                target: PackageTarget::Aab,
                output_dir: out_dir,
                artifacts: vec![bundle],
                notes: vec![
                    format!("built by {} bundleRelease from {}", program.display(), produced.display()),
                    "sign the upload key with packaging/sign.sh apk <file.aab> (jarsigner) when the \
                     ANDROID_KEYSTORE secrets exist; Play App Signing holds the delivery key — \
                     the upload key is the only certificate this pipeline needs"
                        .to_owned(),
                ],
            })
        }
        AabRoute::Bundletool {
            module_dir,
            // The zip is reached through `route` in `aab_steps` below; binding
            // it here too would only be a second name for the same path.
            module_zip: _,
            config,
            bundle,
        } => {
            ToolStep {
                program: "cargo".to_owned(),
                args: vec!["apk".to_owned(), "build".to_owned(), "--release".to_owned()],
                cwd: app_dir.to_owned(),
            }
            .run()?;
            let apk_dir = app_dir.join("target/release/apk");
            let Some(apk) = find_artifact(&apk_dir, "apk", &[], 3) else {
                return Err(PackageError::MissingArtifact(
                    apk_dir.join("<apk_name>.apk"),
                ));
            };

            std::fs::create_dir_all(module_dir)?;
            std::fs::write(config, bundle_config())?;
            // The intermediates are this run's own scratch: a failed run
            // leaves them for a person to read, a successful one cleans up
            // the unpacked module (the zip and the config stay — they are the
            // bundle's provenance, the same honesty MANIFEST.txt practices).
            for step in aab_steps(&route, app_dir, Some(&apk)) {
                step.run()?;
            }
            std::fs::remove_dir_all(module_dir)?;

            if !bundle.is_file() {
                return Err(PackageError::MissingArtifact(bundle.clone()));
            }
            Ok(PackageReport {
                target: PackageTarget::Aab,
                output_dir: bundle
                    .parent()
                    .map(Path::to_owned)
                    .unwrap_or_else(|| app_dir.join("target/package")),
                artifacts: vec![bundle.clone()],
                notes: vec![
                    format!(
                        "base module assembled from {} by unzip/zip (the APK's signatures \
                         excluded) and packed by bundletool build-bundle",
                        apk.display()
                    ),
                    "sign the upload key with packaging/sign.sh apk <file.aab> (jarsigner) when the \
                     ANDROID_KEYSTORE secrets exist; Play App Signing holds the delivery key — \
                     the upload key is the only certificate this pipeline needs"
                        .to_owned(),
                ],
            })
        }
    }
}

// ── the multi-ABI APK (FW-19) ────────────────────────────────────────────────
//
// B19: the APK `package_android` hands over is whatever `cargo apk` builds for
// its default target — arm64 — and an arm64-only APK refuses to install on the
// x86_64 emulator image every Android CI leg runs (`ci/mobile/apk.sh` works
// around that per-invocation by passing `--target` after asking `adb` which ABI
// the attached device has). A multi-ABI APK is one file that installs on both:
// `lib/arm64-v8a/` and `lib/x86_64/` side by side, and Android's installer
// picks the directory matching the device it is running on.
//
// The merge is deliberately built from the same primitives the AAB's
// bundletool route already proved: `cargo apk build --target <triple>` per ABI
// (the same invocation `ci/mobile/apk.sh` uses, so the per-target output
// directory layout below is the one that script already walks), `unzip` to lay
// both APKs into one staging tree, `zip` to pack it, and the SDK's own
// `zipalign` to place the entries. Nothing here reimplements any part of the
// zip format — `zip`/`unzip`/`zipalign` are the tools, exactly as the AAB
// route's own doc says about `bundletool`.
//
// What a merge means for signatures is stated rather than hidden: rebuilding
// the zip invalidates the v2 signature that covers the whole file, and the
// v1 `META-INF` files are excluded on unpack, so the merged APK is **unsigned**
// until `apksigner` re-signs it — the same posture the AAB route takes toward
// its upload key, with `packaging/sign.sh apk` named as the signer.

/// One CPU the merged APK carries a `lib/<dir>/` directory for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ApkAbi {
    /// The Rust triple `cargo apk build --target` takes.
    triple: &'static str,
    /// The Android ABI name, which is literally the directory name under
    /// `lib/` in the APK — the loader looks for its own ABI's directory and
    /// refuses to install when there is none.
    dir: &'static str,
}

/// Every ABI a merged APK carries (FW-19): the phone ABI and the emulator ABI.
///
/// `arm64-v8a` because that is every shipping Android phone, and `x86_64`
/// because that is the emulator image CI installs — the two-machine reality
/// B19 named. Adding a third (say `armeabi-v7a` or `riscv64`) is a one-line
/// change here plus a test, and the merge code below does not change.
const MULTI_ABI: &[ApkAbi] = &[
    ApkAbi {
        triple: "aarch64-linux-android",
        dir: "arm64-v8a",
    },
    ApkAbi {
        triple: "x86_64-linux-android",
        dir: "x86_64",
    },
];

/// Everything the multi-ABI merge needs beyond [`package_android`]'s own gate,
/// each absence one refusal line with its fix — the same composition
/// [`aab_plan`] practices, so a missing SDK, `cargo apk`, `zip`, `unzip` or
/// `zipalign` is named together with how to install it.
fn multi_abi_missing(
    android_home: Option<&str>,
    tools: &AndroidTools,
    zipalign: Option<&Path>,
) -> Vec<ToolRefusal> {
    let mut missing = Vec::new();
    if android_home.map(str::trim).unwrap_or_default().is_empty() {
        missing.push(ToolRefusal {
            tool: "ANDROID_HOME",
            how: "set ANDROID_HOME (or ANDROID_SDK_ROOT) to an installed Android SDK",
        });
    }
    if !tools.cargo_apk {
        missing.push(ToolRefusal {
            tool: "cargo apk",
            how: "cargo install cargo-apk (one `cargo apk build --target` per ABI)",
        });
    }
    if !tools.zip {
        missing.push(ToolRefusal {
            tool: "zip",
            how: "apt install zip / brew install zip",
        });
    }
    if !tools.unzip {
        missing.push(ToolRefusal {
            tool: "unzip",
            how: "apt install unzip / brew install unzip",
        });
    }
    if zipalign.is_none() {
        // Named separately from the SDK itself: a machine can have ANDROID_HOME
        // pointing at a platform-tools-only install with no build-tools, and
        // the fix for that is not "set ANDROID_HOME" but one sdkmanager command.
        missing.push(ToolRefusal {
            tool: "zipalign",
            how: "install Build-Tools into the SDK: sdkmanager \"build-tools;34.0.0\" — the \
                  same ANDROID_HOME/build-tools/<version>/ directory cargo apk takes aapt2 \
                  from",
        });
    }
    missing
}

/// The SDK's `zipalign`, from `$ANDROID_HOME/build-tools/<newest>/` — the same
/// directory `cargo apk` takes `aapt2` from, so a machine that can build an
/// APK at all has it. The newest version wins when several are installed,
/// because that is the one the SDK's own tools updated last.
///
/// Pure filesystem probing of the given SDK root, so a test installs a fake
/// `build-tools/` tree with a fake `zipalign` in it and watches the pick.
fn zipalign_in(android_home: &Path) -> Option<PathBuf> {
    let versions = std::fs::read_dir(android_home.join("build-tools")).ok()?;
    let mut newest: Option<(String, PathBuf)> = None;
    for entry in versions.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        for exe in ["zipalign", "zipalign.exe"] {
            let program = entry.path().join(exe);
            if !program.is_file() {
                continue;
            }
            // Lexical order on the version directory name: build-tools version
            // numbers are dotted numerals (34.0.0, 35.0.0), which sort that way
            // for every pair a real SDK installs.
            if newest.as_ref().is_none_or(|(seen, _)| *seen < name) {
                newest = Some((name.clone(), program));
            }
        }
    }
    newest.map(|(_, program)| program)
}

/// The merge commands: unpack each per-ABI APK into one staging tree, then pack
/// the tree into the still-unaligned APK.
///
/// The unpack excludes `META-INF/*` for the reason the section docs give: a
/// merged zip invalidates the v2 signature outright and leaves v1's files
/// stale, and `apksigner` refuses to re-sign over files it did not write. The
/// pack is `-0` — *store*, no deflate — because `zipalign -p` can only align
/// entries that are stored, and the entries that have to be uncompressed for
/// the install to work (the native libraries, `resources.arsc`) are exactly the
/// ones `cargo apk` stored uncompressed in the per-ABI APKs being merged.
fn multi_abi_steps(apks: &[(ApkAbi, PathBuf)], staging: &Path, unaligned: &Path) -> Vec<ToolStep> {
    let arg = |path: &Path| path.to_string_lossy().into_owned();
    let mut steps = Vec::with_capacity(apks.len() + 1);
    for (_, apk) in apks {
        // `-o` overwrite: every per-ABI APK carries the same manifest and
        // resources (they were built from one project), so the second unpack's
        // copies land on the first's — the only per-ABI difference is the
        // `lib/<abi>/` directory, which never collides.
        steps.push(ToolStep {
            program: "unzip".to_owned(),
            args: vec![
                "-o".to_owned(),
                arg(apk),
                "-d".to_owned(),
                arg(staging),
                "-x".to_owned(),
                "META-INF/*".to_owned(),
            ],
            cwd: staging.to_owned(),
        });
    }
    steps.push(ToolStep {
        program: "zip".to_owned(),
        args: vec![
            "-X".to_owned(),
            "-r".to_owned(),
            "-0".to_owned(),
            arg(unaligned),
            ".".to_owned(),
        ],
        cwd: staging.to_owned(),
    });
    steps
}

/// The merged APK's whole point, checked before anything is signed or shipped:
/// every ABI it claims to carry has a `lib/<abi>/` directory holding at least
/// one `.so`.
///
/// Pure `std::fs` over the staging tree, so it is testable with directories
/// this suite writes itself — no unzip, no SDK, no APK. A failure here is not
/// a merge bug: it means `cargo apk build --target <triple>` produced an APK
/// without a `lib/<abi>/` directory for its own triple, which is a build gone
/// wrong rather than a packaging step gone wrong, and the message says which
/// directory was expected so the next look is at the build, not the merge.
fn verify_lib_layout(staging: &Path, abis: &[ApkAbi]) -> Result<(), String> {
    let mut missing = Vec::new();
    for abi in abis {
        let dir = staging.join("lib").join(abi.dir);
        let holds_a_library = std::fs::read_dir(&dir).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "so"))
        });
        if !holds_a_library {
            missing.push(format!(
                "lib/{} has no .so — did `cargo apk build --target {}` build what it said?",
                abi.dir, abi.triple,
            ));
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing.join("; "))
    }
}

/// Build one APK carrying every ABI in [`MULTI_ABI`] — per-target
/// `cargo apk` builds, merged, aligned, and left for `apksigner` to sign.
///
/// Refuses before running anything when the SDK, `cargo apk`, `zip`, `unzip`
/// or `zipalign` is missing, naming each with its install command; a refusal
/// leaves nothing behind. The merge intermediates live under
/// `target/package/` and the staging tree is removed on success only — a
/// failed run leaves its evidence for a person to read, the same choice
/// [`package_aab`] makes for its own unpacked module.
fn package_android_multi_abi(app_dir: &Path) -> Result<PackageReport, PackageError> {
    let name = read_package_name(app_dir)?;
    let android_home = std::env::var("ANDROID_HOME")
        .ok()
        .or_else(|| std::env::var("ANDROID_SDK_ROOT").ok());
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let tools = AndroidTools::detect(&dirs, app_dir, have_cargo_apk());
    let sdk_root = android_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let zipalign = sdk_root.as_deref().and_then(zipalign_in);

    let missing = multi_abi_missing(android_home.as_deref(), &tools, zipalign.as_deref());
    if !missing.is_empty() {
        let joined = missing
            .iter()
            .map(|entry| entry.line())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(PackageError::MissingTool {
            target: PackageTarget::AndroidMultiAbi,
            tool: "cargo apk / Android SDK (build-tools zipalign) / zip / unzip".to_owned(),
            how_to_get_it: joined,
        });
    }
    let zipalign = zipalign.expect("the refusal above covers its absence");

    // One build per ABI, then the APK that build wrote. `--target` puts the
    // artifacts under target/<triple>/release/, which is where the search
    // looks — the same layout `ci/mobile/apk.sh` walks with `find`.
    let mut apks = Vec::new();
    for abi in MULTI_ABI {
        ToolStep {
            program: "cargo".to_owned(),
            args: vec![
                "apk".to_owned(),
                "build".to_owned(),
                "--release".to_owned(),
                "--target".to_owned(),
                abi.triple.to_owned(),
            ],
            cwd: app_dir.to_owned(),
        }
        .run()?;
        let apk_dir = app_dir.join("target").join(abi.triple).join("release/apk");
        let Some(apk) = find_artifact(&apk_dir, "apk", &[], 3) else {
            return Err(PackageError::MissingArtifact(
                apk_dir.join("<apk_name>.apk"),
            ));
        };
        apks.push((*abi, apk));
    }

    let package_dir = app_dir.join("target/package");
    let staging = package_dir.join("apk-multi-abi");
    let unaligned = package_dir.join(format!("{name}-unaligned.apk"));
    let merged = package_dir.join(format!("{name}-multi-abi.apk"));
    std::fs::create_dir_all(&staging)?;

    for step in multi_abi_steps(&apks, &staging, &unaligned) {
        step.run()?;
    }
    verify_lib_layout(&staging, MULTI_ABI).map_err(PackageError::Build)?;

    // `-p` page-aligns the stored `.so` files to 4096 — without it an
    // `extractNativeLibs="false"` install cannot mmap the library, which is
    // the exact failure the flag exists for. `4` aligns every other stored
    // entry; `-f` overwrites a previous run's output.
    ToolStep {
        program: zipalign.to_string_lossy().into_owned(),
        args: vec![
            "-p".to_owned(),
            "-f".to_owned(),
            "4".to_owned(),
            unaligned.to_string_lossy().into_owned(),
            merged.to_string_lossy().into_owned(),
        ],
        cwd: app_dir.to_owned(),
    }
    .run()?;
    // The unaligned intermediate is this run's own scratch, not provenance:
    // the merged APK is the artifact, and the per-ABI APKs it came from are
    // still under target/<triple>/release/apk for anyone who wants them.
    std::fs::remove_dir_all(&staging)?;
    std::fs::remove_file(&unaligned).ok();

    if !merged.is_file() {
        return Err(PackageError::MissingArtifact(merged));
    }
    let layout = MULTI_ABI
        .iter()
        .map(|abi| format!("lib/{}", abi.dir))
        .collect::<Vec<_>>()
        .join(" + ");
    Ok(PackageReport {
        target: PackageTarget::AndroidMultiAbi,
        output_dir: package_dir,
        artifacts: vec![merged],
        notes: vec![
            format!(
                "{layout} merged from one cargo apk build per target, stored uncompressed \
                     and page-aligned by zipalign -p — installs on an arm64 phone and an \
                     x86_64 emulator image alike (the B19 gap)"
            ),
            "unsigned: the merge rewrote the zip, so the per-ABI signatures did not survive it — \
             re-sign with apksigner (packaging/sign.sh apk) when the ANDROID_KEYSTORE secrets \
             exist"
                .to_owned(),
        ],
    })
}

/// Build and bundle for iOS via `xcrun`, mirroring `ci/mobile/ios-app.sh`'s own
/// simulator build.
///
/// Reached only once [`check_host`] has confirmed a macOS host; the `xcrun`
/// check below is the second half of `ci/mobile/ios-app.sh`'s own opening gate
/// (`command -v xcrun`), kept here too because a macOS host with no Xcode
/// command line tools installed is a real, distinct machine from one with
/// them. Not exercised on this Linux sandbox — `check_host` refuses first.
fn package_ios(app_dir: &Path) -> Result<PackageReport, PackageError> {
    if !tool_on_path("xcrun") {
        return Err(PackageError::MissingTool {
            target: PackageTarget::Ios,
            tool: "xcrun".to_owned(),
            how_to_get_it: "install the Xcode command line tools".to_owned(),
        });
    }

    let name = read_package_name(app_dir)?;
    let output = Command::new("cargo")
        .args(["build", "--release", "--target", "aarch64-apple-ios-sim"])
        .current_dir(app_dir)
        .output()?;
    if !output.status.success() {
        return Err(PackageError::Build(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    let binary = app_dir
        .join("target/aarch64-apple-ios-sim/release")
        .join(&name);
    if !is_probably_binary(&binary) {
        return Err(PackageError::MissingArtifact(binary));
    }

    let bundle = app_dir.join("target/package").join(format!("{name}.app"));
    std::fs::create_dir_all(&bundle)?;
    let bin_dest = bundle.join(&name);
    std::fs::copy(&binary, &bin_dest)?;
    let plist_dest = bundle.join("Info.plist");
    std::fs::write(
        &plist_dest,
        info_plist(&name, wants_background_audio(app_dir)),
    )?;

    Ok(PackageReport {
        target: PackageTarget::Ios,
        output_dir: bundle,
        artifacts: vec![bin_dest, plist_dest],
        notes: vec![
            "built for the simulator triple; ci/mobile/ios-app.sh's --device and --ipa steps additionally \
             need a signing identity this crate does not have"
                .to_owned(),
        ],
    })
}

// ── the update channel (FW-24) ──────────────────────────────────────────────
//
// `package` ends a build; the update channel starts where it ends. The
// integration point between the two crates is one function long:
// [`update_manifest`] writes the unsigned half of a `vieww.update/1`
// manifest for a list of artifact files, and everything after that —
// signing it, pinning the key, checking it, staging, applying — is
// `vieww-update`'s, deliberately: the packager knows where the bytes are
// and nothing about trust, and the updater knows trust and nothing about
// where a build put its bytes.

/// Why [`update_manifest`] could not write one.
///
/// Each variant names the fix rather than the disappointment — the same
/// rule [`PackageError`] follows.
#[derive(Debug)]
pub enum UpdateManifestError {
    /// The version is not `major.minor.patch[-pre]` — an app on an update
    /// channel has to carry a version the updater can compare, and a
    /// version the comparer cannot parse is a release that would never
    /// install over itself.
    Version(String),
    /// An "artifact" that is a directory. The updater downloads and
    /// applies *files*; a directory is a tree an archive should carry.
    Directory(PathBuf),
    /// The artifact list is empty. A manifest with no assets applies
    /// nothing — `vieww-update` refuses to parse one, and this refuses to
    /// write one, so the mistake is caught at the place that made it.
    Empty,
    /// Two artifacts with the same file name. The manifest names assets by
    /// file name alone (a path in a name is how an updater is made to
    /// write outside the install directory — `vieww-update` refuses one),
    /// so two of the same name would have the second silently overwrite
    /// the first at apply time.
    DuplicateName(String),
    /// Reading or hashing an artifact failed.
    Io(std::io::Error),
}

impl fmt::Display for UpdateManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(version) => write!(
                f,
                "version '{version}' is not major.minor.patch[-pre] — an app on the update \
                 channel needs one the updater can compare"
            ),
            Self::Directory(path) => write!(
                f,
                "{} is a directory, and an update asset is a file — wrap the tree in an archive \
                 (tar.gz / zip) the app's own installer understands and pass that instead",
                path.display()
            ),
            Self::Empty => write!(
                f,
                "no artifacts — a manifest with no assets applies nothing, and writing one \
                 would only defer the refusal to the updater that parses it"
            ),
            Self::DuplicateName(name) => write!(
                f,
                "two artifacts are both named '{name}' — the manifest names assets by file name \
                 alone, so the second would silently overwrite the first when the update applied"
            ),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for UpdateManifestError {}

impl From<std::io::Error> for UpdateManifestError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The unsigned half of an update manifest for `artifacts` — the FW-24
/// integration point between this crate and `vieww-update`.
///
/// Every artifact is hashed, and the manifest that comes back carries its
/// SHA-256 and a *zeroed* `ed25519_sig` — unsigned by construction, because
/// the release seed is the one secret in the pipeline and it must never be
/// on the machine that builds (or in the repository that builds from). The
/// publisher's half is two lines against the returned value:
///
/// ```no_run
/// # use vieww_build::package::{update_manifest, PackageReport};
/// # let report: PackageReport = unimplemented!();
/// let unsigned = update_manifest(
///     &report.artifacts,
///     "1.4.0",
///     "http://updates.example.test/1.4.0",
///     "Fixes the caret blink on Wayland.",
/// )
/// .expect("the artifacts are files with distinct names");
///
/// // The publisher, on the machine that holds the release seed:
/// # let release_seed = [0u8; 32];
/// let signed = unsigned.sign_for_release(&release_seed);
/// # let _ = signed;
/// // ...and the app pins `vieww_update::ed25519::public_key(&release_seed)`
/// // at build time; that pair is the whole trust root.
/// ```
///
/// # Which artifacts belong here
///
/// The updater applies every asset by its *plain file name* into one
/// install directory — a path in an asset name is refused by
/// `vieww-update` as a matter of safety, not tolerated as a layout. A
/// [`PackageReport`] whose artifacts are a tree (the Linux target's
/// `bin/` + `share/applications/`) is therefore a set of files that should
/// travel as *one* archive asset the app expands itself: pass the archive
/// to this function, not the tree. Single-file targets (`.msi`, `.apk`,
/// `.ipa`, a `.tar.gz` you made from a tree) are exactly the shape this
/// exists for.
///
/// # Errors
///
/// [`UpdateManifestError`]: an unparsable [`version`](UpdateManifestError::Version),
/// a directory among the artifacts ([`Directory`](UpdateManifestError::Directory)),
/// two artifacts with the same file name
/// ([`DuplicateName`](UpdateManifestError::DuplicateName)),
/// or a read failure ([`Io`](UpdateManifestError::Io)).
pub fn update_manifest(
    artifacts: &[PathBuf],
    version: &str,
    base_url: &str,
    notes: &str,
) -> Result<vieww_update::Manifest, UpdateManifestError> {
    let version = vieww_update::Version::parse(version)
        .ok_or_else(|| UpdateManifestError::Version(version.to_owned()))?;
    if artifacts.is_empty() {
        return Err(UpdateManifestError::Empty);
    }

    let base = base_url.trim_end_matches('/');
    let mut assets = Vec::with_capacity(artifacts.len());
    let mut seen: Vec<String> = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        // A directory here is a tree, and the refusal (rather than a
        // recursive walk) is the honest move: which files inside a tree an
        // app can replace at runtime is the app's own installer contract,
        // not this function's guess.
        if artifact.is_dir() {
            return Err(UpdateManifestError::Directory(artifact.clone()));
        }
        let name = artifact
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                UpdateManifestError::Io(std::io::Error::other(format!(
                    "{} has a file name that is not UTF-8 — the manifest names assets by file name",
                    artifact.display()
                )))
            })?;
        if seen.iter().any(|s| s == name) {
            return Err(UpdateManifestError::DuplicateName(name.to_owned()));
        }
        seen.push(name.to_owned());
        let bytes = std::fs::read(artifact)?;
        assets.push(vieww_update::Asset {
            name: name.to_owned(),
            url: format!("{base}/{name}"),
            sha256: vieww_update::digest(&bytes),
            // Zeroed, and the manifest is unsigned until the publisher's
            // `sign_for_release`: see the function docs for why the seed
            // must not be on the build machine.
            ed25519_sig: [0u8; 64],
        });
    }

    Ok(vieww_update::Manifest {
        version,
        notes: notes.to_owned(),
        assets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- the update channel (FW-24) --------------------------------------

    /// The whole release path from this crate's side of it: hash real
    /// files, round-trip through the parser `vieww-update` itself uses
    /// (proving the writer and the reader are one implementation, not two
    /// shapes), then sign with an ephemeral seed and verify under the key
    /// that seed derives — the exact pair a publisher and a pinned app
    /// hold.
    #[test]
    fn an_update_manifest_hashes_real_files_and_signs_for_release() {
        let dir = std::env::temp_dir().join(format!(
            "vieww-build-update-manifest-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.msi"), b"an installer's bytes").unwrap();
        std::fs::write(dir.join("notes.txt"), b"shipping notes").unwrap();

        let unsigned = update_manifest(
            &[dir.join("app.msi"), dir.join("notes.txt")],
            "1.4.0",
            "http://updates.example.test/1.4.0/",
            "Fixes the caret blink on Wayland.",
        )
        .expect("two files with distinct names hash into a manifest");

        assert_eq!(
            unsigned.version,
            vieww_update::Version::parse("1.4.0").unwrap()
        );
        assert_eq!(unsigned.notes, "Fixes the caret blink on Wayland.");
        assert_eq!(unsigned.assets.len(), 2);
        assert_eq!(unsigned.assets[0].name, "app.msi");
        // `base_url`'s trailing slash is trimmed: the URL is the base the
        // publisher serves from plus the asset's name, exactly once.
        assert_eq!(
            unsigned.assets[0].url,
            "http://updates.example.test/1.4.0/app.msi"
        );
        assert_eq!(
            unsigned.assets[0].sha256,
            vieww_update::digest(b"an installer's bytes")
        );
        assert_eq!(
            unsigned.assets[1].sha256,
            vieww_update::digest(b"shipping notes")
        );
        assert!(
            unsigned.assets.iter().all(|a| a.ed25519_sig == [0u8; 64]),
            "unsigned by construction: the release seed signs later, off the build machine"
        );

        // The parser this crate has no reason to distrust: the manifest's
        // own JSON is what a feed serves, and what comes back is equal.
        let back = vieww_update::parse_manifest(&unsigned.to_json()).expect("round trips");
        assert_eq!(back, unsigned);

        // And the publisher's half: sign, then verify under the derived key.
        let seed = [0x5au8; 32];
        let signed = unsigned.sign_for_release(&seed);
        assert!(signed.verify(&vieww_update::public_key(&seed)).is_ok());
        assert!(signed
            .verify(&vieww_update::public_key(&[1u8; 32]))
            .is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_artifact_is_refused_by_name() {
        let dir = std::env::temp_dir().join("vieww-build-update-manifest-dir");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("a-tree")).unwrap();
        std::fs::write(dir.join("a-tree/inside.txt"), b"not an asset").unwrap();

        let error = update_manifest(&[dir.join("a-tree")], "1.0.0", "http://u/", "").unwrap_err();
        assert!(
            matches!(error, UpdateManifestError::Directory(_)),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("archive"),
            "the refusal names the fix: {error}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_artifacts_with_the_same_file_name_are_refused() {
        let dir = std::env::temp_dir().join("vieww-build-update-manifest-dupes");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("one")).unwrap();
        std::fs::create_dir_all(dir.join("two")).unwrap();
        std::fs::write(dir.join("one/app.bin"), b"first").unwrap();
        std::fs::write(dir.join("two/app.bin"), b"second").unwrap();

        let error = update_manifest(
            &[dir.join("one/app.bin"), dir.join("two/app.bin")],
            "1.0.0",
            "http://u/",
            "",
        )
        .unwrap_err();
        assert!(
            matches!(&error, UpdateManifestError::DuplicateName(name) if name == "app.bin"),
            "{error:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_version_the_updater_cannot_compare_is_refused() {
        let error = update_manifest(&[], "1.x", "http://u/", "").unwrap_err();
        assert!(
            matches!(error, UpdateManifestError::Version(_)),
            "{error:?}"
        );
        assert!(error.to_string().contains("1.x"), "{error}");
    }

    #[test]
    fn an_empty_artifact_list_writes_nothing() {
        let error = update_manifest(&[], "1.0.0", "http://u/", "").unwrap_err();
        assert!(matches!(error, UpdateManifestError::Empty), "{error:?}");
        assert!(error.to_string().contains("applies nothing"), "{error}");
    }

    // ----- background audio (AU-04) -----------------------------------------

    #[test]
    fn the_background_audio_key_is_written_only_when_asked() {
        assert!(!info_plist("demo", false).contains("UIBackgroundModes"));
        let plist = info_plist("demo", true);
        let at = plist.find("<key>UIBackgroundModes</key>").expect("the key");
        assert!(plist[at..].contains("<string>audio</string>"));
        assert!(plist.trim_end().ends_with("</dict>\n</plist>"));

        let dir = std::env::temp_dir().join(format!("vieww-build-bgaudio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml = dir.join("Cargo.toml");
        std::fs::write(&toml, "[package]\nname = \"demo\"\n").unwrap();
        assert!(!wants_background_audio(&dir));
        std::fs::write(
            &toml,
            "[package]\nname = \"demo\"\n\n[package.metadata.vieww]\nbackground-audio = true # lock screen\n",
        )
        .unwrap();
        assert!(wants_background_audio(&dir));
        std::fs::write(
            &toml,
            "[package]\nname = \"demo\"\n[other]\nbackground-audio = true\n",
        )
        .unwrap();
        assert!(
            !wants_background_audio(&dir),
            "only under [package.metadata.vieww]"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ----- MoltenVK bundling (FW-04) -----------------------------------------
    //
    // All pure-shape tests: the recognition, the search order and the ICD
    // rewrite are functions of paths and strings, so they are proven here on
    // any host. Only the *call* from `package_macos` is macOS-only, and that
    // is exactly the split the module doc promises.

    /// A fake MoltenVK source root in a temp dir: the dylib under `lib/`, the
    /// ICD json under whichever sub-layout `icd_sub` names.
    fn fake_moltenvk_root(tag: &str, icd_sub: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("vieww-build-moltenvk-{tag}"));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::create_dir_all(root.join(icd_sub)).unwrap();
        std::fs::write(root.join("lib/libMoltenVK.dylib"), b"fake dylib").unwrap();
        std::fs::write(
            root.join(icd_sub).join("MoltenVK_icd.json"),
            "{\n  \"file_format_version\" : \"1.0.0\",\n  \"ICD\": {\n    \
             \"library_path\" : \"/opt/homebrew/lib/libMoltenVK.dylib\",\n    \
             \"api_version\" : \"1.2.0\"\n  }\n}\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn a_moltenvk_root_is_recognized_under_either_icd_layout() {
        for icd_sub in ["share/vulkan/icd.d", "etc/vulkan/icd.d"] {
            let root = fake_moltenvk_root(&icd_sub.replace('/', "-"), icd_sub);
            let found = moltenvk_under(&root).expect("the dylib is there");
            assert_eq!(found.dylib, root.join("lib/libMoltenVK.dylib"));
            assert_eq!(
                found.icd.as_deref(),
                Some(root.join(icd_sub).join("MoltenVK_icd.json").as_path()),
                "the ICD json is picked up from {icd_sub}"
            );
            std::fs::remove_dir_all(&root).ok();
        }
    }

    #[test]
    fn a_root_without_the_dylib_is_not_a_moltenvk() {
        let root = fake_moltenvk_root("no-dylib", "share/vulkan/icd.d");
        std::fs::remove_file(root.join("lib/libMoltenVK.dylib")).unwrap();
        // Only the json (the half a loader cannot use alone): not a source.
        assert!(moltenvk_under(&root).is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_sdk_is_searched_before_homebrew_before_a_vendored_copy() {
        let sdk = fake_moltenvk_root("sdk", "share/vulkan/icd.d");
        let brew = fake_moltenvk_root("brew", "share/vulkan/icd.d");
        let vendored_dir = std::env::temp_dir().join("vieww-build-moltenvk-vendored");
        std::fs::remove_dir_all(&vendored_dir).ok();
        std::fs::create_dir_all(&vendored_dir).unwrap();
        std::fs::write(vendored_dir.join("libMoltenVK.dylib"), b"vendored").unwrap();
        std::fs::write(
            vendored_dir.join("MoltenVK_icd.json"),
            "{\"ICD\": {\"library_path\" : \"vendored\"}}",
        )
        .unwrap();

        // All three populated: the SDK — the deliberate install — wins.
        let found = find_moltenvk(
            Some(sdk.to_str().unwrap()),
            &[brew.to_str().unwrap()],
            &vendored_dir,
        )
        .expect("a source exists");
        assert_eq!(found.dylib, sdk.join("lib/libMoltenVK.dylib"));

        // No SDK: Homebrew wins over the vendored pair.
        let found =
            find_moltenvk(None, &[brew.to_str().unwrap()], &vendored_dir).expect("a source");
        assert_eq!(found.dylib, brew.join("lib/libMoltenVK.dylib"));

        // Neither: the vendored pair is a flat `moltenvk/` beside the app.
        let found = find_moltenvk(None, &[], &vendored_dir).expect("the vendored pair");
        assert_eq!(found.dylib, vendored_dir.join("libMoltenVK.dylib"));
        assert_eq!(
            found.icd.as_deref(),
            Some(vendored_dir.join("MoltenVK_icd.json").as_path())
        );

        for dir in [sdk, brew, vendored_dir] {
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn a_blank_sdk_falls_through_to_homebrew_and_says_so() {
        let brew = fake_moltenvk_root("brew-blank-sdk", "share/vulkan/icd.d");
        for blank in ["", "   "] {
            let found = find_moltenvk(
                Some(blank),
                &[brew.to_str().unwrap()],
                Path::new("/nowhere"),
            )
            .expect("homebrew is populated");
            assert_eq!(found.dylib, brew.join("lib/libMoltenVK.dylib"));
        }
        std::fs::remove_dir_all(&brew).ok();
    }

    #[test]
    fn no_moltenvk_anywhere_names_every_place_that_was_looked() {
        let searched = find_moltenvk(
            Some("/definitely/not/an/sdk"),
            &["/brew-a", "/brew-b"],
            Path::new("/nowhere/app"),
        )
        .unwrap_err();
        let joined = searched.join(" ; ");
        for needle in [
            "/definitely/not/an/sdk/lib",
            "/brew-a/lib",
            "/brew-b/lib",
            "/nowhere/app",
        ] {
            assert!(joined.contains(needle), "missing {needle} in {joined:?}");
        }
        // And the unset case names the variable, not a bare path.
        let searched = find_moltenvk(None, &[], Path::new("/nowhere/app")).unwrap_err();
        assert!(
            searched.iter().any(|s| s.contains("VULKAN_SDK")),
            "{searched:?}"
        );
    }

    #[test]
    fn an_icd_library_path_is_rewritten_to_the_bundles_own_dylib() {
        let rewritten = rewrite_icd_library_path(
            "{\n  \"ICD\": {\n    \"library_path\" : \
             \"/opt/homebrew/lib/libMoltenVK.dylib\",\n    \"api_version\" : \"1.2.0\"\n  }\n}\n",
        );
        assert!(
            rewritten.contains("\"library_path\" : \"../../Frameworks/libMoltenVK.dylib\""),
            "{rewritten}"
        );
        assert!(
            !rewritten.contains("/opt/homebrew"),
            "the absolute path to a library the bundle does not carry is gone: {rewritten}"
        );
        assert!(
            rewritten.contains("\"api_version\" : \"1.2.0\""),
            "unrelated keys survive untouched: {rewritten}"
        );

        // Spacing is not the contract — the key/value pair is.
        let loose = rewrite_icd_library_path(
            "{\"ICD\":{\"library_path\":\"/usr/local/lib/libMoltenVK.dylib\"}}",
        );
        assert!(
            loose.contains("\"library_path\" : \"../../Frameworks/libMoltenVK.dylib\""),
            "{loose}"
        );

        // A json with no `library_path` (or a value that is not a string)
        // passes through byte-for-byte rather than being mangled.
        let untouched = "{\"file_format_version\" : \"1.0.0\"}\n";
        assert_eq!(rewrite_icd_library_path(untouched), untouched);
        let not_a_string = "{\"ICD\": {\"library_path\" : 3}}";
        assert_eq!(rewrite_icd_library_path(not_a_string), not_a_string);
        // And a *longer* key that merely starts the same is not the key.
        let other_key = "{\"ICD\": {\"library_path_extra\" : \"/x/libMoltenVK.dylib\"}}";
        assert_eq!(rewrite_icd_library_path(other_key), other_key);
    }

    // ----- the host gate, testable without being any particular host -------

    #[test]
    fn linux_targets_only_a_linux_host() {
        assert!(check_host(PackageTarget::Linux, "linux").is_ok());
        assert!(check_host(PackageTarget::Linux, "macos").is_err());
        assert!(check_host(PackageTarget::Linux, "windows").is_err());
    }

    #[test]
    fn macos_and_ios_only_target_a_macos_host() {
        assert!(check_host(PackageTarget::MacOs, "macos").is_ok());
        assert!(check_host(PackageTarget::MacOs, "linux").is_err());
        assert!(check_host(PackageTarget::Ios, "macos").is_ok());
        assert!(check_host(PackageTarget::Ios, "linux").is_err());
        assert!(check_host(PackageTarget::Ios, "windows").is_err());
    }

    #[test]
    fn windows_only_targets_a_windows_host() {
        assert!(check_host(PackageTarget::Windows, "windows").is_ok());
        assert!(check_host(PackageTarget::Windows, "linux").is_err());
    }

    #[test]
    fn android_targets_every_host() {
        for host in ["linux", "macos", "windows"] {
            assert!(check_host(PackageTarget::Android, host).is_ok(), "{host}");
            // The multi-ABI APK is gated on tools, not on the host, for the
            // same reason the single-ABI one is.
            assert!(
                check_host(PackageTarget::AndroidMultiAbi, host).is_ok(),
                "{host}"
            );
        }
    }

    #[test]
    fn a_wrong_host_error_names_both_sides() {
        let error = check_host(PackageTarget::MacOs, "linux").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("macos"), "{message}");
        assert!(message.contains("linux"), "{message}");
    }

    // ----- package(), gated end-to-end on whatever this machine really is --

    /// This suite's own host decides which branch runs, and both branches are
    /// asserted: on a non-Linux host `package` for `Linux` would be refused
    /// before touching disk (exercised above via `check_host` directly, since
    /// this sandbox has no non-Linux host to run it on for real); on a
    /// non-macOS/Windows host — which this sandbox is — the *real*
    /// end-to-end call refuses with `WrongHost`, proving the gate actually
    /// runs inside `package`, not only inside `check_host`.
    #[test]
    fn packaging_for_a_host_this_machine_is_not_is_refused_before_any_build() {
        if host_os() == "macos" {
            eprintln!("skipping: this check targets a non-macOS host");
            return;
        }
        let result = package(PackageTarget::MacOs, Path::new("/nonexistent/app/dir"));
        assert!(
            matches!(result, Err(PackageError::WrongHost { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn packaging_for_windows_on_a_non_windows_host_is_refused() {
        if host_os() == "windows" {
            eprintln!("skipping: this check targets a non-Windows host");
            return;
        }
        let result = package(PackageTarget::Windows, Path::new("/nonexistent/app/dir"));
        assert!(
            matches!(result, Err(PackageError::WrongHost { .. })),
            "{result:?}"
        );
    }

    // ----- Android tool detection, fully testable with no SDK --------------

    #[test]
    fn android_tooling_is_fine_when_both_are_present() {
        assert!(android_tooling(Some("/opt/android-sdk"), true).is_ok());
    }

    #[test]
    fn android_tooling_names_a_missing_sdk() {
        let error = android_tooling(None, true).unwrap_err();
        assert!(error.contains("ANDROID_HOME"), "{error}");
    }

    #[test]
    fn android_tooling_names_a_missing_cargo_apk() {
        let error = android_tooling(Some("/opt/android-sdk"), false).unwrap_err();
        assert!(error.contains("cargo-apk"), "{error}");
    }

    #[test]
    fn an_empty_android_home_counts_as_unset() {
        assert!(android_tooling(Some("   "), true).is_err());
        assert!(android_tooling(Some(""), true).is_err());
    }

    /// The real path, on the real machine running this suite. This sandbox
    /// has neither an Android SDK nor `cargo-apk`, so packaging for Android
    /// here is a real, deterministic `MissingTool` — never a panic, never a
    /// silent no-op, and never an attempt to shell out to a tool that is not
    /// there.
    #[test]
    fn packaging_for_android_without_the_sdk_here_is_a_named_missing_tool() {
        let scratch = std::env::temp_dir().join("vieww-build-package-android-probe");
        std::fs::create_dir_all(&scratch).ok();
        std::fs::write(
            scratch.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let result = package_android(&scratch);
        std::fs::remove_dir_all(&scratch).ok();

        match result {
            Err(PackageError::MissingTool { target, .. }) => {
                assert_eq!(target, PackageTarget::Android);
            }
            other => {
                // If this machine genuinely does have a configured Android
                // SDK and cargo-apk, the call would proceed to a real build
                // instead — which this test does not want to attempt. Either
                // outcome besides an unrelated error is acceptable; only a
                // wrong *kind* of failure is a bug in the gate itself.
                assert!(
                    !matches!(other, Err(PackageError::Io(_))),
                    "unexpected I/O error: {other:?}"
                );
            }
        }
    }

    // ----- the AAB planner: fake tools on a fake PATH (FW-06) ----------------
    //
    // The process's own PATH is never mutated (see util::tool_on_path's doc
    // for the flake that forbids it); instead the fake tools live in a
    // directory this suite creates, and AndroidTools::detect reads exactly that
    // list — the "PATH" the planner is given, fake end to end.

    /// A directory of fake tools — plain files, because that is precisely
    /// what `tool_in_dirs` counts as present (its own doc explains why it
    /// does not check the executable bit).
    fn fake_tool_dir(name: &str, tools: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        for tool in tools {
            std::fs::write(dir.join(tool), b"#!/bin/sh\nexit 0\n").unwrap();
        }
        dir
    }

    /// A scratch cargo project with `[package] name = "probe"`, optionally
    /// carrying a Gradle wrapper.
    fn aab_scratch(name: &str, with_wrapper: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        if with_wrapper {
            std::fs::write(dir.join(wrapper_name()), "#!/bin/sh\nexit 0\n").unwrap();
        }
        dir
    }

    #[test]
    fn aab_tools_are_read_from_a_path_of_fake_tools() {
        let bin = fake_tool_dir(
            "vieww-build-aab-fake-path",
            &["bundletool", "java", "zip", "unzip", "gradle"],
        );
        let no_wrapper = aab_scratch("vieww-build-aab-app-plain", false);
        let tools = AndroidTools::detect(std::slice::from_ref(&bin), &no_wrapper, false);
        std::fs::remove_dir_all(&bin).ok();
        std::fs::remove_dir_all(&no_wrapper).ok();
        assert!(tools.bundletool && tools.java && tools.zip && tools.unzip && tools.gradle);
        assert!(!tools.gradlew, "this project has no wrapper");
        assert!(!tools.cargo_apk, "the caller said cargo apk is absent");

        // And the wrapper is found in the project, not on the PATH.
        let wrapper = aab_scratch("vieww-build-aab-app-wrapped", true);
        let tools = AndroidTools::detect(&[], &wrapper, false);
        std::fs::remove_dir_all(&wrapper).ok();
        assert!(tools.gradlew);
        assert!(!tools.bundletool, "an empty PATH finds nothing else");
    }

    #[test]
    fn the_planner_takes_the_projects_own_gradle_wrapper_first() {
        let app = aab_scratch("vieww-build-aab-plan-wrapper", true);
        let tools = AndroidTools {
            gradlew: true,
            java: true,
            ..AndroidTools::default()
        };
        let route = aab_plan(&tools, &app, "probe").expect("the wrapper route is possible");
        std::fs::remove_dir_all(&app).ok();
        let AabRoute::Gradle { program } = route else {
            panic!("the wrapper route is the Gradle route: {route:?}")
        };
        assert_eq!(
            program.file_name().map(std::ffi::OsStr::to_string_lossy),
            Some(wrapper_name().to_owned().into())
        );
        assert_eq!(program, app.join(wrapper_name()));
    }

    #[test]
    fn the_planner_falls_back_to_bundletool_then_a_host_gradle() {
        // Everything the bundletool route needs, no wrapper, no host gradle:
        // the bundletool route, with every intermediate under target/package.
        let app = aab_scratch("vieww-build-aab-plan-bundletool", false);
        let tools = AndroidTools {
            bundletool: true,
            java: true,
            zip: true,
            unzip: true,
            cargo_apk: true,
            ..AndroidTools::default()
        };
        let route = aab_plan(&tools, &app, "probe").expect("the bundletool route is possible");
        let AabRoute::Bundletool {
            module_dir,
            module_zip,
            config,
            bundle,
        } = route
        else {
            panic!("with every bundletool tool present the route is Bundletool: {route:?}")
        };
        let package = app.join("target/package");
        assert_eq!(module_dir, package.join("aab-module"));
        assert_eq!(module_zip, package.join("aab-base.zip"));
        assert_eq!(config, package.join("BundleConfig.json"));
        assert_eq!(bundle, package.join("probe.aab"));
        std::fs::remove_dir_all(&app).ok();

        // No bundletool but java + a host gradle: the host gradle route.
        let tools = AndroidTools {
            java: true,
            gradle: true,
            ..AndroidTools::default()
        };
        let route = aab_plan(&tools, &app, "probe").expect("the host gradle route is possible");
        assert_eq!(
            route,
            AabRoute::Gradle {
                program: PathBuf::from("gradle")
            }
        );
    }

    #[test]
    fn the_planner_refuses_by_naming_every_missing_tool_and_its_fix() {
        let app = aab_scratch("vieww-build-aab-plan-refused", false);
        let missing = aab_plan(&AndroidTools::default(), &app, "probe").unwrap_err();
        std::fs::remove_dir_all(&app).ok();

        let tools: Vec<&str> = missing.iter().map(|entry| entry.tool).collect();
        for expected in ["cargo apk", "bundletool", "java", "zip", "unzip", "gradle"] {
            assert!(
                tools.contains(&expected),
                "the refusal names {expected}: {missing:?}"
            );
        }
        // Each names its fix — the install command, not just the problem.
        let joined = missing
            .iter()
            .map(|entry| entry.line())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(joined.contains("cargo install cargo-apk"), "{joined}");
        assert!(
            joined.contains("https://github.com/google/bundletool/releases"),
            "{joined}"
        );
        // java is on both routes' lists; the merge names it once.
        assert_eq!(
            missing.iter().filter(|entry| entry.tool == "java").count(),
            1
        );
    }

    #[test]
    fn a_wrapper_without_java_refuses_naming_java_and_nothing_else() {
        let app = aab_scratch("vieww-build-aab-plan-no-java", true);
        let tools = AndroidTools {
            gradlew: true,
            ..AndroidTools::default()
        };
        let missing = aab_plan(&tools, &app, "probe").unwrap_err();
        std::fs::remove_dir_all(&app).ok();
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert_eq!(missing[0].tool, "java");
        assert!(missing[0].how.contains("gradlew"), "{missing:?}");
    }

    #[test]
    fn the_bundletool_steps_shape_the_base_module_and_the_bundle() {
        let app = aab_scratch("vieww-build-aab-steps", false);
        let tools = AndroidTools {
            bundletool: true,
            java: true,
            zip: true,
            unzip: true,
            cargo_apk: true,
            ..AndroidTools::default()
        };
        let route = aab_plan(&tools, &app, "probe").expect("the bundletool route is possible");
        let apk = app.join("target/release/apk/probe.apk");
        let steps = aab_steps(&route, &app, Some(&apk));
        std::fs::remove_dir_all(&app).ok();

        assert_eq!(steps.len(), 3, "{steps:?}");
        // Unpack the APK as a module, excluding the APK's signatures.
        assert_eq!(steps[0].program, "unzip");
        assert_eq!(steps[0].args[0], "-o");
        assert_eq!(steps[0].args[1], apk.to_string_lossy());
        assert!(steps[0].args.contains(&"-d".to_owned()));
        assert!(
            steps[0].args.contains(&"META-INF/*".to_owned()),
            "{steps:?}"
        );
        // Pack the module from inside it — the cwd is the module dir.
        assert_eq!(steps[1].program, "zip");
        assert_eq!(steps[1].cwd, app.join("target/package/aab-module"));
        assert!(steps[1].args.contains(&"-X".to_owned()));
        assert_eq!(steps[1].args.last(), Some(&".".to_owned()));
        // And hand the module to bundletool with the config.
        assert_eq!(steps[2].program, "bundletool");
        let args = steps[2].args.join(" ");
        assert!(
            args.contains("build-bundle --modules")
                && args.contains("--output")
                && args.contains("--config"),
            "{args}"
        );
        assert!(args.contains("probe.aab"), "{args}");
    }

    #[test]
    fn the_bundle_config_keeps_native_libraries_uncompressed() {
        let config = bundle_config();
        assert!(config.contains("\"lib/**/*.so\""), "{config}");
        assert!(config.contains("uncompressedGlob"), "{config}");
    }

    #[test]
    fn the_artifact_walk_finds_nested_outputs_and_skips_avoided_dirs() {
        let dir = std::env::temp_dir().join("vieww-build-aab-walk");
        std::fs::remove_dir_all(&dir).ok();
        let outputs = dir.join("app/build/outputs/bundle/release");
        std::fs::create_dir_all(&outputs).unwrap();
        std::fs::write(outputs.join("app-release.aab"), b"bundle").unwrap();
        // A stale bundle under target/ that must not win when target is avoided.
        let stale = dir.join("target/package");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("stale.aab"), b"stale").unwrap();

        assert_eq!(
            find_artifact(&dir, "aab", &["target", ".git"], 6),
            Some(outputs.join("app-release.aab"))
        );
        // Without the avoidance the stale one is reachable — the walk itself
        // has no opinion about which artifact is right.
        assert!(find_artifact(&dir, "aab", &[], 6).is_some());
        // And depth is honoured rather than recursing forever.
        assert_eq!(find_artifact(&dir, "aab", &[], 2), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The real path, on the real machine running this suite. This sandbox
    /// has no `bundletool`, no `gradle` and no `cargo apk`, so packaging an
    /// AAB here is a real, deterministic `MissingTool` that names the tools
    /// and their install commands — the refusal-first contract, exercised
    /// end to end through `package`'s own dispatch.
    #[test]
    fn packaging_an_aab_with_no_route_here_is_a_named_missing_tool() {
        let scratch = aab_scratch("vieww-build-package-aab-probe", false);
        let result = package(PackageTarget::Aab, &scratch);
        std::fs::remove_dir_all(&scratch).ok();

        match result {
            Err(PackageError::MissingTool {
                target,
                tool,
                how_to_get_it,
            }) => {
                assert_eq!(target, PackageTarget::Aab);
                assert!(
                    tool.contains("bundletool") || tool.contains("gradle"),
                    "{tool}"
                );
                assert!(
                    how_to_get_it.contains("bundletool.jar") || how_to_get_it.contains("gradle"),
                    "the refusal names how to get the tools: {how_to_get_it}"
                );
            }
            other => {
                // A machine that genuinely has a whole Android toolchain
                // would start a real build here, which this test does not
                // attempt; only a wrong *kind* of failure is the gate's bug.
                assert!(
                    !matches!(other, Err(PackageError::Io(_))),
                    "unexpected I/O error: {other:?}"
                );
            }
        }
    }

    #[test]
    fn an_aab_target_needs_no_particular_host() {
        for host in ["linux", "macos", "windows"] {
            assert!(check_host(PackageTarget::Aab, host).is_ok(), "{host}");
        }
    }

    // ----- the multi-ABI APK planner (FW-19) ----------------------------------
    //
    // Same split as the AAB planner above: the ABI table, the merge steps, the
    // refusal lines and the layout check are all pure functions of paths and
    // booleans, so every one of them is tested without an SDK — only the real
    // dispatch at the end touches this machine, and there it is the refusal
    // that is the assertion.

    #[test]
    fn the_multi_abi_table_names_the_phone_and_the_emulator() {
        let triples: Vec<&str> = MULTI_ABI.iter().map(|abi| abi.triple).collect();
        let dirs: Vec<&str> = MULTI_ABI.iter().map(|abi| abi.dir).collect();
        assert!(triples.contains(&"aarch64-linux-android"), "{MULTI_ABI:?}");
        assert!(triples.contains(&"x86_64-linux-android"), "{MULTI_ABI:?}");
        assert!(dirs.contains(&"arm64-v8a"), "{MULTI_ABI:?}");
        assert!(dirs.contains(&"x86_64"), "{MULTI_ABI:?}");
        // Every ABI's directory really is its triple's Android spelling — a
        // mismatched pair installs nowhere and says nothing about why.
        for abi in MULTI_ABI {
            let expected = if abi.triple.contains("aarch64") {
                "arm64-v8a"
            } else {
                "x86_64"
            };
            assert_eq!(
                abi.dir, expected,
                "lib/{} is not the directory {} builds into",
                abi.dir, abi.triple
            );
        }
    }

    #[test]
    fn the_merge_steps_unpack_each_abi_and_pack_the_tree_stored() {
        let app = aab_scratch("vieww-build-multi-abi-steps", false);
        let staging = app.join("target/package/apk-multi-abi");
        let unaligned = app.join("target/package/probe-unaligned.apk");
        let apks = MULTI_ABI
            .iter()
            .map(|abi| {
                (
                    *abi,
                    app.join("target")
                        .join(abi.triple)
                        .join("release/apk/probe.apk"),
                )
            })
            .collect::<Vec<_>>();
        let steps = multi_abi_steps(&apks, &staging, &unaligned);
        std::fs::remove_dir_all(&app).ok();

        // One unpack per ABI plus one pack, and every unpack names its own
        // per-target APK — the whole point of the merge.
        assert_eq!(steps.len(), MULTI_ABI.len() + 1, "{steps:?}");
        for (index, (_, apk)) in apks.iter().enumerate() {
            let step = &steps[index];
            assert_eq!(step.program, "unzip");
            assert!(
                step.args.contains(&apk.to_string_lossy().into_owned()),
                "step {index} does not unpack {}: {:?}",
                apk.display(),
                step.args
            );
            assert!(
                step.args.contains(&"META-INF/*".to_owned()),
                "the stale v1 signature files must not survive the merge"
            );
            assert_eq!(step.cwd, staging);
        }
        // The pack stores rather than deflates — `zipalign -p` can only align
        // stored entries, and the uncompressed native libraries are why.
        let pack = steps.last().expect("the pack step");
        assert_eq!(pack.program, "zip");
        assert!(pack.args.contains(&"-0".to_owned()), "{:?}", pack.args);
        assert!(
            pack.args
                .contains(&unaligned.to_string_lossy().into_owned()),
            "{:?}",
            pack.args
        );
        assert_eq!(pack.cwd, staging, "packed from inside the staging tree");
    }

    #[test]
    fn a_missing_sdk_or_merge_tool_is_named_with_its_fix() {
        // Nothing present at all: every need becomes a line with a fix.
        let missing = multi_abi_missing(None, &AndroidTools::default(), None);
        let tools: Vec<&str> = missing.iter().map(|entry| entry.tool).collect();
        for expected in ["ANDROID_HOME", "cargo apk", "zip", "unzip", "zipalign"] {
            assert!(
                tools.contains(&expected),
                "the refusal names {expected}: {missing:?}"
            );
        }
        let joined = missing
            .iter()
            .map(|entry| entry.line())
            .collect::<Vec<_>>()
            .join("; ");
        assert!(joined.contains("sdkmanager"), "{joined}");
        assert!(joined.contains("cargo install cargo-apk"), "{joined}");

        // Everything present: no refusal at all.
        let sdk = std::env::temp_dir().join("vieww-build-multi-abi-sdk-ok");
        let _ = std::fs::remove_dir_all(&sdk);
        std::fs::create_dir_all(sdk.join("build-tools/34.0.0")).unwrap();
        std::fs::write(sdk.join("build-tools/34.0.0/zipalign"), b"#!/bin/sh\n").unwrap();
        let zipalign = zipalign_in(&sdk);
        assert!(multi_abi_missing(
            Some("/opt/android-sdk"),
            &AndroidTools {
                cargo_apk: true,
                zip: true,
                unzip: true,
                ..AndroidTools::default()
            },
            zipalign.as_deref(),
        )
        .is_empty());
        std::fs::remove_dir_all(&sdk).ok();

        // An SDK without build-tools still names zipalign separately — the fix
        // for that machine is one sdkmanager command, not a new SDK.
        let bare = std::env::temp_dir().join("vieww-build-multi-abi-sdk-bare");
        let _ = std::fs::remove_dir_all(&bare);
        std::fs::create_dir_all(&bare).unwrap();
        let missing = multi_abi_missing(
            Some(bare.to_str().unwrap()),
            &AndroidTools {
                cargo_apk: true,
                zip: true,
                unzip: true,
                ..AndroidTools::default()
            },
            zipalign_in(&bare).as_deref(),
        );
        std::fs::remove_dir_all(&bare).ok();
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert_eq!(missing[0].tool, "zipalign");
    }

    #[test]
    fn zipalign_is_picked_from_the_newest_build_tools_directory() {
        let sdk = std::env::temp_dir().join("vieww-build-multi-abi-zipalign");
        let _ = std::fs::remove_dir_all(&sdk);
        for version in ["33.0.0", "34.0.0"] {
            std::fs::create_dir_all(sdk.join("build-tools").join(version)).unwrap();
            std::fs::write(
                sdk.join("build-tools").join(version).join("zipalign"),
                b"#!/bin/sh\n",
            )
            .unwrap();
        }
        assert_eq!(
            zipalign_in(&sdk),
            Some(sdk.join("build-tools/34.0.0/zipalign")),
            "the newest build-tools wins"
        );
        // A Windows SDK layout is found through the .exe spelling.
        std::fs::create_dir_all(sdk.join("build-tools/35.0.0")).unwrap();
        std::fs::write(sdk.join("build-tools/35.0.0/zipalign.exe"), b"").unwrap();
        assert_eq!(
            zipalign_in(&sdk),
            Some(sdk.join("build-tools/35.0.0/zipalign.exe"))
        );
        // And an SDK with no build-tools names none — that is the refusal
        // above's business, not a guess.
        let empty = std::env::temp_dir().join("vieww-build-multi-abi-empty");
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(zipalign_in(&empty), None);
        let _ = std::fs::remove_dir_all(&sdk);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn the_layout_check_requires_a_library_per_abi() {
        let staging = std::env::temp_dir().join("vieww-build-multi-abi-layout");
        let _ = std::fs::remove_dir_all(&staging);
        for abi in MULTI_ABI {
            std::fs::create_dir_all(staging.join("lib").join(abi.dir)).unwrap();
            std::fs::write(
                staging.join("lib").join(abi.dir).join("libprobe.so"),
                b"ELF...",
            )
            .unwrap();
        }
        assert_eq!(verify_lib_layout(&staging, MULTI_ABI), Ok(()));

        // An ABI directory with no .so in it is a failed build, and the error
        // says which directory — so the next look is at the build, not here.
        std::fs::remove_file(staging.join("lib/x86_64/libprobe.so")).unwrap();
        let error = verify_lib_layout(&staging, MULTI_ABI).unwrap_err();
        assert!(error.contains("lib/x86_64"), "{error}");
        assert!(error.contains("x86_64-linux-android"), "{error}");

        // A missing directory entirely is the same failure, not a panic.
        std::fs::remove_dir_all(staging.join("lib/arm64-v8a")).unwrap();
        let error = verify_lib_layout(&staging, MULTI_ABI).unwrap_err();
        assert!(error.contains("arm64-v8a"), "{error}");
        let _ = std::fs::remove_dir_all(&staging);
    }

    /// The real path, on the real machine running this suite. This sandbox has
    /// no Android SDK (no `ANDROID_HOME`), no `cargo apk` and no build-tools,
    /// so packaging a multi-ABI APK here is a real, deterministic
    /// `MissingTool` that names the SDK, the subcommand and `zipalign` with
    /// their fixes — the refusal-first contract, through `package`'s own
    /// dispatch and past `check_host`, which must not gate this target.
    #[test]
    fn packaging_multi_abi_with_no_sdk_here_is_a_named_missing_tool() {
        let scratch = aab_scratch("vieww-build-package-multi-abi-probe", false);
        let result = package(PackageTarget::AndroidMultiAbi, &scratch);
        std::fs::remove_dir_all(&scratch).ok();

        match result {
            Err(PackageError::MissingTool {
                target,
                tool,
                how_to_get_it,
            }) => {
                assert_eq!(target, PackageTarget::AndroidMultiAbi);
                assert!(tool.contains("cargo apk"), "{tool}");
                // On this sandbox the SDK is absent too, so the refusal says
                // so; a machine with only the SDK set says zipalign instead —
                // whichever line is present, the fix travels with it.
                assert!(
                    how_to_get_it.contains("ANDROID_HOME")
                        || how_to_get_it.contains("sdkmanager")
                        || how_to_get_it.contains("cargo install cargo-apk"),
                    "the refusal names how to get the tools: {how_to_get_it}"
                );
            }
            other => {
                // A machine that genuinely has the whole Android toolchain
                // would start a real per-ABI build here, which this test does
                // not attempt; only a wrong *kind* of failure is the gate's bug.
                assert!(
                    !matches!(other, Err(PackageError::Io(_))),
                    "unexpected I/O error: {other:?}"
                );
            }
        }
    }

    // ----- reading the package name ------------------------------------------

    #[test]
    fn the_package_name_is_read_from_the_package_section_only() {
        let scratch = std::env::temp_dir().join("vieww-build-package-name-probe");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(
            scratch.join("Cargo.toml"),
            "[dependencies]\nname = \"not-this-one\"\n\n[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        assert_eq!(read_package_name(&scratch).unwrap(), "my-app");
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn a_missing_manifest_is_a_named_error_not_a_panic() {
        let result = read_package_name(Path::new("/definitely/not/a/real/path"));
        assert!(matches!(result, Err(PackageError::Manifest(_))));
    }

    // ----- the Linux path, real end to end ----------------------------------

    /// Real, on this sandbox: builds an actual (tiny, dependency-free) crate
    /// with a real `cargo build --release`, then checks the tree
    /// `package_linux` assembled — not merely that files exist, but that the
    /// binary it copied is byte-for-byte the one cargo just built, and that
    /// the desktop entry names the right executable.
    #[test]
    fn packaging_for_linux_builds_and_lays_out_a_real_tree() {
        if host_os() != "linux" {
            eprintln!("skipping: this check only runs the Linux target on a Linux host");
            return;
        }

        let scratch = std::env::temp_dir().join("vieww-build-package-linux-e2e");
        std::fs::remove_dir_all(&scratch).ok();
        std::fs::create_dir_all(scratch.join("src")).unwrap();
        std::fs::write(
            scratch.join("Cargo.toml"),
            "[package]\nname = \"pkgprobe\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            scratch.join("src/main.rs"),
            "fn main() { println!(\"packaged and real\"); }\n",
        )
        .unwrap();

        let report = package(PackageTarget::Linux, &scratch).expect("packaging should succeed");
        assert_eq!(report.target, PackageTarget::Linux);
        assert_eq!(report.artifacts.len(), 2);

        let binary = report
            .artifacts
            .iter()
            .find(|p| p.starts_with(report.output_dir.join("bin")))
            .expect("a binary artifact");
        assert!(binary.is_file());

        let run = Command::new(binary)
            .output()
            .expect("the packaged binary should run");
        assert!(run.status.success());
        assert!(String::from_utf8_lossy(&run.stdout).contains("packaged and real"));

        let desktop = report
            .artifacts
            .iter()
            .find(|p| p.extension().is_some_and(|ext| ext == "desktop"))
            .expect("a .desktop artifact");
        let contents = std::fs::read_to_string(desktop).unwrap();
        assert!(contents.contains("Name=pkgprobe"));
        assert!(contents.contains("Exec=pkgprobe"));

        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn packaging_a_directory_with_no_manifest_fails_before_any_build() {
        if host_os() != "linux" {
            eprintln!("skipping: this check only runs the Linux target on a Linux host");
            return;
        }
        let scratch = std::env::temp_dir().join("vieww-build-package-no-manifest");
        std::fs::remove_dir_all(&scratch).ok();
        std::fs::create_dir_all(&scratch).unwrap();

        let result = package(PackageTarget::Linux, &scratch);
        assert!(
            matches!(result, Err(PackageError::Manifest(_))),
            "{result:?}"
        );

        std::fs::remove_dir_all(&scratch).ok();
    }
}
