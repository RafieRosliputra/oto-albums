//! Spread canvas: pan/zoom, overlays, selection, move/resize/rotate with snapping, crop mode,
//! focus points, photo drops and the designer tools.

use egui::{Color32, CursorIcon, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use oto_core::crop;
use oto_core::geometry::{Rect as NRect, Vec2 as NVec2};
use oto_core::model::{FitMode, FocusPoint, Frame, ItemId, MediaId, Shape as OShape, ShapeKind, Spread, StylePreset, TextBlock};
use oto_core::units::{deg_to_mdeg, mdeg_to_rad};
use oto_media::Level;
use oto_render::raster::Surface;

use crate::draw::{self, paint_surface, PaintOpts, Xf};
use crate::i18n::t;
use crate::settings::AppSettings;
use crate::state::{MsgKind, RightTab, Target, Tool, Workspace};
use crate::theme;

#[derive(Debug, Clone)]
pub enum DragKind {
    Move { ids: Vec<ItemId> },
    Resize { id: ItemId, handle: u8 },
    Rotate { id: ItemId },
    CropPan { id: ItemId },
    Create { tool: Tool },
    Pan,
}

#[derive(Debug, Clone)]
pub struct CanvasDrag {
    pub kind: DragKind,
    pub start: Pos2,
    pub orig: Spread,
    pub working: Spread,
    pub guides: Vec<(bool, f32)>,
    pub swap_target: Option<ItemId>,
}

// ---------------------------------------------------------------------------------------------
// Item helpers
// ---------------------------------------------------------------------------------------------

pub fn item_rect(s: &Spread, id: ItemId) -> Option<(NRect, i32, bool)> {
    if let Some(f) = s.frames.iter().find(|f| f.id == id) {
        return Some((f.rect_norm, f.rotation_mdeg, f.locked));
    }
    if let Some(t) = s.texts.iter().find(|t| t.id == id) {
        return Some((t.rect_norm, t.rotation_mdeg, t.locked));
    }
    s.shapes.iter().find(|x| x.id == id).map(|x| (x.rect_norm, x.rotation_mdeg, x.locked))
}

pub fn set_item_rect(s: &mut Spread, id: ItemId, r: NRect) {
    if let Some(f) = s.frames.iter_mut().find(|f| f.id == id) {
        f.rect_norm = r;
    } else if let Some(t) = s.texts.iter_mut().find(|t| t.id == id) {
        t.rect_norm = r;
    } else if let Some(x) = s.shapes.iter_mut().find(|x| x.id == id) {
        x.rect_norm = r;
    }
}

pub fn set_item_rot(s: &mut Spread, id: ItemId, mdeg: i32) {
    if let Some(f) = s.frames.iter_mut().find(|f| f.id == id) {
        f.rotation_mdeg = mdeg;
    } else if let Some(t) = s.texts.iter_mut().find(|t| t.id == id) {
        t.rotation_mdeg = mdeg;
    } else if let Some(x) = s.shapes.iter_mut().find(|x| x.id == id) {
        x.rotation_mdeg = mdeg;
    }
}

/// Items under a trim-normalized point, topmost first.
fn hit_items(s: &Spread, surf: &Surface, p: NVec2) -> Vec<ItemId> {
    let mut v: Vec<(i32, u8, ItemId, NRect, i32)> = Vec::new();
    v.extend(s.shapes.iter().map(|x| (x.z, 0, x.id, x.rect_norm, x.rotation_mdeg)));
    v.extend(s.frames.iter().map(|x| (x.z, 1, x.id, x.rect_norm, x.rotation_mdeg)));
    v.extend(s.texts.iter().map(|x| (x.z, 2, x.id, x.rect_norm, x.rotation_mdeg)));
    v.sort_by_key(|(z, k, ..)| std::cmp::Reverse((*z, *k)));
    let aspect = surf.w as f32 / surf.h.max(1) as f32;
    v.into_iter()
        .filter(|(_, _, _, r, rot)| {
            let c = r.center();
            // Work in physical-ish units so rotation is correct on non-square surfaces.
            let (dx, dy) = ((p.x - c.x) * aspect, p.y - c.y);
            let a = -mdeg_to_rad(*rot);
            let (sn, cs) = a.sin_cos();
            let lx = dx * cs - dy * sn;
            let ly = dx * sn + dy * cs;
            lx.abs() <= r.w * aspect / 2.0 && ly.abs() <= r.h / 2.0
        })
        .map(|(_, _, id, _, _)| id)
        .collect()
}

fn handle_points(r: Rect) -> [Pos2; 8] {
    let c = r.center();
    [r.left_top(), Pos2::new(c.x, r.top()), r.right_top(), Pos2::new(r.right(), c.y), r.right_bottom(), Pos2::new(c.x, r.bottom()), r.left_bottom(), Pos2::new(r.left(), c.y)]
}

// ---------------------------------------------------------------------------------------------
// Snapping
// ---------------------------------------------------------------------------------------------

fn snap_lines(s: &Spread, surf: &Surface, skip: &[ItemId], is_spread: bool, geom: &oto_core::spec::SpreadGeometry) -> (Vec<f32>, Vec<f32>) {
    let mut xs = vec![0.0, 1.0];
    let mut ys = vec![0.0, 0.5, 1.0];
    if is_spread {
        xs.push(0.5);
        xs.push(0.25);
        xs.push(0.75);
        for r in geom.safe_pages() {
            let n = r.to_norm(surf.w, surf.h);
            xs.push(n.x);
            xs.push(n.right());
            ys.push(n.y);
            ys.push(n.bottom());
        }
    } else {
        xs.push(0.5);
    }
    let rects = s.frames.iter().map(|f| (f.id, f.rect_norm)).chain(s.texts.iter().map(|t| (t.id, t.rect_norm))).chain(s.shapes.iter().map(|x| (x.id, x.rect_norm)));
    for (id, r) in rects {
        if skip.contains(&id) {
            continue;
        }
        xs.extend([r.x, r.right(), r.center().x]);
        ys.extend([r.y, r.bottom(), r.center().y]);
    }
    (xs, ys)
}

fn snap(v: f32, lines: &[f32], tol: f32) -> Option<f32> {
    lines.iter().copied().filter(|l| (l - v).abs() <= tol).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
}

// ---------------------------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------------------------

pub fn toolbar(ui: &mut Ui, ws: &mut Workspace) {
    // Wrap instead of overflowing: an overflowing row would widen the central panel and push the canvas under the inspector.
    ui.horizontal_wrapped(|ui| {
        let tools = [
            (Tool::Select, "⬉", "Select (V)"),
            (Tool::Frame, "▣", "Frame tool (R)"),
            (Tool::Text, "T", "Text tool (T)"),
            (Tool::Rect, "■", "Rectangle"),
            (Tool::Ellipse, "●", "Ellipse"),
            (Tool::Line, "╱", "Line"),
        ];
        for (tool, icon, tip) in tools {
            if ui.selectable_label(ws.tool == tool, icon).on_hover_text(tip).clicked() {
                ws.tool = tool;
                ws.crop_mode = None;
            }
        }
        ui.separator();
        let can_crop = ws.sel.len() == 1 && ws.current().map(|s| s.frames.iter().any(|f| f.id == ws.sel[0] && f.image_id.is_some())).unwrap_or(false);
        if ui
            .add_enabled(can_crop || ws.crop_mode.is_some(), egui::SelectableLabel::new(ws.crop_mode.is_some(), format!("✂ {}", t("Crop"))))
            .on_hover_text("Crop mode (C) — drag to pan, scroll to zoom, Alt-click to set the focus point")
            .clicked()
        {
            ws.crop_mode = if ws.crop_mode.is_some() { None } else { ws.sel.first().copied() };
        }
        if ui.selectable_label(ws.view.compare, format!("◫ {}", t("Compare"))).on_hover_text("Compare the hovered template side by side with the current layout (Space)").clicked() {
            ws.view.compare = !ws.view.compare;
        }
        ui.separator();
        if ui.button("−").on_hover_text("Zoom out ([)").clicked() {
            ws.view.zoom = (ws.view.zoom / 1.25).max(0.05);
        }
        ui.label(format!("{:.0}%", ws.view.zoom / actual_zoom(ui.ctx()) * 100.0));
        if ui.button("+").on_hover_text("Zoom in (])").clicked() {
            ws.view.zoom = (ws.view.zoom * 1.25).min(40.0);
        }
        if ui.button(t("Fit to screen")).on_hover_text("0").clicked() {
            ws.view.fit_pending = true;
            ws.view.user_moved = false;
        }
        if ui.button("100%").on_hover_text("Physical size (1)").clicked() {
            ws.view.zoom = actual_zoom(ui.ctx());
            ws.view.pan = Vec2::ZERO;
        }
        ui.separator();
        if ui.selectable_label(ws.view.continuous, t("Continuous view")).clicked() {
            ws.view.continuous = !ws.view.continuous;
        }
        if let Some(s) = ws.current() {
            if !s.explain.is_empty() {
                ui.separator();
                let explain = s.explain.clone();
                ui.add(egui::Label::new(egui::RichText::new(format!("ℹ {explain}")).small().weak()).truncate()).on_hover_text(format!("{}\n{explain}", t("Why this layout?")));
            }
        }
    });
}

/// Points per mm that show the page at physical size on a ~96 dpi display.
pub fn actual_zoom(_ctx: &egui::Context) -> f32 {
    96.0 / 25.4
}

pub fn canvas(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    // Only the visible part of the panel counts, so "fit" never places the spread under a side panel.
    let visible = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    let (rect, resp) = ui.allocate_exact_size(visible.size().max(Vec2::splat(1.0)), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, pal.canvas);
    if ws.view.continuous && ws.target != Target::Cover {
        continuous(ui, ws, rect, &resp, settings);
        return;
    }
    let Some(current) = ws.current().cloned() else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No spread selected — add a spread from the book map.", egui::FontId::proportional(15.0), pal.muted);
        return;
    };
    let surf = ws.surface();
    let is_spread = matches!(ws.target, Target::Spread(_));
    let geom = ws.project().spec.geometry();
    let ruler = if settings.show_rulers { 22.0 } else { 0.0 };
    let area = Rect::from_min_max(rect.min + Vec2::splat(ruler), rect.max);
    // Zoom / pan input.
    let media_w = (surf.w + 2 * surf.bleed) as f32 / 1000.0;
    let media_h = (surf.h + 2 * surf.bleed) as f32 / 1000.0;
    let compare = ws.view.compare && ws.preview.is_some();
    let view_area = if compare { Rect::from_min_max(area.min, Pos2::new(area.center().x - 4.0, area.max.y)) } else { area };
    let resized = (view_area.size() - ws.view.last_area).length() > 2.0;
    ws.view.last_area = view_area.size();
    if ws.view.fit_pending || (resized && !ws.view.user_moved) {
        ws.view.zoom = ((view_area.width() - 60.0) / media_w).min((view_area.height() - 60.0) / media_h).max(0.05);
        ws.view.pan = Vec2::ZERO;
        ws.view.fit_pending = false;
        ui.ctx().request_repaint();
    }
    let hovered = resp.hovered() || resp.dragged();
    if hovered {
        let (scroll, zoom_delta, mods, ptr) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.hover_pos()));
        let crop_scroll = ws.crop_mode.is_some() && !mods.command;
        if zoom_delta != 1.0 || (mods.command && scroll.y != 0.0) {
            let factor = if zoom_delta != 1.0 { zoom_delta } else { (scroll.y * 0.003).exp() };
            let old = ws.view.zoom;
            ws.view.user_moved = true;
            ws.view.zoom = (old * factor).clamp(0.05, 40.0);
            if let Some(p) = ptr {
                let c = view_area.center() + ws.view.pan;
                let d = p - c;
                ws.view.pan += d - d * (ws.view.zoom / old);
            }
        } else if !crop_scroll && scroll != Vec2::ZERO {
            ws.view.user_moved = true;
            ws.view.pan += scroll;
        }
    }
    let zoom = ws.view.zoom;
    let s_um = zoom / 1000.0;
    let size = Vec2::new(surf.w as f32 * s_um, surf.h as f32 * s_um);
    let center = view_area.center() + ws.view.pan;
    let xf = Xf { origin: center - size / 2.0, s: s_um };
    let px_per_mm_screen = zoom * ui.ctx().pixels_per_point();
    let level = if px_per_mm_screen * (surf.w as f32 / 1000.0) / 2.0 > 1400.0 { Level::Preview } else { Level::Proxy };
    let media_fn = |id: MediaId| ws.media(id).cloned();
    let shown: Spread = match &ws.drag_state {
        Some(d) => d.working.clone(),
        None if !compare => ws.preview.clone().unwrap_or_else(|| current.clone()),
        None => current.clone(),
    };
    let shown = if ws.anim.active() && ws.drag_state.is_none() && ws.preview.is_none() {
        ui.ctx().request_repaint();
        ws.anim.apply(&shown)
    } else {
        shown
    };
    {
        let media_vec: std::collections::HashMap<MediaId, oto_core::media::MediaItem> = shown
            .media_ids()
            .iter()
            .chain(ws.preview.iter().flat_map(|p| p.media_ids()).collect::<Vec<_>>().iter())
            .filter_map(|id| media_fn(*id).map(|m| (*id, m)))
            .chain(shown.background.image.as_ref().and_then(|b| media_fn(b.media_id).map(|m| (b.media_id, m))))
            .collect();
        let lookup = |id: MediaId| media_vec.get(&id).cloned();
        let clip = painter.with_clip_rect(view_area);
        // Paper shadow.
        let paper = xf.surface_rect(&surf, true);
        clip.rect_filled(paper.translate(Vec2::new(0.0, 3.0)).expand(2.0), 2.0, Color32::from_black_alpha(60));
        paint_surface(&clip, ui.ctx(), &mut ws.tex, &shown, &surf, xf, &lookup, &PaintOpts { level, real_text: true, bleed: true, dim_outside_trim: true });
        overlays(&clip, ws, &surf, xf, &geom, is_spread, settings, &pal);
        if compare {
            let right = Rect::from_min_max(Pos2::new(area.center().x + 4.0, area.min.y), area.max);
            let rc = right.center() + ws.view.pan;
            let xf2 = Xf { origin: rc - size / 2.0, s: s_um };
            let clip2 = painter.with_clip_rect(right);
            if let Some(pv) = ws.preview.clone() {
                clip2.rect_filled(xf2.surface_rect(&surf, true), 0.0, Color32::WHITE);
                paint_surface(&clip2, ui.ctx(), &mut ws.tex, &pv, &surf, xf2, &lookup, &PaintOpts { level, real_text: true, bleed: true, dim_outside_trim: true });
                let name = ws.template(&pv.template_id).map(|t| t.name.clone()).unwrap_or_default();
                clip2.text(right.left_top() + Vec2::new(10.0, 10.0), egui::Align2::LEFT_TOP, format!("Preview: {name}"), egui::FontId::proportional(13.0), pal.text);
            }
            painter.text(view_area.left_top() + Vec2::new(10.0, 10.0), egui::Align2::LEFT_TOP, "Current", egui::FontId::proportional(13.0), pal.text);
            painter.line_segment([Pos2::new(area.center().x, area.top()), Pos2::new(area.center().x, area.bottom())], Stroke::new(1.0_f32, pal.border));
        }
        // Crop ghost: the whole photo, faint, so you see what is outside the frame.
        if let Some(cid) = ws.crop_mode {
            if let Some(f) = shown.frames.iter().find(|f| f.id == cid) {
                crop_ghost(&clip, ws, f, &surf, xf, &lookup);
            }
        }
    }
    if settings.show_rulers {
        rulers(&painter, rect, ruler, xf, &surf, settings, &pal);
    }
    selection(&painter, ws, &shown, &surf, xf, &pal);
    dpi_badges(&painter, ws, &shown, &surf, xf, &pal);
    if ws.preview.is_some() && !compare {
        painter.text(view_area.left_top() + Vec2::new(10.0, 10.0), egui::Align2::LEFT_TOP, "Preview — click a template to apply, Space to compare", egui::FontId::proportional(12.0), pal.accent);
    }
    if compare {
        return; // compare view is read-only
    }
    interact(ui, ws, &resp, &current, &surf, xf, is_spread, &geom);
}

