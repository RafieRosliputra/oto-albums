# Changelog

## 0.2.0 — 2026-11-09
SmartAlbums-style workflow, with original designs.
- **Two album types**: *Photo album* (custom / flush-mount, printed as full spreads; photos may cross the centre) and *Photobook* (bound, with a centre fold kept clear). Pick one in the New Project wizard (Photo album is the default).
- **200+ designed templates**: 10+ designs for every photo count from 1 to 10 for each type, about 20 % with a full-bleed hero page, and mirrored versions. Frames keep real photo shapes (3:2, 2:3, 1:1), with one small uniform gap (3 mm by default) and equal margins.
- **Template picker** (wizard + editor "Templates…" + ⌘K): Photo album / Photobook tabs, five groups (Essentials, Hero & Full Bleed, Gallery Grids, Storyline, Minimal) and single templates.
- **Design follows the photos**: dropping photos on a spread adds them and picks the best design for the new count (orientation-aware); removing a photo re-designs the spread; ↑/↓ cycle designs; ←/→ change spread.
- **Photo edit mode**: Return or double-click; arrows move the photo inside its frame; Return/Esc leave.
- **Quick Look**: Space on a photo (strip or spread) opens macOS Quick Look; Space again closes it.
- **Layout animation** (220 ms) when the design changes; respects *Reduce motion*.
- **22 formal grid patterns** with 10 pastel inks, a Patterns tab (click or drag onto a spread) and opacity/size/colour controls.
- **Spacing settings** in Album settings: gap, page margin and photobook fold allowance; unlocked spreads are re-laid in one step.

## 0.1.0 — 2026-11-09
First public release (macOS, Apple Silicon).
- Full album workflow: import → timeline → Auto Build (3 variants) → editor → cover → preflight → PDF/X-4 export → local proof
- 76 built-in templates, plus the new **Clean** template generator (frames 3:2/2:3/1:1, one gap, band layout, hero across the fold, matted, bleed and mirrored variants), engine only
- Crash recovery, snapshots, clean quit handling
- English and Bahasa Melayu UI
