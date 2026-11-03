#!/usr/bin/env bash
#
# Build a vieww Studio anybody can install.
#
# Until this script existed there was no way to give the studio to anyone. There
# was no installer, no bundle, no icon file, no `.desktop` entry and no release
# workflow — the only way to run it was `cargo run` from a checkout of the
# framework. And even a copied binary could not render, because the preview
# compiles against `libvieww.rlib` and the studio only ever looked for it in the
# checkout's own `target/`.
#
# This produces a self-contained tree in which both of those are solved:
#
#   linux / windows                  macOS
#   -----------------                --------------------------------
#   viewwstudio/                     vieww Studio.app/
#     bin/viewwstudio                  Contents/Info.plist
#     lib/vieww/                       Contents/MacOS/viewwstudio
#       vieww-sdk.toml                 Contents/Resources/vieww/
#       libvieww-<hash>.rlib             vieww-sdk.toml
#       deps/...                         libvieww-<hash>.rlib
#       toolchain/bin/rustc              deps/...
#     share/                             toolchain/bin/rustc
#       applications/...               Contents/Resources/viewwstudio.icns
#       icons/...
#
# `install::target_dir` knows both layouts, which is what makes the rlibs
# beside the binary the ones it finds — see that module for the search order.
#
# # The SDK, and why the compiler is in it
#
# A preview is a `cdylib` handing a `Box<dyn Widget>` across a library boundary,
# which is sound only when both sides came out of one compilation. Rather than
# promise a stable ABI for `vieww` — a promise that would freeze the element
# tree at the age it most needs to move — the studio carries the `rustc` it was
# built by and the rlibs it was linked against, and compiles previews with
# those. Host and guest are then the same compilation by construction. It is
# what a mobile toolchain does with its SDK language and what Xcode does with Swift.
#
# `vieww-sdk.toml` is what makes the bundle checkable rather than merely
# present; `sdk::Manifest` reads it and refuses a bundle whose compiler is not
# the one it names.
#
# # The split SDK, and the checksums (ST-14)
#
# `--split-sdk` cuts that bundle in two. The studio archive loses `lib/vieww`
# entirely (an application of ~60 MB instead of ~600), and the rlibs, `deps/`
# and the toolchain ship separately as `viewwstudio-sdk-<os>-<arch>.tar.gz`,
# with the one top-level directory the updater's
# `tar --strip-components=1` unpack strips. A first launch offers to download
# the other half of **its own release** into the directory this script would
# have filled (`lib/vieww` on Linux and Windows, `Contents/Resources/vieww`
# on macOS) - see `apps/viewwstudio/src/update.rs`, which also refuses a
# half from any other release, because the pairing between the binary and its
# rlibs is the whole doctrine of `sdk::Manifest`.
#
# Every run - split or not - also writes `SHA256SUMS` over the release
# assets, in the shape GNU `sha256sum` writes and the studio's updater reads.
# The download page has linked `releases/latest/download/SHA256SUMS` since it
# was written; verification of what was published is a property of the
# release, not of a flag somebody has to remember to pass.
#
# Usage:
#   packaging/package.sh                 # host platform, release profile, archive only
#   packaging/package.sh --installers    # and every native installer this host can make
#   packaging/package.sh --debug         # a faster build, for checking the layout
#   packaging/package.sh --no-toolchain  # ~600 MB smaller, needs a matching rustc on PATH
#   packaging/package.sh --symbols       # and viewwstudio-<os>-<arch>-symbols.tar.gz (FW-12)
#   packaging/package.sh --split-sdk     # a ~60 MB studio archive + viewwstudio-sdk-<os>-<arch>.tar.gz (ST-14)
#   VIEWWSTUDIO_BUILD=$(git rev-parse --short HEAD) packaging/package.sh
#   VIEWW_REPO=owner/name packaging/package.sh   # override the repo the site links to
#
# The download page renders by itself in a GitHub clone; `VIEWW_REPO` is only
# needed when `origin` is not the repository the release is published to.
#
# # The installers, and what each one needs on the machine that builds it
#
#   linux    .deb        dpkg-deb                (any Debian or Ubuntu)
#            .AppImage   appimagetool            (downloaded by CI; see APPIMAGETOOL)
#   macos    .dmg        hdiutil                 (always present)
#   windows  .msi        wix                     (dotnet tool install --global wix)
#
# A format whose tool is missing is **skipped with a line saying so**, never
# silently — except under `--installers`, where it is an error, because that
# flag is what release CI passes and a release that quietly ships two of its
# three Linux downloads is worse than one that fails.
#
# # Asset names carry no version, on purpose
#
# `https://<host>/<owner>/<repo>/releases/latest/download/viewwstudio-linux-x86_64.deb`
# only resolves when the asset is called exactly that in every release, and that
# stable URL is what a download page links to and what a `curl` line in a README
# can be written against. The version is inside the file — in the `.deb`
# control, in `vieww-sdk.toml`, in the About box — where it cannot rot.
#
# Notes on what this deliberately does not do:
#
# * **No code signing or notarisation.** Those need certificates that cannot
#   live in a repository, and a script that pretends to sign is worse than one
#   that says it does not. `release.yml` is where credentials would be applied,
#   and the step is marked there.
# * **No installer format** (`.msi`, `.deb`, `.dmg`). The tree below is what
#   every one of those wraps, and producing them needs tools that are not on a
#   plain runner. `--dmg` is offered on macOS because `hdiutil` is always there.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
cd "$root"

