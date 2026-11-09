# `.otoalbum` project format

A `.otoalbum` is a folder (shown as a single file on macOS):

```
My Album.otoalbum/
  project.sqlite    SQLite (WAL) — the source of truth
  manifest.json     human/tool readable summary (schema_version, app_version, spec, counts)
  media/links.json  where each original lives (path, hash, size, mtime)
  cache/            256 / 1024 / 2048 px previews, keyed by BLAKE3 hash
  snapshots/        VACUUM INTO copies (10 automatic kept, manual kept forever)
  exports/          PDFs, spec sheets, checksums, proofs
  logs/
```

- Originals are linked, never modified. Relink matches by hash; *Consolidate* copies them inside.
- All geometry is stored as integer micrometres, angles as millidegrees, and crops in normalised image space.
- Main tables: `projects`, `album_specs`, `media`, `spreads`, `frames`, `texts`, `shapes`, `templates`, `lab_profiles`, `snapshots`, `exports`, `proof_revisions`, `comments`, `settings`, and `journal` (v2).
- Schema v2 added `spreads.explain`, `media.focus_json` and the `journal` table. Migrations run automatically on open.
- A `session_open` flag detects crashes; the next open offers recovery from the last autosave.
