//! Dialogs: Auto Build, Preflight, Export, Proof, Snapshots, Relink, album/project settings,
//! Save as template, command palette, shortcuts, settings.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver};
use egui::{Color32, RichText, Vec2};
use oto_core::document::Command;
use oto_core::lab::LabProfile;
use oto_core::model::{MediaId, PreflightOverride, ProjectStatus, Spread};
use oto_core::solver::{BuildParams, BuildResult, Grouping, PhotoInfo, Reuse};
use oto_core::spec::{AlbumSpec, Binding};
use oto_core::template::{GutterPolicy, Region, Template, TemplateFrame, TemplateKind, TemplateText};
use oto_core::units::Units;
use oto_print::export::{ExportInput, ExportMsg, ExportOptions, ExportResult};
use oto_print::pdf::Standard;
use oto_print::preflight::{Issue, Severity};
use oto_render::raster::Surface;

use crate::draw;
use crate::i18n::{t, Lang};
use crate::settings::AppSettings;
use crate::state::{ExportJob, MsgKind, Request, Target, Workspace};
use crate::theme;

// ---------------------------------------------------------------------------------------------
// Auto Build
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbSource {
    Unused,
    Selected,
    Staged,
    FromHere,
}

pub struct AutoBuildDlg {
    pub source: AbSource,
    pub selected: Vec<MediaId>,
    pub from: Option<usize>,
    pub auto_spreads: bool,
    pub spreads: usize,
    pub min_per: usize,
    pub max_per: usize,
    pub grouping: Grouping,
    pub reuse: Reuse,
    pub seed: u64,
    pub rx: Option<Receiver<Vec<BuildResult>>>,
    pub results: Vec<BuildResult>,
    pub chosen: usize,
    pub open: bool,
}

impl AutoBuildDlg {
    pub fn new(ws: &Workspace, from: Option<usize>) -> Self {
        let selected = ws.auto_build_source.clone().unwrap_or_default();
        let source = if from.is_some() {
            AbSource::FromHere
        } else if !selected.is_empty() && selected == ws.project().timeline.staged {
            AbSource::Staged
        } else if !selected.is_empty() {
            AbSource::Selected
        } else {
            AbSource::Unused
        };
        let spreads = ws.project().spreads.len().max(ws.project().spec.min_spreads() as usize);
        AutoBuildDlg {
            source,
            selected,
            from,
            auto_spreads: false,
            spreads,
            min_per: 1,
            max_per: 5,
            grouping: Grouping::Time,
            reuse: Reuse::Medium,
            seed: 1,
            rx: None,
            results: vec![],
            chosen: 0,
            open: true,
        }
    }

    fn photos(&self, ws: &Workspace) -> Vec<MediaId> {
        let usage = ws.usage();
        let mut ids: Vec<MediaId> = match self.source {
            AbSource::Selected => self.selected.clone(),
            AbSource::Staged => ws.project().timeline.staged.clone(),
            AbSource::Unused => ws.media.iter().filter(|m| !m.rejected && !usage.contains_key(&m.id)).map(|m| m.id).collect(),
            AbSource::FromHere => {
                let k = self.from.unwrap_or(0);
                let mut v: Vec<MediaId> = ws.project().spreads.iter().skip(k).filter(|s| !s.locked).flat_map(|s| s.media_ids()).collect();
                v.extend(ws.project().timeline.staged.iter().copied());
                v
            }
        };
        ids.retain(|id| ws.media(*id).map(|m| !m.rejected).unwrap_or(false));
        ids.sort_by(|a, b| {
            let (ma, mb) = (ws.media(*a), ws.media(*b));
            ma.and_then(|m| m.capture_time.clone()).cmp(&mb.and_then(|m| m.capture_time.clone())).then(ma.map(|m| m.natural_key()).cmp(&mb.map(|m| m.natural_key())))
        });
        ids.dedup();
        ids
    }

    fn run(&mut self, ws: &Workspace) {
        let ids = self.photos(ws);
        let photos: Vec<PhotoInfo> = ids.iter().filter_map(|id| ws.photo_info(*id)).collect();
        let templates = ws.templates.clone();
        let spec = ws.project().spec.clone();
        let fixed = match self.source {
            AbSource::FromHere => ws.project().spreads.iter().take(self.from.unwrap_or(0)).count() + ws.project().spreads.iter().skip(self.from.unwrap_or(0)).filter(|s| s.locked).count(),
            _ => ws.project().spreads.iter().filter(|s| s.locked).count(),
        };
        let target = if self.auto_spreads { None } else { Some(self.spreads.saturating_sub(fixed).max(1)) };
        let params = BuildParams { spreads: target, min_per: self.min_per, max_per: self.max_per.max(self.min_per), grouping: self.grouping, reuse: self.reuse, seed: self.seed, ..Default::default() };
        let (tx, rx) = unbounded();
        std::thread::spawn(move || {
            let r = oto_core::solver::auto_build_variants(&photos, &templates, &spec, &params);
            let _ = tx.send(r);
        });
        self.rx = Some(rx);
        self.results.clear();
    }
}

/// Merge newly built spreads into the book: locked spreads keep their position.
pub fn merge_spreads(existing: &[Spread], new: Vec<Spread>, from: Option<usize>) -> Vec<Spread> {
    let k = from.unwrap_or(0);
    let mut out: Vec<Spread> = existing.iter().take(k).cloned().collect();
    let tail = &existing[k.min(existing.len())..];
    let mut new_iter = new.into_iter();
    for s in tail {
        if s.locked {
            out.push(s.clone());
        } else if let Some(mut n) = new_iter.next() {
            n.id = s.id;
            out.push(n);
        }
    }
    out.extend(new_iter);
    out
}

