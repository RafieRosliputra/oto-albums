//! Commands, validation and undo/redo.
//!
//! The UI never mutates the project directly: it computes a proposed value and submits a
//! [`Command`]. [`Document::execute`] validates it, applies it, records an undo entry
//! (before/after delta of the affected scope) and returns an [`Event`].

use crate::model::{Cover, Project, ProjectStatus, Spread, SpreadId, Timeline};
use crate::spec::AlbumSpec;

pub const UNDO_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub enum Command {
    /// Replace one spread (by id) with an edited copy.
    SetSpread {
        spread: Spread,
        label: String,
    },
    /// Replace all spreads (Auto Build apply, timeline reflow).
    SetSpreads {
        spreads: Vec<Spread>,
        label: String,
    },
    InsertSpread {
        at: usize,
        spread: Spread,
    },
    DeleteSpread {
        id: SpreadId,
    },
    MoveSpread {
        from: usize,
        to: usize,
    },
    SetLocked {
        id: SpreadId,
        locked: bool,
    },
    SetCover {
        cover: Option<Cover>,
    },
    SetSpec {
        spec: AlbumSpec,
    },
    SetMeta {
        name: String,
        client: String,
        due_date: Option<String>,
        status: ProjectStatus,
    },
    SetStaged {
        staged: Vec<crate::model::MediaId>,
    },
    SetOverrides {
        overrides: Vec<crate::model::PreflightOverride>,
    },
    SetProjectSettings {
        settings: crate::model::ProjectSettings,
    },
}

