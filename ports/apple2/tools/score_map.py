import re,glob,os
total=0; rows=[]
for f in sorted(glob.glob('extracted/scripts/0*.txt')):
    room=int(os.path.basename(f)[:3])
    txt=open(f).read().splitlines()
    # actions: label -> list of instrs
    acts={}; cur=None; sect=None; rules=[]
    for l in txt:
        if l.startswith('== '): sect=l[3:]; continue
        if sect=='ACTIONS':
            m=re.match(r'  A([0-9A-F]{4}):',l)
            if m: cur=int(m.group(1),16); acts[cur]=[]; continue
            m=re.match(r'    ([0-9A-F]{4})  (.*)',l)
            if m and cur is not None: acts[cur].append((int(m.group(1),16),m.group(2)))
        elif sect in('CYCLE RULES','PARSER RULES'):
            m=re.match(r'  ([0-9A-F]{4})  (.*)',l)
            if m: rules.append((sect[0],m.group(2)))
    # action entries that reach an add_score (sequence runs from entry to FF; entries can start mid-sequence)
    flat=[]
    for a,ins in acts.items():
        for off,t in ins: flat.append((off,t,a))
    flat.sort()
    for kind,r in rules:
        m=re.search(r'-> A([0-9A-F]{4})',r)
        if not m: continue
        e=int(m.group(1),16)
        # walk from entry to the end of its sequence
        seq=[t for off,t,a in flat if off>=e and a==max(x for x in acts if x<=e)]
        pts=[int(re.search(r'add_score (\d+)',t).group(1)) for t in seq if t.startswith('add_score')]
        if pts:
            rows.append((room,kind,r.split(' -> ')[0],sum(pts),[t for t in seq if not t.startswith('print')][:6]))
for r in rows: print(r[0], r[1], r[3], '|', r[2], '|', r[4])
print("rules awarding points:",len(rows))