pub fn auto_build(ctx: &egui::Context, ws: &mut Workspace, dlg: &mut AutoBuildDlg, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    if let Some(rx) = &dlg.rx {
        if let Ok(r) = rx.try_recv() {
            dlg.results = r;
            dlg.rx = None;
            dlg.chosen = 0;
        } else {
            ctx.request_repaint();
        }
    }
    let mut open = dlg.open;
    let mut apply: Option<usize> = None;
    egui::Window::new(t("Auto Build")).open(&mut open).collapsible(false).resizable(true).default_size([980.0, 640.0]).show(ctx, |ui| {
        let n = dlg.photos(ws).len();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                ui.label(RichText::new(t("Source")).strong());
                ui.radio_value(&mut dlg.source, AbSource::Unused, t("All unused photos"));
                ui.add_enabled_ui(!dlg.selected.is_empty(), |ui| {
                    ui.radio_value(&mut dlg.source, AbSource::Selected, format!("{} ({})", t("Selected photos"), dlg.selected.len()));
                });
                ui.add_enabled_ui(!ws.project().timeline.staged.is_empty(), |ui| {
                    ui.radio_value(&mut dlg.source, AbSource::Staged, format!("{} ({})", t("Timeline (not placed)"), ws.project().timeline.staged.len()));
                });
                if let Some(k) = dlg.from {
                    ui.radio_value(&mut dlg.source, AbSource::FromHere, format!("{} {}", t("Auto Build from here"), k + 1));
                }
                ui.label(RichText::new(format!("{n} photos in story order (capture time)")).small().color(pal.muted));
                ui.add_space(8.0);
                ui.label(RichText::new(t("Number of spreads")).strong());
                ui.checkbox(&mut dlg.auto_spreads, t("Automatic"));
                ui.add_enabled(!dlg.auto_spreads, egui::Slider::new(&mut dlg.spreads, 1..=100).text(t("Spreads")));
                let pages = dlg.spreads * 2;
                if !dlg.auto_spreads && !ws.project().spec.page_count_ok(pages as u32) {
                    ui.label(RichText::new(format!("⚠ {pages} pages is not allowed by this album size")).small().color(pal.warn));
                }
                ui.label(RichText::new(t("Photos per spread")).strong());
                ui.add(egui::Slider::new(&mut dlg.min_per, 1..=8).text("min"));
                ui.add(egui::Slider::new(&mut dlg.max_per, 1..=8).text("max"));
                if n > 0 && !dlg.auto_spreads {
                    let density = n as f32 / dlg.spreads.max(1) as f32;
                    let warn = density > 6.0;
                    ui.label(RichText::new(format!("{}≈ {density:.1} photos per spread", if warn { "⚠ " } else { "" })).small().color(if warn { pal.warn } else { pal.muted }));
                }
                ui.label(RichText::new(t("Grouping")).strong());
                egui::ComboBox::from_id_salt("ab-group")
                    .selected_text(match dlg.grouping {
                        Grouping::Time => t("Time blocks"),
                        Grouping::BlackWhite => t("Black & white"),
                        Grouping::Similarity => t("Visual similarity"),
                        Grouping::None => t("None"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut dlg.grouping, Grouping::Time, t("Time blocks"));
                        ui.selectable_value(&mut dlg.grouping, Grouping::BlackWhite, t("Black & white"));
                        ui.selectable_value(&mut dlg.grouping, Grouping::Similarity, t("Visual similarity"));
                        ui.selectable_value(&mut dlg.grouping, Grouping::None, t("None"));
                    });
                ui.label(RichText::new(t("Template reuse")).strong());
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut dlg.reuse, Reuse::Low, t("Low"));
                    ui.selectable_value(&mut dlg.reuse, Reuse::Medium, t("Medium"));
                    ui.selectable_value(&mut dlg.reuse, Reuse::High, t("High"));
                });
                ui.label(RichText::new("Always on: faces/focus out of the fold, DPI limits, duplicate guard, locked spreads kept.").small().color(pal.muted));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(n > 0 && dlg.rx.is_none(), egui::Button::new(RichText::new(format!("▶ {}", t("Build"))).strong())).clicked() {
                        dlg.run(ws);
                    }
                    if ui.add_enabled(!dlg.results.is_empty() && dlg.rx.is_none(), egui::Button::new(format!("🔀 {}", t("Shuffle")))).on_hover_text("Same settings, different seed").clicked() {
                        dlg.seed = dlg.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) % 100_000;
                        dlg.run(ws);
                    }
                    ui.label(RichText::new(format!("seed {}", dlg.seed)).small().color(pal.muted));
                });
                if dlg.rx.is_some() {
                    ui.spinner();
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                if dlg.results.is_empty() {
                    ui.label(
                        RichText::new("Choose your settings and press Build. You get three variations — Classic, Airy and Editorial — to compare before anything changes in your album.")
                            .color(pal.muted),
                    );
                    return;
                }
                for (vi, r) in dlg.results.iter().enumerate() {
                    let chosen = dlg.chosen == vi;
                    egui::Frame::new().stroke(egui::Stroke::new(if chosen { 2.0_f32 } else { 1.0_f32 }, if chosen { pal.accent } else { pal.border })).corner_radius(8).inner_margin(8).show(
                        ui,
                        |ui| {
                            ui.horizontal(|ui| {
                                if ui.radio(chosen, RichText::new(r.style.label()).strong()).clicked() {
                                    dlg.chosen = vi;
                                }
                                let photos: usize = r.spreads.iter().map(|s| s.media_ids().len()).sum();
                                ui.label(
                                    RichText::new(format!(
                                        "{} spreads · {:.1} photos/spread · {} ms{}",
                                        r.spreads.len(),
                                        photos as f32 / r.spreads.len().max(1) as f32,
                                        r.elapsed_ms,
                                        if r.warnings.is_empty() { String::new() } else { format!(" · ⚠ {}", r.warnings.len()) }
                                    ))
                                    .small()
                                    .color(pal.muted),
                                );
                            });
                            let h = 54.0;
                            let g = ws.project().spec.geometry();
                            let surf = Surface { w: g.trim_w, h: g.trim_h, bleed: g.bleed };
                            let w = h * surf.w as f32 / surf.h as f32;
                            egui::ScrollArea::horizontal().id_salt(("ab-strip", vi)).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    for s in &r.spreads {
                                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::click());
                                        let Workspace { tex, media, media_idx, .. } = &mut *ws;
                                        let lookup = |id: MediaId| media_idx.get(&id).and_then(|&i| media.get(i)).cloned();
                                        ui.painter().rect_filled(rect, 2.0, Color32::WHITE);
                                        draw::paint_thumb(ui.painter(), ui.ctx(), tex, rect, s, &surf, &lookup);
                                        resp.on_hover_text(&s.explain);
                                    }
                                });
                            });
                            if chosen {
                                ui.collapsing(t("Why this layout?"), |ui| {
                                    for (i, s) in r.spreads.iter().enumerate() {
                                        ui.label(RichText::new(format!("{}. {}", i + 1, s.explain)).small());
                                    }
                                    for w in &r.warnings {
                                        ui.label(RichText::new(format!("⚠ {w}")).small().color(pal.warn));
                                    }
                                });
                            }
                        },
                    );
                    ui.add_space(4.0);
                }
                let locked = ws.project().spreads.iter().filter(|s| s.locked).count();
                let replaced = match dlg.from {
                    Some(k) => ws.project().spreads.iter().skip(k).filter(|s| !s.locked).count(),
                    None => ws.project().spreads.iter().filter(|s| !s.locked).count(),
                };
                ui.label(
                    RichText::new(format!(
                        "Applying replaces {replaced} unlocked spread(s){}; a snapshot is taken first and Undo works.",
                        if locked > 0 { format!(" and keeps {locked} locked") } else { String::new() }
                    ))
                    .small()
                    .color(pal.muted),
                );
                if ui.button(RichText::new(format!("✔ {} — {}", t("Apply variant"), dlg.results[dlg.chosen].style.label())).strong()).clicked() {
                    apply = Some(dlg.chosen);
                }
            });
        });
    });
    if let Some(i) = apply {
        let r = dlg.results[i].clone();
        let used: std::collections::HashSet<MediaId> = r.spreads.iter().flat_map(|s| s.media_ids()).collect();
        let merged = merge_spreads(&ws.project().spreads, r.spreads, if dlg.source == AbSource::FromHere { dlg.from } else { None });
        if ws.exec(Command::SetSpreads { spreads: merged, label: format!("Auto Build ({})", r.style.label()) }) {
            let mut staged = ws.project().timeline.staged.clone();
            staged.retain(|id| !used.contains(id));
            ws.exec(Command::SetStaged { staged });
            let mut ps = ws.project().settings.clone();
            ps.style_preset = r.style.key().into();
            ws.exec(Command::SetProjectSettings { settings: ps });
            if let Some(first) = ws.project().spreads.first().map(|s| s.id) {
                ws.go_to(Target::Spread(first));
            }
            ws.msg(format!("Auto Build applied ({}). Undo restores the previous layout.", r.style.label()), MsgKind::Ok);
        }
        ws.auto_build_source = None;
        dlg.open = false;
        return;
    }
    dlg.open = open;
    if !open {
        ws.auto_build_source = None;
    }
}

// ---------------------------------------------------------------------------------------------
// Preflight
// ---------------------------------------------------------------------------------------------