# `release`, which unwinds: the studio catches panics from the screens it
# previews (`ElementTree::build`'s boundary, in-process previews). Only the
# `dist` profile aborts — see `[profile.release]` in Cargo.toml.
profile="release"
cargo_profile=(--release)
make_dmg=""
make_deb=""
make_appimage=""
make_msi=""
# Set by `--installers`: every format this host can produce, and a hard failure
# rather than a skip when a tool is missing. See the header.
require_installers=""
# Shipping the compiler is the default, because it is the whole point: a studio
# that carries the `rustc` it was built by makes host and guest one compilation
# by construction, and nothing about `vieww`'s ABI has to be promised. It costs
# roughly 600 MB, so `--no-toolchain` produces the smaller "our rlibs, your
# compiler" bundle for someone who knows they want it — and `sdk::Manifest`
# records which of the two this is.
with_toolchain="yes"
# FW-12: build with line tables and split them off the shipped binary into a
# symbols archive (ci/release/symbols.sh), so a release crash report's raw
# frames resolve to functions and lines (ci/release/symbolize.py). The
# shipped binary is stripped of them; the archive is a release asset, never
# part of the bundle. The bundled rlibs keep their line tables, which only
# makes a guest's own backtraces readable.
with_symbols=""
# ST-14: ship the app and the SDK as two archives, so a first launch is a
# ~60 MB download and the compiler arrives only if it is wanted. See the
# header for the shape, and apps/viewwstudio/src/update.rs for the fetch.
split_sdk=""
for arg in "$@"; do
	case "$arg" in
	--debug)
		profile="debug"
		cargo_profile=()
		;;
	--dmg) make_dmg="yes" ;;
	--deb) make_deb="yes" ;;
	--appimage) make_appimage="yes" ;;
	--msi) make_msi="yes" ;;
	--installers)
		require_installers="yes"
		make_dmg="yes"
		make_deb="yes"
		make_appimage="yes"
		make_msi="yes"
		;;
	--with-toolchain) with_toolchain="yes" ;;
	--no-toolchain) with_toolchain="" ;;
	--symbols) with_symbols="yes" ;;
	--split-sdk) split_sdk="yes" ;;
	*)
		echo "unknown option: $arg" >&2
		exit 2
		;;
	esac
done

# ST-14: the split bundle has two halves and no single installer to put them
# in - a `.deb` built from the app-only tree would carry a description, a
# `Depends` and an `Installed-Size` that all lie. The two archives are the
# whole release. (`--dmg` is allowed: on macOS the .dmg *is* the studio
# archive, and the split run makes it by default.)
if [ -n "$split_sdk" ] && [ -n "$require_installers$make_deb$make_appimage$make_msi" ]; then
	echo "--split-sdk produces the two archives; --installers/--deb/--appimage/--msi embed the one-piece bundle - pass one or the other" >&2
	exit 2
fi

# **One compiler, named once.**
#
# `cargo` honours `RUSTC`, and so does the studio's own `build.rs` when it
# stamps `VIEWWSTUDIO_RUSTC`. A script that builds with cargo and then asks a
# bare `rustc` for the manifest is asking a *different* compiler — under a
# `rust-toolchain.toml` override, a rustup shim with no network, or a set
# `RUSTC`, the two genuinely differ. That is precisely the provenance bug this
# bundle exists to close, reintroduced by the thing that assembles it.
rustc_bin="${RUSTC:-rustc}"
rustc_version="$("$rustc_bin" --version 2>/dev/null || true)"
rustc_host="$("$rustc_bin" -vV 2>/dev/null | sed -n 's/^host: //p' || true)"
[ -n "$rustc_version" ] && [ -n "$rustc_host" ] || {
	echo "\`$rustc_bin --version\` produced nothing, so the SDK manifest would be" >&2
	echo "written with empty fields and refused at first launch. Set RUSTC to the" >&2
	echo "compiler cargo will use, or fix the toolchain shim, and run this again." >&2
	exit 1
}

version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
[ -n "$version" ] || {
	echo "could not read the workspace version from Cargo.toml" >&2
	exit 1
}

case "$(uname -s)" in
Darwin) host="macos" ;;
Linux) host="linux" ;;
MINGW* | MSYS* | CYGWIN*) host="windows" ;;
*) host="linux" ;;
esac

# ---------------------------------------------------------------- the repository
#
# Resolved once, here, because two halves of this script need it and the
# first is the build itself: `VIEWWSTUDIO_REPO` is stamped into the binary by
# `build.rs` and read by the studio's updater (ST-14), which refuses to guess
# where releases come from. Precedence is the download page's: the deliberate
# override, then Actions, then the clone. The third case - asking the
# checkout - is `repo_from_git` below.
#
# `git remote get-url origin` handles the three URL shapes a clone can carry:
#
#   https://github.com/owner/name.git
#   git@github.com:owner/name.git
#   ssh://git@github.com/owner/name
#
# Anything that is not GitHub, and any directory that is not a clone, yields
# nothing and falls through to the no-repo messages. The two environment
# variables still win - `GITHUB_REPOSITORY` is right in Actions even when
# `origin` is a fork, and `VIEWW_REPO` is the deliberate override.
repo_from_git() {
	local url
	url="$(git -C "$root" remote get-url origin 2>/dev/null)" || return 0
	case "$url" in
	*github.com[:/]*) ;;
	*) return 0 ;;
	esac
	# Strip everything up to and including the host, and the optional `.git`.
	url="${url#*github.com}"
	url="${url#[:/]}"
	url="${url%.git}"
	url="${url%/}"
	# `owner/name` and nothing else: two segments, no spaces.
	case "$url" in
	*/*/*) return 0 ;;
	*/*) printf '%s' "$url" ;;
	esac
}
repo="${VIEWW_REPO:-${GITHUB_REPOSITORY:-$(repo_from_git)}}"
# Stamped, not re-resolved in build.rs: this script already knows, and a
# second resolution in a second language is a second way to disagree with
# the download page. A pre-set VIEWWSTUDIO_REPO wins - it is the deliberate
# hand-authored one.
if [ -n "$repo" ] && [ -z "${VIEWWSTUDIO_REPO:-}" ]; then
	export VIEWWSTUDIO_REPO="$repo"
