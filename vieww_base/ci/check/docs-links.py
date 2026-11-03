#!/usr/bin/env python3
"""Every relative link in the shipped docs has to point somewhere (FW-18c).

    ci/check/docs-links.py            # all markdown roots, one pass
    ci/check/docs-links.py DIR ...    # your own roots (for trying it out)

`cargo doc` already refuses broken *intra-doc* links (`checks.sh`'s docs
stage), which is why that stage exists at all. The markdown trees have no
equivalent gate: a guide page links `./layout.md`, the architecture pages
cross-link, `docs/guide/README.md` promised a page that was not there for
months — and nothing failed, because nothing asked. This is the ask.

# What it checks, and what it deliberately does not

Relative link targets in inline `[text](target)` links, resolved against the
linking file's own directory: the file has to exist. Nothing else, and each
deliberate narrowing below is a false positive this trees' prose would
otherwise generate:

- **Fenced code blocks and inline code spans are skipped.** The docs are
  full of `Signal<T>` and `wright fork <run>` in prose and tables; an angle
  bracket is not a link here, so `<...>` autolinks are not read at all.
- **Anchors (`#section`) are skipped.** There is no markdown outline parser
  in stdlib, and a half-right anchor check is worse than none.
- **`http(s)://`, `mailto:` are skipped.** No network in CI, and an offline
  DNS failure is not a docs regression.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
CODE_SPAN = re.compile(r"`[^`]*`")
SKIP = ("http://", "https://", "mailto:", "#")


def check_root(root: Path) -> list[str]:
    """One markdown root; returns a list of complaint lines (empty = clean)."""
    complaints = []
    for doc in sorted(root.rglob("*.md")):
        fence = False
        for number, line in enumerate(doc.read_text(encoding="utf-8").splitlines(), 1):
            if line.lstrip().startswith("```"):
                fence = not fence
                continue
            if fence:
                continue
            for match in LINK.finditer(CODE_SPAN.sub("`", line)):
                target = match.group(1)
                if target.startswith(SKIP):
                    continue
                path = target.split("#", 1)[0]
                if path and not (doc.parent / path).resolve().exists():
                    complaints.append(f"{doc}:{number}: {target} -> no such file")
    return complaints


def main(argv: list[str]) -> int:
    base = Path(__file__).resolve().parent.parent.parent  # .../vieww_base
    roots = [Path(a).resolve() for a in argv[1:]] or [
        base / "docs",
        base.parent / "docs",
    ]
    complaints = [c for root in roots if root.is_dir() for c in check_root(root)]
    for complaint in complaints:
        print(f"docs-links: {complaint}")
    if complaints:
        return 1
    print(f"docs-links: every relative link in {len(roots)} root(s) resolves")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
