//! Preflight: every print risk is reported with *why*, *where* and *fix*. Critical issues block
//! export unless they are overridable and the user gave a reason (stored in the audit log).

use std::collections::{HashMap, HashSet};

use oto_core::crop;
use oto_core::geometry::{Rect, Vec2};
use oto_core::lab::LabProfile;
use oto_core::media::MediaItem;
use oto_core::model::{FitMode, ItemId, MediaId, PreflightOverride, Project, Spread, SpreadId};
use oto_core::spec::CoverGeometry;
use oto_media::FileStatus;
use oto_render::raster::Surface;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "Info",
            Severity::Warning => "Warning",
            Severity::Critical => "Critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub severity: Severity,
    pub spread_id: Option<SpreadId>,
    /// 1-based spread number for humans (0 = cover, None = whole book).
    pub spread_no: Option<usize>,
    pub item_id: Option<ItemId>,
    pub message: String,
    pub why: String,
    pub fix: String,
    pub override_allowed: bool,
    /// Reason given when overridden.
    pub overridden: Option<String>,
}

impl Issue {
    pub fn blocks_export(&self) -> bool {
        self.severity == Severity::Critical && self.overridden.is_none()
    }
    pub fn needs_ack(&self) -> bool {
        self.severity == Severity::Warning && self.overridden.is_none()
    }
    pub fn key(&self) -> (String, Option<SpreadId>, Option<ItemId>) {
        (self.code.clone(), self.spread_id, self.item_id)
    }
    pub fn where_label(&self) -> String {
        match self.spread_no {
            None => "Book".into(),
            Some(0) => "Cover".into(),
            Some(n) => format!("Spread {n}"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn count(&self, s: Severity) -> usize {
        self.issues.iter().filter(|i| i.severity == s && i.overridden.is_none()).count()
    }
    pub fn blocking(&self) -> Vec<&Issue> {
        self.issues.iter().filter(|i| i.blocks_export()).collect()
    }
    pub fn can_export(&self) -> bool {
        self.blocking().is_empty()
    }
    pub fn for_spread(&self, id: SpreadId) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(move |i| i.spread_id == Some(id))
    }
}

pub struct Input<'a> {
    pub project: &'a Project,
    pub media: &'a HashMap<MediaId, MediaItem>,
    pub lab: Option<&'a LabProfile>,
    /// Status of originals (computed by the caller; stat() is cheap but not free).
    pub status: &'a HashMap<MediaId, FileStatus>,
    pub cover_geometry: Option<CoverGeometry>,
}

pub const DPI_WARN: f32 = 250.0;
pub const DPI_CRITICAL: f32 = 200.0;
pub const DPI_BLOCK: f32 = 150.0;

struct Ctx<'a> {
    out: Vec<Issue>,
    overrides: &'a [PreflightOverride],
}

impl Ctx<'_> {
    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, code: &str, sev: Severity, spread: Option<(&Spread, usize)>, item: Option<ItemId>, message: String, why: &str, fix: &str, overridable: bool) {
        let spread_id = spread.map(|s| s.0.id);
        let overridden = if overridable { self.overrides.iter().find(|o| o.code == code && o.spread_id == spread_id && o.item_id == item).map(|o| o.reason.clone()) } else { None };
        self.out.push(Issue {
            code: code.into(),
            severity: sev,
            spread_id,
            spread_no: spread.map(|s| s.1),
            item_id: item,
            message,
            why: why.into(),
            fix: fix.into(),
            override_allowed: overridable,
            overridden,
        });
    }
}

