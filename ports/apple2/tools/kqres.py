import sys
D={n:open(f'extracted/disk{n}.do','rb').read() for n in (1,2,3)}
def read_res(disk,t,s,o):
    img=D[disk]; pos=(t*16+s)*256+o
    # stream: logical sectors consecutive in .do order == track*16+sector linear
    n=img[pos]|img[pos+1]<<8
    return n,img[pos+2:pos+2+n]
def rec(b): return (b[1]>>4,b[0],b[1]&15,b[2])
