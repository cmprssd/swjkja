"""Compares the new renderer's views with the reference screenshots.

    python compare.py <reference folder> <ours folder> <out folder>

For each <name>.png in both folders: the mean absolute difference (0-255, RGB), the
share of pixels off by more than 16 in any channel, and a composite
<out>/<name>.jpg: reference | ours | difference x4. Pixels the new renderer left at
the clear colour (pure black: sky, not drawn yet) are reported separately and left
out of the other numbers.
"""
import pathlib
import sys

import numpy as np
from PIL import Image

ref_dir, ours_dir, out_dir = (pathlib.Path(a) for a in sys.argv[1:4])
out_dir.mkdir(parents=True, exist_ok=True)
print(f"{'view':16} {'mean diff':>9} {'>16 off':>8} {'undrawn':>8}")
for ours_path in sorted(ours_dir.glob("*.png")):
    ref_path = ref_dir / ours_path.name
    if not ref_path.exists():
        print(f"{ours_path.stem:16} (no reference)")
        continue
    ref = np.asarray(Image.open(ref_path).convert("RGB")).astype(np.int16)
    ours = np.asarray(Image.open(ours_path).convert("RGB")).astype(np.int16)
    if ref.shape != ours.shape:
        print(f"{ours_path.stem:16} size {ref.shape} vs {ours.shape}")
        continue
    undrawn = (ours == 0).all(axis=2)
    diff = np.abs(ref - ours)
    drawn = ~undrawn
    mean = diff[drawn].mean() if drawn.any() else 0.0
    off = (diff.max(axis=2) > 16)[drawn].mean() * 100 if drawn.any() else 0.0
    print(f"{ours_path.stem:16} {mean:9.2f} {off:7.1f}% {undrawn.mean() * 100:7.1f}%")
    heat = np.clip(diff * 4, 0, 255).astype(np.uint8)
    composite = np.concatenate([ref.astype(np.uint8), ours.astype(np.uint8), heat], axis=1)
    Image.fromarray(composite).resize((composite.shape[1] // 2, composite.shape[0] // 2)).save(out_dir / f"{ours_path.stem}.jpg", quality=85)
