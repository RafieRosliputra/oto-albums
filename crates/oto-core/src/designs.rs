//! Designed layout catalogue: 10+ original designs for every photo count 1..10, for two album types.
//!
//! * **Photobook**: printed on press paper and bound, so the centre fold eats part of the spread.
//!   Designs never put a frame across the fold, and page layouts keep an extra gutter allowance.
//! * **Photo album** (custom / flush-mount): the lab prints the whole spread as one sheet, so frames may
//!   cross the centre freely (heroes, panoramas, full-spread bleed).
//!
//! Design language (see docs/TEMPLATE-STYLE.md): frames keep real photo shapes (3:2, 2:3, 1:1), one small
//! uniform gap, equal top/bottom margin, frames aligned in one horizontal band, white page, no borders.
//! About 20 % of the designs carry a full-bleed hero page (`B`), a photo that fills one whole page edge to edge.
//!
//! Catalogue notation (one design per line, `group | name | layout`):
//! * `LEFT || RIGHT` — one block per page; `~BLOCK` — one block across the whole spread (photo album only).
//! * Block: `-` empty page, `B` full-bleed photo, `F` framed photo (thin even border), or a band of columns
//!   such as `P1 L2 S3` (portrait ×1, landscape stack ×2, square stack ×3), optionally `@0.7` to scale the band.

use serde::{Deserialize, Serialize};

use crate::spec::SpreadGeometry;
use crate::template::{aspect_range, GutterPolicy, Region, Template, TemplateFrame, TemplateKind};

/// Physical album construction; decides whether the centre of a spread may hold a photo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlbumKind {
    /// Custom / flush-mount album printed as full spreads (the app's default).
    #[default]
    Album,
    /// Bound book with a centre fold.
    Photobook,
}

impl AlbumKind {
    pub const ALL: [AlbumKind; 2] = [AlbumKind::Album, AlbumKind::Photobook];
    pub fn label(self) -> &'static str {
        match self {
            AlbumKind::Album => "Photo album",
            AlbumKind::Photobook => "Photobook",
        }
    }
    /// Tag stored on every template of this kind.
    pub fn tag(self) -> &'static str {
        match self {
            AlbumKind::Album => "album",
            AlbumKind::Photobook => "photobook",
        }
    }
    pub fn of(t: &Template) -> Option<AlbumKind> {
        AlbumKind::ALL.into_iter().find(|k| t.tags.iter().any(|g| g == k.tag()))
    }
}

/// Template groups shown in the template picker, in display order (id, label).
pub const GROUPS: [(&str, &str); 5] = [("essentials", "Essentials"), ("hero", "Hero & Full Bleed"), ("grids", "Gallery Grids"), ("story", "Storyline"), ("minimal", "Minimal")];

/// Spacing used to generate the designs for a project.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutParams {
    /// The one gap between neighbouring photos, in mm.
    pub gap_mm: f32,
    /// Top/bottom (and outer) margin as % of the trim height.
    pub margin_pct: f32,
    /// Extra space kept clear on each side of the fold in photobooks, in mm.
    pub book_gutter_mm: f32,
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams { gap_mm: 3.0, margin_pct: 10.0, book_gutter_mm: 6.0 }
    }
}

