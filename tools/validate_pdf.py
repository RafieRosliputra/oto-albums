#!/usr/bin/env python3
"""External PDF checks for oto-albums exports (used in CI and by hand).

  python3 tools/validate_pdf.py file.pdf [--png out_dir]

Uses pikepdf (qpdf) for a structural check and, if PyMuPDF is installed, renders pages to PNG.
Exit code 1 when qpdf reports errors.
"""
import sys
import pathlib

import pikepdf


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    png_dir = None
    if "--png" in args:
        i = args.index("--png")
        png_dir = pathlib.Path(args[i + 1])
        args = args[:i] + args[i + 2:]
    bad = 0
    for f in args:
        pdf = pikepdf.open(f)
        checker = getattr(pdf, "check_pdf_syntax", None) or getattr(pdf, "check")
        problems = checker()
        print(f"{f}: version {pdf.pdf_version}, {len(pdf.pages)} pages, qpdf problems: {len(problems)}")
        for p in problems:
            print("  ", p)
        bad += len(problems)
        for n, page in enumerate(pdf.pages[:3]):
            mb = [round(float(v), 2) for v in page.MediaBox]
            tb = [round(float(v), 2) for v in page.TrimBox]
            print(f"  page {n + 1}: MediaBox {mb} TrimBox {tb} ({(tb[2]-tb[0]) / 72 * 25.4:.2f} × {(tb[3]-tb[1]) / 72 * 25.4:.2f} mm)")
        oi = pdf.Root.get("/OutputIntents")
        print("  OutputIntent:", oi[0].get("/S") if oi else None, oi[0].get("/OutputConditionIdentifier") if oi else None)
        if png_dir is not None:
            try:
                import fitz  # PyMuPDF
            except ImportError:
                print("  (PyMuPDF not installed; skipping PNG render)")
                continue
            png_dir.mkdir(parents=True, exist_ok=True)
            doc = fitz.open(f)
            for n, page in enumerate(doc):
                if n >= 4:
                    break
                pix = page.get_pixmap(dpi=40)
                out = png_dir / f"{pathlib.Path(f).stem}_p{n + 1}.png"
                pix.save(out)
                print("  rendered", out)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
