//! oto-ui: the egui application shell.
#![forbid(unsafe_code)]
// Render and layout helpers take many plain geometric parameters by design (see DECISIONS.md).
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod bookmap;
pub mod bottom;
pub mod canvas;
pub mod dialogs;
pub mod draw;
pub mod hub;
pub mod i18n;
pub mod inspector;
pub mod picker;
pub mod settings;
pub mod state;
pub mod textures;
pub mod theme;
pub mod wizard;
pub mod worker;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Key, RichText, Vec2};
use oto_core::lab::LabProfile;
use oto_core::template::Template;
use oto_media::ImportMsg;
use oto_store::Package;

use crate::dialogs::{AutoBuildDlg, Cmd, ExportDlg, MetaDlg, PreflightDlg, ProofDlg, RelinkDlg, SpecDlg};
use crate::i18n::t;
use crate::settings::{AppSettings, RecentProject};
use crate::state::{ImportJob, MsgKind, Request, SaveState, Target, Workspace};
use crate::worker::{StoreReq, StoreWorker};

struct Toast {
    text: String,
    kind: MsgKind,
    at: Instant,
}

pub struct OtoApp {
    settings: AppSettings,
    labs: Vec<LabProfile>,
    lab_note: String,
    ws: Option<Workspace>,
    wizard: Option<wizard::Wizard>,
    hub_search: String,
    toasts: Vec<Toast>,
    auto_build: Option<AutoBuildDlg>,
    preflight: PreflightDlg,
    export: Option<ExportDlg>,
    proof: Option<ProofDlg>,
    snapshots_open: bool,
    snapshot_confirm: Option<PathBuf>,
    relink: Option<RelinkDlg>,
    meta: Option<MetaDlg>,
    spec: Option<SpecDlg>,
    save_template: Option<String>,
    palette: Option<dialogs::Palette>,
    shortcuts_open: bool,
    settings_open: bool,
    quit_after_close: bool,
    closing_since: Option<Instant>,
    fullscreen: bool,
    pending_open: Option<PathBuf>,
    applied_scale: f32,
}

