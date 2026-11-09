//! New Project wizard: lab preset or custom size, with a live diagram of page vs spread.

use std::path::PathBuf;

use egui::{Color32, Pos2, Rect, RichText, Stroke, Ui, Vec2};
use oto_core::designs::{AlbumKind, LayoutParams};
use oto_core::lab::LabProfile;
use oto_core::model::{font_pairs, Project, ProjectSettings};
use oto_core::solver::Style;
use oto_core::spec::{AlbumSpec, Binding, CoverType};
use oto_core::units::{Um, Units};

use crate::i18n::t;
use crate::picker::{self, PickerState};
use crate::theme;

pub struct Wizard {
    pub name: String,
    pub client: String,
    pub due: String,
    pub folder: PathBuf,
    pub lab: Option<usize>,
    pub size_id: String,
    pub spec: AlbumSpec,
    pub spreads: usize,
    pub style: Style,
    pub font_pair: String,
    /// Catalogue shown in the template library section of the wizard.
    pub templates: Vec<oto_core::template::Template>,
    pub template_filter: String,
    pub enabled_templates: std::collections::BTreeSet<String>,
    pub background: Color32,
    pub border_pt: f32,
    pub gap_pt: f32,
    pub album_kind: AlbumKind,
    pub template_groups: Vec<String>,
    pub template_ids: Vec<String>,
    pub picker: Option<PickerState>,
}

pub struct NewProject {
    pub root: PathBuf,
    pub project: Project,
    pub lab: Option<LabProfile>,
}

pub enum WizardOutcome {
    Continue,
    Cancel,
    Create(Box<NewProject>),
}

impl Wizard {
    pub fn new(labs: &[LabProfile], documents: PathBuf) -> Self {
        let lab = labs.iter().position(|l| l.id == "generic-layflat-10x10").or(if labs.is_empty() { None } else { Some(0) });
        let (spec, size_id) = match lab.and_then(|i| labs.get(i)) {
            Some(l) => (l.spec_for_size(&l.sizes[0]), l.sizes[0].id.clone()),
            None => (AlbumSpec::default(), String::new()),
        };
        let spreads = spec.min_spreads() as usize;
        Wizard {
            name: "New album".into(),
            client: String::new(),
            due: String::new(),
            folder: documents.join("oto-albums"),
            lab,
            size_id,
            spec,
            spreads,
            style: Style::Classic,
            font_pair: font_pairs()[0].name.clone(),
            templates: oto_core::builtin::spread_templates(),
            template_filter: "All templates".into(),
            enabled_templates: oto_core::builtin::spread_templates().iter().map(|t| t.id.clone()).collect(),
            background: Color32::WHITE,
            border_pt: 0.0,
            gap_pt: 4.0,
            album_kind: AlbumKind::Album,
            template_groups: Vec::new(),
            template_ids: Vec::new(),
            picker: None,
        }
    }

    fn apply_lab(&mut self, labs: &[LabProfile]) {
        if let Some(l) = self.lab.and_then(|i| labs.get(i)) {
            let size = l.size(&self.size_id).or(l.sizes.first());
            if let Some(s) = size {
                self.size_id = s.id.clone();
                self.spec = l.spec_for_size(s);
                self.spreads = self.spreads.clamp(self.spec.min_spreads() as usize, self.spec.max_spreads() as usize);
            }
        }
    }

    fn root(&self) -> PathBuf {
        let safe: String = self.name.chars().map(|c| if c.is_alphanumeric() || " -_&".contains(c) { c } else { '_' }).collect();
        self.folder.join(format!("{}.otoalbum", safe.trim()))
    }

    pub fn errors(&self) -> Vec<String> {
        let mut e: Vec<String> = self.spec.validate().into_iter().filter(|i| !(i.field == "bleed" && self.spec.bleed_um == 0)).map(|i| i.message).collect();
        if self.name.trim().is_empty() {
            e.push("Give the project a name.".into());
        }
        let pages = (self.spreads * 2) as u32;
        if !self.spec.page_count_ok(pages) {
            e.push(format!("{} spreads = {pages} pages, but this size takes {}–{} pages in steps of {}.", self.spreads, self.spec.min_pages, self.spec.max_pages, self.spec.page_increment));
        }
        if !self.due.trim().is_empty() && oto_core::solver::parse_time(&format!("{}T00:00:00", self.due.trim())).is_none() {
            e.push("Due date must look like 2026-12-31.".into());
        }
        if self.root().join("project.sqlite").exists() {
            e.push(format!("A project already exists at {}", self.root().display()));
        }
        e
    }