fi

out="$root/target/package"
rm -rf "$out"
mkdir -p "$out"

echo "==> building viewwstudio $version ($profile, $host)"
# `${cargo_profile[@]+...}` rather than `"${cargo_profile[@]}"` throughout: with
# `--debug` the array is empty, and macOS's bash 3.2 treats an empty array as
# unset under `set -u` — "cargo_profile[@]: unbound variable", which is how the
# macOS packaging smoke test died before building anything.
# `VIEWWSTUDIO_BUILD` is optional and read by `about.rs`. A build with none says
# "development build" rather than leaving a blank line in a bug report.
#
# **`--message-format=json-render-diagnostics`, and why it is not cosmetic.**
# Cargo reports the exact artefact files this build produced. That list is the
# difference between shipping the rlibs the binary linked and shipping the ones
# that happen to be newest — see `copy_rlibs`. Diagnostics still render to the
# terminal; only the machine-readable stream is captured.
build_log="$out/build.json"
# **Windows: `-C prefer-dynamic` for the shipped studio, set here.**
#
# Every preview the studio compiles is `-C prefer-dynamic` on every host
# (`apps/viewwstudio/src/compile.rs`), so it imports `std-<hash>.dll`. The
# studio has to import the same DLL or a guest panic is a foreign exception to
# its `catch_unwind` — `copy_windows_std` below checks exactly that, and it
# failed on the first Windows run to get this far, because `.cargo/config.toml`
# sets the flag for Linux and macOS only.
#
# Scoped to packaging rather than added to that file's Windows section: on
# Windows nothing but cargo puts the sysroot on `PATH`, and the gate runs
# binaries directly (`export-suite.sh` runs `target/release/examples/export_check`),
# which would stop starting. The bundle carries the DLL beside the exe, so only
# the bundle needs the flag. No config block exists for Windows, so setting
# `RUSTFLAGS` replaces nothing.
if [ "$host" = "windows" ]; then
	export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C prefer-dynamic"
fi
if [ -n "$with_symbols" ]; then
	export CARGO_PROFILE_RELEASE_DEBUG=line-tables-only
fi
cargo build -p viewwstudio ${cargo_profile[@]+"${cargo_profile[@]}"} \
	--message-format=json-render-diagnostics >"$build_log"

echo "==> drawing icons"
cargo run -p viewwstudio --example icon ${cargo_profile[@]+"${cargo_profile[@]}"} -- "$out/icons" >/dev/null

target="$root/target/$profile"
binary="$target/viewwstudio"
[ "$host" = "windows" ] && binary="$target/viewwstudio.exe"

if [ -n "$with_symbols" ]; then
	echo "==> splitting debug info off the shipped binary (FW-12)"
	stage="$out/stage"
	rm -rf "$stage" "$out/symbols"
	mkdir -p "$stage"
	cp "$binary" "$stage/"
	[ -d "$binary.dSYM" ] && cp -R "$binary.dSYM" "$stage/"
	[ "$host" = "windows" ] && cp "$target/viewwstudio.pdb" "$stage/"
	binary="$stage/$(basename "$binary")"
	bash ci/release/symbols.sh "$binary" "$out/symbols"
	arch="$(uname -m)"
	[ "$arch" = arm64 ] && arch=aarch64
	tar -C "$out" -czf "$out/viewwstudio-$host-$arch-symbols.tar.gz" symbols
fi

# Every compiled artefact this build produced, one path per line.
#
# **Including proc-macro libraries, which is not a detail.** A proc macro is
# compiled to a native `.so`/`.dylib`/`.dll` in `deps/`, and rustc loads it from
# the `-L dependency` path when a crate in the graph uses one. Every version of
# this script until now copied `*.rlib` and `*.rmeta` and nothing else, so a
# packaged studio's `deps/` had none of them — and the guest compile failed with
# `error[E0463]: can't find crate for 'vieww'`, which names the one crate that
# was definitely present and says nothing about the ones that were not. The
# preview has therefore never worked from an installed bundle; it worked in a
# checkout because `target/debug/deps` had the macro libraries sitting in it.
#
# **Cargo's answer, not the filesystem's.** The previous version of this listed
# `deps/` by modification time and kept the newest per crate name. That is a
# guess, and it is wrong in exactly the case that matters: a workspace routinely
# holds two compilations of one crate because feature resolution differed, and
# under unification the studio binary may well have linked the *older* one. The
# bundle then contained a `vieww` the binary beside it had never seen, every
# `TypeId` missed, and the preview came out blank while reporting success —
# which is the failure `loaded::host_fingerprint` exists to catch, shipped in
# the product rather than caught in a checkout.
#
# One line per message, and paths do not contain quotes, so this needs no JSON
# parser and therefore no Python on the packaging machine.
#
# **Windows paths arrive JSON-escaped** — `"D:\\a\\...\\libvieww-….rlib"` — so
# they are turned into forward-slash paths, which Git Bash's `cp` and `[ -f ]`
# accept and which the `/libvieww-` pattern below can match. Before this, the
# pattern matched nothing on Windows and, under `set -euo pipefail`, the
# command substitution that asked for the rlib ended the script silently right
# after "drawing icons" — no message, exit 1, no installer.
artefacts() {
	grep -o '"[^"]*\.\(rlib\|rmeta\|so\|dylib\|dll\)"' "$build_log" | tr -d '"' |
		sed 's#\\\\#/#g; s#\\#/#g' | sort -u
}

