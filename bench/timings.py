#!/usr/bin/env python3
"""Summarise `swjkja view --timings` files: each column's median, p99 and max, the
GPU's time per frame outside our own work (the gap between frames), and the
slowest frames with every step, to show where a stall went.

    bench/timings.py run1.tsv [run2.tsv ...]
"""
import sys


def read(path):
    with open(path) as f:
        head = f.readline().split()
        rows = []
        for line in f:
            rows.append({k: (None if v == "-" else float(v)) for k, v in zip(head, line.split())})
    return rows


def stats(values):
    v = sorted(x for x in values if x is not None)
    if not v:
        return "-"
    pick = lambda q: v[min(len(v) - 1, int(q * len(v)))]
    return f"{pick(0.5):7.3f} {pick(0.99):7.3f} {v[-1]:8.3f}"


def main(paths):
    for path in paths:
        rows = read(path)
        # GPU time between the end of one frame's work and the start of the next.
        for prev, row in zip(rows, rows[1:]):
            if None not in (prev["gpu_start_ms"], row["gpu_start_ms"], prev["gpu_world_ms"]):
                row["gpu_period_ms"] = row["gpu_start_ms"] - prev["gpu_start_ms"]
                row["gpu_gap_ms"] = row["gpu_period_ms"] - prev["gpu_world_ms"] - (prev["gpu_blit_ms"] or 0)
        cols = ["dt_ms", "loop_ms", "wait_ms", "acquire_ms", "submit_ms", "present_ms",
                "gpu_world_ms", "gpu_blit_ms", "gpu_period_ms", "gpu_gap_ms"]
        print(f"{path}: {len(rows)} frames, {1000 * len(rows) / rows[-1]['t_ms']:.1f} FPS")
        print(f"  {'':14} {'median':>7} {'p99':>7} {'max':>8}")
        for c in cols:
            print(f"  {c:14} {stats(r.get(c) for r in rows)}")
        print("  slowest frames (dt is the time since the previous frame started):")
        print("  " + " ".join(f"{c[:-3]:>8}" for c in ["frame_ms", "t_ms"] + cols))
        for r in sorted(rows, key=lambda r: -r["dt_ms"])[:10]:
            vals = [r["frame"], r["t_ms"]] + [r.get(c) for c in cols]
            print("  " + " ".join("       -" if v is None else f"{v:8.2f}" for v in vals))
        print()


if __name__ == "__main__":
    main(sys.argv[1:] or sys.exit(__doc__))
