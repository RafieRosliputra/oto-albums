# oto-albums "Clean" template style (original; inspired by the general feel of modern album apps)
Do NOT copy any other product's template files, coordinates, names or assets. Generate original layouts from these rules.

1. Photo-first: white page, no borders, no radius, no decoration by default.
2. Frame shapes follow photo shapes: every frame is exactly 3:2, 2:3 or 1:1 measured in mm on the real spread (tolerance 0.5%). Photos are never squeezed into odd ratios.
3. One gap everywhere: a single gap value (default 3 mm) between every pair of neighbouring frames, horizontal and vertical.
4. Band layout: frames sit inside one horizontal band; top margin == bottom margin (default ~10% of trim height). Stacked columns fill the band height exactly so all top/bottom edges line up.
5. Columns: a layout is a left-to-right row of columns; each column is 1 frame or a stack of 2-3 equal frames. Column widths are solved so every frame keeps its aspect and the band height is shared.
6. Hero across the fold is allowed only when the hero is >= 40% of spread width; otherwise no frame may cross the fold (keep >= gap/2 from it).
7. Outer edges: each layout exists in a "matted" version (side margin == top margin) and, where the outermost columns are stacks, a "bleed" version where outer columns run off the outer trim into bleed.
8. Single-page designs: one page holds the layout, the other page is empty or holds a mirrored partner layout.
9. Every layout also exists mirrored (flip horizontally).
10. Coverage: photo counts 1..12; >= 3 distinct layouts per count for 1..8 and >= 2 for 9..12, before mirroring.
11. Fill quality: a single-page layout must use >= 70% of its page's band height and its frames must cover >= 35% of the page area; a spread layout >= 30% of the spread area. Layouts that cannot meet this on a given album size are dropped for that size (thin strips of tiny squares look empty).
12. Single-page layouts are centred horizontally on their page.