impl OtoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        let settings = AppSettings::load();
        i18n::set_lang(settings.lang);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, settings.theme, settings.reduced_motion);
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let (labs, errs) = oto_print::labs::all(Some(&oto_platform::app_data_dir().join("labs")));
        let lab_note = if errs.is_empty() {
            format!("{} profile(s) loaded. Add your lab's spec as a TOML file in the lab profiles folder.", labs.len())
        } else {
            format!("{} loaded; {} file(s) rejected: {}", labs.len(), errs.len(), errs.join("; "))
        };
        OtoApp {
            applied_scale: settings.ui_scale,
            settings,
            labs,
            lab_note,
            ws: None,
            wizard: None,
            hub_search: String::new(),
            toasts: Vec::new(),
            auto_build: None,
            preflight: PreflightDlg { open: false, filter: None, overriding: None },
            export: None,
            proof: None,
            snapshots_open: false,
            snapshot_confirm: None,
            relink: None,
            meta: None,
            spec: None,
            save_template: None,
            palette: None,
            shortcuts_open: false,
            settings_open: false,
            quit_after_close: false,
            closing_since: None,
            fullscreen: false,
            pending_open: open,
        }
    }

    fn toast(&mut self, text: impl Into<String>, kind: MsgKind) {
        let text = text.into();
        match kind {
            MsgKind::Error => tracing::error!("{text}"),
            MsgKind::Warn => tracing::warn!("{text}"),
            _ => tracing::info!("{text}"),
        }
        self.toasts.push(Toast { text, kind, at: Instant::now() });
    }

    fn user_templates() -> Vec<Template> {
        let dir = oto_platform::app_data_dir().join("templates");
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if let Ok(s) = std::fs::read_to_string(e.path()) {
                    if let Ok(t) = Template::from_toml(&s) {
                        v.push(t);
                    }
                }
            }
        }
        v
    }

    fn open_project(&mut self, path: &Path) {
        if let Some(ws) = &mut self.ws {
            if ws.root == path {
                return;
            }
            ws.close();
        }
        let (pkg, report) = match Package::open(path) {
            Ok(x) => x,
            Err(e) => {
                self.toast(format!("Cannot open {}: {e}", path.display()), MsgKind::Error);
                return;
            }
        };
        let (project, media) = match pkg.load() {
            Ok(x) => x,
            Err(e) => {
                self.toast(format!("Cannot read project: {e}"), MsgKind::Error);
                return;
            }
        };
        let lab =
            project.lab_id.as_ref().and_then(|id| self.labs.iter().find(|l| &l.id == id).cloned().or_else(|| pkg.lab_profile_toml(id).ok().flatten().and_then(|s| LabProfile::from_toml(&s).ok())));
        let mut templates = oto_core::builtin::all_templates();
        for s in pkg.templates().unwrap_or_default() {
            if let Ok(t) = Template::from_toml(&s) {
                if !templates.iter().any(|x| x.id == t.id) {
                    templates.push(t);
                }
            }
        }
        for t in Self::user_templates() {
            if !templates.iter().any(|x| x.id == t.id) {
                templates.push(t);
            }
        }
        let worker = StoreWorker::spawn(pkg);
        let mut ws = Workspace::new(path.to_path_buf(), project, media, worker, lab, templates);
        ws.store.send(StoreReq::ListSnapshots);
        if report.unclean_shutdown {
            ws.recovery_note = Some(match &report.last_commit_at {
                Some(at) => format!("oto-albums did not close properly last time. Your work was recovered from the last autosave ({}).", local_stamp(at)),
                None => "oto-albums did not close properly last time. Your work was recovered from the last autosave.".into(),
            });
        }
        if let Some(v) = report.migrated_from {
            ws.msg(format!("Project upgraded from format v{v}."), MsgKind::Info);
        }
        self.remember(&ws);
        self.ws = Some(ws);
        self.preflight.open = false;
        self.export = None;
        self.auto_build = None;
    }

    fn remember(&mut self, ws: &Workspace) {
        let p = ws.project();
        self.settings.touch_recent(RecentProject {
            path: ws.root.clone(),
            name: p.name.clone(),
            status: p.status.key().into(),
            due_date: p.due_date.clone(),
            opened_at: oto_core::model::now_iso(),
            spreads: p.spreads.len(),
        });
    }

    fn create_project(&mut self, np: wizard::NewProject) {
        if let Err(e) = std::fs::create_dir_all(np.root.parent().unwrap_or(Path::new("."))) {
            self.toast(format!("Cannot create folder: {e}"), MsgKind::Error);
            return;
        }
        match Package::create(&np.root, &np.project, &[]) {
            Ok(pkg) => {
                if let Some(l) = &np.lab {
                    let _ = pkg.save_lab_profile(&l.id, &l.to_toml());
                }
                let root = pkg.root.clone();
                let _ = pkg.close();
                self.open_project(&root);
                if let Some(ws) = &mut self.ws {
                    ws.msg("Project created. Next: import your photos (drop a folder anywhere in the window).", MsgKind::Ok);
                    ws.bottom_tab = state::BottomTab::Photos;
                }
            }
            Err(e) => self.toast(format!("Cannot create project: {e}"), MsgKind::Error),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = oto_platform::native().pick_project() {
            self.open_project(&p);
        }
    }

    fn open_sample(&mut self) {
        let docs = oto_platform::native().documents_dir().join("oto-albums");
        let _ = std::fs::create_dir_all(&docs);
        match oto_store::sample::create_sample(&docs) {
            Ok(p) => self.open_project(&p),
            Err(e) => self.toast(format!("Cannot create the sample project: {e}"), MsgKind::Error),
        }
    }

    fn start_import(&mut self, paths: Vec<PathBuf>) {
        let Some(ws) = &mut self.ws else { return };
        if ws.import.is_some() {
            ws.msg("An import is already running.", MsgKind::Info);
            return;
        }
        if let Some(first) = paths.first() {
            self.settings.last_import_dir = first.parent().map(Path::to_path_buf).or(Some(first.clone()));
        }
        let known = ws.media.iter().map(|m| m.hash.clone()).collect();
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = oto_media::spawn_import(paths, oto_media::CacheLayout::new(ws.root.join("cache")), known, cancel.clone());
        ws.import = Some(ImportJob { rx, cancel, total: 0, done: 0, current: String::new(), imported: 0, duplicates: 0, failed: vec![], to_timeline: false, new_ids: vec![] });
        ws.bottom_tab = state::BottomTab::Photos;
    }

    fn poll_import(&mut self, ctx: &egui::Context) {
        let Some(ws) = &mut self.ws else { return };
        let Some(job) = &mut ws.import else { return };
        let mut finished = None;
        let mut new_items = Vec::new();
        for _ in 0..500 {
            let Ok(m) = job.rx.try_recv() else { break };
            match m {
                ImportMsg::Started { total } => job.total = total,
                ImportMsg::Progress { done, total, current } => {
                    job.done = done;
                    job.total = total;
                    job.current = current;
                }
                ImportMsg::Item(item) => {
                    job.new_ids.push(item.id);
                    new_items.push(*item);
                }
                ImportMsg::Duplicate { .. } => job.duplicates += 1,
                ImportMsg::Failed { path, error } => job.failed.push(format!("{path}: {error}")),
                ImportMsg::Finished { imported, duplicates, failed, cancelled, elapsed_ms } => finished = Some((imported, duplicates, failed, cancelled, elapsed_ms)),
            }
        }
        if !new_items.is_empty() {
            for it in new_items {
                ws.media_idx.insert(it.id, ws.media.len());
                ws.media.push(it);
            }
            ws.media_dirty = true;
            ws.last_change = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        if let Some((imported, dups, failed, cancelled, ms)) = finished {
            let job = ws.import.take();
            oto_media::assign_similarity(&mut ws.media);
            ws.media.sort_by_key(|a| a.natural_key());
            ws.media_idx = state::media_index(&ws.media);
            ws.media_dirty = true;
            ws.refresh_status();
            let mut text = format!("Imported {imported} photo(s) in {:.1} s", ms as f32 / 1000.0);
            if dups > 0 {
                text.push_str(&format!(" · {dups} exact duplicate(s) skipped"));
            }
            if failed > 0 {
                text.push_str(&format!(" · {failed} file(s) could not be read"));
            }
            if cancelled {
                text.push_str(" · cancelled");
            }
            ws.msg(text, if failed > 0 { MsgKind::Warn } else { MsgKind::Ok });
            if let Some(j) = job {
                for f in j.failed.iter().take(5) {
                    ws.msg(f.clone(), MsgKind::Warn);
                }
            }
            ws.save_now("autosave");
        }
    }

    fn run_cmd(&mut self, c: Cmd, ctx: &egui::Context) {
        match c {
            Cmd::New => self.wizard = Some(wizard::Wizard::new(&self.labs, oto_platform::native().documents_dir())),
            Cmd::Open => self.open_dialog(),
            Cmd::Sample => self.open_sample(),
            Cmd::Settings => self.settings_open = true,
            Cmd::Shortcuts => self.shortcuts_open = true,
            Cmd::LangEn | Cmd::LangMs => {
                self.settings.lang = if c == Cmd::LangEn { i18n::Lang::En } else { i18n::Lang::Ms };
                i18n::set_lang(self.settings.lang);
                self.settings.save();
            }
            Cmd::FullScreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            _ => {
                let Some(ws) = &mut self.ws else { return };
                match c {
                    Cmd::Save => {
                        ws.save_now("save");
                        ws.store.send(StoreReq::Snapshot { reason: "save".into(), manual: false });
                    }
                    Cmd::SaveAs => ws.requests.push(Request::SaveAs),
                    Cmd::Close => ws.requests.push(Request::CloseProject),
                    Cmd::ImportFolder => ws.requests.push(Request::ImportFolder),
                    Cmd::ImportFiles => ws.requests.push(Request::ImportFiles),
                    Cmd::AutoBuild => ws.requests.push(Request::AutoBuild { from: None }),
                    Cmd::Preflight => ws.requests.push(Request::Preflight),
                    Cmd::Export => ws.requests.push(Request::Export),
                    Cmd::Proof => ws.requests.push(Request::Proof),
                    Cmd::Snapshots => ws.requests.push(Request::Snapshots),
                    Cmd::Snapshot => ws.store.send(StoreReq::Snapshot { reason: "manual".into(), manual: true }),
                    Cmd::Relink => ws.requests.push(Request::Relink),
                    Cmd::Consolidate => ws.requests.push(Request::Consolidate),
                    Cmd::AlbumSettings => ws.requests.push(Request::AlbumSettings),
                    Cmd::ProjectInfo => ws.requests.push(Request::ProjectInfo),
                    Cmd::SaveTemplate => ws.requests.push(Request::SaveTemplate),
                    Cmd::AddSpread => {
                        let at = ws.spread_index().map(|i| i + 1).unwrap_or(ws.project().spreads.len());
                        ws.exec(oto_core::document::Command::InsertSpread { at, spread: oto_core::model::Spread::blank() });
                    }
                    Cmd::DeleteSpread => {
                        if let Target::Spread(id) = ws.target {
                            if ws.project().spreads.len() > 1 {
                                ws.exec(oto_core::document::Command::DeleteSpread { id });
                            }
                        }
                    }
                    Cmd::LockSpread => {
                        if let (Target::Spread(id), Some(s)) = (ws.target, ws.current()) {
                            let locked = !s.locked;
                            ws.exec(oto_core::document::Command::SetLocked { id, locked });
                        }
                    }
                    Cmd::Cover => {
                        if ws.project().cover.is_some() {
                            ws.go_to(Target::Cover);
                        } else {
                            let cover = Some(oto_core::model::Cover::default());
                            ws.exec(oto_core::document::Command::SetCover { cover });
                            ws.go_to(Target::Cover);
                        }
                    }
                    Cmd::ToggleLeft => self.settings.left_open = !self.settings.left_open,
                    Cmd::ToggleRight => self.settings.right_open = !self.settings.right_open,
                    Cmd::ToggleBottom => self.settings.bottom_open = !self.settings.bottom_open,
                    Cmd::Focus => ws.focus_mode = !ws.focus_mode,
                    Cmd::Rulers => self.settings.show_rulers = !self.settings.show_rulers,
                    Cmd::Grid => self.settings.show_grid = !self.settings.show_grid,
                    Cmd::Guides => self.settings.show_guides = !self.settings.show_guides,
                    Cmd::Undo => ws.undo(),
                    Cmd::Redo => ws.redo(),
                    _ => {}
                }
            }
        }
    }

    fn handle_requests(&mut self) {
        let Some(ws) = &mut self.ws else { return };
        let reqs = std::mem::take(&mut ws.requests);
        for r in reqs {
            let Some(ws) = &mut self.ws else { return };
            match r {
                Request::AutoBuild { from } => {
                    if ws.media.is_empty() {
                        ws.msg("Import photos first.", MsgKind::Info);
                    } else {
                        self.auto_build = Some(AutoBuildDlg::new(ws, from));
                    }
                }
                Request::Preflight => {
                    ws.refresh_status();
                    ws.run_preflight();
                    self.preflight.open = true;
                }
                Request::Export => {
                    ws.refresh_status();
                    ws.run_preflight();
                    if self.export.is_none() {
                        self.export = Some(ExportDlg::new(ws));
                    } else if let Some(e) = &mut self.export {
                        e.open = true;
                    }
                }
                Request::Proof => self.proof = Some(ProofDlg::new(ws)),
                Request::Snapshots => {
                    ws.store.send(StoreReq::ListSnapshots);
                    self.snapshots_open = true;
                }
                Request::Relink => self.relink = Some(RelinkDlg { open: true, rx: None, matches: vec![], choice: Default::default() }),
                Request::AlbumSettings => self.spec = Some(SpecDlg { open: true, spec: ws.project().spec.clone(), icc_err: None }),
                Request::ProjectInfo => {
                    let p = ws.project();
                    self.meta = Some(MetaDlg { open: true, name: p.name.clone(), client: p.client.clone(), due: p.due_date.clone().unwrap_or_default(), status: p.status });
                }
                Request::SaveTemplate => self.save_template = Some(format!("{} layout", ws.project().name)),
                Request::ImportFolder => {
                    if let Some(p) = oto_platform::native().pick_folder("Import a folder of photos (includes sub-folders)") {
                        self.start_import(vec![p]);
                    }
                }
                Request::ImportFiles => {
                    if let Some(v) = oto_platform::native().pick_files("Import photos") {
                        self.start_import(v);
                    }
                }
                Request::Palette => self.palette = Some(dialogs::Palette { query: String::new(), cursor: 0 }),
                Request::Shortcuts => self.shortcuts_open = true,
                Request::Settings => self.settings_open = true,
                Request::SaveAs => {
                    if let Some(p) = oto_platform::native().save_project(&ws.project().name) {
                        ws.store.send(StoreReq::SaveAs { path: p, project: Box::new(ws.project().clone()), media: Arc::new(ws.media.clone()) });
                    }
                }
                Request::CloseProject => {
                    ws.close();
                    self.closing_since = Some(Instant::now());
                }
                Request::Consolidate => {
                    let dest = ws.root.join("media").join("originals");
                    match oto_media::consolidate(&ws.media, &dest) {
                        Ok(moved) => {
                            for (id, p) in moved {
                                if let Some(m) = ws.media_mut(id) {
                                    m.path = p.display().to_string();
                                }
                            }
                            ws.refresh_status();
                            ws.msg(format!("Originals copied into the project and verified ({}). Your source files were not touched.", dest.display()), MsgKind::Ok);
                        }
                        Err(e) => ws.msg(format!("Consolidate failed: {e}"), MsgKind::Error),
                    }
                }
            }
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let mut cmds: Vec<Cmd> = Vec::new();
        ctx.input_mut(|i| {
            let cmd = egui::Modifiers::COMMAND;
            let cs = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
            let pairs = [
                (cs, Key::S, Cmd::SaveAs),
                (cs, Key::Z, Cmd::Redo),
                (cmd, Key::N, Cmd::New),
                (cmd, Key::O, Cmd::Open),
                (cmd, Key::S, Cmd::Save),
                (cmd, Key::Z, Cmd::Undo),
                (cmd, Key::Y, Cmd::Redo),
                (cmd, Key::I, Cmd::ImportFolder),
                (cmd, Key::B, Cmd::AutoBuild),
                (cmd, Key::P, Cmd::Preflight),
                (cmd, Key::E, Cmd::Export),
                (cmd, Key::Num1, Cmd::ToggleLeft),
                (cmd, Key::Num2, Cmd::ToggleRight),
                (cmd, Key::Num3, Cmd::ToggleBottom),
                (cmd, Key::Comma, Cmd::Settings),
            ];
            for (m, k, c) in pairs {
                if (!typing || m.command) && i.consume_key(m, k) {
                    cmds.push(c);
                }
            }
            if i.consume_key(cmd, Key::K) {
                cmds.push(Cmd::Shortcuts); // placeholder replaced below
                cmds.pop();
                self.palette = Some(dialogs::Palette { query: String::new(), cursor: 0 });
            }
        });
        if !typing {
            let (tab, f, q, a, w, s, lb, rb, zero, one, help, prev, next) = ctx.input_mut(|i| {
                let none = egui::Modifiers::NONE;
                (
                    i.consume_key(none, Key::Tab),
                    i.consume_key(none, Key::F),
                    i.consume_key(none, Key::Q),
                    i.consume_key(none, Key::A),
                    i.consume_key(none, Key::W),
                    i.consume_key(none, Key::S),
                    i.consume_key(none, Key::OpenBracket),
                    i.consume_key(none, Key::CloseBracket),
                    i.key_pressed(Key::Num0) && i.modifiers.is_none(),
                    i.key_pressed(Key::Num1) && i.modifiers.is_none(),
                    i.consume_key(egui::Modifiers::SHIFT, Key::Slash) || i.consume_key(none, Key::Questionmark),
                    i.consume_key(egui::Modifiers::COMMAND, Key::ArrowLeft),
                    i.consume_key(egui::Modifiers::COMMAND, Key::ArrowRight),
                )
            });
            if help {
                self.shortcuts_open = true;
            }
            if f {
                cmds.push(Cmd::FullScreen);
            }
            if let Some(ws) = &mut self.ws {
                if tab {
                    ws.focus_mode = !ws.focus_mode;
                }
                let over_grid = ws.bottom_tab == state::BottomTab::Photos && ctx.is_pointer_over_area() && ctx.pointer_hover_pos().map(|p| p.y > ctx.screen_rect().height() * 0.55).unwrap_or(false);
                if lb {
                    ws.view.zoom = (ws.view.zoom / 1.25).max(0.05);
                }
                if rb {
                    ws.view.zoom = (ws.view.zoom * 1.25).min(40.0);
                }
                if zero && !over_grid {
                    ws.view.fit_pending = true;
                }
                if one && !over_grid {
                    ws.view.zoom = canvas::actual_zoom(ctx);
                    ws.view.pan = Vec2::ZERO;
                }
                if a || q {
                    step_template(ws, if q { 1 } else { -1 }, false);
                }
                if s || w {
                    step_template(ws, if w { 1 } else { -1 }, true);
                }
                if prev || next {
                    let ids: Vec<_> = ws.project().spreads.iter().map(|s| s.id).collect();
                    if let Some(i) = ws.spread_index() {
                        let j = if next { (i + 1).min(ids.len().saturating_sub(1)) } else { i.saturating_sub(1) };
                        ws.go_to(Target::Spread(ids[j]));
                    } else if let Some(first) = ids.first() {
                        ws.go_to(Target::Spread(*first));
                    }
                }
                canvas::keys(ctx, ws);
            }
        }
        for c in cmds {
            self.run_cmd(c, ctx);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let pal = theme::palette(self.settings.theme);
        let mut cmd: Option<Cmd> = None;
        ui.horizontal(|ui| {
            ui.menu_button("☰", |ui| {
                for (c, label, key) in dialogs::commands().into_iter().filter(|(c, ..)| {
                    matches!(
                        c,
                        Cmd::New
                            | Cmd::Open
                            | Cmd::Save
                            | Cmd::SaveAs
                            | Cmd::Close
                            | Cmd::ImportFolder
                            | Cmd::ImportFiles
                            | Cmd::Snapshots
                            | Cmd::Relink
                            | Cmd::Consolidate
                            | Cmd::AlbumSettings
                            | Cmd::ProjectInfo
                            | Cmd::Cover
                            | Cmd::Settings
                            | Cmd::Shortcuts
                    )
                }) {
                    if ui.add(egui::Button::new(t(label)).shortcut_text(key)).clicked() {
                        cmd = Some(c);
                        ui.close_menu();
                    }
                }
            });
            let Some(ws) = &mut self.ws else { return };
            let name = ws.project().name.clone();
            if ui.add(egui::Button::new(RichText::new(&name).font(theme::strong_font(14.0))).frame(false)).on_hover_text(t("Project info")).clicked() {
                ws.requests.push(Request::ProjectInfo);
            }
            ui.label(RichText::new(ws.project().status.key()).small().color(pal.muted));
            ui.separator();
            if ui.add_enabled(ws.doc.can_undo(), egui::Button::new("↶")).on_hover_text(format!("{} {} (⌘Z)", t("Undo"), ws.doc.undo_label().unwrap_or(""))).clicked() {
                ws.undo();
            }
            if ui.add_enabled(ws.doc.can_redo(), egui::Button::new("↷")).on_hover_text(format!("{} {} (⇧⌘Z)", t("Redo"), ws.doc.redo_label().unwrap_or(""))).clicked() {
                ws.redo();
            }
            let (txt, col) = match ws.save_state {
                SaveState::Saved => (format!("✔ {} {}", t("Saved"), ws.last_saved_at), pal.muted),
                SaveState::Saving => (t("Saving…").to_string(), pal.muted),
                SaveState::Dirty => (format!("● {}", t("Unsaved changes")), pal.warn),
                SaveState::Failed => ("✖ autosave failed".into(), pal.crit),
            };
            ui.label(RichText::new(txt).small().color(col)).on_hover_text("Autosaves a few seconds after each change. ⌘S saves immediately.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⌘K").on_hover_text(t("Command palette")).clicked() {
                    ws.requests.push(Request::Palette);
                }
                if ui.add(egui::Button::new(RichText::new(format!("⇪ {}", t("Export"))).strong().color(theme::ON_ACCENT)).fill(pal.accent)).on_hover_text("⌘E").clicked() {
                    ws.requests.push(Request::Export);
                }
                if ui.button(t("Proof")).clicked() {
                    ws.requests.push(Request::Proof);
                }
                let (c, w) = (ws.preflight.count(oto_print::preflight::Severity::Critical), ws.preflight.count(oto_print::preflight::Severity::Warning));
                let label = if c > 0 {
                    RichText::new(format!("✖{c} ⚠{w} {}", t("Preflight"))).color(pal.crit)
                } else if w > 0 {
                    RichText::new(format!("⚠{w} {}", t("Preflight"))).color(pal.warn)
                } else {
                    RichText::new(format!("✔ {}", t("Preflight"))).color(pal.ok)
                };
                if ui.button(label).on_hover_text("⌘P").clicked() {
                    ws.requests.push(Request::Preflight);
                }
                if ui.button(t("Auto Build…")).on_hover_text("⌘B").clicked() {
                    ws.requests.push(Request::AutoBuild { from: None });
                }
                if ws.project().cover.is_some() || ws.project().spec.cover_type != oto_core::spec::CoverType::None {
                    let on_cover = ws.target == Target::Cover;
                    if ui.selectable_label(on_cover, if on_cover { t("Back to spreads") } else { t("Edit cover") }).clicked() {
                        if on_cover {
                            if let Some(id) = ws.project().spreads.first().map(|s| s.id) {
                                ws.go_to(Target::Spread(id));
                            }
                        } else {
                            cmd = Some(Cmd::Cover);
                        }
                    }
                }
            });
        });
        if let Some(c) = cmd {
            let ctx = ui.ctx().clone();
            self.run_cmd(c, &ctx);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let pal = theme::palette(self.settings.theme);
        let Some(ws) = &mut self.ws else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{:.0}%", ws.view.zoom / canvas::actual_zoom(ui.ctx()) * 100.0)).small());
            ui.separator();
            if let Some(i) = ws.spread_index() {
                ui.label(RichText::new(format!("Spread {} / {}", i + 1, ws.project().spreads.len())).small());
            } else {
                ui.label(RichText::new(t("Cover")).small());
            }
            // Effective DPI of the selected frame.
            if let (Some(cur), Some(id)) = (ws.current(), ws.sel.first()) {
                if let Some(f) = cur.frames.iter().find(|f| f.id == *id) {
                    if let Some(m) = f.image_id.and_then(|i| ws.media(i)) {
                        let surf = ws.surface();
                        let pr = surf.print_rect(&f.rect_norm);
                        let ai = m.aspect();
                        let af = (pr.w() / pr.h().max(1.0)) as f32;
                        let c = oto_core::crop::clamp(&f.crop, ai, af);
                        let vis = oto_core::crop::visible(&c, ai, af, f.fit);
                        let dpi = oto_core::crop::effective_dpi(m.width, m.height, &vis, pr.w() as i64, pr.h() as i64);
                        let col = if dpi < 200.0 {
                            pal.crit
                        } else if dpi < 250.0 {
                            pal.warn
                        } else {
                            pal.muted
                        };
                        ui.separator();
                        ui.label(RichText::new(format!("{:.0} dpi · {}", dpi, m.file_name)).small().color(col));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (n, mb) = ws.tex.stats();
                ui.label(RichText::new(format!("{} {} · GPU ≈{mb:.0} MB ({n})", ws.media.len(), t("Photos").to_lowercase())).small().color(pal.muted));
                ui.separator();
                let c = ws.preflight.count(oto_print::preflight::Severity::Critical);
                let w = ws.preflight.count(oto_print::preflight::Severity::Warning);
                if ui.add(egui::Button::new(RichText::new(format!("✖ {c}  ⚠ {w}")).small()).frame(false)).on_hover_text(t("Preflight")).clicked() {
                    ws.requests.push(Request::Preflight);
                }
                ui.separator();
                let lang_btn = ui.add(egui::Label::new(RichText::new(i18n::lang().label()).small().color(pal.muted)).sense(egui::Sense::click())).on_hover_text("Switch language · Tukar bahasa");
                if lang_btn.clicked() {
                    self.settings.lang = if self.settings.lang == i18n::Lang::En { i18n::Lang::Ms } else { i18n::Lang::En };
                    i18n::set_lang(self.settings.lang);
                    self.settings.save();
                }
            });
        });
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        if let Some(ws) = &mut self.ws {
            let msgs = std::mem::take(&mut ws.msgs);
            for (m, k) in msgs {
                self.toast(m, k);
            }
        }
        let pal = theme::palette(self.settings.theme);
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_secs(if t.kind == MsgKind::Error { 12 } else { 6 }));
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -40.0]).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.set_max_width(380.0);
            for t in self.toasts.iter().rev().take(5) {
                let (icon, col) = match t.kind {
                    MsgKind::Ok => ("✔", pal.ok),
                    MsgKind::Info => ("ℹ", pal.guide),
                    MsgKind::Warn => ("⚠", pal.warn),
                    MsgKind::Error => ("✖", pal.crit),
                };
                egui::Frame::new().fill(pal.raised).stroke(egui::Stroke::new(1.0_f32, col)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                    ui.label(RichText::new(format!("{icon}  {}", t.text)).color(pal.text));
                });
                ui.add_space(6.0);
            }
        });
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if dropped.is_empty() {
            return;
        }
        if let Some(p) = dropped.iter().find(|p| p.extension().and_then(|e| e.to_str()) == Some("otoalbum")) {
            let p = p.clone();
            self.open_project(&p);
            return;
        }
        if self.ws.is_some() {
            self.start_import(dropped);
        } else {
            self.toast("Create or open a project first, then drop photos into it.", MsgKind::Info);
        }
    }

    fn workspace_ui(&mut self, ctx: &egui::Context) {
        let focus = self.ws.as_ref().map(|w| w.focus_mode).unwrap_or(false);
        egui::TopBottomPanel::top("top").frame(egui::Frame::new().fill(theme::palette(self.settings.theme).bg).inner_margin(egui::Margin::symmetric(10, 6))).show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("status").frame(egui::Frame::new().fill(theme::palette(self.settings.theme).bg).inner_margin(egui::Margin::symmetric(10, 3))).show(ctx, |ui| self.status_bar(ui));
        let Some(ws) = &mut self.ws else { return };
        if !focus && self.settings.bottom_open {
            egui::TopBottomPanel::bottom("bottom")
                .resizable(true)
                .default_height(210.0)
                .min_height(120.0)
                .max_height(ctx.screen_rect().height() * 0.55)
                .show(ctx, |ui| bottom::bottom(ui, ws, &mut self.settings));
        }
        if !focus && self.settings.left_open {
            egui::SidePanel::left("left").resizable(true).default_width(210.0).min_width(150.0).max_width(360.0).show(ctx, |ui| bookmap::book_map(ui, ws, &mut self.settings));
        }
        if !focus && self.settings.right_open {
            egui::SidePanel::right("right")
                .resizable(true)
                .default_width(290.0)
                .min_width(240.0)
                .max_width(420.0)
                .frame(egui::Frame::new().fill(egui::Color32::WHITE).inner_margin(egui::Margin::symmetric(12, 8)))
                .show(ctx, |ui| {
                    ui.style_mut().visuals = theme::panel_visuals(self.settings.theme);
                    inspector::inspector(ui, ws, &self.settings);
                });
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::palette(self.settings.theme).canvas)).show(ctx, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).fill(theme::palette(self.settings.theme).panel).show(ui, |ui| canvas::toolbar(ui, ws));
            canvas::canvas(ui, ws, &self.settings);
        });
        // Dragged photo follows the pointer.
        if let Some(ids) = &ws.drag_media {
            if let (Some(p), Some(m)) = (ctx.pointer_hover_pos(), ids.first().and_then(|id| ws.media(*id)).cloned()) {
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drag-photo")));
                if let Some((tid, size)) = ws.tex.get(&m.hash, oto_media::Level::Thumb) {
                    let s = 72.0 / size.x.max(size.y);
                    let r = egui::Rect::from_min_size(p + Vec2::new(10.0, 10.0), size * s);
                    painter.image(tid, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::from_white_alpha(220));
                    if ids.len() > 1 {
                        painter.text(r.right_top(), egui::Align2::RIGHT_TOP, format!("{}", ids.len()), egui::FontId::proportional(14.0), Color32::WHITE);
                    }
                }
            }
            if ctx.input(|i| i.pointer.any_released()) {
                // Released outside the canvas: cancel.
                ws.drag_media = None;
            }
        }
    }

    fn dialogs_ui(&mut self, ctx: &egui::Context) {
        if let Some(mut p) = self.palette.take() {
            let (c, close) = dialogs::palette(ctx, &mut p, self.ws.is_some());
            if let Some(c) = c {
                self.run_cmd(c, ctx);
            } else if !close {
                self.palette = Some(p);
            }
        }
        if self.shortcuts_open {
            dialogs::shortcuts(ctx, &mut self.shortcuts_open);
        }
        if self.settings_open {
            let mut open = true;
            if dialogs::settings_window(ctx, &mut open, &mut self.settings, &self.lab_note) {
                i18n::set_lang(self.settings.lang);
                theme::apply(ctx, self.settings.theme, self.settings.reduced_motion);
                self.settings.save();
            }
            self.settings_open = open;
        }
        let Some(ws) = &mut self.ws else { return };
        if let Some(note) = ws.recovery_note.clone() {
            let mut keep = true;
            egui::Window::new(t("Recover work")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(note);
                ui.label("If something looks wrong, restore a snapshot from before the problem.");
                ui.horizontal(|ui| {
                    if ui.button(t("Continue")).clicked() {
                        keep = false;
                    }
                    if ui.button(t("Snapshots")).clicked() {
                        ws.requests.push(Request::Snapshots);
                        keep = false;
                    }
                });
            });
            if !keep {
                ws.recovery_note = None;
            }
        }
        if let Some(d) = &mut self.auto_build {
            dialogs::auto_build(ctx, ws, d, &self.settings);
            if !d.open {
                self.auto_build = None;
            }
        }
        if self.preflight.open {
            dialogs::preflight(ctx, ws, &mut self.preflight, &self.settings);
        }
        if let Some(d) = &mut self.export {
            if d.open || ws.export.is_some() {
                dialogs::export(ctx, ws, d, &self.settings);
            }
        }
        if let Some(d) = &mut self.proof {
            dialogs::proof(ctx, ws, d, &self.settings);
            if !d.open {
                self.proof = None;
            }
        }
        if self.snapshots_open {
            dialogs::snapshots(ctx, ws, &mut self.snapshots_open, &mut self.snapshot_confirm);
        }
        if let Some(d) = &mut self.relink {
            dialogs::relink(ctx, ws, d, &self.settings);
            if !d.open {
                self.relink = None;
            }
        }
        if let Some(d) = &mut self.meta {
            dialogs::meta(ctx, ws, d);
            if !d.open {
                self.meta = None;
            }
        }
        if let Some(d) = &mut self.spec {
            dialogs::album_settings(ctx, ws, d);
            if !d.open {
                self.spec = None;
            }
        }
        if self.save_template.is_some() {
            dialogs::save_template(ctx, ws, &mut self.save_template);
        }
    }
}