    pub fn ui(&mut self, ctx: &egui::Context, labs: &[LabProfile], mode: theme::ThemeMode) -> WizardOutcome {
        let pal = theme::palette(mode);
        let mut out = WizardOutcome::Continue;
        let mut open = true;
        egui::Window::new(t("New project"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(920.0)
            .max_height(ctx.screen_rect().height() - 40.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let body_h = (ctx.screen_rect().height() - 260.0).max(220.0);
                egui::ScrollArea::vertical().max_height(body_h).auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(430.0);
                            egui::Grid::new("wiz-basic").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                                ui.label(t("Project name"));
                                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(260.0));
                                ui.end_row();
                                ui.label(t("Client"));
                                ui.add(egui::TextEdit::singleline(&mut self.client).desired_width(260.0));
                                ui.end_row();
                                ui.label(t("Due"));
                                ui.add(egui::TextEdit::singleline(&mut self.due).hint_text("YYYY-MM-DD").desired_width(120.0));
                                ui.end_row();
                                ui.label(t("Save in"));
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(self.folder.display().to_string()).small());
                                    if ui.small_button(t("Choose…")).clicked() {
                                        if let Some(p) = oto_platform::native().pick_folder("Folder for the project") {
                                            self.folder = p;
                                        }
                                    }
                                });
                                ui.end_row();
                            });
                            ui.separator();
                            ui.label(RichText::new(t("Album size")).font(theme::strong_font(14.0)));
                            let label = self.lab.and_then(|i| labs.get(i)).map(crate::dialogs::lab_label).unwrap_or_else(|| t("Custom size").into());
                            let mut changed_lab = false;
                            egui::ComboBox::from_id_salt("wiz-lab").selected_text(label).width(300.0).show_ui(ui, |ui| {
                                changed_lab |= ui.selectable_value(&mut self.lab, None, t("Custom size")).changed();
                                for (i, l) in labs.iter().enumerate() {
                                    changed_lab |= ui.selectable_value(&mut self.lab, Some(i), crate::dialogs::lab_label(l)).changed();
                                }
                            });
                            if changed_lab {
                                self.size_id.clear();
                                self.apply_lab(labs);
                            }
                            if let Some(l) = self.lab.and_then(|i| labs.get(i)) {
                                let mut sid = self.size_id.clone();
                                egui::ComboBox::from_id_salt("wiz-size").selected_text(format!("{}: {sid}", t("Size"))).show_ui(ui, |ui| {
                                    for s in &l.sizes {
                                        ui.selectable_value(&mut sid, s.id.clone(), format!("{} — {}×{} mm", s.id, s.page_width_mm, s.page_height_mm));
                                    }
                                });
                                if sid != self.size_id {
                                    self.size_id = sid;
                                    self.apply_lab(labs);
                                }
                                ui.label(RichText::new(format!("source: {} · verified {}", l.source, l.verified_at)).small().color(pal.muted));
                                if l.is_example() {
                                    ui.label(RichText::new(format!("⚠ {}", t("Example profile — not verified by a real lab."))).small().color(pal.warn));
                                }
                                ui.collapsing("Details", |ui| {
                                    ui.add_enabled_ui(false, |ui| spec_editor(ui, &mut self.spec.clone()));
                                });
                            } else {
                                spec_editor(ui, &mut self.spec);
                            }
                            ui.add_space(6.0);
                            egui::Grid::new("wiz-more").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                                ui.label(t("Spreads"));
                                ui.horizontal(|ui| {
                                    ui.add(egui::DragValue::new(&mut self.spreads).range(1..=200));
                                    let pages = self.spreads * 2;
                                    let ok = self.spec.page_count_ok(pages as u32);
                                    ui.label(RichText::new(format!("= {pages} pages{}", if ok { "" } else { " ⚠" })).color(if ok { pal.muted } else { pal.warn }));
                                });
                                ui.end_row();
                                ui.label(t("Template style"));
                                ui.horizontal(|ui| {
                                    for s in Style::ALL {
                                        ui.selectable_value(&mut self.style, s, s.label());
                                    }
                                });
                                ui.end_row();
                                ui.label(t("Font"));
                                egui::ComboBox::from_id_salt("wiz-fonts").selected_text(&self.font_pair).show_ui(ui, |ui| {
                                    for p in font_pairs() {
                                        ui.selectable_value(&mut self.font_pair, p.name.clone(), format!("{} — {} / {}", p.name, p.display, p.text));
                                    }
                                });
                                ui.end_row();
                            });
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            diagram(ui, &self.spec, &pal);
                            ui.separator();
                            ui.label(RichText::new(t("Album type")).font(theme::strong_font(14.0)));
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.album_kind, AlbumKind::Album, t("Photo album"));
                                ui.radio_value(&mut self.album_kind, AlbumKind::Photobook, t("Photobook"));
                            });
                            let desc = if self.album_kind == AlbumKind::Album {
                                t("Printed as full spreads; photos may cross the centre.")
                            } else {
                                t("Bound book with a centre fold; photos stay clear of it.")
                            };
                            ui.label(RichText::new(desc).small().color(pal.muted));
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(format!("{}: {} · {}: {}", t("Groups"), self.template_groups.len(), t("Templates"), self.template_ids.len()));
                                if ui.button(t("Choose templates…")).clicked() {
                                    let settings = ProjectSettings {
                                        album_kind: self.album_kind,
                                        template_groups: self.template_groups.clone(),
                                        template_ids: self.template_ids.clone(),
                                        ..Default::default()
                                    };
                                    self.picker = Some(PickerState::open_with(&settings));
                                }
                            });
                        });
                    });
                });
                ui.separator();
                let errs = self.errors();
                if !errs.is_empty() {
                    ui.label(RichText::new(t("Fix these before continuing:")).color(pal.warn));
                    for e in &errs {
                        ui.label(RichText::new(format!("• {e}")).small().color(pal.warn));
                    }
                }
                if self.spec.bleed_um == 0 {
                    ui.label(RichText::new("⚠ No bleed: any trimming error will show white edges.").small().color(pal.warn));
                }
                ui.horizontal(|ui| {
                    if ui.button(t("Cancel")).clicked() {
                        out = WizardOutcome::Cancel;
                    }
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ctx.wants_keyboard_input();
                    if ui.add_enabled(errs.is_empty(), egui::Button::new(RichText::new(t("Create project")).strong()).min_size(Vec2::new(150.0, 32.0))).on_hover_text("Enter").clicked()
                        || (enter && errs.is_empty())
                    {
                        let lab = self.lab.and_then(|i| labs.get(i)).cloned();
                        let mut spec = self.spec.clone();
                        spec.normalize();
                        let mut p = Project::new(self.name.trim(), spec, self.spreads);
                        p.client = self.client.trim().into();
                        p.due_date = if self.due.trim().is_empty() { None } else { Some(self.due.trim().into()) };
                        p.lab_id = lab.as_ref().map(|l| l.id.clone());
                        p.lab_size_id = lab.as_ref().map(|_| self.size_id.clone());
                        p.settings.style_preset = self.style.key().into();
                        p.settings.font_pair = self.font_pair.clone();
                        p.settings.album_kind = self.album_kind;
                        p.settings.template_groups = self.template_groups.clone();
                        p.settings.template_ids = self.template_ids.clone();
                        out = WizardOutcome::Create(Box::new(NewProject { root: self.root(), project: p, lab }));
                    }
                });
            });
        if let Some(st) = &mut self.picker {
            let geom = self.spec.geometry();
            let layout = LayoutParams::default();
            if let Some((kind, groups, ids)) = picker::show(ctx, st, &geom, &layout) {
                self.album_kind = kind;
                self.template_groups = groups;
                self.template_ids = ids;
                self.picker = None;
            }
        }
        if !open {
            out = WizardOutcome::Cancel;
        }
        out
    }
}

