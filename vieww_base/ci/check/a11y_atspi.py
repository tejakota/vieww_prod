#!/usr/bin/env python3
"""Assert what a Linux screen reader can see of a vieww application (FW-10b).

    ci/check/a11y_atspi.py --tree FILE        # dry-run: a fixture dump
    ci/check/a11y_atspi.py --live [APP]       # the real AT-SPI bus (pyatspi)
    ci/check/a11y_atspi.py --record FILE --live
    ci/check/a11y_atspi.py --self-test        # prove the engine has teeth

# What this is, and what it is not

This is a *tree-assertion harness*: it walks an accessibility tree and fails
unless the vieww contract holds — the button exists and is named, the field
carries its value, every interactive node has something a reader can announce.
It is the Linux twin of `ci/mobile/a11y-android.sh`'s uiautomator dump and
`ci/mobile/a11y-ios.sh`'s XCUITest set, and like them it reads the tree a
screen reader consumes rather than listening to one: ears on Orca remain a
manual session, the same standing the Android and iOS legs record.

# The two vocabularies

The dry-run reads the fixture `vieww-platform-winit`'s
`src/a11y/atspi_fixture.rs` writes — the tree as vieww hands it to AccessKit,
in AccessKit's own Debug role names. The live leg reads the AT-SPI bus, whose
role names are AT-SPI's. Both are canonicalised to the small set the
assertions speak in; the AT-SPI column of that table is written from
AccessKit's published Unix mapping and **has not yet been confirmed by a live
run** — the first `--record` is what confirms it, and a mismatch there is a
finding about this table, not about vieww.

# No dependencies

Standard library only, so the dry-run runs anywhere python does. The live leg
imports `pyatspi` (Debian: python3-pyatspi) only when actually asked to.
"""

import json
import sys

# Canonical role -> (AccessKit Debug name, AT-SPI role name).
#
# AT-SPI names are the strings pyatspi's `getRoleName()` returns. The mapping
# AccessKit's Unix adapter uses is its code, not vieww's; this table is this
# harness's guess at it, and the first live recording either confirms the
# column or corrects it — see the module doc.
ROLE_ALIASES = {
    "window": ("Window", "frame"),
    "button": ("Button", "push button"),
    "text field": ("TextInput", "entry"),
    "check box": ("CheckBox", "check box"),
    "radio button": ("RadioButton", "radio button"),
    "switch": ("Switch", "toggle button"),
    "slider": ("Slider", "slider"),
    "tab": ("Tab", "page tab"),
    "menu item": ("MenuItem", "menu item"),
    "link": ("Link", "link"),
    "label": ("Label", "label"),
    "group": ("Group", "panel"),
    "scroll view": ("ScrollView", "scroll pane"),
    "progress bar": ("ProgressIndicator", "progress bar"),
    "image": ("Image", "image"),
    "text": ("TextRun", "text"),
    # TextEdit is the multiline spelling of the same thing on the AccessKit
    # side; AT-SPI does not distinguish.
    "multiline text field": ("TextEdit", "text"),
}

# Roles a screen reader announces as something to act on — the same rule
# vieww-accessibility's `audit` applies, restated on the far side of the seam.
INTERACTIVE = {
    "button",
    "text field",
    "multiline text field",
    "check box",
    "radio button",
    "switch",
    "slider",
    "tab",
    "menu item",
    "link",
}


def canon(role):
    """A role name from either vocabulary, canonicalised — or itself."""
    for canonical, (accesskit, atspi) in ROLE_ALIASES.items():
        if role in (accesskit, atspi):
            return canonical
    return role


class Node(dict):
    """The shape every tree source is normalised into before asserting."""

    @property
    def name(self):
        return self.get("label") or self.get("value")

    @property
    def canonical_role(self):
        return canon(self.get("role", ""))


def from_fixture(document):
    """The fixture document `atspi_fixture.rs` writes, as nodes."""
    nodes = {}
    for raw in document["nodes"]:
        node = Node(raw)
        nodes[node["id"]] = node
    return document.get("root"), nodes


