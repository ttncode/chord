"""Draws assets/chord.ico: a dark macOS-style key with a white ⌘. Needs Pillow and DejaVu Sans Bold.

Run: python3 assets/make-icon.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
SIZE = 1024  # drawn large, scaled down for smooth edges

key = Image.new("RGBA", (SIZE, SIZE))
gradient = Image.new("RGBA", (SIZE, SIZE))
draw = ImageDraw.Draw(gradient)
for y in range(SIZE):
    t = y / (SIZE - 1)
    shade = tuple(round(a + (b - a) * t) for a, b in zip((0x48, 0x48, 0x4C), (0x1C, 0x1C, 0x1E)))
    draw.line([(0, y), (SIZE, y)], fill=shade + (255,))
mask = Image.new("L", (SIZE, SIZE))
margin, radius = 64, 200
ImageDraw.Draw(mask).rounded_rectangle([margin, margin, SIZE - margin, SIZE - margin], radius, fill=255)
key.paste(gradient, mask=mask)
ImageDraw.Draw(key).rounded_rectangle(
    [margin, margin, SIZE - margin, SIZE - margin], radius, outline=(0x6E, 0x6E, 0x73, 255), width=10
)

font = ImageFont.truetype(FONT, 600)
glyph = ImageDraw.Draw(key)
box = glyph.textbbox((0, 0), "⌘", font=font)
x = (SIZE - (box[2] - box[0])) / 2 - box[0]
y = (SIZE - (box[3] - box[1])) / 2 - box[1]
glyph.text((x, y), "⌘", font=font, fill=(0xF5, 0xF5, 0xF7, 255))

out = Path(__file__).with_name("chord.ico")
key.resize((256, 256), Image.LANCZOS).save(out, sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
key.resize((512, 512), Image.LANCZOS).save(out.with_name("chord.png"))
print(out)