const PHOTOBOOK: &str = r#"
essentials|Single landscape|L1 || -
essentials|Single portrait|P1 || -
essentials|Single square|S1 || -
minimal|Small landscape|L1@0.7 || -
minimal|Small portrait|P1@0.7 || -
minimal|Small square|S1@0.62 || -
minimal|Postcard landscape|L1@0.5 || -
minimal|Postcard portrait|P1@0.5 || -
hero|Framed page|F || -
hero|Full page|B || -
essentials|Landscape pair|L1 || L1
essentials|Portrait pair|P1 || P1
essentials|Square pair|S1 || S1
story|Portrait and landscape|P1 || L1
story|Portrait and square|P1 || S1
grids|Stacked landscapes|L2 || -
grids|Twin portraits|P1 P1 || -
hero|Full page with landscape|B || L1@0.75
hero|Full page with portrait|B || P1@0.75
minimal|Framed page with square|F || S1@0.7
story|Landscape and stack|L1 || L2
story|Portrait and twin portraits|P1 || P1 P1
grids|Square and square stack|S1 || S2
story|Portrait and landscape stack|P1 || L2
story|Landscape and twin portraits|L1 || P1 P1
grids|Portrait beside stack|P1 L2 || -
hero|Full page with stack|B || L2
hero|Full page with twin portraits|B || P1 P1
minimal|Framed page with stack|F || L2
grids|Square stack and portrait|S2 || P1
grids|Landscape stacks|L2 || L2
grids|Portrait pairs|P1 P1 || P1 P1
grids|Square grid|S2 S2 || -
story|Stack and twin portraits|L2 || P1 P1
story|Portrait, portrait and stack|P1 || P1 L2
grids|Square stack and landscape stack|S2 || L2
hero|Full page with portrait and stack|B || P1 L2
hero|Full page with squares|B || S2 S1
story|Landscape, portrait and stack|L1 || P1 L2
grids|Portrait grid|P2 P2 || -
story|Stack, portrait and stack|L2 || P1 L2
grids|Square stack and squares|S2 || S2 S1
grids|Landscape stack and triple|L2 || L3
story|Twin portraits, portrait and stack|P1 P1 || P1 L2
grids|Landscape and square grid|L1 || S2 S2
hero|Full page with square grid|B || S2 S2
hero|Full page with landscape stacks|B || L2 L2
grids|Portrait grid and portrait|P2 P2 || P1
story|Triple and twin portraits|L3 || P1 P1
story|Portrait and stack, stack|P1 L2 || L2
grids|Landscape triples|L3 || L3
story|Portrait and stack, twice|P1 L2 || P1 L2
grids|Square grid and stack|S2 S2 || S2
grids|Twin portraits and portrait grid|P1 P1 || P2 P2
grids|Stack and square grid|L2 || S2 S2
hero|Full page with square gallery|B || S2 S2 S1
hero|Full page with stacks|B || L2 L3
grids|Twin portraits and square grid|P1 P1 || S2 S2
story|Squares and triple|S2 S1 || L3
story|Stack and portrait, portrait and stack|L2 P1 || P1 L2
grids|Portrait grid and stack|P2 P2 || L2
grids|Triple and portrait grid|L3 || P2 P2
grids|Square grid and squares|S2 S2 || S2 S1
story|Portrait and stack, portrait grid|P1 L2 || P2 P2
grids|Triple and square grid|L3 || S2 S2
story|Twin portraits and stacks|P1 P1 || L2 L3
hero|Full page with portrait gallery|B || P2 P2 P2
hero|Full page with triples|B || L3 L3
grids|Squares and portrait grid|S2 S1 || P2 P2
story|Stack and portrait, square grid|L2 P1 || S2 S2
grids|Twin portraits and square columns|P1 P1 || S3 S2
grids|Stack and square columns|L2 || S3 S2
grids|Portrait grids|P2 P2 || P2 P2
grids|Square grids|S2 S2 || S2 S2
story|Triple and stacks|L3 || L2 L3
story|Portrait and stack, stacks|P1 L2 || L2 L3
grids|Square grid and portrait grid|S2 S2 || P2 P2
grids|Square columns and triple|S3 S2 || L3
hero|Full page with square columns|B || S2 S3 S2
hero|Full page with mixed columns|B || P2 L3 P2
story|Stacks and portrait stack|L3 L2 || P1 L2
grids|Square columns and stack|S3 S3 || S2
grids|Triples and stack|L3 L3 || L2
grids|Nine-square grid|S3 S3 S3 || -
grids|Triple and square columns|L3 || S3 S3
grids|Portrait grid and square columns|P2 P2 || S3 S2
grids|Square grid and stacks|S2 S2 || L2 L3
grids|Triples and triple|L3 L3 || L3
story|Portrait and stack, square columns|P1 L2 || S3 S3
hero|Full page with portrait grid|B || P2 P2 P2 P2
hero|Full page with square columns|B || S3 S2 S3
story|Squares and square grid|S2 S2 S1 || S2 S2
story|Portrait grid and squares|P2 P2 P1 || S2 S2
grids|Square columns, twice|S3 S2 || S3 S2
grids|Stacks, twice|L3 L2 || L3 L2
grids|Portrait grid and square columns|P2 P2 || S3 S3
grids|Square grid and triples|S2 S2 || L3 L3
story|Stacks and square columns|L2 L3 || S3 S2
hero|Full page with nine squares|B || S3 S3 S3
hero|Full page with triples and squares|B || L3 S3 L3
story|Portraits and squares, square grid|P2 P2 S2 || S2 S2
story|Triple and squares, square columns|L3 S2 || S3 S2
grids|Squares and square columns|S2 S2 S1 || S3 S2
grids|Landscape column|L3 || -
story|Square stack and square|S2 || S1
story|Portrait stack and portrait|P2 || P1
grids|Column of four landscapes|L4 || -
grids|Square stacks|S2 || S2
story|Portrait stack and landscape stack|P2 || L2
story|Triple and portrait|L3 || P1
story|Portrait and square grid|P1 || S2 S2
grids|Square columns|S3 S2 || -
story|Stack and square column|L2 || S3
story|Portrait stack and triple|P2 || L3
grids|Six squares|S3 S3 || -
grids|Triple and square column|L3 || S3
story|Portrait stack and square grid|P2 || S2 S2
story|Square columns and portrait|S3 S3 || P1
grids|Portrait grid and triple|P2 P2 || L3
story|Triples and portrait|L3 L3 || P1
grids|Landscape columns of four|L4 || L4
story|Triple and squares, square stack|L3 S3 || S2
grids|Triple and squares, triple|L3 S3 || L3
grids|Portrait gallery and triple|P2 P2 P2 || L3
grids|Square columns and square grid|S3 S2 || S2 S2
grids|Landscape column and square columns|L4 || S3 S3
grids|Portrait gallery and square grid|P2 P2 P2 || S2 S2
story|Landscape column and triples|L4 || L3 L3
hero|Full page with square stack|B || S2
hero|Full page with portrait stack|B || P2
hero|Full page with landscape column|B || L3
hero|Full page with column of four|B || L4
hero|Full page with portrait grid of four|B || P2 P2
hero|Full page with square columns of five|B || S3 S2
hero|Full page with six squares|B || S3 S3
"#;

