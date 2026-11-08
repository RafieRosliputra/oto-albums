//! Painting spreads with egui meshes: frames (crop, rotation, rounded corners), text rendered by
//! the print engine, shapes, patterns. Shared by the canvas, book map and template strip.

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Pos2, Rect, Shape, Stroke, TextureId};
use oto_core::crop;
use oto_core::media::MediaItem;
use oto_core::model::{FitMode, Frame, MediaId, Rgba, ShapeKind, Spread};
use oto_core::units::mdeg_to_rad;
use oto_media::Level;
use oto_render::raster::{Surface, UmRect};

use crate::textures::TexCache;

pub fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Maps trim micrometres to screen points.
#[derive(Debug, Clone, Copy)]
pub struct Xf {
    /// Screen position of the trim origin.
    pub origin: Pos2,
    /// Screen points per micrometre.
    pub s: f32,
}

impl Xf {
    pub fn p(&self, x_um: f64, y_um: f64) -> Pos2 {
        Pos2::new(self.origin.x + x_um as f32 * self.s, self.origin.y + y_um as f32 * self.s)
    }
    pub fn to_um(&self, p: Pos2) -> (f64, f64) {
        (((p.x - self.origin.x) / self.s) as f64, ((p.y - self.origin.y) / self.s) as f64)
    }
    pub fn rect(&self, r: &UmRect) -> Rect {
        Rect::from_min_max(self.p(r.x0, r.y0), self.p(r.x1, r.y1))
    }
    pub fn surface_rect(&self, s: &Surface, bleed: bool) -> Rect {
        let b = if bleed { s.bleed as f64 } else { 0.0 };
        Rect::from_min_max(self.p(-b, -b), self.p(s.w as f64 + b, s.h as f64 + b))
    }
}

fn rot(p: egui::Vec2, a: f32) -> egui::Vec2 {
    let (s, c) = a.sin_cos();
    egui::vec2(p.x * c - p.y * s, p.x * s + p.y * c)
}

