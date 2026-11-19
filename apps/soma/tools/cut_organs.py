#!/usr/bin/env python3
"""Cut each organ out of its illustration, for the 3D organ layer.

The illustrations are labelled diagrams: text, leader lines, insets and
sometimes a whole torso around the organ. `vieww_mesh::relief::silhouette_solid`
grows geometry from a picture's alpha, so what it is handed must be the organ
alone. For each organ this keeps the pixels close to the organ's own colour
inside a crop box, takes the largest connected shape, closes and fills it,
and writes an RGBA PNG (alpha = the organ) to `assets/organs_cut/<key>.png`.

Deterministic: same inputs, same bytes. Run from anywhere:

    python3 -I apps/soma/tools/cut_organs.py            # writes the PNGs
    python3 -I apps/soma/tools/cut_organs.py --sheet x  # also a contact sheet

Licence: a cutout is a derivative of its source picture. Swap the sources in
`assets/organs/` for licensed or public-domain art and re-run; the app code
does not change.
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

HERE = Path(__file__).resolve().parent
SRC = HERE / "../assets/organs"
OUT = HERE / "../assets/organs_cut"

# key -> (crop box as fractions x0, y0, x1, y1; target RGB or "bg"; tolerance)
# "bg" keeps everything that is neither the paper nor a label (text and
# leader lines are thin and colourless) - for a drawing that is one object
# on a plain ground. A colour keeps pixels near the organ's own hue - for an
# organ drawn inside a larger scene.
# A key missing here keeps the procedural organ (no usable picture: lungs.jpg
# is a photograph of a clinic visit, and the thyroid is a few pixels of a
# neck drawing - too small to cut cleanly).
ORGANS = {
    "heart":        ((0.10, 0.08, 0.75, 0.86), "bg", 0),
    "liver":        ((0.02, 0.06, 0.98, 0.58), (150, 60, 45), 80),
    "stomach":      ((0.20, 0.08, 0.90, 0.95), (225, 150, 140), 70),
    "kidneys":      ((0.30, 0.38, 0.98, 0.90), (205, 120, 120), 90),
    "brain":        ((0.00, 0.10, 0.55, 0.95), (230, 140, 130), 75),
    "pancreas":     ((0.42, 0.25, 0.92, 0.75), (235, 200, 130), 55),
    "liver_gall":   ((0.25, 0.30, 0.55, 0.70), (120, 150, 60), 70),
    "intestines":   ((0.30, 0.45, 0.85, 0.85), (225, 130, 160), 48),
    "spine":        ((0.08, 0.12, 0.40, 0.95), (225, 205, 170), 55),
    "spleen":       ((0.45, 0.05, 0.98, 0.95), (125, 50, 75), 80),
}

# Which source file each key reads (the database key and the file name
# differ for the gallbladder).
FILE = {"liver_gall": "gallbladder"}


def cut(key, spec):
    (x0, y0, x1, y1), target, tol = spec
    im = Image.open(SRC / f"{FILE.get(key, key)}.jpg").convert("RGB")
    w, h = im.size
    box = (int(x0 * w), int(y0 * h), int(x1 * w), int(y1 * h))
    im = im.crop(box)
    px = np.asarray(im).astype(np.int32)
    chroma = px.max(axis=2) - px.min(axis=2)
    if target == "bg":
        paper = np.median(np.concatenate([px[0], px[-1], px[:, 0], px[:, -1]]), axis=0)
        off_paper = np.sqrt(((px - paper) ** 2).sum(axis=2)) > 30
        mask = off_paper & (chroma > 28)
    else:
        dist = np.sqrt(((px - np.array(target)) ** 2).sum(axis=2))
        mask = (dist < tol) & (chroma > 22)
    mask = ndimage.binary_closing(mask, iterations=3)
    mask = ndimage.binary_opening(mask, iterations=1)
    labels, n = ndimage.label(mask)
    if n == 0:
        raise SystemExit(f"{key}: nothing matched")
    sizes = ndimage.sum(mask, labels, range(1, n + 1))
    mask = labels == (int(np.argmax(sizes)) + 1)
    mask = ndimage.binary_fill_holes(mask)
    ys, xs = np.nonzero(mask)
    pad = 4
    b = (max(xs.min() - pad, 0), max(ys.min() - pad, 0),
         min(xs.max() + pad + 1, mask.shape[1]), min(ys.max() + pad + 1, mask.shape[0]))
    alpha = Image.fromarray((mask * 255).astype(np.uint8)).crop(b)
    rgb = im.crop(b)
    # Cap the side: the relief grid samples at ~64 cells; a texture past
    # 256 px buys nothing on a 3D organ a few centimetres tall.
    scale = 256 / max(rgb.size)
    if scale < 1:
        size = (max(1, round(rgb.size[0] * scale)), max(1, round(rgb.size[1] * scale)))
        rgb = rgb.resize(size, Image.LANCZOS)
        alpha = alpha.resize(size, Image.LANCZOS)
    out = rgb.convert("RGBA")
    out.putalpha(alpha)
    return out, float(mask.mean())


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    sheet = []
    for key, spec in ORGANS.items():
        img, cover = cut(key, spec)
        img.save(OUT / f"{key}.png", optimize=True)
        print(f"{key:12} {img.size[0]:4}x{img.size[1]:<4} coverage {cover:.2f}")
        sheet.append(img)
    if "--sheet" in sys.argv:
        path = sys.argv[sys.argv.index("--sheet") + 1]
        cell = 200
        canvas = Image.new("RGBA", (cell * 4, cell * ((len(sheet) + 3) // 4)), (20, 26, 40, 255))
        for i, img in enumerate(sheet):
            t = img.copy()
            t.thumbnail((cell - 10, cell - 10))
            canvas.alpha_composite(t, ((i % 4) * cell + 5, (i // 4) * cell + 5))
        canvas.save(path)


if __name__ == "__main__":
    main()
