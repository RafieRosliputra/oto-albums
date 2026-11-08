//! Compose a spread/cover surface into PDF content (vector text, shapes, patterns; images placed
//! with clip + matrix, never resampled unless needed).

use oto_core::crop;
use oto_core::model::{Adjust, FitMode, Frame, MediaId, Rgba, Shape, ShapeKind, Spread, TextBlock};
use oto_core::units::{mdeg_to_rad, Um};
use oto_render::pattern::{self, Prim};
use oto_render::raster::{Surface, UmRect};
use pdf_writer::types::LineCapStyle;
use pdf_writer::{Content, Name};

use crate::pdf::{ellipse_path, pt, rrect_path, ImageData, PageBoxes, PageRes, PdfBuilder};

/// Supplies PDF image data for a frame: (cache key, loader).
pub trait PdfImages {
    /// `need_px` = pixel size the frame needs at the chosen downsample resolution (if any).
    fn key(&self, id: MediaId, adjust: &Adjust, need_px: Option<(u32, u32)>) -> Option<String>;
    fn load(&self, id: MediaId, adjust: &Adjust, need_px: Option<(u32, u32)>) -> Option<ImageData>;
    /// Oriented pixel size of the image (for crop maths).
    fn size(&self, id: MediaId) -> Option<(u32, u32)>;
}

/// Which part of a surface becomes one PDF page, and how much bleed / crop-mark room it gets.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub surf: Surface,
    /// Extra area outside the bleed for crop marks (µm).
    pub marks: Um,
    /// Visible trim range in surface coordinates (µm).
    pub vx0: f64,
    pub vx1: f64,
    pub bleed_l: f64,
    pub bleed_r: f64,
}

impl Layout {
    pub fn full(surf: Surface, marks: Um) -> Self {
        Layout { surf, marks, vx0: 0.0, vx1: surf.w as f64, bleed_l: surf.bleed as f64, bleed_r: surf.bleed as f64 }
    }
    /// Left page of a spread printed as single pages: bleed outside, `inside` hidden margin at the spine.
    pub fn left_page(surf: Surface, marks: Um, page_w: Um, inside: Um) -> Self {
        Layout { surf, marks, vx0: 0.0, vx1: page_w as f64, bleed_l: surf.bleed as f64, bleed_r: inside as f64 }
    }
    pub fn right_page(surf: Surface, marks: Um, page_w: Um, inside: Um) -> Self {
        Layout { surf, marks, vx0: page_w as f64, vx1: surf.w as f64, bleed_l: inside as f64, bleed_r: surf.bleed as f64 }
    }
    pub fn media_pt(&self) -> (f32, f32) {
        let m = self.marks as f64;
        (pt(self.vx1 - self.vx0 + self.bleed_l + self.bleed_r + 2.0 * m), pt(self.surf.h as f64 + 2.0 * (self.surf.bleed as f64 + m)))
    }
    /// Surface coordinates (µm, y down) → PDF points (y up).
    pub fn x(&self, um: f64) -> f32 {
        pt(self.marks as f64 + self.bleed_l + um - self.vx0)
    }
    pub fn y(&self, um: f64) -> f32 {
        self.media_pt().1 - pt(self.marks as f64 + self.surf.bleed as f64 + um)
    }
    pub fn bleed_rect(&self) -> [f32; 4] {
        let b = self.surf.bleed as f64;
        [self.x(self.vx0 - self.bleed_l), self.y(self.surf.h as f64 + b), self.x(self.vx1 + self.bleed_r), self.y(-b)]
    }
    pub fn boxes(&self, label: Option<String>) -> PageBoxes {
        let (w, h) = self.media_pt();
        PageBoxes { media: (w, h), trim: [self.x(self.vx0), self.y(self.surf.h as f64), self.x(self.vx1), self.y(0.0)], bleed: self.bleed_rect(), label }
    }
}

fn rgb(c: Rgba) -> [f32; 3] {
    c.f32()
}

pub struct ComposeOpts {
    pub downsample_dpi: Option<u32>,
    pub crop_marks: bool,
}

