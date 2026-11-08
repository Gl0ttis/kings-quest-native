"""runfit.py RUNS PLOTS: fit the drawing costs per line segment and fill seed.
RUNS: `plotruns KEYS 0` (the port's runs per drawing pass); PLOTS: refemu's
REFEMU_PLOTLOG=0 ("pass cycles" per plot). Port pass P = refemu pass P."""
import sys, collections, statistics as st
runs = {}; cur = None
for l in open(sys.argv[1]):
    if l.startswith('# pass'):
        _, _, p, _, n = l.split(); cur = int(p); runs.setdefault(cur, []).append((int(n), []))
    else:
        a, k = l.split(); runs[cur][-1][1].append((int(a), k == 'fill'))
plots = collections.defaultdict(list)
for l in open(sys.argv[2]):
    p, c = l.split(); plots[int(p)].append(int(c))
rows = []  # (kind, plots in run, cycles from its first plot to the next run's first plot)
pics = []  # per drawing: (pass, list of (kind, n), total cycles, cumulative cycles per plot)
for p, draws in runs.items():
    c = plots.get(p, [])
    if len(draws) != 1 or len(c) != draws[0][0]:
        continue  # only passes with one picture whose plot count matches
    total, rr = draws[0]
    seq = []
    for i, (s, f) in enumerate(rr):
        e = rr[i + 1][0] if i + 1 < len(rr) else total
        seq.append((f, e - s))
        if i + 1 < len(rr):
            rows.append((f, e - s, c[e] - c[s]))
    pics.append((p, seq, c[-1] - c[0], c))
def fit(r):
    n = len(r); sx = sum(x for _, x, _ in r); sy = sum(y for _, _, y in r)
    sxx = sum(x * x for _, x, _ in r); sxy = sum(x * y for _, x, y in r)
    a = (n * sxy - sx * sy) / (n * sxx - sx * sx); return a, (sy - a * sx) / n
L = fit([r for r in rows if not r[0]]); F = fit([r for r in rows if r[0]])
print(f"{len(pics)} pictures, {len(rows)} runs")
print(f"line segment: {L[0]:.1f} per plot + {L[1]:.0f} per segment")
print(f"fill seed:    {F[0]:.1f} per plot + {F[1]:.0f} per seed")
def model(seq, upto=None):
    t = 0.0; done = 0
    for f, n in seq:
        a, b = F if f else L
        if upto is not None and done + n >= upto:
            return t + b + a * (upto - done)
        t += a * n + b; done += n
    return t
err = []; mid = []
for p, seq, tot, c in pics:
    m = model(seq)
    err.append((m - tot) / tot)
    # Part way: where is the drawing at 25/50/75% of its time, model against original?
    n = len(c)
    for q in (0.25, 0.5, 0.75):
        i = int(n * q); mid.append((model(seq, i) - (c[i] - c[0])) / tot)
e = sorted(abs(x) for x in err); m = sorted(abs(x) for x in mid)
print(f"whole drawing time: median {st.median(e)*100:.2f}%, p90 {e[int(len(e)*.9)]*100:.2f}%, max {e[-1]*100:.1f}%")
print(f"part way (25/50/75% of plots), as a share of the drawing: median {st.median(m)*100:.2f}%, p90 {m[int(len(m)*.9)]*100:.2f}%")
