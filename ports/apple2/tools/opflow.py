# Explore all control-flow paths of each action handler; report the set of operand-fetch counts.
import re,sys
rows=[l for l in open('scratch/engine_linear.asm').read().splitlines() if re.match(r'^[0-9A-F]{4} ',l)]
ins={int(l[:4],16):l for l in rows}
order=sorted(ins); nxt={a:order[i+1] for i,a in enumerate(order[:-1])}
eng=open('extracted/res/engine_4000.bin','rb').read(); m=lambda a:eng[a-0x4000]
FETCH=0x58F2
memo={}
def counts(start,stack=()):
    """Set of fetch counts over all paths from start to RTS. JMP to $4018 etc. = 'noreturn'."""
    if start==FETCH: return {1}
    if start in memo: return memo[start]
    if start in stack: return {('loop',)}
    memo_guard=stack+(start,)
    res=set(); work=[(start,0)]; seen=set()
    while work:
        a,c=work.pop()
        if (a,c) in seen: continue
        seen.add((a,c))
        if c>8: res.add(('many',)); continue
        l=ins.get(a)
        if l is None: res.add(('off',hex(a))); continue
        p=l[16:].split()
        mn=p[0] if p else '.byte'
        if mn=='RTS': res.add(c); continue
        if mn=='JSR':
            t=int(p[1][1:],16)
            if t<0x4000 or t>=0x7200: work.append((nxt[a],c)); continue
            for k in counts(t,memo_guard):
                if isinstance(k,int): work.append((nxt[a],c+k))
                else: res.add(k)
            continue
        if mn=='JMP':
            if p[1].startswith('('): res.add(c); continue   # tail call via vector
            t=int(p[1][1:],16)
            if t==0x4018: res.add(('restart',c)); continue
            if t==0xC314: res.add(c); continue             # XFER to aux, returns to our caller
            work.append((t,c)); continue
        if mn in('BCC','BCS','BEQ','BNE','BMI','BPL','BVS','BVC'):
            work.append((int(p[1][1:],16),c)); work.append((nxt[a],c)); continue
        if mn in ('BRK','.byte'): res.add(('brk',hex(a))); continue
        work.append((nxt[a],c))
    memo[start]=frozenset(res); return memo[start]
for op in range(0x44):
    a=m(0x6AF3+2*op)|m(0x6AF4+2*op)<<8
    r=counts(a)
    ints=sorted(x for x in r if isinstance(x,int)); other=[x for x in r if not isinstance(x,int)]
    print(f"{op:02X} ${a:04X} counts={ints} {other if other else ''}")