/// Write one surface into `c`. Returns warnings (e.g. images that failed to load).
pub fn compose(b: &mut PdfBuilder, c: &mut Content, res: &mut PageRes, lay: &Layout, s: &Spread, imgs: &dyn PdfImages, o: &ComposeOpts) -> Vec<String> {
    let mut warnings = Vec::new();
    let surf = lay.surf;
    // Clip everything to the bleed box.
    c.save_state();
    let [bx0, by0, bx1, by1] = lay.bleed_rect();
    let (bw, bh) = (bx1 - bx0, by1 - by0);
    c.rect(bx0, by0, bw, bh);
    c.clip_nonzero();
    c.end_path();
    let bg = rgb(s.background.color);
    c.set_fill_rgb(bg[0], bg[1], bg[2]);
    c.rect(bx0, by0, bw, bh);
    c.fill_nonzero();
    if let Some(bi) = &s.background.image {
        let mut f = Frame::new(oto_core::geometry::Rect::UNIT);
        f.image_id = Some(bi.media_id);
        f.crop = bi.crop;
        f.opacity = bi.opacity;
        let r = surf.print_rect(&f.rect_norm);
        frame(b, c, res, lay, &f, r, imgs, o, &mut warnings);
    }
    if let Some(p) = &s.background.pattern {
        draw_pattern(b, c, res, lay, p);
    }
    enum It<'a> {
        F(&'a Frame),
        S(&'a Shape),
        T(&'a TextBlock),
    }
    let mut items: Vec<(i32, u8, It)> = Vec::new();
    items.extend(s.shapes.iter().map(|x| (x.z, 0, It::S(x))));
    items.extend(s.frames.iter().map(|x| (x.z, 1, It::F(x))));
    items.extend(s.texts.iter().map(|x| (x.z, 2, It::T(x))));
    items.sort_by_key(|(z, k, _)| (*z, *k));
    for (_, _, it) in items {
        match it {
            It::F(f) => {
                let r = surf.print_rect(&f.rect_norm);
                frame(b, c, res, lay, f, r, imgs, o, &mut warnings);
            }
            It::S(sh) => shape(b, c, res, lay, sh),
            It::T(t) => text(b, c, res, lay, t),
        }
    }
    c.restore_state();
    if o.crop_marks && lay.marks > 0 {
        crop_marks(c, lay);
    }
    warnings
}

fn rot_cm(c: &mut Content, cx: f32, cy: f32, rot_mdeg: i32) {
    let a = -mdeg_to_rad(rot_mdeg); // clockwise on paper
    let (s, co) = a.sin_cos();
    c.transform([co, s, -s, co, cx, cy]);
}

#[allow(clippy::too_many_arguments)]
fn frame(b: &mut PdfBuilder, c: &mut Content, res: &mut PageRes, lay: &Layout, f: &Frame, r: UmRect, imgs: &dyn PdfImages, o: &ComposeOpts, warnings: &mut Vec<String>) {
    let (cx, cy) = (lay.x(r.cx()), lay.y(r.cy()));
    let (w, h) = (pt(r.w()), pt(r.h()));
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    c.save_state();
    rot_cm(c, cx, cy, f.rotation_mdeg);
    let radius = pt(f.radius_mm as f64 * 1000.0);
    rrect_path(c, -w / 2.0, -h / 2.0, w, h, radius);
    c.clip_nonzero();
    c.end_path();
    if f.opacity < 0.999 {
        let g = b.gstate(res, f.opacity);
        c.set_parameters(Name(g.as_bytes()));
    }
    let mut drawn = false;
    if let Some(id) = f.image_id {
        if let Some((iw, ih)) = imgs.size(id) {
            let ai = iw as f32 / ih.max(1) as f32;
            let af = w / h;
            let cr = if f.fit == FitMode::Fill { crop::clamp(&f.crop, ai, af) } else { f.crop };
            let vis = crop::visible(&cr, ai, af, f.fit);
            let need = o.downsample_dpi.map(|dpi| {
                let wi = r.w() / 25_400.0 * dpi as f64 / vis.w as f64;
                let hi = r.h() / 25_400.0 * dpi as f64 / vis.h as f64;
                ((wi.ceil() as u32).max(1), (hi.ceil() as u32).max(1))
            });
            let need = need.filter(|(nw, nh)| *nw < iw || *nh < ih);
            if let Some(key) = imgs.key(id, &f.adjust, need) {
                if let Some(name) = b.image(res, &key, || imgs.load(id, &f.adjust, need)) {
                    let img_w = w / vis.w;
                    let img_h = h / vis.h;
                    c.save_state();
                    rot_cm(c, 0.0, 0.0, cr.rotation_mdeg);
                    c.transform([img_w, 0.0, 0.0, img_h, -img_w * vis.center.x, -img_h * (1.0 - vis.center.y)]);
                    c.x_object(Name(name.as_bytes()));
                    c.restore_state();
                    drawn = true;
                }
            }
        }
        if !drawn {
            warnings.push(format!("Image {id} could not be loaded; a grey placeholder was printed."));
            c.set_fill_rgb(0.78, 0.77, 0.74);
            c.rect(-w / 2.0, -h / 2.0, w, h);
            c.fill_nonzero();
        }
    }
    if f.border.width_mm > 0.0 {
        let bw = pt(f.border.width_mm as f64 * 1000.0);
        let col = rgb(f.border.color);
        c.set_stroke_rgb(col[0], col[1], col[2]);
        c.set_line_width(bw);
        rrect_path(c, -w / 2.0 + bw / 2.0, -h / 2.0 + bw / 2.0, w - bw, h - bw, (radius - bw / 2.0).max(0.0));
        c.stroke();
    }
    c.restore_state();
}

fn shape(b: &mut PdfBuilder, c: &mut Content, res: &mut PageRes, lay: &Layout, s: &Shape) {
    let r = lay.surf.rect(&s.rect_norm);
    let (cx, cy) = (lay.x(r.cx()), lay.y(r.cy()));
    let (w, h) = (pt(r.w()), pt(r.h()));
    c.save_state();
    rot_cm(c, cx, cy, s.rotation_mdeg);
    if s.opacity < 0.999 {
        let g = b.gstate(res, s.opacity);
        c.set_parameters(Name(g.as_bytes()));
    }
    let path = |c: &mut Content| match s.kind {
        ShapeKind::Rect => {
            c.rect(-w / 2.0, -h / 2.0, w, h);
        }
        ShapeKind::Ellipse => ellipse_path(c, 0.0, 0.0, w / 2.0, h / 2.0),
        ShapeKind::Line => {
            c.move_to(-w / 2.0, 0.0);
            c.line_to(w / 2.0, 0.0);
        }
    };
    if let (Some(fc), true) = (s.fill, s.kind != ShapeKind::Line) {
        let f = rgb(fc);
        c.set_fill_rgb(f[0], f[1], f[2]);
        path(c);
        c.fill_nonzero();
    }
    if let Some(sc) = s.stroke {
        let k = rgb(sc);
        c.set_stroke_rgb(k[0], k[1], k[2]);
        c.set_line_width(pt(s.stroke_mm as f64 * 1000.0).max(0.25));
        path(c);
        c.stroke();
    }
    c.restore_state();
}

fn text(b: &mut PdfBuilder, c: &mut Content, res: &mut PageRes, lay: &Layout, t: &TextBlock) {
    if t.text.trim().is_empty() {
        return;
    }
    let r = lay.surf.rect(&t.rect_norm);
    let rot = mdeg_to_rad(t.rotation_mdeg) as f64;
    let sideways = rot.sin().abs() > std::f64::consts::FRAC_1_SQRT_2;
    let (bw, bh) = if sideways { (pt(r.h()), pt(r.w())) } else { (pt(r.w()), pt(r.h())) };
    let l = oto_render::text::layout(&t.text, &t.style, bw, bh);
    c.save_state();
    rot_cm(c, lay.x(r.cx()), lay.y(r.cy()), t.rotation_mdeg);
    if t.style.color.a < 255 {
        let g = b.gstate(res, t.style.color.a as f32 / 255.0);
        c.set_parameters(Name(g.as_bytes()));
    }
    b.text(c, res, &l, -bw / 2.0, bh / 2.0, rgb(t.style.color));
    c.restore_state();
}

fn draw_pattern(b: &mut PdfBuilder, c: &mut Content, res: &mut PageRes, lay: &Layout, p: &oto_core::model::Pattern) {
    let (s, prims) = pattern::tile(p);
    let th = mdeg_to_rad(p.angle_mdeg);
    let (sn, cs) = th.sin_cos();
    let k = 72.0 / 25.4f32; // pt per mm
    c.save_state();
    if p.opacity < 0.999 {
        let g = b.gstate(res, p.opacity);
        c.set_parameters(Name(g.as_bytes()));
    }
    // Pattern space (mm, y down, rotated) → page.
    c.transform([k * cs, -k * sn, -k * sn, -k * cs, lay.x(0.0), lay.y(0.0)]);
    let col = rgb(p.color);
    c.set_fill_rgb(col[0], col[1], col[2]);
    c.set_stroke_rgb(col[0], col[1], col[2]);
    c.set_line_cap(LineCapStyle::RoundCap);
    // Tiles covering the bleed box in pattern space.
    let bl = lay.surf.bleed as f32 / 1000.0;
    let (w, h) = (lay.surf.w as f32 / 1000.0, lay.surf.h as f32 / 1000.0);
    let corners = [(-bl, -bl), (w + bl, -bl), (-bl, h + bl), (w + bl, h + bl)];
    let uv: Vec<(f32, f32)> = corners.iter().map(|&(x, y)| (x * cs + y * sn, -x * sn + y * cs)).collect();
    let (umin, umax) = uv.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (vmin, vmax) = uv.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let (i0, i1) = ((umin / s).floor() as i32 - 1, (umax / s).ceil() as i32 + 1);
    let (j0, j1) = ((vmin / s).floor() as i32 - 1, (vmax / s).ceil() as i32 + 1);
    if ((i1 - i0) as i64) * ((j1 - j0) as i64) > 400_000 {
        c.restore_state();
        return;
    }
    let mut any_fill = false;
    for j in j0..j1 {
        for i in i0..i1 {
            let (ox, oy) = (i as f32 * s, j as f32 * s);
            for pr in &prims {
                match pr {
                    Prim::Circle { cx, cy, r } => {
                        ellipse_path(c, ox + cx, oy + cy, *r, *r);
                        any_fill = true;
                    }
                    Prim::Rect { x, y, w, h } => {
                        c.rect(ox + x, oy + y, *w, *h);
                        any_fill = true;
                    }
                    Prim::Poly { .. } => {}
                }
            }
        }
    }
    if any_fill {
        c.fill_nonzero();
    }
    for pr in &prims {
        if let Prim::Poly { pts, width } = pr {
            c.set_line_width(*width);
            for j in j0..j1 {
                for i in i0..i1 {
                    let (ox, oy) = (i as f32 * s, j as f32 * s);
                    for (n, (x, y)) in pts.iter().enumerate() {
                        if n == 0 {
                            c.move_to(ox + x, oy + y);
                        } else {
                            c.line_to(ox + x, oy + y);
                        }
                    }
                }
            }
            c.stroke();
        }
    }
    c.restore_state();
}

fn crop_marks(c: &mut Content, lay: &Layout) {
    let s = lay.surf;
    let off = pt(s.bleed as f64 + 1_000.0);
    let len = pt(lay.marks as f64 - 1_000.0).max(2.0);
    c.save_state();
    c.set_stroke_rgb(0.0, 0.0, 0.0);
    c.set_line_width(0.25);
    for (xu, yu, sx, sy) in [(lay.vx0, 0.0, -1.0, 1.0), (lay.vx1, 0.0, 1.0, 1.0), (lay.vx0, s.h as f64, -1.0, -1.0), (lay.vx1, s.h as f64, 1.0, -1.0)] {
        let (x, y) = (lay.x(xu), lay.y(yu));
        // Horizontal mark beside the corner, vertical mark above/below it.
        c.move_to(x + sx * off, y);
        c.line_to(x + sx * (off + len), y);
        c.move_to(x, y + sy * off);
        c.line_to(x, y + sy * (off + len));
    }
    c.stroke();
    c.restore_state();
}
