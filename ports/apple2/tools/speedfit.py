# Weighted least squares (relative error) of the original's cycles per pass on its own
# event counts. Usage: speedfit.py orig_trace.csv feature,feature,...
import csv, sys, statistics as st
rows = [r for r in csv.DictReader(open(sys.argv[1])) if int(r['pass']) > 3]
feats = sys.argv[2].split(',')
def x(r):
    v = []
    for f in feats:
        if f == 'one': v.append(1.0)
        elif f == 'drew': v.append(1.0 if int(r['cel_nibbles']) > 0 else 0.0)
        else: v.append(float(r[f]))
    return v
data = [(x(r), float(r['delta'])) for r in rows if int(r['sector_reads']) == 0 and int(r['plots']) == 0 and 0 < int(r['delta']) < 2_000_000]
n = len(feats)
A = [[0.0]*n for _ in range(n)]; b = [0.0]*n
for v, d in data:
    w = 1.0 / d**2
    for i in range(n):
        b[i] += w*v[i]*d
        for j in range(n): A[i][j] += w*v[i]*v[j]
# Gaussian elimination
for i in range(n):
    p = max(range(i, n), key=lambda k: abs(A[k][i])); A[i], A[p] = A[p], A[i]; b[i], b[p] = b[p], b[i]
    for k in range(i+1, n):
        f = A[k][i]/A[i][i]
        for j in range(i, n): A[k][j] -= f*A[i][j]
        b[k] -= f*b[i]
c = [0.0]*n
for i in reversed(range(n)):
    c[i] = (b[i] - sum(A[i][j]*c[j] for j in range(i+1, n)))/A[i][i]
for f, k in zip(feats, c): print(f"  {f:18} {k:10.2f}")
errs = sorted(abs(sum(k*vi for k, vi in zip(c, v)) - d)/d for v, d in data)
print(f"{len(data)} passes: median {st.median(errs)*100:.2f}%  p90 {errs[int(len(errs)*.9)]*100:.2f}%  p99 {errs[int(len(errs)*.99)]*100:.2f}%  max {errs[-1]*100:.1f}%")
