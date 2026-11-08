import sys,re
runs=[]
for l in sys.stdin:
    m=re.search(r"A=\$(..) .* ch=(\d+) cv=(\d+)",l)
    if not m: continue
    a,ch,cv=m[1],int(m[2]),int(m[3])
    if runs and runs[-1][0]==cv and ch in (runs[-1][2],runs[-1][2]+1): runs[-1][2]=ch; runs[-1][3]+=1; runs[-1][4]+=chr(int(a,16)&0x7f) if int(a,16)>=0x20 else '_'
    else: runs.append([cv,ch,ch,1,chr(int(a,16)&0x7f) if int(a,16)>=0x20 else '_'])
for r in runs: print('row %d cols %d-%d (%d chars) %r'%tuple(r))