pub struct PreflightDlg {
    pub open: bool,
    pub filter: Option<Severity>,
    pub overriding: Option<(Issue, String)>,
}

pub fn preflight(ctx: &egui::Context, ws: &mut Workspace, dlg: &mut PreflightDlg, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    let mut open = dlg.open;
    egui::Window::new(t("Preflight")).open(&mut open).default_size([640.0, 560.0]).show(ctx, |ui| {
        let r = ws.preflight.clone();
        let (c, w, i) = (r.count(Severity::Critical), r.count(Severity::Warning), r.count(Severity::Info));
        ui.horizontal(|ui| {
            for (sev, n, label, col) in [
                (Some(Severity::Critical), c, t("Critical"), pal.crit),
                (Some(Severity::Warning), w, t("Warning"), pal.warn),
                (Some(Severity::Info), i, t("Info"), pal.muted),
                (None, r.issues.len(), t("All"), pal.text),
            ] {
                let icon = match sev {
                    Some(Severity::Critical) => "✖",
                    Some(Severity::Warning) => "⚠",
                    Some(Severity::Info) => "ℹ",
                    None => "≡",
                };
                if ui.selectable_label(dlg.filter == sev, RichText::new(format!("{icon} {label} {n}")).color(col)).clicked() {
                    dlg.filter = sev;
                }
            }
            if ui.button("↻").on_hover_text("Re-check now (re-reads files)").clicked() {
                ws.refresh_status();
                ws.run_preflight();
            }
        });
        if r.issues.iter().all(|i| i.severity == Severity::Info || i.overridden.is_some()) {
            ui.label(RichText::new(format!("✔ {}", t("No issues. Ready to print."))).color(pal.ok).strong());
        } else if !r.can_export() {
            ui.label(RichText::new(format!("✖ {} — {} critical issue(s) must be fixed first.", t("Export blocked"), r.blocking().len())).color(pal.crit));
        }
        ui.separator();
        let issues: Vec<Issue> = r.issues.iter().filter(|i| dlg.filter.map(|f| i.severity == f).unwrap_or(true)).cloned().collect();
        let mut goto: Option<Issue> = None;
        let mut fix: Option<Issue> = None;
        let mut unoverride: Option<Issue> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (k, is) in issues.iter().enumerate() {
                let (icon, col) = match is.severity {
                    Severity::Critical => ("✖", pal.crit),
                    Severity::Warning => ("⚠", pal.warn),
                    Severity::Info => ("ℹ", pal.muted),
                };
                let head = format!("{icon} {} · {} · {}{}", is.severity.label(), is.where_label(), is.message, if is.overridden.is_some() { "  (overridden)" } else { "" });
                egui::CollapsingHeader::new(RichText::new(head).color(if is.overridden.is_some() { pal.muted } else { col })).id_salt(("issue", k, &is.code)).show(ui, |ui| {
                    ui.label(RichText::new(format!("{}: {}", t("Why"), is.why)));
                    ui.label(RichText::new(format!("{}: {}", t("Fix"), is.fix)));
                    ui.label(RichText::new(format!("code {}", is.code)).small().color(pal.muted));
                    if let Some(reason) = &is.overridden {
                        ui.label(RichText::new(format!("{}: {reason}", t("Reason"))).italics());
                    }
                    ui.horizontal(|ui| {
                        if is.spread_no.is_some() && ui.button(t("Go to")).clicked() {
                            goto = Some(is.clone());
                        }
                        if matches!(is.code.as_str(), "EDGE_SLIVER" | "EMPTY_FRAME" | "TEXT_UNSAFE" | "PAGE_COUNT") && ui.button(format!("🔧 {}", t("Fix"))).clicked() {
                            fix = Some(is.clone());
                        }
                        if is.override_allowed && is.overridden.is_none() && ui.button(t("Override…")).clicked() {
                            dlg.overriding = Some((is.clone(), String::new()));
                        }
                        if is.overridden.is_some() && ui.button(t("Remove override")).clicked() {
                            unoverride = Some(is.clone());
                        }
                    });
                });
            }
        });
        if let Some(is) = goto {
            match is.spread_no {
                Some(0) => ws.go_to(Target::Cover),
                Some(_) => {
                    if let Some(id) = is.spread_id {
                        ws.go_to(Target::Spread(id));
                    }
                }
                None => {}
            }
            if let Some(it) = is.item_id {
                ws.sel = vec![it];
            }
        }
        if let Some(is) = fix {
            auto_fix(ws, &is);
        }
        if let Some(is) = unoverride {
            let mut o = ws.project().settings.overrides.clone();
            o.retain(|x| !(x.code == is.code && x.spread_id == is.spread_id && x.item_id == is.item_id));
            ws.exec(Command::SetOverrides { overrides: o });
            ws.run_preflight();
        }
    });
    if let Some((is, reason)) = &mut dlg.overriding {
        let mut keep = true;
        let mut done = false;
        egui::Window::new(t("Override…")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("{} · {}", is.where_label(), is.message));
            ui.label(RichText::new("Overrides are written to the export audit log and the spec sheet.").small().color(pal.muted));
            ui.add(egui::TextEdit::multiline(reason).hint_text("Why is this acceptable? (e.g. client approved the softer photo)").desired_rows(2));
            ui.horizontal(|ui| {
                if ui.add_enabled(reason.trim().len() >= 4, egui::Button::new(t("Override…").trim_end_matches('…'))).clicked() {
                    done = true;
                }
                if ui.button(t("Cancel")).clicked() {
                    keep = false;
                }
            });
        });
        if done {
            let mut o = ws.project().settings.overrides.clone();
            o.push(PreflightOverride { code: is.code.clone(), spread_id: is.spread_id, item_id: is.item_id, reason: reason.trim().to_string(), at: oto_core::model::now_iso() });
            ws.exec(Command::SetOverrides { overrides: o });
            ws.run_preflight();
            keep = false;
        }
        if !keep {
            dlg.overriding = None;
        }
    }
    dlg.open = open;
}

/// One-click fixes for issues with an unambiguous remedy.
pub fn auto_fix(ws: &mut Workspace, is: &Issue) {
    if is.code == "PAGE_COUNT" {
        let spec = ws.project().spec.clone();
        let mut n = ws.project().spreads.len() as u32;
        while !spec.page_count_ok(n * 2) && n < spec.max_spreads() {
            n += 1;
        }
        let add = n as usize - ws.project().spreads.len();
        for _ in 0..add {
            let at = ws.project().spreads.len();
            ws.exec(Command::InsertSpread { at, spread: Spread::blank() });
        }
        ws.msg(format!("Added {add} blank spread(s) to reach an allowed page count."), MsgKind::Ok);
        return;
    }
    let target = match is.spread_no {
        Some(0) => Target::Cover,
        _ => match is.spread_id {
            Some(id) => Target::Spread(id),
            None => return,
        },
    };
    ws.go_to(target);
    let Some(cur) = ws.current().cloned() else { return };
    let Some(item) = is.item_id else { return };
    let mut s = cur.clone();
    let surf = ws.surface();
    match is.code.as_str() {
        "EMPTY_FRAME" => {
            s.remove_item(item);
        }
        "EDGE_SLIVER" => {
            if let Some(f) = s.frame_mut(item) {
                let e = 3_000.0 / surf.w as f32;
                let ev = 3_000.0 / surf.h as f32;
                let r = &mut f.rect_norm;
                if r.x < e {
                    r.w += r.x;
                    r.x = 0.0;
                }
                if r.y < ev {
                    r.h += r.y;
                    r.y = 0.0;
                }
                if 1.0 - r.right() < e {
                    r.w = 1.0 - r.x;
                }
                if 1.0 - r.bottom() < ev {
                    r.h = 1.0 - r.y;
                }
            }
        }
        "TEXT_UNSAFE" => {
            if let (Some(tb), Target::Spread(_)) = (s.text_mut(item), target) {
                let g = ws.project().spec.geometry();
                let pages = g.safe_pages();
                let c = tb.rect_norm.center();
                let page = if c.x < 0.5 { pages[0] } else { pages[1] };
                let sr = page.to_norm(surf.w, surf.h);
                let r = &mut tb.rect_norm;
                r.w = r.w.min(sr.w);
                r.h = r.h.min(sr.h);
                r.x = r.x.clamp(sr.x, sr.right() - r.w);
                r.y = r.y.clamp(sr.y, sr.bottom() - r.h);
            }
        }
        _ => {}
    }
    ws.commit(s, "Preflight fix");
    ws.run_preflight();
}

