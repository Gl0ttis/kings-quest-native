"""compositefit.py SHOT.png VISUAL.bin: fit the composite colour model (src/composite.rs)
to 86Box's screen of a room.

SHOT is 86Box's own screenshot of the room (tools/86box/shots.ps1: 960 x 600, each of
the CGA's 640 dots 1.5 pixels wide and each line 3 tall), VISUAL the room picture's
colour numbers from kqextract-pc (extracted/res/room/NNN.visual.bin, 168 rows of 160).

It finds the picture in the shot, takes 86Box's colour of each picture colour from the
middle of areas of that colour, and fits the model's three numbers to them: the hue
(the subcarrier's phase at the line's first dot), the saturation, and the foreground's
level. Prints the colours, the fit and how close it is. Needs numpy.
"""
import math
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import png  # noqa: E402

# The engine's table (DS:$12BA): PCjr colour number -> 4-dot pattern, leftmost dot high.
PATTERN = [0x0, 0x2, 0x1, 0x9, 0x4, 0xA, 0x8, 0x5, 0x0, 0x3, 0xB, 0x7, 0xE, 0x6, 0xD, 0xF]


def measured(shot, visual):
    w, h, bpp, px = png.read(shot)
    img = np.frombuffer(px, np.uint8).reshape(h, w, bpp)[:, :, :3].astype(float)
    pic = np.frombuffer(open(visual, 'rb').read(), np.uint8).reshape(168, 160)
    # Pixels in the middle of an area of one colour (5 x 3).
    inner = np.zeros_like(pic, bool)
    inner[1:-1, 2:-2] = True
    for dy in (-1, 0, 1):
        for dx in (-2, -1, 0, 1, 2):
            inner[1:-1, 2:-2] &= pic[1 + dy:167 + dy, 2 + dx:158 + dx] == pic[1:-1, 2:-2]
    def sample(c, ox, oy):
        ys, xs = np.nonzero(inner & (pic == c))
        return img[np.clip(ys * 3 + 1 + oy, 0, h - 1), np.clip(xs * 6 + 3 + ox, 0, w - 1)]
    # Where the picture is in the shot: the offset with the steadiest colours.
    best = min(((sum(((v - np.median(v, 0)) ** 2).sum() for v in (sample(c, ox, oy) for c in range(16)) if len(v) >= 20), ox, oy)
                for oy in range(-6, 7) for ox in range(-12, 13)))
    out = {}
    for c in range(16):
        v = sample(c, best[1], best[2])
        if len(v) >= 20:
            out.setdefault(PATTERN[c], []).append(np.median(v, 0))
    return {p: np.mean(v, 0) for p, v in out.items()}


def model(pattern, hue, sat, level):
    bits = [(pattern >> (3 - k)) & 1 for k in range(4)]
    y = sum(bits) * level / 4
    i = sum(b * math.cos(hue + math.pi / 2 * k) for k, b in enumerate(bits)) * level / 2 * sat
    q = sum(b * math.sin(hue + math.pi / 2 * k) for k, b in enumerate(bits)) * level / 2 * sat
    rgb = (y + 0.956 * i + 0.621 * q, y - 0.272 * i - 0.647 * q, y - 1.106 * i + 1.703 * q)
    return np.clip(np.array(rgb), 0, 1) * 255


def main():
    m = measured(sys.argv[1], sys.argv[2])
    err = lambda hue, sat, level: sum(((model(p, hue, sat, level) - v) ** 2).sum() for p, v in m.items())
    e, hue, sat, level = min((err(math.radians(d / 2), s / 100, l / 100), d / 2, s / 100, l / 100)
                             for d in range(720) for s in range(30, 160, 2) for l in range(40, 90, 2))
    print(f"hue {hue} degrees, saturation {sat}, level {level}; RMS {math.sqrt(e / len(m) / 3):.1f}/255")
    for p, v in sorted(m.items()):
        print(f"  {p:04b}: 86Box {v.round().astype(int).tolist()}, model {model(p, math.radians(hue), sat, level).round().astype(int).tolist()}")


if __name__ == '__main__':
    main()
