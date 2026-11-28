#!/usr/bin/env python3
"""The one release profile every app is built with (BLD-01).

    python3 ci/lib/app_profile.py --check   # exit 1 if any app differs
    python3 ci/lib/app_profile.py --write   # bring every app to the block

The framework measures `dist` (fat LTO, panic = abort, one codegen unit);
before this, three had no release profile at all, snapsearch / upxcale /
vavlt built with thin LTO, and only soma had `dist`, so the numbers the
framework publishes were not the numbers an app ships. Cargo profiles cannot
be inherited across workspaces, so the block is copied, between markers, into
each app's Cargo.toml, and `--check` (run by ci/tests) keeps the copies equal.

The vieww crates are non-members of an app workspace, so a blanket
`"*" = "s"` would size-optimise the rasteriser. The pixel-facing crates are
named and keep `opt-level = 3`; everything else is `"s"`.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
APPS = ["snapsearch", "upxcale", "vavlt", "three", "soma", "twin"]
BEGIN = "# >>> vieww app dist profile (ci/lib/app_profile.py; do not edit by hand) >>>"
END = "# <<< vieww app dist profile <<<"
BLOCK = f'''{BEGIN}
[profile.dist]
inherits = "release"
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
debug = false
strip = "symbols"

[profile.dist.package."*"]
opt-level = "s"
[profile.dist.package.vieww-paint]
opt-level = 3
[profile.dist.package.vieww-hal]
opt-level = 3
[profile.dist.package.vieww-text]
opt-level = 3
[profile.dist.package.vieww-effects]
opt-level = 3
{END}
'''


def strip_old(text):
    """Remove an earlier hand-written [profile.dist] table and our block."""
    text = re.sub(re.escape(BEGIN) + r".*?" + re.escape(END) + r"\n?", "", text, flags=re.S)
    return re.sub(r"\n\[profile\.dist\]\n(?:(?!\[)[^\n]*\n)*", "\n", text)


def apply(text):
    return strip_old(text).rstrip("\n") + "\n\n" + BLOCK


def main(argv):
    mode = argv[0] if argv else "--check"
    bad = []
    for app in APPS:
        path = ROOT / "apps" / app / "Cargo.toml"
        text = path.read_text()
        want = apply(text)
        if text != want:
            bad.append(app)
            if mode == "--write":
                path.write_text(want)
    if mode == "--check" and bad:
        print("app dist profile differs in: " + ", ".join(bad) + " (run --write)")
        return 1
    if mode == "--write":
        print("updated: " + (", ".join(bad) or "nothing"))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
