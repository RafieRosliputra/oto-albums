//! Print export: PDF/X-4 (or PDF 1.7), cover PDF, optional JPEG/TIFF per spread, spec sheet,
//! SHA-256 checksums and an `.otopack` bundle. Runs in the background with progress + cancel.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver};
use image::RgbaImage;
use oto_core::lab::LabProfile;
use oto_core::media::MediaItem;
use oto_core::model::{Adjust, MediaId, Project, Spread};
use oto_core::spec::{Binding, CoverGeometry};
use oto_core::units::{um_to_mm, Um};
use oto_render::raster::{self, RenderOpts, Surface};
use pdf_writer::Content;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::compose::{compose, ComposeOpts, Layout, PdfImages};
use crate::pdf::{jpeg_info, DocInfo, ImageData, PageRes, PdfBuilder, Standard};
use crate::preflight::Report;
use crate::validate::{self, Expect, PdfCheck};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportOptions {
    pub preset: String,
    pub standard: Standard,
    /// 1-based inclusive spread range; None = all.
    pub range: Option<(usize, usize)>,
    pub include_cover: bool,
    pub single_pages: bool,
    pub crop_marks: bool,
    /// None = keep originals (JPEG pass-through, lossless otherwise).
    pub jpeg_quality: Option<u8>,
    /// Downsample images above this effective resolution (None = full resolution).
    pub downsample_dpi: Option<u32>,
    /// Tokens: {project} {client} {date} {lab}
    pub naming: String,
    pub out_dir: PathBuf,
    pub spec_sheet: bool,
    pub jpeg_per_spread: bool,
    pub tiff_per_spread: bool,
    pub otopack: bool,
}

impl ExportOptions {
    pub fn print_default(out_dir: PathBuf, binding: Binding) -> Self {
        ExportOptions {
            preset: "Print — PDF/X-4".into(),
            standard: Standard::PdfX4,
            range: None,
            include_cover: true,
            single_pages: binding == Binding::SinglePages,
            crop_marks: false,
            jpeg_quality: None,
            downsample_dpi: None,
            naming: "{project}_{date}".into(),
            out_dir,
            spec_sheet: true,
            jpeg_per_spread: false,
            tiff_per_spread: false,
            otopack: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedFile {
    pub path: PathBuf,
    pub kind: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub files: Vec<ExportedFile>,
    pub check: Option<PdfCheck>,
    pub cover_check: Option<PdfCheck>,
    pub audit_json: String,
    pub checksums_json: String,
    pub elapsed_ms: u128,
    pub pages: usize,
    pub warnings: Vec<String>,
}

pub enum ExportMsg {
    Progress { done: usize, total: usize, stage: String },
    Done(Box<Result<ExportResult, String>>),
}

pub struct ExportInput {
    pub project: Project,
    pub media: HashMap<MediaId, MediaItem>,
    pub lab: Option<LabProfile>,
    pub preflight: Report,
    pub warnings_acknowledged: bool,
    pub options: ExportOptions,
}

pub fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || "-_ .&".contains(c) { c } else { '_' }).collect();
    let t = t.trim().replace(' ', "_").replace('&', "and");
    if t.is_empty() {
        "album".into()
    } else {
        t
    }
}

pub fn file_stem(o: &ExportOptions, p: &Project, lab: Option<&LabProfile>) -> String {
    let date = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc()).format(&time::macros::format_description!("[year][month][day]")).unwrap_or_default();
    let s = o.naming.replace("{project}", &p.name).replace("{client}", &p.client).replace("{date}", &date).replace("{lab}", lab.map(|l| l.id.as_str()).unwrap_or("custom"));
    sanitize(&s)
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = fs::File::open(p)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("partial");
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot finalize {}: {e}", path.display()))
}

/// Image provider reading originals (JPEG pass-through when nothing needs changing).
pub struct MediaImages<'a> {
    pub media: &'a HashMap<MediaId, MediaItem>,
    pub jpeg_quality: Option<u8>,
}

