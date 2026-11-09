"""Fit the speed model: the port's per-pass work counts (examples/costdump.rs) against
the original's cycles per pass (refemu-pcjr's trace, reference/*.csv).

    python tools/costfit.py --fit COSTS.csv TRACE.csv [...] [--check COSTS.csv TRACE.csv ...]

The drawing of a room picture is timed on its own (tools/drawfit.py, from every byte
the original writes as it draws), so its costs (DRAWING) are fixed here, not fitted on
the passes: of a loading pass, what is left after them is the sectors' and a picture's
other work (c34F5).

Row k of the counts is pass k's work; the trace's row k+1 gives its cycles. The fit is
over the --fit routes; the --check routes are only measured with the constants found.

Passes are of three kinds:
- ordinary: no disk, no picture, no busy wait. Fitted on the counts, weighted by
  1/cycles^2 (the error fitted is relative).
- loading: a disk read or a picture drawn. The ordinary model plus the sectors and the
  drawing's work, fitted on what is left (least squares).
- waiting: a busy wait for the timer (messages printing). Their length is the time to
  the tick that ends the wait, which the paced clock gives; they aren't fitted.
"""

import csv
import sys

import numpy as np


def load(files):
    rows = []
    for costs, trace in zip(files[::2], files[1::2]):
        c = {int(r["pass"]): r for r in csv.DictReader(open(costs))}
        t = {int(r["pass"]): r for r in csv.DictReader(open(trace))}
        for k, r in c.items():
            if k + 1 in t and k > 1:
                rows.append((r, int(t[k + 1]["delta"])))
    names = [n for n in rows[0][0] if n != "pass"]
    X = np.array([[float(r[n]) for n in names] for r, _ in rows])
    y = np.array([float(d) for _, d in rows])
    return names, X, y


# The drawing's costs, from tools/drawfit.py over the max-score run's 146 pictures:
# the clear before the first byte and the merge after the last (c34F5), and the lines'
# and fills' counts.
DRAWING = {
    "c34F5": 366_744.0 + 104_454.0,
    "abs_steps": 1_110.35,
    "to_steps": 1_217.87,
    "lines": 1_093.98,
    "fill_bytes": 26.83,
    "fill_scans": 11.28,
    "fill_runs": 923.48,
    "fill_seeds": 319.92,
}

args = sys.argv[1:]
i_check = args.index("--check") if "--check" in args else len(args)
fit_files = args[1:i_check]
check_files = args[i_check + 1 :]
names, X, y = load(fit_files)
col = lambda X, n: X[:, names.index(n)]

LOAD = ["sectors", "abs_steps", "to_steps", "lines", "fill_bytes", "fill_scans", "fill_runs", "fill_seeds", "c34F5"]
NOT_ORDINARY = LOAD + ["plots", "wait_ticks"]


def kinds(X, y):
    waiting = col(X, "wait_ticks") > 0
    loading = ((col(X, "sectors") > 0) | (col(X, "plots") > 0)) & ~waiting
    ordinary = ~loading & ~waiting
    return ordinary, loading, waiting


ordinary, loading, waiting = kinds(X, y)
ORD = ["c14F2"] + [n for n in names if n not in NOT_ORDINARY and n != "c14F2" and col(X, n)[ordinary].std() > 0]
# Checkpoints always visited together are one column for the fit (14F2, once a pass, is
# kept: it carries the pass's fixed work).
seen = {}
for n in list(ORD):
    key = tuple(col(X, n)[ordinary])
    if key in seen:
        ORD.remove(n)
    else:
        seen[key] = n


def design(X, cols):
    # No intercept: every pass's fixed work is in its checkpoints (one 14F2 a pass),
    # and each constant is a cost the port adds as that work is done.
    return np.column_stack([np.zeros(len(X))] + [col(X, n) for n in cols])


# Every constant must be >= 0: the port adds them up as the work is done, and time
# doesn't run backwards. A column whose constant comes out negative is dropped (its
# work goes to the columns it moves with) and the fit made again.
w = 1.0 / y[ordinary]
while True:
    c_ord, *_ = np.linalg.lstsq(design(X[ordinary], ORD) * w[:, None], y[ordinary] * w, rcond=None)
    neg = [i for i in range(len(ORD)) if c_ord[i + 1] < 0]
    if not neg:
        break
    del ORD[min(neg, key=lambda i: c_ord[i + 1])]
LD = ["sectors", "c34F5"]
drawing = lambda X: sum(col(X, n) * v for n, v in DRAWING.items())
resid = y - design(X, ORD) @ c_ord - drawing(X)
while True:
    c_load, *_ = np.linalg.lstsq(np.column_stack([col(X, n)[loading] for n in LD]), resid[loading], rcond=None)
    neg = [i for i in range(len(LD)) if c_load[i] < 0]
    if not neg:
        break
    del LD[min(neg, key=lambda i: c_load[i])]

print("ordinary passes (cycles per count):")
for n, v in zip(ORD, c_ord[1:]):
    print(f"  {n} {v:.2f}")
print("loading passes, added (besides the drawing):")
for n, v in zip(LD, c_load):
    print(f"  {n} {v:.2f}")


def predict(X):
    p = design(X, ORD) @ c_ord + drawing(X)
    return p + sum(col(X, n) * v for n, v in zip(LD, c_load))


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
    o, l, wt = kinds(X2, y2)
    p = predict(X2)
    print(f"{label}:")
    report("ordinary", o, y2, p)
    report("loading", l, y2, p)
    report("ordinary and loading", o | l, y2, p)
    print(f"  waiting: {wt.sum()} passes, {y2[wt].sum() / y2.sum() * 100:.1f}% of the time (the clock's)")
