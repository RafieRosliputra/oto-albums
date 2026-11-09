# Template format (TOML)

Built-in templates are exported to `templates/` with `oto-cli templates templates/`. Rects are normalised (0..1) to a **region**: `surface`, `left_page`, `right_page`, and for covers `front`, `back`, `spine`.

```toml
id = "oto-airy-05"
name = "Small left"
family = "airy"
kind = "spread"            # spread | cover
photo_count = 1
orientation = "P"          # slot orientations, e.g. "LLP"
style = "airy"
gutter_policy = "safe"     # safe | full_bleed | no_face
version = 1
tags = ["whitespace"]

[[frames]]
id = "l1"
region = "left_page"
x = 0.3
y = 0.27
w = 0.4
h = 0.46
role = "hero"              # hero | support | background
min_aspect = 0.5
max_aspect = 1.52
allow_rotate = false
z = 0
```

Texts use `[[texts]]` with the same rect fields plus `preset` (h1, h2, body, caption, cover_title, cover_subtitle), `placeholder` and `rotation_mdeg`.

The **Clean** set is not stored as files. It is generated for the album's real size by `oto_core::clean::clean_templates()`; see docs/TEMPLATE-STYLE.md.