impl PdfImages for MediaImages<'_> {
    fn key(&self, id: MediaId, adjust: &Adjust, need: Option<(u32, u32)>) -> Option<String> {
        let m = self.media.get(&id)?;
        Some(format!("{}:{:?}:{:?}:{:?}", m.hash, adjust, need, self.jpeg_quality))
    }
    fn size(&self, id: MediaId) -> Option<(u32, u32)> {
        self.media.get(&id).map(|m| (m.width, m.height))
    }
    fn load(&self, id: MediaId, adjust: &Adjust, need: Option<(u32, u32)>) -> Option<ImageData> {
        let m = self.media.get(&id)?;
        let path = Path::new(&m.path);
        let is_jpeg = matches!(m.format.as_str(), "jpeg" | "jpg");
        if is_jpeg && m.orientation <= 1 && adjust.is_identity() && need.is_none() && self.jpeg_quality.is_none() {
            if let Ok(bytes) = fs::read(path) {
                if let Some((w, h, 3)) = jpeg_info(&bytes) {
                    if w == m.width && h == m.height {
                        return Some(ImageData::Jpeg { bytes, w, h });
                    }
                }
            }
        }
        let d = oto_media::load_full(path).ok()?;
        let mut img = d.image.to_rgba8();
        if let Some((nw, nh)) = need {
            let s = (nw as f32 / img.width() as f32).max(nh as f32 / img.height() as f32).min(1.0);
            if s < 0.999 {
                let (w, h) = (((img.width() as f32 * s).round() as u32).max(1), ((img.height() as f32 * s).round() as u32).max(1));
                img = image::imageops::resize(&img, w, h, image::imageops::FilterType::Lanczos3);
            }
        }
        let (w, h) = img.dimensions();
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut alpha = Vec::with_capacity((w * h) as usize);
        for p in img.pixels() {
            let c = raster::adjust_px([p.0[0] as f32, p.0[1] as f32, p.0[2] as f32], adjust);
            rgb.extend(c.map(|v| v.round() as u8));
            alpha.push(p.0[3]);
        }
        let q = self.jpeg_quality.or(if is_jpeg { Some(95) } else { None });
        Some(ImageData::Rgb { rgb, alpha: Some(alpha), w, h, jpeg_quality: q })
    }
}

pub fn cover_geometry(p: &Project, lab: Option<&LabProfile>) -> Option<CoverGeometry> {
    p.cover.as_ref()?;
    let pages = p.page_count();
    lab.filter(|l| Some(l.id.as_str()) == p.lab_id.as_deref()).and_then(|l| l.cover_geometry(&p.spec, pages)).or_else(|| Some(oto_core::lab::generic_cover_geometry(&p.spec, pages)))
}

fn mm(v: Um) -> String {
    format!("{:.2} mm", um_to_mm(v))
}

