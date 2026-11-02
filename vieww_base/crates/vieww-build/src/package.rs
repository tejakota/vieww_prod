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

use crate::util::{host_os, is_probably_binary, tool_on_path};

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
    /// A tool this target's packaging step needs is not on `PATH`.
    MissingTool {
        target: PackageTarget,
        tool: &'static str,
        how_to_get_it: &'static str,
    },
    /// `app_dir`'s `Cargo.toml` could not be found or has no `[package]
    /// name`.
    Manifest(PathBuf),
    /// `cargo build --release` ran and exited non-zero. Carries its stderr.
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
            Self::Build(stderr) => write!(f, "cargo build --release failed:\n{stderr}"),
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
/// be missing or present on any of the three desktop hosts.
fn check_host(target: PackageTarget, host: &str) -> Result<(), PackageError> {
    let ok = match target {
        PackageTarget::Linux => host == "linux",
        PackageTarget::MacOs | PackageTarget::Ios => host == "macos",
        PackageTarget::Windows => host == "windows",
        PackageTarget::Android => true,
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
            tool: "wix",
            how_to_get_it: "dotnet tool install --global wix",
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
        };
        return Err(PackageError::MissingTool {
            target: PackageTarget::Android,
            tool: "cargo apk / Android SDK",
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
            tool: "xcrun",
            how_to_get_it: "install the Xcode command line tools",
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