// ---------------------------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------------------------

pub struct ExportDlg {
    pub open: bool,
    pub opts: ExportOptions,
    pub ack: bool,
    pub result: Option<Result<ExportResult, String>>,
}

impl ExportDlg {
    pub fn new(ws: &Workspace) -> Self {
        ExportDlg { open: true, opts: ExportOptions::print_default(ws.root.join("exports"), ws.project().spec.binding), ack: false, result: None }
    }
}

pub fn export(ctx: &egui::Context, ws: &mut Workspace, dlg: &mut ExportDlg, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    // Poll the running job.
    if let Some(job) = &mut ws.export {
        let mut finished = None;
        while let Ok(m) = job.rx.try_recv() {
            match m {
                ExportMsg::Progress { done, total, stage } => {
                    job.done = done;
                    job.total = total;
                    job.stage = stage;
                }
                ExportMsg::Done(r) => finished = Some(*r),
            }
        }
        ctx.request_repaint();
        if let Some(r) = finished {
            ws.export = None;
            if let Ok(res) = &r {
                if let Some(f) = res.files.first() {
                    ws.store.send(crate::worker::StoreReq::RecordExport {
                        kind: "print".into(),
                        path: f.path.clone(),
                        preset: dlg.opts.preset.clone(),
                        audit: res.audit_json.clone(),
                        checksums: res.checksums_json.clone(),
                    });
                }
                ws.exported = true;
                ws.msg(format!("{} — {} file(s) in {:.1} s", t("Export finished"), res.files.len(), res.elapsed_ms as f32 / 1000.0), MsgKind::Ok);
            }
            dlg.result = Some(r);
        }
    }
    let mut open = dlg.open;
    egui::Window::new(t("Export")).open(&mut open).default_size([560.0, 600.0]).show(ctx, |ui| {
        let report = ws.preflight.clone();
        let blocking = report.blocking();
        let warns = report.issues.iter().filter(|i| i.needs_ack()).count();
        ui.label(RichText::new(t("Preset")).strong());
        ui.horizontal_wrapped(|ui| {
            let presets: [(&str, fn(&mut ExportOptions)); 4] = [
                ("Print — PDF/X-4", |o| {
                    o.standard = Standard::PdfX4;
                    o.jpeg_per_spread = false;
                    o.otopack = false;
                }),
                ("Print — PDF 1.7", |o| {
                    o.standard = Standard::Pdf17;
                    o.jpeg_per_spread = false;
                }),
                ("Print + JPEG", |o| {
                    o.standard = Standard::PdfX4;
                    o.jpeg_per_spread = true;
                }),
                ("Lab package (.otopack)", |o| {
                    o.standard = Standard::PdfX4;
                    o.otopack = true;
                    o.spec_sheet = true;
                }),
            ];
            for (name, f) in presets {
                if ui.selectable_label(dlg.opts.preset == name, name).clicked() {
                    dlg.opts.preset = name.into();
                    f(&mut dlg.opts);
                }
            }
        });
        ui.separator();
        egui::Grid::new("export-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("PDF");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut dlg.opts.standard, Standard::PdfX4, "PDF/X-4");
                ui.selectable_value(&mut dlg.opts.standard, Standard::Pdf17, "PDF 1.7");
            });
            ui.end_row();
            ui.label(t("Spreads"));
            ui.horizontal(|ui| {
                let n = ws.project().spreads.len();
                let mut all = dlg.opts.range.is_none();
                if ui.checkbox(&mut all, t("All")).changed() {
                    dlg.opts.range = if all { None } else { Some((1, n)) };
                }
                if let Some((a, b)) = &mut dlg.opts.range {
                    ui.add(egui::DragValue::new(a).range(1..=n));
                    ui.label("–");
                    ui.add(egui::DragValue::new(b).range(1..=n));
                }
            });
            ui.end_row();
            ui.label("Layout");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut dlg.opts.single_pages, false, "Spreads");
                ui.selectable_value(&mut dlg.opts.single_pages, true, t("Single pages"));
            });
            ui.end_row();
            ui.label("");
            ui.vertical(|ui| {
                ui.add_enabled(ws.project().cover.is_some(), egui::Checkbox::new(&mut dlg.opts.include_cover, t("Include cover")));
                ui.checkbox(&mut dlg.opts.crop_marks, t("Crop marks"));
                ui.checkbox(&mut dlg.opts.spec_sheet, t("Spec sheet"));
                ui.checkbox(&mut dlg.opts.jpeg_per_spread, t("JPEG per spread"));
                ui.checkbox(&mut dlg.opts.tiff_per_spread, t("TIFF per spread"));
                ui.checkbox(&mut dlg.opts.otopack, t("Lab package (.otopack)"));
            });
            ui.end_row();
            ui.label(t("Image quality"));
            ui.vertical(|ui| {
                let mut keep = dlg.opts.jpeg_quality.is_none();
                if ui.checkbox(&mut keep, t("Keep originals")).on_hover_text("JPEG originals are embedded byte-for-byte; other formats are embedded lossless.").changed() {
                    dlg.opts.jpeg_quality = if keep { None } else { Some(92) };
                }
                if let Some(q) = &mut dlg.opts.jpeg_quality {
                    ui.add(egui::Slider::new(q, 60..=100).text("JPEG"));
                }
                let mut ds = dlg.opts.downsample_dpi.is_some();
                if ui.checkbox(&mut ds, t("Downsample")).changed() {
                    dlg.opts.downsample_dpi = if ds { Some(400) } else { None };
                }
                if let Some(d) = &mut dlg.opts.downsample_dpi {
                    ui.add(egui::Slider::new(d, 200..=600).text("dpi"));
                }
            });
            ui.end_row();
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut dlg.opts.naming).hint_text("{project}_{date}")).on_hover_text("{project} {client} {date} {lab}");
            ui.end_row();
            ui.label(t("Output folder"));
            ui.horizontal(|ui| {
                ui.label(RichText::new(dlg.opts.out_dir.display().to_string()).small());
                if ui.button(t("Choose…")).clicked() {
                    if let Some(p) = oto_platform::native().pick_folder("Export to") {
                        dlg.opts.out_dir = p;
                    }
                }
            });
            ui.end_row();
        });
        ui.separator();
        if !blocking.is_empty() {
            ui.label(RichText::new(format!("✖ {} — {} critical issue(s):", t("Export blocked"), blocking.len())).color(pal.crit).strong());
            for b in blocking.iter().take(6) {
                ui.label(RichText::new(format!("• {} · {}", b.where_label(), b.message)).small());
            }
            if ui.button(t("Preflight")).clicked() {
                ws.requests.push(Request::Preflight);
            }
        } else if warns > 0 {
            ui.checkbox(&mut dlg.ack, format!("{} ({warns})", t("I have reviewed the warnings")));
        } else {
            ui.label(RichText::new(format!("✔ {}", t("No issues. Ready to print."))).color(pal.ok));
        }
        ui.label(RichText::new("PDF/X-4 here means: PDF 1.6, output intent with embedded ICC, Trim/Bleed boxes, embedded subset fonts, XMP identification — checked by oto-albums after export. It is not a third-party certification.").small().color(pal.muted));
        ui.add_space(6.0);
        let running = ws.export.is_some();
        if let Some(job) = &ws.export {
            let frac = if job.total == 0 { 0.0 } else { job.done as f32 / job.total as f32 };
            ui.add(egui::ProgressBar::new(frac).text(format!("{} {}", t("Exporting"), job.stage)));
            if ui.button(t("Cancel")).clicked() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        } else if ui.add_enabled(blocking.is_empty() && (warns == 0 || dlg.ack), egui::Button::new(RichText::new(format!("⇪ {}", t("Export"))).strong()).min_size(Vec2::new(140.0, 34.0))).clicked() {
            start_export(ws, dlg);
        }
        if !running {
            if let Some(r) = &dlg.result {
                ui.separator();
                match r {
                    Ok(res) => {
                        ui.label(RichText::new(format!("✔ {}", t("Export finished"))).color(pal.ok).strong());
                        if let Some(c) = &res.check {
                            if c.ok {
                                ui.label(RichText::new(format!("Self-check passed: {} pages, {} font(s), {} image(s), PDF {}", c.pages, c.fonts, c.images, c.version)).small());
                            } else {
                                for p in &c.problems {
                                    ui.label(RichText::new(format!("⚠ {p}")).small().color(pal.warn));
                                }
                            }
                        }
                        for f in &res.files {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{} · {:.1} MB", f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), f.bytes as f32 / 1_048_576.0)).small());
                                if ui.small_button(t("Reveal")).clicked() {
                                    let _ = oto_platform::native().reveal(&f.path);
                                }
                            });
                        }
                        for w in &res.warnings {
                            ui.label(RichText::new(format!("⚠ {w}")).small().color(pal.warn));
                        }
                    }
                    Err(e) => {
                        ui.label(RichText::new(format!("✖ {e}")).color(pal.crit));
                    }
                }
            }
        }
    });
    dlg.open = open;
}