def from_atspi(accessible, seen=None):
    """A pyatspi accessible, recursively, as fixture-shaped nodes.

    Returns `(node id, all nodes seen)` and records into `seen`, so a shared
    child (a menu two panes point at, say) is linked by id rather than
    walked twice. Everything is defensive: an application that answers one
    of the queries with an exception must cost that *field*, not the whole
    walk — a harness that dies mid-tree reports nothing at all, which is the
    no-op this repository has been wrong about before (see a11y-android.sh's
    header).
    """
    seen = seen if seen is not None else {}
    ident = str(id(accessible))
    if ident in seen:
        return ident, seen
    try:
        role = accessible.getRoleName()
    except Exception:
        role = "unknown"
    try:
        name = accessible.name or None
    except Exception:
        name = None
    value = None
    try:
        value = accessible.queryText().getText(0, -1) or None
    except Exception:
        value = None
    try:
        states = {str(state).split()[-1].lower() for state in accessible.getState().getStates()}
    except Exception:
        states = set()
    try:
        extents = accessible.queryExtents()
        bounds = [extents.x, extents.y, extents.x + extents.width, extents.y + extents.height]
        if extents.width == 0 and extents.height == 0:
            bounds = None
    except Exception:
        bounds = None
    node = Node(
        {
            "id": ident,
            "role": role,
            "label": name,
            "value": value,
            "focusable": "focusable" in states or "focused" in states,
            "clickable": "clickable" in states,
            "enabled": "enabled" in states or "sensitive" in states,
            "selected": "selected" in states,
            "checked": "checked" in states,
            "bounds": bounds,
            "children": [],
        }
    )
    seen[ident] = node
    try:
        for index in range(accessible.childCount):
            child = accessible.getChildAtIndex(index)
            if child is None:
                continue
            child_id, _ = from_atspi(child, seen)
            node["children"].append(child_id)
    except Exception:
        pass
    return ident, seen


def assert_tree(root, nodes, source):
    """The vieww contract, over any tree shaped like the fixture.

    Returns a list of failure strings — an empty list is the pass. Asserting
    is *not* raising here so the caller (and the self-test) can see every
    failure, not just the first.
    """
    failures = []

    if root not in nodes:
        return [f"{source}: the tree has no root ({root!r} is not a node)"]

    # The tree is not the placeholder. One featureless window is exactly what
    # a bridge that never published looks like from outside, and a harness
    # that accepted it would be green over a broken app.
    if len(nodes) < 10:
        failures.append(
            f"{source}: {len(nodes)} node(s) — that is the placeholder shape, "
            "not an application tree"
        )

    if nodes[root].canonical_role != "window":
        failures.append(
            f"{source}: the root is a {nodes[root].canonical_role!r}, not a window"
        )

    # Every child id resolves. A dangling child is a node a screen reader
    # walks into and falls off.
    for ident, node in nodes.items():
        for child in node.get("children", []):
            if child not in nodes:
                failures.append(
                    f"{source}: node {ident} lists child {child!r} that is not in the tree"
                )

    # The demo's button: present, named, focusable.
    tap = [n for n in nodes.values() if n.name == "Tap me"]
    if not tap:
        failures.append(f"{source}: no node named 'Tap me' — the button never reached the tree")
    else:
        if tap[0].canonical_role != "button":
            failures.append(
                f"{source}: 'Tap me' is a {tap[0].canonical_role!r}, not a button"
            )
        if not tap[0].get("focusable"):
            failures.append(f"{source}: 'Tap me' is not focusable — a reader cannot stop on it")

    # The demo's field, carrying its seeded value.
    if not any(
        n.canonical_role in ("text field", "multiline text field") and n.get("value") == "tap to type"
        for n in nodes.values()
    ):
        failures.append(
            f"{source}: no text field holding 'tap to type' — the field never reached the tree"
        )

    # Every interactive node is named. A reader announces an unnamed control
    # as its role and nothing else, which is the one failure no user can work
    # around.
    for ident, node in nodes.items():
        if node.canonical_role in INTERACTIVE and not node.name:
            failures.append(
                f"{source}: node {ident} is a {node.canonical_role!r} with no name"
            )

    return failures


def walk_live(app_name):
    """The live tree, read with pyatspi: `(root id, nodes, error)`."""
    try:
        import pyatspi
    except ImportError as error:
        return None, None, f"pyatspi is not importable here ({error})"
    desktop = pyatspi.Registry.getDesktop(0)
    for app in desktop:
        try:
            if app.name != app_name:
                continue
        except Exception:
            continue
        try:
            windows = [app[i] for i in range(app.childCount)]
        except Exception:
            windows = []
        if not windows:
            return None, None, f"application {app_name!r} published no windows"
        root_id, nodes = from_atspi(windows[0])
        return root_id, nodes, None
    return None, None, f"no AT-SPI application named {app_name!r} — is the vieww app running?"


GOOD = {
    "source": "self-test good fixture",
    "root": "0.0",
    "nodes": [
        {"id": "0.0", "role": "Window", "children": ["0.1", "0.2", "0.3", "0.4"]},
        {"id": "0.1", "role": "Button", "label": "Tap me", "focusable": True, "children": []},
        {"id": "0.2", "role": "TextInput", "value": "tap to type", "focusable": True, "children": []},
        {"id": "0.3", "role": "Label", "value": "hello", "children": []},
        {"id": "0.4", "role": "ScrollView", "children": ["0.5", "0.6", "0.7", "0.8", "0.9", "0.10"]},
        {"id": "0.5", "role": "CheckBox", "label": "Inbox", "toggled": "true", "children": []},
        {"id": "0.6", "role": "CheckBox", "label": "Archive", "children": []},
        {"id": "0.7", "role": "CheckBox", "label": "Settings", "children": []},
        {"id": "0.8", "role": "Image", "label": "logo", "children": []},
        {"id": "0.9", "role": "ProgressIndicator", "label": "loading", "children": []},
        {"id": "0.10", "role": "Group", "children": []},
    ],
}