# The umbrella `vieww` rlib, by its real hashed name.
#
# `libvieww-` with the hyphen: `vieww_widget` and friends are `libvieww_` with
# an underscore, so the prefix picks out the one crate the buffer writes
# `use vieww::prelude::*` against. This is the same rule `compile::vieww_candidates`
# applies, which is not a coincidence — it is the rule this bundle exists to
# make unnecessary.
vieww_rlib_path() {
	# `|| true`: no match must reach the explanatory message in copy_rlibs, not
	# end the script through `set -e` inside a command substitution.
	artefacts | grep '/libvieww-[^/]*\.rlib$' | head -1 || true
}

# The rlibs the preview links against. Copied rather than symlinked: the whole
# point is a tree that survives being moved to another machine.
copy_rlibs() {
	local dest="$1"
	mkdir -p "$dest/deps"

	local rlib
	rlib="$(vieww_rlib_path)"
	[ -n "$rlib" ] || {
		echo "cargo reported no libvieww rlib for this build — is $build_log complete?" >&2
		exit 1
	}
	cp "$rlib" "$dest/"

	# **`.rmeta` only where there is no `.rlib` beside it.**
	#
	# Cargo emits both for every crate: the `.rlib` carries the code *and* the
	# metadata, and the `.rmeta` exists so that dependent crates can start
	# type-checking before the code is finished. A preview is compiled with
	# `--crate-type cdylib`, which needs the code — rustc will not link a
	# `.rmeta` — so shipping both is 400 MB of duplication in a debug bundle
	# and roughly 40% of the whole SDK. Kept where a crate produced *only* an
	# `.rmeta`, because then it is not a duplicate of anything.
	local file
	while IFS= read -r file; do
		[ -f "$file" ] || continue
		case "$file" in
		*.rmeta) [ -f "${file%.rmeta}.rlib" ] && continue ;;
		esac
		cp "$file" "$dest/deps/"
	done < <(artefacts)

	write_manifest "$dest" "$(basename "$rlib")"
}

# The compiler, so host and guest are one compilation by construction.
#
# A sysroot copy rather than a curated one: `rustc --print sysroot` names a
# directory that is already self-contained — the driver, its shared libraries
# and `lib/rustlib` — and knowing which parts of it a given compile will reach
# for is precisely the kind of thing that is right until it is not.
copy_toolchain() {
	local dest="$1"
	[ -n "$with_toolchain" ] || return 0
	local sysroot
	sysroot="$("$rustc_bin" --print sysroot)"
	[ -d "$sysroot" ] || {
		echo "rustc reports a sysroot that is not there: $sysroot" >&2
		exit 1
	}
	echo "==> bundling the toolchain from $sysroot"
	mkdir -p "$dest/toolchain"
	cp -R "$sysroot"/. "$dest/toolchain/"
	# `sdk::rustc_path` looks for exactly this and silently reports "no bundled
	# compiler" if it is absent, so a sysroot laid out unexpectedly would
	# otherwise degrade to a PATH lookup without saying so.
	[ -x "$dest/toolchain/bin/rustc" ] || [ -x "$dest/toolchain/bin/rustc.exe" ] || {
		echo "no bin/rustc under the copied sysroot — the bundle would fall back to PATH" >&2
		exit 1
	}
}

# Put the Rust `std` DLL next to `viewwstudio.exe`, and refuse to ship without it.
#
# # Why Windows needs a step the other two do not
#
# The studio links `std` dynamically on every desktop platform, because a
# `cdylib` preview that carries its own static `std` raises panics the host's
# `catch_unwind` cannot catch — see `.cargo/config.toml` for the whole argument.
# On Linux and macOS that is all it takes: the binary records a runpath, and the
# loader reads it.
#
# **Windows has no runpath.** `LoadLibrary` looks in the directory the `.exe`
# lives in, then the system directories, then `PATH` — never at a list baked
# into the image. So `-C rpath` is inert there and the DLL has to be *placed*.
#
# It goes in `bin\` beside the executable rather than in `lib\vieww\toolchain\`
# with the rest of the sysroot, because `bin\` is the one directory Windows
# will look in without being told.
#
# # The check is the point
#
# This is the half of the Windows panic boundary that a Linux or macOS build
# machine cannot exercise, so it is written to fail loudly rather than quietly
# do nothing: a missing DLL, or an executable whose import table does not name
# it, stops the packaging run. A studio that cannot find its `std` does not
# start at all, and finding that out here beats finding it out from the first
# person who downloads it.
copy_windows_std() {
	local dest="$1"
	local sysroot
	sysroot="$("$rustc_bin" --print sysroot)"

	# The same two directories `compile::sysroot_lib` searches at runtime, in
	# the same order.
	local candidates=(
		"$sysroot/lib/rustlib/$rustc_host/lib"
		"$sysroot/lib"
		"$sysroot/bin"
	)
	local dll=""
	for dir in "${candidates[@]}"; do
		[ -d "$dir" ] || continue
		dll="$(find "$dir" -maxdepth 1 -name 'std-*.dll' 2>/dev/null | head -1)"
		[ -n "$dll" ] && break
	done

	if [ -z "$dll" ]; then
		echo "no std-*.dll under $sysroot" >&2
		echo "The studio links std dynamically (.cargo/config.toml). Without the" >&2
		echo "DLL beside the executable it will not start on a machine that has" >&2
		echo "no Rust toolchain. Refusing to ship a bundle that cannot run." >&2
		exit 1
	fi

	cp "$dll" "$dest/"
	echo "    std: $(basename "$dll") beside the executable"

	# Does the executable actually ask for the file that was just copied? A PE
	# import table stores DLL names as plain ASCII, so `grep` on the binary
	# answers this without `dumpbin`, which is not on a build machine unless
	# Visual Studio put it there.
	local name
	name="$(basename "$dll")"
	if ! grep -qa "$name" "$dest/viewwstudio.exe"; then
		echo "viewwstudio.exe does not import $name" >&2
		echo "That means it was linked with a static std after all, so a panic" >&2
		echo "inside a previewed widget will abort the studio instead of being" >&2
		echo "caught. Check that .cargo/config.toml's Windows section applied." >&2
		exit 1
	fi
	echo "    verified: the executable imports it"
}

