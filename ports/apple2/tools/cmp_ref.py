"""cmp_ref.py PORT_TRACE ORIG_TRACE: compare a port trace (kqplay --headless --keys
--trace) with refemu's (--keys --trace). Row k+1 of refemu's trace is the port's pass k.
Prints the first differences and a summary."""
import csv, sys
port = {int(r['pass']): r for r in csv.DictReader(open(sys.argv[1]))}
orig = {int(r['pass']) - 1: r for r in csv.DictReader(open(sys.argv[2]))}
keys = ('room', 'ego_x', 'ego_y', 'score', 'vars_hash')
common = sorted(set(port) & set(orig) - {0})
bad = [k for k in common if tuple(port[k][c] for c in keys) != tuple(orig[k][c] for c in keys)]
print(f"{len(common)} passes compared, {len(bad)} differ")
for k in bad[:8]:
    print(f"  pass {k}: port {[port[k][c] for c in keys]}  orig {[orig[k][c] for c in keys]}")
last = common[-1]
print(f"  last pass {last}: room {port[last]['room']}, score {port[last]['score']}")
