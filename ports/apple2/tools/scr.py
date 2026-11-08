import sys,glob,re
NOPS={}
for l in open('scratch/tools/opcounts.txt'):
    p=l.split(); NOPS[int(p[0],16)]=int(p[2].split('=')[1])
def sections(b):
    L=[b[i]|b[i+1]<<8 for i in (0,2,4,6)]; s=[8]
    for x in L: s.append(s[-1]+x)
    return s  # starts of sect0..3 and end
def parse_actions(b,start,end):
    seqs=[];pos=start;bad=[]
    while pos<end:
        st=pos; ops=[]
        while True:
            if pos>=end: bad.append(('overrun',st)); break
            op=b[pos]; pos+=1
            if op==0xFF: break
            if op not in NOPS: bad.append(('badop',pos-1,op)); break
            ops.append((op,list(b[pos:pos+NOPS[op]]))); pos+=NOPS[op]
        seqs.append((st-start,ops))
        if bad: break
    return seqs,bad,pos
tot=0;badfiles=0
for f in sorted(glob.glob('extracted/res/logic/*.bin')):
    b=open(f,'rb').read(); s=sections(b)
    if s[4]!=len(b): print(f,"section sum",s[4],"!= len",len(b))
    seqs,bad,pos=parse_actions(b,s[0],s[1])
    tot+=len(seqs)
    if bad or pos!=s[1]: badfiles+=1; print(f, bad[:3], "end",pos,"expected",s[1])
print("action sequences:",tot,"files with problems:",badfiles)

def parse_rules(b,start,end,mode):
    """Structural parse. Returns list of (offset, kind, fields) and problems."""
    pos=start; out=[]; prob=[]
    def conds():
        nonlocal pos; c=[]
        while True:
            if pos>=end: raise IndexError
            v=b[pos]; pos+=1
            if v==0xFE: return c
            c.append((v,b[pos])); pos+=1
    def w():
        nonlocal pos; x=b[pos]|b[pos+1]<<8; pos+=2; return x
    try:
        while pos<end:
            st=pos; op=b[pos]; pos+=1
            if op==0xFF: out.append((st,'end',None)); 
            elif op==0xFE: o=w(); out.append((st,'goto',pos+o))
            elif op==0xFC or (op==0xFB and mode==0): c=conds(); o=w(); out.append((st,'if',(c,pos+o)))
            elif op==0xFB: c=conds(); a=w(); out.append((st,'always',(c,a)))
            elif op==0xFD and mode==0: r=list(b[pos:pos+4]); pos+=4; c=conds(); o=w(); out.append((st,'ifpos',(r,c,pos+o)))
            elif mode==0: n=b[pos]; pos+=1; c=conds(); a=w(); out.append((st,'said',(op,n,c,a)))
            else: r=[op]+list(b[pos:pos+3]); pos+=3; c=conds(); a=w(); out.append((st,'at',(r,c,a)))
    except IndexError: prob.append(('overrun',st))
    if pos!=end: prob.append(('end',pos,end))
    return out,prob

if __name__=='__main__':
    issues=0; nr=0
    for f in sorted(glob.glob('extracted/res/logic/*.bin')):
        b=open(f,'rb').read(); s=sections(b)
        seqs,_,_=parse_actions(b,s[0],s[1]); starts={o for o,_ in seqs}
        for mode,(a,e) in ((1,(s[1],s[2])),(0,(s[2],s[3]))):
            rules,prob=parse_rules(b,a,e,mode); nr+=len(rules)
            bounds={r[0] for r in rules}|{e}
            for r in rules:
                if r[1] in('said','always','at'):
                    act=r[2][-1]
                    if act not in starts: prob.append(('act',hex(r[0]),act))
                tgt={'goto':lambda r:r[2],'if':lambda r:r[2][1],'ifpos':lambda r:r[2][2]}.get(r[1])
                if tgt and tgt(r) not in bounds: prob.append(('tgt',hex(r[0]),hex(tgt(r))))
            if prob: issues+=1; print(f,'mode',mode,prob[:4])
    print("rules:",nr,"sections with issues:",issues)
