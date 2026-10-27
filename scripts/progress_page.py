"""Builds the progress page: the numbers of scripts/progress.py, every reference view
(the original's screenshot, swjkja's render and their difference), the same views as
each milestone's build drew them (bench/history.sh), and the videos of
bench/progress-media.sh. Game imagery stays out of the repository: the page is built
into a folder outside it and published from there as a private claude.ai artifact.

    uv run --with numpy --with pillow python scripts/progress_page.py <out> \\
        --ours <render folder>... [--work <%USERPROFILE%\\.swjkja as a WSL path>]

<work>/reference-shots holds the original's screenshots, <work>/history/<PR>-<hash>
the milestones' renders, <work>/progress-media the videos; --ours the current
renders of every reference view (later folders win).
"""
import argparse
import json
import pathlib
import shutil
import subprocess
import sys

import numpy as np
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent
WIDTH = 960  # images are shown at most this wide


def webp(img, path, quality=82):
    path.parent.mkdir(parents=True, exist_ok=True)
    img.resize((WIDTH, WIDTH * img.height // img.width), Image.LANCZOS).save(path, quality=quality)


def load(path):
    return np.asarray(Image.open(path).convert("RGB")).astype(np.int16)


def score(ref, ours):
    """Mean absolute difference over every pixel (undrawn ones included, unlike
    compare.py, so a missing sky counts against an early build), and the share of
    pixels off by more than 16."""
    diff = np.abs(ref - ours)
    return round(float(diff.mean()), 2), round(float((diff.max(axis=2) > 16).mean() * 100), 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--ours", type=pathlib.Path, nargs="+", required=True)
    ap.add_argument("--work", type=pathlib.Path, default=pathlib.Path("/mnt/c/Users/tb/.swjkja"))
    a = ap.parse_args()
    out = a.out
    out.mkdir(parents=True, exist_ok=True)

    stats = json.loads(subprocess.run([sys.executable, str(ROOT / "scripts/progress.py"), "--json", "/dev/stdout"],
                                      capture_output=True, text=True, check=True).stdout)
    refs = a.work / "reference-shots"
    ours = {}
    for d in a.ours:
        ours.update({p.stem: p for p in d.glob("*.png")})

    # Every reference view: reference, ours, difference x4 (as compare.py's composite).
    for v in stats["views"]:
        name = v["view"]
        if name not in ours or not (refs / f"{name}.png").exists():
            v["missing"] = True
            continue
        if not (out / "img/diff" / f"{name}.webp").exists():
            ref, now = load(refs / f"{name}.png"), load(ours[name])
            webp(Image.fromarray(ref.astype(np.uint8)), out / "img/ref" / f"{name}.webp")
            webp(Image.fromarray(now.astype(np.uint8)), out / "img/now" / f"{name}.webp")
            heat = np.clip(np.abs(ref - now) * 4, 0, 255).astype(np.uint8)
            webp(Image.fromarray(heat), out / "img/diff" / f"{name}.webp", 70)

    # The milestones: each build's render of the history views, scored against today's
    # reference screenshots.
    labels = {}
    milestones = ROOT / "bench/reference/milestones.txt"
    for line in milestones.read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            h, label = line.split(None, 1)
            labels[h] = label.strip()
    history = []
    for d in sorted((a.work / "history").glob("*-*")):
        pr, h = d.name.split("-", 1)
        if h not in labels:
            continue
        shots = {}
        for p in sorted(d.glob("*.png")):
            dest = out / "img/h" / d.name / f"{p.stem}.webp"
            ref_path = refs / f"{p.stem}.png"
            if not ref_path.exists():
                continue
            mean, off = score(load(ref_path), load(p))
            if not dest.exists():
                webp(Image.open(p).convert("RGB"), dest)
            shots[p.stem] = {"mean": mean, "off": off}
        subject = next((c["subject"] for c in stats["history"] if c["hash"] and h.startswith(c["hash"])),
                       subprocess.run(["git", "-C", str(ROOT), "log", "-1", "--format=%s", h],
                                      capture_output=True, text=True).stdout.strip())
        date = subprocess.run(["git", "-C", str(ROOT), "log", "-1", "--format=%ad", "--date=short", h],
                              capture_output=True, text=True).stdout.strip()
        history.append({"dir": d.name, "pr": int(pr), "hash": h, "label": labels[h],
                        "subject": subject, "date": date, "shots": shots})

    media = a.work / "progress-media"
    videos = []
    for p in sorted(media.glob("*.mp4")) if media.exists() else []:
        (out / "vid").mkdir(parents=True, exist_ok=True)
        shutil.copy2(p, out / "vid" / p.name)
        caption = p.with_suffix(".txt")
        videos.append({"file": f"vid/{p.name}", "caption": caption.read_text().strip() if caption.exists() else p.stem})

    data = {**stats, "milestones": history, "videos": videos}
    (out / "data.json").write_text(json.dumps(data, separators=(",", ":")))
    shutil.copy2(ROOT / "scripts/progress_page.html", out / "index.html")
    size = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    print(f"{out}: {sum(1 for _ in out.rglob('*.webp'))} images, {len(videos)} videos, {size / 1e6:.1f} MB")


main()