const ALBUM: &str = r#"
essentials|Landscape across the spread|~L1
hero|Full spread|~B
hero|Full page|B || -
essentials|Single landscape|L1 || -
essentials|Single portrait|P1 || -
essentials|Single square|S1 || -
minimal|Small landscape across|~L1@0.7
essentials|Square across the fold|~S1
minimal|Portrait on the fold|~P1@0.85
hero|Framed spread|~F
essentials|Landscape pair|L1 || L1
essentials|Portrait pair|P1 || P1
essentials|Square pair|S1 || S1
story|Portrait and landscape|P1 || L1
story|Portrait and landscape across|~P1 L1
grids|Squares meeting at the fold|~S1 S1
grids|Landscapes across|~L1 L1
grids|Portraits across|~P1 P1
hero|Full page with landscape|B || L1@0.75
hero|Full page with portrait|B || P1@0.75
hero|Hero centre, portraits|~P1 L1 P1
story|Stack and wide hero|~L2 L1
grids|Three portraits|~P1 P1 P1
grids|Three squares|~S1 S1 S1
story|Portrait and stack|P1 || L2
story|Landscape and twin portraits|L1 || P1 P1
hero|Full page with stack|B || L2
hero|Full page with twin portraits|B || P1 P1
story|Portrait and stack across|~P1 L2
story|Squares and landscape|~S2 L1
grids|Landscape grid|~L2 L2
grids|Square grid|~S2 S2
grids|Four portraits|~P1 P1 P1 P1
hero|Landscapes around squares|~L1 S2 L1
story|Stack, landscape and portrait|~L2 L1 P1
grids|Landscape stacks|L2 || L2
grids|Portrait pairs|P1 P1 || P1 P1
hero|Full page with portrait and stack|B || P1 L2
hero|Full page with squares|B || S2 S1
story|Square, landscape and squares|~S1 L1 S2
grids|Portrait grid across|~P2 P2
hero|Hero centre, portrait stacks|~P2 L1 P2
hero|Hero centre, landscape stacks|~L2 L1 L2
grids|Squares around a square|~S2 S1 S2
story|Portraits around a landscape|~P1 P1 L1 P1 P1
hero|Hero centre, square stacks|~S2 L1 S2
grids|Triple and stack|~L3 L2
story|Stack, portrait and stack|L2 || P1 L2
hero|Full page with square grid|B || S2 S2
hero|Full page with landscape stacks|B || L2 L2
story|Twin portraits, portrait and stack|P1 P1 || P1 L2
hero|Landscapes framed by portrait stacks|~P2 L2 P2
grids|Landscape grid of six|~L2 L2 L2
grids|Square grid of six|~S2 S2 S2
grids|Two triples|~L3 L3
grids|Portrait grid of six|~P2 P2 P2
story|Squares between stacks|~L2 S2 L2
grids|Landscape triples|L3 || L3
hero|Full page with square gallery|B || S2 S2 S1
hero|Full page with stacks|B || L2 L3
story|Square stack, landscape and squares|~S2 L1 S3
story|Portrait and stack, twice|P1 L2 || P1 L2
hero|Hero with portrait stacks|~P2 L1 P2 P2
grids|Square columns|~S2 S3 S2
hero|Hero between triples|~L3 L1 L3
grids|Stacks around a triple|~L2 L3 L2
hero|Hero with square stacks|~S2 L1 S2 S2
hero|Full page with portrait gallery|B || P2 P2 P2
hero|Full page with triples|B || L3 L3
grids|Triple and portrait grid|L3 || P2 P2
grids|Square grid and squares|S2 S2 || S2 S1
story|Stacks around a landscape|~L2 L1 L2 L2
story|Portraits around a portrait|~P2 P1 P2 P2
story|Landscape stacks between portraits|~P2 L2 L2 P2
grids|Square grid of eight|~S2 S2 S2 S2
grids|Portrait grid of eight|~P2 P2 P2 P2
grids|Stacks between triples|~L3 L2 L3
grids|Square columns of eight|~S3 S2 S3
grids|Squares between triples|~L3 S2 L3
hero|Full page with square columns|B || S2 S3 S2
hero|Full page with mixed columns|B || P2 L3 P2
grids|Portrait grids|P2 P2 || P2 P2
grids|Landscape grid of eight|~L2 L2 L2 L2
story|Hero with square columns|~S2 L1 S3 S2
grids|Nine squares|~S3 S3 S3
grids|Nine landscapes|~L3 L3 L3
hero|Hero centre, stacks and portraits|~P2 L2 L1 L2 P2
hero|Hero centre, four square stacks|~S2 S2 L1 S2 S2
story|Square columns of nine|~S3 S2 S2 S2
grids|Squares between triples of nine|~L3 S3 L3
hero|Full page with portrait grid|B || P2 P2 P2 P2
hero|Full page with nine-square columns|B || S3 S2 S3
grids|Triple and square columns|L3 || S3 S3
grids|Portrait grid and square columns|P2 P2 || S3 S2
hero|Hero centre, portrait grids|~P2 P2 L1 P2 P2
grids|Square columns of ten|~S3 S2 S2 S3
grids|Landscape columns of ten|~L3 L2 L2 L3
hero|Wide hero with portrait stacks|~P2 P2 L2 P2 P2
grids|Ten squares|~S2 S2 S2 S2 S2
grids|Triples between stacks|~L2 L3 L3 L2
hero|Hero with square columns|~S3 L1 S3 S3
hero|Full page with nine squares|B || S3 S3 S3
hero|Full page with triples and squares|B || L3 S3 L3
grids|Square columns, twice|S3 S2 || S3 S2
story|Portraits and squares|~P2 S3 S3 P2
grids|Squares and columns|~S2 S3 S3 S2
grids|Landscape stack|L2 || -
story|Portrait stack|P2 || -
grids|Landscape column|L3 || -
story|Square stack and square|S2 || S1
story|Portrait stack and portrait|P2 || P1
grids|Square stacks|S2 || S2
story|Portrait stack and landscape stack|P2 || L2
grids|Square grid on a page|S2 S2 || -
story|Portrait and square grid|P1 || S2 S2
story|Stack and square column|L2 || S3
story|Portrait stack and triple|P2 || L3
grids|Square columns on a page|S3 S2 || -
grids|Triple and squares, triple|L3 S3 || L3
grids|Square columns and square grid|S3 S2 || S2 S2
hero|Full page with square stack|B || S2
hero|Full page with landscape column|B || L3
hero|Full page with portrait grid of four|B || P2 P2
"#;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Col {
    aspect: f64,
    letter: char,
    count: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Block {
    Empty,
    Bleed,
    Framed,
    Band { cols: Vec<Col>, scale: f64 },
}

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Pages(Block, Block),
    Spread(Block),
}

