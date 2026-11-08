//! Integration: synthetic photos → import → Auto Build → text/pattern → preflight → export → validate.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use image::{Rgb, RgbImage};
use oto_core::builtin::all_templates;
use oto_core::geometry::Rect;
use oto_core::model::*;
use oto_core::solver::{auto_build_variants, BuildParams, PhotoInfo};
use oto_core::spec::AlbumSpec;
use oto_media::{CacheLayout, FileStatus};
use oto_print::export::{self, ExportInput, ExportOptions};
use oto_print::preflight;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oto-golden-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Synthetic photo: gradient background + a skin-toned "face" disc. No copyrighted content.
fn synth(w: u32, h: u32, seed: u32) -> RgbImage {
    let (fx, fy, r) = ((w as f32 * (0.3 + (seed % 5) as f32 * 0.1)), h as f32 * 0.42, w.min(h) as f32 * 0.12);
    RgbImage::from_fn(w, h, |x, y| {
        let d = ((x as f32 - fx).powi(2) + (y as f32 - fy).powi(2)).sqrt();
        if d < r {
            Rgb([224, 172, 140])
        } else {
            let t = (x + y) as f32 / (w + h) as f32;
            Rgb([((40.0 + 150.0 * t) as u32 + seed * 7) as u8, (90.0 + 80.0 * (1.0 - t)) as u8, (120 + (seed * 13) % 120) as u8])
        }
    })
}

fn build_library(dir: &std::path::Path, n: u32) -> (Vec<oto_core::media::MediaItem>, CacheLayout) {
    let src = dir.join("photos");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..n {
        let (w, h) = if i % 3 == 0 { (1600, 2400) } else { (2400, 1600) };
        synth(w, h, i).save(src.join(format!("IMG_{i:04}.jpg"))).unwrap();
    }
    let cache = CacheLayout::new(dir.join("cache"));
    cache.ensure().unwrap();
    let mut items: Vec<_> = oto_media::scan(&[src]).iter().map(|p| oto_media::import_one(p, &cache).unwrap()).collect();
    oto_media::assign_similarity(&mut items);
    (items, cache)
}

#[test]
fn golden_path_export_validates() {
    let dir = tmp("export");
    let (media, _cache) = build_library(&dir, 40);
    let spec = AlbumSpec::default(); // generic 10x10 layflat
    let mut project = Project::new("Aina & Danial", spec.clone(), 0);
    let templates = all_templates();
    let photos: Vec<PhotoInfo> = media.iter().map(PhotoInfo::from_media).collect();
    let params = BuildParams { spreads: Some(10), min_per: 1, max_per: 5, ..Default::default() };
    let variants = auto_build_variants(&photos, &templates, &spec, &params);
    assert_eq!(variants.len(), 3);
    project.spreads = variants[0].spreads.clone();
    assert_eq!(project.spreads.len(), 10);
    // Title on spread 1 (inside the right page's safe zone), pattern on spread 2.
    project.spreads[0].texts.push(TextBlock::new(Rect::new(0.56, 0.42, 0.38, 0.12), "Aina & Danial", StylePreset::H1.style(&font_pairs()[0])));
    project.spreads[1].background.pattern = Some(Pattern::new(PatternKind::Waves));
    project.cover = Some(Cover::default());
    let media_map: HashMap<MediaId, _> = media.iter().map(|m| (m.id, m.clone())).collect();
    let status: HashMap<MediaId, FileStatus> = media.iter().map(|m| (m.id, oto_media::file_status(m))).collect();
    let cover_g = export::cover_geometry(&project, None);
    let report = preflight::run(&preflight::Input { project: &project, media: &media_map, lab: None, status: &status, cover_geometry: cover_g });
    assert!(report.can_export(), "blocking: {:?}", report.blocking().iter().map(|i| &i.message).collect::<Vec<_>>());

    let out = dir.join("export");
    let mut opts = ExportOptions::print_default(out.clone(), spec.binding);
    opts.jpeg_per_spread = true;
    opts.otopack = true;
    opts.crop_marks = true;
    let inp = ExportInput { project: project.clone(), media: media_map.clone(), lab: None, preflight: report.clone(), warnings_acknowledged: false, options: opts.clone() };
    // Unacknowledged warnings stop the export.
    if report.issues.iter().any(|i| i.needs_ack()) {
        assert!(export::run(&inp, &AtomicBool::new(false), &|_, _, _| {}).is_err());
    }
    let inp = ExportInput { warnings_acknowledged: true, ..inp };
    let r = export::run(&inp, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
    let check = r.check.clone().unwrap();
    assert!(check.ok, "{:?}", check.problems);
    assert_eq!(check.pages, 10);
    assert_eq!(check.version, "1.6");
    assert!(check.fonts >= 1, "title font embedded");
    assert!(check.images >= 30, "images {}", check.images);
    assert!(r.cover_check.as_ref().unwrap().ok, "{:?}", r.cover_check);
    for kind in ["print-pdf", "cover-pdf", "spread-jpeg", "spec-sheet", "otopack"] {
        assert!(r.files.iter().any(|f| f.kind == kind), "missing {kind}");
    }
    for f in &r.files {
        assert!(f.path.exists());
        assert_eq!(f.sha256, export::sha256_file(&f.path).unwrap());
    }
    let jpeg = r.files.iter().find(|f| f.kind == "spread-jpeg").unwrap();
    let img = image::open(&jpeg.path).unwrap();
    // 514 mm × 260 mm at 300 dpi.
    assert_eq!((img.width(), img.height()), (6071, 3071));
    println!("export dir: {}", out.display());

    // Single-page export doubles the page count.
    let mut o2 = opts.clone();
    o2.single_pages = true;
    o2.jpeg_per_spread = false;
    o2.otopack = false;
    o2.include_cover = false;
    o2.naming = "{project}_single".into();
    let r2 = export::run(&ExportInput { options: o2, ..inp }, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
    let c2 = r2.check.unwrap();
    assert!(c2.ok, "{:?}", c2.problems);
    assert_eq!(c2.pages, 20);
}

#[test]
fn missing_original_blocks_export() {
    let dir = tmp("missing");
    let (media, _) = build_library(&dir, 4);
    let mut project = Project::new("Gone", AlbumSpec::default(), 10);
    let mut f = Frame::new(Rect::new(0.55, 0.1, 0.4, 0.8));
    f.image_id = Some(media[0].id);
    project.spreads[0].frames.push(f);
    std::fs::remove_file(&media[0].path).unwrap();
    let media_map: HashMap<MediaId, _> = media.iter().map(|m| (m.id, m.clone())).collect();
    let status: HashMap<MediaId, FileStatus> = media.iter().map(|m| (m.id, oto_media::file_status(m))).collect();
    let report = preflight::run(&preflight::Input { project: &project, media: &media_map, lab: None, status: &status, cover_geometry: None });
    assert!(!report.can_export());
    let inp =
        ExportInput { project, media: media_map, lab: None, preflight: report, warnings_acknowledged: true, options: ExportOptions::print_default(dir.join("out"), oto_core::spec::Binding::Layflat) };
    let e = export::run(&inp, &AtomicBool::new(false), &|_, _, _| {}).unwrap_err();
    assert!(e.contains("blocked"), "{e}");
}
