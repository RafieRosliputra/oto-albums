# Decisions

Short records of choices that shape the code. Each has the context, the decision and the consequence.

## D1 — Native Rust + egui/eframe on wgpu
- **Context**: the spec forbids Electron/Tauri web views and asks for fast, offline, cross-platform software.
- **Decision**: use egui 0.31 immediate-mode UI with wgpu rendering, and AccessKit for screen readers.
- **Consequence**: one binary of about 30 MB with no runtime. Custom widgets (canvas, book map, timeline) are drawn by hand. Native-looking menus are not used; the ⌘K palette covers commands.

## D2 — Integer micrometres for all geometry
- **Context**: albums must reopen and export bit-identically, whatever the OS or float behaviour.
- **Decision**: store µm (`i64`), millidegrees and normalised crops. Floats are only used inside rendering.
- **Consequence**: the reopen test compares spreads on the µm grid. Sub-µm f32 noise never reaches disk.

## D3 — SQLite package, originals linked (not copied)
- **Context**: projects hold thousands of photos and must survive crashes.
- **Decision**: a `.otoalbum` folder (a macOS package) with SQLite in WAL mode, `BEGIN IMMEDIATE` saves, a JSON manifest for humans and tools, and `media/links.json`. Originals stay where they are. Relink and "Consolidate" (copy into the package) are explicit actions.
- **Consequence**: small projects, fast saves and recoverable state. Moving photo folders needs Relink, which matches files by hash.

## D4 — Autosave on a DB thread, plus snapshots
- **Decision**: debounce 2 s after the last edit and commit at least every 5–10 s while editing. A `session_open` flag detects crashes. `VACUUM INTO` takes snapshots (10 automatic ones are kept; manual ones are kept forever) before risky actions (Auto Build apply, restore) and on a timer. A clean quit (window close, ⌘Q or a system quit) saves and clears the flag.

## D5 — Deterministic, explainable solver (no ML)
- **Context**: no AI model with a clear commercial licence is bundled, and photographers must trust the result.
- **Decision**: a classical DP partition plus Hungarian assignment, scored by readable terms. A seeded RNG is used only to shuffle ties. Each spread gets an explanation string.
- **Consequence**: there is no face detector. The focus point comes from a saliency estimate (sharpness/contrast) or the user (Alt-click in crop mode). Preflight says when the fold cannot be checked (`FOLD_UNCHECKED`).

## D6 — When the photos-per-spread maximum cannot hold all photos
- **Decision**: Auto Build never drops photos silently. If `photos / spreads` exceeds the max, it raises the effective max and says so (a warning in the dialog plus the explanation text). The timeline then flags "Too many photos on this spread". The user can add spreads or move photos to the staging area.

## D7 — PDF/X-4 *structure*, honestly labelled
- **Decision**: write every PDF/X-4 structural element and verify it ourselves (lopdf in-app; qpdf/pikepdf in tools).
- **Consequence**: the UI and docs say "checked by oto-albums, not a third-party certification". veraPDF and Acrobat Preflight were not available in this build environment.

## D8 — Colour: sRGB only in v1
- **Decision**: images are embedded as ICCBased sRGB with an sRGB OutputIntent. Images with another embedded profile are not converted; preflight reports `PROFILE_ASSUMED`. A custom output ICC can be chosen per project, and preflight blocks export if it goes missing.
- **Reason**: correct colour conversion needs a CMS (lcms2 via C bindings). It is deferred rather than done halfway.

## D9 — Bundled OFL fonts only
- Only fonts that may legally be embedded are offered: Inter, Lora, Playfair Display, Cormorant Garamond, Montserrat and Great Vibes, all OFL with the licence next to each. System fonts are not used for print, so exports match on every machine.

## D10 — Text rendered by the print engine in the UI
- Text and patterns on the canvas are rasterised by `oto-render` (the same shaping as the PDF) and uploaded as textures, so line breaks on screen match print.

## D11 — Lab specs are data, never code
- Lab profiles are TOML files (`labs/`, or added in the app) with `source` and `verified_at`. The four bundled ones are **generic examples** marked `example-only`, and preflight shows `LAB_EXAMPLE` for them.

## D12 — Clippy allowances
- `too_many_arguments` and `type_complexity` are allowed in `oto-ui`, `oto-print` and `oto-proof`. Render and PDF helpers take plain geometric parameters (x, y, w, h, colour, ...), and wrapping them in structs added noise without safety. Everything else passes `clippy -D warnings`.

## D13 — `unsafe`
- Every crate has `#![forbid(unsafe_code)]`. There is no `unsafe` in project code.
