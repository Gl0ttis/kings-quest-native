"""walkspeed.py FRAMES T0_MS T1_MS: Graham's walking speed in an 86Box capture (measure.ps1),
in picture rows a second: his light-cyan legs, lowest row per frame (the 800-pixel-wide
view: 2.5 screen pixels a picture row), fitted by least squares between T0 and T1.
Needs Pillow."""
import os
import sys

from PIL import Image

d, t0, t1 = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
pts = []
for n in sorted(os.listdir(d)):
    t = int(n[:-4])
    if not (t0 <= t <= t1):
        continue
    im = Image.open(os.path.join(d, n)).convert('RGB')
    px = im.load()
    ys = [y for y in range(60, 560) for x in range(0, im.width, 2)
          if px[x, y][0] < 120 and px[x, y][1] > 200 and px[x, y][2] > 200]
    if ys:
        pts.append((t / 1000, max(ys) / 2.5))
n = len(pts)
mt = sum(t for t, _ in pts) / n
my = sum(y for _, y in pts) / n
slope = sum((t - mt) * (y - my) for t, y in pts) / sum((t - mt) ** 2 for t, _ in pts)
print(f'{n} frames: {abs(slope):.2f} picture rows a second')
