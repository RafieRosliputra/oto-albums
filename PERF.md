# Performance

**Machine**: Apple M1 Pro (10 cores), 16 GB RAM, macOS, internal SSD. **Build**: `--release` (LTO thin). **Date**: 25 Oct 2026.
Photos are synthetic (`oto-cli synth`, quality-88 JPEG). Real camera JPEGs of the same pixel count decode in a similar time.

| Target (spec) | Goal | Measured | How |
|---|---|---|---|
| Import 500 JPEG + thumbnails | < 30 s | **300 × 24 MP (6000×4000, 349 MB) in 13.4 s** (≈ 45 ms/photo, all cores), so ≈ 22 s for 500 by extrapolation | `oto-cli synth /tmp/p300 300 6000 && oto-cli bench --import /tmp/p300` |
| Import (UI) | — | 80 × 3000 px photos in 3.2 s, previews streaming in while the UI stays responsive | app toast + log |
| Auto Build, 30 spreads | < 3 s | **300 photos → 30 spreads × 3 variants in 3–4 ms** (about 1 ms per variant) | `oto-cli bench --photos 300 --spreads 30` |
| Auto Build (golden) | — | 80 photos → 20 spreads × 3 variants in 1–2 ms | `oto-cli golden` |
| Export 20 spreads, 300 dpi | < 2 min | **PDF/X-4 (20 pages, 80 images) + cover + spec sheet + 20 JPEGs in 4.7 s** | `oto-cli golden` step 13 |
| Export from UI (JPEG pass-through, PDF only) | — | 10 spreads, 80 photos, 27 MB PDF in 0.1 s (identity crops reuse the original JPEG bytes) | app export dialog |
| Save + close + reopen, compare | — | identical to 1 µm; the whole golden path runs in 16.5 s | `oto-cli golden` |
| Template change | < 100 ms | feels instant; the preview is cached per template. **Not instrumented.** | — |
| Cold start | < 2.5 s | the window appears in well under a second on this machine. **Not instrumented.** | — |
| Open 1,000-photo project (warm cache) | < 3 s | **not measured** | — |
| Pan/zoom 60 fps at 4K | 60 fps | **not measured** on a 4K display; the canvas is GPU-drawn (meshes + textures) | — |
| Memory | < 2 GB UI | GPU texture cache ≈ 49 MB for 88 textures (shown in the status bar) | status bar |

## How to reproduce

```sh
cargo build --release -p oto-cli
./target/release/oto-cli synth /tmp/oto-bench/p300 300 6000
./target/release/oto-cli bench --photos 300 --spreads 30 --import /tmp/oto-bench/p300
./target/release/oto-cli golden /tmp/oto-golden
```

## Notes

- Decoding and previews run on worker threads. The UI thread only uploads finished textures under an LRU budget.
- The 2048 px preview is the largest image the UI loads. Full-resolution decoding happens only in export.
- No `criterion` benches yet. The numbers come from the timed CLI commands above, run 2–3 times; the spread between runs was under 10%. Criterion benches are listed as a follow-up in STATUS.md.