fn overlays(p: &egui::Painter, ws: &Workspace, surf: &Surface, xf: Xf, geom: &oto_core::spec::SpreadGeometry, is_spread: bool, settings: &AppSettings, pal: &theme::Palette) {
    let trim = xf.surface_rect(surf, false);
    let media = xf.surface_rect(surf, true);
    // Bleed area: dim it so the eye reads the trimmed page.
    let dim = Color32::from_black_alpha(70);
    for r in [
        Rect::from_min_max(media.min, Pos2::new(media.max.x, trim.min.y)),
        Rect::from_min_max(Pos2::new(media.min.x, trim.max.y), media.max),
        Rect::from_min_max(Pos2::new(media.min.x, trim.min.y), Pos2::new(trim.min.x, trim.max.y)),
        Rect::from_min_max(Pos2::new(trim.max.x, trim.min.y), Pos2::new(media.max.x, trim.max.y)),
    ] {
        p.rect_filled(r, 0.0, dim);
    }
    p.rect_stroke(media, 0.0, Stroke::new(1.0_f32, pal.crit.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    p.rect_stroke(trim, 0.0, Stroke::new(1.0_f32, Color32::from_black_alpha(160)), egui::StrokeKind::Middle);
    if !settings.show_guides {
        return;
    }
    let dash = |a: Pos2, b: Pos2, c: Color32| {
        p.extend(Shape::dashed_line(&[a, b], Stroke::new(1.0_f32, c), 5.0, 4.0));
    };
    let rect_dash = |r: Rect, c: Color32| {
        dash(r.left_top(), r.right_top(), c);
        dash(r.right_top(), r.right_bottom(), c);
        dash(r.right_bottom(), r.left_bottom(), c);
        dash(r.left_bottom(), r.left_top(), c);
    };
    if is_spread {
        for sr in geom.safe_pages() {
            let r = xf.rect(&oto_render::raster::UmRect { x0: sr.x as f64, y0: sr.y as f64, x1: sr.right() as f64, y1: sr.bottom() as f64 });
            rect_dash(r, pal.guide);
        }
        let band = geom.gutter_band();
        let br = xf.rect(&oto_render::raster::UmRect { x0: band.x as f64, y0: 0.0, x1: band.right() as f64, y1: surf.h as f64 });
        p.rect_filled(br, 0.0, pal.guide.gamma_multiply(0.10));
        p.line_segment([Pos2::new(trim.center().x, trim.top()), Pos2::new(trim.center().x, trim.bottom())], Stroke::new(1.0_f32, pal.guide.gamma_multiply(0.8)));
        // Trim tolerance band (thin, inside the trim line).
        if geom.trim_outer > 0 {
            let to = geom.trim_outer as f32 * xf.s;
            p.rect_stroke(trim.shrink(to), 0.0, Stroke::new(0.5_f32, Color32::from_black_alpha(60)), egui::StrokeKind::Middle);
        }
        // Page numbers.
        if let Some(i) = ws.spread_index() {
            let first = i * 2 + 1;
            let y = media.bottom() + 12.0;
            p.text(Pos2::new(trim.left() + trim.width() * 0.25, y), egui::Align2::CENTER_CENTER, format!("Page {first}"), egui::FontId::proportional(11.0), pal.muted);
            p.text(Pos2::new(trim.left() + trim.width() * 0.75, y), egui::Align2::CENTER_CENTER, format!("Page {}", first + 1), egui::FontId::proportional(11.0), pal.muted);
            p.text(Pos2::new(trim.center().x, y), egui::Align2::CENTER_CENTER, format!("Spread {}", i + 1), egui::FontId::proportional(11.0), pal.text);
        }
    } else {
        let g = ws.cover_geometry();
        for sr in g.safe_rects() {
            let r = xf.rect(&oto_render::raster::UmRect { x0: sr.x as f64, y0: sr.y as f64, x1: sr.right() as f64, y1: sr.bottom() as f64 });
            rect_dash(r, pal.guide);
        }
        let sp = g.spine();
        let r = xf.rect(&oto_render::raster::UmRect { x0: sp.x as f64, y0: 0.0, x1: sp.right() as f64, y1: g.trim_h() as f64 });
        p.rect_filled(r, 0.0, pal.guide.gamma_multiply(0.10));
        dash(r.left_top(), r.left_bottom(), pal.guide);
        dash(r.right_top(), r.right_bottom(), pal.guide);
        if g.wrap > 0 {
            let w = g.wrap as f32 * xf.s;
            rect_dash(trim.shrink(w), pal.accent.gamma_multiply(0.8));
        }
        for x in [sp.x - g.hinge, sp.right() + g.hinge] {
            if g.hinge > 0 {
                let px = xf.p(x as f64, 0.0).x;
                dash(Pos2::new(px, trim.top()), Pos2::new(px, trim.bottom()), pal.muted);
            }
        }
        let y = media.bottom() + 12.0;
        p.text(Pos2::new(xf.p((g.back().x + g.back().w / 2) as f64, 0.0).x, y), egui::Align2::CENTER_CENTER, "Back", egui::FontId::proportional(11.0), pal.muted);
        p.text(Pos2::new(r.center().x, y), egui::Align2::CENTER_CENTER, format!("Spine {:.1} mm", g.spine_w as f32 / 1000.0), egui::FontId::proportional(11.0), pal.text);
        p.text(Pos2::new(xf.p((g.front().x + g.front().w / 2) as f64, 0.0).x, y), egui::Align2::CENTER_CENTER, "Front", egui::FontId::proportional(11.0), pal.muted);
    }
    if settings.show_grid {
        let step = 10_000.0;
        let mut x = 0.0;
        while x <= surf.w as f64 {
            let px = xf.p(x, 0.0).x;
            p.line_segment([Pos2::new(px, trim.top()), Pos2::new(px, trim.bottom())], Stroke::new(0.5_f32, Color32::from_black_alpha(30)));
            x += step;
        }
        let mut y = 0.0;
        while y <= surf.h as f64 {
            let py = xf.p(0.0, y).y;
            p.line_segment([Pos2::new(trim.left(), py), Pos2::new(trim.right(), py)], Stroke::new(0.5_f32, Color32::from_black_alpha(30)));
            y += step;
        }
    }
}

fn rulers(p: &egui::Painter, rect: Rect, w: f32, xf: Xf, surf: &Surface, settings: &AppSettings, pal: &theme::Palette) {
    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), w));
    let left = Rect::from_min_size(rect.min, Vec2::new(w, rect.height()));
    p.rect_filled(top, 0.0, pal.panel);
    p.rect_filled(left, 0.0, pal.panel);
    let units = settings.units;
    let unit_um = match units {
        oto_core::units::Units::In => 25_400.0,
        oto_core::units::Units::Cm => 10_000.0,
        oto_core::units::Units::Mm => 10_000.0,
    };
    let px_per_unit = unit_um as f32 * xf.s;
    let label_every = (60.0 / px_per_unit).ceil().max(1.0) as i64;
    let font = egui::FontId::proportional(9.0);
    let (x0, _) = xf.to_um(Pos2::new(rect.left() + w, 0.0));
    let (x1, _) = xf.to_um(Pos2::new(rect.right(), 0.0));
    let mut k = (x0 / unit_um).floor() as i64;
    while (k as f64) * unit_um <= x1 {
        let px = xf.p(k as f64 * unit_um, 0.0).x;
        if px > rect.left() + w {
            let major = k % label_every == 0;
            p.line_segment([Pos2::new(px, top.bottom() - if major { 9.0 } else { 4.0 }), Pos2::new(px, top.bottom())], Stroke::new(1.0_f32, pal.muted));
            if major {
                let v = match units {
                    oto_core::units::Units::Mm => k * 10,
                    _ => k,
                };
                p.text(Pos2::new(px + 2.0, top.top() + 2.0), egui::Align2::LEFT_TOP, v.to_string(), font.clone(), pal.muted);
            }
        }
        k += 1;
    }
    let (_, y0) = xf.to_um(Pos2::new(0.0, rect.top() + w));
    let (_, y1) = xf.to_um(Pos2::new(0.0, rect.bottom()));
    let mut k = (y0 / unit_um).floor() as i64;
    while (k as f64) * unit_um <= y1 {
        let py = xf.p(0.0, k as f64 * unit_um).y;
        if py > rect.top() + w {
            let major = k % label_every == 0;
            p.line_segment([Pos2::new(left.right() - if major { 9.0 } else { 4.0 }, py), Pos2::new(left.right(), py)], Stroke::new(1.0_f32, pal.muted));
            if major {
                let v = match units {
                    oto_core::units::Units::Mm => k * 10,
                    _ => k,
                };
                p.text(Pos2::new(left.left() + 2.0, py + 1.0), egui::Align2::LEFT_TOP, v.to_string(), font.clone(), pal.muted);
            }
        }
        k += 1;
    }
    p.text(rect.min + Vec2::splat(3.0), egui::Align2::LEFT_TOP, units.label(), font, pal.text);
    let _ = surf;
}