# Compile a screen against the bundle, exactly as the studio's Render does.
#
# **The check this script most needed and did not have.** Everything above can
# be right by inspection — the manifest complete, the rlib named, 271
# dependencies present — and the bundle still not compile anything, which is
# precisely what shipping no proc-macro libraries did. A layout is not a
# guarantee. The only evidence that an installed studio can preview is a
# preview, so one is built here, from the bundle, before the archive is made.
#
# `--crate-type cdylib` and the two flags are copied from `compile.rs` rather
# than approximated: a check that compiles differently from the thing it
# certifies certifies nothing.
verify_bundle() {
	local dest="$1"
	local rustc="$rustc_bin"
	[ -x "$dest/toolchain/bin/rustc" ] && rustc="$dest/toolchain/bin/rustc"

	local probe="$out/probe"
	mkdir -p "$probe"
	cat >"$probe/screen.rs" <<'EOF'
use vieww::prelude::*;
pub fn screen() -> impl Widget {
	Text::new("packaged")
}
EOF
	local rlib
	rlib="$dest/$(basename "$(vieww_rlib_path)")"
	echo "==> verifying the bundle compiles a preview"
	if ! "$rustc" --edition 2021 --crate-type cdylib -C opt-level=0 \
		--extern vieww="$rlib" -L dependency="$dest/deps" \
		"$probe/screen.rs" -o "$probe/screen.so" 2>"$probe/errors"; then
		echo "the bundle cannot compile a preview, so the studio in it could not render:" >&2
		sed 's/^/    /' "$probe/errors" >&2
		exit 1
	fi
	rm -rf "$probe"
}

# What the bundle claims to be. Read by `sdk::Manifest`, which refuses a bundle
# whose compiler is not the one named here — the partially-updated install that
# every presence-based check passes.
write_manifest() {
	local dest="$1" rlib="$2"
	cat >"$dest/vieww-sdk.toml" <<EOF
# Written by packaging/package.sh. Describes the compilation this studio previews against.
format = 1
vieww = "$version"
rustc = "$rustc_version"
host = "$rustc_host"
rlib = "$rlib"
EOF
}

# The SDK half of a split release (ST-14): archived on its own, under the
# stable version-less name the updater asks its own release for, with the
# one top-level directory its `tar --strip-components=1` unpack strips.
#
# `uname -m` mapped to rust's arch spelling, because the name has to match
# `std::env::consts::ARCH` on the machine that downloads it - arm64 from
# uname is aarch64 to everything in this repo.
archive_sdk() {
	local sdk="$1" arch
	arch="$(uname -m)"
	[ "$arch" = arm64 ] && arch=aarch64
	tar -C "$out" -czf "$out/viewwstudio-sdk-$host-$arch.tar.gz" "$(basename "$sdk")"
	echo "==> $out/viewwstudio-sdk-$host-$arch.tar.gz"
	du -sh "$out/viewwstudio-sdk-$host-$arch.tar.gz" | awk '{print "    " $1}'
}

# A tool that is not there, said once, in the one voice.
#
# Under `--installers` this is fatal: that flag is what release CI passes, and a
# release missing one of its downloads because a runner image changed is exactly
# the failure that should stop the build rather than reach the download page.
need_tool() {
	local tool="$1" format="$2" how="$3"
	command -v "$tool" >/dev/null 2>&1 && return 0
	if [ -n "$require_installers" ]; then
		echo "no \`$tool\` on PATH, so the $format cannot be built — $how" >&2
		exit 1
	fi
	echo "    skipping the $format: no \`$tool\` on PATH ($how)"
	return 1
}

# The size of a tree in kilobytes, for `Installed-Size`.
tree_kb() {
	du -sk "$1" | awk '{print $1}'
}

# A Debian package, from the same tree the tarball holds.
#
# # Why `/usr` and not `/opt`
#
# `install::target_dir` looks for a `lib/vieww` beside and one level above the
# binary, so `/usr/bin/viewwstudio` finds `/usr/lib/vieww` with no environment
# variable and no wrapper script. `/opt/viewwstudio/...` would need one of those,
# and a wrapper script is a second place the SDK path is written down.
#
# # `Depends: gcc | clang`
#
# The one dependency that is genuinely required and easy to forget: a preview is
# a `cdylib`, rustc drives `cc` to link it, and the bundled compiler cannot
# bring a C toolchain with it. Declared as an alternative because either will
# do, and as a hard `Depends` rather than a `Recommends` because a studio that
# cannot link is a studio whose headline feature does not run.
make_deb() {
	local tree="$1"
	need_tool dpkg-deb ".deb" "apt install dpkg-dev" || return 0

	local stage="$out/deb"
	rm -rf "$stage"
	mkdir -p "$stage/usr/bin" "$stage/usr/lib" "$stage/usr/share" "$stage/DEBIAN"
	cp "$tree/bin/viewwstudio" "$stage/usr/bin/"
	cp -R "$tree/lib/vieww" "$stage/usr/lib/"
	cp -R "$tree/share/." "$stage/usr/share/"

	cat >"$stage/DEBIAN/control" <<EOF
Package: viewwstudio
Version: $version
Section: devel
Priority: optional
Architecture: amd64
Depends: libc6, libgcc-s1, gcc | clang | build-essential
Installed-Size: $(tree_kb "$stage")
Maintainer: ${VIEWWSTUDIO_MAINTAINER:-vieww <noreply@example.invalid>}
Description: A code editor and device-framed preview for vieww screens
 Edit a vieww screen on the left, compile it, and watch it mount on the
 right. The package carries the rustc it was built by and the rlibs it was
 linked against, so the preview compiles against exactly the compilation
 this studio is — see /usr/lib/vieww/vieww-sdk.toml.
 .
 A C toolchain is required: rustc links the preview through cc.
EOF

	dpkg-deb --root-owner-group --build "$stage" "$out/viewwstudio-linux-x86_64.deb" >/dev/null
	rm -rf "$stage"
	echo "==> $out/viewwstudio-linux-x86_64.deb"
	du -sh "$out/viewwstudio-linux-x86_64.deb" | awk '{print "    " $1}'
}

