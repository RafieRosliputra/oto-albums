//! Left rail: book map with every spread, warnings, density, lock state; reorder by drag.

use egui::{Color32, RichText, Sense, Stroke, Ui, Vec2};
use oto_core::document::Command;
use oto_core::model::{MediaId, Spread};
use oto_render::raster::Surface;

use crate::draw;
use crate::i18n::t;
use crate::settings::AppSettings;
use crate::state::{MsgKind, Request, Target, Workspace};
use crate::theme;

pub const DENSE: usize = 6;

pub fn book_map(ui: &mut Ui, ws: &mut Workspace, settings: &mut AppSettings) {
    let pal = theme::palette(settings.theme);
    let p = ws.project();
    let pages = p.page_count();
    let ok = p.spec.page_count_ok(pages);
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("Book map")).font(theme::strong_font(14.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let txt = format!("{} {} · {pages} pp", p.spreads.len(), t("Spreads").to_lowercase());
            if ok {
                ui.label(RichText::new(txt).small().color(pal.muted));
            } else {
                ui.label(RichText::new(format!("⚠ {txt}")).small().color(pal.warn))
                    .on_hover_text(format!("This book takes {}–{} pages in steps of {}.", p.spec.min_pages, p.spec.max_pages, p.spec.page_increment));
            }
        });
    });
    ui.add_space(4.0);
    let width = ui.available_width() - 8.0;
    let g = ws.project().spec.geometry();
    let surf = Surface { w: g.trim_w, h: g.trim_h, bleed: g.bleed };
    let thumb_h = width * surf.h as f32 / surf.w as f32;
    let mut go: Option<Target> = None;
    let mut cmds: Vec<Command> = Vec::new();
    let mut reflow_from: Option<usize> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Cover
        if let Some(cover) = ws.project().cover.clone() {
            let cg = ws.cover_geometry();
            let csurf = Surface { w: cg.trim_w(), h: cg.trim_h(), bleed: cg.bleed };
            let ch = width * csurf.h as f32 / csurf.w as f32;
            let selected = ws.target == Target::Cover;
            let (r, resp) = ui.allocate_exact_size(Vec2::new(width, ch + 18.0), Sense::click());
            let tr = egui::Rect::from_min_size(r.min + Vec2::new(0.0, 16.0), Vec2::new(width, ch));
            ui.painter().text(r.min, egui::Align2::LEFT_TOP, t("Cover"), egui::FontId::proportional(11.0), if selected { pal.accent } else { pal.muted });
            paint(ui, ws, tr, &cover.content, &csurf);
            if selected {
                ui.painter().rect_stroke(tr.expand(2.0), 3.0, Stroke::new(2.0_f32, pal.selection), egui::StrokeKind::Outside);
            }
            if resp.clicked() {
                go = Some(Target::Cover);
            }
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Cover"));
            ui.add_space(6.0);
        }
        let spreads = ws.project().spreads.clone();
        let n = spreads.len();
        for (i, s) in spreads.iter().enumerate() {
            let selected = ws.target == Target::Spread(s.id);
            let (r, resp) = ui.allocate_exact_size(Vec2::new(width, thumb_h + 20.0), Sense::click_and_drag());
            let tr = egui::Rect::from_min_size(r.min + Vec2::new(0.0, 18.0), Vec2::new(width, thumb_h));
            let photos = s.media_ids().len();
            let (crit, warn) = ws.issues_for(s.id);
            let painter = ui.painter().clone();
            painter.text(r.min, egui::Align2::LEFT_TOP, format!("{}", i + 1), theme::strong_font(11.0), if selected { pal.accent } else { pal.text });
            let mut badges = String::new();
            if s.locked {
                badges.push_str("🔒 ");
            }
            if crit > 0 {
                badges.push_str(&format!("✖{crit} "));
            }
            if warn > 0 {
                badges.push_str(&format!("⚠{warn} "));
            }
            badges.push_str(&format!("{photos}▣"));
            let col = if crit > 0 {
                pal.crit
            } else if warn > 0 || photos > DENSE {
                pal.warn
            } else {
                pal.muted
            };
            painter.text(egui::Pos2::new(r.right(), r.top()), egui::Align2::RIGHT_TOP, badges, egui::FontId::proportional(11.0), col);
            paint(ui, ws, tr, s, &surf);
            if photos > DENSE {
                painter.rect_filled(egui::Rect::from_min_size(tr.left_bottom() - Vec2::new(0.0, 3.0), Vec2::new(tr.width() * (photos as f32 / 10.0).min(1.0), 3.0)), 0.0, pal.warn);
            }
            if selected {
                painter.rect_stroke(tr.expand(2.0), 3.0, Stroke::new(2.0_f32, pal.selection), egui::StrokeKind::Outside);
            } else if resp.hovered() {
                painter.rect_stroke(tr.expand(1.0), 3.0, Stroke::new(1.0_f32, pal.border), egui::StrokeKind::Outside);
            }
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Spread {} with {} photos", i + 1, photos)));
            let resp = resp.on_hover_text(if s.explain.is_empty() { format!("Spread {}", i + 1) } else { s.explain.clone() });
            if resp.clicked() {
                go = Some(Target::Spread(s.id));
            }
            // Reorder by drag and drop.
            resp.dnd_set_drag_payload(i);
            if let Some(from) = resp.dnd_release_payload::<usize>() {
                if *from != i {
                    cmds.push(Command::MoveSpread { from: *from, to: i });
                }
            }
            if resp.dnd_hover_payload::<usize>().is_some() {
                painter.line_segment([tr.left_top() - Vec2::new(0.0, 4.0), tr.right_top() - Vec2::new(0.0, 4.0)], Stroke::new(3.0_f32, pal.accent));
            }
            let sid = s.id;
            resp.context_menu(|ui| {
                if ui.button(t("Insert blank spread")).clicked() {
                    cmds.push(Command::InsertSpread { at: i + 1, spread: Spread::blank() });
                    ui.close_menu();
                }
                if ui.button(t("Insert story page")).clicked() {
                    if let Some(sp) = story_spread(ws) {
                        cmds.push(Command::InsertSpread { at: i + 1, spread: sp });
                    }
                    ui.close_menu();
                }
                if ui.button(t("Duplicate spread")).clicked() {
                    let mut c = s.clone();
                    c.id = oto_core::model::new_id();
                    for f in &mut c.frames {
                        f.id = oto_core::model::new_id();
                    }
                    for x in &mut c.texts {
                        x.id = oto_core::model::new_id();
                    }
                    for x in &mut c.shapes {
                        x.id = oto_core::model::new_id();
                    }
                    c.locked = false;
                    cmds.push(Command::InsertSpread { at: i + 1, spread: c });
                    ui.close_menu();
                }
                if ui.button(if s.locked { t("Unlock spread") } else { t("Lock spread") }).clicked() {
                    cmds.push(Command::SetLocked { id: sid, locked: !s.locked });
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(t("Auto Build from here")).clicked() {
                    ws.requests.push(Request::AutoBuild { from: Some(i) });
                    ui.close_menu();
                }
                if ui.button(t("Reflow from here")).on_hover_text("Re-balance the photos of this and later spreads (locked spreads stay).").clicked() {
                    reflow_from = Some(i);
                    ui.close_menu();
                }
                ui.separator();
                if ui.add_enabled(n > 1, egui::Button::new(RichText::new(t("Delete spread")).color(pal.crit))).clicked() {
                    cmds.push(Command::DeleteSpread { id: sid });
                    ui.close_menu();
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
        if ui.add_sized([width, 30.0], egui::Button::new(format!("+ {}", t("Add spread")))).clicked() {
            cmds.push(Command::InsertSpread { at: n, spread: Spread::blank() });
        }
        if !settings.onboarding_hidden {
            ui.add_space(10.0);
            onboarding(ui, ws, settings);
        }
    });
    if let Some(t) = go {
        ws.go_to(t);
    }
    for c in cmds {
        let insert = matches!(c, Command::InsertSpread { .. });
        let at = if let Command::InsertSpread { at, .. } = &c { Some(*at) } else { None };
        if ws.exec(c) && insert {
            if let Some(sp) = at.and_then(|a| ws.project().spreads.get(a)) {
                let id = sp.id;
                ws.go_to(Target::Spread(id));
            }
        }
    }
    if let Some(i) = reflow_from {
        reflow(ws, i);
    }
}

fn paint(ui: &mut Ui, ws: &mut Workspace, rect: egui::Rect, s: &Spread, surf: &Surface) {
    let Workspace { tex, media, media_idx, .. } = ws;
    let lookup = |id: MediaId| media_idx.get(&id).and_then(|&i| media.get(i)).cloned();
    ui.painter().rect_filled(rect, 2.0, Color32::from_gray(90));
    draw::paint_thumb(ui.painter(), ui.ctx(), tex, rect, s, surf, &lookup);
}

pub fn story_spread(ws: &Workspace) -> Option<Spread> {
    let t = ws.templates.iter().find(|t| t.id.starts_with("oto-story") && t.photo_count == 0)?;
    let ctx = ws.ctx();
    let pair = crate::canvas::pair_for(ws);
    let (mut sp, _) = oto_core::solver::apply_template(&ctx, &Spread::blank(), t, &|_| None, &pair);
    sp.id = oto_core::model::new_id();
    Some(sp)
}

/// Re-balance photos from spread `from` onwards into the same number of spreads.
pub fn reflow(ws: &mut Workspace, from: usize) {
    let spreads = ws.project().spreads.clone();
    let tail: Vec<&Spread> = spreads.iter().skip(from).filter(|s| !s.locked && !s.media_ids().is_empty()).collect();
    if tail.is_empty() {
        ws.msg("Nothing to reflow after this spread.", MsgKind::Info);
        return;
    }
    let ids: Vec<MediaId> = tail.iter().flat_map(|s| s.media_ids()).collect();
    let infos: Vec<oto_core::solver::PhotoInfo> = ids.iter().filter_map(|id| ws.photo_info(*id)).collect();
    let k = tail.len();
    let style = oto_core::solver::Style::parse(&ws.project().settings.style_preset);
    let Some(parts) = oto_core::solver::partition(&infos, k, 1, 8, oto_core::solver::Grouping::Time, style) else {
        ws.msg("Could not reflow these photos into the same number of spreads.", MsgKind::Warn);
        return;
    };
    let ctx = ws.ctx();
    let mut new = spreads.clone();
    let slots: Vec<usize> = spreads.iter().enumerate().skip(from).filter(|(_, s)| !s.locked && !s.media_ids().is_empty()).map(|(i, _)| i).collect();
    for (slot, r) in slots.into_iter().zip(parts) {
        let refs: Vec<&oto_core::solver::PhotoInfo> = infos[r].iter().collect();
        let mut sp = oto_core::solver::layout_group(&ctx, &ws.templates, &refs, style, 11 + slot as u64);
        let old = &spreads[slot];
        sp.id = old.id;
        sp.background = old.background.clone();
        sp.texts = old.texts.clone();
        sp.shapes = old.shapes.clone();
        new[slot] = sp;
    }
    ws.exec(Command::SetSpreads { spreads: new, label: "Reflow".into() });
}

fn onboarding(ui: &mut Ui, ws: &Workspace, settings: &mut AppSettings) {
    let pal = theme::palette(settings.theme);
    egui::Frame::new().fill(pal.raised).corner_radius(8).inner_margin(10).stroke(Stroke::new(1.0_f32, pal.border)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("Getting started")).font(theme::strong_font(13.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(t("Hide")).clicked() {
                    settings.onboarding_hidden = true;
                    settings.save();
                }
            });
        });
        let p = ws.project();
        let steps = [
            (t("Choose size"), true),
            (t("Import photos"), !ws.media.is_empty()),
            (t("Auto Build"), p.spreads.iter().any(|s| !s.frames.is_empty())),
            (t("Edit"), ws.edited),
            (t("Export"), ws.exported),
        ];
        for (i, (label, done)) in steps.iter().enumerate() {
            let mark = if *done { "✔" } else { "○" };
            ui.label(RichText::new(format!("{mark}  {}. {label}", i + 1)).color(if *done { pal.ok } else { pal.text }));
        }
    });
}
