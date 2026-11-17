#!/usr/bin/env python3
"""Pick Firebase Test Lab phones for Market Watch: high refresh rate first.

    gcloud firebase test android models list --format=json > models.json
    pick_devices.py models.json [--count N] [--only "id:version,id:version"]
    pick_devices.py --self-test

Prints one `model=ID,version=V,orientation=portrait` per line, for
`gcloud firebase test android run --device`. Physical phones only, each at
its newest Android version. Models on PREFERRED come first, in its order: the
90 and 120 Hz panels the cadence work is about. Then any other physical phone,
newest Android first, so a catalog that has dropped them all still runs.
`--only` names the phones and skips the choosing (a repository variable,
FIREBASE_DEVICES, sets it in CI).
"""

import argparse
import json
import sys

# Firebase catalog ids, highest refresh rate and newest first. Ids missing
# from a given catalog are skipped.
PREFERRED = [
    "tokay", "caiman", "komodo", "comet",   # Pixel 9, 9 Pro, 9 Pro XL, 9 Pro Fold (120 Hz)
    "shiba", "husky", "akita",              # Pixel 8, 8 Pro, 8a (120 Hz)
    "e3q", "e2s", "e1q",                    # Galaxy S24 Ultra, S24+, S24 (120 Hz)
    "dm3q", "dm1q",                         # Galaxy S23 Ultra, S23 (120 Hz)
    "cheetah", "lynx",                      # Pixel 7 Pro (120 Hz), 7a (90 Hz)
    "b0q",                                  # Galaxy S22 Ultra (120 Hz)
    "redfin",                               # Pixel 5 (90 Hz)
]


def newest(versions):
    def key(v):
        try:
            return int(v)
        except ValueError:
            return -1
    return max(versions, key=key) if versions else None


def pick(models, count, only=""):
    if only.strip():
        out = []
        for item in only.split(","):
            model, _, version = item.strip().partition(":")
            if model:
                out.append(f"model={model}" + (f",version={version}" if version else "") + ",orientation=portrait")
        return out[:count]
    physical = {m["id"]: m for m in models
                if m.get("form") == "PHYSICAL" and m.get("supportedVersionIds")}
    order = [i for i in PREFERRED if i in physical]
    rest = sorted((i for i in physical if i not in order),
                  key=lambda i: -int(newest(physical[i]["supportedVersionIds"]) or 0))
    out = []
    for i in order + rest:
        v = newest(physical[i]["supportedVersionIds"])
        if v:
            out.append(f"model={i},version={v},orientation=portrait")
        if len(out) == count:
            break
    return out


def self_test():
    models = [
        {"id": "redfin", "form": "PHYSICAL", "supportedVersionIds": ["30"]},
        {"id": "shiba", "form": "PHYSICAL", "supportedVersionIds": ["34", "33"]},
        {"id": "MediumPhone.arm", "form": "VIRTUAL", "supportedVersionIds": ["34"]},
        {"id": "a10", "form": "PHYSICAL", "supportedVersionIds": ["29"]},
        {"id": "x1q", "form": "PHYSICAL", "supportedVersionIds": ["33"]},
    ]
    assert pick(models, 2) == ["model=shiba,version=34,orientation=portrait",
                               "model=redfin,version=30,orientation=portrait"], pick(models, 2)
    assert pick(models, 4)[2:] == ["model=x1q,version=33,orientation=portrait",
                                   "model=a10,version=29,orientation=portrait"], pick(models, 4)
    assert pick(models, 3, "husky:34, oriole") == ["model=husky,version=34,orientation=portrait",
                                                    "model=oriole,orientation=portrait"]
    print("pick_devices self-test: ok")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("models", nargs="?")
    ap.add_argument("--count", type=int, default=3)
    ap.add_argument("--only", default="")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args(argv)
    if a.self_test:
        self_test()
        return 0
    models = json.load(open(a.models)) if a.models and not a.only.strip() else []
    devices = pick(models, a.count, a.only)
    print("\n".join(devices))
    return 0 if devices else 1


if __name__ == "__main__":
    sys.exit(main())