/// Rounded-rectangle outline in local coordinates (centred).
fn rrect_points(hw: f32, hh: f32, r: f32) -> Vec<egui::Vec2> {
    let r = r.min(hw).min(hh).max(0.0);
    if r < 0.6 {
        return vec![egui::vec2(-hw, -hh), egui::vec2(hw, -hh), egui::vec2(hw, hh), egui::vec2(-hw, hh)];
    }
    let mut pts = Vec::with_capacity(28);
    let corners = [(hw - r, -hh + r, -90.0f32), (hw - r, hh - r, 0.0), (-hw + r, hh - r, 90.0), (-hw + r, -hh + r, 180.0)];
    for (cx, cy, a0) in corners {
        for k in 0..=6 {
            let a = (a0 + k as f32 * 15.0).to_radians();
            pts.push(egui::vec2(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    pts
}

/// Mesh of an image inside a (possibly rotated, rounded) frame.
#[allow(clippy::too_many_arguments)]
pub fn frame_mesh(tex: TextureId, img_w: f32, img_h: f32, center: Pos2, hw: f32, hh: f32, frame_rot: f32, f: &Frame, tint: Color32, radius: f32) -> Mesh {
    let ai = img_w / img_h.max(1.0);
    let af = hw / hh.max(0.01);
    let mut mesh = Mesh::with_texture(tex);
    if f.fit == FitMode::Fit {
        let vis = crop::visible(&f.crop, ai, af, FitMode::Fit);
        let (qw, qh) = (hw / vis.w, hh / vis.h);
        let corners = [(-qw, -qh, 0.0, 0.0), (qw, -qh, 1.0, 0.0), (qw, qh, 1.0, 1.0), (-qw, qh, 0.0, 1.0)];
        for (x, y, u, v) in corners {
            mesh.vertices.push(Vertex { pos: center + rot(egui::vec2(x, y), frame_rot), uv: Pos2::new(u, v), color: tint });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        return mesh;
    }
    let c = crop::clamp(&f.crop, ai, af);
    let vis = crop::visible(&c, ai, af, FitMode::Fill);
    let (cs, cc) = (-mdeg_to_rad(c.rotation_mdeg)).sin_cos();
    let uv_of = |l: egui::Vec2| {
        let u = l.x / (2.0 * hw);
        let v = l.y / (2.0 * hh);
        let dx = u * vis.w * ai;
        let dy = v * vis.h;
        let ix = vis.center.x * ai + dx * cc - dy * cs;
        let iy = vis.center.y + dx * cs + dy * cc;
        Pos2::new(ix / ai, iy)
    };
    mesh.vertices.push(Vertex { pos: center, uv: uv_of(egui::Vec2::ZERO), color: tint });
    let pts = rrect_points(hw, hh, radius);
    for p in &pts {
        mesh.vertices.push(Vertex { pos: center + rot(*p, frame_rot), uv: uv_of(*p), color: tint });
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    mesh
}

pub fn rotated_quad(center: Pos2, hw: f32, hh: f32, a: f32) -> Vec<Pos2> {
    [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].iter().map(|&(x, y)| center + rot(egui::vec2(x, y), a)).collect()
}

pub struct PaintOpts {
    pub level: Level,
    /// Draw text with the print engine (canvas) or as grey bars (thumbnails).
    pub real_text: bool,
    pub bleed: bool,
    pub dim_outside_trim: bool,
}

/// Paint a spread / cover surface.
#[allow(clippy::too_many_arguments)]
pub fn paint_surface(painter: &egui::Painter, ctx: &egui::Context, tex: &mut TexCache, spread: &Spread, surf: &Surface, xf: Xf, media: &dyn Fn(MediaId) -> Option<MediaItem>, o: &PaintOpts) {
    let full = xf.surface_rect(surf, o.bleed);
    painter.rect_filled(full, 0.0, c32(spread.background.color));
    if let Some(bi) = &spread.background.image {
        let mut f = Frame::new(oto_core::geometry::Rect::UNIT);
        f.image_id = Some(bi.media_id);
        f.crop = bi.crop;
        f.opacity = bi.opacity;
        paint_frame(painter, tex, &f, surf, xf, media, o.level, None);
    }
    if let Some(p) = &spread.background.pattern {
        let px_per_mm = xf.s * 1000.0 * ctx.pixels_per_point();
        let tid = tex.pattern(ctx, p, px_per_mm);
        let th = mdeg_to_rad(p.angle_mdeg);
        let (sn, cs) = th.sin_cos();
        let s = p.scale_mm.max(1.0);
        let b = if o.bleed { surf.bleed as f64 } else { 0.0 };
        let corners = [(-b, -b), (surf.w as f64 + b, -b), (surf.w as f64 + b, surf.h as f64 + b), (-b, surf.h as f64 + b)];
        let mut mesh = Mesh::with_texture(tid);
        for (x, y) in corners {
            let (mx, my) = (x as f32 / 1000.0, y as f32 / 1000.0);
            let u = (mx * cs + my * sn) / s;
            let v = (-mx * sn + my * cs) / s;
            mesh.vertices.push(Vertex { pos: xf.p(x, y), uv: Pos2::new(u, v), color: Color32::WHITE });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(Shape::mesh(mesh));
    }
    enum It<'a> {
        F(&'a Frame),
        S(&'a oto_core::model::Shape),
        T(&'a oto_core::model::TextBlock),
    }
    let mut items: Vec<(i32, u8, It)> = Vec::new();
    items.extend(spread.shapes.iter().map(|x| (x.z, 0, It::S(x))));
    items.extend(spread.frames.iter().map(|x| (x.z, 1, It::F(x))));
    items.extend(spread.texts.iter().map(|x| (x.z, 2, It::T(x))));
    items.sort_by_key(|(z, k, _)| (*z, *k));
    for (_, _, it) in items {
        match it {
            It::F(f) => paint_frame(painter, tex, f, surf, xf, media, o.level, None),
            It::S(s) => paint_shape(painter, s, surf, xf),
            It::T(t) => {
                let r = surf.rect(&t.rect_norm);
                let a = mdeg_to_rad(t.rotation_mdeg);
                let sideways = a.sin().abs() > std::f32::consts::FRAC_1_SQRT_2;
                let (bw, bh) = if sideways { (r.h(), r.w()) } else { (r.w(), r.h()) };
                let center = xf.p(r.cx(), r.cy());
                if o.real_text {
                    let ppu = xf.s as f64 * ctx.pixels_per_point() as f64;
                    if let Some((tid, _size, pad_um)) = tex.text(ctx, t, bw, bh, ppu) {
                        let hw = (bw as f32 / 2.0 + pad_um) * xf.s;
                        let hh = (bh as f32 / 2.0 + pad_um) * xf.s;
                        let q = rotated_quad(center, hw, hh, a);
                        let mut mesh = Mesh::with_texture(tid);
                        for (p, uv) in q.iter().zip([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]) {
                            mesh.vertices.push(Vertex { pos: *p, uv: Pos2::new(uv.0, uv.1), color: Color32::WHITE });
                        }
                        mesh.add_triangle(0, 1, 2);
                        mesh.add_triangle(0, 2, 3);
                        painter.add(Shape::mesh(mesh));
                    }
                } else if !t.text.trim().is_empty() {
                    let lines = (bh / (t.style.size_pt as f64 * 25_400.0 / 72.0 * t.style.line_height as f64)).clamp(1.0, 4.0) as i32;
                    let col = c32(t.style.color).gamma_multiply(0.55);
                    for k in 0..lines {
                        let y = center.y - (lines as f32 - 1.0) * 2.5 + k as f32 * 5.0;
                        let w = (bw as f32 * xf.s * if k == lines - 1 && lines > 1 { 0.6 } else { 0.9 }).max(4.0);
                        painter.line_segment([Pos2::new(center.x - w / 2.0, y), Pos2::new(center.x + w / 2.0, y)], Stroke::new(2.0_f32, col));
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn paint_frame(painter: &egui::Painter, tex: &mut TexCache, f: &Frame, surf: &Surface, xf: Xf, media: &dyn Fn(MediaId) -> Option<MediaItem>, level: Level, tint: Option<Color32>) {
    let r = surf.print_rect(&f.rect_norm);
    let sr = xf.rect(&r);
    let (hw, hh) = (sr.width() / 2.0, sr.height() / 2.0);
    let a = mdeg_to_rad(f.rotation_mdeg);
    let radius = f.radius_mm * 1000.0 * xf.s;
    let tint = tint.unwrap_or(Color32::WHITE).gamma_multiply(f.opacity.clamp(0.0, 1.0));
    let mut drew = false;
    if let Some(m) = f.image_id.and_then(media) {
        if !m.missing {
            if let Some((tid, _)) = tex.get(&m.hash, level) {
                // Adjustments preview: B&W and brightness via vertex colour are approximations;
                // the export applies the exact per-pixel maths.
                let mut t = tint;
                if f.adjust.brightness != 0.0 {
                    let k = (1.0 + f.adjust.brightness * 0.6).clamp(0.2, 1.0);
                    t = t.gamma_multiply(k);
                }
                painter.add(Shape::mesh(frame_mesh(tid, m.width as f32, m.height as f32, sr.center(), hw, hh, a, f, t, radius)));
                if f.adjust.bw {
                    // Desaturate preview by overlaying a neutral wash.
                    let q = rotated_quad(sr.center(), hw, hh, a);
                    painter.add(Shape::convex_polygon(q, Color32::from_rgba_unmultiplied(128, 128, 128, 90), Stroke::NONE));
                }
                drew = true;
            }
        }
        if !drew {
            let q = rotated_quad(sr.center(), hw, hh, a);
            painter.add(Shape::convex_polygon(q, Color32::from_rgb(200, 196, 188), Stroke::NONE));
            if m.missing {
                painter.text(sr.center(), egui::Align2::CENTER_CENTER, "⚠ missing", egui::FontId::proportional((hh * 0.2).clamp(8.0, 16.0)), Color32::from_rgb(150, 40, 30));
            }
        }
    } else if f.image_id.is_none() {
        let q = rotated_quad(sr.center(), hw, hh, a);
        painter.add(Shape::convex_polygon(q.clone(), Color32::from_rgba_unmultiplied(120, 120, 120, 40), Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 120, 120, 160))));
        painter.text(sr.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional((hh * 0.4).clamp(8.0, 28.0)), Color32::from_gray(140));
    }
    if f.border.width_mm > 0.0 {
        let bw = f.border.width_mm * 1000.0 * xf.s;
        let pts: Vec<Pos2> = rrect_points(hw - bw / 2.0, hh - bw / 2.0, (radius - bw / 2.0).max(0.0)).into_iter().map(|p| sr.center() + rot(p, a)).collect();
        painter.add(Shape::closed_line(pts, Stroke::new(bw.max(0.5), c32(f.border.color))));
    }
}

pub fn paint_shape(painter: &egui::Painter, s: &oto_core::model::Shape, surf: &Surface, xf: Xf) {
    let r = xf.rect(&surf.rect(&s.rect_norm));
    let a = mdeg_to_rad(s.rotation_mdeg);
    let (hw, hh) = (r.width() / 2.0, r.height() / 2.0);
    let op = s.opacity.clamp(0.0, 1.0);
    let stroke = s.stroke.map(|c| Stroke::new((s.stroke_mm * 1000.0 * xf.s).max(0.5), c32(c).gamma_multiply(op))).unwrap_or(Stroke::NONE);
    let fill = s.fill.map(|c| c32(c).gamma_multiply(op)).unwrap_or(Color32::TRANSPARENT);
    match s.kind {
        ShapeKind::Rect => {
            painter.add(Shape::convex_polygon(rotated_quad(r.center(), hw, hh, a), fill, stroke));
        }
        ShapeKind::Ellipse => {
            let pts: Vec<Pos2> = (0..48)
                .map(|k| {
                    let t = k as f32 / 48.0 * std::f32::consts::TAU;
                    r.center() + rot(egui::vec2(hw * t.cos(), hh * t.sin()), a)
                })
                .collect();
            painter.add(Shape::convex_polygon(pts, fill, stroke));
        }
        ShapeKind::Line => {
            painter.line_segment([r.center() + rot(egui::vec2(-hw, 0.0), a), r.center() + rot(egui::vec2(hw, 0.0), a)], stroke);
        }
    }
}

/// Small, fast thumbnail of a spread (book map, template strip, Auto Build previews).
pub fn paint_thumb(painter: &egui::Painter, ctx: &egui::Context, tex: &mut TexCache, rect: Rect, spread: &Spread, surf: &Surface, media: &dyn Fn(MediaId) -> Option<MediaItem>) {
    let s = (rect.width() / surf.w as f32).min(rect.height() / surf.h as f32);
    let size = egui::vec2(surf.w as f32 * s, surf.h as f32 * s);
    let r = Rect::from_center_size(rect.center(), size);
    let xf = Xf { origin: r.min, s };
    let p = painter.with_clip_rect(r);
    paint_surface(&p, ctx, tex, spread, surf, xf, media, &PaintOpts { level: Level::Thumb, real_text: false, bleed: false, dim_outside_trim: false });
    // Fold line.
    if surf.w > surf.h {
        p.line_segment([Pos2::new(r.center().x, r.top()), Pos2::new(r.center().x, r.bottom())], Stroke::new(1.0_f32, Color32::from_black_alpha(40)));
    }
}
