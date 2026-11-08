import sys, csv
port = {r['pass']: r for r in csv.DictReader(open(sys.argv[1]))}
emu = {r['pass']: r for r in csv.DictReader(open(sys.argv[2]))}
keys = [('room','room'),('ego_x','ego_x'),('ego_y','ego_y'),('ego_dir','ego_dir'),('score','score'),('vars_hash','vars_hash')]
n = bad = 0
for p, r in port.items():
    e = emu.get(str(int(p) + int(sys.argv[3]) if len(sys.argv) > 3 else p))
    if not e: continue
    n += 1
    if any(r[a] != e[b] for a, b in keys):
        bad += 1
        if bad <= 5: print('pass', p, 'port', [r[a] for a,_ in keys], 'emu', [e[b] for _,b in keys])
last = max(emu, key=int)
print(f"compared {n} passes, {bad} differ; original final score {emu[last]['score']}")