/// Build the human-readable spec sheet (A4) — a plain PDF with embedded Inter.
pub fn spec_sheet(p: &Project, lab: Option<&LabProfile>, opts: &ExportOptions, files: &[ExportedFile], cover: Option<CoverGeometry>, report: &Report) -> Result<Vec<u8>, String> {
    let s = &p.spec;
    let g = s.geometry();
    let mut rows: Vec<(String, String)> = vec![
        ("Project".into(), p.name.clone()),
        ("Client".into(), if p.client.is_empty() { "—".into() } else { p.client.clone() }),
        (
            "Lab profile".into(),
            match lab {
                Some(l) => format!("{} ({}; source: {}, verified {})", l.name, l.id, l.source, l.verified_at),
                None => "Custom size (no lab profile)".into(),
            },
        ),
        ("Page size (trim)".into(), format!("{} × {}", mm(s.page_width_um), mm(s.page_height_um))),
        ("Spread size (trim)".into(), format!("{} × {}", mm(s.spread_width_um), mm(s.spread_height_um))),
        ("File size incl. bleed".into(), format!("{} × {}", mm(g.media_w()), mm(g.media_h()))),
        ("Bleed".into(), mm(s.bleed_um)),
        ("Trim tolerance (outer / inside)".into(), format!("{} / {}", mm(s.trim_outer_um), mm(s.trim_inside_um))),
        ("Safe zone".into(), mm(s.safe_um)),
        ("Gutter (no-face band)".into(), mm(s.gutter_um)),
        ("Spreads / pages".into(), format!("{} / {}", p.spreads.len(), p.page_count())),
        ("Binding / layout".into(), format!("{:?} / {}", s.binding, if opts.single_pages { "single pages" } else { "spreads" })),
        ("Output profile".into(), s.output_profile.name.clone()),
        ("PDF standard".into(), format!("{} (structure self-checked; not certified)", opts.standard.label())),
        (
            "Images".into(),
            match (opts.jpeg_quality, opts.downsample_dpi) {
                (None, None) => "Full resolution; JPEG originals embedded unchanged".into(),
                (q, d) => format!("JPEG quality {:?}, downsample above {:?} dpi", q, d),
            },
        ),
        ("Crop marks".into(), if opts.crop_marks { "Yes".into() } else { "No".into() }),
    ];
    if let Some(c) = cover {
        rows.push(("Cover (trim, incl. wrap)".into(), format!("{} × {} (spine {}, wrap {}, hinge {})", mm(c.trim_w()), mm(c.trim_h()), mm(c.spine_w), mm(c.wrap), mm(c.hinge))));
    }
    let overrides: Vec<String> = report.issues.iter().filter_map(|i| i.overridden.as_ref().map(|r| format!("{} · {} — {}", i.where_label(), i.code, r))).collect();
    rows.push((
        "Preflight".into(),
        format!("{} critical, {} warnings open; {} overridden", report.count(crate::preflight::Severity::Critical), report.count(crate::preflight::Severity::Warning), overrides.len()),
    ));
    for o in overrides {
        rows.push(("  Override".into(), o));
    }
    rows.push(("Exported".into(), oto_core::model::now_iso()));
    rows.push(("Software".into(), format!("oto-albums {}", env!("CARGO_PKG_VERSION"))));
    for f in files {
        rows.push((format!("SHA-256 {}", f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), f.sha256.clone()));
    }

    let mut b = PdfBuilder::new(DocInfo {
        title: format!("{} — spec sheet", p.name),
        author: "oto-albums".into(),
        standard: Standard::Pdf17,
        output_condition: "sRGB IEC61966-2.1".into(),
        icc: None,
        id_seed: format!("{}-spec", p.id),
    });
    let (pw, ph) = (595.28f32, 841.89f32);
    let margin = 48.0;
    let mut rows_iter = rows.into_iter().peekable();
    let mut page_no = 0;
    while rows_iter.peek().is_some() || page_no == 0 {
        page_no += 1;
        let mut c = Content::new();
        let mut res = PageRes::default();
        let mut y = ph - margin;
        let title_style = oto_core::model::TextStyle {
            font_family: "Inter".into(),
            size_pt: 18.0,
            bold: true,
            color: oto_core::model::Rgba::CHARCOAL,
            align: oto_core::model::Align::Left,
            line_height: 1.2,
            ..Default::default()
        };
        let lay = oto_render::text::layout(if page_no == 1 { "Print specification" } else { "Print specification (cont.)" }, &title_style, pw - 2.0 * margin, 40.0);
        b.text(&mut c, &mut res, &lay, margin, y, [0.09, 0.1, 0.11]);
        y -= 40.0;
        c.set_fill_rgb(0.85, 0.36, 0.22);
        c.rect(margin, y + 14.0, 40.0, 2.0);
        c.fill_nonzero();
        let key_style = oto_core::model::TextStyle {
            font_family: "Inter".into(),
            size_pt: 8.5,
            bold: true,
            color: oto_core::model::Rgba::CHARCOAL,
            align: oto_core::model::Align::Left,
            line_height: 1.3,
            kerning: true,
            ..Default::default()
        };
        let val_style = oto_core::model::TextStyle { bold: false, ..key_style.clone() };
        while let Some((k, v)) = rows_iter.peek().cloned() {
            let kl = oto_render::text::layout(&k, &key_style, 170.0, 200.0);
            let vl = oto_render::text::layout(&v, &val_style, pw - 2.0 * margin - 180.0, 400.0);
            let h = kl.height.max(vl.height) + 6.0;
            if y - h < margin {
                break;
            }
            b.text(&mut c, &mut res, &kl, margin, y, [0.35, 0.36, 0.38]);
            b.text(&mut c, &mut res, &vl, margin + 180.0, y, [0.09, 0.1, 0.11]);
            y -= h;
            rows_iter.next();
        }
        b.add_page(c, res, crate::pdf::PageBoxes { media: (pw, ph), trim: [0.0, 0.0, pw, ph], bleed: [0.0, 0.0, pw, ph], label: None });
    }
    b.finish()
}

fn spreads_in_range(p: &Project, o: &ExportOptions) -> Vec<(usize, Spread)> {
    let n = p.spreads.len();
    let (a, z) = o.range.map(|(a, z)| (a.max(1), z.min(n))).unwrap_or((1, n));
    p.spreads.iter().enumerate().filter(|(i, _)| *i + 1 >= a && *i < z).map(|(i, s)| (i + 1, s.clone())).collect()
}

/// Decoded images for raster export, downscaled to what the surface can use.
struct RasterImages {
    map: HashMap<MediaId, Arc<RgbaImage>>,
}

