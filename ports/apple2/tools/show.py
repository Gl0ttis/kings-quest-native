# show.py ADDR [N]  - print N lines of engine disassembly starting at ADDR
import sys
rows=open('scratch/engine_linear.asm').read().splitlines()
a=sys.argv[1].upper(); n=int(sys.argv[2]) if len(sys.argv)>2 else 30
i=next(k for k,l in enumerate(rows) if l.startswith(a)); print('\n'.join(rows[i:i+n])); print('-----')