#[derive(Debug, Clone)]
struct Design {
    group: String,
    name: String,
    shape: Shape,
}

/// A frame in micrometres on the trim surface.
#[derive(Debug, Clone, Copy)]
struct Fr {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    letter: char,
    /// Full-bleed / framed photos accept any photo shape (they are cropped to fit).
    free: bool,
}

fn parse_block(s: &str) -> Option<Block> {
    let s = s.trim();
    match s {
        "-" => return Some(Block::Empty),
        "B" => return Some(Block::Bleed),
        "F" => return Some(Block::Framed),
        _ => {}
    }
    let (body, scale) = match s.split_once('@') {
        Some((b, sc)) => (b, sc.trim().parse::<f64>().ok()?),
        None => (s, 1.0),
    };
    let mut cols = Vec::new();
    for tok in body.split_whitespace() {
        let mut ch = tok.chars();
        let letter = ch.next()?;
        let count: usize = ch.as_str().parse().ok()?;
        let aspect = match letter {
            'L' => 1.5,
            'P' => 2.0 / 3.0,
            'S' => 1.0,
            _ => return None,
        };
        if count == 0 || count > 4 {
            return None;
        }
        cols.push(Col { aspect, letter, count });
    }
    if cols.is_empty() || !(0.3..=1.0).contains(&scale) {
        return None;
    }
    Some(Block::Band { cols, scale })
}