fn step_template(ws: &mut Workspace, dir: i32, by_family: bool) {
    let n = ws.current().map(|s| s.media_ids().len()).unwrap_or(0);
    let cands: Vec<usize> = bottom::candidates(ws).into_iter().filter(|i| ws.templates[*i].photo_count as usize == n || matches!(ws.target, Target::Cover)).collect();
    if cands.is_empty() {
        return;
    }
    let cur = ws.current().map(|s| s.template_id.clone()).unwrap_or_default();
    let pos = cands.iter().position(|i| ws.templates[*i].id == cur);
    let next = if by_family {
        let fam = pos.map(|p| ws.templates[cands[p]].family.clone()).unwrap_or_default();
        let order: Vec<usize> = if dir > 0 {
            (1..=cands.len()).map(|k| (pos.unwrap_or(0) + k) % cands.len()).collect()
        } else {
            (1..=cands.len()).map(|k| (pos.unwrap_or(0) + cands.len() * 2 - k) % cands.len()).collect()
        };
        order.into_iter().find(|&k| ws.templates[cands[k]].family != fam).unwrap_or(0)
    } else {
        match pos {
            Some(p) => ((p as i32 + dir).rem_euclid(cands.len() as i32)) as usize,
            None => 0,
        }
    };
    bottom::apply_template_idx(ws, cands[next]);
}

