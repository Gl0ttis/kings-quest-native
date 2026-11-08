# For each action opcode handler: entry, direct operand fetches ($58F2), JSR targets (until first RTS/JMP).
import re,sys
eng=open('extracted/res/engine_4000.bin','rb').read(); m=lambda a:eng[a-0x4000]
lines=open('scratch/engine_linear.asm').read().splitlines()
idx={int(l[:4],16):i for i,l in enumerate(lines) if re.match(r'^[0-9A-F]{4} ',l)}
for op in range(0x44):
    a=m(0x6AF3+2*op)|m(0x6AF4+2*op)<<8
    i=idx.get(a); fetch=0; calls=[]; n=0
    while i is not None and n<60:
        l=lines[i]; n+=1
        if 'JSR $58F2' in l: fetch+=1
        elif 'JSR' in l: calls.append(l.split('$')[-1])
        if re.search(r'\b(RTS|JMP)\b',l): calls.append('->'+l.split()[-1] if 'JMP' in l else 'RTS'); break
        i+=1
    print(f"{op:02X} ${a:04X} fetch={fetch} {' '.join(calls)}")
