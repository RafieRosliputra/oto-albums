//! CPU raster renderer for spreads and covers (JPEG/TIFF export, proofs, golden tests).

use std::sync::Arc;

use ab_glyph::{Font, FontRef, GlyphId, PxScale};
use image::{Rgb, RgbImage, RgbaImage};
use oto_core::crop;
use oto_core::model::{Adjust, FitMode, Frame, MediaId, Rgba, Shape, ShapeKind, Spread, TextBlock};
use oto_core::units::{mdeg_to_rad, Um};
use rayon::prelude::*;

use crate::fonts;
use crate::pattern;
use crate::text;

/// Supplies decoded, upright images (RGBA) by media id.
pub trait ImageSource: Sync {
    fn get(&self, id: MediaId) -> Option<Arc<RgbaImage>>;
}

impl<F: Fn(MediaId) -> Option<Arc<RgbaImage>> + Sync> ImageSource for F {
    fn get(&self, id: MediaId) -> Option<Arc<RgbaImage>> {
        self(id)
    }
}

/// Printable surface: trimmed size and bleed (micrometres).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    pub w: Um,
    pub h: Um,
    pub bleed: Um,
}

/// Rectangle in trim coordinates (µm, may extend into the bleed: negative / beyond w,h).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UmRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl UmRect {
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn cx(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn cy(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }
}

/// Edge tolerance for "touches the trim edge" (0.5 mm).
pub const EDGE_EPS_UM: f64 = 500.0;

