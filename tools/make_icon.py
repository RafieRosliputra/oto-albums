#!/usr/bin/env python3
"""Render the oto-albums icon (open book, vermilion fold, charcoal tile) into an .iconset folder."""
import sys
import pathlib

from PIL import Image, ImageDraw

CHARCOAL = (0x17, 0x19, 0x1C, 255)
BONE = (0xF3, 0xEF, 0xE6, 255)
VERMILION = (0xD9, 0x5D, 0x39, 255)


def render(size: int) -> Image.Image:
    s = size * 4  # supersample
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    pad = int(s * 0.08)
    d.rounded_rectangle([pad, pad, s - pad, s - pad], radius=int(s * 0.2), fill=CHARCOAL)
    l, r, top, bot = s * 0.2, s * 0.8, s * 0.32, s * 0.72
    d.polygon([(l, top + s * 0.03), (s * 0.5, top + s * 0.06), (s * 0.5, bot), (l, bot - s * 0.02)], fill=BONE)
    d.polygon([(r, top + s * 0.03), (s * 0.5, top + s * 0.06), (s * 0.5, bot), (r, bot - s * 0.02)], fill=(0xFB, 0xF8, 0xF2, 255))
    d.rectangle([s * 0.49, top + s * 0.05, s * 0.51, bot + s * 0.02], fill=VERMILION)
    d.rectangle([l, bot + s * 0.03, r, bot + s * 0.06], fill=VERMILION)
    return im.resize((size, size), Image.LANCZOS)


def main() -> None:
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for base in (16, 32, 128, 256, 512):
        render(base).save(out / f"icon_{base}x{base}.png")
        render(base * 2).save(out / f"icon_{base}x{base}@2x.png")


if __name__ == "__main__":
    main()
