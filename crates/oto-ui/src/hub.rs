//! Project hub: recent projects, search, status, due date; first-run onboarding.

use std::path::PathBuf;

use egui::{RichText, Ui, Vec2};

use crate::i18n::t;
use crate::settings::AppSettings;
use crate::theme;

pub enum HubAction {
    None,
    New,
    Open,
    OpenPath(PathBuf),
    Sample,
    Settings,
}

pub fn hub(ui: &mut Ui, settings: &mut AppSettings, search: &mut String) -> HubAction {
    let pal = theme::palette(settings.theme);
    let mut act = HubAction::None;
    ui.add_space(28.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(820.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("oto-albums").font(theme::strong_font(30.0)).color(pal.text));
            ui.label(RichText::new(format!("  {}", t("Design the story. Print it right."))).size(15.0).color(pal.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(format!("⚙ {}", t("Settings"))).clicked() {
                    act = HubAction::Settings;
                }
            });
        });
        ui.add_space(18.0);
        let first_run = settings.recent.is_empty();
        if first_run {
            egui::Frame::new().fill(pal.raised).corner_radius(12).inner_margin(24).stroke(egui::Stroke::new(1.0_f32, pal.border)).show(ui, |ui| {
                ui.label(RichText::new("Welcome. Make your first album in five steps:").font(theme::strong_font(16.0)));
                ui.label(RichText::new("1 choose the size · 2 import photos · 3 Auto Build · 4 edit · 5 export the print file").color(pal.muted));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new(format!("+ {}", t("Create your first album"))).strong().size(15.0).color(theme::ON_ACCENT)).min_size(Vec2::new(240.0, theme::HIT)).fill(pal.accent)).clicked() {
                        act = HubAction::New;
                    }
                    if ui.add(egui::Button::new(t("Open project…")).min_size(Vec2::new(160.0, theme::HIT))).clicked() {
                        act = HubAction::Open;
                    }
                    if ui.add(egui::Button::new(t("Open sample project")).min_size(Vec2::new(180.0, theme::HIT))).on_hover_text("12 synthetic photos on 6 spreads — safe to experiment with.").clicked()
                    {
                        act = HubAction::Sample;
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Works offline. No account. Your photos never leave this computer unless you export them.").small().color(pal.muted));
            });
            return;
        }
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new(format!("+ {}", t("New project"))).strong().color(theme::ON_ACCENT)).min_size(Vec2::new(150.0, 36.0)).fill(pal.accent)).clicked() {
                act = HubAction::New;
            }
            if ui.add(egui::Button::new(t("Open project…")).min_size(Vec2::new(140.0, 36.0))).clicked() {
                act = HubAction::Open;
            }
            if ui.add(egui::Button::new(t("Open sample project")).min_size(Vec2::new(150.0, 36.0))).clicked() {
                act = HubAction::Sample;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::TextEdit::singleline(search).hint_text(t("Search projects")).desired_width(220.0));
            });
        });
        ui.add_space(14.0);
        ui.label(RichText::new(t("Recent projects")).font(theme::strong_font(15.0)));
        ui.separator();
        let q = search.to_lowercase();
        let mut remove: Option<PathBuf> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for r in settings.recent.iter().filter(|r| q.is_empty() || r.name.to_lowercase().contains(&q) || r.path.to_string_lossy().to_lowercase().contains(&q)) {
                let exists = r.path.join("project.sqlite").exists();
                let resp = egui::Frame::new().fill(pal.raised).corner_radius(8).inner_margin(12).stroke(egui::Stroke::new(1.0_f32, pal.border)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&r.name).font(theme::strong_font(14.0)).color(if exists { pal.text } else { pal.muted }));
                            ui.label(RichText::new(r.path.display().to_string()).small().color(pal.muted));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !exists {
                                ui.label(RichText::new("⚠ not found").color(pal.warn));
                            }
                            if let Some(d) = &r.due_date {
                                ui.label(RichText::new(format!("{}: {d}", t("Due"))).small());
                            }
                            ui.label(RichText::new(format!("{} · {} {}", r.status, r.spreads, t("Spreads").to_lowercase())).small().color(pal.muted));
                        });
                    });
                });
                let resp = ui.interact(resp.response.rect, ui.id().with(&r.path), egui::Sense::click());
                if resp.clicked() && exists {
                    act = HubAction::OpenPath(r.path.clone());
                }
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, exists, format!("Open project {}", r.name)));
                let p = r.path.clone();
                resp.context_menu(|ui| {
                    if ui.button(t("Show in Finder")).clicked() {
                        let _ = oto_platform::native().reveal(&p);
                        ui.close_menu();
                    }
                    if ui.button(t("Remove from list")).clicked() {
                        remove = Some(p.clone());
                        ui.close_menu();
                    }
                });
                ui.add_space(6.0);
            }
        });
        if let Some(p) = remove {
            settings.recent.retain(|r| r.path != p);
            settings.save();
        }
    });
    act
}