fn start_export(ws: &mut Workspace, dlg: &mut ExportDlg) {
    ws.refresh_status();
    ws.run_preflight();
    let media: HashMap<MediaId, _> = ws.media.iter().map(|m| (m.id, m.clone())).collect();
    let input = ExportInput { project: ws.project().clone(), media, lab: ws.lab.clone(), preflight: ws.preflight.clone(), warnings_acknowledged: dlg.ack, options: dlg.opts.clone() };
    let cancel = Arc::new(AtomicBool::new(false));
    let rx = oto_print::export::spawn(input, cancel.clone());
    ws.export = Some(ExportJob { rx, cancel, done: 0, total: 1, stage: String::new() });
    dlg.result = None;
}

// ---------------------------------------------------------------------------------------------
// Proof
// ---------------------------------------------------------------------------------------------

pub struct ProofDlg {
    pub open: bool,
    pub opts: oto_proof::local::ProofOptions,
    pub rx: Option<Receiver<Result<oto_proof::local::ProofResult, String>>>,
    pub result: Option<Result<oto_proof::local::ProofResult, String>>,
}

impl ProofDlg {
    pub fn new(ws: &Workspace) -> Self {
        ProofDlg { open: true, opts: oto_proof::local::ProofOptions::new(oto_proof::local::default_proof_dir(&ws.root)), rx: None, result: None }
    }
}

pub fn proof(ctx: &egui::Context, ws: &mut Workspace, dlg: &mut ProofDlg, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    if let Some(rx) = &dlg.rx {
        if let Ok(r) = rx.try_recv() {
            if let Ok(res) = &r {
                ws.store.send(crate::worker::StoreReq::RecordProof { path: res.pdf.clone(), note: dlg.opts.note.clone() });
                ws.msg(format!("Proof ready: {} pages", res.pages), MsgKind::Ok);
            }
            dlg.result = Some(r);
            dlg.rx = None;
        } else {
            ctx.request_repaint();
        }
    }
    let mut open = dlg.open;
    egui::Window::new(t("Local proof")).open(&mut open).default_size([460.0, 360.0]).show(ctx, |ui| {
        ui.label("A PDF for your client: every spread with its number, numbered photos, the fold line and a watermark, plus a comment sheet. Made offline from previews — nothing is uploaded.");
        ui.add(egui::TextEdit::singleline(&mut dlg.opts.watermark).hint_text("Watermark"));
        ui.add(egui::TextEdit::singleline(&mut dlg.opts.note).hint_text("Revision note (optional)"));
        ui.add(egui::Slider::new(&mut dlg.opts.dpi, 72.0..=200.0).text("dpi"));
        ui.checkbox(&mut dlg.opts.include_cover, t("Include cover"));
        ui.checkbox(&mut dlg.opts.jpgs, "Also save JPG per spread");
        if dlg.rx.is_some() {
            ui.spinner();
        } else if ui.button(RichText::new(t("Make proof")).strong()).clicked() {
            let p = ws.project().clone();
            let media: HashMap<MediaId, _> = ws.media.iter().map(|m| (m.id, m.clone())).collect();
            let cache = oto_media::CacheLayout::new(ws.root.join("cache"));
            let cg = if p.cover.is_some() { Some(ws.cover_geometry()) } else { None };
            let o = dlg.opts.clone();
            let (tx, rx) = unbounded();
            std::thread::spawn(move || {
                let _ = tx.send(oto_proof::local::export_local_proof(&p, &media, &cache, cg, &o));
            });
            dlg.rx = Some(rx);
        }
        if let Some(r) = &dlg.result {
            match r {
                Ok(res) => {
                    ui.label(RichText::new(format!("✔ {}", res.pdf.display())).small().color(pal.ok));
                    ui.horizontal(|ui| {
                        if ui.button(t("Reveal")).clicked() {
                            let _ = oto_platform::native().reveal(&res.pdf);
                        }
                        if ui.button("Open").clicked() {
                            let _ = oto_platform::native().open(&res.pdf);
                        }
                    });
                }
                Err(e) => {
                    ui.label(RichText::new(format!("✖ {e}")).color(pal.crit));
                }
            }
        }
        ui.separator();
        ui.label(RichText::new("Web proofing (share link, anchored comments, approval) is planned for v1.1; its API contract is in docs/web-proof-api.md.").small().color(pal.muted));
    });
    dlg.open = open;
}

// ---------------------------------------------------------------------------------------------
// Snapshots / recovery
// ---------------------------------------------------------------------------------------------