impl eframe::App for OtoApp {
    fn on_exit(&mut self) {
        if let Some(ws) = &mut self.ws {
            ws.close_and_wait(std::time::Duration::from_secs(5));
        }
        self.settings.save();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(p) = self.pending_open.take() {
            self.open_project(&p);
        }
        if (self.settings.ui_scale - self.applied_scale).abs() > 0.001 {
            ctx.set_zoom_factor(self.settings.ui_scale);
            self.applied_scale = self.settings.ui_scale;
        }
        // Background work.
        self.poll_import(ctx);
        if let Some(ws) = &mut self.ws {
            ws.tick();
            if ws.tex.poll(ctx) || ws.tex.has_pending() || ws.export.is_some() {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
            let title = format!("oto-albums — {}", ws.project().name);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        // Closing a project / quitting waits for the DB thread to finish its last save.
        let closed = self.ws.as_ref().map(|w| w.closed).unwrap_or(false);
        let timed_out = self.closing_since.map(|t| t.elapsed() > Duration::from_secs(6)).unwrap_or(false);
        if closed || timed_out {
            if let Some(ws) = &self.ws {
                let snapshot = (ws.root.clone(), ws.project().name.clone(), ws.project().status.key().to_string(), ws.project().due_date.clone(), ws.project().spreads.len());
                self.settings.touch_recent(RecentProject { path: snapshot.0, name: snapshot.1, status: snapshot.2, due_date: snapshot.3, opened_at: oto_core::model::now_iso(), spreads: snapshot.4 });
            }
            self.ws = None;
            self.closing_since = None;
            self.auto_build = None;
            self.export = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Title("oto-albums".into()));
            if self.quit_after_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_after_close {
            if let Some(ws) = &mut self.ws {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ws.close();
                self.quit_after_close = true;
                self.closing_since = Some(Instant::now());
            }
            self.settings.save();
        }
        self.dropped_files(ctx);
        self.keys(ctx);
        self.handle_requests();
        if self.ws.is_some() {
            self.workspace_ui(ctx);
        } else {
            let mut search = std::mem::take(&mut self.hub_search);
            let mut act = hub::HubAction::None;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::palette(self.settings.theme).bg).inner_margin(24)).show(ctx, |ui| {
                act = hub::hub(ui, &mut self.settings, &mut search);
            });
            self.hub_search = search;
            match act {
                hub::HubAction::New => self.wizard = Some(wizard::Wizard::new(&self.labs, oto_platform::native().documents_dir())),
                hub::HubAction::Open => self.open_dialog(),
                hub::HubAction::OpenPath(p) => self.open_project(&p),
                hub::HubAction::Sample => self.open_sample(),
                hub::HubAction::Settings => self.settings_open = true,
                hub::HubAction::None => {}
            }
        }
        if let Some(w) = &mut self.wizard {
            match w.ui(ctx, &self.labs, self.settings.theme) {
                wizard::WizardOutcome::Continue => {}
                wizard::WizardOutcome::Cancel => self.wizard = None,
                wizard::WizardOutcome::Create(np) => {
                    self.wizard = None;
                    self.create_project(*np);
                }
            }
        }
        self.dialogs_ui(ctx);
        self.toasts(ctx);
    }
}