# A single-file AppImage, for every distribution that is not Debian.
#
# The AppDir is the same `/usr` layout the `.deb` installs, which is what makes
# the SDK reachable: an AppImage mounts itself at a random path under `/tmp`,
# `current_exe()` reports the binary inside that mount, and `install::target_dir`
# finds `../lib/vieww` beside it exactly as it would under `/usr`. Nothing in
# the studio knows it is inside an AppImage.
#
# `--appimage-extract-and-run` because a CI container has no FUSE, and the tool
# is itself an AppImage.
make_appimage() {
	local tree="$1"
	local tool="${APPIMAGETOOL:-appimagetool}"
	command -v "$tool" >/dev/null 2>&1 || [ -x "$tool" ] || {
		if [ -n "$require_installers" ]; then
			echo "no appimagetool, so the .AppImage cannot be built — set APPIMAGETOOL" >&2
			echo "to a downloaded appimagetool-x86_64.AppImage, or drop --appimage" >&2
			exit 1
		fi
		echo "    skipping the .AppImage: no appimagetool (set APPIMAGETOOL to one)"
		return 0
	}

	local dir="$out/AppDir"
	rm -rf "$dir"
	mkdir -p "$dir/usr/bin" "$dir/usr/lib"
	cp "$tree/bin/viewwstudio" "$dir/usr/bin/"
	cp -R "$tree/lib/vieww" "$dir/usr/lib/"
	cp -R "$tree/share/." "$dir/usr/share/"
	# The three files the format wants at the root of the AppDir.
	cp "$here/viewwstudio.desktop" "$dir/viewwstudio.desktop"
	cp "$out/icons/icon-256.png" "$dir/viewwstudio.png"
	cp "$out/icons/icon-256.png" "$dir/.DirIcon"
	cat >"$dir/AppRun" <<'APPRUN'
#!/bin/sh
# The AppImage's own entry point. `exec` rather than a launcher process, so the
# studio is the process the desktop sees and a signal reaches it directly.
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/viewwstudio" "$@"
APPRUN
	chmod +x "$dir/AppRun"

	ARCH=x86_64 "$tool" --appimage-extract-and-run "$dir" \
		"$out/viewwstudio-linux-x86_64.AppImage" >/dev/null 2>&1 || {
		echo "appimagetool failed on $dir" >&2
		exit 1
	}
	chmod +x "$out/viewwstudio-linux-x86_64.AppImage"
	rm -rf "$dir"
	echo "==> $out/viewwstudio-linux-x86_64.AppImage"
	du -sh "$out/viewwstudio-linux-x86_64.AppImage" | awk '{print "    " $1}'
}

# A Windows installer, from the same tree the zip holds.
#
# WiX 4 and later, for one reason: its `<Files Include="...\**"/>` takes a
# directory of ten thousand files — which is what a bundled sysroot is — without
# a harvesting step that has to be re-run whenever the compiler's own layout
# changes. `viewwstudio.wxs` beside this script is the whole authoring.
make_msi() {
	local tree="$1"
	need_tool wix ".msi" "dotnet tool install --global wix" || return 0

	# FW-06: sign every executable in the payload, then the .msi (no-ops,
	# stated, without a certificate).
	bash "$here/sign.sh" win-tree "$tree"
	wix build -arch x64 \
		-d "Version=$version" \
		-d "Payload=$tree" \
		-o "$out/viewwstudio-windows-x86_64.msi" \
		"$here/viewwstudio.wxs" || {
		echo "wix build failed" >&2
		exit 1
	}
	echo "==> $out/viewwstudio-windows-x86_64.msi"
	bash "$here/sign.sh" win "$out/viewwstudio-windows-x86_64.msi"
}

