#!/usr/bin/env python3
"""Generates Ronnie's app icon: the "R" of the logo (Metal Mania) in the Dracula theme colors.

Writes assets/icon/icon.png (1024 px, also embedded as the window icon), assets/icon/Ronnie.icns
(macOS, needs iconutil) and assets/icon/ronnie.ico (Windows). Requires Pillow.
"""
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "icon"
FONT = ROOT / "assets" / "fonts" / "MetalMania-Regular.ttf"

SIZE = 1024
# Dracula: background, chrome, pink (accent), purple.
BG_TOP, BG_BOTTOM = (0x28, 0x2A, 0x36), (0x19, 0x1A, 0x21)
PINK, PURPLE = (0xFF, 0x79, 0xC6), (0xBD, 0x93, 0xF9)


def vertical_gradient(size, top, bottom):
    grad = Image.new("RGB", (1, size[1]))
    for y in range(size[1]):
        t = y / (size[1] - 1)
        grad.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))
    return grad.resize(size)


def icon():
    # macOS-style tile: rounded square with a margin, as in Apple's icon grid.
    margin, radius = 100, 185
    tile = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(tile).rounded_rectangle((margin, margin, SIZE - margin, SIZE - margin), radius, fill=255)

    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 110), (0, 12), tile)
    img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(18)))
    bg = vertical_gradient((SIZE, SIZE), BG_TOP, BG_BOTTOM).convert("RGBA")
    bg.putalpha(tile)
    img.alpha_composite(bg)
    # Thin light rim so the tile reads on dark docks.
    rim = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(rim).rounded_rectangle((margin, margin, SIZE - margin, SIZE - margin), radius, outline=255, width=4)
    rim_layer = Image.new("RGBA", (SIZE, SIZE), (0x44, 0x47, 0x5A, 0))
    rim_layer.putalpha(rim)
    img.alpha_composite(rim_layer)

    # The letter, centered on its ink box.
    font = ImageFont.truetype(str(FONT), 700)
    mask = Image.new("L", (SIZE, SIZE), 0)
    draw = ImageDraw.Draw(mask)
    l, t, r, b = draw.textbbox((0, 0), "R", font=font)
    pos = ((SIZE - (r - l)) / 2 - l, (SIZE - (b - t)) / 2 - t + 10)
    draw.text(pos, "R", font=font, fill=255)

    glow = Image.new("RGBA", (SIZE, SIZE), PINK + (0,))
    glow.putalpha(mask.filter(ImageFilter.GaussianBlur(40)).point(lambda v: v * 0.55))
    img.alpha_composite(glow)

    drop = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    drop.paste((0, 0, 0, 170), (0, 16), mask)
    img.alpha_composite(drop.filter(ImageFilter.GaussianBlur(6)))

    outline = mask.filter(ImageFilter.MaxFilter(17))
    dark = Image.new("RGBA", (SIZE, SIZE), (0x0E, 0x0E, 0x14, 255))
    dark.putalpha(outline)
    img.alpha_composite(dark)

    fill = vertical_gradient((SIZE, SIZE), PINK, PURPLE).convert("RGBA")
    fill.putalpha(mask)
    img.alpha_composite(fill)

    # Light top edge, like the sidebar logo.
    edge = ImageChops.subtract(mask, ImageChops.offset(mask, 0, 8))
    shine = Image.new("RGBA", (SIZE, SIZE), (255, 255, 255, 0))
    shine.putalpha(edge.point(lambda v: v * 0.6))
    img.alpha_composite(shine)
    return img


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    img = icon()
    img.save(OUT / "icon.png")
    img.save(OUT / "ronnie.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    if shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            iconset = Path(tmp) / "Ronnie.iconset"
            iconset.mkdir()
            for s in (16, 32, 128, 256, 512):
                img.resize((s, s), Image.LANCZOS).save(iconset / f"icon_{s}x{s}.png")
                img.resize((s * 2, s * 2), Image.LANCZOS).save(iconset / f"icon_{s}x{s}@2x.png")
            subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(OUT / "Ronnie.icns")], check=True)


if __name__ == "__main__":
    main()