/// Run all checks.
pub fn run(inp: &Input) -> Report {
    let p = inp.project;
    let mut cx = Ctx { out: Vec::new(), overrides: &p.settings.overrides };
    let geom = p.spec.geometry();
    let surf = Surface { w: geom.trim_w, h: geom.trim_h, bleed: geom.bleed };

    // Book-level checks.
    let pages = p.page_count();
    if !p.spec.page_count_ok(pages) {
        cx.push(
            "PAGE_COUNT",
            Severity::Critical,
            None,
            None,
            format!("{pages} pages ({} spreads), but this book takes {}–{} pages in steps of {}.", p.spreads.len(), p.spec.min_pages, p.spec.max_pages, p.spec.page_increment),
            "Labs reject or pad books with an unsupported page count, which changes your layout or price.",
            "Add or remove spreads until the page count is allowed.",
            true,
        );
    }
    if let (Some(lab), Some(size_id)) = (inp.lab, p.lab_size_id.as_deref()) {
        if let Some(size) = lab.size(size_id) {
            let s = lab.spec_for_size(size);
            if s.page_width_um != p.spec.page_width_um || s.page_height_um != p.spec.page_height_um || s.bleed_um != p.spec.bleed_um {
                cx.push(
                    "SPEC_MISMATCH",
                    Severity::Warning,
                    None,
                    None,
                    format!("Album size differs from lab profile '{}' size {}.", lab.name, size.id),
                    "The lab expects exact dimensions; a mismatch is usually rejected or scaled.",
                    "Open Album settings and re-apply the lab size, or confirm the custom size with your lab.",
                    true,
                );
            }
        }
        if lab.is_example() {
            cx.push(
                "LAB_EXAMPLE",
                Severity::Info,
                None,
                None,
                format!("Lab profile '{}' is an example (source: {}, verified {}).", lab.name, lab.source, lab.verified_at),
                "Example profiles are not confirmed by a real lab.",
                "Ask your lab for their spec sheet and create a verified profile before ordering.",
                false,
            );
        }
    }
    if let Some(icc) = &p.spec.output_profile.icc_path {
        if std::fs::metadata(icc).is_err() {
            cx.push(
                "ICC_MISSING",
                Severity::Critical,
                None,
                None,
                format!("Output ICC profile not found: {icc}"),
                "The PDF must embed the output profile the lab prints with.",
                "Choose the ICC file again in Album settings or switch back to the built-in sRGB profile.",
                false,
            );
        }
    }

    // Missing / changed originals (once per media item, reported on first use).
    let mut reported_media = HashSet::new();
    let spreads: Vec<(&Spread, usize, Surface, bool)> = p
        .spreads
        .iter()
        .enumerate()
        .map(|(i, s)| (s, i + 1, surf, false))
        .chain(p.cover.as_ref().and_then(|c| inp.cover_geometry.map(|g| (&c.content, 0usize, Surface { w: g.trim_w(), h: g.trim_h(), bleed: g.bleed }, true))))
        .collect();

    let band = geom.gutter_band();
    let band_x0 = band.x as f32 / geom.trim_w as f32;
    let band_x1 = band.right() as f32 / geom.trim_w as f32;
    let [safe_l, safe_r] = geom.safe_pages();
    let safe_norm: Vec<Rect> = [safe_l, safe_r].iter().map(|r| r.to_norm(geom.trim_w, geom.trim_h)).collect();
    let tol = geom.trim_outer.max(1_000) as f64;

    for (s, no, sf, is_cover) in spreads {
        let at = Some((s, no));
        if !is_cover && s.is_empty() && s.texts.is_empty() {
            cx.push("BLANK_SPREAD", Severity::Warning, at, None, "Spread is blank.".into(), "A blank spread prints as empty paper.", "Add photos or text, or delete the spread.", true);
        }
        let mut seen_media: HashMap<MediaId, ItemId> = HashMap::new();
        let mut seen_groups: HashMap<u32, ItemId> = HashMap::new();
        for f in &s.frames {
            let item = Some(f.id);
            let Some(mid) = f.image_id else {
                cx.push("EMPTY_FRAME", Severity::Warning, at, item, "A photo frame is empty.".into(), "Empty frames print as blank boxes.", "Drop a photo into the frame or delete it.", true);
                continue;
            };
            let Some(m) = inp.media.get(&mid) else {
                cx.push(
                    "MISSING_IMAGE",
                    Severity::Critical,
                    at,
                    item,
                    "A placed photo is not in the library.".into(),
                    "The original cannot be found, so it cannot be printed.",
                    "Replace the photo.",
                    false,
                );
                continue;
            };
            match inp.status.get(&mid).copied().unwrap_or(FileStatus::Ok) {
                FileStatus::Missing => {
                    if reported_media.insert(mid) {
                        cx.push(
                            "MISSING_IMAGE",
                            Severity::Critical,
                            at,
                            item,
                            format!("Original missing: {}", m.file_name),
                            "Export needs the full-resolution original; the preview is not enough for print.",
                            "Use Relink to point oto-albums at the folder where the file now lives.",
                            false,
                        );
                    }
                }
                FileStatus::Changed => {
                    if reported_media.insert(mid) {
                        cx.push(
                            "IMAGE_CHANGED",
                            Severity::Warning,
                            at,
                            item,
                            format!("{} changed since import.", m.file_name),
                            "The file was edited after import; crops and previews may not match the print.",
                            "Re-import the file to refresh previews, then check the crop.",
                            true,
                        );
                    }
                }
                FileStatus::Ok => {}
            }
            if let Some(prev) = seen_media.insert(mid, f.id) {
                let _ = prev;
                cx.push(
                    "DUPLICATE_ON_SPREAD",
                    Severity::Warning,
                    at,
                    item,
                    format!("{} appears twice on this spread.", m.file_name),
                    "Repeating a photo on one spread looks like a mistake.",
                    "Replace one of the copies.",
                    true,
                );
            } else if let Some(g) = m.similar_group {
                if seen_groups.insert(g, f.id).is_some() {
                    cx.push(
                        "SIMILAR_ON_SPREAD",
                        Severity::Info,
                        at,
                        item,
                        format!("{} is nearly identical to another photo on this spread.", m.file_name),
                        "Near-duplicates side by side weaken the story.",
                        "Swap one of them for a different moment.",
                        false,
                    );
                }
            }
            // Effective DPI (on the printed rect, including bleed extension).
            let pr = sf.print_rect(&f.rect_norm);
            let ai = m.aspect();
            let af = (pr.w() / pr.h().max(1.0)) as f32;
            let c = if f.fit == FitMode::Fill { crop::clamp(&f.crop, ai, af) } else { f.crop };
            let vis = crop::visible(&c, ai, af, f.fit);
            let dpi = crop::effective_dpi(m.width, m.height, &vis, pr.w() as i64, pr.h() as i64);
            if dpi < DPI_BLOCK {
                cx.push(
                    "DPI_BLOCK",
                    Severity::Critical,
                    at,
                    item,
                    format!("{} prints at {dpi:.0} dpi.", m.file_name),
                    "Below 150 dpi the print will look visibly soft and pixelated.",
                    "Make the frame smaller, zoom the crop out, or use a larger original.",
                    false,
                );
            } else if dpi < DPI_CRITICAL {
                cx.push(
                    "DPI_LOW",
                    Severity::Critical,
                    at,
                    item,
                    format!("{} prints at {dpi:.0} dpi.", m.file_name),
                    "Between 150 and 200 dpi softness is noticeable at normal viewing distance.",
                    "Make the frame smaller, zoom the crop out, or use a larger original. Override only if the client accepts it.",
                    true,
                );
            } else if dpi < DPI_WARN {
                cx.push(
                    "DPI_WARN",
                    Severity::Warning,
                    at,
                    item,
                    format!("{} prints at {dpi:.0} dpi.", m.file_name),
                    "Below 250 dpi fine detail may soften slightly.",
                    "Fine for most photos; reduce the frame if the photo has fine detail.",
                    true,
                );
            }
            // Colour profile of the original.
            match m.color_profile.as_deref() {
                None => cx.push(
                    "PROFILE_ASSUMED",
                    Severity::Info,
                    at,
                    item,
                    format!("{} has no embedded colour profile; treated as sRGB.", m.file_name),
                    "Without a profile colours are interpreted as sRGB.",
                    "Export from your editor with sRGB embedded if colours look off.",
                    false,
                ),
                Some(name) if !name.contains("sRGB") => cx.push(
                    "PROFILE_UNSUPPORTED",
                    Severity::Warning,
                    at,
                    item,
                    format!("{} is tagged {name}; oto-albums v1 does not convert colour spaces.", m.file_name),
                    "Pixels are sent as sRGB, so wide-gamut images may print duller or shifted.",
                    "Convert the file to sRGB in your editor, then relink or re-import it.",
                    true,
                ),
                _ => {}
            }
            // Crop/orientation sanity: losing more than ~45 % of the image.
            let kept = (vis.w.min(1.0) * vis.h.min(1.0)).clamp(0.0, 1.0);
            if f.fit == FitMode::Fill && kept < 0.55 {
                cx.push(
                    "HEAVY_CROP",
                    Severity::Info,
                    at,
                    item,
                    format!("{} is cropped to {:.0}% of the frame-filling area.", m.file_name, kept * 100.0),
                    "The slot's orientation does not suit this photo.",
                    "Try another template or swap the photo into a slot with a closer shape.",
                    false,
                );
            }
            // Faces / focus in the fold.
            if !is_cover {
                let r = f.rect_norm;
                if r.x < band_x1 && r.right() > band_x0 {
                    let mut points: Vec<Vec2> = m.features.faces.iter().map(|b| b.center()).collect();
                    if let Some(fp) = f.focus.filter(|f| f.manual) {
                        points.push(Vec2::new(fp.x, fp.y));
                    } else if let Some(fp) = m.focus {
                        points.push(fp);
                    }
                    if points.is_empty() {
                        cx.push(
                            "FOLD_UNCHECKED",
                            Severity::Info,
                            at,
                            item,
                            format!("{} crosses the fold and has no face or focus point.", m.file_name),
                            "oto-albums cannot tell whether someone's face sits in the fold.",
                            "Set a focus point on the subject (Image tab → Focus) so the fold can be checked.",
                            false,
                        );
                    } else {
                        let tr = sf.rect(&r);
                        let hit = points.iter().any(|pt| {
                            let fp = vis.image_to_frame(*pt);
                            let x_um = tr.x0 + fp.x as f64 * tr.w();
                            let xn = (x_um / geom.trim_w as f64) as f32;
                            xn > band_x0 && xn < band_x1 && (0.0..=1.0).contains(&fp.y)
                        });
                        if hit {
                            cx.push(
                                "FACE_IN_GUTTER",
                                Severity::Warning,
                                at,
                                item,
                                format!("The subject of {} sits in the fold.", m.file_name),
                                "Faces in the binding are pulled into the gutter and lost.",
                                "Pan the crop so the subject sits clear of the centre, or use a template that does not cross the fold.",
                                true,
                            );
                        }
                    }
                }
            }
            // Thin white sliver near the trim edge.
            let u = sf.rect(&f.rect_norm);
            let gaps = [u.x0, u.y0, sf.w as f64 - u.x1, sf.h as f64 - u.y1];
            if gaps.iter().any(|&g| g > oto_render::raster::EDGE_EPS_UM && g < 2_500.0) {
                cx.push(
                    "EDGE_SLIVER",
                    Severity::Warning,
                    at,
                    item,
                    "A photo stops just short of the trim edge.".into(),
                    "Cutting tolerance will leave an uneven white sliver along that edge.",
                    "Snap the frame to the edge (it then bleeds automatically) or pull it in at least 3 mm.",
                    true,
                );
            } else if !is_cover && gaps.iter().any(|&g| g > 2_500.0 && g < tol) {
                cx.push(
                    "IMAGE_NEAR_TRIM",
                    Severity::Info,
                    at,
                    item,
                    "A photo edge sits inside the trim tolerance band.".into(),
                    "The cut may land slightly inside the photo edge.",
                    "Move the frame to the safe zone or let it bleed.",
                    false,
                );
            }
        }
        for t in &s.texts {
            let item = Some(t.id);
            if t.text.trim().is_empty() {
                continue;
            }
            if !oto_render::fonts::is_known_family(&t.style.font_family) {
                cx.push(
                    "FONT_MISSING",
                    Severity::Critical,
                    at,
                    item,
                    format!("Font '{}' is not available.", t.style.font_family),
                    "Only bundled, licensed fonts can be embedded in the print file.",
                    "Pick one of the bundled fonts in the Text tab.",
                    false,
                );
            }
            let r = sf.rect(&t.rect_norm);
            let pt = 72.0 / 25_400.0;
            let lay = oto_render::text::layout(&t.text, &t.style, (r.w() * pt) as f32, (r.h() * pt) as f32);
            if lay.overflow {
                cx.push(
                    "TEXT_OVERFLOW",
                    Severity::Warning,
                    at,
                    item,
                    "Text does not fit its box.".into(),
                    "Overflowing text is cut off or collides with other items.",
                    "Enlarge the text box or reduce the font size.",
                    true,
                );
            }
            if r.x0 < 0.0 || r.y0 < 0.0 || r.x1 > sf.w as f64 || r.y1 > sf.h as f64 {
                cx.push("TEXT_OUTSIDE_TRIM", Severity::Critical, at, item, "Text runs past the trim edge.".into(), "Anything past the trim is cut off.", "Move the text inside the page.", true);
            } else if is_cover {
                if let Some(g) = inp.cover_geometry {
                    let tr = t.rect_norm;
                    let rects: Vec<Rect> = g.safe_rects().iter().chain(std::iter::once(&g.spine())).map(|r| r.to_norm(g.trim_w(), g.trim_h())).collect();
                    if !rects.iter().any(|sr| inside(&tr, sr)) {
                        cx.push(
                            "TEXT_UNSAFE",
                            Severity::Warning,
                            at,
                            item,
                            "Cover text is outside the safe area.".into(),
                            "Cover wrap and hinge areas fold around the board; text there bends or disappears.",
                            "Keep cover text on the front, back or spine safe area.",
                            true,
                        );
                    }
                }
            } else if !safe_norm.iter().any(|sr| inside(&t.rect_norm, sr)) {
                cx.push(
                    "TEXT_UNSAFE",
                    Severity::Warning,
                    at,
                    item,
                    "Text is outside the safe zone (or crosses the fold).".into(),
                    "Text near the trim or in the fold can be cut or pulled into the binding.",
                    "Move the text so it sits fully inside one page's safe zone.",
                    true,
                );
            }
        }
    }
    Report { issues: cx.out }
}

