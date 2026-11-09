//! Right panel: Spread / Image / Text / Layers.

use egui::{Color32, RichText, Ui};
use oto_core::crop;
use oto_core::document::Command;
use oto_core::model::{Align, BackgroundImage, FitMode, Pattern, PatternKind, Rgba, ShapeKind, Spread, StylePreset, ALBUM_FONTS};
use oto_core::units::{deg_to_mdeg, um_to_mm};
use oto_print::preflight::{DPI_BLOCK, DPI_CRITICAL, DPI_WARN};

use crate::canvas;
use crate::i18n::t;
use crate::settings::AppSettings;
use crate::state::{MsgKind, Request, RightTab, Target, Workspace};
use crate::theme;

fn rgba_edit(ui: &mut Ui, c: &mut Rgba) -> bool {
    let mut arr = [c.r, c.g, c.b];
    let ch = ui.color_edit_button_srgb(&mut arr).changed();
    if ch {
        c.r = arr[0];
        c.g = arr[1];
        c.b = arr[2];
    }
    ch
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).font(theme::strong_font(12.5)));
}

pub fn inspector(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    ui.horizontal(|ui| {
        for (tab, label) in
            [(RightTab::Spread, if matches!(ws.target, Target::Cover) { t("Cover") } else { t("Spread") }), (RightTab::Image, t("Image")), (RightTab::Text, t("Text")), (RightTab::Layers, t("Layers"))]
        {
            if ui.selectable_label(ws.right_tab == tab, label).clicked() {
                ws.right_tab = tab;
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match ws.right_tab {
        RightTab::Spread => spread_tab(ui, ws, settings),
        RightTab::Image => image_tab(ui, ws, settings),
        RightTab::Text => text_tab(ui, ws, settings),
        RightTab::Layers => layers_tab(ui, ws, settings),
    });
}

fn spread_tab(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    let pal = theme::panel_palette(settings.theme);
    let Some(cur) = ws.current().cloned() else { return };
    let mut s = cur.clone();
    let tpl = ws.template(&s.template_id).map(|t| format!("{} · {}", t.name, t.family)).unwrap_or_else(|| s.template_id.clone());
    ui.label(RichText::new(tpl).color(pal.muted));
    if !s.explain.is_empty() {
        ui.label(RichText::new(format!("ℹ {}", s.explain)).small().color(pal.muted));
    }
    if matches!(ws.target, Target::Spread(_)) {
        let mut locked = s.locked;
        if ui.checkbox(&mut locked, format!("🔒 {}", t("Lock spread"))).on_hover_text("Locked spreads are kept unchanged by Auto Build, reflow and edits.").changed() {
            ws.exec(Command::SetLocked { id: s.id, locked });
            return;
        }
    } else {
        let g = ws.cover_geometry();
        ui.label(format!("Spine {:.1} mm · wrap {:.0} mm · hinge {:.0} mm", um_to_mm(g.spine_w), um_to_mm(g.wrap), um_to_mm(g.hinge)));
        ui.label(RichText::new("Spine width follows the page count and paper caliper of the lab profile.").small().color(pal.muted));
    }
    section(ui, t("Background"));
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(t("Color"));
        changed |= rgba_edit(ui, &mut s.background.color);
        for (name, c) in [("Paper", Rgba::PAPER), ("White", Rgba::WHITE), ("Charcoal", Rgba::CHARCOAL)] {
            if ui.small_button(name).clicked() {
                s.background.color = c;
                changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        let sel = ws.media_sel.first().copied();
        if ui.add_enabled(sel.is_some(), egui::Button::new("Use selected photo")).on_hover_text("Select a photo in the Photos tab first.").clicked() {
            if let Some(id) = sel {
                s.background.image = Some(BackgroundImage { media_id: id, crop: Default::default(), opacity: 1.0 });
                changed = true;
            }
        }
        if s.background.image.is_some() && ui.button("Remove image").clicked() {
            s.background.image = None;
            changed = true;
        }
    });
    if let Some(bi) = &mut s.background.image {
        changed |= ui.add(egui::Slider::new(&mut bi.opacity, 0.05..=1.0).text(t("Opacity"))).changed();
    }
    section(ui, t("Pattern"));
    let mut kind = s.background.pattern.map(|p| p.kind);
    egui::ComboBox::from_id_salt("pattern-kind").selected_text(kind.map(|k| k.label()).unwrap_or(t("None"))).show_ui(ui, |ui| {
        changed |= ui.selectable_value(&mut kind, None, t("None")).changed();
        // The formal collection first, then the playful ones.
        for k in PatternKind::FORMAL.into_iter().chain(PatternKind::ALL.into_iter().filter(|k| !PatternKind::FORMAL.contains(k))) {
            changed |= ui.selectable_value(&mut kind, Some(k), k.label()).changed();
        }
    });
    match (kind, &mut s.background.pattern) {
        (None, p) => *p = None,
        (Some(k), Some(p)) => p.kind = k,
        (Some(k), p @ None) => *p = Some(Pattern::new(k)),
    }
    if let Some(p) = &mut s.background.pattern {
        changed |= ui.add(egui::Slider::new(&mut p.scale_mm, 2.0..=60.0).text(format!("{} (mm)", t("Scale")))).changed();
        let mut ang = p.angle_mdeg as f32 / 1000.0;
        if ui.add(egui::Slider::new(&mut ang, -90.0..=90.0).text(format!("{} °", t("Angle")))).changed() {
            p.angle_mdeg = deg_to_mdeg(ang);
            changed = true;
        }
        changed |= ui.add(egui::Slider::new(&mut p.opacity, 0.05..=1.0).text(t("Opacity"))).changed();
        ui.horizontal_wrapped(|ui| {
            ui.label(t("Color"));
            for (name, c) in oto_core::model::PASTELS {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().rect_filled(r, 9.0, egui::Color32::from_rgb(c.r, c.g, c.b));
                let on = p.color.r == c.r && p.color.g == c.g && p.color.b == c.b;
                ui.painter().rect_stroke(
                    r,
                    9.0,
                    egui::Stroke::new(if on { 2.0_f32 } else { 1.0_f32 }, if on { ui.visuals().strong_text_color() } else { egui::Color32::from_gray(150) }),
                    egui::StrokeKind::Outside,
                );
                if resp.on_hover_text(name).clicked() {
                    p.color = c;
                    changed = true;
                }
            }
            changed |= rgba_edit(ui, &mut p.color);
        });
    }
    section(ui, t("Notes"));
    changed |= ui.add(egui::TextEdit::multiline(&mut s.notes).desired_rows(3).hint_text("Notes for yourself or the client")).changed();
    if changed && s != cur {
        ws.commit_live(s, "Background");
    }
    section(ui, "Typography");
    let mut ps = ws.project().settings.clone();
    let before = ps.font_pair.clone();
    egui::ComboBox::from_id_salt("font-pair").selected_text(&ps.font_pair).show_ui(ui, |ui| {
        for p in oto_core::model::font_pairs() {
            ui.selectable_value(&mut ps.font_pair, p.name.clone(), format!("{} — {} / {}", p.name, p.display, p.text));
        }
    });
    if ps.font_pair != before {
        ws.exec(Command::SetProjectSettings { settings: ps });
    }
    ui.add_space(8.0);
    if ui.button(t("Save as template")).on_hover_text("Save this layout so you can reuse it on other spreads.").clicked() {
        ws.requests.push(Request::SaveTemplate);
    }
}

fn dpi_label(ui: &mut Ui, dpi: f32, pal: &theme::Palette) {
    let (txt, col) = if dpi < DPI_BLOCK {
        (format!("✖ {dpi:.0} dpi — too low to print"), pal.crit)
    } else if dpi < DPI_CRITICAL {
        (format!("✖ {dpi:.0} dpi — critical"), pal.crit)
    } else if dpi < DPI_WARN {
        (format!("⚠ {dpi:.0} dpi — acceptable"), pal.warn)
    } else {
        (format!("✔ {dpi:.0} dpi — good"), pal.ok)
    };
    ui.label(RichText::new(txt).color(col).strong());
}

fn media_info(ui: &mut Ui, m: &oto_core::media::MediaItem, used: u32, pal: &theme::Palette) {
    egui::Grid::new(("info", m.id)).num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
        let mut row = |k: &str, v: String| {
            ui.label(RichText::new(k).small().color(pal.muted));
            ui.label(RichText::new(v).small());
            ui.end_row();
        };
        row("File", m.file_name.clone());
        row("Pixels", format!("{} × {} ({:.1} MP)", m.width, m.height, m.megapixels()));
        row("Profile", m.color_profile.clone().unwrap_or_else(|| "none (sRGB assumed)".into()));
        if let Some(c) = &m.capture_time {
            row("Taken", c.replace('T', " "));
        }
        if let Some(c) = &m.camera {
            row("Camera", c.clone());
        }
        if let Some(c) = &m.lens {
            row("Lens", c.clone());
        }
        let exp = [m.iso.map(|v| format!("ISO {v}")), m.aperture.map(|v| format!("f/{v:.1}")), m.shutter.clone(), m.focal_mm.map(|v| format!("{v:.0} mm"))]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        if !exp.is_empty() {
            row("Exposure", exp);
        }
        row("Orientation", format!("EXIF {}", m.orientation));
        row(t("Used on"), format!("{used}×"));
        if m.missing {
            row("Status", "⚠ original missing".into());
        }
    });
}

fn image_tab(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    let pal = theme::panel_palette(settings.theme);
    let Some(cur) = ws.current().cloned() else { return };
    let frame = ws.sel.first().and_then(|id| cur.frames.iter().find(|f| f.id == *id)).cloned();
    let Some(f) = frame else {
        // No frame selected: show info of the selected library photo.
        if let Some(m) = ws.media_sel.first().and_then(|id| ws.media(*id)).cloned() {
            let used = ws.usage().get(&m.id).copied().unwrap_or(0);
            media_info(ui, &m, used, &pal);
        } else {
            ui.label(RichText::new(t("Select a photo frame to edit it.")).color(pal.muted));
        }
        return;
    };
    let mut s = cur.clone();
    let surf = ws.surface();
    let m = f.image_id.and_then(|id| ws.media(id)).cloned();
    if let Some(m) = &m {
        let pr = surf.print_rect(&f.rect_norm);
        let ai = m.aspect();
        let af = (pr.w() / pr.h().max(1.0)) as f32;
        let c = if f.fit == FitMode::Fill { crop::clamp(&f.crop, ai, af) } else { f.crop };
        let vis = crop::visible(&c, ai, af, f.fit);
        let dpi = crop::effective_dpi(m.width, m.height, &vis, pr.w() as i64, pr.h() as i64);
        ui.label(RichText::new(t("Effective DPI")).small().color(pal.muted));
        dpi_label(ui, dpi, &pal);
        ui.label(RichText::new(format!("Frame {:.1} × {:.1} mm", pr.w() / 1000.0, pr.h() / 1000.0)).small().color(pal.muted));
    } else {
        ui.label(RichText::new("Empty frame — drag a photo onto it.").color(pal.muted));
    }
    let mut changed = false;
    let mut label = "Edit photo";
    let Some(sf) = s.frame_mut(f.id) else { return };
    if sf.locked {
        ui.label(RichText::new("🔒 This frame is locked.").color(pal.warn));
    }
    section(ui, t("Crop"));
    ui.horizontal(|ui| {
        changed |= ui.radio_value(&mut sf.fit, FitMode::Fill, t("Fill")).changed();
        changed |= ui.radio_value(&mut sf.fit, FitMode::Fit, t("Fit")).changed();
    });
    if ui.add(egui::Slider::new(&mut sf.crop.zoom, if sf.fit == FitMode::Fit { 0.2 } else { 1.0 }..=8.0).text(t("Zoom"))).changed() {
        sf.crop_manual = true;
        changed = true;
        label = "Zoom crop";
    }
    let mut crot = sf.crop.rotation_mdeg as f32 / 1000.0;
    if ui.add(egui::Slider::new(&mut crot, -45.0..=45.0).text(format!("{} °", t("Rotation")))).changed() {
        sf.crop.rotation_mdeg = deg_to_mdeg(crot);
        sf.crop_manual = true;
        changed = true;
        label = "Rotate crop";
    }
    ui.horizontal(|ui| {
        if ui.button(format!("✂ {}", t("Crop"))).on_hover_text("C").clicked() {
            ws.crop_mode = Some(f.id);
        }
        if ui.button(t("Reset crop")).clicked() {
            if let Some(info) = f.image_id.and_then(|id| ws.photo_info(id)) {
                sf.crop = oto_core::solver::recrop(&ws.ctx(), &f, &info);
                sf.crop_manual = false;
                changed = true;
                label = "Reset crop";
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{}: {}", t("Focus point"), if f.focus.map(|x| x.manual).unwrap_or(false) { "set" } else { "auto" })).small());
        if f.focus.is_some() && ui.small_button("Clear").clicked() {
            sf.focus = None;
            changed = true;
        }
    });
    ui.label(RichText::new("In crop mode: drag to pan, scroll to zoom, Alt-click or right-click to set the focus point.").small().color(pal.muted));
    section(ui, t("Frame"));
    if sf.allow_rotate || sf.rotation_mdeg != 0 {
        let mut r = sf.rotation_mdeg as f32 / 1000.0;
        if ui.add(egui::Slider::new(&mut r, -180.0..=180.0).text(format!("{} °", t("Rotation")))).changed() {
            sf.rotation_mdeg = deg_to_mdeg(r);
            changed = true;
        }
    } else if ui.small_button("Allow rotation").clicked() {
        sf.allow_rotate = true;
        changed = true;
    }
    changed |= ui.add(egui::Slider::new(&mut sf.opacity, 0.0..=1.0).text(t("Opacity"))).changed();
    changed |= ui.add(egui::Slider::new(&mut sf.border.width_mm, 0.0..=10.0).text(format!("{} (mm)", t("Border")))).changed();
    if sf.border.width_mm > 0.0 {
        ui.horizontal(|ui| {
            ui.label(t("Color"));
            changed |= rgba_edit(ui, &mut sf.border.color);
        });
    }
    changed |= ui.add(egui::Slider::new(&mut sf.radius_mm, 0.0..=60.0).text(format!("{} (mm)", t("Radius")))).changed();
    section(ui, "Adjust");
    changed |= ui.add(egui::Slider::new(&mut sf.adjust.brightness, -1.0..=1.0).text(t("Brightness"))).changed();
    changed |= ui.add(egui::Slider::new(&mut sf.adjust.contrast, -1.0..=1.0).text(t("Contrast"))).changed();
    changed |= ui.checkbox(&mut sf.adjust.bw, t("Black & white")).changed();
    if !sf.adjust.is_identity() && ui.small_button("Reset adjustments").clicked() {
        sf.adjust = Default::default();
        changed = true;
    }
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut sf.locked, t("Lock")).changed();
    });
    let sel_media = ws.media_sel.first().copied();
    if ui.add_enabled(sel_media.is_some(), egui::Button::new("Replace with selected photo")).clicked() {
        if let Some(id) = sel_media {
            sf.image_id = Some(id);
            sf.crop_manual = false;
            sf.focus = None;
            if let Some(info) = ws.photo_info(id) {
                let fr = sf.clone();
                sf.crop = oto_core::solver::recrop(&ws.ctx(), &fr, &info);
            }
            changed = true;
            label = "Replace photo";
        }
    }
    if changed && s != cur {
        if label == "Edit photo" || label == "Zoom crop" || label == "Rotate crop" {
            ws.commit_live(s, label);
        } else {
            ws.commit(s, label);
        }
    }
    if let Some(m) = m {
        section(ui, "Photo");
        let used = ws.usage().get(&m.id).copied().unwrap_or(0);
        media_info(ui, &m, used, &pal);
    }
}

