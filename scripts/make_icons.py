#!/usr/bin/env python3
"""Generate the app icons (src-tauri/icons). Requires Pillow: pip install pillow

The artwork is a tiny pixel "map": coloured terrain tiles with a map pin.
Replace src-tauri/icons/* with your own artwork any time.
"""
import os
import random
from PIL import Image, ImageDraw

OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")
os.makedirs(OUT, exist_ok=True)

N = 1024
GRID = 8
PAD = 96
cell = (N - 2 * PAD) // GRID

palette = {
    "grass": [(94, 152, 64), (104, 166, 72), (84, 140, 58)],
    "water": [(52, 110, 190), (60, 120, 200)],
    "sand": [(218, 200, 140), (226, 208, 150)],
    "stone": [(124, 128, 136), (136, 140, 148)],
}
layout = [
    "gggggwww",
    "ggggwwww",
    "gggsswww",
    "gggsgggg",
    "ssgggggt",
    "sggggttt",
    "ggggtttt",
    "gggggttt",
]
kind = {"g": "grass", "w": "water", "s": "sand", "t": "stone"}

random.seed(7)
img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([0, 0, N - 1, N - 1], radius=190, fill=(29, 32, 38, 255))
for r, row in enumerate(layout):
    for c, ch in enumerate(row):
        col = random.choice(palette[kind[ch]])
        x0, y0 = PAD + c * cell, PAD + r * cell
        d.rectangle([x0, y0, x0 + cell - 4, y0 + cell - 4], fill=col)

# map pin
cx, cy, rad = 560, 430, 150
d.ellipse([cx - rad, cy - rad, cx + rad, cy + rad], fill=(231, 76, 60, 255))
d.polygon([(cx - rad + 28, cy + 70), (cx + rad - 28, cy + 70), (cx, cy + 330)], fill=(231, 76, 60, 255))
d.ellipse([cx - 62, cy - 62, cx + 62, cy + 62], fill=(255, 255, 255, 255))

img.save(os.path.join(OUT, "icon.png"))
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]:
    img.resize((size, size), Image.LANCZOS).save(os.path.join(OUT, name))
img.save(os.path.join(OUT, "icon.ico"), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
img.save(os.path.join(OUT, "icon.icns"))
print("icons written to", os.path.abspath(OUT))
