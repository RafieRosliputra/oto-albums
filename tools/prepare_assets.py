#!/usr/bin/env python3
"""Fetch OFL fonts from google/fonts, instantiate static weights, and build an sRGB ICC profile.

Run once (needs network):  python3 tools/prepare_assets.py
Requires: fonttools, pillow. Output lands in assets/ and is committed to the repo,
so normal builds never touch the network.
"""
import io
import pathlib
import urllib.request

from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = pathlib.Path(__file__).resolve().parent.parent
FONTS = ROOT / "assets" / "fonts"
ICC = ROOT / "assets" / "icc"
BASE = "https://raw.githubusercontent.com/google/fonts/main/ofl/"

FAMILIES = [
    ("inter", "Inter[opsz,wght].ttf", "Inter-Italic[opsz,wght].ttf", "Inter",
     [("Regular", 400, False), ("SemiBold", 600, False), ("Bold", 700, False), ("Italic", 400, True), ("BoldItalic", 700, True)]),
    ("lora", "Lora[wght].ttf", "Lora-Italic[wght].ttf", "Lora",
     [("Regular", 400, False), ("Bold", 700, False), ("Italic", 400, True), ("BoldItalic", 700, True)]),
    ("playfairdisplay", "PlayfairDisplay[wght].ttf", "PlayfairDisplay-Italic[wght].ttf", "PlayfairDisplay",
     [("Regular", 400, False), ("Bold", 700, False), ("Italic", 400, True), ("BoldItalic", 700, True)]),
    ("cormorantgaramond", "CormorantGaramond[wght].ttf", "CormorantGaramond-Italic[wght].ttf", "CormorantGaramond",
     [("Regular", 400, False), ("Bold", 700, False), ("Italic", 400, True), ("BoldItalic", 700, True)]),
    ("montserrat", "Montserrat[wght].ttf", "Montserrat-Italic[wght].ttf", "Montserrat",
     [("Regular", 400, False), ("Bold", 700, False), ("Italic", 400, True), ("BoldItalic", 700, True)]),
]
STATIC = [("greatvibes", "GreatVibes-Regular.ttf", "GreatVibes-Regular.ttf")]


def fetch(path: str) -> bytes:
    with urllib.request.urlopen(BASE + path, timeout=60) as r:
        return r.read()


def main() -> None:
    FONTS.mkdir(parents=True, exist_ok=True)
    ICC.mkdir(parents=True, exist_ok=True)
    for d, up, it, fam, styles in FAMILIES:
        (FONTS / f"{fam}-OFL.txt").write_bytes(fetch(f"{d}/OFL.txt"))
        cache = {}
        for style, wght, italic in styles:
            src = it if italic else up
            if src not in cache:
                cache[src] = fetch(f"{d}/{src}")
            font = TTFont(io.BytesIO(cache[src]))
            axes = {a.axisTag for a in font["fvar"].axes}
            loc = {"wght": wght}
            if "opsz" in axes:
                loc["opsz"] = 14 if fam == "Inter" else 24
            static = instancer.instantiateVariableFont(font, loc, updateFontNames=False)
            static.save(FONTS / f"{fam}-{style}.ttf")
            print("wrote", fam, style)
    for d, src, out in STATIC:
        (FONTS / out).write_bytes(fetch(f"{d}/{src}"))
        (FONTS / f"{out.split('-')[0]}-OFL.txt").write_bytes(fetch(f"{d}/OFL.txt"))
        print("wrote", out)

    from PIL import ImageCms
    prof = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB"))
    (ICC / "sRGB-oto.icc").write_bytes(prof.tobytes())
    print("wrote sRGB ICC")


if __name__ == "__main__":
    main()