fn text_tab(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    let pal = theme::panel_palette(settings.theme);
    let Some(cur) = ws.current().cloned() else { return };
    let Some(tb) = ws.sel.first().and_then(|id| cur.texts.iter().find(|t| t.id == *id)).cloned() else {
        ui.label(RichText::new(t("Select a text box to edit it.")).color(pal.muted));
        ui.label(RichText::new("Use the Text tool (T) and click on the spread to add one.").small().color(pal.muted));
        return;
    };
    let mut s = cur.clone();
    let Some(st) = s.text_mut(tb.id) else { return };
    let mut changed = false;
    let mut discrete = false;
    let resp = ui.add(egui::TextEdit::multiline(&mut st.text).desired_rows(3).desired_width(f32::INFINITY));
    if ws.focus_text_edit {
        resp.request_focus();
        ws.focus_text_edit = false;
    }
    changed |= resp.changed();
    section(ui, t("Style"));
    let pair = canvas::pair_for(ws);
    ui.horizontal_wrapped(|ui| {
        for p in StylePreset::ALL {
            if ui.small_button(p.label()).clicked() {
                st.style = p.style(&pair);
                changed = true;
                discrete = true;
            }
        }
    });
    section(ui, t("Font"));
    egui::ComboBox::from_id_salt("font-family").selected_text(RichText::new(&st.style.font_family).family(egui::FontFamily::Name(st.style.font_family.clone().into()))).width(200.0).show_ui(
        ui,
        |ui| {
            for fam in ALBUM_FONTS {
                if ui.selectable_label(st.style.font_family == fam, RichText::new(fam).family(egui::FontFamily::Name(fam.into())).size(16.0)).clicked() {
                    st.style.font_family = fam.into();
                    changed = true;
                    discrete = true;
                }
            }
        },
    );
    ui.horizontal(|ui| {
        changed |= ui.add(egui::DragValue::new(&mut st.style.size_pt).range(2.0..=400.0).speed(0.25).suffix(" pt")).changed();
        changed |= ui.toggle_value(&mut st.style.bold, RichText::new("B").strong()).on_hover_text(t("Bold")).changed();
        changed |= ui.toggle_value(&mut st.style.italic, RichText::new("I").italics()).on_hover_text(t("Italic")).changed();
        changed |= rgba_edit(ui, &mut st.style.color);
    });
    ui.horizontal(|ui| {
        ui.label(t("Align"));
        changed |= ui.selectable_value(&mut st.style.align, Align::Left, "⇤").changed();
        changed |= ui.selectable_value(&mut st.style.align, Align::Center, "↔").changed();
        changed |= ui.selectable_value(&mut st.style.align, Align::Right, "⇥").changed();
    });
    changed |= ui.add(egui::Slider::new(&mut st.style.line_height, 0.7..=3.0).text(t("Line height"))).changed();
    changed |= ui.add(egui::Slider::new(&mut st.style.letter_spacing, -100.0..=400.0).text(format!("{} (‰ em)", t("Letter spacing")))).changed();
    changed |= ui.checkbox(&mut st.style.kerning, t("Kerning")).changed();
    let mut alpha = st.style.color.a as f32 / 255.0;
    if ui.add(egui::Slider::new(&mut alpha, 0.05..=1.0).text(t("Opacity"))).changed() {
        st.style.color.a = (alpha * 255.0).round() as u8;
        changed = true;
    }
    let mut r = st.rotation_mdeg as f32 / 1000.0;
    if ui.add(egui::Slider::new(&mut r, -180.0..=180.0).text(format!("{} °", t("Rotation")))).changed() {
        st.rotation_mdeg = deg_to_mdeg(r);
        changed = true;
    }
    changed |= ui.checkbox(&mut st.locked, t("Lock")).changed();
    // Overflow check with the print engine.
    let surf = ws.surface();
    let rr = surf.rect(&st.rect_norm);
    let lay = oto_render::text::layout(&st.text, &st.style, (rr.w() * 72.0 / 25_400.0) as f32, (rr.h() * 72.0 / 25_400.0) as f32);
    if lay.overflow {
        ui.label(RichText::new("⚠ Text does not fit its box — enlarge the box or reduce the size.").color(pal.warn));
    }
    ui.label(RichText::new("All bundled fonts are SIL Open Font License and embed in the PDF.").small().color(pal.muted));
    if changed && s != cur {
        if discrete {
            ws.commit(s, "Text style");
        } else {
            ws.commit_live(s, "Edit text");
        }
    }
}