fn crop_ghost(p: &egui::Painter, ws: &mut Workspace, f: &Frame, surf: &Surface, xf: Xf, lookup: &dyn Fn(MediaId) -> Option<oto_core::media::MediaItem>) {
    let Some(m) = f.image_id.and_then(lookup) else { return };
    let Some((tid, _)) = ws.tex.get(&m.hash, Level::Proxy) else { return };
    let r = xf.rect(&surf.print_rect(&f.rect_norm));
    let (hw, hh) = (r.width() / 2.0, r.height() / 2.0);
    let ai = m.aspect();
    let af = hw / hh.max(0.01);
    let c = crop::clamp(&f.crop, ai, af);
    let vis = crop::visible(&c, ai, af, FitMode::Fill);
    let th = mdeg_to_rad(c.rotation_mdeg);
    let fa = mdeg_to_rad(f.rotation_mdeg);
    let corner = |x: f32, y: f32| {
        let (dx, dy) = ((x - vis.center.x) * ai, y - vis.center.y);
        let (s, co) = th.sin_cos();
        let (rx, ry) = (dx * co - dy * s, dx * s + dy * co);
        let l = Vec2::new(rx / (vis.w * ai) * 2.0 * hw, ry / vis.h * 2.0 * hh);
        let (s2, c2) = fa.sin_cos();
        r.center() + Vec2::new(l.x * c2 - l.y * s2, l.x * s2 + l.y * c2)
    };
    let mut mesh = egui::epaint::Mesh::with_texture(tid);
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        mesh.vertices.push(egui::epaint::Vertex { pos: corner(x, y), uv: Pos2::new(x, y), color: Color32::from_white_alpha(90) });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(Shape::mesh(mesh));
    let q: Vec<Pos2> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().map(|&(x, y)| corner(x, y)).collect();
    p.add(Shape::closed_line(q, Stroke::new(1.0_f32, Color32::from_white_alpha(160))));
    // Frame on top again at full strength.
    draw::paint_frame(p, &mut ws.tex, f, surf, xf, lookup, Level::Proxy, None);
    // Focus point.
    let focus = f.focus.map(|fp| NVec2::new(fp.x, fp.y)).or(m.focus).or(m.features.focus_auto);
    if let Some(fp) = focus {
        let pos = corner(fp.x, fp.y);
        let manual = f.focus.map(|x| x.manual).unwrap_or(false) || m.focus.is_some();
        p.circle_stroke(pos, 9.0, Stroke::new(2.0_f32, if manual { theme::VERMILION } else { Color32::from_white_alpha(200) }));
        p.line_segment([pos - Vec2::new(14.0, 0.0), pos + Vec2::new(14.0, 0.0)], Stroke::new(1.0_f32, Color32::WHITE));
        p.line_segment([pos - Vec2::new(0.0, 14.0), pos + Vec2::new(0.0, 14.0)], Stroke::new(1.0_f32, Color32::WHITE));
    }
}