impl raster::ImageSource for RasterImages {
    fn get(&self, id: MediaId) -> Option<Arc<RgbaImage>> {
        self.map.get(&id).cloned()
    }
}

fn raster_images(s: &Spread, media: &HashMap<MediaId, MediaItem>, max_side: u32) -> RasterImages {
    use rayon::prelude::*;
    let mut ids: Vec<MediaId> = s.media_ids();
    if let Some(b) = &s.background.image {
        ids.push(b.media_id);
    }
    ids.sort();
    ids.dedup();
    let map = ids
        .par_iter()
        .filter_map(|id| {
            let m = media.get(id)?;
            let d = oto_media::load_full(Path::new(&m.path)).ok()?;
            let img = d.image;
            let img = if img.width().max(img.height()) > max_side { img.resize(max_side, max_side, image::imageops::FilterType::Lanczos3) } else { img };
            Some((*id, Arc::new(img.to_rgba8())))
        })
        .collect();
    RasterImages { map }
}

pub fn spawn(input: ExportInput, cancel: Arc<AtomicBool>) -> Receiver<ExportMsg> {
    let (tx, rx) = unbounded();
    let res = std::thread::Builder::new().name("oto-export".into()).spawn(move || {
        let tx2 = tx.clone();
        let r = run(&input, &cancel, &move |done, total, stage| {
            let _ = tx2.send(ExportMsg::Progress { done, total, stage: stage.to_string() });
        });
        let _ = tx.send(ExportMsg::Done(Box::new(r)));
    });
    if let Err(e) = res {
        tracing::error!("cannot start export thread: {e}");
    }
    rx
}

