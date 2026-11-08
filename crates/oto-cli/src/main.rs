//! oto-cli: batch preflight/export, fixtures, benchmarks and the automated golden path.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use oto_core::builtin::all_templates;
use oto_core::document::{Command, Document};
use oto_core::geometry::Rect;
use oto_core::model::*;
use oto_core::solver::{auto_build_variants, BuildParams, PhotoInfo};
use oto_media::{CacheLayout, FileStatus};
use oto_print::export::{self, ExportInput, ExportOptions};
use oto_print::preflight::{self, Severity};
use oto_store::Package;

const HELP: &str = "oto-cli — oto-albums batch tool

USAGE:
  oto-cli preflight <project.otoalbum>
  oto-cli export <project.otoalbum> [--out DIR] [--ack] [--jpeg] [--single] [--pdf17] [--pack]
  oto-cli proof <project.otoalbum> [--out DIR]
  oto-cli validate <file.pdf> --pages N [--x4]
  oto-cli sample <DIR>                 create the sample project
  oto-cli fixtures <DIR>               write synthetic test images (no copyrighted content)
  oto-cli synth <DIR> [N] [LONG_SIDE]  write N synthetic photos (default 300 at 6000 px) for import benchmarks
  oto-cli templates <DIR>              write every built-in template as TOML
  oto-cli bench [--photos N] [--spreads N] [--import DIR]
  oto-cli golden <DIR>                 run the scripted golden path end to end
";

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(2);
    }
}

fn arg_val(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn flag(args: &[String], k: &str) -> bool {
    args.iter().any(|a| a == k)
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        print!("{HELP}");
        return Ok(());
    };
    let pos = |i: usize| args.get(i).map(PathBuf::from).ok_or_else(|| anyhow!("missing argument\n\n{HELP}"));
    match cmd {
        "preflight" => {
            let (p, media, lab, _) = load(&pos(1)?)?;
            let r = run_preflight(&p, &media, lab.as_ref());
            for i in &r.issues {
                println!("{:8} {:10} {:20} {}{}", i.severity.label(), i.where_label(), i.code, i.message, i.overridden.as_ref().map(|o| format!(" [overridden: {o}]")).unwrap_or_default());
            }
            println!(
                "\n{} critical, {} warnings, {} info — export {}",
                r.count(Severity::Critical),
                r.count(Severity::Warning),
                r.count(Severity::Info),
                if r.can_export() { "allowed" } else { "BLOCKED" }
            );
            if !r.can_export() {
                std::process::exit(1);
            }
        }
        "export" => {
            let root = pos(1)?;
            let (p, media, lab, _) = load(&root)?;
            let report = run_preflight(&p, &media, lab.as_ref());
            let mut o = ExportOptions::print_default(arg_val(&args, "--out").map(PathBuf::from).unwrap_or_else(|| root.join("exports")), p.spec.binding);
            o.jpeg_per_spread = flag(&args, "--jpeg");
            o.single_pages = flag(&args, "--single") || o.single_pages;
            o.otopack = flag(&args, "--pack");
            if flag(&args, "--pdf17") {
                o.standard = oto_print::pdf::Standard::Pdf17;
            }
            let map: HashMap<MediaId, _> = media.iter().map(|m| (m.id, m.clone())).collect();
            let r = export::run(&ExportInput { project: p, media: map, lab, preflight: report, warnings_acknowledged: flag(&args, "--ack"), options: o }, &AtomicBool::new(false), &|d, t, s| {
                eprintln!("[{d}/{t}] {s}")
            })
            .map_err(|e| anyhow!(e))?;
            for f in &r.files {
                println!("{}  {}  ({} bytes)", f.sha256, f.path.display(), f.bytes);
            }
            if let Some(c) = &r.check {
                println!("self-check: {} — {} pages, {} fonts, {} images, PDF {}", if c.ok { "OK" } else { "PROBLEMS" }, c.pages, c.fonts, c.images, c.version);
                for p in &c.problems {
                    println!("  {p}");
                }
            }
        }
        "proof" => {
            let root = pos(1)?;
            let (p, media, lab, _) = load(&root)?;
            let map: HashMap<MediaId, _> = media.iter().map(|m| (m.id, m.clone())).collect();
            let out = arg_val(&args, "--out").map(PathBuf::from).unwrap_or_else(|| oto_proof::local::default_proof_dir(&root));
            let cg = export::cover_geometry(&p, lab.as_ref());
            let r = oto_proof::local::export_local_proof(&p, &map, &CacheLayout::new(root.join("cache")), cg, &oto_proof::local::ProofOptions::new(out)).map_err(|e| anyhow!(e))?;
            println!("proof: {} ({} pages)", r.pdf.display(), r.pages);
        }
        "validate" => {
            let f = pos(1)?;
            let pages: usize = arg_val(&args, "--pages").ok_or_else(|| anyhow!("--pages N required"))?.parse()?;
            let c = oto_print::validate::check(&f, &oto_print::validate::Expect { pages, trim_pt: vec![], pdfx4: flag(&args, "--x4") });
            println!("{}", serde_json::to_string_pretty(&c)?);
            if !c.ok {
                std::process::exit(1);
            }
        }
        "sample" => {
            let d = pos(1)?;
            std::fs::create_dir_all(&d)?;
            println!("{}", oto_store::sample::create_sample(&d)?.display());
        }
        "fixtures" => fixtures(&pos(1)?)?,
        "synth" => {
            let n: u32 = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(300);
            let long: u32 = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(6000);
            let t0 = Instant::now();
            let files = oto_media::synth::write_set(std::path::Path::new(&pos(1)?), n, long)?;
            println!("wrote {} synthetic photos ({long} px long side) in {:.1} s", files.len(), t0.elapsed().as_secs_f32());
        }
        "templates" => {
            let d = pos(1)?;
            std::fs::create_dir_all(&d)?;
            let all = all_templates();
            for t in &all {
                std::fs::write(d.join(format!("{}.toml", t.id)), t.to_toml())?;
            }
            println!("wrote {} templates to {}", all.len(), d.display());
        }
        "bench" => bench(&args)?,
        "golden" => golden(&pos(1)?)?,
        _ => print!("{HELP}"),
    }
    Ok(())
}