impl Surface {
    pub fn rect(&self, r: &oto_core::geometry::Rect) -> UmRect {
        UmRect { x0: r.x as f64 * self.w as f64, y0: r.y as f64 * self.h as f64, x1: r.right() as f64 * self.w as f64, y1: r.bottom() as f64 * self.h as f64 }
    }
    /// Frame rect as printed: edges touching the trim line are extended through the bleed.
    pub fn print_rect(&self, r: &oto_core::geometry::Rect) -> UmRect {
        let mut u = self.rect(r);
        // Overshoot the media edge slightly so anti-aliasing never leaves a hairline at the paper edge.
        let b = self.bleed as f64 + 1000.0;
        if u.x0 <= EDGE_EPS_UM {
            u.x0 = -b;
        }
        if u.y0 <= EDGE_EPS_UM {
            u.y0 = -b;
        }
        if u.x1 >= self.w as f64 - EDGE_EPS_UM {
            u.x1 = self.w as f64 + b;
        }
        if u.y1 >= self.h as f64 - EDGE_EPS_UM {
            u.y1 = self.h as f64 + b;
        }
        u
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RenderOpts {
    pub dpi: f32,
    /// Render the bleed area too (export); otherwise only the trim box (proofs).
    pub include_bleed: bool,
}

/// Output pixel size for a surface.
pub fn output_size(s: &Surface, o: &RenderOpts) -> (u32, u32) {
    let ppu = o.dpi as f64 / 25_400.0;
    let (w, h) = if o.include_bleed { (s.w + 2 * s.bleed, s.h + 2 * s.bleed) } else { (s.w, s.h) };
    (((w as f64 * ppu).round() as u32).max(1), ((h as f64 * ppu).round() as u32).max(1))
}

/// Mapping between output pixels and trim micrometres.
#[derive(Debug, Clone, Copy)]
pub struct PxMap {
    pub ppu: f64,
    pub off: f64,
}

impl PxMap {
    pub fn new(s: &Surface, o: &RenderOpts) -> Self {
        PxMap { ppu: o.dpi as f64 / 25_400.0, off: if o.include_bleed { s.bleed as f64 } else { 0.0 } }
    }
    pub fn to_px(&self, um: f64) -> f64 {
        (um + self.off) * self.ppu
    }
    pub fn to_um(&self, px: f64) -> f64 {
        px / self.ppu - self.off
    }
}

#[inline]
fn blend(dst: &mut Rgb<u8>, c: [f32; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let a = a.min(1.0);
    for (d, s) in dst.0.iter_mut().zip(c) {
        *d = (*d as f32 * (1.0 - a) + s * a).round().clamp(0.0, 255.0) as u8;
    }
}

fn rgba_f(c: Rgba) -> [f32; 3] {
    [c.r as f32, c.g as f32, c.b as f32]
}

/// Bilinear sample in pixel coordinates; returns None outside the image.
fn sample(img: &RgbaImage, x: f64, y: f64) -> Option<[f32; 4]> {
    let (w, h) = (img.width() as f64, img.height() as f64);
    // Coordinates are in pixel-edge space: the image spans [0, w] × [0, h].
    if x < -0.01 || y < -0.01 || x > w + 0.01 || y > h + 0.01 {
        return None;
    }
    let x = (x - 0.5).clamp(0.0, w - 1.0);
    let y = (y - 0.5).clamp(0.0, h - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
    let p = |xx, yy| img.get_pixel(xx, yy).0.map(|v| v as f32);
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let mut o = [0.0; 4];
    for k in 0..4 {
        o[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
    }
    Some(o)
}

pub fn adjust_px(c: [f32; 3], a: &Adjust) -> [f32; 3] {
    if a.is_identity() {
        return c;
    }
    let mut c = c.map(|v| (v - 128.0) * (1.0 + a.contrast) + 128.0 + a.brightness * 100.0);
    if a.bw {
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        c = [l, l, l];
    }
    c.map(|v| v.clamp(0.0, 255.0))
}

/// Coverage of a rounded rectangle at local point (lx, ly) in px (rect centred at 0).
fn rrect_cov(lx: f64, ly: f64, hw: f64, hh: f64, r: f64) -> f32 {
    let r = r.min(hw).min(hh).max(0.0);
    let qx = lx.abs() - (hw - r);
    let qy = ly.abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
    (0.5 - outside).clamp(0.0, 1.0) as f32
}

struct FramePlan<'a> {
    rect: UmRect,
    rot: f64,
    img: Option<Arc<RgbaImage>>,
    frame: &'a Frame,
}

fn draw_frame(out: &mut RgbImage, map: &PxMap, p: &FramePlan) {
    let f = p.frame;
    let (cx, cy) = (map.to_px(p.rect.cx()), map.to_px(p.rect.cy()));
    let (hw, hh) = (p.rect.w() * map.ppu / 2.0, p.rect.h() * map.ppu / 2.0);
    if hw < 0.5 || hh < 0.5 {
        return;
    }
    let (sin, cos) = p.rot.sin_cos();
    let ext = (hw * cos.abs() + hh * sin.abs(), hw * sin.abs() + hh * cos.abs());
    let (w, h) = (out.width() as i64, out.height() as i64);
    let x0 = ((cx - ext.0).floor() as i64).clamp(0, w);
    let x1 = ((cx + ext.0).ceil() as i64 + 1).clamp(0, w);
    let y0 = ((cy - ext.1).floor() as i64).clamp(0, h);
    let y1 = ((cy + ext.1).ceil() as i64 + 1).clamp(0, h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let radius = f.radius_mm as f64 * 1000.0 * map.ppu;
    let border = f.border.width_mm as f64 * 1000.0 * map.ppu;
    let bcol = rgba_f(f.border.color);
    // Image mapping.
    let img_plan = p.img.as_ref().map(|img| {
        let (iw, ih) = (img.width() as f64, img.height() as f64);
        let ai = (iw / ih) as f32;
        let af = (p.rect.w() / p.rect.h()) as f32;
        let c = if f.fit == FitMode::Fill { crop::clamp(&f.crop, ai, af) } else { f.crop };
        let vis = crop::visible(&c, ai, af, f.fit);
        let (cs, cc) = (-mdeg_to_rad(c.rotation_mdeg) as f64).sin_cos();
        (img.clone(), vis, cs, cc, iw, ih)
    });
    let row_w = out.width() as usize * 3;
    let buf: &mut [u8] = out.as_mut();
    buf.par_chunks_mut(row_w).enumerate().filter(|(y, _)| (*y as i64) >= y0 && (*y as i64) < y1).for_each(|(y, row)| {
        for x in x0..x1 {
            let (px, py) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            // Inverse frame rotation.
            let lx = px * cos + py * sin;
            let ly = -px * sin + py * cos;
            let cov = rrect_cov(lx, ly, hw, hh, radius);
            if cov <= 0.0 {
                continue;
            }
            let mut color: Option<([f32; 3], f32)> = None;
            if let Some((img, vis, cs, cc, iw, ih)) = &img_plan {
                let u = lx / (2.0 * hw);
                let v = ly / (2.0 * hh);
                let dx = u * vis.w as f64 * iw;
                let dy = v * vis.h as f64 * ih;
                let ix = vis.center.x as f64 * iw + dx * cc - dy * cs;
                let iy = vis.center.y as f64 * ih + dx * cs + dy * cc;
                if let Some(s) = sample(img, ix, iy) {
                    color = Some((adjust_px([s[0], s[1], s[2]], &f.adjust), s[3] / 255.0));
                }
            } else if f.image_id.is_some() {
                // Missing image: neutral grey placeholder so the problem is visible, never white.
                color = Some(([200.0, 196.0, 188.0], 1.0));
            }
            let i = x as usize * 3;
            let mut px_ = Rgb([row[i], row[i + 1], row[i + 2]]);
            if let Some((c, a)) = color {
                blend(&mut px_, c, a * cov * f.opacity);
            }
            if border > 0.0 {
                let inner = rrect_cov(lx, ly, (hw - border).max(0.0), (hh - border).max(0.0), (radius - border).max(0.0));
                let bc = (cov - inner).max(0.0);
                blend(&mut px_, bcol, bc * f.opacity);
            }
            row[i..i + 3].copy_from_slice(&px_.0);
        }
    });
}

fn draw_shape(out: &mut RgbImage, map: &PxMap, surf: &Surface, s: &Shape) {
    let r = surf.rect(&s.rect_norm);
    let (cx, cy) = (map.to_px(r.cx()), map.to_px(r.cy()));
    let (hw, hh) = (r.w() * map.ppu / 2.0, r.h() * map.ppu / 2.0);
    let rot = mdeg_to_rad(s.rotation_mdeg) as f64;
    let (sin, cos) = rot.sin_cos();
    let stroke = (s.stroke_mm as f64 * 1000.0 * map.ppu).max(0.75);
    let ext = hw.max(hh) * 1.5 + stroke;
    let (w, h) = (out.width() as i64, out.height() as i64);
    let (x0, x1) = (((cx - ext) as i64).clamp(0, w), ((cx + ext) as i64 + 1).clamp(0, w));
    let (y0, y1) = (((cy - ext) as i64).clamp(0, h), ((cy + ext) as i64 + 1).clamp(0, h));
    let fill = s.fill.map(rgba_f);
    let stroke_c = s.stroke.map(rgba_f);
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let lx = px * cos + py * sin;
            let ly = -px * sin + py * cos;
            let (fill_cov, edge_d) = match s.kind {
                ShapeKind::Rect => {
                    let d = (lx.abs() - hw).max(ly.abs() - hh);
                    ((0.5 - d).clamp(0.0, 1.0), d.abs())
                }
                ShapeKind::Ellipse => {
                    let k = ((lx / hw.max(0.5)).powi(2) + (ly / hh.max(0.5)).powi(2)).sqrt();
                    let d = (k - 1.0) * hw.min(hh);
                    ((0.5 - d).clamp(0.0, 1.0), d.abs())
                }
                ShapeKind::Line => (0.0, (ly).abs().max(lx.abs() - hw).max(0.0).max(ly.abs())),
            };
            let p = out.get_pixel_mut(x as u32, y as u32);
            if let Some(c) = fill {
                if s.kind != ShapeKind::Line {
                    blend(p, c, fill_cov as f32 * s.opacity);
                }
            }
            if let Some(c) = stroke_c {
                let d = if s.kind == ShapeKind::Line {
                    if lx.abs() <= hw {
                        ly.abs()
                    } else {
                        f64::MAX
                    }
                } else {
                    edge_d
                };
                let cov = (stroke / 2.0 + 0.5 - d).clamp(0.0, 1.0);
                blend(p, c, cov as f32 * s.opacity);
            }
        }
    }
}

/// Draw a text block (laid out with the shared engine).
pub fn draw_text(out: &mut RgbImage, map: &PxMap, surf: &Surface, t: &TextBlock) {
    let r = surf.rect(&t.rect_norm);
    let rot = mdeg_to_rad(t.rotation_mdeg) as f64;
    let sideways = (rot.sin().abs()) > std::f64::consts::FRAC_1_SQRT_2;
    let (box_w_um, box_h_um) = if sideways { (r.h(), r.w()) } else { (r.w(), r.h()) };
    let um_to_pt = 72.0 / 25_400.0;
    let lay = text::layout(&t.text, &t.style, (box_w_um * um_to_pt) as f32, (box_h_um * um_to_pt) as f32);
    let face = lay.face();
    let Ok(font) = FontRef::try_from_slice(face.data) else {
        return;
    };
    let px_per_pt = map.ppu / um_to_pt;
    let (bw, bh) = ((box_w_um * map.ppu).ceil().max(1.0) as usize, (box_h_um * map.ppu).ceil().max(1.0) as usize);
    // Render a little beyond the box so descenders/overflow are not clipped silently.
    let pad = (lay.size as f64 * px_per_pt).ceil() as usize;
    let (cw, ch) = (bw + 2 * pad, bh + 2 * pad);
    if cw * ch > 80_000_000 {
        return;
    }
    let mut cov = vec![0f32; cw * ch];
    let scale: PxScale = font.pt_to_px_scale((lay.size as f64 * px_per_pt) as f32).unwrap_or(PxScale::from(lay.size));
    for line in &lay.lines {
        for g in &line.glyphs {
            let gx = pad as f32 + (g.x as f64 * px_per_pt) as f32;
            let gy = pad as f32 + ((line.baseline - g.y_offset) as f64 * px_per_pt) as f32;
            let glyph = GlyphId(g.gid).with_scale_and_position(scale, ab_glyph::point(gx, gy));
            if let Some(og) = font.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|x, y, c| {
                    let (xx, yy) = (b.min.x as i64 + x as i64, b.min.y as i64 + y as i64);
                    if xx >= 0 && yy >= 0 && (xx as usize) < cw && (yy as usize) < ch {
                        let i = yy as usize * cw + xx as usize;
                        cov[i] = (cov[i] + c).min(1.0);
                    }
                });
            }
        }
    }
    let color = rgba_f(t.style.color);
    let alpha = t.style.color.a as f32 / 255.0;
    let (cx, cy) = (map.to_px(r.cx()), map.to_px(r.cy()));
    let (sin, cos) = rot.sin_cos();
    let ext = ((cw * cw + ch * ch) as f64).sqrt() / 2.0 + 2.0;
    let (w, h) = (out.width() as i64, out.height() as i64);
    for y in ((cy - ext) as i64).max(0)..((cy + ext) as i64 + 1).min(h) {
        for x in ((cx - ext) as i64).max(0)..((cx + ext) as i64 + 1).min(w) {
            let (px, py) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let lx = px * cos + py * sin + cw as f64 / 2.0;
            let ly = -px * sin + py * cos + ch as f64 / 2.0;
            if lx < 0.0 || ly < 0.0 || lx >= cw as f64 || ly >= ch as f64 {
                continue;
            }
            let c = cov[ly as usize * cw + lx as usize];
            if c > 0.0 {
                blend(out.get_pixel_mut(x as u32, y as u32), color, c * alpha);
            }
        }
    }
}

fn draw_pattern(out: &mut RgbImage, map: &PxMap, p: &oto_core::model::Pattern) {
    let px_per_mm = (map.ppu * 1000.0) as f32;
    let (n, mask) = pattern::tile_mask(p, px_per_mm);
    let s = p.scale_mm.max(1.0) as f64;
    let (sin, cos) = (-(mdeg_to_rad(p.angle_mdeg) as f64)).sin_cos();
    let color = rgba_f(p.color);
    let op = p.opacity;
    let row_w = out.width() as usize * 3;
    let buf: &mut [u8] = out.as_mut();
    buf.par_chunks_mut(row_w).enumerate().for_each(|(y, row)| {
        for x in 0..row.len() / 3 {
            let mx = map.to_um(x as f64 + 0.5) / 1000.0;
            let my = map.to_um(y as f64 + 0.5) / 1000.0;
            let (u, v) = (mx * cos - my * sin, mx * sin + my * cos);
            let tu = (u.rem_euclid(s) / s * n as f64) as usize % n;
            let tv = (v.rem_euclid(s) / s * n as f64) as usize % n;
            let c = mask[tv * n + tu] as f32 / 255.0;
            if c > 0.0 {
                let i = x * 3;
                let mut p_ = Rgb([row[i], row[i + 1], row[i + 2]]);
                blend(&mut p_, color, c * op);
                row[i..i + 3].copy_from_slice(&p_.0);
            }
        }
    });
}

/// Render a spread (or cover) surface.
pub fn render(spread: &Spread, surf: &Surface, opts: &RenderOpts, src: &dyn ImageSource) -> RgbImage {
    let (w, h) = output_size(surf, opts);
    let bg = spread.background.color;
    let mut out = RgbImage::from_pixel(w, h, Rgb([bg.r, bg.g, bg.b]));
    let map = PxMap::new(surf, opts);
    if let Some(bi) = &spread.background.image {
        let mut f = Frame::new(oto_core::geometry::Rect::UNIT);
        f.image_id = Some(bi.media_id);
        f.crop = bi.crop;
        f.opacity = bi.opacity;
        let plan = FramePlan { rect: surf.print_rect(&f.rect_norm), rot: 0.0, img: src.get(bi.media_id), frame: &f };
        draw_frame(&mut out, &map, &plan);
    }
    if let Some(p) = &spread.background.pattern {
        draw_pattern(&mut out, &map, p);
    }
    enum Item<'a> {
        F(&'a Frame),
        S(&'a Shape),
        T(&'a TextBlock),
    }
    let mut items: Vec<(i32, u8, Item)> = Vec::new();
    items.extend(spread.shapes.iter().map(|s| (s.z, 0, Item::S(s))));
    items.extend(spread.frames.iter().map(|f| (f.z, 1, Item::F(f))));
    items.extend(spread.texts.iter().map(|t| (t.z, 2, Item::T(t))));
    items.sort_by_key(|(z, k, _)| (*z, *k));
    for (_, _, it) in items {
        match it {
            Item::F(f) => {
                let plan = FramePlan { rect: surf.print_rect(&f.rect_norm), rot: mdeg_to_rad(f.rotation_mdeg) as f64, img: f.image_id.and_then(|id| src.get(id)), frame: f };
                draw_frame(&mut out, &map, &plan);
            }
            Item::S(s) => draw_shape(&mut out, &map, surf, s),
            Item::T(t) => draw_text(&mut out, &map, surf, t),
        }
    }
    out
}

/// Rasterize a text block (unrotated, box laid out like print) into straight-alpha RGBA.
/// Returns the image and the padding (px) added on each side so overflow stays visible.
pub fn text_rgba(t: &TextBlock, box_w_um: f64, box_h_um: f64, px_per_um: f64) -> Option<(RgbaImage, u32)> {
    let um_to_pt = 72.0 / 25_400.0;
    let lay = text::layout(&t.text, &t.style, (box_w_um * um_to_pt) as f32, (box_h_um * um_to_pt) as f32);
    let font = FontRef::try_from_slice(lay.face().data).ok()?;
    let px_per_pt = px_per_um / um_to_pt;
    let (bw, bh) = ((box_w_um * px_per_um).ceil().max(1.0) as u32, (box_h_um * px_per_um).ceil().max(1.0) as u32);
    let pad = ((lay.size as f64 * px_per_pt).ceil() as u32).min(512);
    let (cw, ch) = (bw + 2 * pad, bh + 2 * pad);
    if (cw as u64) * (ch as u64) > 40_000_000 {
        return None;
    }
    let mut cov = vec![0f32; (cw * ch) as usize];
    let scale: PxScale = font.pt_to_px_scale((lay.size as f64 * px_per_pt) as f32).unwrap_or(PxScale::from(lay.size));
    for line in &lay.lines {
        for g in &line.glyphs {
            let gx = pad as f32 + (g.x as f64 * px_per_pt) as f32;
            let gy = pad as f32 + ((line.baseline - g.y_offset) as f64 * px_per_pt) as f32;
            if let Some(og) = font.outline_glyph(GlyphId(g.gid).with_scale_and_position(scale, ab_glyph::point(gx, gy))) {
                let b = og.px_bounds();
                og.draw(|x, y, c| {
                    let (xx, yy) = (b.min.x as i64 + x as i64, b.min.y as i64 + y as i64);
                    if xx >= 0 && yy >= 0 && (xx as u32) < cw && (yy as u32) < ch {
                        let i = (yy as u32 * cw + xx as u32) as usize;
                        cov[i] = (cov[i] + c).min(1.0);
                    }
                });
            }
        }
    }
    let c = t.style.color;
    let a = c.a as f32;
    let img = RgbaImage::from_fn(cw, ch, |x, y| image::Rgba([c.r, c.g, c.b, (cov[(y * cw + x) as usize] * a) as u8]));
    Some((img, pad))
}

/// Draw a small label (Inter) onto an image — used for proof overlays.
pub fn draw_label(out: &mut RgbImage, text: &str, x: f32, y: f32, size_px: f32, fg: [u8; 3], bg: Option<[u8; 3]>) {
    let Some(face) = fonts::FACES.iter().find(|f| f.postscript == "Inter-Bold") else {
        return;
    };
    let Ok(font) = FontRef::try_from_slice(face.data) else {
        return;
    };
    let scale = font.pt_to_px_scale(size_px).unwrap_or(PxScale::from(size_px));
    let sf = ab_glyph::Font::as_scaled(&font, scale);
    use ab_glyph::ScaleFont;
    let width: f32 = text.chars().map(|c| sf.h_advance(font.glyph_id(c))).sum();
    let (asc, desc) = (sf.ascent(), sf.descent());
    if let Some(bg) = bg {
        let pad = size_px * 0.35;
        let (x0, y0) = ((x - pad).max(0.0) as u32, (y - pad).max(0.0) as u32);
        let (x1, y1) = (((x + width + pad) as u32).min(out.width()), ((y + asc - desc + pad) as u32).min(out.height()));
        for yy in y0..y1 {
            for xx in x0..x1 {
                blend(out.get_pixel_mut(xx, yy), bg.map(|v| v as f32), 0.85);
            }
        }
    }
    let mut pen = x;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        let g = gid.with_scale_and_position(scale, ab_glyph::point(pen, y + asc));
        pen += sf.h_advance(gid);
        if let Some(og) = font.outline_glyph(g) {
            let b = og.px_bounds();
            og.draw(|gx, gy, cv| {
                let (xx, yy) = (b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64);
                if xx >= 0 && yy >= 0 && (xx as u32) < out.width() && (yy as u32) < out.height() {
                    blend(out.get_pixel_mut(xx as u32, yy as u32), fg.map(|v| v as f32), cv);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oto_core::geometry::Rect;
    use oto_core::model::{new_id, Pattern, PatternKind, TextStyle};
    use std::collections::HashMap;

    fn surf() -> Surface {
        Surface { w: 508_000, h: 254_000, bleed: 3_000 }
    }

    #[test]
    fn full_bleed_frame_covers_bleed_and_crop_is_correct() {
        let id = new_id();
        // Left half red, right half blue.
        let img = RgbaImage::from_fn(400, 200, |x, _| if x < 200 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let imgs: HashMap<MediaId, Arc<RgbaImage>> = [(id, Arc::new(img))].into();
        let src = |m: MediaId| imgs.get(&m).cloned();
        let mut s = Spread::blank();
        let mut f = Frame::new(Rect::UNIT);
        f.image_id = Some(id);
        s.frames.push(f);
        let opts = RenderOpts { dpi: 72.0, include_bleed: true };
        let out = render(&s, &surf(), &opts, &src);
        let (w, h) = output_size(&surf(), &opts);
        assert_eq!((out.width(), out.height()), (w, h));
        assert_eq!(out.get_pixel(0, 0).0, [255, 0, 0], "bleed corner covered by the photo");
        assert_eq!(out.get_pixel(w - 1, h - 1).0, [0, 0, 255]);
    }

    #[test]
    fn golden_like_composition() {
        let mut s = Spread::blank();
        s.background.color = Rgba::rgb(250, 248, 242);
        s.background.pattern = Some(Pattern::new(PatternKind::Dots));
        s.shapes.push(Shape::new(ShapeKind::Rect, Rect::new(0.1, 0.1, 0.2, 0.2)));
        s.texts.push(TextBlock::new(Rect::new(0.55, 0.4, 0.4, 0.2), "Aina & Danial", TextStyle { size_pt: 36.0, ..Default::default() }));
        let none = |_m: MediaId| None;
        let out = render(&s, &surf(), &RenderOpts { dpi: 40.0, include_bleed: false }, &none);
        // Text produced dark pixels on the right page; shape filled its area.
        let dark = out.enumerate_pixels().filter(|(x, _, p)| *x > out.width() / 2 && p.0[0] < 100).count();
        assert!(dark > 50, "text ink {dark}");
        let c = out.get_pixel(out.width() / 5, out.height() / 5).0;
        assert_eq!(c, [0xE8, 0xE1, 0xD3]);
    }

    #[test]
    fn missing_image_is_visible_not_white() {
        let mut s = Spread::blank();
        let mut f = Frame::new(Rect::new(0.1, 0.1, 0.3, 0.6));
        f.image_id = Some(new_id());
        s.frames.push(f);
        let none = |_m: MediaId| None;
        let out = render(&s, &surf(), &RenderOpts { dpi: 20.0, include_bleed: false }, &none);
        let p = out.get_pixel(out.width() / 4, out.height() / 2).0;
        assert_ne!(p, [255, 255, 255]);
    }
}
