//! Bottom panel: timeline (story + cut markers), photo browser, template strip.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use oto_core::document::Command;
use oto_core::media::{ColorLabel, MediaItem};
use oto_core::model::{MediaId, Spread};
use oto_core::solver::{PhotoInfo, Style};
use oto_core::template::TemplateKind;
use oto_media::Level;

use crate::draw;
use crate::i18n::t;
use crate::settings::AppSettings;
use crate::state::{BottomTab, MediaFilter, MediaSort, MsgKind, Request, Target, Workspace};
use crate::theme;

pub const BINS: [&str; 5] = ["Preparation", "Ceremony", "Portraits", "Reception", "Client Picks"];

pub fn bottom(ui: &mut Ui, ws: &mut Workspace, settings: &mut AppSettings) {
    ui.horizontal(|ui| {
        for (tab, label) in [(BottomTab::Timeline, t("Timeline")), (BottomTab::Photos, t("Photos")), (BottomTab::Templates, t("Templates")), (BottomTab::Patterns, t("Patterns"))] {
            let n = match tab {
                BottomTab::Photos => format!(" ({})", ws.media.len()),
                _ => String::new(),
            };
            if ui.selectable_label(ws.bottom_tab == tab, format!("{label}{n}")).clicked() {
                ws.bottom_tab = tab;
            }
        }
        if let Some(job) = &ws.import {
            ui.separator();
            let frac = if job.total == 0 { 0.0 } else { job.done as f32 / job.total as f32 };
            ui.add(egui::ProgressBar::new(frac).desired_width(180.0).text(format!("{} {}/{}", t("Importing"), job.done, job.total)));
            if ui.small_button(t("Cancel")).clicked() {
                job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });
    ui.separator();
    match ws.bottom_tab {
        BottomTab::Timeline => timeline(ui, ws, settings),
        BottomTab::Photos => photos(ui, ws, settings),
        BottomTab::Templates => templates(ui, ws, settings),
        BottomTab::Patterns => patterns(ui, ws),
    }
}

fn thumb(ui: &mut Ui, ws: &mut Workspace, m: &MediaItem, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, 3.0, Color32::from_gray(60));
    if m.missing {
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "⚠", egui::FontId::proportional(18.0), theme::AMBER);
        return;
    }
    if let Some((tid, size)) = ws.tex.get(&m.hash, Level::Thumb) {
        let s = (rect.width() / size.x).min(rect.height() / size.y);
        let r = Rect::from_center_size(rect.center(), size * s);
        p.image(tid, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), if m.rejected { Color32::from_white_alpha(80) } else { Color32::WHITE });
    }
}

// ---------------------------------------------------------------------------------------------
// Photos
// ---------------------------------------------------------------------------------------------

pub fn filtered_media(ws: &Workspace) -> Vec<usize> {
    let usage = ws.usage();
    let q = ws.media_search.to_lowercase();
    let mut v: Vec<usize> = ws
        .media
        .iter()
        .enumerate()
        .filter(|(_, m)| match ws.media_filter {
            MediaFilter::All => true,
            MediaFilter::Used => usage.contains_key(&m.id),
            MediaFilter::Unused => !usage.contains_key(&m.id) && !m.rejected,
            MediaFilter::Missing => m.missing,
            MediaFilter::Rejected => m.rejected,
            MediaFilter::Picks => m.client_pick || m.bins.iter().any(|b| b == "Client Picks"),
        })
        .filter(|(_, m)| q.is_empty() || m.file_name.to_lowercase().contains(&q))
        .filter(|(_, m)| ws.media_bin.as_ref().map(|b| m.bins.contains(b)).unwrap_or(true))
        .map(|(i, _)| i)
        .collect();
    match ws.media_sort {
        MediaSort::Name => v.sort_by(|a, b| ws.media[*a].natural_key().cmp(&ws.media[*b].natural_key())),
        MediaSort::Time => v.sort_by(|a, b| ws.media[*a].capture_time.cmp(&ws.media[*b].capture_time).then(ws.media[*a].natural_key().cmp(&ws.media[*b].natural_key()))),
        MediaSort::Modified => v.sort_by_key(|i| std::cmp::Reverse(ws.media[*i].mtime)),
    }
    v
}

