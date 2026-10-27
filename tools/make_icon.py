#!/usr/bin/env python3
"""Generate the mysql-front-rs logo: a navy rounded square with a light
database cylinder and a Rust-orange lightning bolt. Outputs:
  crates/mff-gui/assets/logo_256.png  (runtime window icon)
  crates/mff-gui/assets/app.ico       (embedded exe icon, multi-size)
"""
from PIL import Image, ImageDraw
import os

S = 256
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

NAVY_TOP = (27, 42, 65, 255)      # #1B2A41
NAVY_BOT = (37, 59, 92, 255)      # #253B5C
STEEL = (233, 237, 243, 255)      # cylinder body
STEEL_DARK = (176, 188, 205, 255) # cylinder shading
ORANGE = (247, 103, 7, 255)       # Rust orange #F76707
ORANGE_DARK = (217, 72, 15, 255)  # bolt outline

# --- rounded-square background with vertical gradient ---
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle([8, 8, S - 8, S - 8], radius=52, fill=255)
grad = Image.new("RGBA", (S, S))
gd = ImageDraw.Draw(grad)
for y in range(S):
    t = y / S
    gd.line([(0, y), (S, y)], fill=tuple(
        int(a + (b - a) * t) for a, b in zip(NAVY_TOP, NAVY_BOT)))
img.paste(grad, (0, 0), mask)
d = ImageDraw.Draw(img)

# --- database cylinder (centered slightly left) ---
cx, rx = 118, 66
top, bot = 78, 178
ry = 26
d.ellipse([cx - rx, bot - ry, cx + rx, bot + ry], fill=STEEL_DARK)
d.rectangle([cx - rx, (top + bot) // 2, cx + rx, bot], fill=STEEL)
d.ellipse([cx - rx, top - ry, cx + rx, top + ry], fill=STEEL)
d.ellipse([cx - rx, top - ry, cx + rx, top + ry], outline=STEEL_DARK, width=4)
# separator bands
for frac in (0.38, 0.72):
    y = top + (bot - top) * frac
    d.arc([cx - rx, y - ry, cx + rx, y + ry], 0, 180, fill=STEEL_DARK, width=4)

# --- lightning bolt (right side, overlapping) ---
bolt = [(196, 44), (150, 132), (182, 132), (160, 212), (216, 118), (182, 118), (212, 44)]
d.polygon(bolt, fill=ORANGE, outline=ORANGE_DARK)

out = os.path.join(os.path.dirname(__file__), "..", "crates", "mff-gui", "assets")
os.makedirs(out, exist_ok=True)
png = os.path.join(out, "logo_256.png")
ico = os.path.join(out, "app.ico")
img.save(png)
img.save(ico, sizes=[(256, 256), (64, 64), (48, 48), (32, 32), (24, 24), (16, 16)])
print("wrote", os.path.abspath(png))
print("wrote", os.path.abspath(ico))
