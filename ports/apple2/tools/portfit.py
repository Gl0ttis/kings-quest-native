# Fit the cost constants: port event counts (costdump CSV) against the original's cycles
# per pass (refemu trace, offset 1). Ordinary passes first, then loading passes.
import csv, statistics as st
P={int(r['pass']):r for r in csv.DictReader(open('scratch/cmp523f/port_counts.csv'))}
O={int(r['pass']):r for r in csv.DictReader(open('scratch/cmp523f/ms_orig_new.csv'))}
def solve(data, n):
    A=[[0.0]*n for _ in range(n)]; b=[0.0]*n
    for v,d in data:
        w=1.0/d**2
        for i in range(n):
            b[i]+=w*v[i]*d
            for j in range(n): A[i][j]+=w*v[i]*v[j]
    for i in range(n):
        p=max(range(i,n),key=lambda k:abs(A[k][i])); A[i],A[p]=A[p],A[i]; b[i],b[p]=b[p],b[i]
        for k in range(i+1,n):
            f=A[k][i]/A[i][i]
            for j in range(i,n): A[k][j]-=f*A[i][j]
            b[k]-=f*b[i]
    c=[0.0]*n
    for i in reversed(range(n)): c[i]=(b[i]-sum(A[i][j]*c[j] for j in range(i+1,n)))/A[i][i]
    return c
ok = lambda p: p+1 in O and p > 3 and int(O[p+1]['delta'])>0
ordinary=[p for p in P if ok(p) and int(P[p]['sectors'])==0 and int(P[p]['plots'])==0 and int(O[p+1]['sector_reads'])==0 and int(O[p+1]['delta'])<2_000_000]
F=['rule_bytes','cel_nibbles','screen_pixels','chars']
data=[([1.0]+[float(P[p][k]) for k in F], float(O[p+1]['delta'])) for p in ordinary]
c=solve(data,5)
print('BASE %.1f '%c[0] + ' '.join('%s %.2f'%(k,v) for k,v in zip(F,c[1:])))
def pred(p, c, load=None):
    v=c[0]+sum(k*float(P[p][f]) for k,f in zip(c[1:],F))
    if load: v+=load[0]*float(P[p]['sectors'])+load[1]*float(P[p]['plots'])+load[2]*float(P[p]['room_changes'])
    return v
e=sorted(abs(pred(p,c)-float(O[p+1]['delta']))/float(O[p+1]['delta']) for p in ordinary)
print(f"ordinary {len(e)}: median {st.median(e)*100:.2f}% p90 {e[int(len(e)*.9)]*100:.2f}% p99 {e[int(len(e)*.99)]*100:.2f}%")
cur=[74.0,55.0,118.0,144.0,2183.0]
e=sorted(abs(pred(p,cur)-float(O[p+1]['delta']))/float(O[p+1]['delta']) for p in ordinary)
print(f"  current constants: median {st.median(e)*100:.2f}% p90 {e[int(len(e)*.9)]*100:.2f}% p99 {e[int(len(e)*.99)]*100:.2f}%")
# Loading passes: port pass with sectors and the same in the original (no disk-swap wait).
load=[p for p in P if ok(p) and (int(P[p]['sectors'])>0 or int(P[p]['plots'])>0) and int(O[p+1]['sector_reads'])==int(P[p]['sectors']) and int(O[p+1]['chars'])==int(P[p]['chars'])]
data=[([float(P[p]['sectors']),float(P[p]['plots']),float(P[p]['room_changes'])], float(O[p+1]['delta'])-pred(p,c)) for p in load]
# plain LS (absolute) for the loading terms
L=solve([(v,d) for v,d in data if d>0], 3)
print('SECTOR %.0f PLOT %.1f ROOM_CHANGE %.0f'%tuple(L))
e=sorted(abs(pred(p,c,L)-float(O[p+1]['delta']))/float(O[p+1]['delta']) for p in load)
print(f"loading {len(e)}: median {st.median(e)*100:.2f}% p90 {e[int(len(e)*.9)]*100:.2f}% max {e[-1]*100:.1f}%")
e=sorted(abs(pred(p,cur,[93600,357,250000])-float(O[p+1]['delta']))/float(O[p+1]['delta']) for p in load)
print(f"  current constants: median {st.median(e)*100:.2f}% p90 {e[int(len(e)*.9)]*100:.2f}% max {e[-1]*100:.1f}%")
skipped=[p for p in P if ok(p) and (int(P[p]['sectors'])>0) and p not in load]
print('loading passes not comparable (disk swap etc):', len(skipped))
print("worst loading passes:")
for p in sorted(load, key=lambda p: -abs(pred(p,c,L)-float(O[p+1]['delta']))/float(O[p+1]['delta']))[:12]:
    d=float(O[p+1]['delta']); m=pred(p,c,L)
    print(f"  pass {p} room {O[p+1]['room']}: orig {d:.0f} model {m:.0f} ({(m-d)/d*100:+.0f}%) sectors {P[p]['sectors']} plots {P[p]['plots']} rc {P[p]['room_changes']} chars {P[p]['chars']}")
