//! Local proof: one page per spread with spread number, numbered photos, gutter line and a
//! watermark, followed by a comment sheet. Works fully offline from the preview cache.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;
use oto_core::media::MediaItem;
use oto_core::model::{MediaId, Project, Rgba, Spread, TextStyle};
use oto_core::spec::CoverGeometry;
use oto_media::{CacheLayout, Level};
use oto_print::pdf::{DocInfo, ImageData, PageBoxes, PageRes, PdfBuilder, Standard};
use oto_render::raster::{self, RenderOpts, Surface};
use pdf_writer::Content;

#[derive(Debug, Clone)]
pub struct ProofOptions {
    pub out_dir: PathBuf,
    /// Raster resolution of the proof images.
    pub dpi: f32,
    pub watermark: String,
    pub note: String,
    pub include_cover: bool,
    pub jpgs: bool,
}

impl ProofOptions {
    pub fn new(out_dir: PathBuf) -> Self {
        ProofOptions { out_dir, dpi: 110.0, watermark: "PROOF".into(), note: String::new(), include_cover: true, jpgs: false }
    }
}

#[derive(Debug, Clone)]
pub struct ProofResult {
    pub pdf: PathBuf,
    pub jpgs: Vec<PathBuf>,
    pub pages: usize,
    /// media id → photo number printed on the proof
    pub numbers: HashMap<MediaId, usize>,
}

struct CacheImages<'a> {
    cache: &'a CacheLayout,
    media: &'a HashMap<MediaId, MediaItem>,
    memo: std::sync::Mutex<HashMap<MediaId, Arc<RgbaImage>>>,
}

impl raster::ImageSource for CacheImages<'_> {
    fn get(&self, id: MediaId) -> Option<Arc<RgbaImage>> {
        if let Some(i) = self.memo.lock().ok()?.get(&id) {
            return Some(i.clone());
        }
        let m = self.media.get(&id)?;
        let img = Arc::new(oto_media::load_cached(self.cache, Level::Preview, &m.hash)?);
        self.memo.lock().ok()?.insert(id, img.clone());
        Some(img)
    }
}

/// Photo numbers in reading order through the book.
pub fn photo_numbers(p: &Project) -> HashMap<MediaId, usize> {
    let mut map = HashMap::new();
    let mut n = 0;
    for s in &p.spreads {
        let mut frames: Vec<_> = s.frames.iter().filter(|f| f.image_id.is_some()).collect();
        frames.sort_by(|a, b| (a.rect_norm.x, a.rect_norm.y).partial_cmp(&(b.rect_norm.x, b.rect_norm.y)).unwrap_or(std::cmp::Ordering::Equal));
        for f in frames {
            if let Some(id) = f.image_id {
                map.entry(id).or_insert_with(|| {
                    n += 1;
                    n
                });
            }
        }
    }
    map
}

fn overlay(img: &mut image::RgbImage, s: &Spread, surf: &Surface, opts: &RenderOpts, numbers: &HashMap<MediaId, usize>, media: &HashMap<MediaId, MediaItem>, watermark: &str, fold: bool) {
    let map = raster::PxMap::new(surf, opts);
    let (w, h) = (img.width(), img.height());
    let px = |mm: f32| mm * opts.dpi / 25.4;
    // Gutter line (dashed).
    if fold {
        let x = (w / 2) as i64;
        let dash = px(3.0).max(2.0) as u32;
        for y in 0..h {
            if (y / dash) % 2 == 0 {
                for dx in -1..=1i64 {
                    let xx = (x + dx).clamp(0, w as i64 - 1) as u32;
                    img.put_pixel(xx, y, image::Rgb([217, 93, 57]));
                }
            }
        }
    }
    // Watermark rows.
    // Sparse, small watermark: visible on every photo without hiding the content.
    let size = px(6.0);
    let mut row = 0;
    let mut y = px(30.0);
    while y < h as f32 {
        let mut x = px(20.0) + if row % 2 == 0 { 0.0 } else { px(60.0) };
        while x < w as f32 {
            raster::draw_label(img, watermark, x, y, size, [205, 200, 192], None);
            x += px(120.0);
        }
        y += px(70.0);
        row += 1;
    }
    // Photo numbers.
    for f in &s.frames {
        let Some(id) = f.image_id else { continue };
        let r = surf.rect(&f.rect_norm);
        let (x, y) = (map.to_px(r.x0) as f32 + px(1.5), map.to_px(r.y0) as f32 + px(1.5));
        let name = media.get(&id).map(|m| m.file_name.as_str()).unwrap_or("?");
        let label = format!("#{} {}", numbers.get(&id).copied().unwrap_or(0), name);
        raster::draw_label(img, &label, x.max(2.0), y.max(2.0), px(3.2).max(9.0), [255, 255, 255], Some([23, 25, 28]));
    }
}