fn parse_catalogue(src: &str) -> Vec<Design> {
    let mut out = Vec::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let parts: Vec<&str> = line.splitn(3, '|').collect();
        if parts.len() != 3 {
            continue;
        }
        let layout = parts[2].trim();
        let shape = if let Some(rest) = layout.strip_prefix('~') {
            parse_block(rest).map(Shape::Spread)
        } else if let Some((l, r)) = layout.split_once("||") {
            match (parse_block(l), parse_block(r)) {
                (Some(a), Some(b)) => Some(Shape::Pages(a, b)),
                _ => None,
            }
        } else {
            None
        };
        if let Some(shape) = shape {
            out.push(Design { group: parts[0].trim().to_string(), name: parts[1].trim().to_string(), shape });
        }
    }
    out
}

fn block_count(b: &Block) -> usize {
    match b {
        Block::Empty => 0,
        Block::Bleed | Block::Framed => 1,
        Block::Band { cols, .. } => cols.iter().map(|c| c.count).sum(),
    }
}

fn design_count(d: &Design) -> usize {
    match &d.shape {
        Shape::Pages(a, b) => block_count(a) + block_count(b),
        Shape::Spread(b) => block_count(b),
    }
}

fn is_hero(d: &Design) -> bool {
    match &d.shape {
        Shape::Pages(a, b) => *a == Block::Bleed || *b == Block::Bleed,
        Shape::Spread(b) => *b == Block::Bleed,
    }
}

fn letter_for(aspect: f64) -> char {
    if aspect > 1.1 {
        'L'
    } else if aspect < 0.9 {
        'P'
    } else {
        'S'
    }
}

/// Lay out a band of columns centred in `[x0, x1] × [top, bottom]`.
/// Returns None when the band would be too thin to look intentional (under 55 % of the space it may use).
fn solve_band(cols: &[Col], scale: f64, x0: f64, x1: f64, top: f64, bottom: f64, g: f64) -> Option<Vec<Fr>> {
    let avail_w = x1 - x0;
    let avail_h = (bottom - top) * scale;
    if avail_w <= 0.0 || avail_h <= 0.0 {
        return None;
    }
    let k = cols.len() as f64;
    // width(H) = sum_r·H − c + (k−1)·g, with r_i = a_i/n_i and c = Σ a_i (n_i−1) g / n_i
    let sum_r: f64 = cols.iter().map(|c| c.aspect / c.count as f64).sum();
    let c: f64 = cols.iter().map(|c| c.aspect * (c.count as f64 - 1.0) * g / c.count as f64).sum();
    let h_fit = (avail_w - (k - 1.0) * g + c) / sum_r;
    let hb = avail_h.min(h_fit);
    // A single column (one photo or one stack) may float in white space; a row of columns that ends up as a
    // thin strip looks accidental, so it needs most of the band height.
    let min_share = if cols.len() == 1 { 0.40 } else { 0.55 };
    if hb < min_share * avail_h {
        return None;
    }
    let total_w = sum_r * hb - c + (k - 1.0) * g;
    let mut x = x0 + (avail_w - total_w) / 2.0;
    let y0 = top + ((bottom - top) - hb) / 2.0;
    let mut out = Vec::new();
    for col in cols {
        let n = col.count as f64;
        let fh = (hb - (n - 1.0) * g) / n;
        if fh <= 0.0 {
            return None;
        }
        let fw = fh * col.aspect;
        for i in 0..col.count {
            out.push(Fr { x, y: y0 + i as f64 * (fh + g), w: fw, h: fh, letter: col.letter, free: false });
        }
        x += fw + g;
    }
    Some(out)
}