case "$host" in
macos)
	app="$out/vieww Studio.app"
	# A split release's studio archive on macOS is the .dmg - it is the asset
	# the updater asks for - so make it even when nothing asked for installers.
	[ -n "$split_sdk" ] && make_dmg="yes"
	mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
	cp "$binary" "$app/Contents/MacOS/viewwstudio"
	if [ -n "$split_sdk" ]; then
		# ST-14: the app without the SDK, and the SDK on its own. The updater
		# unpacks the latter into Contents/Resources/vieww - the one place
		# install::sdk_dir names on a Mac.
		sdk="$out/viewwstudio-sdk-$version"
		copy_rlibs "$sdk"
		copy_toolchain "$sdk"
		verify_bundle "$sdk"
	else
		copy_rlibs "$app/Contents/Resources/vieww"
		copy_toolchain "$app/Contents/Resources/vieww"
		verify_bundle "$app/Contents/Resources/vieww"
	fi

	# One version number, from Cargo.toml, into both plist keys.
	sed -e "s|<string>0\.0\.0</string>|<string>$version</string>|g" \
		"$here/Info.plist" >"$app/Contents/Info.plist"

	if command -v iconutil >/dev/null 2>&1; then
		set="$out/viewwstudio.iconset"
		mkdir -p "$set"
		for size in 16 32 128 256 512; do
			cp "$out/icons/icon-$size.png" "$set/icon_${size}x${size}.png"
			double=$((size * 2))
			[ -f "$out/icons/icon-$double.png" ] &&
				cp "$out/icons/icon-$double.png" "$set/icon_${size}x${size}@2x.png"
		done
		iconutil -c icns "$set" -o "$app/Contents/Resources/viewwstudio.icns"
		rm -rf "$set"
	else
		# Stated, not silently skipped: an app with no icon is the grey
		# placeholder in the dock, and knowing why beats wondering.
		echo "    iconutil not found — shipping PNGs instead of an .icns"
		cp -R "$out/icons" "$app/Contents/Resources/"
	fi

	echo "==> $app"
	bash "$here/sign.sh" mac-app "$app"
	if [ -n "$make_dmg" ]; then
		# Per architecture, because the bundle carries rlibs for one triple and
		# `sdk::Manifest` refuses the other — an Apple silicon Mac must not be
		# handed the Intel download. `rustc_host` is the authority, since it is
		# the compiler that decided what is in the bundle.
		arch="${rustc_host%%-*}"
		dmg="$out/viewwstudio-macos-$arch.dmg"
		hdiutil create -volname "vieww Studio" -srcfolder "$app" \
			-ov -format UDZO "$dmg" >/dev/null
		echo "==> $dmg"
		du -sh "$dmg" | awk '{print "    " $1}'
		bash "$here/sign.sh" mac-dmg "$dmg"
	fi
	[ -n "$split_sdk" ] && archive_sdk "$sdk"
	;;
linux)
	tree="$out/viewwstudio-$version"
	mkdir -p "$tree/bin" "$tree/share/applications" "$tree/share/icons/hicolor"
	cp "$binary" "$tree/bin/viewwstudio"
	if [ -n "$split_sdk" ]; then
		# ST-14: the app-only tree. `lib/vieww` is deliberately absent - it is
		# what a first launch downloads - so the tree is ~60 MB and the SDK
		# below is archived on its own.
		sdk="$out/viewwstudio-sdk-$version"
		copy_rlibs "$sdk"
		copy_toolchain "$sdk"
		verify_bundle "$sdk"
	else
		copy_rlibs "$tree/lib/vieww"
		copy_toolchain "$tree/lib/vieww"
		verify_bundle "$tree/lib/vieww"
	fi
	cp "$here/viewwstudio.desktop" "$tree/share/applications/"
	for size in 16 32 48 64 128 256 512; do
		dir="$tree/share/icons/hicolor/${size}x${size}/apps"
		mkdir -p "$dir"
		cp "$out/icons/icon-$size.png" "$dir/viewwstudio.png"
	done

	if [ -n "$split_sdk" ]; then
		cat >"$tree/install.sh" <<'INSTALL'
#!/usr/bin/env sh
# Install the app-only half of a split bundle (ST-14). The SDK - the rlibs
# and the compiler the preview compiles with - is fetched by the studio
# itself on first launch, into the lib/vieww this installer does not write.
set -eu
prefix="${1:-$HOME/.local}"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$prefix/bin" "$prefix/share"
cp "$here/bin/viewwstudio" "$prefix/bin/"
cp -R "$here/share/." "$prefix/share/"
command -v update-desktop-database >/dev/null 2>&1 &&
	update-desktop-database "$prefix/share/applications" 2>/dev/null || true
echo "installed to $prefix"
echo "if 'viewwstudio' is not found, add $prefix/bin to PATH"
INSTALL
	else
		cat >"$tree/install.sh" <<'INSTALL'
#!/usr/bin/env sh
# Install into a prefix. ~/.local needs no privileges and is on the XDG path
# every desktop reads, so it is the default.
set -eu
prefix="${1:-$HOME/.local}"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$prefix/bin" "$prefix/lib" "$prefix/share"
cp "$here/bin/viewwstudio" "$prefix/bin/"
rm -rf "$prefix/lib/vieww"
cp -R "$here/lib/vieww" "$prefix/lib/"
cp -R "$here/share/." "$prefix/share/"
command -v update-desktop-database >/dev/null 2>&1 &&
	update-desktop-database "$prefix/share/applications" 2>/dev/null || true
echo "installed to $prefix"
echo "if 'viewwstudio' is not found, add $prefix/bin to PATH"
INSTALL
	fi
	chmod +x "$tree/install.sh"

	tar -C "$out" -czf "$out/viewwstudio-linux-x86_64.tar.gz" "viewwstudio-$version"
	echo "==> $out/viewwstudio-linux-x86_64.tar.gz"
	# Said out loud, because the rlibs dominate it and a debug build is much
	# larger than a release one. A number nobody printed is a number nobody
	# noticed growing.
	du -sh "$out/viewwstudio-linux-x86_64.tar.gz" | awk '{print "    " $1}'

	[ -n "$split_sdk" ] && archive_sdk "$sdk"

	[ -n "$make_deb" ] && make_deb "$tree"
	[ -n "$make_appimage" ] && make_appimage "$tree"
	;;