fn layers_tab(ui: &mut Ui, ws: &mut Workspace, settings: &AppSettings) {
    let pal = theme::panel_palette(settings.theme);
    let Some(cur) = ws.current().cloned() else { return };
    let mut items: Vec<(i32, oto_core::model::ItemId, String, bool)> = Vec::new();
    for f in &cur.frames {
        let name = f.image_id.and_then(|id| ws.media(id)).map(|m| m.file_name.clone()).unwrap_or_else(|| "Empty frame".into());
        items.push((f.z, f.id, format!("▣ {name}"), f.locked));
    }
    for tb in &cur.texts {
        items.push((tb.z, tb.id, format!("T {}", tb.text.lines().next().unwrap_or("").chars().take(24).collect::<String>()), tb.locked));
    }
    for sh in &cur.shapes {
        let k = match sh.kind {
            ShapeKind::Rect => "■ Rectangle",
            ShapeKind::Ellipse => "● Ellipse",
            ShapeKind::Line => "╱ Line",
        };
        items.push((sh.z, sh.id, k.into(), sh.locked));
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.0));
    if items.is_empty() {
        ui.label(RichText::new("No items on this spread.").color(pal.muted));
    }
    for (_, id, name, locked) in items {
        ui.horizontal(|ui| {
            let sel = ws.sel.contains(&id);
            if ui.selectable_label(sel, name).clicked() {
                if ui.input(|i| i.modifiers.shift) {
                    ws.sel.push(id);
                } else {
                    ws.sel = vec![id];
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("▼").on_hover_text(t("Send backward")).clicked() {
                    canvas::reorder(ws, &cur, id, -1);
                }
                if ui.small_button("▲").on_hover_text(t("Bring forward")).clicked() {
                    canvas::reorder(ws, &cur, id, 1);
                }
                if locked {
                    ui.label("🔒");
                }
            });
        });
    }
    if ws.sel.len() >= 2 {
        section(ui, "Align & distribute");
        ui.horizontal_wrapped(|ui| {
            for (label, op) in [("⇤ Left", 0), ("↔ Centre", 1), ("⇥ Right", 2), ("Top", 3), ("Middle", 4), ("Bottom", 5), ("Distribute H", 6), ("Distribute V", 7)] {
                if ui.button(label).clicked() {
                    align(ws, &cur, op);
                }
            }
        });
    }
    let _ = Color32::WHITE;
}