/// Start the native app.
pub fn run(open: Option<PathBuf>) -> eframe::Result {
    let icon = app_icon();
    let viewport = egui::ViewportBuilder::default().with_title("oto-albums").with_inner_size([1440.0, 900.0]).with_min_inner_size([1024.0, 640.0]).with_drag_and_drop(true).with_icon(icon);
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, ..Default::default() };
    eframe::run_native("oto-albums", options, Box::new(move |cc| Ok(Box::new(OtoApp::new(cc, open)))))
}

/// Simple original app icon: an open book in vermilion on charcoal.
pub fn app_icon_rgba(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 / s, y as f32 / s);
            let r = 0.22;
            let dx = (fx - 0.5).abs() - (0.5 - r);
            let dy = (fy - 0.5).abs() - (0.5 - r);
            let inside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() <= r;
            let i = ((y * size + x) * 4) as usize;
            if !inside {
                continue;
            }
            let mut c = [0x17, 0x19, 0x1C, 255];
            // Two pages, slightly tilted, meeting at the fold.
            let page_l = fx > 0.18 && fx < 0.5 && fy > 0.3 + (0.5 - fx) * 0.12 && fy < 0.74;
            let page_r = (0.5..0.82).contains(&fx) && fy > 0.3 + (fx - 0.5) * 0.12 && fy < 0.74;
            if page_l || page_r {
                c = [0xF3, 0xEF, 0xE6, 255];
            }
            if (fx - 0.5).abs() < 0.012 && fy > 0.3 && fy < 0.76 {
                c = [0xD9, 0x5D, 0x39, 255];
            }
            if fy > 0.74 && fy < 0.79 && fx > 0.18 && fx < 0.82 {
                c = [0xD9, 0x5D, 0x39, 255];
            }
            px[i..i + 4].copy_from_slice(&c);
        }
    }
    px
}

fn app_icon() -> egui::IconData {
    let size = 256;
    egui::IconData { rgba: app_icon_rgba(size), width: size, height: size }
}

/// Shows a stored RFC 3339 (UTC) timestamp in the user's local time; falls back to the raw text.
pub(crate) fn local_stamp(at: &str) -> String {
    let fallback = || at.replace('T', " ").chars().take(19).collect::<String>();
    let Ok(t) = time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339) else { return fallback() };
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    t.to_offset(offset).format(&time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]")).unwrap_or_else(|_| fallback())
}
