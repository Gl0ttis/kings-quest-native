"""Time the room pictures' drawing as the port shows it against the original's.

Input: examples/drawcmp.rs's CSV, for an event log (the original's timing from
refemu-pc's kqplotlog-pc): per picture, at each mark of the port's drawing, the bytes
written to the CGA (counted as the CGA takes them, scaled to the original's total) and
the work done so far, and the original's cycles from the picture's start to that byte;
and a last row with the original's cycles to the colours merged.

It reports, per picture, the error of the whole drawing (start to merge) as the port
charges it (cost.rs: PICTURE plus the line and fill costs), and of how far the drawing
has got part way: at each mark, the share of the drawing done by the original's clock
against the port's. Then it fits the shape: a start (the clear, before the first byte),
per-count costs for the lines and fills, and an end (after the last byte, the merge),
and reports the same with them.

    python tools/drawfit.py draw.csv
"""

import csv
import sys

import numpy as np

COUNTS = ["abs_steps", "to_steps", "lines", "fill_bytes", "fill_scans", "fill_runs", "fill_seeds"]
# cost.rs's constants: the clear and the frame before the first byte (PICTURE_START),
# the counts', and the cycles from the last byte to the merge (the first 100,723 of
# PICTURE_END).
PICTURE_START = 171_962.0
TO_MERGE = 100_723.0
COST = {
    "abs_steps": 808.38,
    "to_steps": 933.24,
    "lines": 579.31,
    "fill_bytes": 26.77,
    "fill_scans": 44.72,
    "fill_runs": 2_413.0,
    "fill_seeds": 0.0,
}


def load(path):
    pics = {}
    for r in csv.DictReader(open(path)):
        p = pics.setdefault(int(r["pic"]), {"marks": [], "end": None})
        if r["bytes"] == "end":
            p["end"] = float(r["orig"])
        else:
            p["marks"].append((np.array([float(r[c]) for c in COUNTS]), float(r["orig"])))
    return [pics[k] for k in sorted(pics)]


def pct(a, q):
    return float(np.percentile(np.abs(a), q)) if len(a) else 0.0


def report(name, pics, start, unit, end):
    """start/end: cycles before the first byte and after the last; unit: per count."""
    total_err, progress_err = [], []
    for p in pics:
        if not p["marks"]:
            continue
        work = np.array([w for w, _ in p["marks"]])
        orig = np.array([t for _, t in p["marks"]])
        model = start + work @ unit
        total = model[-1] + end
        total_err.append(total / p["end"] - 1)
        # The share of the whole time at each mark, the port's against the original's.
        progress_err.extend(model / total - orig / p["end"])
    total_err, progress_err = np.array(total_err), np.array(progress_err)
    print(f"{name}:")
    print(f"  whole drawing: median {100 * pct(total_err, 50):.1f}%, 90% within {100 * pct(total_err, 90):.1f}%")
    print(f"  part way: median {100 * pct(progress_err, 50):.1f} points, 90% within {100 * pct(progress_err, 90):.1f} points")


def main():
    pics = [p for p in load(sys.argv[1]) if p["marks"] and p["end"]]
    print(f"{len(pics)} pictures, {sum(len(p['marks']) for p in pics)} marks")
    unit = np.array([COST.get(c, 0.0) for c in COUNTS])
    report("the port (cost.rs)", pics, PICTURE_START, unit, TO_MERGE)

    # The fit: every mark's original cycles from a start and the counts so far.
    X = np.array([np.concatenate([[1.0], w]) for p in pics for w, _ in p["marks"]])
    y = np.array([t for p in pics for _, t in p["marks"]])
    used = list(range(len(COUNTS) + 1))
    while True:
        c, *_ = np.linalg.lstsq(X[:, used], y, rcond=None)
        if (c >= 0).all():
            break
        used.pop(int(np.argmin(c)))
    coef = np.zeros(len(COUNTS) + 1)
    coef[used] = c
    ends = [p["end"] - p["marks"][-1][1] for p in pics]
    end = float(np.median(ends))
    print("fit: start {:.0f}, end (median) {:.0f} (from {:.0f} to {:.0f})".format(coef[0], end, min(ends), max(ends)))
    for n, v in zip(COUNTS, coef[1:]):
        print(f"  {n}: {v:.2f}")
    report("fitted", pics, coef[0], coef[1:], end)


if __name__ == "__main__":
    main()