fn inside(a: &Rect, b: &Rect) -> bool {
    const E: f32 = 1e-4;
    a.x >= b.x - E && a.y >= b.y - E && a.right() <= b.right() + E && a.bottom() <= b.bottom() + E
}

#[cfg(test)]
mod tests {
    use super::*;
    use oto_core::media::{ColorLabel, Features};
    use oto_core::model::{new_id, now_iso, Frame, TextBlock, TextStyle};
    use oto_core::spec::AlbumSpec;

    fn item(w: u32, h: u32) -> MediaItem {
        MediaItem {
            id: new_id(),
            path: "/x.jpg".into(),
            file_name: "x.jpg".into(),
            hash: "h".into(),
            size: 1,
            mtime: 0,
            width: w,
            height: h,
            orientation: 1,
            format: "jpeg".into(),
            capture_time: None,
            camera: None,
            lens: None,
            iso: None,
            aperture: None,
            shutter: None,
            focal_mm: None,
            color_profile: Some("sRGB".into()),
            rating: 0,
            color_label: ColorLabel::None,
            rejected: false,
            flagged: false,
            client_pick: false,
            bins: vec![],
            missing: false,
            features: Features::default(),
            focus: None,
            similar_group: None,
            imported_at: now_iso(),
        }
    }

    #[test]
    fn detects_low_dpi_missing_and_overrides() {
        let small = item(600, 400);
        let big = item(8000, 6000);
        let mut p = Project::new("T", AlbumSpec::default(), 10);
        let mut f = Frame::new(Rect::new(0.55, 0.1, 0.4, 0.8));
        f.image_id = Some(small.id);
        p.spreads[0].frames.push(f.clone());
        let mut g = Frame::new(Rect::new(0.05, 0.1, 0.4, 0.8));
        g.image_id = Some(big.id);
        p.spreads[0].frames.push(g.clone());
        p.spreads[1].texts.push(TextBlock::new(Rect::new(0.45, 0.4, 0.1, 0.1), "Fold", TextStyle::default()));
        let media: HashMap<_, _> = [(small.id, small.clone()), (big.id, big.clone())].into();
        let mut status = HashMap::new();
        status.insert(big.id, FileStatus::Missing);
        let r = run(&Input { project: &p, media: &media, lab: None, status: &status, cover_geometry: None });
        let codes: Vec<&str> = r.issues.iter().map(|i| i.code.as_str()).collect();
        assert!(codes.contains(&"DPI_BLOCK"), "{codes:?}");
        assert!(codes.contains(&"MISSING_IMAGE"));
        assert!(codes.contains(&"TEXT_UNSAFE"));
        assert!(codes.contains(&"BLANK_SPREAD"));
        assert!(!r.can_export());
        for i in &r.issues {
            assert!(!i.why.is_empty() && !i.fix.is_empty(), "{}", i.code);
        }
        // Override a warning: it no longer needs acknowledgement.
        let blank = r.issues.iter().find(|i| i.code == "BLANK_SPREAD").unwrap();
        p.settings.overrides.push(PreflightOverride { code: blank.code.clone(), spread_id: blank.spread_id, item_id: None, reason: "intentional pause".into(), at: now_iso() });
        let r2 = run(&Input { project: &p, media: &media, lab: None, status: &status, cover_geometry: None });
        assert!(r2.issues.iter().any(|i| i.code == "BLANK_SPREAD" && i.overridden.as_deref() == Some("intentional pause")));
    }

    #[test]
    fn edge_sliver_detected() {
        let big = item(8000, 6000);
        let mut p = Project::new("T", AlbumSpec::default(), 10);
        let mut f = Frame::new(Rect::new(0.003, 0.1, 0.4, 0.8)); // ~1.5 mm from the left trim
        f.image_id = Some(big.id);
        p.spreads[0].frames.push(f);
        let media: HashMap<_, _> = [(big.id, big)].into();
        let r = run(&Input { project: &p, media: &media, lab: None, status: &HashMap::new(), cover_geometry: None });
        assert!(r.issues.iter().any(|i| i.code == "EDGE_SLIVER"));
    }
}
