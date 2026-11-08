# usage: dis.py image.do T S nsect loadaddr [start [count]]
import sys
OPS={}
def d(mn,mode,*codes):
    for c in codes: pass
tbl="""
00 BRK imp|01 ORA izx|05 ORA zp|06 ASL zp|08 PHP imp|09 ORA imm|0A ASL acc|0D ORA abs|0E ASL abs
10 BPL rel|11 ORA izy|15 ORA zpx|16 ASL zpx|18 CLC imp|19 ORA aby|1D ORA abx|1E ASL abx
20 JSR abs|21 AND izx|24 BIT zp|25 AND zp|26 ROL zp|28 PLP imp|29 AND imm|2A ROL acc|2C BIT abs|2D AND abs|2E ROL abs
30 BMI rel|31 AND izy|35 AND zpx|36 ROL zpx|38 SEC imp|39 AND aby|3D AND abx|3E ROL abx
40 RTI imp|41 EOR izx|45 EOR zp|46 LSR zp|48 PHA imp|49 EOR imm|4A LSR acc|4C JMP abs|4D EOR abs|4E LSR abs
50 BVC rel|51 EOR izy|55 EOR zpx|56 LSR zpx|58 CLI imp|59 EOR aby|5D EOR abx|5E LSR abx
60 RTS imp|61 ADC izx|65 ADC zp|66 ROR zp|68 PLA imp|69 ADC imm|6A ROR acc|6C JMP ind|6D ADC abs|6E ROR abs
70 BVS rel|71 ADC izy|75 ADC zpx|76 ROR zpx|78 SEI imp|79 ADC aby|7D ADC abx|7E ROR abx
81 STA izx|84 STY zp|85 STA zp|86 STX zp|88 DEY imp|8A TXA imp|8C STY abs|8D STA abs|8E STX abs
90 BCC rel|91 STA izy|94 STY zpx|95 STA zpx|96 STX zpy|98 TYA imp|99 STA aby|9A TXS imp|9D STA abx
A0 LDY imm|A1 LDA izx|A2 LDX imm|A4 LDY zp|A5 LDA zp|A6 LDX zp|A8 TAY imp|A9 LDA imm|AA TAX imp|AC LDY abs|AD LDA abs|AE LDX abs
B0 BCS rel|B1 LDA izy|B4 LDY zpx|B5 LDA zpx|B6 LDX zpy|B8 CLV imp|B9 LDA aby|BA TSX imp|BC LDY abx|BD LDA abx|BE LDX aby
C0 CPY imm|C1 CMP izx|C4 CPY zp|C5 CMP zp|C6 DEC zp|C8 INY imp|C9 CMP imm|CA DEX imp|CC CPY abs|CD CMP abs|CE DEC abs
D0 BNE rel|D1 CMP izy|D5 CMP zpx|D6 DEC zpx|D8 CLD imp|D9 CMP aby|DD CMP abx|DE DEC abx
E0 CPX imm|E1 SBC izx|E4 CPX zp|E5 SBC zp|E6 INC zp|E8 INX imp|E9 SBC imm|EA NOP imp|EC CPX abs|ED SBC abs|EE INC abs
F0 BEQ rel|F1 SBC izy|F5 SBC zpx|F6 INC zpx|F8 SED imp|F9 SBC aby|FD SBC abx|FE INC abx"""
for e in tbl.replace("\n","|").split("|"):
    e=e.strip()
    if e: c,m,md=e.split(); OPS[int(c,16)]=(m,md)
L={'imp':1,'acc':1,'imm':2,'zp':2,'zpx':2,'zpy':2,'izx':2,'izy':2,'rel':2,'abs':3,'abx':3,'aby':3,'ind':3}
img=open(sys.argv[1],'rb').read(); t,s,n,base=int(sys.argv[2]),int(sys.argv[3]),int(sys.argv[4]),int(sys.argv[5],16)
mem=img[(t*16+s)*256:(t*16+s+n)*256]
pc=int(sys.argv[6],16) if len(sys.argv)>6 else base
end=pc+(int(sys.argv[7],16) if len(sys.argv)>7 else len(mem))
while pc<min(end,base+len(mem)):
    o=mem[pc-base]
    if o not in OPS: print(f"{pc:04X}  {o:02X}        .byte"); pc+=1; continue
    m,md=OPS[o]; ln=L[md]; b=mem[pc-base:pc-base+ln]
    if len(b)<ln: break
    a=b[1] if ln==2 else (b[1]|b[2]<<8 if ln==3 else 0)
    f={'imp':'','acc':'A','imm':f'#${a:02X}','zp':f'${a:02X}','zpx':f'${a:02X},X','zpy':f'${a:02X},Y','izx':f'(${a:02X},X)','izy':f'(${a:02X}),Y','rel':f'${(pc+2+(a-256 if a>127 else a))&0xFFFF:04X}','abs':f'${a:04X}','abx':f'${a:04X},X','aby':f'${a:04X},Y','ind':f'(${a:04X})'}[md]
    print(f"{pc:04X}  {' '.join(f'{x:02X}' for x in b):<9} {m} {f}"); pc+=ln