/// Frames for one block placed on a page (`right` selects the right page) or on the whole spread.
fn place(block: &Block, geom: &SpreadGeometry, kind: AlbumKind, p: &LayoutParams, page: Option<bool>) -> Option<Vec<Fr>> {
    let w = geom.trim_w as f64;
    let h = geom.trim_h as f64;
    let half = w / 2.0;
    let g = (p.gap_mm as f64 * 1000.0).max(0.0);
    let m = (p.margin_pct as f64 / 100.0 * h).max(0.0);
    let book_extra = if kind == AlbumKind::Photobook { (p.book_gutter_mm as f64 * 1000.0).max(0.0) } else { 0.0 };
    // Horizontal extent of the block's canvas, with outer / inner margins.
    let (px0, px1, inner_left, inner_right) = match page {
        Some(false) => (0.0, half, m, m + book_extra),
        Some(true) => (half, w, m + book_extra, m),
        None => (0.0, w, m, m),
    };
    match block {
        Block::Empty => Some(Vec::new()),
        Block::Bleed => {
            let a = (px1 - px0) / h;
            Some(vec![Fr { x: px0, y: 0.0, w: px1 - px0, h, letter: letter_for(a), free: true }])
        }
        Block::Framed => {
            let b = m * 0.5;
            let (l, r) = match page {
                Some(false) => (b, b + book_extra),
                Some(true) => (b + book_extra, b),
                None => (b, b),
            };
            let fw = (px1 - px0) - l - r;
            let fh = h - 2.0 * b;
            if fw <= 0.0 || fh <= 0.0 {
                return None;
            }
            Some(vec![Fr { x: px0 + l, y: b, w: fw, h: fh, letter: letter_for(fw / fh), free: true }])
        }
        Block::Band { cols, scale } => solve_band(cols, *scale, px0 + inner_left, px1 - inner_right, m, h - m, g),
    }
}

fn frames_for(d: &Design, geom: &SpreadGeometry, kind: AlbumKind, p: &LayoutParams) -> Option<Vec<Fr>> {
    match &d.shape {
        Shape::Pages(l, r) => {
            let mut a = place(l, geom, kind, p, Some(false))?;
            a.extend(place(r, geom, kind, p, Some(true))?);
            Some(a)
        }
        Shape::Spread(b) => {
            if kind == AlbumKind::Photobook {
                return None;
            }
            place(b, geom, kind, p, None)
        }
    }
}

fn to_template(id: String, d: &Design, frames: &[Fr], geom: &SpreadGeometry, kind: AlbumKind, mirrored: bool) -> Template {
    let tw = geom.trim_w as f64;
    let th = geom.trim_h as f64;
    let max_area = frames.iter().map(|f| f.w * f.h).fold(0.0, f64::max);
    let hero_idx = frames.iter().position(|f| (f.w * f.h - max_area).abs() < 1.0).unwrap_or(0);
    let fold = tw / 2.0;
    let crosses = frames.iter().any(|f| f.x < fold - 1.0 && f.x + f.w > fold + 1.0);
    let mut orientation = String::new();
    let tf: Vec<TemplateFrame> = frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            orientation.push(f.letter);
            let a = (f.w / f.h) as f32;
            let (lo, hi) = if f.free { (0.3, 4.0) } else { aspect_range(a) };
            let role = if i == hero_idx && frames.len() > 1 {
                "hero"
            } else if f.w * f.h < max_area * 0.3 {
                "detail"
            } else if frames.len() == 1 {
                "hero"
            } else {
                "support"
            };
            TemplateFrame {
                id: format!("f{}", i + 1),
                x: (f.x / tw).clamp(0.0, 1.0) as f32,
                y: (f.y / th).clamp(0.0, 1.0) as f32,
                w: (f.w / tw).clamp(0.0, 1.0) as f32,
                h: (f.h / th).clamp(0.0, 1.0) as f32,
                region: Region::Surface,
                role: role.into(),
                min_aspect: (lo * 100.0).round() / 100.0,
                max_aspect: (hi * 100.0).round() / 100.0,
                allow_rotate: false,
                z: 0,
            }
        })
        .collect();
    let mut tags = vec![kind.tag().to_string(), d.group.clone()];
    if is_hero(d) {
        tags.push("full-bleed".into());
    }
    if mirrored {
        tags.push("mirrored".into());
    }
    Template {
        id,
        name: d.name.clone(),
        family: d.group.clone(),
        kind: TemplateKind::Spread,
        photo_count: frames.len() as u32,
        orientation,
        tags,
        style: "any".into(),
        gutter_policy: if crosses { GutterPolicy::FullBleed } else { GutterPolicy::Safe },
        version: 1,
        custom: false,
        frames: tf,
        texts: Vec::new(),
    }
}

