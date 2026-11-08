import glob,os
def nibbles(b,p):
    while True:
        yield b[p]>>4; yield b[p]&15; p+=1
def decode_cel(b,c):
    h,w=b[c],b[c+1]; it=nibbles(b,c+2); rows=[]; maxw=0
    for r in range(h):
        x=0; row={}
        while True:
            n=next(it)
            if n==8: break
            if n>8: x+=n&7; continue
            if n==0: raise ValueError("zero run")
            col=next(it)
            for k in range(n): row[x]=col; x+=1
        maxw=max(maxw,x); rows.append(row)
    return h,w,rows,maxw
def view(b):
    nl=b[0]; out=[]
    for l in range(nl):
        lo=b[1+2*l]|b[2+2*l]<<8
        ncel=b[lo+3]; cels=[]
        for c in range(ncel):
            co=lo+(b[lo+4+2*c]|b[lo+5+2*c]<<8)
            cels.append((co,)+decode_cel(b,co))
        out.append((lo,b[lo:lo+4],cels))
    return out
if __name__=='__main__':
    bad=0;n=0;over=0
    for f in sorted(glob.glob('extracted/res/obj/*.bin')):
        b=open(f,'rb').read()
        try:
            for lo,hdr,cels in view(b):
                for co,h,w,rows,mw in cels:
                    n+=1
                    if mw>w: over+=1; print(f,"cel@",co,"row width",mw,">",w)
        except Exception as e: bad+=1; print(f,"ERR",e)
    print("cels",n,"views failing",bad,"cels wider than header",over)