pub fn snapshots(ctx: &egui::Context, ws: &mut Workspace, open: &mut bool, confirm: &mut Option<PathBuf>) {
    egui::Window::new(t("Snapshots")).open(open).default_size([520.0, 420.0]).show(ctx, |ui| {
        ui.label("Snapshots are full copies of the project database. oto-albums takes one before big changes (Auto Build, deleting spreads, changing size) and every few minutes; the last 10 automatic ones are kept.");
        if ui.button(t("Take snapshot")).clicked() {
            ws.store.send(crate::worker::StoreReq::Snapshot { reason: "manual".into(), manual: true });
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for s in ws.snapshots.clone() {
                ui.horizontal(|ui| {
                    ui.label(format!("{} · {}{}", crate::local_stamp(&s.created_at), s.reason, if s.manual { " (manual)" } else { "" }));
                    if ui.small_button(t("Restore")).clicked() {
                        *confirm = Some(s.path.clone());
                    }
                });
            }
            if ws.snapshots.is_empty() {
                ui.label("No snapshots yet.");
            }
        });
    });
    if let Some(p) = confirm.clone() {
        egui::Window::new(t("Restore")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label("Restore this snapshot? The current state is saved as a snapshot first, so you can come back.");
            ui.horizontal(|ui| {
                if ui.button(t("Restore")).clicked() {
                    ws.store.send(crate::worker::StoreReq::Restore(p.clone()));
                    *confirm = None;
                }
                if ui.button(t("Cancel")).clicked() {
                    *confirm = None;
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Relink
// ---------------------------------------------------------------------------------------------

pub struct RelinkDlg {
    pub open: bool,
    pub rx: Option<Receiver<Vec<oto_media::RelinkMatch>>>,
    pub matches: Vec<oto_media::RelinkMatch>,
    pub choice: HashMap<MediaId, Option<PathBuf>>,
}

pub fn relink(ctx: &egui::Context, ws: &mut Workspace, dlg: &mut RelinkDlg, settings: &AppSettings) {
    let pal = theme::palette(settings.theme);
    if let Some(rx) = &dlg.rx {
        if let Ok(m) = rx.try_recv() {
            for x in &m {
                dlg.choice.insert(x.media_id, x.exact.clone());
            }
            dlg.matches = m;
            dlg.rx = None;
        } else {
            ctx.request_repaint();
        }
    }
    let mut open = dlg.open;
    egui::Window::new(t("Relink…")).open(&mut open).default_size([560.0, 440.0]).show(ctx, |ui| {
        let missing: Vec<_> = ws.media.iter().filter(|m| m.missing).cloned().collect();
        ui.label(format!("{} original(s) are missing. Choose the folder where they are now; files are matched by name and verified by content hash.", missing.len()));
        if ui.add_enabled(dlg.rx.is_none(), egui::Button::new(t("Choose…"))).clicked() {
            if let Some(dir) = oto_platform::native().pick_folder("Folder with the originals") {
                let (tx, rx) = unbounded();
                std::thread::spawn(move || {
                    let _ = tx.send(oto_media::find_relinks(&missing, &dir));
                });
                dlg.rx = Some(rx);
            }
        }
        if dlg.rx.is_some() {
            ui.spinner();
        }
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for m in &dlg.matches {
                let name = ws.media(m.media_id).map(|x| x.file_name.clone()).unwrap_or_default();
                ui.horizontal(|ui| match &m.exact {
                    Some(p) => {
                        ui.label(RichText::new(format!("✔ {name} → {}", p.display())).small().color(pal.ok));
                    }
                    None if !m.candidates.is_empty() => {
                        ui.label(RichText::new(format!("? {name}: same name, different content —")).small().color(pal.warn));
                        let cur = dlg.choice.entry(m.media_id).or_insert(None);
                        egui::ComboBox::from_id_salt(("relink", m.media_id)).selected_text(cur.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "skip".into())).show_ui(ui, |ui| {
                            ui.selectable_value(cur, None, "skip");
                            for c in &m.candidates {
                                ui.selectable_value(cur, Some(c.clone()), c.display().to_string());
                            }
                        });
                    }
                    None => {
                        ui.label(RichText::new(format!("✖ {name}: not found")).small().color(pal.crit));
                    }
                });
            }
        });
        let n = dlg.choice.values().filter(|v| v.is_some()).count();
        if n > 0 && ui.button(format!("{} ({n})", t("Apply"))).clicked() {
            let cache = oto_media::CacheLayout::new(ws.root.join("cache"));
            let mut ok = 0;
            for (id, p) in dlg.choice.clone() {
                let Some(p) = p else { continue };
                // Re-import metadata from the new location (hash may differ for a confirmed candidate).
                if let Ok(fresh) = oto_media::import_one(&p, &cache) {
                    if let Some(m) = ws.media_mut(id) {
                        let keep = (m.id, m.rating, m.color_label, m.rejected, m.flagged, m.client_pick, m.bins.clone(), m.focus);
                        *m = fresh;
                        (m.id, m.rating, m.color_label, m.rejected, m.flagged, m.client_pick, m.bins, m.focus) = keep;
                        ok += 1;
                    }
                }
            }
            ws.refresh_status();
            ws.msg(format!("Relinked {ok} photo(s)."), MsgKind::Ok);
            dlg.matches.clear();
            dlg.choice.clear();
        }
    });
    dlg.open = open;
}

// ---------------------------------------------------------------------------------------------
// Project info & album settings
// ---------------------------------------------------------------------------------------------

pub struct MetaDlg {
    pub open: bool,
    pub name: String,
    pub client: String,
    pub due: String,
    pub status: ProjectStatus,
}

pub fn meta(ctx: &egui::Context, ws: &mut Workspace, dlg: &mut MetaDlg) {
    let mut open = dlg.open;
    let mut save = false;
    egui::Window::new(t("Project info")).open(&mut open).collapsible(false).show(ctx, |ui| {
        egui::Grid::new("meta").num_columns(2).show(ui, |ui| {
            ui.label(t("Project name"));
            ui.text_edit_singleline(&mut dlg.name);
            ui.end_row();
            ui.label(t("Client"));
            ui.text_edit_singleline(&mut dlg.client);
            ui.end_row();
            ui.label(t("Due"));
            ui.add(egui::TextEdit::singleline(&mut dlg.due).hint_text("YYYY-MM-DD"));
            ui.end_row();
            ui.label(t("Status"));
            egui::ComboBox::from_id_salt("status").selected_text(dlg.status.key()).show_ui(ui, |ui| {
                for s in ProjectStatus::ALL {
                    ui.selectable_value(&mut dlg.status, s, s.key());
                }
            });
            ui.end_row();
        });
        let due_ok = dlg.due.trim().is_empty() || oto_core::solver::parse_time(&format!("{}T00:00:00", dlg.due.trim())).is_some();
        if !due_ok {
            ui.label(RichText::new("Use the date format YYYY-MM-DD.").color(theme::AMBER));
        }
        if ui.add_enabled(due_ok, egui::Button::new(t("Save"))).clicked() {
            save = true;
        }
    });
    if save {
        let due = if dlg.due.trim().is_empty() { None } else { Some(dlg.due.trim().to_string()) };
        ws.exec(Command::SetMeta { name: dlg.name.clone(), client: dlg.client.clone(), due_date: due, status: dlg.status });
        open = false;
    }
    dlg.open = open;
}

pub struct SpecDlg {
    pub open: bool,
    pub spec: AlbumSpec,
    pub icc_err: Option<String>,
}

pub fn album_settings(ctx: &egui::Context, ws: &mut Workspace, dlg: &mut SpecDlg) {
    let mut open = dlg.open;
    let mut apply = false;
    egui::Window::new(t("Album settings")).open(&mut open).default_size([460.0, 520.0]).show(ctx, |ui| {
        crate::wizard::spec_editor(ui, &mut dlg.spec);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(t("Output profile"));
            ui.label(&dlg.spec.output_profile.name);
            if ui.button("Use ICC file…").clicked() {
                if let Some(p) = oto_platform::native().pick_file("Output ICC profile", &["icc", "icm"]) {
                    match std::fs::read(&p) {
                        Ok(b) if b.len() > 128 && &b[36..40] == b"acsp" && &b[16..20] == b"RGB " => {
                            dlg.spec.output_profile =
                                oto_core::spec::ColorProfile { name: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), icc_path: Some(p.display().to_string()) };
                            dlg.icc_err = None;
                        }
                        Ok(_) => dlg.icc_err = Some("Not an RGB ICC profile. oto-albums v1 exports RGB only.".into()),
                        Err(e) => dlg.icc_err = Some(e.to_string()),
                    }
                }
            }
            if dlg.spec.output_profile.icc_path.is_some() && ui.button("Built-in sRGB").clicked() {
                dlg.spec.output_profile = Default::default();
            }
        });
        if let Some(e) = &dlg.icc_err {
            ui.label(RichText::new(e).color(theme::AMBER));
        }
        let issues = dlg.spec.validate();
        for i in &issues {
            ui.label(RichText::new(format!("⚠ {}", i.message)).color(theme::AMBER));
        }
        ui.label(RichText::new("Layouts are stored relative to the page, so they scale to the new size. A snapshot is taken first.").small());
        if ui.add_enabled(issues.iter().all(|i| i.field == "bleed" && dlg.spec.bleed_um == 0), egui::Button::new(t("Apply"))).clicked() {
            apply = true;
        }
    });
    if apply {
        dlg.spec.normalize();
        if ws.exec(Command::SetSpec { spec: dlg.spec.clone() }) {
            ws.view.fit_pending = true;
            open = false;
        }
    }
    dlg.open = open;
}