fn selection(p: &egui::Painter, ws: &Workspace, s: &Spread, surf: &Surface, xf: Xf, pal: &theme::Palette) {
    if let Some(d) = &ws.drag_state {
        for (vertical, v) in &d.guides {
            if *vertical {
                let x = xf.p(*v as f64 * surf.w as f64, 0.0).x;
                p.line_segment([Pos2::new(x, xf.origin.y - 2000.0), Pos2::new(x, xf.origin.y + 4000.0)], Stroke::new(1.0_f32, theme::CYAN));
            } else {
                let y = xf.p(0.0, *v as f64 * surf.h as f64).y;
                p.line_segment([Pos2::new(xf.origin.x - 4000.0, y), Pos2::new(xf.origin.x + 8000.0, y)], Stroke::new(1.0_f32, theme::CYAN));
            }
        }
        if let Some(sw) = d.swap_target {
            if let Some((r, _, _)) = item_rect(s, sw) {
                p.rect_stroke(xf.rect(&surf.rect(&r)), 0.0, Stroke::new(3.0_f32, pal.accent), egui::StrokeKind::Inside);
            }
        }
    }
    for id in &ws.sel {
        let Some((r, rot, locked)) = item_rect(s, *id) else { continue };
        let sr = xf.rect(&surf.rect(&r));
        let a = mdeg_to_rad(rot);
        let q = draw::rotated_quad(sr.center(), sr.width() / 2.0, sr.height() / 2.0, a);
        p.add(Shape::closed_line(q.clone(), Stroke::new(1.5_f32, pal.selection)));
        if locked {
            p.text(sr.left_top() + Vec2::new(4.0, 4.0), egui::Align2::LEFT_TOP, "🔒", egui::FontId::proportional(13.0), pal.selection);
            continue;
        }
        if ws.sel.len() == 1 && ws.crop_mode.is_none() {
            let hp = if rot == 0 { handle_points(sr).to_vec() } else { vec![q[0], q[1], q[2], q[3]] };
            for h in hp {
                p.rect_filled(Rect::from_center_size(h, Vec2::splat(9.0)), 2.0, Color32::WHITE);
                p.rect_stroke(Rect::from_center_size(h, Vec2::splat(9.0)), 2.0, Stroke::new(1.0_f32, pal.selection), egui::StrokeKind::Middle);
            }
            let top = sr.center() + Vec2::new(0.0, -sr.height() / 2.0 - 22.0).rotated(a);
            p.circle_filled(top, 5.5, pal.selection);
            p.line_segment([sr.center() + Vec2::new(0.0, -sr.height() / 2.0).rotated(a), top], Stroke::new(1.0_f32, pal.selection));
        }
    }
}

trait Rot {
    fn rotated(self, a: f32) -> Self;
}
impl Rot for Vec2 {
    fn rotated(self, a: f32) -> Vec2 {
        let (s, c) = a.sin_cos();
        Vec2::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }
}

fn dpi_badges(p: &egui::Painter, ws: &Workspace, s: &Spread, surf: &Surface, xf: Xf, pal: &theme::Palette) {
    for i in ws.preflight.for_spread(s.id) {
        if !i.code.starts_with("DPI") && i.code != "FACE_IN_GUTTER" && i.code != "MISSING_IMAGE" {
            continue;
        }
        let Some(id) = i.item_id else { continue };
        let Some((r, _, _)) = item_rect(s, id) else { continue };
        let sr = xf.rect(&surf.rect(&r));
        let crit = i.severity == oto_print::preflight::Severity::Critical;
        let (col, label) = if crit { (pal.crit, "✖") } else { (pal.warn, "⚠") };
        let text = match i.code.as_str() {
            "FACE_IN_GUTTER" => format!("{label} face in fold"),
            "MISSING_IMAGE" => format!("{label} missing"),
            _ => format!("{label} {}", i.message.split(" prints at ").nth(1).unwrap_or("low dpi").trim_end_matches('.')),
        };
        let pos = sr.right_top() + Vec2::new(-6.0, 6.0);
        let g = p.layout_no_wrap(text, egui::FontId::proportional(11.0), Color32::WHITE);
        let br = Rect::from_min_size(pos - Vec2::new(g.size().x + 8.0, 0.0), g.size() + Vec2::new(8.0, 4.0));
        p.rect_filled(br, 4.0, col);
        p.galley(br.min + Vec2::new(4.0, 2.0), g, Color32::WHITE);
    }
}