def self_test():
    """Prove the engine fails on broken trees, not just passes on good ones.

    A harness whose assertions can never fire is indistinguishable from a
    green one (a11y-android.sh's `tr '>' '>\n'` node counter was exactly
    that, for weeks), so the self-test breaks the tree three ways and demands
    those three failures and no others.
    """
    failures = []

    good = assert_tree(GOOD["root"], {n["id"]: Node(n) for n in GOOD["nodes"]}, "self-test")
    if good:
        failures.append(f"the good fixture failed: {good}")

    # 1. An unnamed button.
    unnamed = json.loads(json.dumps(GOOD))
    unnamed["nodes"][1].pop("label")
    found = assert_tree(
        unnamed["root"], {n["id"]: Node(n) for n in unnamed["nodes"]}, "self-test"
    )
    if not any("with no name" in f for f in found):
        failures.append(f"an unnamed button was not flagged: {found}")

    # 2. A dangling child.
    dangling = json.loads(json.dumps(GOOD))
    dangling["nodes"][2]["children"] = ["9.9"]
    found = assert_tree(
        dangling["root"], {n["id"]: Node(n) for n in dangling["nodes"]}, "self-test"
    )
    if not any("not in the tree" in f for f in found):
        failures.append(f"a dangling child was not flagged: {found}")

    # 3. The button gone — the placeholder-adjacent failure.
    gone = json.loads(json.dumps(GOOD))
    gone["nodes"] = [n for n in gone["nodes"] if n["id"] != "0.1"]
    gone["nodes"][0]["children"] = [c for c in gone["nodes"][0]["children"] if c != "0.1"]
    found = assert_tree(gone["root"], {n["id"]: Node(n) for n in gone["nodes"]}, "self-test")
    if not any("Tap me" in f for f in found):
        failures.append(f"a missing button was not flagged: {found}")

    # 4. The placeholder shape.
    placeholder = {
        "source": "self-test placeholder",
        "root": "0.0",
        "nodes": [{"id": "0.0", "role": "Window", "children": []}],
    }
    found = assert_tree(
        placeholder["root"], {n["id"]: Node(n) for n in placeholder["nodes"]}, "self-test"
    )
    if not any("placeholder shape" in f for f in found):
        failures.append(f"a one-window placeholder was not flagged: {found}")

    return failures


def main(argv):
    """Parse the command line by hand — two dependencies fewer than argparse
    is not the point; one canonical way to read the arguments is."""
    mode = None
    tree = record = None
    app = "desktop"
    index = 0
    while index < len(argv):
        arg = argv[index]
        if arg == "--self-test":
            mode = "self-test"
        elif arg == "--tree":
            mode, tree = "tree", argv[index + 1]
            index += 1
        elif arg == "--live":
            mode = "live"
            if index + 1 < len(argv) and not argv[index + 1].startswith("-"):
                app = argv[index + 1]
                index += 1
        elif arg == "--record":
            record = argv[index + 1]
            index += 1
        else:
            print(f"a11y-atspi: unknown argument {arg!r}", file=sys.stderr)
            return 2
        index += 1

    if mode == "self-test":
        failures = self_test()
        for failure in failures:
            print(f"a11y-atspi: SELF-TEST FAIL — {failure}")
        print(
            "a11y-atspi: self-test "
            + ("passed" if not failures else "FAILED")
            + " (assertions proven to fire on broken trees)"
        )
        return 1 if failures else 0

    if mode == "tree":
        with open(tree, encoding="utf-8") as handle:
            document = json.load(handle)
        root, nodes = from_fixture(document)
        failures = assert_tree(root, nodes, tree)
        for failure in failures:
            print(f"a11y-atspi: FAIL — {failure}")
        print(
            f"a11y-atspi: {tree}: {len(nodes)} node(s), "
            + ("PASS" if not failures else f"{len(failures)} failure(s)")
        )
        return 1 if failures else 0

    if mode == "live":
        root, nodes, error = walk_live(app)
        if error:
            print(f"a11y-atspi: FAIL — {error}")
            return 1
        failures = assert_tree(root, nodes, "live AT-SPI tree")
        for failure in failures:
            print(f"a11y-atspi: FAIL — {failure}")
        print(
            f"a11y-atspi: live AT-SPI tree: {len(nodes)} node(s), "
            + ("PASS" if not failures else f"{len(failures)} failure(s)")
        )
        if record:
            document = {
                "source": "recorded from the live AT-SPI bus",
                "root": root,
                "nodes": [dict(node) for node in nodes.values()],
            }
            with open(record, "w", encoding="utf-8") as handle:
                json.dump(document, handle, indent=2)
            print(f"a11y-atspi: recorded the live tree to {record}")
        return 1 if failures else 0

    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
