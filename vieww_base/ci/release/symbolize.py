"""Turn a crash report's raw frames back into functions and lines (FW-12).

    python3 symbolize.py REPORT SYMBOLS_DIR

REPORT is a crash file (`vieww_foundation::crash::FileSink`) whose `frame:`
lines read `#3 0x… libmain.so+0x2b4f10 build-id=5f1c…`. SYMBOLS_DIR holds
`<build-id>.debug` files from `ci/release/symbols.sh`. Each frame with a
debug file is resolved with llvm-symbolizer (or addr2line); the rest are
printed as they came. Standard library only.
"""

import os
import re
import shutil
import subprocess
import sys

FRAME = re.compile(r"^frame: #(\d+) (0x[0-9a-f]+) (\S+)\+(0x[0-9a-f]+) build-id=([0-9a-f]+|unknown)")


def frames(report):
    out = []
    for line in report.splitlines():
        if not line:
            break  # the backtrace follows the blank line
        m = FRAME.match(line)
        if m:
            out.append((int(m.group(1)), m.group(2), m.group(3), m.group(4), m.group(5)))
    return out


def resolve(debug, offset):
    tool = shutil.which("llvm-symbolizer")
    if tool:
        r = subprocess.run([tool, "--obj", debug, "--functions=linkage", "--demangle", "--inlining", offset],
                           capture_output=True, text=True)
        lines = [x for x in r.stdout.splitlines() if x.strip()]
        return [(lines[i], lines[i + 1]) for i in range(0, len(lines) - 1, 2)]
    tool = shutil.which("addr2line")
    if tool:
        r = subprocess.run([tool, "-e", debug, "-f", "-C", "-i", offset], capture_output=True, text=True)
        lines = r.stdout.splitlines()
        return [(lines[i], lines[i + 1]) for i in range(0, len(lines) - 1, 2)]
    return []


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    with open(argv[0]) as f:
        report = f.read()
    found = frames(report)
    if not found:
        print("symbolize: the report has no raw frames")
        return 1
    resolved = 0
    for index, ip, module, offset, build_id in found:
        debug = os.path.join(argv[1], f"{build_id}.debug")
        # Every frame but the first holds a return address, which points
        # past the call; the call itself is one byte back (the crashpad and
        # breakpad convention). Without this a call that ends a function
        # (a panic, always) resolves to `??` or to the next function.
        lookup = offset if index == 0 else hex(int(offset, 16) - 1)
        names = resolve(debug, lookup) if os.path.exists(debug) else []
        if names:
            resolved += 1
            for k, (function, where) in enumerate(names):
                lead = f"#{index:<3}" if k == 0 else "    "
                print(f"{lead} {function}  {where}  [{module}+{offset}]")
        else:
            print(f"#{index:<3} {module}+{offset}  (no symbols for build-id {build_id})  [{ip}]")
    print(f"symbolize: {resolved} of {len(found)} frame(s) resolved")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