type Loaded = (Project, Vec<oto_core::media::MediaItem>, Option<oto_core::lab::LabProfile>, Package);

fn load(root: &Path) -> Result<Loaded> {
    let (pkg, _) = Package::open(root).with_context(|| format!("opening {}", root.display()))?;
    let (p, media) = pkg.load()?;
    let lab = p
        .lab_id
        .as_ref()
        .and_then(|id| oto_print::labs::bundled().into_iter().find(|l| &l.id == id).or_else(|| pkg.lab_profile_toml(id).ok().flatten().and_then(|s| oto_core::lab::LabProfile::from_toml(&s).ok())));
    Ok((p, media, lab, pkg))
}

fn run_preflight(p: &Project, media: &[oto_core::media::MediaItem], lab: Option<&oto_core::lab::LabProfile>) -> preflight::Report {
    let map: HashMap<MediaId, _> = media.iter().map(|m| (m.id, m.clone())).collect();
    let status: HashMap<MediaId, FileStatus> = media.iter().map(|m| (m.id, oto_media::file_status(m))).collect();
    let cg = export::cover_geometry(p, lab);
    preflight::run(&preflight::Input { project: p, media: &map, lab, status: &status, cover_geometry: cg })
}

/// 24 synthetic images in mixed formats + one low-res + a folder whose file will go missing.
fn fixtures(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let photos = dir.join("photos");
    let paths = oto_media::synth::write_set(&photos, 21, 3000)?;
    // Other formats.
    oto_media::synth::photo(2400, 1600, 101, 2, false).save(photos.join("OTO_png.png"))?;
    let tiff = image::DynamicImage::ImageRgb8(oto_media::synth::photo(1800, 2700, 102, 1, false)).to_rgb16();
    tiff.save(photos.join("OTO_16bit.tif"))?;
    oto_media::synth::photo(900, 600, 103, 1, false).save(photos.join("OTO_lowres.jpg"))?;
    let missing = dir.join("will-go-missing");
    std::fs::create_dir_all(&missing)?;
    oto_media::synth::photo(2000, 1400, 104, 2, false).save(missing.join("OTO_missing.jpg"))?;
    println!("wrote {} files to {}", paths.len() + 4, dir.display());
    Ok(())
}

