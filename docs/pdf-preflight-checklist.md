# PDF and preflight checklist

## What every print export contains
- PDF 1.6 with an OutputIntent `GTS_PDFX` and the embedded output ICC (sRGB by default)
- TrimBox and BleedBox on every page, with the bleed taken from the lab profile (usually 3 mm)
- Images as ICCBased; identity-crop sRGB JPEGs are passed through without re-encoding
- Only bundled OFL fonts, embedded as CID subsets with ToUnicode
- XMP with `pdfxid:GTS_PDFXVersion = PDF/X-4`, document/instance IDs, `Trapped = False`
- A self-check after writing (lopdf); `tools/validate_pdf.py` adds a qpdf/pikepdf check. **Not a third-party PDF/X certification.**

## Preflight codes
Critical blocks export. An *override* needs a written reason, which is stored in the export audit.

| Code | Severity | Override | Why it matters |
|---|---|---|---|
| PAGE_COUNT | Critical | yes | Labs reject or pad unsupported page counts |
| SPEC_MISMATCH | Warning | yes | Size differs from the lab profile |
| LAB_EXAMPLE | Info | – | Example profiles are not confirmed by a real lab |
| ICC_MISSING | Critical | no | The output profile must be embedded |
| BLANK_SPREAD | Warning | yes | Prints as empty paper |
| EMPTY_FRAME | Warning | yes | Prints as a blank box |
| MISSING_IMAGE | Critical | no | The original is needed for print (use Relink) |
| IMAGE_CHANGED | Warning | yes | The file was edited after import |
| DUPLICATE_ON_SPREAD | Warning | yes | The same photo twice on one spread |
| SIMILAR_ON_SPREAD | Info | – | Near-duplicates side by side |
| DPI_BLOCK | Critical | no | Below 150 dpi |
| DPI_LOW | Critical | yes | 150–200 dpi |
| DPI_WARN | Warning | yes | Below 250 dpi |
| PROFILE_ASSUMED | Info | – | A non-sRGB image is treated as sRGB |
| HEAVY_CROP | Info | – | The slot shape does not suit the photo |
| FOLD_UNCHECKED | Info | – | No focus point, so the fold cannot be checked |
| FACE_IN_GUTTER | Warning | yes | The subject sits in the binding |
| EDGE_SLIVER | Warning | yes | A frame stops just short of the trim |
| IMAGE_NEAR_TRIM | Info | – | The cut may land inside the photo edge |
| FONT_MISSING | Critical | no | Only bundled fonts can be embedded |
| TEXT_OVERFLOW | Warning | yes | Text does not fit its box |
| TEXT_OUTSIDE_TRIM | Critical | yes | Text past the trim is cut off |
| TEXT_UNSAFE | Warning | yes | Text in the fold, wrap or hinge |
