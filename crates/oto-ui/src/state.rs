//! Open-project state: document + media library + background jobs + view state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use oto_core::document::{Command, Document, Event};
use oto_core::lab::LabProfile;
use oto_core::library;
use oto_core::media::MediaItem;
use oto_core::model::{ItemId, MediaId, Project, Spread, SpreadId};
use oto_core::solver::{Ctx, PhotoInfo};
use oto_core::spec::CoverGeometry;
use oto_core::template::Template;
use oto_media::{FileStatus, ImportMsg};
use oto_print::export::ExportMsg;
use oto_print::preflight::{self, Report};
use oto_render::raster::Surface;
use oto_store::SnapshotInfo;

use crate::textures::TexCache;
use crate::worker::{StoreReq, StoreResp, StoreWorker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Spread(SpreadId),
    Cover,
}

/// Requests from panels to the app shell (dialogs, file pickers).
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    AutoBuild { from: Option<usize> },
    Preflight,
    Export,
    Proof,
    Snapshots,
    Relink,
    AlbumSettings,
    ProjectInfo,
    SaveTemplate,
    ImportFolder,
    ImportFiles,
    Palette,
    Shortcuts,
    Settings,
    SaveAs,
    CloseProject,
    Consolidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Frame,
    Text,
    Rect,
    Ellipse,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Timeline,
    Photos,
    Templates,
    Patterns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Spread,
    Image,
    Text,
    Layers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaFilter {
    All,
    Used,
    Unused,
    Missing,
    Rejected,
    Picks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSort {
    Name,
    Time,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    Info,
    Ok,
    Warn,
    Error,
}

pub struct ImportJob {
    pub rx: Receiver<ImportMsg>,
    pub cancel: Arc<AtomicBool>,
    pub total: usize,
    pub done: usize,
    pub current: String,
    pub imported: usize,
    pub duplicates: usize,
    pub failed: Vec<String>,
    pub to_timeline: bool,
    pub new_ids: Vec<MediaId>,
}

pub struct ExportJob {
    pub rx: Receiver<ExportMsg>,
    pub cancel: Arc<AtomicBool>,
    pub done: usize,
    pub total: usize,
    pub stage: String,
}

pub struct ViewState {
    /// Screen points per millimetre.
    pub zoom: f32,
    pub pan: egui::Vec2,
    pub fit_pending: bool,
    pub continuous: bool,
    pub compare: bool,
    /// The user zoomed or panned by hand: stop auto-fitting on resize.
    pub user_moved: bool,
    pub last_area: egui::Vec2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveState {
    Saved,
    Dirty,
    Saving,
    Failed,
}

pub struct Workspace {
    pub root: PathBuf,
    pub doc: Document,
    pub media: Vec<MediaItem>,
    pub media_idx: HashMap<MediaId, usize>,
    pub media_dirty: bool,
    pub store: StoreWorker,
    pub tex: TexCache,
    pub lab: Option<LabProfile>,
    pub templates: Vec<Template>,
    /// Templates saved in the project or by the user; always offered on top of the library.
    pub extra_templates: Vec<Template>,
    /// Settings/spec fingerprint the current template list was built for.
    templates_key: String,
    pub target: Target,
    pub view: ViewState,
    pub sel: Vec<ItemId>,
    /// Layout animation of the current spread.
    pub anim: crate::anim::Anim,
    /// Mirrors AppSettings.reduced_motion (set every frame).
    pub reduced_motion: bool,
    /// Photo under the pointer (strip or spread), for Quick Look.
    pub hover_media: Option<MediaId>,
    /// Pattern being dragged from the Patterns tab onto the spread.
    pub drag_pattern: Option<oto_core::model::PatternKind>,
    /// Space was used to pan the canvas while held (then releasing it must not open Quick Look).
    pub space_panned: bool,
    /// Running macOS Quick Look preview.
    pub quick_look: Option<std::process::Child>,
    pub tool: Tool,
    pub crop_mode: Option<ItemId>,
    pub preview: Option<Spread>,
    pub media_sel: Vec<MediaId>,
    pub media_anchor: Option<usize>,
    pub media_filter: MediaFilter,
    pub media_sort: MediaSort,
    pub media_search: String,
    pub media_bin: Option<String>,
    pub thumb_size: f32,
    pub bottom_tab: BottomTab,
    pub right_tab: RightTab,
    pub import: Option<ImportJob>,
    pub export: Option<ExportJob>,
    pub preflight: Report,
    pub preflight_rev: u64,
    pub file_status: HashMap<MediaId, FileStatus>,
    pub status_checked: Instant,
    pub save_state: SaveState,
    pub last_saved_rev: u64,
    pub last_saved_media: bool,
    pub last_change: Instant,
    pub last_save_req: Instant,
    pub last_saved_at: String,
    pub last_snapshot: Instant,
    pub snapshots: Vec<SnapshotInfo>,
    pub msgs: Vec<(String, MsgKind)>,
    pub edited: bool,
    pub exported: bool,
    pub drag_media: Option<Vec<MediaId>>,
    pub focus_mode: bool,
    pub closing: bool,
    pub closed: bool,
    pub recovery_note: Option<String>,
    pub drag_state: Option<crate::canvas::CanvasDrag>,
    pub focus_text_edit: bool,
    pub template_cursor: usize,
    pub requests: Vec<Request>,
    pub tpl_cache: HashMap<String, Spread>,
    pub tpl_cache_key: (u64, Option<SpreadId>, bool),
    pub auto_build_source: Option<Vec<MediaId>>,
}

pub fn media_index(media: &[MediaItem]) -> HashMap<MediaId, usize> {
    media.iter().enumerate().map(|(i, m)| (m.id, i)).collect()
}

/// Library templates plus project/user templates (ids not already present).
fn merge_templates(mut base: Vec<Template>, extra: &[Template]) -> Vec<Template> {
    for t in extra {
        if !base.iter().any(|x| x.id == t.id) {
            base.push(t.clone());
        }
    }
    base
}

impl Workspace {
    pub fn new(root: PathBuf, project: Project, media: Vec<MediaItem>, store: StoreWorker, lab: Option<LabProfile>, extra_templates: Vec<Template>) -> Self {
        let templates = merge_templates(library::project_templates(&project.spec.geometry(), &project.settings), &extra_templates);
        let target = project.spreads.first().map(|s| Target::Spread(s.id)).unwrap_or(Target::Cover);
        let media_idx = media_index(&media);
        let tex = TexCache::new(root.join("cache"));
        let mut ws = Workspace {
            root,
            doc: Document::new(project),
            media,
            media_idx,
            media_dirty: false,
            store,
            tex,
            lab,
            templates,
            extra_templates,
            templates_key: String::new(),
            target,
            view: ViewState { zoom: 1.0, pan: egui::Vec2::ZERO, fit_pending: true, continuous: false, compare: false, user_moved: false, last_area: egui::Vec2::ZERO },
            sel: Vec::new(),
            anim: Default::default(),
            reduced_motion: false,
            hover_media: None,
            drag_pattern: None,
            space_panned: false,
            quick_look: None,
            tool: Tool::Select,
            crop_mode: None,
            preview: None,
            media_sel: Vec::new(),
            media_anchor: None,
            media_filter: MediaFilter::All,
            media_sort: MediaSort::Name,
            media_search: String::new(),
            media_bin: None,
            thumb_size: 96.0,
            bottom_tab: BottomTab::Timeline,
            right_tab: RightTab::Spread,
            import: None,
            export: None,
            preflight: Report::default(),
            preflight_rev: u64::MAX,
            file_status: HashMap::new(),
            status_checked: Instant::now() - Duration::from_secs(3600),
            save_state: SaveState::Saved,
            last_saved_rev: 0,
            last_saved_media: false,
            last_change: Instant::now(),
            last_save_req: Instant::now(),
            last_saved_at: String::new(),
            last_snapshot: Instant::now(),
            snapshots: Vec::new(),
            msgs: Vec::new(),
            edited: false,
            exported: false,
            drag_media: None,
            focus_mode: false,
            closing: false,
            closed: false,
            recovery_note: None,
            drag_state: None,
            focus_text_edit: false,
            template_cursor: 0,
            requests: Vec::new(),
            tpl_cache: HashMap::new(),
            tpl_cache_key: (u64::MAX, None, false),
            auto_build_source: None,
        };
        if ws.doc.project.spreads.iter().all(|s| s.frames.is_empty()) && !ws.media.is_empty() {
            ws.bottom_tab = BottomTab::Photos;
        }
        ws.refresh_status();
        ws
    }

    // ----- Template variations & automatic layout (SmartAlbums-style "design follows the photos") -----

    fn spread_infos(&self, s: &Spread) -> Vec<PhotoInfo> {
        s.frames.iter().filter_map(|f| f.image_id).filter_map(|id| self.photo_info(id)).collect()
    }

    /// Templates for this spread's photos, best first (indices into `self.templates`).
    pub fn ranked_templates(&self, s: &Spread) -> Vec<usize> {
        let infos = self.spread_infos(s);
        if infos.is_empty() {
            return Vec::new();
        }
        let refs: Vec<&PhotoInfo> = infos.iter().collect();
        oto_core::library::rank_templates(&self.ctx(), &refs, &self.templates).into_iter().map(|(i, _)| i).collect()
    }

    /// Lay `s` (with whatever photos its frames hold) onto template `ti` and commit, animating the change.
    pub fn relayout_onto(&mut self, s: Spread, ti: usize, label: &str) -> bool {
        let Some(t) = self.templates.get(ti).cloned() else { return false };
        let ctx = self.ctx();
        let mut laid = {
            let lookup = |id: MediaId| self.photo_info(id);
            oto_core::library::layout_with(&ctx, &s, &t, &lookup)
        };
        laid.id = s.id;
        laid.locked = s.locked;
        if let Some(before) = self.current().cloned() {
            let enabled = !self.reduced_motion;
            self.anim.begin(&before, enabled);
        }
        self.commit(laid, label)
    }

    /// Re-lay a spread onto the best template for its photos (used after adding or removing photos).
    pub fn relayout_best(&mut self, s: Spread, label: &str) -> bool {
        if s.locked || !matches!(self.target, Target::Spread(_)) {
            return self.commit(s, label);
        }
        match self.ranked_templates(&s).first().copied() {
            Some(ti) => self.relayout_onto(s, ti, label),
            None => self.commit(s, label),
        }
    }

    /// Re-lay every unlocked spread onto its own template (after spacing changes). One undo step.
    pub fn relayout_all(&mut self, label: &str) -> usize {
        let ctx = self.ctx();
        let mut n = 0;
        let spreads: Vec<Spread> = self
            .doc
            .project
            .spreads
            .iter()
            .map(|s| {
                if s.locked {
                    return s.clone();
                }
                match self.templates.iter().find(|t| t.id == s.template_id) {
                    Some(t) => {
                        let lookup = |id: MediaId| self.photo_info(id);
                        let mut laid = oto_core::library::layout_with(&ctx, s, t, &lookup);
                        laid.id = s.id;
                        n += 1;
                        laid
                    }
                    None => s.clone(),
                }
            })
            .collect();
        if n > 0 {
            self.exec(Command::SetSpreads { spreads, label: label.to_string() });
        }
        n
    }

    /// Up/Down: step through the designs for this many photos, best first.
    pub fn variation(&mut self, step: i32) {
        if !matches!(self.target, Target::Spread(_)) {
            return;
        }
        let Some(cur) = self.current().cloned() else { return };
        if cur.locked {
            self.msg("This spread is locked.", MsgKind::Warn);
            return;
        }
        let ranked = self.ranked_templates(&cur);
        if ranked.is_empty() {
            return;
        }
        let n = ranked.len() as i32;
        let pos = ranked.iter().position(|&i| self.templates[i].id == cur.template_id);
        let next = match pos {
            Some(p) => (p as i32 + step).rem_euclid(n),
            None if step >= 0 => 0,
            None => n - 1,
        } as usize;
        if self.relayout_onto(cur, ranked[next], "Change design") {
            let name = self.templates[ranked[next]].name.clone();
            self.msg(format!("Design {} of {} · {name}", next + 1, n), MsgKind::Info);
        }
    }

    /// Put a background pattern on the current spread (keeps the colour/opacity already chosen).
    pub fn set_pattern(&mut self, k: oto_core::model::PatternKind) {
        let Some(mut s) = self.current().cloned() else { return };
        let mut p = oto_core::model::Pattern::new(k);
        if let Some(old) = s.background.pattern {
            p.color = old.color;
            p.opacity = old.opacity;
        }
        s.background.pattern = Some(p);
        self.commit(s, "Pattern");
    }

    /// Space: open or close macOS Quick Look for a photo.
    pub fn toggle_quick_look(&mut self, id: Option<MediaId>) {
        if let Some(mut child) = self.quick_look.take() {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        let Some(m) = id.and_then(|id| self.media(id)) else { return };
        let path = std::path::PathBuf::from(&m.path);
        self.quick_look = oto_platform::quick_look(&path);
    }

    pub fn reload_templates(&mut self) {
        let templates = {
            let geom = self.doc.project.spec.geometry();
            merge_templates(library::project_templates(&geom, &self.doc.project.settings), &self.extra_templates)
        };
        self.templates = templates;
        self.tpl_cache.clear();
        self.tpl_cache_key = (u64::MAX, None, false);
    }

    pub fn project(&self) -> &Project {
        &self.doc.project
    }

    pub fn msg(&mut self, s: impl Into<String>, k: MsgKind) {
        self.msgs.push((s.into(), k));
    }

    pub fn media(&self, id: MediaId) -> Option<&MediaItem> {
        self.media_idx.get(&id).and_then(|&i| self.media.get(i))
    }

    pub fn media_mut(&mut self, id: MediaId) -> Option<&mut MediaItem> {
        let i = *self.media_idx.get(&id)?;
        self.media_dirty = true;
        self.last_change = Instant::now();
        self.media.get_mut(i)
    }

    pub fn photo_info(&self, id: MediaId) -> Option<PhotoInfo> {
        self.media(id).map(PhotoInfo::from_media)
    }

    pub fn ctx(&self) -> Ctx {
        Ctx::new(&self.doc.project.spec)
    }

    pub fn cover_geometry(&self) -> CoverGeometry {
        oto_print::export::cover_geometry(&self.doc.project, self.lab.as_ref()).unwrap_or_else(|| oto_core::lab::generic_cover_geometry(&self.doc.project.spec, self.doc.project.page_count()))
    }

    pub fn surface(&self) -> Surface {
        match self.target {
            Target::Cover => {
                let g = self.cover_geometry();
                Surface { w: g.trim_w(), h: g.trim_h(), bleed: g.bleed }
            }
            Target::Spread(_) => {
                let g = self.doc.project.spec.geometry();
                Surface { w: g.trim_w, h: g.trim_h, bleed: g.bleed }
            }
        }
    }

    pub fn spread_index(&self) -> Option<usize> {
        match self.target {
            Target::Spread(id) => self.doc.project.spread_index(id),
            Target::Cover => None,
        }
    }

    pub fn current(&self) -> Option<&Spread> {
        match self.target {
            Target::Spread(id) => self.doc.project.spreads.iter().find(|s| s.id == id),
            Target::Cover => self.doc.project.cover.as_ref().map(|c| &c.content),
        }
    }

    pub fn ensure_target(&mut self) {
        let ok = match self.target {
            Target::Spread(id) => self.doc.project.spread_index(id).is_some(),
            Target::Cover => self.doc.project.cover.is_some(),
        };
        if !ok {
            self.target = self.doc.project.spreads.first().map(|s| Target::Spread(s.id)).unwrap_or(Target::Cover);
            self.sel.clear();
            self.crop_mode = None;
        }
    }

    /// Left/Right (and ⌘←/⌘→): previous / next spread.
    pub fn step_spread(&mut self, step: i32) {
        let ids: Vec<_> = self.project().spreads.iter().map(|s| s.id).collect();
        if ids.is_empty() {
            return;
        }
        let j = match self.spread_index() {
            Some(i) => (i as i32 + step).clamp(0, ids.len() as i32 - 1) as usize,
            None => 0,
        };
        self.go_to(Target::Spread(ids[j]));
    }

    pub fn go_to(&mut self, t: Target) {
        if self.target != t {
            self.target = t;
            self.sel.clear();
            self.crop_mode = None;
            self.preview = None;
            self.view.fit_pending = true;
            self.view.user_moved = false;
        }
    }

    /// Execute a command and report problems to the user.
    pub fn exec(&mut self, cmd: Command) -> bool {
        if cmd.is_major() {
            self.store.send(StoreReq::Snapshot { reason: format!("before {}", cmd.label().to_lowercase()), manual: false });
        }
        match self.doc.execute(cmd) {
            Ok(Event::Nothing) => false,
            Ok(_) => {
                self.after_edit();
                true
            }
            Err(e) => {
                self.msg(e.to_string(), MsgKind::Warn);
                false
            }
        }
    }

    pub fn after_edit(&mut self) {
        self.edited = true;
        self.last_change = Instant::now();
        self.ensure_target();
        self.sync_templates();
    }

    /// Rebuild the template list when album kind, template selection, spacing or album size changed
    /// (also after undo/redo of those edits).
    pub fn sync_templates(&mut self) {
        let p = &self.doc.project;
        let key = format!("{:?}|{:?}|{:?}|{:?}|{}x{}", p.settings.album_kind, p.settings.template_groups, p.settings.template_ids, p.settings.layout, p.spec.page_width_um, p.spec.page_height_um);
        if key != self.templates_key {
            self.templates_key = key;
            self.reload_templates();
        }
    }

    /// Replace the current surface (spread or cover) with an edited copy.
    pub fn commit(&mut self, s: Spread, label: &str) -> bool {
        match self.target {
            Target::Cover => {
                let mut c = self.doc.project.cover.clone().unwrap_or_default();
                c.content = s;
                self.exec(Command::SetCover { cover: Some(c) })
            }
            Target::Spread(_) => self.exec(Command::SetSpread { spread: s, label: label.into() }),
        }
    }

    /// Commit from a continuous control (slider, colour picker): consecutive edits with the same
    /// label within a second become one undo step.
    pub fn commit_live(&mut self, s: Spread, label: &str) -> bool {
        let merge = self.last_change.elapsed() < Duration::from_millis(1200);
        let cmd = match self.target {
            Target::Cover => {
                let mut c = self.doc.project.cover.clone().unwrap_or_default();
                c.content = s;
                Command::SetCover { cover: Some(c) }
            }
            Target::Spread(_) => Command::SetSpread { spread: s, label: label.into() },
        };
        match self.doc.execute_merge(cmd, merge) {
            Ok(Event::Nothing) => false,
            Ok(_) => {
                self.after_edit();
                true
            }
            Err(e) => {
                self.msg(e.to_string(), MsgKind::Warn);
                false
            }
        }
    }

    pub fn undo(&mut self) {
        if self.doc.undo().is_some() {
            self.after_edit();
        }
    }

    pub fn redo(&mut self) {
        if self.doc.redo().is_some() {
            self.after_edit();
        }
    }

    /// Re-stat originals (cheap) at most every 15 s.
    pub fn refresh_status(&mut self) {
        let mut changed = false;
        for m in self.media.iter_mut() {
            let st = oto_media::file_status(m);
            let missing = st == FileStatus::Missing;
            if m.missing != missing {
                m.missing = missing;
                changed = true;
            }
            self.file_status.insert(m.id, st);
        }
        if changed {
            self.media_dirty = true;
        }
        self.status_checked = Instant::now();
        self.preflight_rev = u64::MAX;
    }

    pub fn run_preflight(&mut self) {
        let media: HashMap<MediaId, MediaItem> = self.media.iter().map(|m| (m.id, m.clone())).collect();
        let cg = if self.doc.project.cover.is_some() { Some(self.cover_geometry()) } else { None };
        self.preflight = preflight::run(&preflight::Input { project: &self.doc.project, media: &media, lab: self.lab.as_ref(), status: &self.file_status, cover_geometry: cg });
        self.preflight_rev = self.doc.revision;
    }

    pub fn tick(&mut self) {
        if self.status_checked.elapsed() > Duration::from_secs(15) {
            self.refresh_status();
        }
        if self.preflight_rev != self.doc.revision && self.last_change.elapsed() > Duration::from_millis(400) {
            self.run_preflight();
        }
        self.autosave();
        while let Ok(r) = self.store.rx.try_recv() {
            match r {
                StoreResp::Saved { revision, kind } => {
                    if revision >= self.last_saved_rev {
                        self.last_saved_rev = revision;
                    }
                    self.save_state = if self.doc.revision == self.last_saved_rev && !self.media_dirty { SaveState::Saved } else { SaveState::Dirty };
                    self.last_saved_at = time::OffsetDateTime::now_local()
                        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
                        .format(&time::macros::format_description!("[hour]:[minute]:[second]"))
                        .unwrap_or_default();
                    if kind == "save" {
                        self.msg("Project saved.", MsgKind::Ok);
                    }
                }
                StoreResp::SaveFailed(e) => {
                    self.save_state = SaveState::Failed;
                    self.msg(format!("Autosave failed: {e}"), MsgKind::Error);
                }
                StoreResp::Snapshot(Ok(s)) => {
                    if s.manual {
                        self.msg(format!("Snapshot taken ({}).", s.reason), MsgKind::Ok);
                    }
                    self.store.send(StoreReq::ListSnapshots);
                }
                StoreResp::Snapshot(Err(e)) => self.msg(format!("Snapshot failed: {e}"), MsgKind::Error),
                StoreResp::Snapshots(v) => self.snapshots = v,
                StoreResp::Restored(Ok(restored)) => {
                    let (p, media) = *restored;
                    self.doc = Document::new(p);
                    self.media = media;
                    self.media_idx = media_index(&self.media);
                    self.ensure_target();
                    self.view.fit_pending = true;
                    self.preflight_rev = u64::MAX;
                    self.msg("Snapshot restored. A safety snapshot of the previous state was kept.", MsgKind::Ok);
                    self.store.send(StoreReq::ListSnapshots);
                }
                StoreResp::Restored(Err(e)) => self.msg(format!("Restore failed: {e}"), MsgKind::Error),
                StoreResp::SavedAs(Ok(path)) => {
                    self.root = path.clone();
                    self.tex = TexCache::new(path.join("cache"));
                    self.msg(format!("Saved as {}", path.display()), MsgKind::Ok);
                }
                StoreResp::SavedAs(Err(e)) => self.msg(format!("Save As failed: {e}"), MsgKind::Error),
                StoreResp::Closed => self.closed = true,
                StoreResp::Error(e) => self.msg(e, MsgKind::Error),
            }
        }
    }

    fn dirty(&self) -> bool {
        self.doc.revision != self.last_saved_rev || self.media_dirty
    }

    /// Debounce 2 s after the last change, but commit at least every `autosave_secs` while dirty.
    fn autosave(&mut self) {
        if self.closing {
            return;
        }
        if !self.dirty() {
            if self.save_state == SaveState::Dirty {
                self.save_state = SaveState::Saved;
            }
            return;
        }
        if self.save_state != SaveState::Saving {
            self.save_state = SaveState::Dirty;
        }
        let every = Duration::from_secs(self.doc.project.settings.autosave_secs.clamp(5, 10) as u64);
        let quiet = self.last_change.elapsed() > Duration::from_secs(2);
        if (quiet || self.last_save_req.elapsed() > every) && self.last_save_req.elapsed() > Duration::from_millis(900) {
            self.save_now("autosave");
        }
        let snap_every = Duration::from_secs(self.doc.project.settings.snapshot_minutes.max(1) as u64 * 60);
        if self.last_snapshot.elapsed() > snap_every {
            self.last_snapshot = Instant::now();
            self.store.send(StoreReq::Snapshot { reason: "periodic".into(), manual: false });
        }
    }

    pub fn save_now(&mut self, kind: &'static str) {
        self.last_save_req = Instant::now();
        self.save_state = SaveState::Saving;
        self.media_dirty = false;
        self.store.send(StoreReq::Save { project: Box::new(self.doc.project.clone()), media: Arc::new(self.media.clone()), kind, revision: self.doc.revision });
    }

    /// Used when the app is quitting (e.g. ⌘Q or a system quit): save, close the package cleanly and wait
    /// briefly for the DB thread, so the next start does not report a crash.
    pub fn close_and_wait(&mut self, timeout: std::time::Duration) {
        if !self.closing {
            self.close();
        }
        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.store.rx.recv_timeout(left) {
                Ok(StoreResp::Closed) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }

    pub fn close(&mut self) {
        self.closing = true;
        self.store.send(StoreReq::Save { project: Box::new(self.doc.project.clone()), media: Arc::new(self.media.clone()), kind: "close", revision: self.doc.revision });
        self.store.send(StoreReq::Close);
    }

    pub fn usage(&self) -> HashMap<MediaId, u32> {
        self.doc.project.usage()
    }

    pub fn issues_for(&self, id: SpreadId) -> (usize, usize) {
        let mut c = 0;
        let mut w = 0;
        for i in self.preflight.for_spread(id) {
            if i.overridden.is_some() {
                continue;
            }
            match i.severity {
                oto_print::preflight::Severity::Critical => c += 1,
                oto_print::preflight::Severity::Warning => w += 1,
                _ => {}
            }
        }
        (c, w)
    }

    pub fn template(&self, id: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.id == id)
    }
}