fn mirror(frames: &[Fr], w: f64) -> Vec<Fr> {
    let mut m: Vec<Fr> = frames.iter().map(|f| Fr { x: w - f.x - f.w, ..*f }).collect();
    m.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    m
}

fn same_layout(a: &[Fr], b: &[Fr]) -> bool {
    let key = |v: &[Fr]| {
        let mut k: Vec<(i64, i64, i64, i64)> = v.iter().map(|f| (f.x.round() as i64, f.y.round() as i64, f.w.round() as i64, f.h.round() as i64)).collect();
        k.sort();
        k
    };
    a.len() == b.len() && key(a).iter().zip(key(b).iter()).all(|(p, q)| (p.0 - q.0).abs() <= 2 && (p.1 - q.1).abs() <= 2 && (p.2 - q.2).abs() <= 2 && (p.3 - q.3).abs() <= 2)
}

/// Every designed template of one album kind for this spread size, plus mirrored versions of asymmetric designs.
/// Ids: `oto-{album|book}-{count:02}-{k:02}`, mirrored `…-m`.
pub fn designed_templates(geom: &SpreadGeometry, kind: AlbumKind, p: &LayoutParams) -> Vec<Template> {
    let (src, prefix) = match kind {
        AlbumKind::Album => (ALBUM, "album"),
        AlbumKind::Photobook => (PHOTOBOOK, "book"),
    };
    let mut next = [1u32; 32];
    let mut out = Vec::new();
    for d in parse_catalogue(src) {
        let Some(frames) = frames_for(&d, geom, kind, p) else { continue };
        let n = frames.len();
        if n == 0 || n >= next.len() || n != design_count(&d) {
            continue;
        }
        let k = next[n];
        next[n] += 1;
        let id = format!("oto-{prefix}-{n:02}-{k:02}");
        out.push(to_template(id.clone(), &d, &frames, geom, kind, false));
        let mf = mirror(&frames, geom.trim_w as f64);
        if !same_layout(&frames, &mf) {
            out.push(to_template(format!("{id}-m"), &d, &mf, geom, kind, true));
        }
    }
    out
}