/// Run the export synchronously (the UI calls `spawn`).
pub fn run(inp: &ExportInput, cancel: &AtomicBool, progress: &dyn Fn(usize, usize, &str)) -> Result<ExportResult, String> {
    let t0 = std::time::Instant::now();
    let p = &inp.project;
    let o = &inp.options;
    if !inp.preflight.can_export() {
        let n = inp.preflight.blocking().len();
        return Err(format!("Export blocked: {n} critical preflight issue(s) must be fixed (or overridden where allowed)."));
    }
    let open_warnings = inp.preflight.issues.iter().filter(|i| i.needs_ack()).count();
    if open_warnings > 0 && !inp.warnings_acknowledged {
        return Err(format!("{open_warnings} preflight warning(s) need to be acknowledged before export."));
    }
    fs::create_dir_all(&o.out_dir).map_err(|e| format!("cannot create {}: {e}", o.out_dir.display()))?;
    let stem = file_stem(o, p, inp.lab.as_ref());
    let spreads = spreads_in_range(p, o);
    if spreads.is_empty() {
        return Err("Nothing to export: the album has no spreads in this range.".into());
    }
    let geom = p.spec.geometry();
    let surf = Surface { w: geom.trim_w, h: geom.trim_h, bleed: geom.bleed };
    let marks: Um = if o.crop_marks { 10_000 } else { 0 };
    let cover_g = if o.include_cover { cover_geometry(p, inp.lab.as_ref()) } else { None };
    let total = spreads.len() + usize::from(cover_g.is_some()) + usize::from(o.jpeg_per_spread || o.tiff_per_spread) * spreads.len() + 2;
    let mut done = 0;
    let mut warnings = Vec::new();
    let mut files = Vec::new();
    let imgs = MediaImages { media: &inp.media, jpeg_quality: o.jpeg_quality };
    let copts = ComposeOpts { downsample_dpi: o.downsample_dpi, crop_marks: o.crop_marks };
    let icc = match &p.spec.output_profile.icc_path {
        Some(path) => Some(fs::read(path).map_err(|e| format!("cannot read output ICC {path}: {e}"))?),
        None => None,
    };
    let doc_info = |suffix: &str| DocInfo {
        title: format!("{}{suffix}", p.name),
        author: if p.client.is_empty() { "oto-albums".into() } else { p.client.clone() },
        standard: o.standard,
        output_condition: p.spec.output_profile.name.clone(),
        icc: icc.clone(),
        id_seed: format!("{}-{}{suffix}", p.id, p.modified_at),
    };

    // --- Interior PDF ---
    let mut b = PdfBuilder::new(doc_info(""));
    let mut expect_trim = Vec::new();
    for (no, s) in &spreads {
        if cancel.load(Ordering::Relaxed) {
            return Err("Export cancelled.".into());
        }
        progress(done, total, &format!("Spread {no}"));
        let layouts: Vec<(Layout, String)> = if o.single_pages {
            let pg = (no - 1) * 2 + 1;
            vec![
                (Layout::left_page(surf, marks, geom.page_w, geom.trim_inside), format!("Page {pg} · ")),
                (Layout::right_page(surf, marks, geom.page_w, geom.trim_inside), format!("Page {} · ", pg + 1)),
            ]
        } else {
            vec![(Layout::full(surf, marks), format!("Spread {no} · "))]
        };
        for (lay, label) in layouts {
            let mut c = Content::new();
            let mut res = PageRes::default();
            warnings.extend(compose(&mut b, &mut c, &mut res, &lay, s, &imgs, &copts).into_iter().map(|w| format!("Spread {no}: {w}")));
            let bx = lay.boxes(Some(label));
            expect_trim.push((bx.trim[2] - bx.trim[0], bx.trim[3] - bx.trim[1]));
            b.add_page(c, res, bx);
        }
        done += 1;
    }
    let pages = b.page_count();
    let bytes = b.finish()?;
    let pdf_path = o.out_dir.join(format!("{stem}.pdf"));
    write_file(&pdf_path, &bytes)?;
    progress(done, total, "Validating PDF");
    let check = validate::check(&pdf_path, &Expect { pages, trim_pt: expect_trim, pdfx4: o.standard == Standard::PdfX4 });
    if !check.ok {
        warnings.push(format!("PDF self-check found problems: {}", check.problems.join("; ")));
    }
    files.push(ExportedFile { bytes: bytes.len() as u64, sha256: sha256_file(&pdf_path).map_err(|e| e.to_string())?, path: pdf_path, kind: "print-pdf".into() });
    done += 1;

    // --- Cover PDF ---
    let mut cover_check = None;
    if let (Some(cg), Some(cover)) = (cover_g, &p.cover) {
        progress(done, total, "Cover");
        let mut cb = PdfBuilder::new(doc_info(" — cover"));
        let csurf = Surface { w: cg.trim_w(), h: cg.trim_h(), bleed: cg.bleed };
        let lay = Layout::full(csurf, marks);
        let mut c = Content::new();
        let mut res = PageRes::default();
        warnings.extend(compose(&mut cb, &mut c, &mut res, &lay, &cover.content, &imgs, &copts));
        let bx = lay.boxes(Some("Cover".into()));
        let trim = (bx.trim[2] - bx.trim[0], bx.trim[3] - bx.trim[1]);
        cb.add_page(c, res, bx);
        let cbytes = cb.finish()?;
        let cpath = o.out_dir.join(format!("{stem}_cover.pdf"));
        write_file(&cpath, &cbytes)?;
        let ck = validate::check(&cpath, &Expect { pages: 1, trim_pt: vec![trim], pdfx4: o.standard == Standard::PdfX4 });
        if !ck.ok {
            warnings.push(format!("Cover PDF self-check: {}", ck.problems.join("; ")));
        }
        cover_check = Some(ck);
        files.push(ExportedFile { bytes: cbytes.len() as u64, sha256: sha256_file(&cpath).map_err(|e| e.to_string())?, path: cpath, kind: "cover-pdf".into() });
        done += 1;
    }

    // --- JPEG / TIFF per spread ---
    if o.jpeg_per_spread || o.tiff_per_spread {
        let dir = o.out_dir.join(format!("{stem}_images"));
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ropts = RenderOpts { dpi: p.spec.target_dpi as f32, include_bleed: true };
        let (ow, oh) = raster::output_size(&surf, &ropts);
        for (no, s) in &spreads {
            if cancel.load(Ordering::Relaxed) {
                return Err("Export cancelled.".into());
            }
            progress(done, total, &format!("Image {no}"));
            let src = raster_images(s, &inp.media, ow.max(oh));
            let img = raster::render(s, &surf, &ropts, &src);
            if o.jpeg_per_spread {
                let path = dir.join(format!("{stem}_spread{no:02}.jpg"));
                let mut buf = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, o.jpeg_quality.unwrap_or(95)).encode_image(&img).map_err(|e| e.to_string())?;
                write_file(&path, &buf)?;
                files.push(ExportedFile { bytes: buf.len() as u64, sha256: sha256_file(&path).map_err(|e| e.to_string())?, path, kind: "spread-jpeg".into() });
            }
            if o.tiff_per_spread {
                let path = dir.join(format!("{stem}_spread{no:02}.tif"));
                img.save(&path).map_err(|e| e.to_string())?;
                let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                files.push(ExportedFile { bytes, sha256: sha256_file(&path).map_err(|e| e.to_string())?, path, kind: "spread-tiff".into() });
            }
            done += 1;
        }
    }

    // --- Spec sheet, spec.json, checksums ---
    progress(done, total, "Spec sheet");
    if o.spec_sheet {
        let bytes = spec_sheet(p, inp.lab.as_ref(), o, &files, cover_g, &inp.preflight)?;
        let path = o.out_dir.join(format!("{stem}_spec-sheet.pdf"));
        write_file(&path, &bytes)?;
        files.push(ExportedFile { bytes: bytes.len() as u64, sha256: sha256_file(&path).map_err(|e| e.to_string())?, path, kind: "spec-sheet".into() });
    }
    let spec_json = serde_json::json!({
        "project": p.name, "project_id": p.id.to_string(), "lab_profile": p.lab_id, "spec": p.spec,
        "spreads": spreads.len(), "pages": pages, "single_pages": o.single_pages, "standard": o.standard.label(),
        "cover": cover_g.map(|c| serde_json::json!({"trim_w_mm": um_to_mm(c.trim_w()), "trim_h_mm": um_to_mm(c.trim_h()), "spine_mm": um_to_mm(c.spine_w), "wrap_mm": um_to_mm(c.wrap), "hinge_mm": um_to_mm(c.hinge)})),
        "files": files.iter().map(|f| serde_json::json!({"name": f.path.file_name().map(|n| n.to_string_lossy().to_string()), "kind": f.kind, "bytes": f.bytes, "sha256": f.sha256})).collect::<Vec<_>>(),
    });
    let spec_path = o.out_dir.join(format!("{stem}_spec.json"));
    write_file(&spec_path, serde_json::to_string_pretty(&spec_json).map_err(|e| e.to_string())?.as_bytes())?;
    let mut sums = String::new();
    for f in &files {
        let rel = f.path.strip_prefix(&o.out_dir).unwrap_or(&f.path);
        sums.push_str(&format!("{}  {}\n", f.sha256, rel.display()));
    }
    let sums_path = o.out_dir.join(format!("{stem}_checksums.sha256"));
    write_file(&sums_path, sums.as_bytes())?;
    let checksums_json = serde_json::to_string(&files.iter().map(|f| (f.path.display().to_string(), f.sha256.clone())).collect::<Vec<_>>()).unwrap_or_default();

    // --- .otopack ---
    if o.otopack {
        progress(done, total, "Packing");
        let pack_path = o.out_dir.join(format!("{stem}.otopack"));
        let manifest = serde_json::json!({"format": "otopack", "version": 1, "created_at": oto_core::model::now_iso(), "app_version": env!("CARGO_PKG_VERSION"), "spec": spec_json});
        let f = fs::File::create(&pack_path).map_err(|e| e.to_string())?;
        let mut z = zip::ZipWriter::new(f);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("manifest.json", opt).map_err(|e| e.to_string())?;
        z.write_all(serde_json::to_string_pretty(&manifest).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())?;
        for path in files.iter().map(|f| f.path.clone()).chain([spec_path.clone(), sums_path.clone()]) {
            let rel = path.strip_prefix(&o.out_dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            z.start_file(rel, opt).map_err(|e| e.to_string())?;
            let mut src = fs::File::open(&path).map_err(|e| e.to_string())?;
            std::io::copy(&mut src, &mut z).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
        let bytes = fs::metadata(&pack_path).map(|m| m.len()).unwrap_or(0);
        files.push(ExportedFile { bytes, sha256: sha256_file(&pack_path).map_err(|e| e.to_string())?, path: pack_path, kind: "otopack".into() });
    }
    let audit = serde_json::json!({
        "exported_at": oto_core::model::now_iso(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "options": o,
        "warnings_acknowledged": inp.warnings_acknowledged,
        "open_warnings": open_warnings,
        "overrides": inp.preflight.issues.iter().filter(|i| i.overridden.is_some()).map(|i| serde_json::json!({"code": i.code, "where": i.where_label(), "reason": i.overridden})).collect::<Vec<_>>(),
        "pdf_check": check,
        "warnings": warnings,
    });
    progress(total, total, "Done");
    Ok(ExportResult { files, check: Some(check), cover_check, audit_json: audit.to_string(), checksums_json, elapsed_ms: t0.elapsed().as_millis(), pages, warnings })
}
