use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind};

use oto_core::designs::{self, AlbumKind, LayoutParams};
use oto_core::library;
use oto_core::model::ProjectSettings;
use oto_core::spec::SpreadGeometry;
use oto_core::template::Template;

use crate::theme;

pub struct PickerState {
    open: bool,
    kind: AlbumKind,
    groups: Vec<String>,
    ids: Vec<String>,
    tab_kind: AlbumKind,
    cache: Option<(LayoutParams, i64, i64, Vec<Template>)>,
}

impl PickerState {
    pub fn open_with(settings: &ProjectSettings) -> Self {
        Self {
            open: true,
            kind: settings.album_kind,
            groups: settings.template_groups.clone(),
            ids: settings.template_ids.clone(),
            tab_kind: settings.album_kind,
            cache: None,
        }
    }
}

pub fn show(
    ctx: &egui::Context,
    st: &mut PickerState,
    geom: &SpreadGeometry,
    layout: &LayoutParams,
) -> Option<(AlbumKind, Vec<String>, Vec<String>)> {
    if !st.open {
        return None;
    }

    let all_templates: Vec<Template> = match &st.cache {
        Some((cached_layout, w, h, tpls))
            if *cached_layout == *layout && *w == geom.trim_w && *h == geom.trim_h =>
        {
            tpls.clone()
        }
        _ => {
            let fresh = library::library(geom, layout);
            st.cache = Some((*layout, geom.trim_w, geom.trim_h, fresh.clone()));
            fresh
        }
    };

    let templates: Vec<&Template> = all_templates
        .iter()
        .filter(|t| AlbumKind::of(t) == Some(st.tab_kind))
        .filter(|t| !t.id.ends_with("-m"))
        .collect();

    let mut open = st.open;
    let mut done = false;
    let mut cancel = false;

    let max_h = (ctx.screen_rect().height() - 40.0).max(80.0);
    egui::Window::new("Templates")
        .resizable(true)
        .default_width(900.0)
        .default_height(640.0)
        .max_height(max_h)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for kind in AlbumKind::ALL {
                    if ui.selectable_label(st.tab_kind == kind, kind.label()).clicked() {
                        st.tab_kind = kind;
                        st.kind = kind;
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(st.groups.is_empty(), "All").clicked() {
                    st.groups.clear();
                }
                for (id, label) in designs::GROUPS {
                    let selected = st.groups.iter().any(|g| g.as_str() == id);
                    if ui.selectable_label(selected, label).clicked() {
                        if selected {
                            st.groups.retain(|g| g.as_str() != id);
                        } else {
                            st.groups.push(id.to_string());
                        }
                    }
                }
            });

            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if templates.is_empty() {
                        ui.label("No templates");
                    }
                    for n in 1..=10u32 {
                        let section: Vec<&Template> = templates
                            .iter()
                            .filter(|t| t.photo_count == n)
                            .copied()
                            .collect();
                        if section.is_empty() {
                            continue;
                        }
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(format!("{} photos", n)).strong());
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            for t in section {
                                thumbnail(ui, st, t, geom, 150.0);
                            }
                        });
                    }
                });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label(format!("{} templates in use", st.ids.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Done").clicked() {
                        done = true;
                    }
                });
            });
        });

    if done || cancel {
        st.open = false;
    } else {
        st.open = open;
    }

    if done {
        Some((st.kind, st.groups.clone(), st.ids.clone()))
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn thumbnail(
    ui: &mut egui::Ui,
    st: &mut PickerState,
    t: &Template,
    geom: &SpreadGeometry,
    thumb_w: f32,
) {
    let in_group = st.groups.is_empty() || st.groups.iter().any(|g| g == &t.family);
    let selected = st.ids.iter().any(|id| id == &t.id);
    let dim = !in_group;

    let aspect = geom.trim_w as f32 / geom.trim_h.max(1) as f32;
    let pad = 3.0;
    let name_h = 16.0;
    let page_w = (thumb_w - 2.0 * pad).max(20.0);
    let page_h = (page_w / aspect).max(8.0);
    let card_size = vec2(thumb_w, page_h + name_h + 2.0 * pad);

    let (rect, response) = ui.allocate_exact_size(card_size, Sense::click());
    if response.clicked() {
        if selected {
            st.ids.retain(|id| id != &t.id);
        } else {
            st.ids.push(t.id.clone());
        }
    }

    let accent = ui.visuals().selection.bg_fill;
    let border = ui.visuals().widgets.noninteractive.bg_stroke;
    let mut text_color = ui.visuals().text_color();
    let painter = ui.painter();

    if selected {
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.5, accent),
            StrokeKind::Inside,
        );
    } else {
        painter.rect_stroke(rect, CornerRadius::same(4), border, StrokeKind::Inside);
    }

    let page_rect = Rect::from_min_size(rect.min + vec2(pad, pad), vec2(page_w, page_h));
    let page_color = if dim {
        Color32::WHITE.gamma_multiply(0.4)
    } else {
        Color32::WHITE
    };
    painter.rect_filled(page_rect, CornerRadius::ZERO, page_color);

    let grey = Color32::from_gray(170);
    let dark_grey = Color32::from_gray(110);
    let grey = if dim { grey.gamma_multiply(0.4) } else { grey };
    let dark_grey = if dim {
        dark_grey.gamma_multiply(0.4)
    } else {
        dark_grey
    };

    for f in &t.frames {
        if f.w <= 0.0 || f.h <= 0.0 {
            continue;
        }
        let frame_rect = Rect::from_min_size(
            pos2(
                page_rect.left() + f.x * page_rect.width(),
                page_rect.top() + f.y * page_rect.height(),
            ),
            vec2(f.w * page_rect.width(), f.h * page_rect.height()),
        );
        let fill = if f.min_aspect <= 0.31 {
            dark_grey
        } else {
            grey
        };
        painter.rect_filled(frame_rect, CornerRadius::ZERO, fill);
    }

    let fold_x = page_rect.center().x;
    let fold_color = Color32::from_gray(120);
    let fold_color = if dim {
        fold_color.gamma_multiply(0.4)
    } else {
        fold_color
    };
    painter.line_segment(
        [pos2(fold_x, page_rect.top()), pos2(fold_x, page_rect.bottom())],
        Stroke::new(1.0, fold_color),
    );

    if dim {
        text_color = text_color.gamma_multiply(0.4);
    }
    let name_center = pos2(page_rect.center().x, page_rect.bottom() + name_h * 0.5 + 1.0);
    painter.text(
        name_center,
        Align2::CENTER_CENTER,
        t.name.as_str(),
        FontId::proportional(11.0),
        text_color,
    );

    if selected {
        let badge_center = pos2(page_rect.right() - 8.0, page_rect.top() + 8.0);
        painter.circle_filled(badge_center, 8.0, accent);
        painter.text(
            badge_center,
            Align2::CENTER_CENTER,
            "✔",
            FontId::proportional(10.0),
            theme::ON_ACCENT,
        );
    }
}