// ---------------------------------------------------------------------------------------------
// Save as template
// ---------------------------------------------------------------------------------------------

pub fn spread_to_template(s: &Spread, name: &str, kind: TemplateKind) -> Template {
    let id = format!("custom-{}", oto_core::model::new_id().simple());
    let frames: Vec<TemplateFrame> = s
        .frames
        .iter()
        .enumerate()
        .map(|(i, f)| TemplateFrame {
            id: format!("f{}", i + 1),
            x: f.rect_norm.x,
            y: f.rect_norm.y,
            w: f.rect_norm.w,
            h: f.rect_norm.h,
            region: Region::Surface,
            role: f.role.clone(),
            min_aspect: 0.2,
            max_aspect: 5.0,
            allow_rotate: f.allow_rotate,
            z: f.z,
        })
        .collect();
    let texts = s
        .texts
        .iter()
        .map(|t| TemplateText {
            x: t.rect_norm.x,
            y: t.rect_norm.y,
            w: t.rect_norm.w,
            h: t.rect_norm.h,
            region: Region::Surface,
            preset: oto_core::model::StylePreset::Body,
            placeholder: t.text.clone(),
            rotation_mdeg: t.rotation_mdeg,
        })
        .collect();
    let crosses = s.frames.iter().any(|f| f.rect_norm.x < 0.499 && f.rect_norm.right() > 0.501);
    let n = frames.len() as u32;
    Template {
        id,
        name: name.into(),
        family: "custom".into(),
        kind,
        photo_count: n,
        orientation: String::new(),
        tags: vec!["custom".into()],
        style: "any".into(),
        gutter_policy: if crosses && kind == TemplateKind::Spread { GutterPolicy::NoFace } else { GutterPolicy::Safe },
        version: 1,
        custom: true,
        frames,
        texts,
    }
}

pub fn save_template(ctx: &egui::Context, ws: &mut Workspace, name: &mut Option<String>) {
    let Some(n) = name.as_mut() else { return };
    let mut close = false;
    egui::Window::new(t("Save as template")).collapsible(false).show(ctx, |ui| {
        ui.text_edit_singleline(n);
        ui.horizontal(|ui| {
            if ui.add_enabled(!n.trim().is_empty(), egui::Button::new(t("Save"))).clicked() {
                if let Some(s) = ws.current().cloned() {
                    let kind = if matches!(ws.target, Target::Cover) { TemplateKind::Cover } else { TemplateKind::Spread };
                    let tpl = spread_to_template(&s, n.trim(), kind);
                    let errs = tpl.validate();
                    if errs.is_empty() || errs.iter().all(|e| e.contains("overlap")) {
                        let toml = tpl.to_toml();
                        let dir = oto_platform::app_data_dir().join("templates");
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::fs::write(dir.join(format!("{}.toml", tpl.id)), &toml);
                        ws.store.send(crate::worker::StoreReq::SaveTemplate { id: tpl.id.clone(), toml, version: 1 });
                        ws.templates.push(tpl);
                        ws.tpl_cache.clear();
                        ws.msg("Template saved — find it in the Templates tab.", MsgKind::Ok);
                        close = true;
                    } else {
                        ws.msg(format!("Template not saved: {}", errs.join("; ")), MsgKind::Warn);
                    }
                }
            }
            if ui.button(t("Cancel")).clicked() {
                close = true;
            }
        });
    });
    if close {
        *name = None;
    }
}

// ---------------------------------------------------------------------------------------------
// Command palette & shortcuts
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    Close,
    ImportFolder,
    ImportFiles,
    AutoBuild,
    Preflight,
    Export,
    Proof,
    Snapshots,
    Snapshot,
    Relink,
    AlbumSettings,
    ProjectInfo,
    AddSpread,
    DeleteSpread,
    LockSpread,
    Cover,
    SaveTemplate,
    ToggleLeft,
    ToggleRight,
    ToggleBottom,
    Focus,
    FullScreen,
    Rulers,
    Grid,
    Guides,
    Shortcuts,
    Settings,
    Undo,
    Redo,
    LangEn,
    LangMs,
    Sample,
    Consolidate,
    ChooseTemplates,
}

pub fn commands() -> Vec<(Cmd, &'static str, &'static str)> {
    vec![
        (Cmd::New, "New project", "⌘N"),
        (Cmd::Open, "Open project…", "⌘O"),
        (Cmd::Save, "Save", "⌘S"),
        (Cmd::SaveAs, "Save as…", "⇧⌘S"),
        (Cmd::Close, "Close project", ""),
        (Cmd::ImportFolder, "Import folder…", "⌘I"),
        (Cmd::ImportFiles, "Import files…", ""),
        (Cmd::AutoBuild, "Auto Build…", "⌘B"),
        (Cmd::Preflight, "Preflight", "⌘P"),
        (Cmd::Export, "Export", "⌘E"),
        (Cmd::Proof, "Local proof", ""),
        (Cmd::Snapshots, "Snapshots", ""),
        (Cmd::Snapshot, "Take snapshot", ""),
        (Cmd::Relink, "Relink…", ""),
        (Cmd::Consolidate, "Consolidate originals into project", ""),
        (Cmd::AlbumSettings, "Album settings", ""),
        (Cmd::ProjectInfo, "Project info", ""),
        (Cmd::AddSpread, "Add spread", ""),
        (Cmd::DeleteSpread, "Delete spread", ""),
        (Cmd::LockSpread, "Lock spread", ""),
        (Cmd::Cover, "Edit cover", ""),
        (Cmd::ChooseTemplates, "Choose templates", ""),
        (Cmd::SaveTemplate, "Save as template", ""),
        (Cmd::ToggleLeft, "Toggle book map", "⌘1"),
        (Cmd::ToggleRight, "Toggle inspector", "⌘2"),
        (Cmd::ToggleBottom, "Toggle timeline", "⌘3"),
        (Cmd::Focus, "Focus mode", "Tab"),
        (Cmd::FullScreen, "Full screen", "F"),
        (Cmd::Rulers, "Rulers", ""),
        (Cmd::Grid, "Grid", ""),
        (Cmd::Guides, "Guides", ""),
        (Cmd::Undo, "Undo", "⌘Z"),
        (Cmd::Redo, "Redo", "⇧⌘Z"),
        (Cmd::Shortcuts, "Keyboard shortcuts", "?"),
        (Cmd::Settings, "Settings", "⌘,"),
        (Cmd::LangEn, "Language: English", ""),
        (Cmd::LangMs, "Bahasa: Melayu", ""),
        (Cmd::Sample, "Open sample project", ""),
    ]
}

