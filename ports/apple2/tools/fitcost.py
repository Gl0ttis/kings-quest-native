import csv,sys
rows=list(csv.DictReader(open(sys.argv[1])))
feats=sys.argv[2].split(',')
data=[]
prev_room=None
for r in rows:
    d=int(r['delta'])
    room=r['room']
    ok = d>0 and d<400000 and int(r['plots'])==0 and room==prev_room
    prev_room=room
    if ok: data.append(([1.0]+[float(r[f]) for f in feats], float(d)))
n=len(feats)+1
# normal equations
A=[[0.0]*n for _ in range(n)]; b=[0.0]*n
for x,y in data:
    for i in range(n):
        b[i]+=x[i]*y
        for j in range(n): A[i][j]+=x[i]*x[j]
# solve (Gaussian elimination with partial pivoting)
M=[A[i]+[b[i]] for i in range(n)]
for c in range(n):
    p=max(range(c,n),key=lambda r:abs(M[r][c])); M[c],M[p]=M[p],M[c]
    if abs(M[c][c])<1e-9: print("singular on",(['base']+feats)[c]); sys.exit(1)
    for r in range(n):
        if r!=c:
            f=M[r][c]/M[c][c]
            for k in range(c,n+1): M[r][k]-=f*M[c][k]
coef=[M[i][n]/M[i][i] for i in range(n)]
for name,cf in zip(['base']+feats,coef): print(f"  {name:18s} {cf:10.1f}")
err=[abs(sum(c*xi for c,xi in zip(coef,x))-y)/y for x,y in data]
err.sort()
print(f"passes fitted {len(data)}; relative error median {100*err[len(err)//2]:.2f}% p90 {100*err[int(len(err)*.9)]:.2f}% max {100*err[-1]:.2f}%")