/// Both album kinds.
pub fn all_designed_templates(geom: &SpreadGeometry, p: &LayoutParams) -> Vec<Template> {
    let mut v = designed_templates(geom, AlbumKind::Album, p);
    v.extend(designed_templates(geom, AlbumKind::Photobook, p));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: f64) -> i64 {
        (v * 1000.0).round() as i64
    }
    fn geom(page_w: f64, page_h: f64) -> SpreadGeometry {
        SpreadGeometry { trim_w: mm(2.0 * page_w), trim_h: mm(page_h), page_w: mm(page_w), bleed: mm(3.0), safe: mm(5.0), trim_outer: mm(3.0), trim_inside: 0, gutter: mm(10.0) }
    }
    fn sizes() -> Vec<SpreadGeometry> {
        vec![geom(254.0, 254.0), geom(304.8, 304.8), geom(304.8, 203.2), geom(210.0, 297.0)]
    }

    #[test]
    fn catalogue_parses_completely() {
        for (src, n) in [(ALBUM, "album"), (PHOTOBOOK, "photobook")] {
            let lines = src.lines().filter(|l| !l.trim().is_empty()).count();
            assert_eq!(parse_catalogue(src).len(), lines, "{n}: a line failed to parse");
        }
    }

    #[test]
    fn ten_designs_per_count_on_square_albums() {
        for g in [geom(254.0, 254.0), geom(304.8, 304.8)] {
            for kind in AlbumKind::ALL {
                let t = designed_templates(&g, kind, &LayoutParams::default());
                for n in 1..=10u32 {
                    let base = t.iter().filter(|x| x.photo_count == n && !x.id.ends_with("-m")).count();
                    assert!(base >= 10, "{:?} {n} photos: only {base} designs", kind);
                }
            }
        }
    }

    #[test]
    fn at_least_seven_designs_per_count_on_other_sizes() {
        for g in [geom(304.8, 203.2), geom(210.0, 297.0)] {
            for kind in AlbumKind::ALL {
                let t = designed_templates(&g, kind, &LayoutParams::default());
                for n in 1..=10u32 {
                    let base = t.iter().filter(|x| x.photo_count == n && !x.id.ends_with("-m")).count();
                    assert!(base >= 7, "{:?} {n} photos on {}x{}: only {base}", kind, g.page_w, g.trim_h);
                }
            }
        }
    }

    #[test]
    fn about_twenty_percent_full_bleed_heroes() {
        let g = geom(254.0, 254.0);
        for kind in AlbumKind::ALL {
            let t: Vec<_> = designed_templates(&g, kind, &LayoutParams::default()).into_iter().filter(|x| !x.id.ends_with("-m")).collect();
            let hero = t.iter().filter(|x| x.tags.iter().any(|g| g == "full-bleed")).count() as f64;
            let share = hero / t.len() as f64;
            assert!((0.15..=0.25).contains(&share), "{:?}: {:.0}% heroes", kind, share * 100.0);
        }
    }

    #[test]
    fn all_valid_and_ids_unique() {
        for g in sizes() {
            let t = all_designed_templates(&g, &LayoutParams::default());
            let mut ids = std::collections::HashSet::new();
            for x in &t {
                assert!(ids.insert(x.id.clone()), "duplicate {}", x.id);
                let e = x.validate();
                assert!(e.is_empty(), "{}: {:?}", x.id, e);
            }
        }
    }

    #[test]
    fn photobook_never_crosses_the_fold_and_keeps_gutter() {
        for g in sizes() {
            let p = LayoutParams::default();
            for t in designed_templates(&g, AlbumKind::Photobook, &p) {
                for f in &t.frames {
                    let (x0, x1) = (f.x as f64 * g.trim_w as f64, (f.x + f.w) as f64 * g.trim_w as f64);
                    let fold = g.trim_w as f64 / 2.0;
                    assert!(!(x0 < fold - 1.0 && x1 > fold + 1.0), "{} crosses the fold", t.id);
                    let full_bleed = f.min_aspect <= 0.31;
                    if !full_bleed {
                        let dist = if x1 <= fold + 1.0 { fold - x1 } else { x0 - fold };
                        assert!(dist >= p.book_gutter_mm as f64 * 1000.0 - 5.0, "{} too close to the fold", t.id);
                    }
                }
            }
        }
    }

    #[test]
    fn band_frames_keep_photo_shapes_and_one_gap() {
        let g = geom(254.0, 254.0);
        let p = LayoutParams::default();
        for t in all_designed_templates(&g, &p) {
            let rects: Vec<(f64, f64, f64, f64)> = t
                .frames
                .iter()
                .filter(|f| f.min_aspect > 0.31)
                .map(|f| (f.x as f64 * g.trim_w as f64, f.y as f64 * g.trim_h as f64, f.w as f64 * g.trim_w as f64, f.h as f64 * g.trim_h as f64))
                .collect();
            for r in &rects {
                let a = r.2 / r.3;
                let ok = [1.5, 2.0 / 3.0, 1.0].iter().any(|t| (a / t - 1.0).abs() < 0.005);
                assert!(ok, "{}: aspect {a:.3}", t.id);
            }
            // vertical neighbours in the same column are exactly one gap apart
            for a in &rects {
                for b in &rects {
                    if (a.0 - b.0).abs() < 2.0 && b.1 > a.1 {
                        let d = b.1 - (a.1 + a.3);
                        if d < 10_000.0 {
                            assert!((d - 3000.0).abs() < 100.0, "{}: vertical gap {d}", t.id);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn gap_setting_changes_layout() {
        let g = geom(254.0, 254.0);
        let a = designed_templates(&g, AlbumKind::Album, &LayoutParams::default());
        let b = designed_templates(&g, AlbumKind::Album, &LayoutParams { gap_mm: 8.0, ..LayoutParams::default() });
        assert_ne!(a.iter().find(|t| t.photo_count == 4).map(|t| t.frames.clone()), b.iter().find(|t| t.photo_count == 4).map(|t| t.frames.clone()));
    }
}