fn text(b: &mut PdfBuilder, c: &mut Content, res: &mut PageRes, s: &str, size: f32, bold: bool, x: f32, y: f32, w: f32, color: [f32; 3]) -> f32 {
    let st = TextStyle { font_family: "Inter".into(), size_pt: size, bold, color: Rgba::CHARCOAL, align: oto_core::model::Align::Left, line_height: 1.3, ..Default::default() };
    let l = oto_render::text::layout(s, &st, w, 1000.0);
    b.text(c, res, &l, x, y, color);
    l.height
}

/// Build the proof PDF (and optional JPGs).
pub fn export_local_proof(p: &Project, media: &HashMap<MediaId, MediaItem>, cache: &CacheLayout, cover: Option<CoverGeometry>, o: &ProofOptions) -> Result<ProofResult, String> {
    std::fs::create_dir_all(&o.out_dir).map_err(|e| e.to_string())?;
    let numbers = photo_numbers(p);
    let src = CacheImages { cache, media, memo: Default::default() };
    let geom = p.spec.geometry();
    let surf = Surface { w: geom.trim_w, h: geom.trim_h, bleed: geom.bleed };
    let ropts = RenderOpts { dpi: o.dpi, include_bleed: false };
    let stamp = time::OffsetDateTime::now_utc().format(&time::macros::format_description!("[year]-[month]-[day] [hour]:[minute] UTC")).unwrap_or_default();
    let mut b = PdfBuilder::new(DocInfo {
        title: format!("{} — proof", p.name),
        author: "oto-albums".into(),
        standard: Standard::Pdf17,
        output_condition: "sRGB IEC61966-2.1".into(),
        icc: None,
        id_seed: format!("{}-proof-{stamp}", p.id),
    });
    let mut jpgs = Vec::new();
    let date =
        time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc()).format(&time::macros::format_description!("[year][month][day]-[hour][minute]")).unwrap_or_default();
    let stem = format!("{}_proof_{date}", oto_print::export::sanitize(&p.name));
    let mut surfaces: Vec<(String, &Spread, Surface, bool)> = Vec::new();
    if o.include_cover {
        if let (Some(c), Some(g)) = (&p.cover, cover) {
            surfaces.push(("Cover".into(), &c.content, Surface { w: g.trim_w(), h: g.trim_h(), bleed: g.bleed }, false));
        }
    }
    for (i, s) in p.spreads.iter().enumerate() {
        surfaces.push((format!("Spread {} of {}", i + 1, p.spreads.len()), s, surf, true));
    }
    let page_w = 842.0f32; // A4 landscape
    let margin = 28.0;
    for (k, (title, s, sf, fold)) in surfaces.iter().enumerate() {
        let mut img = raster::render(s, sf, &ropts, &src);
        overlay(&mut img, s, sf, &ropts, &numbers, media, &o.watermark, *fold);
        if o.jpgs {
            let path = o.out_dir.join(format!("{stem}_{:02}.jpg", k));
            img.save(&path).map_err(|e| e.to_string())?;
            jpgs.push(path);
        }
        let (iw, ih) = img.dimensions();
        let draw_w = page_w - 2.0 * margin;
        let draw_h = draw_w * ih as f32 / iw as f32;
        let page_h = draw_h + 2.0 * margin + 70.0;
        let mut c = Content::new();
        let mut res = PageRes::default();
        let key = format!("proof-{k}");
        let rgb = img.into_raw();
        let name = b.image(&mut res, &key, || Some(ImageData::Rgb { rgb, alpha: None, w: iw, h: ih, jpeg_quality: Some(85) })).ok_or("proof image failed")?;
        let top = page_h - margin;
        text(&mut b, &mut c, &mut res, &format!("{title}  ·  {}", p.name), 13.0, true, margin, top, draw_w, [0.09, 0.1, 0.11]);
        let sub = format!("Proof {stamp}{}  ·  photo numbers match the comment sheet  ·  dashed line = fold", if o.note.is_empty() { String::new() } else { format!(" · {}", o.note) });
        text(&mut b, &mut c, &mut res, &sub, 8.0, false, margin, top - 18.0, draw_w, [0.4, 0.4, 0.42]);
        c.save_state();
        c.transform([draw_w, 0.0, 0.0, draw_h, margin, margin + 18.0]);
        c.x_object(pdf_writer::Name(name.as_bytes()));
        c.restore_state();
        c.set_stroke_rgb(0.8, 0.8, 0.8);
        c.set_line_width(0.5);
        c.rect(margin, margin + 18.0, draw_w, draw_h);
        c.stroke();
        if !s.explain.is_empty() {
            text(&mut b, &mut c, &mut res, &s.explain, 7.0, false, margin, margin + 12.0, draw_w, [0.45, 0.45, 0.47]);
        }
        b.add_page(c, res, PageBoxes { media: (page_w, page_h), trim: [0.0, 0.0, page_w, page_h], bleed: [0.0, 0.0, page_w, page_h], label: Some(format!("{title} ")) });
    }
    // Comment sheet.
    let (pw, ph) = (595.28f32, 841.89f32);
    let mut rows: Vec<(String, String)> = p
        .spreads
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut ns: Vec<usize> = s.media_ids().iter().filter_map(|id| numbers.get(id).copied()).collect();
            ns.sort_unstable();
            (format!("Spread {}", i + 1), ns.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(" "))
        })
        .collect();
    rows.reverse();
    while !rows.is_empty() {
        let mut c = Content::new();
        let mut res = PageRes::default();
        let mut y = ph - 48.0;
        text(&mut b, &mut c, &mut res, &format!("Comment sheet — {}", p.name), 16.0, true, 48.0, y, pw - 96.0, [0.09, 0.1, 0.11]);
        y -= 26.0;
        text(&mut b, &mut c, &mut res, "Write changes next to each spread. Refer to photos by their number, e.g. “swap #12 and #14”, “remove #31”.", 8.5, false, 48.0, y, pw - 96.0, [0.4, 0.4, 0.42]);
        y -= 30.0;
        while let Some((label, nums)) = rows.last().cloned() {
            if y < 90.0 {
                break;
            }
            text(&mut b, &mut c, &mut res, &label, 9.5, true, 48.0, y, 90.0, [0.09, 0.1, 0.11]);
            text(&mut b, &mut c, &mut res, &nums, 7.5, false, 48.0, y - 13.0, 90.0, [0.4, 0.4, 0.42]);
            c.set_stroke_rgb(0.82, 0.8, 0.77);
            c.set_line_width(0.5);
            for k in 0..3 {
                let ly = y - 12.0 - k as f32 * 16.0;
                c.move_to(150.0, ly);
                c.line_to(pw - 48.0, ly);
            }
            c.stroke();
            y -= 58.0;
            rows.pop();
        }
        text(&mut b, &mut c, &mut res, "Approved by: ____________________    Date: ____________", 9.0, false, 48.0, 60.0, pw - 96.0, [0.09, 0.1, 0.11]);
        b.add_page(c, res, PageBoxes { media: (pw, ph), trim: [0.0, 0.0, pw, ph], bleed: [0.0, 0.0, pw, ph], label: Some("Comments ".into()) });
    }
    let pages = b.page_count();
    let bytes = b.finish()?;
    let pdf = o.out_dir.join(format!("{stem}.pdf"));
    std::fs::write(&pdf, bytes).map_err(|e| e.to_string())?;
    Ok(ProofResult { pdf, jpgs, pages, numbers })
}

pub fn default_proof_dir(project_root: &Path) -> PathBuf {
    project_root.join("exports").join("proofs")
}
