# Lab profile format (TOML)

Lab specs are data, never code. Put files in `labs/` or add them in the app. The bundled profiles are **examples only** (`source = "example-only"`), and preflight shows `LAB_EXAMPLE` for them.

```toml
schema_version = 1
id = "generic-layflat-10x10"
name = "Generic Layflat 10x10 (example)"
country = "MY"
units = "mm"
source = "example-only"        # or the URL / document you took the spec from
verified_at = "2026-10-25"
binding = "layflat"            # layflat | perfect | single_pages

[[sizes]]                      # one or more sizes
id = "10x10"
page_width_mm = 254
page_height_mm = 254
bleed_mm = 3
trim_outer_mm = 3
trim_inside_mm = 0
safe_mm = 5
gutter_mm = 0
min_pages = 20
max_pages = 100
page_increment = 2

[color]
profile = "sRGB"
pdf_standard = "PDF/X-4"
rendering_intent = "relative_colorimetric"
dpi = 300

[cover]
type = "hardcover"
wrap_mm = 15
hinge_mm = 8
spine_formula = "pages * paper_caliper_mm / 2 + 2.0"   # + - * / ( ) with pages, paper_caliper_mm
paper_caliper_mm = 0.6
min_spine_mm = 6
```
