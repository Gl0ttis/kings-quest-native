import sys, csv, statistics as st
port = list(csv.DictReader(open(sys.argv[1])))
orig = list(csv.DictReader(open(sys.argv[2])))
O = {int(r['pass']): r for r in orig}
rows = []
for r in port:
    p = int(r['pass']); o = O.get(p + 1)
    if not o: continue
    d = int(o['delta']); m = float(r['model_cycles'])
    if d == 0: continue
    rows.append((p, int(r['room']), d, m, (m - d) / d, o))
errs = [abs(e) for *_, e, o in rows]
print(f"passes {len(rows)}  median |err| {st.median(errs)*100:.2f}%  p90 {sorted(errs)[int(len(errs)*.9)]*100:.2f}%  p99 {sorted(errs)[int(len(errs)*.99)]*100:.2f}%  max {max(errs)*100:.1f}%")
tot_d = sum(r[2] for r in rows); tot_m = sum(r[3] for r in rows)
print(f"total time: original {tot_d/1020484:.0f} s, model {tot_m/1020484:.0f} s ({(tot_m-tot_d)/tot_d*100:+.2f}%)")
# heavy passes: top 10% by original cycles, excluding room changes (sector reads)
ordinary = [r for r in rows if int(r[5]['sector_reads']) == 0]
ordinary.sort(key=lambda r: r[2])
heavy = ordinary[int(len(ordinary)*.9):]
he = [abs(r[4]) for r in heavy]
print(f"ordinary passes {len(ordinary)}: median {st.median([abs(r[4]) for r in ordinary])*100:.2f}%; heaviest 10% (>{heavy[0][2]} cycles): median {st.median(he)*100:.2f}%, mean signed {st.mean(r[4] for r in heavy)*100:+.2f}%")
# by room
by = {}
for r in ordinary: by.setdefault(r[1], []).append(r)
print("room  n   median_cyc  signed_mean_err  median|err|")
for k in sorted(by, key=lambda k: -st.mean(abs(r[4]) for r in by[k]))[:15]:
    v = by[k]
    print(f"{k:4} {len(v):5} {st.median(r[2] for r in v):9.0f}  {st.mean(r[4] for r in v)*100:+7.2f}%  {st.median(abs(r[4]) for r in v)*100:6.2f}%")
rc = [r for r in rows if int(r[5]['sector_reads']) > 0]
print(f"passes with disk reads {len(rc)}: median |err| {st.median(abs(r[4]) for r in rc)*100:.2f}%, max {max(abs(r[4]) for r in rc)*100:.1f}%")
worst = sorted(rows, key=lambda r: -abs(r[4]))[:12]
for p, room, d, m, e, o in worst:
    print(f"  pass {p} room {room}: orig {d} model {m:.0f} ({e*100:+.1f}%) sectors {o['sector_reads']} chars {o['chars']} plots {o['plots']}")