windows)
	tree="$out/viewwstudio-$version"
	mkdir -p "$tree/bin"
	cp "$binary" "$tree/bin/viewwstudio.exe"
	if [ -n "$split_sdk" ]; then
		# ST-14: the app-only zip, and the SDK on its own; the updater
		# unpacks the latter into the tree's lib/vieww. Windows 10+ ships
		# bsdtar as tar.exe, which speaks --strip-components.
		sdk="$out/viewwstudio-sdk-$version"
		copy_rlibs "$sdk"
		copy_toolchain "$sdk"
		verify_bundle "$sdk"
	else
		copy_rlibs "$tree/lib/vieww"
		copy_toolchain "$tree/lib/vieww"
		verify_bundle "$tree/lib/vieww"
	fi
	copy_windows_std "$tree/bin"
	cp -R "$out/icons" "$tree/"

	# A zip as well as an installer, because "unpack it anywhere and run it" is
	# the one shape that needs no administrator, no uninstaller entry and no
	# trust decision — and because it is what the studio's own layout already
	# supports: `install::target_dir` finds `lib\vieww` beside the executable.
	if command -v powershell.exe >/dev/null 2>&1; then
		# **PowerShell needs Windows paths, and `$tree`/`$out` are Git Bash
		# ones.** `\` on a `/d/a/...` string produces `\d\a\...`, which
		# PowerShell reads as a UNC-ish path that does not exist:
		# "Compress-Archive : The path '\d\a\...\target\package' either does
		# not exist or is not a valid file system path" — the whole Windows
		# release, stopped after everything had been built and verified.
		# `cygpath -w` is the conversion, and it ships with Git Bash.
		tree_win="$(cygpath -w "$tree" 2>/dev/null || echo "$tree")"
		out_win="$(cygpath -w "$out" 2>/dev/null || echo "$out")"
		powershell.exe -NoProfile -Command \
			"Compress-Archive -Path '$tree_win\\*' -DestinationPath '$out_win\\viewwstudio-windows-x86_64.zip' -Force"
	elif command -v zip >/dev/null 2>&1; then
		(cd "$out" && zip -qr "viewwstudio-windows-x86_64.zip" "viewwstudio-$version")
	else
		echo "    no zip and no powershell: shipping the tree unarchived"
	fi
	[ -f "$out/viewwstudio-windows-x86_64.zip" ] && {
		echo "==> $out/viewwstudio-windows-x86_64.zip"
		du -sh "$out/viewwstudio-windows-x86_64.zip" | awk '{print "    " $1}'
	}

	[ -n "$split_sdk" ] && archive_sdk "$sdk"
	[ -n "$make_msi" ] && make_msi "$tree"
	echo "    unsigned: Authenticode signing needs a certificate this script does not have"
	;;
esac

# ---------------------------------------------------------------- the checksums
#
# **SHA256SUMS over every release asset (ST-14).** The download page has
# linked `releases/latest/download/SHA256SUMS` since it was written, and
# until this block existed nothing anywhere produced the file - a link to a
# checksum nobody wrote is worse than no link. The studio's updater reads it
# before anything it fetched is applied; a person can read it by hand.
#
# GNU `sha256sum` where it exists (Linux, Git Bash), `shasum -a 256` on a
# macOS without coreutils - the two line shapes the updater's parser
# accepts, so which one wrote the file is a fact about the build machine and
# nothing else. Every `viewwstudio-*` file in the output directory, and
# nothing else: index.html is not checksummed (it links the checksums), and
# neither is SHA256SUMS itself.
(
	cd "$out"
	assets="$(find . -maxdepth 1 -type f -name 'viewwstudio-*' | sed 's|^\./||' | sort)"
	if [ -n "$assets" ]; then
		if command -v sha256sum >/dev/null 2>&1; then
			printf '%s\n' "$assets" | xargs sha256sum --
		else
			printf '%s\n' "$assets" | xargs shasum -a 256
		fi
	fi
) >"$out/SHA256SUMS"
[ -s "$out/SHA256SUMS" ] && {
	echo "==> $out/SHA256SUMS"
	echo "    $(wc -l <"$out/SHA256SUMS" | tr -d ' 	') asset(s)"
}

# ---------------------------------------------------------------- the site
#
# **`packaging/site/index.html` contains nine `__REPO__` placeholders and, until
# this block existed, nothing anywhere replaced any of them.** The download page
# — the one artefact whose entire job is to link to the release this script just
# built — shipped with every download link, the checksums link, the source link
# and the issues link pointing at `github.com/__REPO__/…`. Nine dead links on
# the first page a new user sees.
#
# The repository is not in `Cargo.toml`: the workspace manifest omits
# `repository` deliberately (see its own comment — an unverified wrong URL on
# twelve crate pages is worse than none). So it comes from the environment.
# `GITHUB_REPOSITORY` is set by every GitHub Actions run for free; `VIEWW_REPO`
# is the manual override for a release cut from a laptop.
#
# **With neither set, the page is not written at all.** Rendering it with the
# placeholders left in would produce exactly the artefact this block exists to
# stop, and rendering it with a guess would produce a page that looks right and
# 404s. An absent page is the only one of the three that cannot mislead anybody,
# and the message says how to get the real one.
# **Third: ask the checkout** - `repo_from_git`, defined above the build
# alongside the precedence order, because the first consumer of the answer
# is the build itself (ST-14 stamps it into the binary). The failure mode of
# forgetting a variable was a release with no download page at all; now the
# same resolution answers the page, the stamp, and nothing else.
#
if [ -n "$repo" ]; then
	sed "s|__REPO__|$repo|g" "$root/packaging/site/index.html" > "$out/index.html"
	echo "==> $out/index.html"
	echo "    links point at github.com/$repo"
	# Belt and braces: a placeholder that survives the substitution means a new
	# one was added in a form this `sed` does not match, and a silently broken
	# download page is the whole point of the block above.
	if grep -q "__REPO__" "$out/index.html"; then
		echo "    ERROR: __REPO__ still present after substitution" >&2
		exit 1
	fi
else
	echo "    no download page: this is not a GitHub clone and neither"
	echo "    VIEWW_REPO=owner/name nor GITHUB_REPOSITORY is set, so there is no"
	echo "    repository to point packaging/site/index.html at"
fi

rm -rf "$out/icons"
echo "done"
