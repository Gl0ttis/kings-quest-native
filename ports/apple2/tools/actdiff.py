SEM=[0,3,4,5,8,10,11,12,13,14,15,16,0x18,0x19,0x1A,0x1B,0x1D,0x1E]
P={}
for l in open('scratch/varlog.txt'):
    p,v=l.strip().split(':'); P[int(p)]=v.split('|')[1].split(',')
for l in open('scratch/ea.txt'):
    head,vals=l.split(': '); n=int(head.split()[-1]); e=vals.strip().split(',')
    p=P[n-1]
    d=[(i//31,hex(i%31),p[i],e[i]) for i in range(len(e)) if i%31 in SEM and p[i]!=e[i]]
    if d: print(n-1, d[:12])
