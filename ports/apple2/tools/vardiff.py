import sys
P={}
for l in open('scratch/varlog.txt'):
    p,v=l.strip().split(':'); P[int(p)]=v.split(',')
for l in open('scratch/ev.txt'):
    head,vals=l.split(': '); n=int(head.split()[-1]); e=vals.strip().split(',')
    p=P[n-1]
    print(n-1, [(i,p[i],e[i]) for i in range(len(e)) if p[i]!=e[i]])