fn bench(args: &[String]) -> Result<()> {
    let n: usize = arg_val(args, "--photos").map(|s| s.parse()).transpose()?.unwrap_or(300);
    let s: usize = arg_val(args, "--spreads").map(|s| s.parse()).transpose()?.unwrap_or(30);
    let spec = oto_core::spec::AlbumSpec::default();
    let templates = all_templates();
    let photos: Vec<PhotoInfo> = (0..n)
        .map(|i| {
            let land = i % 3 != 1;
            let (w, h) = if land { (6000, 4000) } else { (4000, 6000) };
            PhotoInfo {
                id: new_id(),
                name: format!("P{i}"),
                aspect: w as f32 / h as f32,
                width: w,
                height: h,
                time: Some(1_700_000_000 + i as i64 * 45),
                rating: (i % 6) as u8,
                pick: i % 13 == 0,
                is_bw: i % 11 == 0,
                sharpness: (i % 7) as f32 / 7.0,
                brightness: 0.5,
                colorfulness: 0.4,
                focus: None,
                faces: vec![],
                similar_group: None,
            }
        })
        .collect();
    let t0 = Instant::now();
    let v = auto_build_variants(&photos, &templates, &spec, &BuildParams { spreads: Some(s), min_per: 1, max_per: 8, ..Default::default() });
    println!(
        "auto_build: {n} photos → {s} spreads × 3 variants in {} ms ({})",
        t0.elapsed().as_millis(),
        v.iter().map(|r| format!("{} {} ms", r.style.label(), r.elapsed_ms)).collect::<Vec<_>>().join(", ")
    );
    if let Some(dir) = arg_val(args, "--import") {
        let tmp = std::env::temp_dir().join(format!("oto-bench-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let t0 = Instant::now();
        let rx = oto_media::spawn_import(vec![PathBuf::from(&dir)], CacheLayout::new(&tmp), Default::default(), Default::default());
        let mut n = 0;
        for m in rx.iter() {
            match m {
                oto_media::ImportMsg::Item(_) => n += 1,
                oto_media::ImportMsg::Finished { .. } => break,
                _ => {}
            }
        }
        println!("import: {n} files with thumbnails/proxies/previews in {} ms", t0.elapsed().as_millis());
        let _ = std::fs::remove_dir_all(&tmp);
    }
    Ok(())
}

fn step(n: u32, s: &str) {
    println!("[{n:02}] {s}");
}

/// Scripted golden path (sections 5.2–5.16 of the brief) without the UI.
fn golden(dir: &Path) -> Result<()> {
    let t_all = Instant::now();
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    // 2. New project from the lab preset, 20 spreads, 300 dpi, bleed 3, safe 5.
    let lab = oto_print::labs::bundled().into_iter().find(|l| l.id == "generic-layflat-10x10").ok_or_else(|| anyhow!("lab missing"))?;
    let spec = lab.spec_for_size(&lab.sizes[0]);
    if spec.bleed_um != 3_000 || spec.safe_um != 5_000 || spec.target_dpi != 300 {
        bail!("lab preset does not match the golden path");
    }
    let mut project = Project::new("Aina & Danial", spec.clone(), 20);
    project.lab_id = Some(lab.id.clone());
    project.lab_size_id = Some("10x10".into());
    project.cover = Some(Cover::default());
    let root = dir.join("Aina & Danial.otoalbum");
    let pkg = Package::create(&root, &project, &[])?;
    pkg.save_lab_profile(&lab.id, &lab.to_toml())?;
    pkg.close()?;
    step(2, "project created (Generic Layflat 10x10, 20 spreads, 300 dpi, bleed 3 mm, safe 5 mm)");
    // 3–4. Import 80 synthetic photos.
    let photos = dir.join("photos");
    oto_media::synth::write_set(&photos, 80, 3200)?;
    let cache = CacheLayout::new(root.join("cache"));
    let t0 = Instant::now();
    let rx = oto_media::spawn_import(vec![photos.clone()], cache.clone(), Default::default(), Default::default());
    let mut media = Vec::new();
    for m in rx.iter() {
        match m {
            oto_media::ImportMsg::Item(i) => media.push(*i),
            oto_media::ImportMsg::Finished { .. } => break,
            _ => {}
        }
    }
    oto_media::assign_similarity(&mut media);
    media.sort_by_key(|m| m.natural_key());
    if media.len() != 80 || !media.iter().all(|m| cache.has_all(&m.hash)) {
        bail!("import incomplete: {}", media.len());
    }
    step(4, &format!("imported {} photos with 256/1024/2048 px previews in {} ms", media.len(), t0.elapsed().as_millis()));
    // 5–7. All photos to the timeline → Auto Build 20 spreads, 1–5 per spread, 3 variants → Classic.
    let (mut pkg, _) = Package::open(&root)?;
    let (p, _) = pkg.load()?;
    let mut doc = Document::new(p);
    doc.execute(Command::SetStaged { staged: media.iter().map(|m| m.id).collect() })?;
    let infos: Vec<PhotoInfo> = media.iter().map(PhotoInfo::from_media).collect();
    let templates = all_templates();
    let t0 = Instant::now();
    let variants = auto_build_variants(&infos, &templates, &spec, &BuildParams { spreads: Some(20), min_per: 1, max_per: 5, ..Default::default() });
    let classic = variants.into_iter().find(|v| v.style == oto_core::solver::Style::Classic).ok_or_else(|| anyhow!("no classic variant"))?;
    let mut new = classic.spreads.clone();
    for (n, o) in new.iter_mut().zip(doc.project.spreads.iter()) {
        n.id = o.id;
    }
    doc.execute(Command::SetSpreads { spreads: new, label: "Auto Build".into() })?;
    doc.execute(Command::SetStaged { staged: vec![] })?;
    let placed: usize = doc.project.spreads.iter().map(|s| s.media_ids().len()).sum();
    if placed != 80 || doc.project.spreads.len() != 20 {
        bail!("auto build placed {placed} photos on {} spreads", doc.project.spreads.len());
    }
    step(7, &format!("Auto Build: 3 variants in {} ms; Classic applied (80 photos on 20 spreads)", t0.elapsed().as_millis()));
    // 8. Spread 3: change template (preview first, then apply), undoable.
    let ctx = oto_core::solver::Ctx::new(&doc.project.spec);
    let s3 = doc.project.spreads[2].clone();
    let n3 = s3.media_ids().len() as u32;
    let alt = templates.iter().find(|t| t.photo_count == n3 && t.id != s3.template_id && t.kind == oto_core::template::TemplateKind::Spread).ok_or_else(|| anyhow!("no alternative template"))?;
    let lookup = |id: MediaId| media.iter().find(|m| m.id == id).map(PhotoInfo::from_media);
    let pair = font_pairs()[0].clone();
    let before = doc.project.clone();
    let (preview, extra) = oto_core::solver::apply_template(&ctx, &s3, alt, &lookup, &pair);
    if doc.project != before || !extra.is_empty() {
        bail!("template preview changed the project");
    }
    doc.execute(Command::SetSpread { spread: preview, label: "Template".into() })?;
    step(8, &format!("spread 3 template → '{}' (preview did not touch the project)", alt.name));
    // 9. Crop a photo (zoom in) and keep it legal.
    let mut s = doc.project.spreads[2].clone();
    if let Some(f) = s.frames.iter_mut().find(|f| f.image_id.is_some()) {
        f.crop.zoom = 1.3;
        f.crop_manual = true;
    }
    doc.execute(Command::SetSpread { spread: s, label: "Crop".into() })?;
    step(9, "cropped a photo on spread 3 (zoom 1.3)");
    // 10. Title on spread 1, pattern on spread 2.
    let mut s1 = doc.project.spreads[0].clone();
    let safe = doc.project.spec.geometry().safe_pages()[1].to_norm(doc.project.spec.geometry().trim_w, doc.project.spec.geometry().trim_h);
    s1.texts.push(TextBlock::new(Rect::new(safe.x + 0.02, safe.y + 0.02, safe.w - 0.04, 0.1), "Aina & Danial", StylePreset::H1.style(&pair)));
    doc.execute(Command::SetSpread { spread: s1, label: "Title".into() })?;
    let mut s2 = doc.project.spreads[1].clone();
    s2.background.pattern = Some(Pattern::new(PatternKind::Dots));
    doc.execute(Command::SetSpread { spread: s2, label: "Pattern".into() })?;
    step(10, "title on spread 1, dot pattern on spread 2");
    // 11. Preflight: fix two warnings, override one with a reason.
    let mut report = run_preflight(&doc.project, &media, Some(&lab));
    let warns_before = report.count(Severity::Warning);
    // Create two fixable problems on purpose and fix them through the same code paths the UI uses.
    let mut s5 = doc.project.spreads[4].clone();
    if let Some(f) = s5.frames.first_mut() {
        f.rect_norm.x = 0.003; // ~1.5 mm sliver
    }
    doc.execute(Command::SetSpread { spread: s5, label: "nudge".into() })?;
    let mut s6 = doc.project.spreads[5].clone();
    s6.frames.push(Frame::new(Rect::new(0.6, 0.6, 0.1, 0.1)));
    doc.execute(Command::SetSpread { spread: s6, label: "empty frame".into() })?;
    report = run_preflight(&doc.project, &media, Some(&lab));
    let sliver = report.issues.iter().find(|i| i.code == "EDGE_SLIVER").cloned().ok_or_else(|| anyhow!("expected EDGE_SLIVER"))?;
    let empty = report.issues.iter().find(|i| i.code == "EMPTY_FRAME").cloned().ok_or_else(|| anyhow!("expected EMPTY_FRAME"))?;
    let mut s5 = doc.project.spreads[4].clone();
    if let Some(f) = s5.frame_mut(sliver.item_id.unwrap_or_default()) {
        f.rect_norm.w += f.rect_norm.x;
        f.rect_norm.x = 0.0;
    }
    doc.execute(Command::SetSpread { spread: s5, label: "Fix sliver".into() })?;
    let mut s6 = doc.project.spreads[5].clone();
    s6.remove_item(empty.item_id.unwrap_or_default());
    doc.execute(Command::SetSpread { spread: s6, label: "Fix empty".into() })?;
    report = run_preflight(&doc.project, &media, Some(&lab));
    if report.issues.iter().any(|i| i.code == "EDGE_SLIVER" || i.code == "EMPTY_FRAME") {
        bail!("fixes did not clear the issues");
    }
    let to_override = report.issues.iter().find(|i| i.override_allowed && i.severity == Severity::Warning).cloned();
    if let Some(i) = &to_override {
        let mut o = doc.project.settings.overrides.clone();
        o.push(PreflightOverride { code: i.code.clone(), spread_id: i.spread_id, item_id: i.item_id, reason: "Client approved this in the meeting".into(), at: now_iso() });
        doc.execute(Command::SetOverrides { overrides: o })?;
    }
    report = run_preflight(&doc.project, &media, Some(&lab));
    step(
        11,
        &format!(
            "preflight: {warns_before} warning(s) initially; fixed EDGE_SLIVER + EMPTY_FRAME; overrode {}; export {}",
            to_override.map(|i| i.code).unwrap_or_else(|| "nothing (no warnings left)".into()),
            if report.can_export() { "allowed" } else { "blocked" }
        ),
    );
    // 14 (part). Save.
    pkg.save(&doc.project, &media, "save", doc.revision)?;
    // 12–13. Export PDF/X-4 + spec sheet + JPEG proof per spread; validate.
    let map: HashMap<MediaId, _> = media.iter().map(|m| (m.id, m.clone())).collect();
    let mut o = ExportOptions::print_default(root.join("exports"), spec.binding);
    o.jpeg_per_spread = true;
    let t0 = Instant::now();
    let r = export::run(
        &ExportInput { project: doc.project.clone(), media: map.clone(), lab: Some(lab.clone()), preflight: report.clone(), warnings_acknowledged: true, options: o },
        &AtomicBool::new(false),
        &|_, _, _| {},
    )
    .map_err(|e| anyhow!(e))?;
    let c = r.check.clone().ok_or_else(|| anyhow!("no check"))?;
    if !c.ok || c.pages != 20 {
        bail!("PDF check failed: {:?}", c.problems);
    }
    step(
        13,
        &format!(
            "exported PDF/X-4 ({} pages, {} fonts, {} images, PDF {}) + cover + spec sheet + {} JPEGs in {} ms — self-check OK",
            c.pages,
            c.fonts,
            c.images,
            c.version,
            r.files.iter().filter(|f| f.kind == "spread-jpeg").count(),
            t0.elapsed().as_millis()
        ),
    );
    pkg.record_export("print", &r.files[0].path, "Print — PDF/X-4", &r.audit_json, &r.checksums_json)?;
    // 14. Close and reopen: identical.
    pkg.close()?;
    let (pkg, rep) = Package::open(&root)?;
    let (p2, m2) = pkg.load()?;
    // Geometry is stored as integer micrometres, so compare on that grid (exact to 1 µm).
    let g = doc.project.spec.geometry();
    let diffs = spread_diffs(&doc.project.spreads, &p2.spreads, g.trim_w, g.trim_h);
    if rep.unclean_shutdown || !diffs.is_empty() || m2 != media {
        bail!("reopened project differs: {}", diffs.join("; "));
    }
    step(14, "saved, closed and reopened — spreads (to 1 µm) and media identical");
    // 15. Simulated crash: edit, autosave, drop without closing, reopen → recovery prompt, edit kept.
    let mut doc2 = Document::new(p2);
    let mut s = doc2.project.spreads[7].clone();
    s.notes = "edited right before the crash".into();
    doc2.execute(Command::SetSpread { spread: s, label: "notes".into() })?;
    let mut pkg = pkg;
    pkg.save(&doc2.project, &m2, "autosave", doc2.revision)?;
    drop(pkg); // process "killed": no clean close
    let (pkg, rep) = Package::open(&root)?;
    let (p3, _) = pkg.load()?;
    if !rep.unclean_shutdown || p3.spreads[7].notes != "edited right before the crash" {
        bail!("crash recovery failed");
    }
    step(15, &format!("simulated crash: recovery offered (last autosave {}), last edit preserved", rep.last_commit_at.unwrap_or_default()));
    // 16. Local proof with spread/photo numbers.
    let cg = export::cover_geometry(&p3, Some(&lab));
    let pr = oto_proof::local::export_local_proof(&p3, &map, &cache, cg, &oto_proof::local::ProofOptions::new(root.join("exports/proofs"))).map_err(|e| anyhow!(e))?;
    step(16, &format!("local proof: {} ({} pages, {} numbered photos)", pr.pdf.display(), pr.pages, pr.numbers.len()));
    pkg.close()?;
    println!("\nGolden path OK in {:.1} s — project: {}", t_all.elapsed().as_secs_f32(), root.display());
    Ok(())
}

/// Differences between two spread lists, with rectangles compared on the 1 µm storage grid.
fn spread_diffs(a: &[Spread], b: &[Spread], w: i64, h: i64) -> Vec<String> {
    use oto_core::geometry::RectUm;
    let q = |r: &Rect| RectUm::from_norm(r, w, h);
    let mut out = Vec::new();
    if a.len() != b.len() {
        out.push(format!("spread count {} vs {}", a.len(), b.len()));
        return out;
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let mut xc = x.clone();
        let mut yc = y.clone();
        for (f, g) in xc.frames.iter_mut().zip(yc.frames.iter_mut()) {
            if q(&f.rect_norm) != q(&g.rect_norm) {
                out.push(format!("spread {} frame rect {:?} vs {:?}", i + 1, q(&f.rect_norm), q(&g.rect_norm)));
            }
            f.rect_norm = Rect::UNIT;
            g.rect_norm = Rect::UNIT;
        }
        for (f, g) in xc.texts.iter_mut().zip(yc.texts.iter_mut()) {
            if q(&f.rect_norm) != q(&g.rect_norm) {
                out.push(format!("spread {} text rect", i + 1));
            }
            f.rect_norm = Rect::UNIT;
            g.rect_norm = Rect::UNIT;
        }
        for (f, g) in xc.shapes.iter_mut().zip(yc.shapes.iter_mut()) {
            f.rect_norm = Rect::UNIT;
            g.rect_norm = Rect::UNIT;
        }
        if xc != yc {
            out.push(format!(
                "spread {} other fields: {:?}",
                i + 1,
                (xc.frames.len(), yc.frames.len(), xc.texts == yc.texts, xc.background == yc.background, xc.explain == yc.explain, xc.frames == yc.frames)
            ));
        }
    }
    out
}
