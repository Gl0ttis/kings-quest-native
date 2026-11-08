import re,glob,os,json
exits={}
for f in sorted(glob.glob('extracted/scripts/0*.txt')):
    room=int(os.path.basename(f)[:3])
    if room==0: continue
    txt=open(f).read().splitlines()
    acts={}; cur=None; sect=None; rules=[]
    for l in txt:
        if l.startswith('== '): sect=l[3:]; continue
        if sect=='ACTIONS':
            m=re.match(r'  A([0-9A-F]{4}):',l)
            if m: cur=int(m.group(1),16); acts[cur]=[]; continue
            m=re.match(r'    ([0-9A-F]{4})  (.*)',l)
            if m: acts[cur].append(m.group(2))
        elif sect=='CYCLE RULES':
            m=re.match(r'  ([0-9A-F]{4})  (.*)',l)
            if m: rules.append(m.group(2))
    e={}
    for r in rules:
        m=re.search(r'v3==([1-4]).*-> A([0-9A-F]{4})',r)
        if not m: continue
        a=int(m.group(2),16)
        for t in acts.get(a,[]):
            n=re.match(r'new_room (\d+)',t)
            if n: e[int(m.group(1))]=int(n.group(1))
    exits[room]=e
json.dump(exits,open('routes/room_exits.json','w'))
names={1:'L',2:'R',3:'U',4:'D'}
for r,e in exits.items(): print(r, {names[k]:v for k,v in sorted(e.items())})
