"""walkspeed.py FRAMES T0_MS T1_MS: Graham's walking speed in an 86Box capture (measure.ps1),
in picture rows a second, walking up out of room 1.

Each frame is compared with the one before, in the band of the window Graham walks up
(x 690-800, picture rows 20-128: the window shows each of the CGA's lines 3 pixels tall
from y 110); the lowest changed row is his feet. A straight line is fitted to them
between T0 and T1. Needs numpy.
"""
import os
import sys

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
import png  # noqa: E402

d, t0, t1 = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
TOP, BOTTOM, ROW = 110 + 3 * 20, 110 + 3 * 128, 2.98
prev, pts = None, []
for name in sorted(f for f in os.listdir(d) if f.endswith('.png')):
    t = int(name[:-4])
    if not (t0 <= t <= t1):
        continue
    w, h, bpp, px = png.read(os.path.join(d, name))
    a = np.frombuffer(px, np.uint8).reshape(h, w, bpp)[TOP:BOTTOM, 690:800, :3].astype(int)
    if prev is not None:
        ys = np.nonzero((np.abs(a - prev).sum(2) > 60).any(1))[0]
        if len(ys) > 3:
            pts.append((t / 1000, 20 + ys.max() / ROW))
    prev = a
t = np.array([p[0] for p in pts])
y = np.array([p[1] for p in pts])
print(f'{len(pts)} frames: {abs(np.polyfit(t, y, 1)[0]):.2f} picture rows a second')