fn align(ws: &mut Workspace, cur: &Spread, op: u8) {
    let rects: Vec<(oto_core::model::ItemId, oto_core::geometry::Rect)> = ws.sel.iter().filter_map(|id| canvas::item_rect(cur, *id).filter(|r| !r.2).map(|r| (*id, r.0))).collect();
    if rects.len() < 2 {
        return;
    }
    let l = rects.iter().map(|r| r.1.x).fold(f32::MAX, f32::min);
    let r_ = rects.iter().map(|r| r.1.right()).fold(f32::MIN, f32::max);
    let tp = rects.iter().map(|r| r.1.y).fold(f32::MAX, f32::min);
    let b = rects.iter().map(|r| r.1.bottom()).fold(f32::MIN, f32::max);
    let mut s = cur.clone();
    let mut sorted = rects.clone();
    match op {
        6 => sorted.sort_by(|a, b| a.1.x.total_cmp(&b.1.x)),
        7 => sorted.sort_by(|a, b| a.1.y.total_cmp(&b.1.y)),
        _ => {}
    }
    let n = sorted.len();
    let total_w: f32 = sorted.iter().map(|r| r.1.w).sum();
    let total_h: f32 = sorted.iter().map(|r| r.1.h).sum();
    let mut cursor_x = l;
    let mut cursor_y = tp;
    for (id, mut rr) in sorted {
        match op {
            0 => rr.x = l,
            1 => rr.x = (l + r_) / 2.0 - rr.w / 2.0,
            2 => rr.x = r_ - rr.w,
            3 => rr.y = tp,
            4 => rr.y = (tp + b) / 2.0 - rr.h / 2.0,
            5 => rr.y = b - rr.h,
            6 => {
                rr.x = cursor_x;
                cursor_x += rr.w + ((r_ - l) - total_w) / (n as f32 - 1.0).max(1.0);
            }
            7 => {
                rr.y = cursor_y;
                cursor_y += rr.h + ((b - tp) - total_h) / (n as f32 - 1.0).max(1.0);
            }
            _ => {}
        }
        canvas::set_item_rect(&mut s, id, rr);
    }
    ws.commit(s, "Align");
    let _ = MsgKind::Info;
}
