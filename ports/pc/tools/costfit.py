"""Fit the speed model: the port's per-pass work counts (examples/costdump.rs) against
the original's work per pass: its cycles (refemu-pc's trace, reference/*.csv) less the
cycles it spent waiting (kqframes-pc --waits).

    python tools/costfit.py --fit COSTS.csv TRACE.csv WAITS.csv [...] [--check ...]

Row k of the counts is pass k's work; the trace's and the waits' row k+1 give its
cycles. The fit is over the --fit routes; the --check routes are only measured with
the constants found.

The PC waits in most passes: before the actors move, for a tick since they last did
($3FE7), and in the busy waits the PCjr has too (the messages, the pause). The waits
aren't work: the port waits as the original does, to the tick that ends the wait. What
is fitted is the rest.

Passes are of two kinds:
- ordinary: no disk read, no picture drawn. Fitted on the counts, weighted by
  1/cycles^2 (the error fitted is relative).
- loading: a disk read or a picture drawn. The ordinary model, the drive's time (the
  port's model of the drive, the same as refemu-pc's: `drive`) and the picture's
  drawing (DRAWING, timed on its own by tools/drawfit.py) are taken off; the sectors'
  work and a picture's fixed rest (c48E3: the merge and what follows) are fitted on
  what is left (least squares).
"""

import csv
import sys

import numpy as np


def load(files):
    rows = []
    for costs, trace, waits in zip(files[::3], files[1::3], files[2::3]):
        c = {int(r["pass"]): r for r in csv.DictReader(open(costs))}
        t = {int(r["pass"]): r for r in csv.DictReader(open(trace))}
        w = {int(r["pass"]): int(r["waited"]) for r in csv.DictReader(open(waits))}
        for k, r in c.items():
            if k + 1 in t and k + 1 in w and k > 1:
                rows.append((r, int(t[k + 1]["delta"]) - w[k + 1]))
    names = [n for n in rows[0][0] if n != "pass"]
    X = np.array([[float(r[n]) for n in names] for r, _ in rows])
    y = np.array([float(d) for _, d in rows])
    return names, X, y


args = sys.argv[1:]
i_check = args.index("--check") if "--check" in args else len(args)
fit_files = args[1:i_check]
check_files = args[i_check + 1 :]
names, X, y = load(fit_files)
col = lambda X, n: X[:, names.index(n)]

# The pass's checkpoint (the game cycle, once a pass) carries its fixed work; the
# picture's (draw_room_picture) a picture's fixed part.
PASS = "c293A"
PICTURE = "c48E3"
# The drawing, from tools/drawfit.py over the max-score run's 172 pictures: the start
# (the clear and the frame, before the first byte), the last byte to the merge, and the
# lines' and fills' counts.
DRAWING = {
    PICTURE: 171_962.0 + 100_723.0,
    "abs_steps": 808.38,
    "to_steps": 933.24,
    "lines": 579.31,
    "fill_bytes": 26.77,
    "fill_scans": 44.72,
    "fill_runs": 2_413.04,
}
LOAD = ["sectors", PICTURE] + list(DRAWING)
NOT_ORDINARY = LOAD + ["plots", "wait_ticks", "drive"]


def kinds(X):
    loading = (col(X, "sectors") > 0) | (col(X, "plots") > 0)
    return ~loading, loading


ordinary, loading = kinds(X)
ORD = [PASS] + [n for n in names if n not in NOT_ORDINARY and n != PASS and col(X, n)[ordinary].std() > 0]
# Checkpoints always visited together are one column for the fit.
seen = {}
for n in list(ORD):
    key = tuple(col(X, n)[ordinary])
    if key in seen:
        ORD.remove(n)
    else:
        seen[key] = n


def design(X, cols):
    return np.column_stack([col(X, n) for n in cols])


def nonneg(A, b, cols):
    """Least squares with every constant >= 0: a column whose constant comes out
    negative is dropped (its work goes to the columns it moves with) and the fit made
    again. The port adds the constants up as the work is done, and time doesn't run
    backwards."""
    cols = list(cols)
    while True:
        c, *_ = np.linalg.lstsq(np.column_stack([A[:, names.index(n)] for n in cols]), b, rcond=None)
        neg = [i for i in range(len(cols)) if c[i] < 0]
        if not neg:
            return cols, c
        del cols[min(neg, key=lambda i: c[i])]


w = 1.0 / y[ordinary]
ORD, c_ord = nonneg(X[ordinary] * w[:, None], y[ordinary] * w, ORD)
drawing = lambda X: sum(col(X, n) * v for n, v in DRAWING.items())
resid = y - design(X, ORD) @ c_ord - col(X, "drive") - drawing(X)
LD, c_load = nonneg(X[loading], resid[loading], ["sectors", PICTURE])

print("ordinary passes (cycles per count):")
for n, v in zip(ORD, c_ord):
    print(f"  {n} {v:.2f}")
print("loading passes, added (besides the drive and the drawing):")
for n, v in zip(LD, c_load):
    print(f"  {n} {v:.2f}")


def predict(X):
    return design(X, ORD) @ c_ord + design(X, LD) @ c_load + col(X, "drive") + drawing(X)


def report(label, mask, y, pred):
    if mask.sum() == 0:
        return
    e = np.sort(np.abs(pred[mask] - y[mask]) / y[mask])
    n = len(e)
    total = (pred[mask].sum() - y[mask].sum()) / y[mask].sum()
    print(
        f"  {label}: {n} passes, median {np.median(e)*100:.2f}%, p90 {e[int(n*.9)]*100:.2f}%, "
        f"p99 {e[int(n*.99)]*100:.2f}%, total {total*100:+.2f}%"
    )


for label, files in (("fitted routes", fit_files), ("checked routes", check_files)):
    if not files:
        continue
    _, X2, y2 = load(files)
    o, l = kinds(X2)
    p = predict(X2)
    print(f"{label}:")
    report("ordinary", o, y2, p)
    report("loading", l, y2, p)
    report("all", o | l, y2, p)
