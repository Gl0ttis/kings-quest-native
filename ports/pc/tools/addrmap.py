"""addrmap.py PCJR_ENGINE PC_ENGINE [ADDR ...]: map code addresses of the PCjr engine to the
PC engine's.

The two engines are the same code, laid out differently: the PC's code is at other
addresses and its data `$606` bytes further on. Both code segments (kqextract-pcjr's and
kqextract-pc's `res/engine.bin`) are disassembled linearly, and each instruction is
reduced to its shape: the mnemonic and operands, with addresses, displacements and
jump targets left out (small constants are kept). A PCjr address maps to the PC address
where the same run of shapes starts, if there is exactly one; the run starts at 8
instructions and grows to 40 until the match is unique. An address with no unique
match is looked for near where its neighbour went (marked `~`, with how many of 16
shapes agree).

With --data, data addresses instead: wherever a run of 16 shapes is found once in each
engine, the two instructions' memory operands are paired, and each PCjr data address is
given the PC address it was paired with most (and how often).

With no ADDR, every address cited as `$XXXX` in the PCjr port's engine sources is
mapped. Prints `PCJR PC LENGTH` per address (`-` where there is no unique match), then a
summary. Needs capstone.
"""
import glob
import os
import re
import sys

import capstone

# The PCjr engine's code runs from $200 (the boot code before it), its data segment
# from $4E30; the PC engine's code from 0, its data from $5110.
PCJR_CODE = (0x200, 0x4E30)
PC_CODE = (0x0, 0x5110)


def shapes(code, start, end):
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_16)
    md.skipdata = True
    out = []
    for i in md.disasm(code[start:end], start):
        ops = re.sub(r'0x[0-9a-f]{3,}', 'N', i.op_str)
        if i.mnemonic.startswith('j') or i.mnemonic in ('call', 'loop', 'loope', 'loopne', 'jcxz'):
            ops = 'T' if not re.search(r'[a-z]{2}', ops.replace('ptr', '')) or 'N' in ops else ops
        out.append((i.address, f'{i.mnemonic} {ops}'))
    return out


def data_map(jr, pc, a, b):
    """The data addresses: along every run of 16 shapes found once in each engine,
    the two instructions' memory operands paired; PCjr address -> {PC address: count}."""
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_16)
    md.detail = True

    def mems(code, addr):
        i = next(md.disasm(code[addr:addr + 8], addr), None)
        if i is None:
            return []
        out = []
        for o in i.operands:
            if o.type == capstone.x86.X86_OP_MEM and o.mem.disp >= 0x10:
                out.append(o.mem.disp & 0xFFFF)
            elif o.type == capstone.x86.X86_OP_IMM and i.mnemonic == 'mov' and 0x100 <= (o.imm & 0xFFFF) < 0x2400:
                out.append(o.imm & 0xFFFF)
        return out
    sa = [s for _, s in a]
    sb = [s for _, s in b]
    index = {}
    for j in range(len(sb) - 16):
        index.setdefault(tuple(sb[j:j + 16]), []).append(j)
    pairs = {}
    for k in range(len(sa) - 16):
        hits = index.get(tuple(sa[k:k + 16]), [])
        if len(hits) != 1:
            continue
        x, y = mems(jr, a[k][0]), mems(pc, b[hits[0]][0])
        for u, v in zip(x, y):
            pairs.setdefault(u, {}).setdefault(v, 0)
            pairs[u][v] += 1
    return pairs


def main():
    data = '--data' in sys.argv
    if data:
        sys.argv.remove('--data')
    jr = open(sys.argv[1], 'rb').read()
    pc = open(sys.argv[2], 'rb').read()
    a, b = shapes(jr, *PCJR_CODE), shapes(pc, *PC_CODE)
    if data:
        pairs = data_map(jr, pc, a, b)
        want = [int(x, 16) for x in sys.argv[3:]] or sorted(pairs)
        for u in want:
            seen = pairs.get(u)
            if not seen:
                print(f'{u:04X} -')
                continue
            v, n = max(seen.items(), key=lambda kv: kv[1])
            print(f'{u:04X} {v:04X} ({n} of {sum(seen.values())} uses, {v - u:+#x})')
        return
    at_a = {addr: k for k, (addr, _) in enumerate(a)}
    sb = [s for _, s in b]
    addrs = [int(x, 16) for x in sys.argv[3:]]
    if not addrs:
        here = os.path.dirname(os.path.abspath(__file__))
        src = glob.glob(os.path.join(here, '../../pcjr/src/**/*.rs'), recursive=True)
        found = set()
        for f in src:
            found |= {int(m, 16) for m in re.findall(r'\$([0-9A-F]{4})\b', open(f, encoding='utf-8').read())}
        addrs = sorted(x for x in found if PCJR_CODE[0] <= x < PCJR_CODE[1])
    mapped = 0
    exact, pending = {}, []
    for x in addrs:
        k = at_a.get(x)
        if k is None:
            print(f'{x:04X} -    (not an instruction start)')
            continue
        hit = None
        for n in range(8, 41, 4):
            pat = [s for _, s in a[k:k + n]]
            starts = [j for j in range(len(sb) - n + 1) if sb[j] == pat[0] and sb[j:j + n] == pat]
            if len(starts) == 1:
                hit = (b[starts[0]][0], n)
                break
            if not starts:
                break
        if hit:
            mapped += 1
            exact[x] = hit[0]
            print(f'{x:04X} {hit[0]:04X} {hit[1]}')
        else:
            pending.append((x, k))
    # The rest: code moves in blocks, so the nearest address mapped before it gives
    # where to look; the most alike run of 16 shapes within $40 bytes of there, if at
    # least 12 are the same.
    at_b = {addr: j for j, (addr, _) in enumerate(b)}
    near = 0
    for x, k in pending:
        before = [y for y in exact if y < x]
        if not before:
            print(f'{x:04X} -')
            continue
        y = max(before)
        guess = exact[y] + (x - y)
        pat = [s for _, s in a[k:k + 16]]
        best = None
        for j, (addr, _) in enumerate(b):
            if abs(addr - guess) <= 0x40:
                same = sum(p == q for p, q in zip(pat, sb[j:j + 16]))
                if best is None or same > best[0] or (same == best[0] and abs(addr - guess) < abs(best[1] - guess)):
                    best = (same, addr)
        if best and best[0] >= 12:
            near += 1
            print(f'{x:04X} {best[1]:04X} ~{best[0]}/16')
        else:
            print(f'{x:04X} -')
    print(f'{mapped} of {len(addrs)} mapped exactly, {near} by their neighbours', file=sys.stderr)


if __name__ == '__main__':
    main()
