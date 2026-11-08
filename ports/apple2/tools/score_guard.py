import re,glob,os
for f in sorted(glob.glob('extracted/scripts/0*.txt')):
    room=int(os.path.basename(f)[:3])
    txt=open(f).read().splitlines()
    acts={}; cur=None; sect=None; rules=[]
    for l in txt:
        if l.startswith('== '): sect=l[3:]; continue
        if sect=='ACTIONS':
            m=re.match(r'  A([0-9A-F]{4}):',l)
            if m: cur=int(m.group(1),16); acts[cur]=[]; continue
            m=re.match(r'    ([0-9A-F]{4})  (.*)',l)
            if m: acts[cur].append((int(m.group(1),16),m.group(2)))
        elif sect in('CYCLE RULES','PARSER RULES'):
            m=re.match(r'  ([0-9A-F]{4})  (.*)',l)
            if m: rules.append((sect[0],int(m.group(1),16),m.group(2)))
    flat=sorted((off,t,a) for a,ins in acts.items() for off,t in ins)
    def seq(e):
        base=max(x for x in acts if x<=e); return [t for off,t,a in flat if off>=e and a==base]
    for kind in 'CP':
        rs=[(o,t) for k,o,t in rules if k==kind]
        blocks=[]  # (start, end, text)
        for o,t in rs:
            m=re.match(r'if (.*) else goto ([0-9A-F]{4})',t)
            if m: blocks.append((o,int(m.group(2),16),m.group(1)))
        for o,t in rs:
            m=re.search(r'-> A([0-9A-F]{4})',t)
            if not m: continue
            s=seq(int(m.group(1),16))
            pts=sum(int(x.split()[1]) for x in s if x.startswith('add_score'))-sum(int(x.split()[1]) for x in s if x.startswith('sub_score'))
            if not any(x.startswith('add_score') for x in s): continue
            guards=[g for b,e,g in blocks if b<o<e]
            print(f"room {room:2} {kind} {pts:+3} | {' / '.join(guards)} | {t.split(' -> ')[0]}")