fn photos(ui: &mut Ui, ws: &mut Workspace, settings: &mut AppSettings) {
    let pal = theme::palette(settings.theme);
    let usage = ws.usage();
    ui.horizontal_wrapped(|ui| {
        if ui.button(t("Import folder…")).clicked() {
            ws.requests.push(Request::ImportFolder);
        }
        if ui.button(t("Import files…")).clicked() {
            ws.requests.push(Request::ImportFiles);
        }
        ui.separator();
        let counts = |f: MediaFilter| -> usize {
            ws.media
                .iter()
                .filter(|m| match f {
                    MediaFilter::All => true,
                    MediaFilter::Used => usage.contains_key(&m.id),
                    MediaFilter::Unused => !usage.contains_key(&m.id) && !m.rejected,
                    MediaFilter::Missing => m.missing,
                    MediaFilter::Rejected => m.rejected,
                    MediaFilter::Picks => m.client_pick || m.bins.iter().any(|b| b == "Client Picks"),
                })
                .count()
        };
        for (f, label) in [
            (MediaFilter::All, t("All")),
            (MediaFilter::Used, t("Used")),
            (MediaFilter::Unused, t("Unused")),
            (MediaFilter::Missing, t("Missing")),
            (MediaFilter::Rejected, t("Rejected")),
            (MediaFilter::Picks, t("Client picks")),
        ] {
            let n = counts(f);
            if f == MediaFilter::Missing && n == 0 {
                continue;
            }
            if ui.selectable_label(ws.media_filter == f, format!("{label} {n}")).clicked() {
                ws.media_filter = f;
            }
        }
        ui.separator();
        ui.add(egui::TextEdit::singleline(&mut ws.media_search).hint_text(t("Search")).desired_width(120.0));
        egui::ComboBox::from_id_salt("media-sort")
            .selected_text(format!(
                "{}: {}",
                t("Sort"),
                match ws.media_sort {
                    MediaSort::Name => t("Name"),
                    MediaSort::Time => t("Capture time"),
                    MediaSort::Modified => t("Modified"),
                }
            ))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut ws.media_sort, MediaSort::Name, t("Name"));
                ui.selectable_value(&mut ws.media_sort, MediaSort::Time, t("Capture time"));
                ui.selectable_value(&mut ws.media_sort, MediaSort::Modified, t("Modified"));
            });
        egui::ComboBox::from_id_salt("media-bin").selected_text(format!("{}: {}", t("Bin"), ws.media_bin.clone().unwrap_or_else(|| t("All").into()))).show_ui(ui, |ui| {
            ui.selectable_value(&mut ws.media_bin, None, t("All"));
            for b in BINS {
                ui.selectable_value(&mut ws.media_bin, Some(b.to_string()), b);
            }
        });
        ui.add(egui::Slider::new(&mut ws.thumb_size, 56.0..=200.0).show_value(false)).on_hover_text("Thumbnail size");
        ui.separator();
        let has_sel = !ws.media_sel.is_empty();
        if ui.add_enabled(has_sel, egui::Button::new(t("Add to timeline"))).on_hover_text("Adds the selected photos to the timeline as 'not placed yet'.").clicked() {
            add_to_timeline(ws);
        }
        if ui.button(t("Auto Build…")).clicked() {
            ws.auto_build_source = if has_sel { Some(ws.media_sel.clone()) } else { None };
            ws.requests.push(Request::AutoBuild { from: None });
        }
        if ws.media.iter().any(|m| m.missing) && ui.button(RichText::new(t("Relink…")).color(pal.warn)).clicked() {
            ws.requests.push(Request::Relink);
        }
    });
    ui.add_space(4.0);
    let list = filtered_media(ws);
    if ws.media.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(RichText::new("No photos yet. Drop a folder here or use Import folder…").color(pal.muted));
        });
        return;
    }
    let size = ws.thumb_size;
    let cell = size + 10.0;
    let cols = ((ui.available_width() / cell).floor() as usize).max(1);
    let rows = list.len().div_ceil(cols);
    let mut clicked: Option<(usize, bool, bool)> = None;
    let mut drag_start: Option<usize> = None;
    let mut hovered_idx: Option<usize> = None;
    let mut hover_id = None;
    let mut ctx_action: Option<(usize, MediaAction)> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, cell + 14.0, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                for c in 0..cols {
                    let k = row * cols + c;
                    let Some(&mi) = list.get(k) else { break };
                    let m = ws.media[mi].clone();
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(cell, cell + 12.0), Sense::click_and_drag());
                    let img_r = Rect::from_min_size(r.min + Vec2::new(5.0, 5.0), Vec2::splat(size));
                    thumb(ui, ws, &m, img_r);
                    let selected = ws.media_sel.contains(&m.id);
                    let p = ui.painter();
                    if selected {
                        p.rect_stroke(img_r.expand(3.0), 4.0, Stroke::new(2.0_f32, pal.selection), egui::StrokeKind::Outside);
                    }
                    // Badges (text + shape, never colour alone).
                    let used = usage.get(&m.id).copied().unwrap_or(0);
                    if used > 0 {
                        let b = Rect::from_min_size(img_r.right_top() - Vec2::new(24.0, -2.0), Vec2::new(22.0, 14.0));
                        p.rect_filled(b, 3.0, Color32::from_black_alpha(170));
                        p.text(b.center(), egui::Align2::CENTER_CENTER, format!("✔{used}"), egui::FontId::proportional(9.5), Color32::WHITE);
                    }
                    if m.rejected {
                        p.text(img_r.center(), egui::Align2::CENTER_CENTER, "✖", egui::FontId::proportional(22.0), pal.crit);
                    }
                    if m.client_pick {
                        p.text(img_r.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, "♥", egui::FontId::proportional(13.0), pal.accent);
                    }
                    if m.color_label != ColorLabel::None {
                        let [r_, g_, b_] = m.color_label.rgb();
                        p.circle_filled(img_r.left_bottom() + Vec2::new(7.0, -7.0), 4.5, Color32::from_rgb(r_, g_, b_));
                    }
                    if m.similar_group.is_some() {
                        p.text(img_r.right_bottom() - Vec2::new(4.0, 2.0), egui::Align2::RIGHT_BOTTOM, "≈", egui::FontId::proportional(12.0), Color32::WHITE);
                    }
                    let stars: String = (0..m.rating).map(|_| '★').collect();
                    let label = if stars.is_empty() { trunc(&m.file_name, 16) } else { stars };
                    p.text(Pos2::new(img_r.center().x, img_r.bottom() + 8.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(9.5), if m.rating > 0 { pal.accent } else { pal.muted });
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::ImageButton, true, selected, m.file_name.clone()));
                    let resp = resp.on_hover_text(media_tip(&m, used));
                    if resp.hovered() {
                        hovered_idx = Some(k);
                        hover_id = Some(m.id);
                    }
                    if resp.clicked() {
                        let mods = ui.input(|i| i.modifiers);
                        clicked = Some((k, mods.command, mods.shift));
                    }
                    if resp.double_clicked() {
                        ctx_action = Some((mi, MediaAction::PlaceInSelectedFrame));
                    }
                    if resp.drag_started() {
                        drag_start = Some(k);
                    }
                    resp.context_menu(|ui| {
                        for (a, label) in [
                            (MediaAction::AddToTimeline, t("Add to timeline")),
                            (MediaAction::ToggleReject, if m.rejected { t("Unreject") } else { t("Reject") }),
                            (MediaAction::TogglePick, t("Pick")),
                            (MediaAction::Reveal, t("Show in Finder")),
                        ] {
                            if ui.button(label).clicked() {
                                ctx_action = Some((mi, a));
                                ui.close_menu();
                            }
                        }
                        ui.menu_button(t("Rating"), |ui| {
                            for r in 0..=5u8 {
                                if ui.button(if r == 0 { "—".to_string() } else { "★".repeat(r as usize) }).clicked() {
                                    ctx_action = Some((mi, MediaAction::Rate(r)));
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.menu_button(t("Label"), |ui| {
                            for l in ColorLabel::ALL {
                                if ui.button(l.key()).clicked() {
                                    ctx_action = Some((mi, MediaAction::Label(l)));
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.menu_button(t("Add to bin"), |ui| {
                            for b in BINS {
                                let has = m.bins.iter().any(|x| x == b);
                                if ui.button(format!("{}{b}", if has { "✔ " } else { "" })).clicked() {
                                    ctx_action = Some((mi, MediaAction::ToggleBin(b)));
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                }
            });
        }
    });
    // Selection.
    if let Some((k, toggle, range)) = clicked {
        let id = ws.media[list[k]].id;
        if range {
            let a = ws.media_anchor.unwrap_or(k);
            let (lo, hi) = (a.min(k), a.max(k));
            ws.media_sel = list[lo..=hi.min(list.len() - 1)].iter().map(|i| ws.media[*i].id).collect();
        } else if toggle {
            if let Some(p) = ws.media_sel.iter().position(|x| *x == id) {
                ws.media_sel.remove(p);
            } else {
                ws.media_sel.push(id);
            }
            ws.media_anchor = Some(k);
        } else {
            ws.media_sel = vec![id];
            ws.media_anchor = Some(k);
        }
    }
    if let Some(k) = drag_start {
        let id = ws.media[list[k]].id;
        let ids = if ws.media_sel.contains(&id) { ws.media_sel.clone() } else { vec![id] };
        ws.drag_media = Some(ids);
    }
    if hover_id.is_some() {
        ws.hover_media = hover_id;
    }
    // Keyboard rating while hovering the grid.
    if let Some(k) = hovered_idx {
        if !ui.ctx().wants_keyboard_input() {
            let keys = ui.input(|i| {
                let mut r = None;
                for (n, key) in [egui::Key::Num0, egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5].iter().enumerate() {
                    if i.key_pressed(*key) && i.modifiers.is_none() {
                        r = Some(n as u8);
                    }
                }
                (r, i.key_pressed(egui::Key::X) && i.modifiers.is_none(), i.key_pressed(egui::Key::P) && i.modifiers.is_none())
            });
            let mi = list[k];
            let targets: Vec<MediaId> = if ws.media_sel.contains(&ws.media[mi].id) { ws.media_sel.clone() } else { vec![ws.media[mi].id] };
            if let Some(r) = keys.0 {
                for id in &targets {
                    if let Some(m) = ws.media_mut(*id) {
                        m.rating = r;
                    }
                }
            }
            if keys.1 {
                for id in &targets {
                    if let Some(m) = ws.media_mut(*id) {
                        m.rejected = !m.rejected;
                    }
                }
            }
            if keys.2 {
                for id in &targets {
                    if let Some(m) = ws.media_mut(*id) {
                        m.client_pick = !m.client_pick;
                    }
                }
            }
        }
    }
    if let Some((mi, a)) = ctx_action {
        media_action(ws, mi, a);
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

fn media_tip(m: &MediaItem, used: u32) -> String {
    let mut s = format!("{}\n{} × {} px · {:.1} MP", m.file_name, m.width, m.height, m.megapixels());
    if let Some(t) = &m.capture_time {
        s.push_str(&format!("\n{}", t.replace('T', " ")));
    }
    if let Some(c) = &m.camera {
        s.push_str(&format!("\n{c}"));
    }
    s.push_str(&format!("\nUsed {used}×"));
    if m.missing {
        s.push_str("\n⚠ Original missing — use Relink");
    }
    s.push_str("\n0–5 rate · X reject · P pick · drag onto the canvas");
    s
}

#[derive(Debug, Clone, Copy)]
enum MediaAction {
    AddToTimeline,
    ToggleReject,
    TogglePick,
    Reveal,
    Rate(u8),
    Label(ColorLabel),
    ToggleBin(&'static str),
    PlaceInSelectedFrame,
}

fn media_action(ws: &mut Workspace, mi: usize, a: MediaAction) {
    let id = ws.media[mi].id;
    let targets: Vec<MediaId> = if ws.media_sel.contains(&id) { ws.media_sel.clone() } else { vec![id] };
    match a {
        MediaAction::AddToTimeline => {
            ws.media_sel = targets;
            add_to_timeline(ws);
        }
        MediaAction::Reveal => {
            let p = std::path::PathBuf::from(&ws.media[mi].path);
            if let Err(e) = oto_platform::native().reveal(&p) {
                ws.msg(e.to_string(), MsgKind::Warn);
            }
        }
        MediaAction::PlaceInSelectedFrame => {
            let Some(cur) = ws.current().cloned() else { return };
            let Some(fid) = ws.sel.first().copied().filter(|f| cur.frames.iter().any(|x| x.id == *f)) else {
                ws.msg("Select a frame on the canvas, then double-click a photo to place it.", MsgKind::Info);
                return;
            };
            let mut s = cur.clone();
            let ctx = ws.ctx();
            let info = ws.photo_info(id);
            if let Some(f) = s.frame_mut(fid) {
                f.image_id = Some(id);
                f.crop_manual = false;
                if let Some(info) = info {
                    let fr = f.clone();
                    f.crop = oto_core::solver::recrop(&ctx, &fr, &info);
                }
            }
            ws.commit(s, "Replace photo");
        }
        other => {
            for tid in targets {
                if let Some(m) = ws.media_mut(tid) {
                    match other {
                        MediaAction::ToggleReject => m.rejected = !m.rejected,
                        MediaAction::TogglePick => m.client_pick = !m.client_pick,
                        MediaAction::Rate(r) => m.rating = r,
                        MediaAction::Label(l) => m.color_label = l,
                        MediaAction::ToggleBin(b) => {
                            if let Some(p) = m.bins.iter().position(|x| x == b) {
                                m.bins.remove(p);
                            } else {
                                m.bins.push(b.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

pub fn add_to_timeline(ws: &mut Workspace) {
    let usage = ws.usage();
    let mut staged = ws.project().timeline.staged.clone();
    let mut added = 0;
    let mut skipped = 0;
    let ids = ws.media_sel.clone();
    for id in ids {
        let rejected = ws.media(id).map(|m| m.rejected).unwrap_or(true);
        if usage.contains_key(&id) || staged.contains(&id) || rejected {
            skipped += 1;
            continue;
        }
        staged.push(id);
        added += 1;
    }
    // Keep story order by capture time, then name.
    staged.sort_by(|a, b| {
        let (ma, mb) = (ws.media(*a), ws.media(*b));
        ma.and_then(|m| m.capture_time.clone()).cmp(&mb.and_then(|m| m.capture_time.clone())).then(ma.map(|m| m.natural_key()).cmp(&mb.map(|m| m.natural_key())))
    });
    ws.exec(Command::SetStaged { staged });
    ws.msg(format!("{added} photo(s) added to the timeline{}.", if skipped > 0 { format!(" ({skipped} already used, staged or rejected)") } else { String::new() }), MsgKind::Ok);
    ws.bottom_tab = BottomTab::Timeline;
}

// ---------------------------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum TlAction {
    Split { spread: usize, at: usize },
    Merge { spread: usize },
    Move { photo: MediaId, to_spread: usize, at: usize },
    PlaceStaged,
    Unstage(MediaId),
}

fn timeline(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    let spreads = ws.project().spreads.clone();
    let staged = ws.project().timeline.staged.clone();
    let size = 64.0f32;
    let mut action: Option<TlAction> = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Click between two photos to cut a spread, click a cut marker to merge. Drag photos to move them between spreads.").small().color(pal.muted));
    });
    egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            for (si, s) in spreads.iter().enumerate() {
                let ids = s.media_ids();
                if ids.is_empty() {
                    continue;
                }
                // Story order inside a spread: left→right by frame position.
                let mut frames: Vec<_> = s.frames.iter().filter(|f| f.image_id.is_some()).collect();
                frames.sort_by(|a, b| a.rect_norm.x.total_cmp(&b.rect_norm.x).then(a.rect_norm.y.total_cmp(&b.rect_norm.y)));
                let ordered: Vec<MediaId> = frames.iter().filter_map(|f| f.image_id).collect();
                let dense = ordered.len() > crate::bookmap::DENSE;
                let current = ws.target == Target::Spread(s.id);
                let bg = if current {
                    pal.accent.gamma_multiply(0.18)
                } else if dense {
                    pal.warn.gamma_multiply(0.15)
                } else {
                    pal.raised
                };
                let seg_w = ordered.len() as f32 * (size + 6.0) + 6.0;
                let (seg, seg_resp) = ui.allocate_exact_size(Vec2::new(seg_w, size + 30.0), Sense::click());
                ui.painter().rect_filled(seg, 6.0, bg);
                let head = format!("{}{}{}", si + 1, if s.locked { " 🔒" } else { "" }, if dense { format!(" ⚠ {}", t("Too many photos on this spread")) } else { String::new() });
                ui.painter().text(seg.left_top() + Vec2::new(6.0, 3.0), egui::Align2::LEFT_TOP, head, egui::FontId::proportional(10.5), if dense { pal.warn } else { pal.muted });
                if seg_resp.clicked() {
                    ws.go_to(Target::Spread(s.id));
                }
                for (k, id) in ordered.iter().enumerate() {
                    let r = Rect::from_min_size(seg.min + Vec2::new(6.0 + k as f32 * (size + 6.0), 22.0), Vec2::splat(size));
                    let resp = ui.interact(r, ui.id().with(("tl", si, k)), Sense::click_and_drag());
                    if let Some(m) = ws.media(*id).cloned() {
                        thumb(ui, ws, &m, r);
                        let resp = resp.on_hover_text(&m.file_name);
                        resp.dnd_set_drag_payload(*id);
                        if resp.drag_started() {
                            ws.drag_media = Some(vec![*id]);
                        }
                        if resp.clicked() {
                            ws.go_to(Target::Spread(s.id));
                            ws.media_sel = vec![*id];
                        }
                    }
                    // Gap after this photo (inside the segment): click = cut here; drop = move here.
                    if k + 1 < ordered.len() {
                        let gap = Rect::from_min_size(Pos2::new(r.right(), r.top()), Vec2::new(6.0, size));
                        let g = ui.interact(gap.expand2(Vec2::new(2.0, 0.0)), ui.id().with(("gap", si, k)), Sense::click());
                        if g.hovered() {
                            ui.painter().line_segment([gap.center_top(), gap.center_bottom()], Stroke::new(2.0_f32, pal.accent));
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if g.clicked() && !s.locked {
                            action = Some(TlAction::Split { spread: si, at: k + 1 });
                        }
                        if let Some(pid) = g.dnd_release_payload::<MediaId>() {
                            action = Some(TlAction::Move { photo: *pid, to_spread: si, at: k + 1 });
                        }
                    }
                }
                // Drop at the end of the segment.
                if let Some(pid) = seg_resp.dnd_release_payload::<MediaId>() {
                    action = Some(TlAction::Move { photo: *pid, to_spread: si, at: ordered.len() });
                }
                // Cut marker between segments (click = merge with the next spread).
                let (m, mr) = ui.allocate_exact_size(Vec2::new(10.0, size + 30.0), Sense::click());
                ui.painter().rect_filled(Rect::from_center_size(m.center(), Vec2::new(3.0, size + 20.0)), 1.0, if mr.hovered() { pal.accent } else { pal.border });
                if mr.on_hover_text("Cut marker — click to merge with the next spread").clicked() {
                    action = Some(TlAction::Merge { spread: si });
                }
            }
            if !staged.is_empty() {
                ui.separator();
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{} ({})", t("Not placed yet"), staged.len())).small().color(pal.accent));
                    ui.horizontal(|ui| {
                        if ui.small_button(t("Place as new spreads")).clicked() {
                            action = Some(TlAction::PlaceStaged);
                        }
                        if ui.small_button(t("Auto Build…")).clicked() {
                            ws.auto_build_source = Some(staged.clone());
                            ws.requests.push(Request::AutoBuild { from: None });
                        }
                    });
                });
                for id in &staged {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
                    if let Some(m) = ws.media(*id).cloned() {
                        thumb(ui, ws, &m, r);
                        ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0_f32, pal.accent), egui::StrokeKind::Inside);
                        resp.dnd_set_drag_payload(*id);
                        if resp.drag_started() {
                            ws.drag_media = Some(vec![*id]);
                        }
                        let id2 = *id;
                        resp.on_hover_text(&m.file_name).context_menu(|ui| {
                            if ui.button("Remove from timeline").clicked() {
                                action = Some(TlAction::Unstage(id2));
                                ui.close_menu();
                            }
                        });
                    }
                }
            }
        });
    });
    if let Some(a) = action {
        timeline_action(ws, a);
    }
}

fn relayout(ws: &Workspace, old: &Spread, ids: &[MediaId], seed: u64) -> Spread {
    let infos: Vec<PhotoInfo> = ids.iter().filter_map(|id| ws.photo_info(*id)).collect();
    let refs: Vec<&PhotoInfo> = infos.iter().collect();
    let style = Style::parse(&ws.project().settings.style_preset);
    let mut sp = if refs.is_empty() { Spread::blank() } else { oto_core::solver::layout_group(&ws.ctx(), &ws.templates, &refs, style, seed) };
    sp.id = old.id;
    sp.background = old.background.clone();
    sp.texts = old.texts.clone();
    sp.shapes = old.shapes.clone();
    sp.notes = old.notes.clone();
    sp
}

fn ordered_ids(s: &Spread) -> Vec<MediaId> {
    let mut frames: Vec<_> = s.frames.iter().filter(|f| f.image_id.is_some()).collect();
    frames.sort_by(|a, b| a.rect_norm.x.total_cmp(&b.rect_norm.x).then(a.rect_norm.y.total_cmp(&b.rect_norm.y)));
    frames.iter().filter_map(|f| f.image_id).collect()
}

fn similar_clash(ws: &Workspace, ids: &[MediaId]) -> bool {
    let mut seen = std::collections::HashSet::new();
    ids.iter().filter_map(|id| ws.media(*id).and_then(|m| m.similar_group)).any(|g| !seen.insert(g))
}

fn timeline_action(ws: &mut Workspace, a: TlAction) {
    let mut spreads = ws.project().spreads.clone();
    match a {
        TlAction::Split { spread, at } => {
            let ids = ordered_ids(&spreads[spread]);
            let (l, r) = ids.split_at(at.min(ids.len()));
            let first = relayout(ws, &spreads[spread], l, 3);
            let mut second = relayout(ws, &Spread::blank(), r, 5);
            second.texts.clear();
            spreads[spread] = first;
            spreads.insert(spread + 1, second);
            ws.exec(Command::SetSpreads { spreads, label: "Cut timeline".into() });
        }
        TlAction::Merge { spread } => {
            let Some(next) = (spread + 1..spreads.len()).find(|i| !spreads[*i].media_ids().is_empty()) else { return };
            if spreads[spread].locked || spreads[next].locked {
                ws.msg("A locked spread cannot be merged.", MsgKind::Warn);
                return;
            }
            let mut ids = ordered_ids(&spreads[spread]);
            ids.extend(ordered_ids(&spreads[next]));
            if ids.len() > 8 {
                ws.msg("Merging would put more than 8 photos on one spread.", MsgKind::Warn);
                return;
            }
            if similar_clash(ws, &ids) {
                ws.msg("Note: near-identical photos now share a spread (duplicate guard).", MsgKind::Warn);
            }
            let merged = relayout(ws, &spreads[spread], &ids, 9);
            spreads[spread] = merged;
            spreads.remove(next);
            ws.exec(Command::SetSpreads { spreads, label: "Merge spreads".into() });
        }
        TlAction::Move { photo, to_spread, at } => {
            let from = spreads.iter().position(|s| s.media_ids().contains(&photo));
            let mut staged = ws.project().timeline.staged.clone();
            if let Some(f) = from {
                if spreads[f].locked || spreads[to_spread].locked {
                    ws.msg("A locked spread cannot change.", MsgKind::Warn);
                    return;
                }
                let mut src = ordered_ids(&spreads[f]);
                src.retain(|x| *x != photo);
                if f == to_spread {
                    src.insert(at.min(src.len()), photo);
                    spreads[f] = relayout(ws, &spreads[f], &src, 13);
                } else {
                    let mut dst = ordered_ids(&spreads[to_spread]);
                    dst.insert(at.min(dst.len()), photo);
                    if dst.len() > 8 {
                        ws.msg("A spread holds at most 8 photos.", MsgKind::Warn);
                        return;
                    }
                    spreads[f] = relayout(ws, &spreads[f], &src, 13);
                    spreads[to_spread] = relayout(ws, &spreads[to_spread], &dst, 17);
                }
            } else if staged.contains(&photo) {
                let mut dst = ordered_ids(&spreads[to_spread]);
                dst.insert(at.min(dst.len()), photo);
                if dst.len() > 8 || spreads[to_spread].locked {
                    ws.msg("That spread is full or locked.", MsgKind::Warn);
                    return;
                }
                spreads[to_spread] = relayout(ws, &spreads[to_spread], &dst, 17);
                staged.retain(|x| *x != photo);
                ws.exec(Command::SetStaged { staged });
            }
            ws.exec(Command::SetSpreads { spreads, label: "Move photo".into() });
        }
        TlAction::PlaceStaged => {
            let staged = ws.project().timeline.staged.clone();
            let infos: Vec<PhotoInfo> = staged.iter().filter_map(|id| ws.photo_info(*id)).collect();
            let k = infos.len().div_ceil(3).max(1);
            let style = Style::parse(&ws.project().settings.style_preset);
            let parts = oto_core::solver::partition(&infos, k, 1, 5, oto_core::solver::Grouping::Time, style).unwrap_or_else(|| std::iter::once(0..infos.len()).collect());
            // Fill empty spreads first, then append.
            let mut empties: Vec<usize> = spreads.iter().enumerate().filter(|(_, s)| s.frames.is_empty() && s.texts.is_empty() && !s.locked).map(|(i, _)| i).collect();
            empties.reverse();
            for (n, r) in parts.into_iter().enumerate() {
                let refs: Vec<&PhotoInfo> = infos[r].iter().collect();
                let mut sp = oto_core::solver::layout_group(&ws.ctx(), &ws.templates, &refs, style, 21 + n as u64);
                match empties.pop() {
                    Some(i) => {
                        sp.id = spreads[i].id;
                        sp.background = spreads[i].background.clone();
                        spreads[i] = sp;
                    }
                    None => spreads.push(sp),
                }
            }
            ws.exec(Command::SetSpreads { spreads, label: "Place timeline photos".into() });
            ws.exec(Command::SetStaged { staged: vec![] });
        }
        TlAction::Unstage(id) => {
            let mut staged = ws.project().timeline.staged.clone();
            staged.retain(|x| *x != id);
            ws.exec(Command::SetStaged { staged });
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------------------------

/// Candidate templates for the current surface (same photo count first).
pub fn candidates(ws: &Workspace) -> Vec<usize> {
    let Some(cur) = ws.current() else { return vec![] };
    let n = cur.media_ids().len() as i64;
    let cover = matches!(ws.target, Target::Cover);
    let mut v: Vec<(i64, usize)> = ws
        .templates
        .iter()
        .enumerate()
        .filter(|(_, t)| (t.kind == TemplateKind::Cover) == cover)
        .map(|(i, t)| ((t.photo_count as i64 - n).abs(), i))
        .filter(|(d, _)| cover || *d <= 1 || n == 0)
        .collect();
    v.sort_by_key(|(d, i)| (*d, ws.templates[*i].family.clone(), *i));
    v.into_iter().map(|(_, i)| i).collect()
}

/// Spread as it would look with template `ti` (cached; never touches the project).
pub fn preview_for(ws: &mut Workspace, ti: usize) -> Option<Spread> {
    let cur = ws.current()?.clone();
    let key = (ws.doc.revision, Some(cur.id), matches!(ws.target, Target::Cover));
    if ws.tpl_cache_key != key {
        ws.tpl_cache.clear();
        ws.tpl_cache_key = key;
    }
    let t = ws.templates.get(ti)?.clone();
    if let Some(s) = ws.tpl_cache.get(&t.id) {
        return Some(s.clone());
    }
    let pair = crate::canvas::pair_for(ws);
    let sp = if matches!(ws.target, Target::Cover) {
        let cg = ws.cover_geometry();
        let mut ids: Vec<MediaId> = cur.media_ids();
        if ids.is_empty() {
            ids = ws.media_sel.clone();
        }
        let infos: Vec<PhotoInfo> = ids.iter().filter_map(|id| ws.photo_info(*id)).collect();
        let refs: Vec<&PhotoInfo> = infos.iter().collect();
        let mut sp = oto_core::solver::cover_from_template(&t, &cg, &refs, &pair, Some(&cur));
        if cur.texts.is_empty() {
            oto_core::solver::personalize_cover_texts(&mut sp, &t, &ws.project().name);
        }
        sp
    } else {
        let ctx = ws.ctx();
        let lookup = |id: MediaId| ws.photo_info(id);
        oto_core::solver::apply_template(&ctx, &cur, &t, &lookup, &pair).0
    };
    ws.tpl_cache.insert(t.id.clone(), sp.clone());
    Some(sp)
}

pub fn apply_template_idx(ws: &mut Workspace, ti: usize) {
    let Some(cur) = ws.current().cloned() else { return };
    if cur.locked {
        ws.msg("This spread is locked.", MsgKind::Warn);
        return;
    }
    let Some(sp) = preview_for(ws, ti) else { return };
    let dropped: Vec<MediaId> = cur.media_ids().into_iter().filter(|id| !sp.media_ids().contains(id)).collect();
    let name = ws.templates[ti].name.clone();
    ws.commit(sp, &format!("Template: {name}"));
    if !dropped.is_empty() {
        let mut staged = ws.project().timeline.staged.clone();
        staged.extend(dropped.iter().copied());
        ws.exec(Command::SetStaged { staged });
        ws.msg(format!("{} photo(s) did not fit and went back to the timeline.", dropped.len()), MsgKind::Info);
    }
    ws.preview = None;
}

fn templates(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    let cands = candidates(ws);
    if cands.is_empty() {
        ui.label(RichText::new("No templates for this spread.").color(pal.muted));
        return;
    }
    let cur_tid = ws.current().map(|s| s.template_id.clone()).unwrap_or_default();
    let n = ws.current().map(|s| s.media_ids().len()).unwrap_or(0);
    ui.label(RichText::new(format!("{} templates for {n} photo(s) · hover to preview · Space to compare · A/Q and S/W to step", cands.len())).small().color(pal.muted));
    let h = (ui.available_height() - 24.0).clamp(60.0, 140.0);
    let surf = ws.surface();
    let w = h * surf.w as f32 / surf.h as f32;
    let mut hovered_any = None;
    let mut clicked = None;
    egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            for &ti in &cands {
                let t = ws.templates[ti].clone();
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, h + 18.0), Sense::click());
                let tr = Rect::from_min_size(r.min, Vec2::new(w, h));
                if let Some(sp) = preview_for(ws, ti) {
                    let Workspace { tex, media, media_idx, .. } = ws;
                    let lookup = |id: MediaId| media_idx.get(&id).and_then(|&i| media.get(i)).cloned();
                    draw::paint_thumb(ui.painter(), ui.ctx(), tex, tr, &sp, &surf, &lookup);
                }
                let active = t.id == cur_tid;
                let hl = if active {
                    pal.accent
                } else if resp.hovered() {
                    pal.text
                } else {
                    pal.border
                };
                ui.painter().rect_stroke(tr, 2.0, Stroke::new(if active || resp.hovered() { 2.0_f32 } else { 1.0_f32 }, hl), egui::StrokeKind::Outside);
                let diff = t.photo_count as i64 - n as i64;
                let label = if diff == 0 { t.name.clone() } else { format!("{} ({:+})", t.name, diff) };
                ui.painter().text(Pos2::new(tr.left(), tr.bottom() + 3.0), egui::Align2::LEFT_TOP, trunc(&label, 26), egui::FontId::proportional(10.0), if active { pal.accent } else { pal.muted });
                let resp = resp.on_hover_text(format!("{} · {} · {} photos · {}", t.name, t.family, t.photo_count, t.gutter_policy.key()));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Template {}", t.name)));
                if resp.hovered() {
                    hovered_any = Some(ti);
                }
                if resp.clicked() {
                    clicked = Some(ti);
                }
            }
        });
    });
    if let Some(ti) = clicked {
        apply_template_idx(ws, ti);
    } else if let Some(ti) = hovered_any {
        ws.preview = preview_for(ws, ti);
        if ui.input(|i| i.key_pressed(egui::Key::Space)) {
            ws.view.compare = !ws.view.compare;
        }
    } else if !ws.view.compare {
        ws.preview = None;
    }
}

/// Pattern swatches: the formal collection first. Click = apply to the current spread; drag onto the spread.
fn patterns(ui: &mut Ui, ws: &mut Workspace) {
    use oto_core::model::{Pattern, PatternKind};
    ui.label(egui::RichText::new(t("Click a pattern to use it as this spread's background, or drag it onto the spread. Opacity, size and colour are in the inspector.")).small().weak());
    let kinds: Vec<PatternKind> = PatternKind::FORMAL.into_iter().chain(PatternKind::ALL.into_iter().filter(|k| !PatternKind::FORMAL.contains(k))).collect();
    let mut apply = None;
    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.horizontal(|ui| {
            for k in kinds {
                ui.vertical(|ui| {
                    let size = 76.0;
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
                    let mut p = Pattern::new(k);
                    p.opacity = 1.0;
                    p.color = oto_core::model::Rgba::rgb(0x9C, 0x8D, 0x78);
                    let ppm = 3.0;
                    let tex = ws.tex.pattern(ui.ctx(), &p, ppm);
                    let reps = size / (p.scale_mm * ppm).max(1.0);
                    let painter = ui.painter();
                    painter.rect_filled(r, 4.0, Color32::WHITE);
                    painter.image(tex, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(reps, reps)), Color32::WHITE);
                    let hl = if resp.hovered() { ui.visuals().strong_text_color() } else { Color32::from_gray(120) };
                    painter.rect_stroke(r, 4.0, Stroke::new(1.0_f32, hl), egui::StrokeKind::Outside);
                    ui.label(egui::RichText::new(k.label()).small());
                    if resp.clicked() {
                        apply = Some(k);
                    }
                    if resp.drag_started() {
                        ws.drag_pattern = Some(k);
                    }
                });
            }
        });
    });
    if let Some(k) = apply {
        ws.set_pattern(k);
    }
}
