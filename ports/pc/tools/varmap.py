"""varmap.py PCJR_ROOMS PC_ROOMS [V ...]: the PC scripts' number for each of the PCjr
scripts' variables.

The PC scripts are a revision of the PCjr's whose variables are mostly renumbered. Each
room's script (kqextract-pcjr's and kqextract-pc's `res/room/NNN.script.bin`) is aligned
byte by byte with its other version; where one or two bytes differ between unchanged
stretches, the pair (PCjr byte, PC byte) is counted. A number maps to the PC number it
was paired with most, if that is at least 5 times and 3 times all its other pairings
together; otherwise it is taken as unchanged. (The pairs also catch other operands that
were renumbered, so read the counts, not just the answer.)

Prints `PCJR -> PC (counts)` for each V given, or for every number paired.
"""
import collections
import difflib
import os
import sys


def pairs(jr_dir, pc_dir):
    out = collections.defaultdict(collections.Counter)
    for r in range(85):
        a, b = os.path.join(jr_dir, f'{r:03}.script.bin'), os.path.join(pc_dir, f'{r:03}.script.bin')
        if not (os.path.exists(a) and os.path.exists(b)):
            continue
        x, y = open(a, 'rb').read()[8:], open(b, 'rb').read()[8:]
        for op, i1, i2, j1, j2 in difflib.SequenceMatcher(None, x, y, autojunk=False).get_opcodes():
            if op == 'replace' and i2 - i1 == j2 - j1 <= 2:
                for k in range(i2 - i1):
                    out[x[i1 + k]][y[j1 + k]] += 1
    return out


def main():
    p = pairs(sys.argv[1], sys.argv[2])
    want = [int(v) for v in sys.argv[3:]] or sorted(p)
    for v in want:
        c = p.get(v, collections.Counter())
        if c:
            w, n = c.most_common(1)[0]
            to = w if n >= 5 and n >= 3 * (sum(c.values()) - n) else v
        else:
            to = v
        print(f'{v} -> {to} ({dict(c.most_common(4))})')


if __name__ == '__main__':
    main()
