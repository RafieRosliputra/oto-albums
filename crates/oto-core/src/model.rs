//! Project domain model. SQLite (oto-store) is the source of truth; this is the in-memory form.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::designs::{AlbumKind, LayoutParams};
use crate::geometry::{Rect, Vec2};
use crate::spec::AlbumSpec;
use crate::units::MilliDeg;

pub type MediaId = Uuid;
pub type SpreadId = Uuid;
pub type ItemId = Uuid;
pub type TemplateId = String;

pub const SCHEMA_VERSION: u32 = 1;

pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

pub fn now_iso() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Draft,
    Design,
    Proofing,
    Approved,
    Exported,
    Delivered,
}

impl ProjectStatus {
    pub const ALL: [ProjectStatus; 6] = [Self::Draft, Self::Design, Self::Proofing, Self::Approved, Self::Exported, Self::Delivered];
    pub fn key(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Design => "design",
            Self::Proofing => "proofing",
            Self::Approved => "approved",
            Self::Exported => "exported",
            Self::Delivered => "delivered",
        }
    }
    pub fn parse(s: &str) -> Self {
        Self::ALL.into_iter().find(|v| v.key() == s).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const PAPER: Rgba = Rgba::rgb(0xFB, 0xF8, 0xF2);
    pub const CHARCOAL: Rgba = Rgba::rgb(0x17, 0x19, 0x1C);
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        Some(Rgba::rgb(p(0)?, p(2)?, p(4)?))
    }
    pub fn f32(self) -> [f32; 3] {
        [self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    /// Point of the (oriented) image shown at the frame centre, 0..1.
    pub center_norm: Vec2,
    /// 1.0 = image exactly covers the frame.
    pub zoom: f32,
    pub rotation_mdeg: MilliDeg,
}

impl Default for Crop {
    fn default() -> Self {
        Crop { center_norm: Vec2::CENTER, zoom: 1.0, rotation_mdeg: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    #[default]
    Fill,
    Fit,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FocusPoint {
    pub x: f32,
    pub y: f32,
    /// Set by the user (wins over automatic estimates).
    pub manual: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub width_mm: f32,
    pub color: Rgba,
}

impl Default for Border {
    fn default() -> Self {
        Border { width_mm: 0.0, color: Rgba::WHITE }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Adjust {
    /// -1..1
    pub brightness: f32,
    /// -1..1
    pub contrast: f32,
    pub bw: bool,
}

impl Default for Adjust {
    fn default() -> Self {
        Adjust { brightness: 0.0, contrast: 0.0, bw: false }
    }
}

impl Adjust {
    pub fn is_identity(&self) -> bool {
        self.brightness == 0.0 && self.contrast == 0.0 && !self.bw
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub id: ItemId,
    /// Normalized to the trimmed spread (0..1 both axes).
    pub rect_norm: Rect,
    pub z: i32,
    pub image_id: Option<MediaId>,
    pub crop: Crop,
    /// True when the user changed the crop by hand (solver never overrides it).
    pub crop_manual: bool,
    pub focus: Option<FocusPoint>,
    pub rotation_mdeg: MilliDeg,
    pub opacity: f32,
    pub border: Border,
    pub radius_mm: f32,
    pub fit: FitMode,
    pub adjust: Adjust,
    pub locked: bool,
    pub allow_rotate: bool,
    /// Role from the template ("hero", "support", "detail").
    pub role: String,
}

impl Frame {
    pub fn new(rect: Rect) -> Self {
        Frame {
            id: new_id(),
            rect_norm: rect,
            z: 0,
            image_id: None,
            crop: Crop::default(),
            crop_manual: false,
            focus: None,
            rotation_mdeg: 0,
            opacity: 1.0,
            border: Border::default(),
            radius_mm: 0.0,
            fit: FitMode::Fill,
            adjust: Adjust::default(),
            locked: false,
            allow_rotate: true,
            role: "support".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_family: String,
    pub size_pt: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Rgba,
    pub align: Align,
    /// Multiple of font size.
    pub line_height: f32,
    /// Extra tracking in 1/1000 em.
    pub letter_spacing: f32,
    pub kerning: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { font_family: "Lora".into(), size_pt: 14.0, bold: false, italic: false, color: Rgba::CHARCOAL, align: Align::Center, line_height: 1.3, letter_spacing: 0.0, kerning: true }
    }
}

/// Named paragraph styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylePreset {
    H1,
    H2,
    Body,
    Caption,
    CoverTitle,
    CoverSubtitle,
}

impl StylePreset {
    pub const ALL: [StylePreset; 6] = [Self::H1, Self::H2, Self::Body, Self::Caption, Self::CoverTitle, Self::CoverSubtitle];
    pub fn label(self) -> &'static str {
        match self {
            Self::H1 => "H1",
            Self::H2 => "H2",
            Self::Body => "Body",
            Self::Caption => "Caption",
            Self::CoverTitle => "Cover title",
            Self::CoverSubtitle => "Cover subtitle",
        }
    }
    pub fn style(self, pair: &FontPair) -> TextStyle {
        let base = TextStyle::default();
        match self {
            Self::H1 => TextStyle { font_family: pair.display.clone(), size_pt: 36.0, line_height: 1.1, ..base },
            Self::H2 => TextStyle { font_family: pair.display.clone(), size_pt: 22.0, line_height: 1.15, ..base },
            Self::Body => TextStyle { font_family: pair.text.clone(), size_pt: 11.0, line_height: 1.45, align: Align::Left, ..base },
            Self::Caption => TextStyle { font_family: pair.text.clone(), size_pt: 8.5, italic: true, letter_spacing: 20.0, ..base },
            Self::CoverTitle => TextStyle { font_family: pair.display.clone(), size_pt: 48.0, line_height: 1.05, ..base },
            Self::CoverSubtitle => TextStyle { font_family: pair.text.clone(), size_pt: 12.0, letter_spacing: 180.0, ..base },
        }
    }
}

/// Licensed (OFL) album font pairings bundled with the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontPair {
    pub name: String,
    pub display: String,
    pub text: String,
}

pub fn font_pairs() -> Vec<FontPair> {
    let p = |n: &str, d: &str, t: &str| FontPair { name: n.into(), display: d.into(), text: t.into() };
    vec![
        p("Classic Serif", "Playfair Display", "Lora"),
        p("Soft Editorial", "Cormorant Garamond", "Montserrat"),
        p("Modern Clean", "Montserrat", "Inter"),
        p("Romantic Script", "Great Vibes", "Cormorant Garamond"),
        p("Quiet Book", "Lora", "Lora"),
    ]
}

/// Font families bundled with the app (all SIL OFL 1.1).
pub const ALBUM_FONTS: [&str; 6] = ["Inter", "Lora", "Playfair Display", "Cormorant Garamond", "Montserrat", "Great Vibes"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub id: ItemId,
    pub rect_norm: Rect,
    pub text: String,
    pub style: TextStyle,
    pub rotation_mdeg: MilliDeg,
    pub z: i32,
    pub locked: bool,
}

impl TextBlock {
    pub fn new(rect: Rect, text: &str, style: TextStyle) -> Self {
        TextBlock { id: new_id(), rect_norm: rect, text: text.into(), style, rotation_mdeg: 0, z: 100, locked: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Line,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub id: ItemId,
    pub kind: ShapeKind,
    pub rect_norm: Rect,
    pub fill: Option<Rgba>,
    pub stroke: Option<Rgba>,
    pub stroke_mm: f32,
    pub opacity: f32,
    pub rotation_mdeg: MilliDeg,
    pub z: i32,
    pub locked: bool,
}

impl Shape {
    pub fn new(kind: ShapeKind, rect: Rect) -> Self {
        Shape {
            id: new_id(),
            kind,
            rect_norm: rect,
            fill: if kind == ShapeKind::Line { None } else { Some(Rgba::rgb(0xE8, 0xE1, 0xD3)) },
            stroke: if kind == ShapeKind::Line { Some(Rgba::CHARCOAL) } else { None },
            stroke_mm: 0.5,
            opacity: 1.0,
            rotation_mdeg: 0,
            z: -10,
            locked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    Dots,
    Stripes,
    Grid,
    Diagonal,
    Chevron,
    Waves,
    Confetti,
    FineGrid,
    Windowpane,
    DoubleGrid,
    Tattersall,
    Pinstripe,
    Ticking,
    Pindot,
    OffsetDot,
    CrossGrid,
    Lattice,
    Trellis,
    Herringbone,
    Basketweave,
    Brick,
    Scales,
    Arches,
    Meander,
    Quatrefoil,
    Rings,
    FineChevron,
    Twill,
    Plaid,
}

impl PatternKind {
    pub const ALL: [PatternKind; 29] = [
        Self::Dots,
        Self::Stripes,
        Self::Grid,
        Self::Diagonal,
        Self::Chevron,
        Self::Waves,
        Self::Confetti,
        Self::FineGrid,
        Self::Windowpane,
        Self::DoubleGrid,
        Self::Tattersall,
        Self::Pinstripe,
        Self::Ticking,
        Self::Pindot,
        Self::OffsetDot,
        Self::CrossGrid,
        Self::Lattice,
        Self::Trellis,
        Self::Herringbone,
        Self::Basketweave,
        Self::Brick,
        Self::Scales,
        Self::Arches,
        Self::Meander,
        Self::Quatrefoil,
        Self::Rings,
        Self::FineChevron,
        Self::Twill,
        Self::Plaid,
    ];
    /// The formal grid collection (thin lines, quiet geometry) shown first in the pattern picker.
    pub const FORMAL: [PatternKind; 22] = [
        Self::FineGrid,
        Self::Windowpane,
        Self::DoubleGrid,
        Self::Tattersall,
        Self::Pinstripe,
        Self::Ticking,
        Self::Pindot,
        Self::OffsetDot,
        Self::CrossGrid,
        Self::Lattice,
        Self::Trellis,
        Self::Herringbone,
        Self::Basketweave,
        Self::Brick,
        Self::Scales,
        Self::Arches,
        Self::Meander,
        Self::Quatrefoil,
        Self::Rings,
        Self::FineChevron,
        Self::Twill,
        Self::Plaid,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Dots => "Dots",
            Self::Stripes => "Stripes",
            Self::Grid => "Grid",
            Self::Diagonal => "Diagonal",
            Self::Chevron => "Chevron",
            Self::Waves => "Waves",
            Self::Confetti => "Confetti",
            Self::FineGrid => "Fine grid",
            Self::Windowpane => "Windowpane",
            Self::DoubleGrid => "Double grid",
            Self::Tattersall => "Tattersall",
            Self::Pinstripe => "Pinstripe",
            Self::Ticking => "Ticking",
            Self::Pindot => "Pin dot",
            Self::OffsetDot => "Offset dot",
            Self::CrossGrid => "Cross grid",
            Self::Lattice => "Lattice",
            Self::Trellis => "Trellis",
            Self::Herringbone => "Herringbone",
            Self::Basketweave => "Basketweave",
            Self::Brick => "Brick",
            Self::Scales => "Scales",
            Self::Arches => "Arches",
            Self::Meander => "Meander",
            Self::Quatrefoil => "Quatrefoil",
            Self::Rings => "Rings",
            Self::FineChevron => "Fine chevron",
            Self::Twill => "Twill",
            Self::Plaid => "Plaid",
        }
    }
    /// Repeat size that reads well at print size, in mm.
    pub fn default_scale_mm(self) -> f32 {
        match self {
            Self::Dots => 8.0,
            Self::Stripes => 8.0,
            Self::Grid => 8.0,
            Self::Diagonal => 8.0,
            Self::Chevron => 8.0,
            Self::Waves => 8.0,
            Self::Confetti => 8.0,
            Self::FineGrid => 6.0,
            Self::Windowpane => 26.0,
            Self::DoubleGrid => 14.0,
            Self::Tattersall => 12.0,
            Self::Pinstripe => 6.0,
            Self::Ticking => 10.0,
            Self::Pindot => 5.0,
            Self::OffsetDot => 8.0,
            Self::CrossGrid => 10.0,
            Self::Lattice => 12.0,
            Self::Trellis => 14.0,
            Self::Herringbone => 8.0,
            Self::Basketweave => 10.0,
            Self::Brick => 12.0,
            Self::Scales => 10.0,
            Self::Arches => 14.0,
            Self::Meander => 12.0,
            Self::Quatrefoil => 16.0,
            Self::Rings => 12.0,
            Self::FineChevron => 10.0,
            Self::Twill => 4.0,
            Self::Plaid => 20.0,
        }
    }
}

/// Soft pastel inks for patterns (and backgrounds): quiet, print-safe colours.
pub const PASTELS: [(&str, Rgba); 10] = [
    ("Blush", Rgba::rgb(0xE9, 0xC9, 0xC4)),
    ("Peach", Rgba::rgb(0xEE, 0xD0, 0xB8)),
    ("Butter", Rgba::rgb(0xEE, 0xE1, 0xB9)),
    ("Champagne", Rgba::rgb(0xE3, 0xD5, 0xBB)),
    ("Sage", Rgba::rgb(0xC3, 0xD1, 0xBC)),
    ("Mint", Rgba::rgb(0xC4, 0xDE, 0xD3)),
    ("Sky", Rgba::rgb(0xC2, 0xD4, 0xE3)),
    ("Lavender", Rgba::rgb(0xD3, 0xCA, 0xE3)),
    ("Dove", Rgba::rgb(0xD3, 0xD0, 0xCB)),
    ("Taupe", Rgba::rgb(0xC9, 0xBC, 0xAE)),
];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub kind: PatternKind,
    /// Repeat size in mm.
    pub scale_mm: f32,
    pub angle_mdeg: MilliDeg,
    pub opacity: f32,
    pub color: Rgba,
}

impl Pattern {
    pub fn new(kind: PatternKind) -> Self {
        Pattern { kind, scale_mm: kind.default_scale_mm(), angle_mdeg: 0, opacity: 0.9, color: PASTELS[9].1 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundImage {
    pub media_id: MediaId,
    pub crop: Crop,
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Background {
    pub color: Rgba,
    pub image: Option<BackgroundImage>,
    pub pattern: Option<Pattern>,
}

impl Default for Background {
    fn default() -> Self {
        Background { color: Rgba::WHITE, image: None, pattern: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    pub id: SpreadId,
    pub template_id: TemplateId,
    pub background: Background,
    pub frames: Vec<Frame>,
    pub texts: Vec<TextBlock>,
    pub shapes: Vec<Shape>,
    pub locked: bool,
    pub notes: String,
    /// Short explanation written by Auto Build.
    pub explain: String,
}

impl Spread {
    pub fn blank() -> Self {
        Spread {
            id: new_id(),
            template_id: "blank".into(),
            background: Background::default(),
            frames: Vec::new(),
            texts: Vec::new(),
            shapes: Vec::new(),
            locked: false,
            notes: String::new(),
            explain: String::new(),
        }
    }
    pub fn media_ids(&self) -> Vec<MediaId> {
        self.frames.iter().filter_map(|f| f.image_id).collect()
    }
    pub fn is_empty(&self) -> bool {
        self.frames.iter().all(|f| f.image_id.is_none()) && self.texts.iter().all(|t| t.text.trim().is_empty()) && self.background.image.is_none()
    }
    pub fn frame(&self, id: ItemId) -> Option<&Frame> {
        self.frames.iter().find(|f| f.id == id)
    }
    pub fn frame_mut(&mut self, id: ItemId) -> Option<&mut Frame> {
        self.frames.iter_mut().find(|f| f.id == id)
    }
    pub fn text_mut(&mut self, id: ItemId) -> Option<&mut TextBlock> {
        self.texts.iter_mut().find(|t| t.id == id)
    }
    pub fn shape_mut(&mut self, id: ItemId) -> Option<&mut Shape> {
        self.shapes.iter_mut().find(|t| t.id == id)
    }
    pub fn max_z(&self) -> i32 {
        self.frames.iter().map(|f| f.z).chain(self.texts.iter().map(|t| t.z)).chain(self.shapes.iter().map(|s| s.z)).max().unwrap_or(0)
    }
    pub fn min_z(&self) -> i32 {
        self.frames.iter().map(|f| f.z).chain(self.texts.iter().map(|t| t.z)).chain(self.shapes.iter().map(|s| s.z)).min().unwrap_or(0)
    }
    /// Remove an item of any kind; returns true if something was removed.
    pub fn remove_item(&mut self, id: ItemId) -> bool {
        let n = self.frames.len() + self.texts.len() + self.shapes.len();
        self.frames.retain(|f| f.id != id);
        self.texts.retain(|t| t.id != id);
        self.shapes.retain(|s| s.id != id);
        n != self.frames.len() + self.texts.len() + self.shapes.len()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Stamp {
    pub text: String,
    pub finish: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cover {
    /// Cover artwork; coordinates normalized to the full cover trim surface (wrap included).
    pub content: Spread,
    pub material_id: Option<String>,
    pub stamp: Option<Stamp>,
}

impl Default for Cover {
    fn default() -> Self {
        let mut content = Spread::blank();
        content.template_id = "cover-blank".into();
        Cover { content, material_id: None, stamp: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreflightOverride {
    pub code: String,
    pub spread_id: Option<SpreadId>,
    pub item_id: Option<ItemId>,
    pub reason: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub autosave_secs: u32,
    pub snapshot_minutes: u32,
    pub overrides: Vec<PreflightOverride>,
    pub font_pair: String,
    pub style_preset: String,
    #[serde(default)]
    pub album_kind: AlbumKind,
    #[serde(default)]
    pub layout: LayoutParams,
    #[serde(default)]
    pub template_groups: Vec<String>,
    #[serde(default)]
    pub template_ids: Vec<String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            autosave_secs: 6,
            snapshot_minutes: 5,
            overrides: Vec::new(),
            font_pair: "Classic Serif".into(),
            style_preset: "classic".into(),
            album_kind: AlbumKind::default(),
            layout: LayoutParams::default(),
            template_groups: Vec::new(),
            template_ids: Vec::new(),
        }
    }
}

/// Story order + cut markers. Segment `i` (between cuts) feeds spread `i`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Timeline {
    pub order: Vec<MediaId>,
    /// Sorted indices into `order` where a new spread starts (never 0).
    pub cuts: Vec<usize>,
    /// Photos added to the timeline but not yet laid out on a spread.
    #[serde(default)]
    pub staged: Vec<MediaId>,
}

impl Timeline {
    pub fn segments(&self) -> Vec<std::ops::Range<usize>> {
        let mut out = Vec::new();
        let mut start = 0;
        for &c in &self.cuts {
            if c > start && c < self.order.len() {
                out.push(start..c);
                start = c;
            }
        }
        if start < self.order.len() {
            out.push(start..self.order.len());
        }
        out
    }
    pub fn normalize(&mut self) {
        let n = self.order.len();
        self.cuts.retain(|&c| c > 0 && c < n);
        self.cuts.sort_unstable();
        self.cuts.dedup();
    }
    pub fn from_spreads(spreads: &[Spread]) -> Timeline {
        let mut t = Timeline::default();
        for s in spreads {
            let ids = s.media_ids();
            if ids.is_empty() {
                continue;
            }
            if !t.order.is_empty() {
                t.cuts.push(t.order.len());
            }
            t.order.extend(ids);
        }
        t
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,
    pub client: String,
    pub created_at: String,
    pub modified_at: String,
    pub due_date: Option<String>,
    pub status: ProjectStatus,
    pub spec: AlbumSpec,
    pub lab_id: Option<String>,
    pub lab_size_id: Option<String>,
    pub spreads: Vec<Spread>,
    pub cover: Option<Cover>,
    pub settings: ProjectSettings,
    pub timeline: Timeline,
}

impl Project {
    pub fn new(name: &str, spec: AlbumSpec, spreads: usize) -> Self {
        let now = now_iso();
        let cover = if spec.cover_type == crate::spec::CoverType::None { None } else { Some(Cover::default()) };
        Project {
            schema_version: SCHEMA_VERSION,
            id: new_id(),
            name: name.into(),
            client: String::new(),
            created_at: now.clone(),
            modified_at: now,
            due_date: None,
            status: ProjectStatus::Draft,
            spec,
            lab_id: None,
            lab_size_id: None,
            spreads: (0..spreads).map(|_| Spread::blank()).collect(),
            cover,
            settings: ProjectSettings::default(),
            timeline: Timeline::default(),
        }
    }
    pub fn page_count(&self) -> u32 {
        self.spec.pages_for_spreads(self.spreads.len() as u32)
    }
    pub fn spread_index(&self, id: SpreadId) -> Option<usize> {
        self.spreads.iter().position(|s| s.id == id)
    }
    /// How many times each media item is placed (frames + backgrounds, spreads + cover).
    pub fn usage(&self) -> std::collections::HashMap<MediaId, u32> {
        let mut m = std::collections::HashMap::new();
        let all = self.spreads.iter().chain(self.cover.as_ref().map(|c| &c.content));
        for s in all {
            for id in s.media_ids() {
                *m.entry(id).or_insert(0) += 1;
            }
            if let Some(b) = &s.background.image {
                *m.entry(b.media_id).or_insert(0) += 1;
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_segments() {
        let ids: Vec<_> = (0..6).map(|_| new_id()).collect();
        let mut t = Timeline { order: ids, cuts: vec![4, 2, 2, 0, 9], staged: vec![] };
        t.normalize();
        assert_eq!(t.cuts, vec![2, 4]);
        assert_eq!(t.segments(), vec![0..2, 2..4, 4..6]);
    }

    #[test]
    fn hex_colors() {
        assert_eq!(Rgba::from_hex("#D95D39"), Some(Rgba::rgb(0xD9, 0x5D, 0x39)));
        assert_eq!(Rgba::rgb(1, 2, 3).to_hex(), "#010203");
        assert_eq!(Rgba::from_hex("zz"), None);
    }

    #[test]
    fn project_settings_old_json_loads() {
        let old = r#"{
            "autosave_secs": 6,
            "snapshot_minutes": 5,
            "overrides": [],
            "font_pair": "Classic Serif",
            "style_preset": "classic"
        }"#;
        let settings: ProjectSettings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.album_kind, AlbumKind::Album);
        assert_eq!(settings.layout, LayoutParams::default());
        assert!(settings.template_groups.is_empty());
        assert!(settings.template_ids.is_empty());
    }
}
