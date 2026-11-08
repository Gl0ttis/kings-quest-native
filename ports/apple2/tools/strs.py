import sys,re
for p in sys.argv[1:]:
    b=open(p,'rb').read()
    print("==",p)
    for hi in (False,True):
        d=bytes((c&0x7f) if (c&0x80) else 0 for c in b) if hi else b
        for m in re.finditer(rb'[ -~]{10,}',d):
            o=m.start(); print(f"  {'H' if hi else 'L'} T{o//4096:02}S{(o//256)%16:X}+{o%256:02X} {m.group()[:90].decode()}")