impl Wizard {
    /// Template library + appearance, mirroring the reference product's New Project step:
    /// a filter, a grid of slot diagrams, then background / border / gap.
    fn template_library(&mut self, ui: &mut Ui, pal: &theme::Palette) {
        ui.label(RichText::new(t("Template library")).font(theme::strong_font(14.0)));
        let mut filter = self.template_filter.clone();
        egui::ComboBox::from_id_salt("wiz-tpl-filter").selected_text(filter.clone()).width(240.0).show_ui(ui, |ui| {
            for f in ["All templates", "1 photo", "2 photos", "3 photos", "4 photos", "5+ photos"] {
                ui.selectable_value(&mut filter, f.to_string(), f);
            }
        });
        self.template_filter = filter.clone();
        let exact: Option<u32> = match filter.as_str() {
            "1 photo" => Some(1),
            "2 photos" => Some(2),
            "3 photos" => Some(3),
            "4 photos" => Some(4),
            _ => None,
        };
        let many = filter == "5+ photos";
        ui.add_space(4.0);
        let mut toggles: Vec<(String, bool)> = Vec::new();
        egui::ScrollArea::vertical().id_salt("wiz-tpl-scroll").max_height(140.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for tpl in &self.templates {
                    if let Some(n) = exact {
                        if tpl.photo_count != n {
                            continue;
                        }
                    }
                    if many && tpl.photo_count < 5 {
                        continue;
                    }
                    let on = self.enabled_templates.contains(&tpl.id);
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(96.0, 66.0), egui::Sense::click());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 3.0, if on { Color32::WHITE } else { Color32::from_gray(242) });
                    p.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, if on { pal.accent } else { pal.border }), egui::StrokeKind::Inside);
                    let inner = rect.shrink(5.0);
                    for f in &tpl.frames {
                        let mut fx = f.x;
                        if matches!(f.region, oto_core::template::Region::RightPage | oto_core::template::Region::Front) {
                            fx += 0.5;
                        }
                        let r = Rect::from_min_size(Pos2::new(inner.min.x + fx * inner.width(), inner.min.y + f.y * inner.height()), Vec2::new(f.w * inner.width(), f.h * inner.height()));
                        p.rect_filled(r, 1.0, if on { Color32::from_rgb(0x6E, 0xD3, 0xE8) } else { Color32::from_gray(206) });
                    }
                    if resp.clicked() {
                        toggles.push((tpl.id.clone(), !on));
                    }
                }
            });
        });
        for (id, on) in toggles {
            if on {
                self.enabled_templates.insert(id);
            } else {
                self.enabled_templates.remove(&id);
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new(t("Appearance")).font(theme::strong_font(14.0)));
        ui.horizontal(|ui| {
            ui.label(t("Background color"));
            ui.color_edit_button_srgba(&mut self.background);
            ui.add_space(14.0);
            ui.label(t("Border around images"));
            ui.add(egui::DragValue::new(&mut self.border_pt).range(0.0..=20.0).suffix(" pt"));
            ui.add_space(14.0);
            ui.label(t("Gap between images"));
            ui.add(egui::DragValue::new(&mut self.gap_pt).range(0.0..=40.0).suffix(" pt"));
        });
    }
}