#[allow(clippy::too_many_arguments)]
fn interact(ui: &mut Ui, ws: &mut Workspace, resp: &egui::Response, current: &Spread, surf: &Surface, xf: Xf, is_spread: bool, geom: &oto_core::spec::SpreadGeometry) {
    let ptr = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
    let mods = ui.input(|i| i.modifiers);
    let space = ui.input(|i| i.key_down(Key::Space));
    let to_norm = |p: Pos2| {
        let (x, y) = xf.to_um(p);
        NVec2::new((x / surf.w as f64) as f32, (y / surf.h as f64) as f32)
    };
    // --- Media drop from the browser / timeline ---
    if let Some(ids) = ws.drag_media.clone() {
        if resp.hovered() || resp.contains_pointer() {
            ui.ctx().set_cursor_icon(CursorIcon::Copy);
            if ui.input(|i| i.pointer.any_released()) {
                if let Some(p) = ptr {
                    drop_media(ws, current, surf, &ids, to_norm(p));
                }
                ws.drag_media = None;
            }
        }
    }
    // --- Pattern drop from the Patterns tab ---
    if let Some(k) = ws.drag_pattern {
        if resp.contains_pointer() {
            ui.ctx().set_cursor_icon(CursorIcon::Copy);
        }
        if ui.input(|i| i.pointer.any_released()) {
            if resp.contains_pointer() {
                ws.set_pattern(k);
            }
            ws.drag_pattern = None;
        }
    }
    let Some(p) = ptr else { return };
    if resp.hovered() {
        let np = to_norm(p);
        ws.hover_media = hit_items(current, surf, np).into_iter().find_map(|id| current.frame(id).and_then(|f| f.image_id));
    }
    if space && ui.input(|i| i.pointer.any_down()) {
        ws.space_panned = true;
    }
    let np = to_norm(p);

    // --- Crop mode ---
    if let Some(cid) = ws.crop_mode {
        if let Some(f) = current.frames.iter().find(|f| f.id == cid).cloned() {
            let r = xf.rect(&surf.print_rect(&f.rect_norm));
            let inside = r.expand(40.0).contains(p);
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if inside && resp.hovered() && scroll != 0.0 && !mods.command {
                let mut s = current.clone();
                if let (Some(fr), Some(m)) = (s.frame_mut(cid), f.image_id.and_then(|id| ws.media(id))) {
                    let ai = m.aspect();
                    let af = r.width() / r.height().max(1.0);
                    fr.crop.zoom = (fr.crop.zoom * (scroll * 0.004).exp()).clamp(1.0, 12.0);
                    fr.crop = crop::clamp(&fr.crop, ai, af);
                    fr.crop_manual = true;
                }
                ws.commit(s, "Zoom crop");
            }
            if (resp.clicked_by(PointerButton::Secondary) || (resp.clicked() && mods.alt)) && r.contains(p) {
                if let Some(m) = f.image_id.and_then(|id| ws.media(id)).cloned() {
                    let ai = m.aspect();
                    let af = r.width() / r.height().max(1.0);
                    let c = crop::clamp(&f.crop, ai, af);
                    let vis = crop::visible(&c, ai, af, FitMode::Fill);
                    let (u, v) = ((p.x - r.center().x) / r.width(), (p.y - r.center().y) / r.height());
                    let (sn, cs) = (-mdeg_to_rad(c.rotation_mdeg)).sin_cos();
                    let (dx, dy) = (u * vis.w * ai, v * vis.h);
                    let fx = (vis.center.x * ai + dx * cs - dy * sn) / ai;
                    let fy = vis.center.y + dx * sn + dy * cs;
                    let mut s = current.clone();
                    if let Some(fr) = s.frame_mut(cid) {
                        fr.focus = Some(FocusPoint { x: fx.clamp(0.0, 1.0), y: fy.clamp(0.0, 1.0), manual: true });
                    }
                    if let Some(mm) = ws.media_mut(m.id) {
                        mm.focus = Some(NVec2::new(fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0)));
                    }
                    ws.commit(s, "Set focus point");
                    ws.msg("Focus point set — Auto Build and template changes keep it in frame.", MsgKind::Info);
                }
                return;
            }
            if resp.drag_started() && r.expand(40.0).contains(p) {
                ws.drag_state = Some(CanvasDrag { kind: DragKind::CropPan { id: cid }, start: p, orig: current.clone(), working: current.clone(), guides: vec![], swap_target: None });
            } else if resp.drag_started() {
                ws.crop_mode = None;
            }
            if resp.double_clicked() && !r.contains(p) {
                ws.crop_mode = None;
            }
        } else {
            ws.crop_mode = None;
        }
    }

    // --- Begin interactions ---
    if resp.drag_started() && ws.drag_state.is_none() {
        if space || resp.dragged_by(PointerButton::Middle) {
            ws.drag_state = Some(CanvasDrag { kind: DragKind::Pan, start: p, orig: current.clone(), working: current.clone(), guides: vec![], swap_target: None });
        } else if ws.tool != Tool::Select {
            ws.drag_state = Some(CanvasDrag { kind: DragKind::Create { tool: ws.tool }, start: p, orig: current.clone(), working: current.clone(), guides: vec![], swap_target: None });
        } else {
            // Handles of the single selection first.
            let mut started = false;
            if ws.sel.len() == 1 {
                if let Some((r, rot, locked)) = item_rect(current, ws.sel[0]) {
                    if !locked {
                        let sr = xf.rect(&surf.rect(&r));
                        let a = mdeg_to_rad(rot);
                        let top = sr.center() + Vec2::new(0.0, -sr.height() / 2.0 - 22.0).rotated(a);
                        if top.distance(p) < 10.0 {
                            ws.drag_state = Some(CanvasDrag { kind: DragKind::Rotate { id: ws.sel[0] }, start: p, orig: current.clone(), working: current.clone(), guides: vec![], swap_target: None });
                            started = true;
                        } else {
                            let hp = if rot == 0 { handle_points(sr).to_vec() } else { draw::rotated_quad(sr.center(), sr.width() / 2.0, sr.height() / 2.0, a).to_vec() };
                            for (i, h) in hp.iter().enumerate() {
                                if h.distance(p) < 9.0 {
                                    let handle = if rot == 0 { i as u8 } else { [0u8, 2, 4, 6][i] };
                                    ws.drag_state = Some(CanvasDrag {
                                        kind: DragKind::Resize { id: ws.sel[0], handle },
                                        start: p,
                                        orig: current.clone(),
                                        working: current.clone(),
                                        guides: vec![],
                                        swap_target: None,
                                    });
                                    started = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if !started {
                let hits = hit_items(current, surf, np);
                if let Some(&top) = hits.first() {
                    if !ws.sel.contains(&top) {
                        if mods.shift {
                            ws.sel.push(top);
                        } else {
                            ws.sel = vec![top];
                        }
                    }
                    let movable: Vec<ItemId> = ws.sel.iter().copied().filter(|id| item_rect(current, *id).map(|r| !r.2).unwrap_or(false)).collect();
                    if !movable.is_empty() {
                        ws.drag_state = Some(CanvasDrag { kind: DragKind::Move { ids: movable }, start: p, orig: current.clone(), working: current.clone(), guides: vec![], swap_target: None });
                    }
                } else {
                    ws.sel.clear();
                    ws.drag_state = Some(CanvasDrag { kind: DragKind::Pan, start: p, orig: current.clone(), working: current.clone(), guides: vec![], swap_target: None });
                }
            }
        }
    }

    // --- Clicks ---
    if resp.clicked() && ws.drag_state.is_none() && ws.crop_mode.is_none() {
        match ws.tool {
            Tool::Text => {
                let mut s = current.clone();
                let pair = pair_for(ws);
                let w = 0.3f32;
                let mut tb = TextBlock::new(NRect::new(np.x - w / 2.0, np.y - 0.04, w, 0.08).clamp_to_unit(), "Your text", StylePreset::H2.style(&pair));
                tb.z = s.max_z() + 1;
                ws.sel = vec![tb.id];
                s.texts.push(tb);
                ws.commit(s, "Add text");
                ws.tool = Tool::Select;
                ws.right_tab = RightTab::Text;
            }
            _ => {
                let hits = hit_items(current, surf, np);
                match hits.first() {
                    Some(&id) => {
                        if mods.shift {
                            if let Some(i) = ws.sel.iter().position(|x| *x == id) {
                                ws.sel.remove(i);
                            } else {
                                ws.sel.push(id);
                            }
                        } else {
                            ws.sel = vec![id];
                        }
                        if current.frames.iter().any(|f| f.id == id) {
                            ws.right_tab = RightTab::Image;
                        } else if current.texts.iter().any(|t| t.id == id) {
                            ws.right_tab = RightTab::Text;
                        }
                    }
                    None => ws.sel.clear(),
                }
            }
        }
    }
    if resp.double_clicked() && ws.crop_mode.is_none() {
        if let Some(&id) = hit_items(current, surf, np).first() {
            if current.frames.iter().any(|f| f.id == id && f.image_id.is_some()) {
                ws.sel = vec![id];
                ws.crop_mode = Some(id);
            } else if current.texts.iter().any(|t| t.id == id) {
                ws.sel = vec![id];
                ws.right_tab = RightTab::Text;
                ws.focus_text_edit = true;
            }
        }
    }
    // Context menu.
    let ctx_target = hit_items(current, surf, np).first().copied();
    resp.context_menu(|ui| {
        let Some(id) = ctx_target.or(ws.sel.first().copied()) else {
            if ui.button(t("Insert blank spread")).clicked() {
                ui.close_menu();
            }
            return;
        };
        if !ws.sel.contains(&id) {
            ws.sel = vec![id];
        }
        context_menu(ui, ws, current, id);
    });

    // --- Dragging ---
    let Some(mut d) = ws.drag_state.take() else { return };
    let delta = p - d.start;
    let tol_x = 6.0 / (surf.w as f32 * xf.s);
    let tol_y = 6.0 / (surf.h as f32 * xf.s);
    let dn = NVec2::new(delta.x / (surf.w as f32 * xf.s), delta.y / (surf.h as f32 * xf.s));
    d.guides.clear();
    d.swap_target = None;
    match d.kind.clone() {
        DragKind::Pan => {
            ws.view.user_moved = true;
            ws.view.pan += resp.drag_delta();
        }
        DragKind::Move { ids } => {
            let mut w = d.orig.clone();
            let (xs, ys) = snap_lines(&d.orig, surf, &ids, is_spread, geom);
            // Snap using the first item's edges.
            let mut off = dn;
            if let Some((r0, _, _)) = item_rect(&d.orig, ids[0]) {
                if !mods.alt {
                    let cand_x = [r0.x + dn.x, r0.right() + dn.x, r0.center().x + dn.x];
                    let mut best: Option<(f32, f32)> = None;
                    for (k, v) in cand_x.iter().enumerate() {
                        if let Some(l) = snap(*v, &xs, tol_x) {
                            let adj = l - v;
                            if best.map(|b| adj.abs() < b.0.abs()).unwrap_or(true) {
                                best = Some((adj, l));
                            }
                            let _ = k;
                        }
                    }
                    if let Some((adj, l)) = best {
                        off.x += adj;
                        d.guides.push((true, l));
                    }
                    let cand_y = [r0.y + dn.y, r0.bottom() + dn.y, r0.center().y + dn.y];
                    let mut best: Option<(f32, f32)> = None;
                    for v in cand_y {
                        if let Some(l) = snap(v, &ys, tol_y) {
                            let adj = l - v;
                            if best.map(|b| adj.abs() < b.0.abs()).unwrap_or(true) {
                                best = Some((adj, l));
                            }
                        }
                    }
                    if let Some((adj, l)) = best {
                        off.y += adj;
                        d.guides.push((false, l));
                    }
                }
            }
            for id in &ids {
                if let Some((r, _, _)) = item_rect(&d.orig, *id) {
                    set_item_rect(&mut w, *id, NRect::new(r.x + off.x, r.y + off.y, r.w, r.h));
                }
            }
            // Cmd/Ctrl-drag a photo onto another frame to swap photos.
            if mods.command && ids.len() == 1 && d.orig.frames.iter().any(|f| f.id == ids[0]) {
                w = d.orig.clone();
                d.swap_target = hit_items(&d.orig, surf, np).into_iter().find(|h| *h != ids[0] && d.orig.frames.iter().any(|f| f.id == *h));
            }
            d.working = w;
        }
        DragKind::Resize { id, handle } => {
            let mut w = d.orig.clone();
            if let Some((r, rot, _)) = item_rect(&d.orig, id) {
                let (mut x0, mut y0, mut x1, mut y1) = (r.x, r.y, r.right(), r.bottom());
                if rot != 0 {
                    // Rotated items resize symmetrically around their centre.
                    let a = -mdeg_to_rad(rot);
                    let local = Vec2::new(dn.x * surf.w as f32, dn.y * surf.h as f32).rotated(a);
                    let sx = if matches!(handle, 2 | 4) { 1.0 } else { -1.0 };
                    let sy = if matches!(handle, 4 | 6) { 1.0 } else { -1.0 };
                    let dw = local.x * sx / surf.w as f32;
                    let dh = local.y * sy / surf.h as f32;
                    let c = r.center();
                    let (nw, nh) = ((r.w + 2.0 * dw).max(0.01), (r.h + 2.0 * dh).max(0.01));
                    set_item_rect(&mut w, id, NRect::new(c.x - nw / 2.0, c.y - nh / 2.0, nw, nh));
                } else {
                    let (xs, ys) = snap_lines(&d.orig, surf, &[id], is_spread, geom);
                    let sx = |v: f32, guides: &mut Vec<(bool, f32)>| match snap(v, &xs, tol_x) {
                        Some(l) if !mods.alt => {
                            guides.push((true, l));
                            l
                        }
                        _ => v,
                    };
                    match handle {
                        0 | 6 | 7 => x0 = sx(r.x + dn.x, &mut d.guides),
                        2..=4 => x1 = sx(r.right() + dn.x, &mut d.guides),
                        _ => {}
                    }
                    let sy = |v: f32, guides: &mut Vec<(bool, f32)>| match snap(v, &ys, tol_y) {
                        Some(l) if !mods.alt => {
                            guides.push((false, l));
                            l
                        }
                        _ => v,
                    };
                    match handle {
                        0..=2 => y0 = sy(r.y + dn.y, &mut d.guides),
                        4..=6 => y1 = sy(r.bottom() + dn.y, &mut d.guides),
                        _ => {}
                    }
                    if mods.shift && matches!(handle, 0 | 2 | 4 | 6) {
                        let ar = r.w / r.h.max(1e-4);
                        let nh = (x1 - x0) / ar;
                        if matches!(handle, 0 | 2) {
                            y0 = y1 - nh;
                        } else {
                            y1 = y0 + nh;
                        }
                    }
                    let (nx0, nx1) = (x0.min(x1 - 0.005), x1.max(x0 + 0.005));
                    let (ny0, ny1) = (y0.min(y1 - 0.005), y1.max(y0 + 0.005));
                    set_item_rect(&mut w, id, NRect::new(nx0, ny0, nx1 - nx0, ny1 - ny0));
                }
            }
            d.working = w;
        }
        DragKind::Rotate { id } => {
            let mut w = d.orig.clone();
            if let Some((r, _, _)) = item_rect(&d.orig, id) {
                let c = xf.rect(&surf.rect(&r)).center();
                let v = p - c;
                let mut deg = v.x.atan2(-v.y).to_degrees();
                if mods.shift {
                    deg = (deg / 15.0).round() * 15.0;
                } else if (deg / 90.0).round() * 90.0 - deg < 2.0 && deg - (deg / 90.0).round() * 90.0 < 2.0 {
                    deg = (deg / 90.0).round() * 90.0;
                }
                set_item_rot(&mut w, id, deg_to_mdeg(deg));
            }
            d.working = w;
        }
        DragKind::CropPan { id } => {
            let mut w = d.orig.clone();
            if let Some(f) = d.orig.frames.iter().find(|f| f.id == id) {
                if let Some(m) = f.image_id.and_then(|i| ws.media(i)) {
                    let r = xf.rect(&surf.print_rect(&f.rect_norm));
                    let ai = m.aspect();
                    let af = r.width() / r.height().max(1.0);
                    let c = crop::clamp(&f.crop, ai, af);
                    let vis = crop::visible(&c, ai, af, FitMode::Fill);
                    let (sn, cs) = (-mdeg_to_rad(c.rotation_mdeg)).sin_cos();
                    let (u, v) = (-delta.x / r.width(), -delta.y / r.height());
                    let (dx, dy) = (u * vis.w * ai, v * vis.h);
                    let mut nc = c;
                    nc.center_norm = NVec2::new(c.center_norm.x + (dx * cs - dy * sn) / ai, c.center_norm.y + dx * sn + dy * cs);
                    if let Some(fr) = w.frame_mut(id) {
                        fr.crop = crop::clamp(&nc, ai, af);
                        fr.crop_manual = true;
                    }
                }
            }
            d.working = w;
        }
        DragKind::Create { tool } => {
            let mut w = d.orig.clone();
            let a = to_norm(d.start);
            let r = NRect::new(a.x.min(np.x), a.y.min(np.y), (a.x - np.x).abs().max(0.01), (a.y - np.y).abs().max(0.01));
            let z = w.max_z() + 1;
            match tool {
                Tool::Frame => {
                    let mut f = Frame::new(r);
                    f.z = z;
                    w.frames.push(f);
                }
                Tool::Text => {
                    let mut tb = TextBlock::new(r, "Your text", StylePreset::Body.style(&pair_for(ws)));
                    tb.z = z;
                    w.texts.push(tb);
                }
                Tool::Rect | Tool::Ellipse | Tool::Line => {
                    let kind = match tool {
                        Tool::Rect => ShapeKind::Rect,
                        Tool::Ellipse => ShapeKind::Ellipse,
                        _ => ShapeKind::Line,
                    };
                    let mut s = OShape::new(kind, r);
                    s.z = if kind == ShapeKind::Line { z } else { w.min_z() - 1 };
                    w.shapes.push(s);
                }
                Tool::Select => {}
            }
            d.working = w;
        }
    }
    if resp.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
        match &d.kind {
            DragKind::Pan => {}
            DragKind::Move { ids } if d.swap_target.is_some() => {
                let (a, b) = (ids[0], d.swap_target.unwrap_or(ids[0]));
                swap_photos(ws, &d.orig, a, b);
            }
            DragKind::Create { tool } => {
                let tool = *tool;
                let created = d.working.frames.len() + d.working.texts.len() + d.working.shapes.len() > d.orig.frames.len() + d.orig.texts.len() + d.orig.shapes.len();
                if created && delta.length() > 4.0 {
                    let new_id = d.working.frames.last().map(|f| f.id).filter(|_| tool == Tool::Frame).or(d.working.texts.last().map(|t| t.id).filter(|_| tool == Tool::Text)).or(d
                        .working
                        .shapes
                        .last()
                        .map(|s| s.id));
                    ws.commit(d.working.clone(), "Add item");
                    if let Some(id) = new_id {
                        ws.sel = vec![id];
                    }
                    ws.tool = Tool::Select;
                }
            }
            kind => {
                if d.working != d.orig {
                    let label = match kind {
                        DragKind::Move { .. } => "Move",
                        DragKind::Resize { .. } => "Resize",
                        DragKind::Rotate { .. } => "Rotate",
                        DragKind::CropPan { .. } => "Pan crop",
                        _ => "Edit",
                    };
                    ws.commit(d.working.clone(), label);
                }
            }
        }
        ws.drag_state = None;
    } else {
        ws.drag_state = Some(d);
    }
}

pub fn pair_for(ws: &Workspace) -> oto_core::model::FontPair {
    let pairs = oto_core::model::font_pairs();
    pairs.iter().find(|p| p.name == ws.project().settings.font_pair).cloned().unwrap_or_else(|| pairs[0].clone())
}

fn swap_photos(ws: &mut Workspace, orig: &Spread, a: ItemId, b: ItemId) {
    let mut s = orig.clone();
    let ia = s.frame(a).and_then(|f| f.image_id);
    let ib = s.frame(b).and_then(|f| f.image_id);
    let ctx = ws.ctx();
    for (id, img) in [(a, ib), (b, ia)] {
        let info = img.and_then(|m| ws.photo_info(m));
        if let Some(f) = s.frame_mut(id) {
            f.image_id = img;
            f.focus = None;
            f.crop_manual = false;
            if let Some(info) = info {
                let fr = f.clone();
                f.crop = oto_core::solver::recrop(&ctx, &fr, &info);
            }
        }
    }
    ws.commit(s, "Swap photos");
}

/// Drop photos from the browser: onto a frame = replace; onto an empty spread = lay out; else new frame.
fn drop_media(ws: &mut Workspace, current: &Spread, surf: &Surface, ids: &[MediaId], at: NVec2) {
    let Some(&first) = ids.first() else { return };
    let mut s = current.clone();
    let ctx = ws.ctx();
    let hit = hit_items(current, surf, at).into_iter().find(|h| current.frames.iter().any(|f| f.id == *h));
    if let Some(fid) = hit {
        if current.frame(fid).map(|f| f.locked).unwrap_or(false) || current.locked {
            ws.msg("That frame or spread is locked.", MsgKind::Warn);
            return;
        }
        let info = ws.photo_info(first);
        if let Some(f) = s.frame_mut(fid) {
            f.image_id = Some(first);
            f.focus = None;
            f.crop_manual = false;
            if let Some(info) = info {
                let fr = f.clone();
                f.crop = if matches!(ws.target, Target::Cover) {
                    let af = (fr.rect_norm.w * surf.w as f32) / (fr.rect_norm.h * surf.h as f32);
                    oto_core::crop::auto_crop(info.aspect, af, info.focus, &info.faces, None).crop
                } else {
                    oto_core::solver::recrop(&ctx, &fr, &info)
                };
            }
        }
        ws.commit(s, "Replace photo");
        return;
    }
    if matches!(ws.target, Target::Spread(_)) && current.frames.is_empty() && ids.len() <= 8 {
        let infos: Vec<_> = ids.iter().filter_map(|id| ws.photo_info(*id)).collect();
        let refs: Vec<&oto_core::solver::PhotoInfo> = infos.iter().collect();
        let mut laid = oto_core::solver::layout_group(&ctx, &ws.templates, &refs, oto_core::solver::Style::Classic, 7);
        laid.id = current.id;
        laid.background = current.background.clone();
        laid.texts = current.texts.clone();
        laid.shapes = current.shapes.clone();
        ws.commit(laid, "Place photos");
        return;
    }
    if matches!(ws.target, Target::Spread(_)) && !current.locked {
        // SmartAlbums-style: adding photos re-designs the spread for the new photo count.
        let mut grown = current.clone();
        for id in ids {
            if grown.frames.iter().any(|f| f.image_id == Some(*id)) {
                continue;
            }
            let mut f = Frame::new(NRect::new(0.0, 0.0, 0.1, 0.1));
            f.image_id = Some(*id);
            grown.frames.push(f);
        }
        ws.sel.clear();
        ws.relayout_best(grown, "Add photo");
        return;
    }
    let mut f = Frame::new(NRect::new(at.x - 0.12, at.y - 0.15, 0.24, 0.3).clamp_to_unit());
    f.image_id = Some(first);
    f.z = s.max_z() + 1;
    if let Some(info) = ws.photo_info(first) {
        let fr = f.clone();
        f.crop = oto_core::solver::recrop(&ctx, &fr, &info);
    }
    ws.sel = vec![f.id];
    s.frames.push(f);
    ws.commit(s, "Add photo");
}

pub fn context_menu(ui: &mut Ui, ws: &mut Workspace, current: &Spread, id: ItemId) {
    let is_frame = current.frames.iter().any(|f| f.id == id);
    if is_frame {
        if ui.button(format!("✂ {}", t("Crop"))).clicked() {
            ws.crop_mode = Some(id);
            ui.close_menu();
        }
        if ui.button(t("Reset crop")).clicked() {
            let mut s = current.clone();
            let ctx = ws.ctx();
            let info = s.frame(id).and_then(|f| f.image_id).and_then(|m| ws.photo_info(m));
            if let (Some(f), Some(info)) = (s.frame_mut(id), info) {
                f.crop_manual = false;
                let fr = f.clone();
                f.crop = oto_core::solver::recrop(&ctx, &fr, &info);
            }
            ws.commit(s, "Reset crop");
            ui.close_menu();
        }
        if ui.button("Remove photo (keep frame)").clicked() {
            let mut s = current.clone();
            if let Some(f) = s.frame_mut(id) {
                f.image_id = None;
            }
            ws.commit(s, "Remove photo");
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button(t("Bring forward")).clicked() {
        reorder(ws, current, id, 1);
        ui.close_menu();
    }
    if ui.button(t("Send backward")).clicked() {
        reorder(ws, current, id, -1);
        ui.close_menu();
    }
    let locked = item_rect(current, id).map(|r| r.2).unwrap_or(false);
    if ui.button(if locked { "Unlock" } else { t("Lock") }).clicked() {
        let mut s = current.clone();
        if let Some(f) = s.frame_mut(id) {
            f.locked = !locked;
        } else if let Some(x) = s.text_mut(id) {
            x.locked = !locked;
        } else if let Some(x) = s.shape_mut(id) {
            x.locked = !locked;
        }
        ws.commit(s, "Lock item");
        ui.close_menu();
    }
    if ui.button(format!("{}  ⌘D", t("Duplicate"))).clicked() {
        duplicate(ws, current);
        ui.close_menu();
    }
    if ui.button(format!("{}  ⌫", t("Delete"))).clicked() {
        delete_selected(ws, current);
        ui.close_menu();
    }
}

pub fn reorder(ws: &mut Workspace, current: &Spread, id: ItemId, dir: i32) {
    let mut s = current.clone();
    let z = if dir > 0 { s.max_z() + 1 } else { s.min_z() - 1 };
    if let Some(f) = s.frame_mut(id) {
        f.z = z;
    } else if let Some(x) = s.text_mut(id) {
        x.z = z;
    } else if let Some(x) = s.shape_mut(id) {
        x.z = z;
    }
    ws.commit(s, "Arrange");
}

pub fn delete_selected(ws: &mut Workspace, current: &Spread) {
    if ws.sel.is_empty() {
        return;
    }
    let mut s = current.clone();
    let mut n = 0;
    for id in ws.sel.clone() {
        if item_rect(current, id).map(|r| r.2).unwrap_or(true) {
            continue;
        }
        if s.remove_item(id) {
            n += 1;
        }
    }
    if n > 0 {
        let removed_photo = current.frames.iter().filter(|f| f.image_id.is_some()).count() > s.frames.iter().filter(|f| f.image_id.is_some()).count();
        let photos_left = s.frames.iter().filter(|f| f.image_id.is_some()).count();
        // Removing a photo re-designs the spread for the photos that are left (empty frames are dropped).
        if removed_photo && photos_left > 0 && matches!(ws.target, Target::Spread(_)) && !s.locked {
            s.frames.retain(|f| f.image_id.is_some());
            ws.relayout_best(s, "Remove photo");
        } else {
            ws.commit(s, "Delete");
        }
        ws.sel.clear();
    }
}

pub fn duplicate(ws: &mut Workspace, current: &Spread) {
    let mut s = current.clone();
    let mut new_sel = Vec::new();
    let off = 0.02;
    for id in ws.sel.clone() {
        if let Some(f) = current.frame(id) {
            let mut n = f.clone();
            n.id = oto_core::model::new_id();
            n.rect_norm = NRect::new(f.rect_norm.x + off, f.rect_norm.y + off, f.rect_norm.w, f.rect_norm.h).clamp_to_unit();
            n.z = s.max_z() + 1;
            new_sel.push(n.id);
            s.frames.push(n);
        } else if let Some(t) = current.texts.iter().find(|t| t.id == id) {
            let mut n = t.clone();
            n.id = oto_core::model::new_id();
            n.rect_norm = NRect::new(t.rect_norm.x + off, t.rect_norm.y + off, t.rect_norm.w, t.rect_norm.h).clamp_to_unit();
            new_sel.push(n.id);
            s.texts.push(n);
        } else if let Some(x) = current.shapes.iter().find(|x| x.id == id) {
            let mut n = x.clone();
            n.id = oto_core::model::new_id();
            n.rect_norm = NRect::new(x.rect_norm.x + off, x.rect_norm.y + off, x.rect_norm.w, x.rect_norm.h).clamp_to_unit();
            new_sel.push(n.id);
            s.shapes.push(n);
        }
    }
    if !new_sel.is_empty() {
        ws.commit(s, "Duplicate");
        ws.sel = new_sel;
    }
}

/// Keyboard handling for the canvas (only when no text field has focus).
pub fn keys(ctx: &egui::Context, ws: &mut Workspace) {
    if ctx.wants_keyboard_input() {
        return;
    }
    let Some(current) = ws.current().cloned() else { return };
    let surf = ws.surface();
    let (del, esc, enter, c_key, arrows, cmd_d, v, r, tk, shift) = ctx.input(|i| {
        let a = [(Key::ArrowLeft, -1.0, 0.0), (Key::ArrowRight, 1.0, 0.0), (Key::ArrowUp, 0.0, -1.0), (Key::ArrowDown, 0.0, 1.0)];
        let mut d = Vec2::ZERO;
        if !i.modifiers.command {
            for (k, x, y) in a {
                if i.key_pressed(k) {
                    d += Vec2::new(x, y);
                }
            }
        }
        (
            i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            i.key_pressed(Key::Escape),
            i.key_pressed(Key::Enter),
            i.key_pressed(Key::C) && i.modifiers.is_none(),
            d,
            i.modifiers.command && i.key_pressed(Key::D),
            i.key_pressed(Key::V) && i.modifiers.is_none(),
            i.key_pressed(Key::R) && i.modifiers.is_none(),
            i.key_pressed(Key::T) && i.modifiers.is_none(),
            i.modifiers.shift,
        )
    });
    if del {
        delete_selected(ws, &current);
    }
    if esc {
        if ws.crop_mode.is_some() {
            ws.crop_mode = None;
        } else if ws.preview.is_some() {
            ws.preview = None;
        } else {
            ws.sel.clear();
            ws.tool = Tool::Select;
        }
    }
    if enter {
        // Return = double-click: enter photo edit (crop) mode for the selected photo; Return again leaves.
        ws.crop_mode = if ws.crop_mode.is_some() { None } else { ws.sel.first().copied().filter(|id| current.frames.iter().any(|f| f.id == *id && f.image_id.is_some())) };
    }
    let (space_up, quick_close) = ctx.input(|i| (i.key_released(Key::Space), i.key_pressed(Key::Escape)));
    if quick_close && ws.quick_look.is_some() {
        ws.toggle_quick_look(None);
    }
    if space_up {
        if !ws.space_panned {
            let id = ws.hover_media.or_else(|| ws.sel.first().and_then(|s| current.frame(*s)).and_then(|f| f.image_id));
            ws.toggle_quick_look(id);
        }
        ws.space_panned = false;
    }
    if c_key {
        ws.crop_mode = if ws.crop_mode.is_some() { None } else { ws.sel.first().copied().filter(|id| current.frames.iter().any(|f| f.id == *id && f.image_id.is_some())) };
    }
    if v {
        ws.tool = Tool::Select;
    }
    if r {
        ws.tool = Tool::Frame;
    }
    if tk {
        ws.tool = Tool::Text;
    }
    if cmd_d {
        duplicate(ws, &current);
    }
    // Arrow keys follow the mode: photo edit mode pans the crop; a selected text/shape is nudged;
    // otherwise Up/Down change the design and Left/Right go to the previous/next spread.
    if arrows != Vec2::ZERO {
        if let Some(fid) = ws.crop_mode {
            let step = if shift { 0.05 } else { 0.01 };
            let mut s = current.clone();
            if let Some(f) = s.frame_mut(fid) {
                f.crop.center_norm.x = (f.crop.center_norm.x + arrows.x * step).clamp(0.0, 1.0);
                f.crop.center_norm.y = (f.crop.center_norm.y + arrows.y * step).clamp(0.0, 1.0);
                f.crop_manual = true;
            }
            ws.commit(s, "Move photo in frame");
            return;
        }
        let free_items = !ws.sel.is_empty() && ws.sel.iter().all(|id| current.frames.iter().all(|f| f.id != *id));
        if !free_items {
            if arrows.y != 0.0 {
                ws.variation(if arrows.y > 0.0 { 1 } else { -1 });
            } else if arrows.x != 0.0 {
                ws.step_spread(if arrows.x > 0.0 { 1 } else { -1 });
            }
            return;
        }
    }
    if arrows != Vec2::ZERO && !ws.sel.is_empty() {
        // 1 screen px (10 with Shift) at the current zoom.
        let px = if shift { 10.0 } else { 1.0 };
        let s_um = ws.view.zoom / 1000.0;
        let dx = arrows.x * px / (surf.w as f32 * s_um);
        let dy = arrows.y * px / (surf.h as f32 * s_um);
        let mut s = current.clone();
        for id in ws.sel.clone() {
            if let Some((r, _, locked)) = item_rect(&current, id) {
                if !locked {
                    set_item_rect(&mut s, id, NRect::new(r.x + dx, r.y + dy, r.w, r.h));
                }
            }
        }
        ws.commit(s, "Nudge");
    }
}

fn continuous(ui: &mut Ui, ws: &mut Workspace, rect: Rect, resp: &egui::Response, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    let g = ws.project().spec.geometry();
    let surf = Surface { w: g.trim_w, h: g.trim_h, bleed: g.bleed };
    let w = (rect.width() - 80.0).max(200.0);
    let s = w / surf.w as f32;
    let h = surf.h as f32 * s;
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        ws.view.pan.y += scroll.y;
    }
    let total = (h + 40.0) * ws.project().spreads.len() as f32;
    ws.view.pan.y = ws.view.pan.y.clamp(-(total - rect.height() + 40.0).max(0.0), 0.0);
    let painter = ui.painter_at(rect);
    let spreads = ws.project().spreads.clone();
    let media: std::collections::HashMap<MediaId, oto_core::media::MediaItem> = ws.media.iter().map(|m| (m.id, m.clone())).collect();
    let lookup = |id: MediaId| media.get(&id).cloned();
    for (i, sp) in spreads.iter().enumerate() {
        let top = rect.top() + 30.0 + ws.view.pan.y + i as f32 * (h + 40.0);
        if top > rect.bottom() || top + h < rect.top() {
            continue;
        }
        let r = Rect::from_min_size(Pos2::new(rect.left() + 40.0, top), Vec2::new(w, h));
        let xf = Xf { origin: r.min, s };
        paint_surface(&painter, ui.ctx(), &mut ws.tex, sp, &surf, xf, &lookup, &PaintOpts { level: Level::Proxy, real_text: true, bleed: false, dim_outside_trim: false });
        painter.text(Pos2::new(r.left(), r.top() - 6.0), egui::Align2::LEFT_BOTTOM, format!("Spread {}", i + 1), egui::FontId::proportional(12.0), pal.muted);
        if resp.clicked() && resp.interact_pointer_pos().map(|p| r.contains(p)).unwrap_or(false) {
            ws.go_to(Target::Spread(sp.id));
            ws.view.continuous = false;
        }
    }
    let _ = Modifiers::NONE;
}
