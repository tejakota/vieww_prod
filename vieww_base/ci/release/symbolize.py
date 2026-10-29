"""Turn a crash report's raw frames back into functions and lines (FW-12).

    python3 symbolize.py REPORT SYMBOLS_DIR

REPORT is a crash file (`vieww_foundation::crash::FileSink`) whose `frame:`
lines read `#3 0x… libmain.so+0x2b4f10 build-id=5f1c…`. SYMBOLS_DIR is what
`ci/release/symbols.sh` wrote: per binary, its debug info under the id the
report names (`<id>.debug` for ELF, `<id>.dSYM` for Mach-O, `<id>/` with the
.exe/.dll and .pdb for PE) and `<id>.json` with the format and the load
address a `module+offset` is relative to. Each frame with symbols is
resolved with llvm-symbolizer (any format), or addr2line (ELF) or atos
(Mach-O, on a Mac); the rest are printed as they came. Standard library only.
"""

import glob
import json
import os
import re
import shutil
import subprocess
import sys

FRAME = re.compile(r"^frame: #(\d+) (0x[0-9a-f]+) (.+)\+(0x[0-9a-f]+) build-id=([0-9a-f]+|unknown)\s*$")


def frames(report):
    out = []
    for line in report.splitlines():
        if not line:
            break  # the backtrace follows the blank line
        m = FRAME.match(line)
        if m:
            out.append((int(m.group(1)), m.group(2), m.group(3), m.group(4), m.group(5)))
    return out


def symbols_for(root, build_id):
    """(format, object to look up in, load address) for an id, or None."""
    meta = {}
    path = os.path.join(root, f"{build_id}.json")
    if os.path.exists(path):
        with open(path) as f:
            meta = json.load(f)
    base = int(meta.get("base", 0))
    elf = os.path.join(root, f"{build_id}.debug")
    if os.path.exists(elf):
        return "elf", elf, base
    dsym = os.path.join(root, f"{build_id}.dSYM")
    if os.path.isdir(dsym):
        dwarf = sorted(glob.glob(os.path.join(dsym, "Contents", "Resources", "DWARF", "*")))
        if dwarf:
            return "macho", dwarf[0], base
    pe = os.path.join(root, build_id)
    if os.path.isdir(pe):
        images = [p for p in sorted(os.listdir(pe)) if p.lower().endswith((".exe", ".dll"))]
        if images:
            return "pe", os.path.join(pe, images[0]), base
    return None


def pairs(lines):
    lines = [x for x in lines if x.strip()]
    return [(lines[i], lines[i + 1]) for i in range(0, len(lines) - 1, 2)]


def symbolizer():
    tool = shutil.which("llvm-symbolizer")
    if not tool and shutil.which("xcrun"):
        # Xcode's toolchain carries one; it is not on PATH by default.
        r = subprocess.run(["xcrun", "-f", "llvm-symbolizer"], capture_output=True, text=True)
        tool = r.stdout.strip() if r.returncode == 0 and r.stdout.strip() else None
    return tool


def resolve(fmt, obj, address):
    tool = symbolizer()
    if tool:
        r = subprocess.run([tool, "--obj", obj, "--functions=linkage", "--demangle", "--inlining", hex(address)],
                           capture_output=True, text=True)
        got = pairs(r.stdout.splitlines())
        if got and not got[0][0].startswith("??"):
            return got
    if fmt == "elf" and shutil.which("addr2line"):
        r = subprocess.run(["addr2line", "-e", obj, "-f", "-C", "-i", hex(address)], capture_output=True, text=True)
        return pairs(r.stdout.splitlines())
    if fmt == "macho" and shutil.which("atos"):
        # `atos -l` takes the load address; the DWARF's own __TEXT address
        # is the base, so addresses pass through unchanged.
        base = address & ~0xFFFFFFF
        r = subprocess.run(["atos", "-o", obj, "-l", hex(base), hex(address)], capture_output=True, text=True)
        out = r.stdout.strip()
        m = re.match(r"^(.*) \(in [^)]*\) \(([^)]*)\)$", out)
        if m:
            return [(m.group(1), m.group(2))]
    return []


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    with open(argv[0], encoding="utf-8", errors="replace") as f:
        report = f.read()
    found = frames(report)
    if not found:
        print("symbolize: the report has no raw frames")
        return 1
    resolved = 0
    for index, ip, module, offset, build_id in found:
        sym = symbols_for(argv[1], build_id) if build_id != "unknown" else None
        # Every frame but the first holds a return address, which points
        # past the call; the call itself is one byte back (the crashpad and
        # breakpad convention). Without this a call that ends a function
        # (a panic, always) resolves to `??` or to the next function.
        off = int(offset, 16) - (0 if index == 0 else 1)
        names = resolve(sym[0], sym[1], sym[2] + off) if sym else []
        if names:
            resolved += 1
            for k, (function, where) in enumerate(names):
                lead = f"#{index:<3}" if k == 0 else "    "
                print(f"{lead} {function}  {where}  [{os.path.basename(module)}+{offset}]")
        else:
            print(f"#{index:<3} {module}+{offset}  (no symbols for build-id {build_id})  [{ip}]")
    print(f"symbolize: {resolved} of {len(found)} frame(s) resolved")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