pub struct Palette {
    pub query: String,
    pub cursor: usize,
}

/// Returns a command when the user picks one.
pub fn palette(ctx: &egui::Context, p: &mut Palette, have_project: bool) -> (Option<Cmd>, bool) {
    let mut chosen = None;
    let mut close = false;
    let q = p.query.to_lowercase();
    let list: Vec<(Cmd, &str, &str)> = commands()
        .into_iter()
        .filter(|(c, _, _)| have_project || matches!(c, Cmd::New | Cmd::Open | Cmd::Settings | Cmd::Shortcuts | Cmd::LangEn | Cmd::LangMs | Cmd::Sample | Cmd::FullScreen))
        .filter(|(_, l, _)| {
            let tl = t(l).to_lowercase();
            q.split_whitespace().all(|w| l.to_lowercase().contains(w) || tl.contains(w))
        })
        .collect();
    egui::Window::new(t("Command palette")).collapsible(false).resizable(false).title_bar(false).anchor(egui::Align2::CENTER_TOP, [0.0, 80.0]).fixed_size([460.0, 380.0]).show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut p.query).hint_text("Type a command…").desired_width(f32::INFINITY));
        r.request_focus();
        let (down, up, enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if down {
            p.cursor = (p.cursor + 1).min(list.len().saturating_sub(1));
        }
        if up {
            p.cursor = p.cursor.saturating_sub(1);
        }
        if p.cursor >= list.len() {
            p.cursor = 0;
        }
        if enter {
            chosen = list.get(p.cursor).map(|x| x.0);
        }
        if esc {
            close = true;
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, (c, l, k)) in list.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.selectable_label(i == p.cursor, t(l)).clicked() {
                        chosen = Some(*c);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(*k).small().weak());
                    });
                });
            }
        });
    });
    (chosen, close)
}

pub fn shortcuts(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(t("Keyboard shortcuts")).open(open).default_size([440.0, 520.0]).show(ctx, |ui| {
        let rows = [
            ("⌘N / ⌘O / ⌘S / ⇧⌘S", "New, open, save, save as"),
            ("⌘Z / ⇧⌘Z", "Undo / redo (100 steps)"),
            ("⌘K", "Command palette"),
            ("Tab", "Focus mode (hide panels)"),
            ("F", "Full screen"),
            ("⌘1 / ⌘2 / ⌘3", "Toggle book map / inspector / timeline"),
            ("A / Q", "Previous / next template (same photo count)"),
            ("S / W", "Previous / next template family"),
            ("Space (on a template)", "Compare side by side"),
            ("[ / ] / 0 / 1", "Zoom out / in / fit / physical size"),
            ("Space + drag", "Pan the canvas"),
            ("V / R / T", "Select / frame tool / text tool"),
            ("C", "Crop mode for the selected photo"),
            ("Alt-click (crop mode)", "Set focus point"),
            ("⌘-drag a photo", "Swap with the photo it is dropped on"),
            ("Delete", "Remove selection"),
            ("⌘D", "Duplicate"),
            ("Arrows / Shift+Arrows", "Nudge 1 px / 10 px"),
            ("⌘← / ⌘→", "Previous / next spread"),
            ("0–5, X, P (photo grid)", "Rate, reject, pick"),
            ("Esc", "Cancel / deselect"),
            ("?", "This list"),
        ];
        egui::Grid::new("keys").striped(true).num_columns(2).show(ui, |ui| {
            for (k, d) in rows {
                ui.label(RichText::new(k).monospace());
                ui.label(d);
                ui.end_row();
            }
        });
        ui.label(RichText::new("On Windows use Ctrl instead of ⌘.").small().weak());
    });
}

pub fn settings_window(ctx: &egui::Context, open: &mut bool, s: &mut AppSettings, labs_note: &str) -> bool {
    let mut changed = false;
    egui::Window::new(t("Settings")).open(open).default_size([420.0, 460.0]).show(ctx, |ui| {
        egui::Grid::new("settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(t("Language"));
            ui.horizontal(|ui| {
                changed |= ui.selectable_value(&mut s.lang, Lang::En, Lang::En.label()).changed();
                changed |= ui.selectable_value(&mut s.lang, Lang::Ms, Lang::Ms.label()).changed();
            });
            ui.end_row();
            ui.label(t("Theme"));
            ui.horizontal(|ui| {
                changed |= ui.selectable_value(&mut s.theme, crate::theme::ThemeMode::Dark, t("Dark")).changed();
                changed |= ui.selectable_value(&mut s.theme, crate::theme::ThemeMode::Light, t("Light")).changed();
                changed |= ui.selectable_value(&mut s.theme, crate::theme::ThemeMode::HighContrast, t("High contrast")).changed();
            });
            ui.end_row();
            ui.label(t("UI scale"));
            changed |= ui.add(egui::Slider::new(&mut s.ui_scale, 0.8..=1.6)).changed();
            ui.end_row();
            ui.label(t("Units"));
            ui.horizontal(|ui| {
                for u in Units::ALL {
                    changed |= ui.selectable_value(&mut s.units, u, u.label()).changed();
                }
            });
            ui.end_row();
            ui.label("");
            changed |= ui.checkbox(&mut s.reduced_motion, t("Reduce motion")).changed();
            ui.end_row();
            ui.label(t("Guides"));
            ui.vertical(|ui| {
                changed |= ui.checkbox(&mut s.show_rulers, t("Rulers")).changed();
                changed |= ui.checkbox(&mut s.show_grid, t("Grid")).changed();
                changed |= ui.checkbox(&mut s.show_guides, "Bleed / safe / gutter").changed();
            });
            ui.end_row();
        });
        ui.separator();
        ui.label(RichText::new("Lab profiles").strong());
        ui.label(RichText::new(labs_note).small());
        ui.horizontal(|ui| {
            if ui.button("Open lab profiles folder").clicked() {
                let d = oto_platform::app_data_dir().join("labs");
                let _ = std::fs::create_dir_all(&d);
                let _ = oto_platform::native().open(&d);
            }
            if ui.button("Register .otoalbum files").clicked() {
                let exe = std::env::current_exe().unwrap_or_default();
                match oto_platform::native().register_file_association(&exe) {
                    Ok(m) => tracing::info!("{m}"),
                    Err(e) => tracing::warn!("{e}"),
                }
            }
        });
        ui.separator();
        ui.label(RichText::new(format!("oto-albums {} · works fully offline · no account · no telemetry", env!("CARGO_PKG_VERSION"))).small());
        ui.label(RichText::new("Fonts: Inter, Lora, Playfair Display, Cormorant Garamond, Montserrat, Great Vibes — SIL Open Font License 1.1.").small().weak());
    });
    changed
}

pub fn lab_label(l: &LabProfile) -> String {
    let tag = l.is_example() && !l.name.to_lowercase().contains("example");
    format!("{}{}", l.name, if tag { " · example" } else { "" })
}

pub fn spec_from_lab(l: &LabProfile, size: &str) -> Option<AlbumSpec> {
    l.size(size).map(|s| l.spec_for_size(s))
}

pub fn binding_label(b: Binding) -> &'static str {
    match b {
        Binding::Layflat => t("Layflat (spreads)"),
        Binding::SinglePages => t("Single pages"),
    }
}