fn um_field(ui: &mut Ui, label: &str, v: &mut Um, units: Units, max_mm: f64) {
    ui.label(label);
    let mut x = units.from_um(*v);
    let max = units.from_um(oto_core::units::mm(max_mm));
    let step = match units {
        Units::Mm => 0.5,
        Units::Cm => 0.05,
        Units::In => 0.01,
    };
    if ui.add(egui::DragValue::new(&mut x).range(0.0..=max).speed(step).max_decimals(units.decimals()).suffix(format!(" {}", units.label()))).changed() {
        *v = units.to_um(x);
    }
    ui.end_row();
}

/// Editable spec grid (custom sizes and Album settings).
pub fn spec_editor(ui: &mut Ui, s: &mut AlbumSpec) {
    egui::Grid::new(ui.id().with("spec-grid")).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(t("Units"));
        ui.horizontal(|ui| {
            for u in Units::ALL {
                ui.selectable_value(&mut s.units, u, u.label());
            }
        });
        ui.end_row();
        let u = s.units;
        um_field(ui, t("Page width"), &mut s.page_width_um, u, 1000.0);
        um_field(ui, t("Page height"), &mut s.page_height_um, u, 1000.0);
        um_field(ui, t("Bleed"), &mut s.bleed_um, u, 20.0);
        um_field(ui, t("Safe zone"), &mut s.safe_um, u, 60.0);
        um_field(ui, t("Gutter"), &mut s.gutter_um, u, 100.0);
        um_field(ui, t("Trim tolerance"), &mut s.trim_outer_um, u, 10.0);
        um_field(ui, t("Inside trim"), &mut s.trim_inside_um, u, 50.0);
        ui.label(t("Pages min"));
        ui.add(egui::DragValue::new(&mut s.min_pages).range(1..=1000));
        ui.end_row();
        ui.label(t("Pages max"));
        ui.add(egui::DragValue::new(&mut s.max_pages).range(1..=1000));
        ui.end_row();
        ui.label(t("Page step"));
        ui.add(egui::DragValue::new(&mut s.page_increment).range(1..=16));
        ui.end_row();
        ui.label(t("Cover"));
        egui::ComboBox::from_id_salt(ui.id().with("cover")).selected_text(s.cover_type.key()).show_ui(ui, |ui| {
            for c in CoverType::ALL {
                ui.selectable_value(&mut s.cover_type, c, c.key());
            }
        });
        ui.end_row();
        ui.label(t("Binding"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.binding, Binding::Layflat, t("Layflat (spreads)"));
            ui.selectable_value(&mut s.binding, Binding::SinglePages, t("Single pages"));
        });
        ui.end_row();
        ui.label(t("Resolution"));
        ui.add(egui::DragValue::new(&mut s.target_dpi).range(72..=1200).suffix(" dpi"));
        ui.end_row();
    });
    s.normalize();
}

