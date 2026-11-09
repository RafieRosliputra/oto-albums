# Status — v0.1.0 (9 Nov 2026)

Target platform for this release: **macOS, Apple Silicon**. Built and tested there only.

## Done (works in the app and is covered by tests or a scripted run)
- Hub (recent projects, sample project), New Project wizard with lab or custom size and a live geometry diagram
- `.otoalbum` package: SQLite in WAL mode, migrations v1→v2, manifest, links, snapshots, restore, integrity check
- Import: folders and files, EXIF and orientation, BLAKE3 duplicates, dHash similarity, 256/1024/2048 previews in the background, relink, consolidate
- Timeline with cut markers, photo grid with rating, reject, pick and bins (nothing is ever auto-deleted)
- 76 built-in templates (65 spread, 11 cover) exported as TOML in `templates/`, plus saving your own templates
- Auto Build: 3 deterministic variants (Classic, Airy, Editorial), "Why this layout?", locked spreads kept, undo
- Editor: frames, crop/pan/zoom/rotate, focus point, text with bundled OFL fonts, shapes, patterns, layers, snapping, guides, compare view, undo/redo (100 steps), ⌘K palette
- 2D cover editor with a spine width taken from the lab formula
- Preflight: about 22 checks with why, where, fix and override (the override reason is audited); critical issues block export
- Export: a PDF/X-4-structured PDF and PDF 1.7, spreads or single pages, cover, spec sheet (PDF + JSON), SHA-256, JPEG/TIFF and `.otopack`
- Local proof PDF with numbered photos and a comment sheet
- Autosave, periodic snapshots, crash detection and recovery, clean save on ⌘Q or a system quit
- English UI with a Bahasa Melayu switch, high-contrast theme, UI scale, AccessKit
- `oto-cli` for preflight, export, proof, validate, sample, fixtures, synth, templates, bench and golden
- **New in this version:** the Clean template generator (`oto-core/src/clean.rs`), see docs/TEMPLATE-STYLE.md

## Not done / known issues
- Clean templates are not wired into the UI yet (Templates strip, Auto Build), and there is no "Flip spread", "Apply template to all spreads" or template filter yet
- Bahasa Melayu translation is partial: untranslated strings fall back to English
- Auto Build raises the photos-per-spread maximum instead of dropping photos (it warns in the dialog)
- PDF/X-4 is checked by oto-albums itself (lopdf and qpdf: 0 problems). It is **not** certified by veraPDF or Acrobat
- The app is ad-hoc signed only, so on first open right-click → Open

## [deferred]
- [deferred] Windows build, installer, DPI manifest and jump list
- [deferred] 3D cover preview
- [deferred] Web proof server (the API contract exists in `oto-proof/src/api.rs`)
- [deferred] Display ICC profile and colour conversion of non-sRGB images (preflight warns `PROFILE_ASSUMED`)
- [deferred] Face detection (no commercially licensed model; uses a focus estimate plus a manual focus point)
- [deferred] PSD layers (the flattened composite is used)
- [deferred] egui_kittest UI tests, criterion benches, cargo-deny in CI
- [deferred] Developer ID signing and notarisation
