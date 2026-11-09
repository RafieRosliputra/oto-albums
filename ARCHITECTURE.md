# Architecture

A Cargo workspace of ten crates. The dependencies flow one way: `core` has no I/O, and the UI is a thin shell over the same engine the CLI uses.

```
                    oto-app (bin "oto-albums")        oto-cli (bin)
                              │                           │
                           oto-ui ──── oto-platform        │
                              │                           │
        ┌──────────┬──────────┼───────────┬──────────────┘
     oto-proof  oto-print  oto-store   oto-media
        │          │          │           │
        └──── oto-render ─────┴───────────┘
                   │
                oto-core   (pure: model, geometry, solver, commands, preflight inputs)
```

| Crate | Responsibility |
|---|---|
| **oto-core** | Data model (`Project`, `Spread`, `Frame`, `TextBlock`, `Shape`, `Cover`, `Timeline`), µm geometry, album spec and lab profiles (TOML + spine expression evaluator), templates (TOML, region maps), the 76 built-in templates, crop maths, the Auto Build solver, and the command/undo `Document`. It has no I/O and no threads. |
| **oto-media** | Folder scan, EXIF (kamadak-exif), orientation, decode (zune-jpeg via `image`, PSD composite), BLAKE3 hash, dHash similarity, features (sharpness, brightness, colourfulness, B&W, focus estimate), the 256/1024/2048 cache, background import with progress and cancel, relink, consolidation, and synthetic fixtures. |
| **oto-store** | The `.otoalbum` package: SQLite (WAL) schema with migrations, save/load, `manifest.json`, `media/links.json`, snapshots (`VACUUM INTO`) and restore (backup API), the crash flag, the integrity check, and the export/proof audit records. Also builds the sample project. |
| **oto-render** | The CPU rasteriser that turns a spread into pixels at any DPI. It handles text shaping (rustybuzz, kerning on, ligatures off for predictability), the bundled OFL fonts and the sRGB ICC. The UI uses it too for text and pattern textures, so on-screen text is WYSIWYG with print. |
| **oto-print** | Preflight (about 22 issue codes with why/where/fix/override), the PDF writer (pdf-writer: PDF/X-4 structure, ICCBased images, CID subset fonts with ToUnicode, XMP), composition (spreads, single pages, cover, crop marks), the post-export self-check (lopdf), bundled lab profiles, and the export pipeline (PDF, spec sheet, JPEG/TIFF, SHA-256, `.otopack`). |
| **oto-proof** | The local proof PDF (numbered photos, comment sheet, watermark) and the typed web-proof API contract (v1.1, no server). |
| **oto-ui** | egui shell: hub, wizard, book map, canvas, timeline/photos/templates strip, inspector, dialogs (Auto Build, Preflight, Export, Proof, Snapshots, Relink, Settings, ⌘K palette), i18n (EN → MS table) and themes. |
| **oto-platform** | A trait for things that differ per OS: file dialogs (rfd), reveal/open, file association, app data dir. |
| **oto-app** | `main`: logging to the app data `logs/`, a panic hook that writes `crash-<ts>.log`, and opening a `.otoalbum` passed on the command line. |
| **oto-cli** | Batch and test entry point: preflight, export, proof, validate, sample, fixtures, synth, templates, bench, golden. |

## Units and geometry

- All layout is stored as **integer micrometres** (µm), angles as millidegrees, and crops in normalised oriented-image space (centre, zoom, rotation). Integers make save/reload exact and avoid float drift across platforms.
- Spread surfaces are `trim width × trim height`. Bleed is added at render time. When a frame edge lies within 0.5 mm of the trim, the print rect extends through the bleed (+1 mm overshoot), so "touching the edge" always bleeds correctly.
- Template rects are normalised to a *region* (surface, left_page, right_page, front, back, spine). One template therefore fits any size or aspect.

## Threads (the UI thread never blocks)

| Thread | Work |
|---|---|
| UI (egui) | Input, layout and painting only. |
| DB worker | Every SQLite write: coalesced autosaves, snapshots, restore, close. Requests travel over a channel; replies are polled each frame. |
| Import workers | Decode, hash, previews and features, streamed back as `ImportMsg`. |
| Texture decoders | Load cached previews into GPU textures, with an LRU budget. |
| Export / proof | The full pipeline with progress and cancel, writing to temp files that are renamed at the end. |

## Editing model

`Document::execute(cmd)` stores before/after snapshots of the affected scope (one spread, the cover, the spec, ...). This gives cheap, exact undo with 100 steps. Drags use `execute_merge`, so one gesture is one undo step. Locked spreads refuse edits from Auto Build and template changes.

## Auto Build (solver)

1. **Order**: capture time (natural file-name order as fallback). Rejected photos are skipped and picks are favoured.
2. **Partition** (DP): split the sequence into N spreads. The cost covers density rhythm, time gaps (moments), B&W runs, similar-photo grouping and duplicate guards.
3. **Template choice per group**: score candidate templates by orientation fit, crop kept, focus/faces out of the gutter, effective DPI (hard floor 150), hero slot, colour, sequence variety and novelty. No three spreads in a row from one family; a hero spread every 4–6.
4. **Assignment**: Hungarian algorithm, photos → slots.
5. **Variants**: Classic, Airy and Editorial change weights and template families. They share the same seed (SplitMix64), so results are deterministic, and each spread stores a one-line explanation.

## PDF pipeline

compose (layout per page) → images: JPEG pass-through when the crop is identity and the colour is sRGB, otherwise re-encode at the target DPI → PDF/X-4 structure (OutputIntent GTS_PDFX + ICC, Trim/Bleed boxes, XMP `pdfxid:GTS_PDFXVersion`, file ID, Trapped) → write to a temp file → self-check with lopdf (page count, boxes, OutputIntent, fonts embedded, no RGB-uncalibrated images) → rename → SHA-256 → audit row in the project DB.