/// Scaled diagram: page vs spread, bleed, trim, safe, gutter, with the formula.
pub fn diagram(ui: &mut Ui, s: &AlbumSpec, pal: &theme::Palette) {
    let g = s.geometry();
    let avail = ui.available_width().clamp(280.0, 460.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(avail, avail * 0.62 + 40.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let mw = g.media_w() as f32;
    let mh = g.media_h() as f32;
    let sc = ((rect.width() - 20.0) / mw).min((rect.height() - 50.0) / mh);
    let origin = rect.min + Vec2::new((rect.width() - mw * sc) / 2.0, 10.0);
    let r = |x: Um, y: Um, w: Um, h: Um| Rect::from_min_size(origin + Vec2::new(x as f32 * sc, y as f32 * sc), Vec2::new(w as f32 * sc, h as f32 * sc));
    let media = r(0, 0, g.media_w(), g.media_h());
    p.rect_filled(media, 0.0, Color32::from_rgb(0xF6, 0xD9, 0xCF));
    let trim = r(g.bleed, g.bleed, g.trim_w, g.trim_h);
    p.rect_filled(trim, 0.0, theme::PAPER);
    p.rect_stroke(trim, 0.0, Stroke::new(1.0_f32, Color32::BLACK), egui::StrokeKind::Middle);
    for sr in g.safe_pages() {
        let q = r(g.bleed + sr.x, g.bleed + sr.y, sr.w, sr.h);
        p.rect_stroke(q, 0.0, Stroke::new(1.0_f32, theme::CYAN), egui::StrokeKind::Middle);
    }
    let band = g.gutter_band();
    p.rect_filled(r(g.bleed + band.x, g.bleed, band.w, g.trim_h), 0.0, theme::CYAN.gamma_multiply(0.25));
    let fold_x = trim.left() + trim.width() / 2.0;
    p.line_segment([Pos2::new(fold_x, trim.top()), Pos2::new(fold_x, trim.bottom())], Stroke::new(1.0_f32, theme::CYAN));
    let f = egui::FontId::proportional(10.5);
    let dark = Color32::from_rgb(0x30, 0x30, 0x30);
    p.text(trim.center() - Vec2::new(trim.width() / 4.0, 0.0), egui::Align2::CENTER_CENTER, "page 1", f.clone(), dark);
    p.text(trim.center() + Vec2::new(trim.width() / 4.0, 0.0), egui::Align2::CENTER_CENTER, "page 2", f.clone(), dark);
    p.text(media.left_top() + Vec2::new(3.0, 1.0), egui::Align2::LEFT_TOP, "bleed", f.clone(), theme::RED);
    let y = media.bottom() + 6.0;
    let legend = [(theme::RED, "bleed (cut off)"), (Color32::BLACK, "trim"), (theme::CYAN, "safe / gutter")];
    let mut x = media.left();
    for (c, l) in legend {
        p.rect_filled(Rect::from_min_size(Pos2::new(x, y + 3.0), Vec2::splat(9.0)), 1.0, c);
        p.text(Pos2::new(x + 13.0, y), egui::Align2::LEFT_TOP, l, f.clone(), pal.text);
        x += 110.0;
    }
    ui.label(RichText::new(s.formula()).small().color(pal.muted));
    ui.label(
        RichText::new(format!(
            "Spread trim {} × {} · safe {} from trim · gutter band {}",
            s.units.format(g.trim_w),
            s.units.format(g.trim_h),
            s.units.format(g.safe),
            s.units.format(g.gutter_half() * 2)
        ))
        .small()
        .color(pal.muted),
    );
}
