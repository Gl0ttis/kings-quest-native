import re
lines=open('scratch/engine_linear.asm').read().splitlines()
rows=[(int(l[:4],16),l) for l in lines if re.match(r'^[0-9A-F]{4} ',l)]
idx={a:i for i,(a,_) in enumerate(rows)}
eng=open('extracted/res/engine_4000.bin','rb').read(); m=lambda a:eng[a-0x4000]
memo={}
def routine(a,depth=0):
    """Linear walk from a until RTS; follow JMP abs. Returns (fetches, has_branch_over_fetch, trace)"""
    if a in memo: return memo[a]
    memo[a]=(0,False,'(rec)')
    if a==0x58F2: memo[a]=(1,False,''); return memo[a]
    i=idx.get(a); f=0; br=False; tr=[]
    if i is None or a<0x4000 or a>=0x7200: memo[a]=(0,False,f'ext{a:04X}'); return memo[a]
    seen=0
    while i is not None and i<len(rows) and seen<200:
        seen+=1; ad,l=rows[i]; p=l.split()
        if ' JSR $' in l:
            t=int(l.split('$')[-1],16)
            if 0x4000<=t<0x7200:
                ff,bb,_=routine(t,depth+1); f+=ff; br|=bb
                if ff: tr.append(f'{t:04X}:{ff}')
        elif re.search(r' B(CC|CS|EQ|NE|MI|PL|VS|VC) ',l):
            br=True
        if ' RTS' in l: break
        if ' JMP $' in l:
            t=int(l.split('$')[-1],16)
            if t in idx: i=idx[t]; continue
            break
        if ' JMP (' in l: break
        i+=1
    memo[a]=(f,br,' '.join(tr)); return memo[a]
for op in range(0x44):
    a=m(0x6AF3+2*op)|m(0x6AF4+2*op)<<8
    f,br,tr=routine(a)
    print(f"{op:02X} ${a:04X} operands={f} {'branchy' if br else ''} {tr}")