impl Command {
    pub fn label(&self) -> String {
        match self {
            Command::SetSpread { label, .. } | Command::SetSpreads { label, .. } => label.clone(),
            Command::InsertSpread { .. } => "Add spread".into(),
            Command::DeleteSpread { .. } => "Delete spread".into(),
            Command::MoveSpread { .. } => "Reorder spreads".into(),
            Command::SetLocked { locked, .. } => if *locked { "Lock spread" } else { "Unlock spread" }.into(),
            Command::SetCover { .. } => "Edit cover".into(),
            Command::SetSpec { .. } => "Change album size".into(),
            Command::SetMeta { .. } => "Edit project info".into(),
            Command::SetStaged { .. } => "Update timeline".into(),
            Command::SetOverrides { .. } => "Preflight override".into(),
            Command::SetProjectSettings { .. } => "Project settings".into(),
        }
    }
    /// Operations that warrant an automatic snapshot first.
    pub fn is_major(&self) -> bool {
        matches!(self, Command::SetSpreads { .. } | Command::DeleteSpread { .. } | Command::SetSpec { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    SpreadChanged(SpreadId),
    SpreadsChanged,
    CoverChanged,
    SpecChanged,
    MetaChanged,
    Nothing,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CommandError {
    #[error("Spread {0} is locked. Unlock it first.")]
    Locked(usize),
    #[error("That spread no longer exists.")]
    NoSuchSpread,
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
enum State {
    Spread(Spread),
    Spreads(Vec<Spread>),
    Cover(Option<Cover>),
    Spec(AlbumSpec, Vec<Spread>),
    Meta(String, String, Option<String>, ProjectStatus),
    Staged(Vec<crate::model::MediaId>),
    Overrides(Vec<crate::model::PreflightOverride>),
    Settings(crate::model::ProjectSettings),
}

#[derive(Debug, Clone)]
struct Entry {
    label: String,
    before: State,
    after: State,
}

#[derive(Debug, Default)]
pub struct Document {
    pub project: Project,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    pub dirty: bool,
    /// Bumped on every change; used for autosave debounce and render caches.
    pub revision: u64,
}

impl Default for Project {
    fn default() -> Self {
        Project::new("Untitled", AlbumSpec::default(), 0)
    }
}

impl Document {
    pub fn new(project: Project) -> Self {
        Document { project, undo: Vec::new(), redo: Vec::new(), dirty: false, revision: 0 }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    fn capture(&self, s: &State) -> State {
        let p = &self.project;
        match s {
            State::Spread(sp) => State::Spread(p.spreads.iter().find(|x| x.id == sp.id).cloned().unwrap_or_else(|| sp.clone())),
            State::Spreads(_) => State::Spreads(p.spreads.clone()),
            State::Cover(_) => State::Cover(p.cover.clone()),
            State::Spec(..) => State::Spec(p.spec.clone(), p.spreads.clone()),
            State::Meta(..) => State::Meta(p.name.clone(), p.client.clone(), p.due_date.clone(), p.status),
            State::Staged(_) => State::Staged(p.timeline.staged.clone()),
            State::Overrides(_) => State::Overrides(p.settings.overrides.clone()),
            State::Settings(_) => State::Settings(p.settings.clone()),
        }
    }

    fn restore(&mut self, s: State) -> Event {
        let p = &mut self.project;
        let ev = match s {
            State::Spread(sp) => {
                let id = sp.id;
                if let Some(slot) = p.spreads.iter_mut().find(|x| x.id == id) {
                    *slot = sp;
                }
                Event::SpreadChanged(id)
            }
            State::Spreads(v) => {
                p.spreads = v;
                Event::SpreadsChanged
            }
            State::Cover(c) => {
                p.cover = c;
                Event::CoverChanged
            }
            State::Spec(spec, spreads) => {
                p.spec = spec;
                p.spreads = spreads;
                Event::SpecChanged
            }
            State::Meta(n, c, d, s) => {
                p.name = n;
                p.client = c;
                p.due_date = d;
                p.status = s;
                Event::MetaChanged
            }
            State::Staged(v) => {
                p.timeline.staged = v;
                Event::SpreadsChanged
            }
            State::Overrides(v) => {
                p.settings.overrides = v;
                Event::MetaChanged
            }
            State::Settings(v) => {
                p.settings = v;
                Event::MetaChanged
            }
        };
        self.after_change();
        ev
    }

    fn after_change(&mut self) {
        let staged = std::mem::take(&mut self.project.timeline.staged);
        let mut t = Timeline::from_spreads(&self.project.spreads);
        let placed: std::collections::HashSet<_> = t.order.iter().copied().collect();
        t.staged = staged.into_iter().filter(|id| !placed.contains(id)).collect();
        self.project.timeline = t;
        self.project.modified_at = crate::model::now_iso();
        self.dirty = true;
        self.revision += 1;
    }

    fn locked_index(&self, id: SpreadId) -> Option<usize> {
        self.project.spreads.iter().position(|s| s.id == id && s.locked)
    }

    pub fn execute(&mut self, cmd: Command) -> Result<Event, CommandError> {
        self.execute_ext(cmd, false)
    }

    /// Like `execute`, but when `merge` is true and the previous undo entry has the same label
    /// and scope, the two edits become one undo step (slider drags, colour pickers).
    pub fn execute_merge(&mut self, cmd: Command, merge: bool) -> Result<Event, CommandError> {
        self.execute_ext(cmd, merge)
    }

    fn execute_ext(&mut self, cmd: Command, merge: bool) -> Result<Event, CommandError> {
        let label = cmd.label();
        let (before_key, ev) = match &cmd {
            Command::SetSpread { spread, .. } => {
                let idx = self.project.spread_index(spread.id).ok_or(CommandError::NoSuchSpread)?;
                if self.project.spreads[idx].locked && spread.locked {
                    // Allow pure metadata (notes) edits on locked spreads, nothing else.
                    let cur = &self.project.spreads[idx];
                    if cur.frames != spread.frames || cur.texts != spread.texts || cur.shapes != spread.shapes || cur.background != spread.background {
                        return Err(CommandError::Locked(idx + 1));
                    }
                }
                (State::Spread(spread.clone()), Event::SpreadChanged(spread.id))
            }
            Command::SetSpreads { spreads, .. } => {
                // Locked spreads must survive unchanged at their position.
                for (i, s) in self.project.spreads.iter().enumerate() {
                    if s.locked {
                        match spreads.iter().position(|n| n.id == s.id) {
                            Some(j) if spreads[j] == *s => {}
                            _ => return Err(CommandError::Locked(i + 1)),
                        }
                    }
                }
                (State::Spreads(vec![]), Event::SpreadsChanged)
            }
            Command::InsertSpread { .. } | Command::MoveSpread { .. } => (State::Spreads(vec![]), Event::SpreadsChanged),
            Command::DeleteSpread { id } => {
                if let Some(i) = self.locked_index(*id) {
                    return Err(CommandError::Locked(i + 1));
                }
                (State::Spreads(vec![]), Event::SpreadsChanged)
            }
            Command::SetLocked { id, .. } => {
                let sp = self.project.spreads.iter().find(|s| s.id == *id).ok_or(CommandError::NoSuchSpread)?;
                (State::Spread(sp.clone()), Event::SpreadChanged(*id))
            }
            Command::SetCover { .. } => (State::Cover(None), Event::CoverChanged),
            Command::SetSpec { spec } => {
                let issues = spec.validate();
                if let Some(i) = issues.iter().find(|i| i.field != "bleed" || spec.bleed_um != 0) {
                    return Err(CommandError::Invalid(i.message.clone()));
                }
                (State::Spec(spec.clone(), vec![]), Event::SpecChanged)
            }
            Command::SetMeta { .. } => (State::Meta(String::new(), String::new(), None, ProjectStatus::Draft), Event::MetaChanged),
            Command::SetStaged { .. } => (State::Staged(vec![]), Event::SpreadsChanged),
            Command::SetOverrides { .. } => (State::Overrides(vec![]), Event::MetaChanged),
            Command::SetProjectSettings { .. } => (State::Settings(Default::default()), Event::MetaChanged),
        };
        let before = self.capture(&before_key);
        let p = &mut self.project;
        match cmd {
            Command::SetSpread { mut spread, .. } => {
                sanitize_spread(&mut spread);
                if let Some(slot) = p.spreads.iter_mut().find(|x| x.id == spread.id) {
                    *slot = spread;
                }
            }
            Command::SetSpreads { mut spreads, .. } => {
                spreads.iter_mut().for_each(sanitize_spread);
                p.spreads = spreads;
            }
            Command::InsertSpread { at, mut spread } => {
                sanitize_spread(&mut spread);
                let at = at.min(p.spreads.len());
                p.spreads.insert(at, spread);
            }
            Command::DeleteSpread { id } => p.spreads.retain(|s| s.id != id),
            Command::MoveSpread { from, to } => {
                if from < p.spreads.len() && to < p.spreads.len() && from != to {
                    let s = p.spreads.remove(from);
                    p.spreads.insert(to, s);
                }
            }
            Command::SetLocked { id, locked } => {
                if let Some(s) = p.spreads.iter_mut().find(|s| s.id == id) {
                    s.locked = locked;
                }
            }
            Command::SetCover { cover } => p.cover = cover,
            Command::SetSpec { mut spec } => {
                spec.normalize();
                p.spec = spec;
            }
            Command::SetMeta { name, client, due_date, status } => {
                p.name = if name.trim().is_empty() { "Untitled".into() } else { name };
                p.client = client;
                p.due_date = due_date;
                p.status = status;
            }
            Command::SetStaged { staged } => p.timeline.staged = staged,
            Command::SetOverrides { overrides } => p.settings.overrides = overrides,
            Command::SetProjectSettings { settings } => p.settings = settings,
        }
        let after = self.capture(&before);
        if format!("{before:?}") == format!("{after:?}") {
            return Ok(Event::Nothing);
        }
        let same_scope = |a: &State, b: &State| match (a, b) {
            (State::Spread(x), State::Spread(y)) => x.id == y.id,
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        };
        if merge && self.redo.is_empty() {
            if let Some(last) = self.undo.last_mut() {
                if last.label == label && same_scope(&last.before, &before) {
                    last.after = after;
                    self.after_change();
                    return Ok(ev);
                }
            }
        }
        self.undo.push(Entry { label, before, after });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.after_change();
        Ok(ev)
    }

    pub fn undo(&mut self) -> Option<Event> {
        let e = self.undo.pop()?;
        let ev = self.restore(e.before.clone());
        self.redo.push(e);
        Some(ev)
    }

    pub fn redo(&mut self) -> Option<Event> {
        let e = self.redo.pop()?;
        let ev = self.restore(e.after.clone());
        self.undo.push(e);
        Some(ev)
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/// Keep geometry legal: everything stays inside the surface, opacities in range.
pub fn sanitize_spread(s: &mut Spread) {
    for f in &mut s.frames {
        f.rect_norm = f.rect_norm.clamp_to_unit();
        f.opacity = f.opacity.clamp(0.0, 1.0);
        f.crop.zoom = f.crop.zoom.clamp(0.05, 20.0);
        f.radius_mm = f.radius_mm.clamp(0.0, 200.0);
        f.border.width_mm = f.border.width_mm.clamp(0.0, 50.0);
    }
    for t in &mut s.texts {
        t.rect_norm = t.rect_norm.clamp_to_unit();
        t.style.size_pt = t.style.size_pt.clamp(2.0, 400.0);
        t.style.line_height = t.style.line_height.clamp(0.5, 4.0);
    }
    for sh in &mut s.shapes {
        sh.rect_norm = sh.rect_norm.clamp_to_unit();
        sh.opacity = sh.opacity.clamp(0.0, 1.0);
    }
    if let Some(p) = &mut s.background.pattern {
        p.scale_mm = p.scale_mm.clamp(1.0, 200.0);
        p.opacity = p.opacity.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::model::{Frame, TextBlock, TextStyle};

    fn doc() -> Document {
        Document::new(Project::new("Test", AlbumSpec::default(), 3))
    }

    #[test]
    fn undo_redo_spread_edit() {
        let mut d = doc();
        let mut s = d.project.spreads[0].clone();
        s.texts.push(TextBlock::new(Rect::new(0.1, 0.1, 0.3, 0.1), "Hello", TextStyle::default()));
        d.execute(Command::SetSpread { spread: s, label: "Add text".into() }).unwrap();
        assert_eq!(d.project.spreads[0].texts.len(), 1);
        assert_eq!(d.undo_label(), Some("Add text"));
        d.undo();
        assert!(d.project.spreads[0].texts.is_empty());
        d.redo();
        assert_eq!(d.project.spreads[0].texts.len(), 1);
    }

    #[test]
    fn locked_spreads_are_protected() {
        let mut d = doc();
        let id = d.project.spreads[1].id;
        d.execute(Command::SetLocked { id, locked: true }).unwrap();
        let mut s = d.project.spreads[1].clone();
        s.frames.push(Frame::new(Rect::new(0.1, 0.1, 0.2, 0.2)));
        assert_eq!(d.execute(Command::SetSpread { spread: s, label: "x".into() }), Err(CommandError::Locked(2)));
        assert_eq!(d.execute(Command::DeleteSpread { id }), Err(CommandError::Locked(2)));
        assert_eq!(d.execute(Command::SetSpreads { spreads: vec![], label: "x".into() }), Err(CommandError::Locked(2)));
    }

    #[test]
    fn history_is_bounded_and_noops_ignored() {
        let mut d = doc();
        for i in 0..150 {
            let mut s = d.project.spreads[0].clone();
            s.notes = format!("{i}");
            d.execute(Command::SetSpread { spread: s, label: "n".into() }).unwrap();
        }
        let s = d.project.spreads[0].clone();
        assert_eq!(d.execute(Command::SetSpread { spread: s, label: "same".into() }).unwrap(), Event::Nothing);
        let mut n = 0;
        while d.undo().is_some() {
            n += 1;
        }
        assert_eq!(n, UNDO_LIMIT);
    }

    #[test]
    fn merged_edits_are_one_undo_step() {
        let mut d = doc();
        for i in 0..10 {
            let mut s = d.project.spreads[0].clone();
            s.background.color.r = i;
            d.execute_merge(Command::SetSpread { spread: s, label: "Background".into() }, true).unwrap();
        }
        assert_eq!(d.project.spreads[0].background.color.r, 9);
        d.undo();
        assert_eq!(d.project.spreads[0].background.color.r, 255);
        assert!(!d.can_undo());
    }

    #[test]
    fn sanitizes_geometry() {
        let mut d = doc();
        let mut s = d.project.spreads[0].clone();
        s.frames.push(Frame::new(Rect::new(1.5, -0.2, 0.4, 0.4)));
        d.execute(Command::SetSpread { spread: s, label: "x".into() }).unwrap();
        assert!(d.project.spreads[0].frames[0].rect_norm.within_unit());
    }
}
