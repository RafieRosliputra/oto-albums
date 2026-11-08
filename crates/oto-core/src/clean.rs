//! Original "clean" band-layout generator (see `docs/TEMPLATE-STYLE.md`).
//!
//! Every frame keeps an exact 3:2, 2:3 or 1:1 aspect measured on the real spread; one
//! uniform gap separates every pair of neighbours; the band is centred so top == bottom
//! margin. Nothing here is copied from another product.

use crate::builtin;
use crate::spec::SpreadGeometry;
use crate::template::{aspect_range, GutterPolicy, Region, Template, TemplateFrame, TemplateKind};

/// Exact photo aspect a clean frame keeps (width / height).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Aspect {
    /// 3:2
    Landscape,
    /// 2:3
    Portrait,
    /// 1:1
    Square,
}

impl Aspect {
    fn ratio(self) -> f64 {
        match self {
            Aspect::Landscape => 3.0 / 2.0,
            Aspect::Portrait => 2.0 / 3.0,
            Aspect::Square => 1.0,
        }
    }

    fn letter(self) -> char {
        match self {
            Aspect::Landscape => 'L',
            Aspect::Portrait => 'P',
            Aspect::Square => 'S',
        }
    }
}

/// One column of 1..=3 same-aspect frames stacked vertically.
#[derive(Debug, Clone)]
struct Column {
    aspect: Aspect,
    count: usize,
}

impl Column {
    fn new(aspect: Aspect, count: usize) -> Self {
        Self { aspect, count }
    }
}

/// Whether a layout lives on one page or spans the whole spread.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Page,
    Spread,
}

/// A curated layout: an English name plus the column sequence (left to right).
struct Layout {
    kind: Kind,
    name: &'static str,
    columns: Vec<Column>,
}

/// Which horizontal fit a variant uses.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Side margin == top margin target (shrinks only to fit, centred otherwise).
    Center,
    /// Side margin == top margin exactly (uniform shrink).
    Matted,
    /// Outer edges reach the trim edge (uniform grow, stacks only).
    Bleed,
}

/// A solved frame in micrometres relative to the trim origin.
struct BandFrame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    aspect: Aspect,
}

/// A fully solved band.
struct SolvedBand {
    frames: Vec<BandFrame>,
    policy: GutterPolicy,
    hero: usize,
}

fn col(aspect: Aspect, count: usize) -> Column {
    Column::new(aspect, count)
}

fn page(name: &'static str, columns: Vec<Column>) -> Layout {
    Layout { kind: Kind::Page, name, columns }
}

fn spread(name: &'static str, columns: Vec<Column>) -> Layout {
    Layout { kind: Kind::Spread, name, columns }
}

/// The curated clean catalog. Counts are the number of photo frames per layout.
// A long commented list reads better as pushes than as one vec! literal.
#[allow(clippy::vec_init_then_push)]
fn catalog() -> Vec<Layout> {
    use Aspect::*;
    let c = col;
    let mut v = Vec::new();

    // 1 photo
    v.push(page("Single landscape", vec![c(Landscape, 1)]));
    v.push(page("Single portrait", vec![c(Portrait, 1)]));
    v.push(page("Single square", vec![c(Square, 1)]));
    v.push(spread("Panorama hero across the spread", vec![c(Landscape, 1)]));
    v.push(spread("Square hero across the spread", vec![c(Square, 1)]));

    // 2 photos
    v.push(page("Stacked landscapes", vec![c(Landscape, 2)]));
    v.push(page("Stacked portraits", vec![c(Portrait, 2)]));
    v.push(page("Stacked squares", vec![c(Square, 2)]));
    v.push(page("Twin squares", vec![c(Square, 1), c(Square, 1)]));
    v.push(page("Landscape beside square", vec![c(Landscape, 1), c(Square, 1)]));
    v.push(spread("Squares across the fold", vec![c(Square, 1), c(Square, 1)]));
    v.push(spread("Portraits across the fold", vec![c(Portrait, 1), c(Portrait, 1)]));
    v.push(page("Twin portraits", vec![c(Portrait, 1), c(Portrait, 1)]));

    // 3 photos
    v.push(page("Triple landscapes", vec![c(Landscape, 3)]));
    v.push(page("Triple portraits", vec![c(Portrait, 3)]));
    v.push(page("Triple squares", vec![c(Square, 3)]));
    v.push(page("Landscape stack beside square", vec![c(Landscape, 2), c(Square, 1)]));
    v.push(page("Square beside landscape stack", vec![c(Square, 1), c(Landscape, 2)]));
    v.push(page("Three squares in a row", vec![c(Square, 1), c(Square, 1), c(Square, 1)]));
    v.push(spread("Hero centre, portraits both sides", vec![c(Portrait, 1), c(Landscape, 1), c(Portrait, 1)]));
    v.push(page("Portrait beside landscape stack", vec![c(Portrait, 1), c(Landscape, 2)]));
    v.push(spread("Landscape stack beside wide hero", vec![c(Landscape, 2), c(Landscape, 1)]));

    // 4 photos
    v.push(page("Two landscape stacks", vec![c(Landscape, 2), c(Landscape, 2)]));
    v.push(page("Two portrait stacks", vec![c(Portrait, 2), c(Portrait, 2)]));
    v.push(page("Two square stacks", vec![c(Square, 2), c(Square, 2)]));
    v.push(page("Four squares", vec![c(Square, 1), c(Square, 1), c(Square, 1), c(Square, 1)]));
    v.push(page("Landscape stack beside two squares", vec![c(Landscape, 2), c(Square, 1), c(Square, 1)]));
    v.push(spread("Square stacks across the fold", vec![c(Square, 2), c(Square, 2)]));
    v.push(spread("Landscape stacks across the fold", vec![c(Landscape, 2), c(Landscape, 2)]));
    v.push(spread("Four squares across the fold", vec![c(Square, 1), c(Square, 1), c(Square, 1), c(Square, 1)]));

    // 5 photos
    v.push(page("Triple landscapes beside square stack", vec![c(Landscape, 3), c(Square, 2)]));
    v.push(page("Square stack beside triple landscapes", vec![c(Square, 2), c(Landscape, 3)]));
    v.push(page("Two square stacks beside square", vec![c(Square, 2), c(Square, 2), c(Square, 1)]));
    v.push(page("Landscape stack beside triple squares", vec![c(Landscape, 2), c(Square, 3)]));
    v.push(page("Portrait triple beside portrait pair", vec![c(Portrait, 3), c(Portrait, 2)]));
    v.push(spread("Hero centre, portrait stacks both sides", vec![c(Portrait, 2), c(Landscape, 1), c(Portrait, 2)]));
    v.push(spread("Hero centre, stacks both sides", vec![c(Square, 2), c(Landscape, 1), c(Square, 2)]));

    // 6 photos
    v.push(page("Three square stacks", vec![c(Square, 2), c(Square, 2), c(Square, 2)]));
    v.push(page("Two triple squares", vec![c(Square, 3), c(Square, 3)]));
    v.push(page("Two triple landscapes", vec![c(Landscape, 3), c(Landscape, 3)]));
    v.push(page("Triple squares beside triple landscapes", vec![c(Square, 3), c(Landscape, 3)]));
    v.push(page("Two portrait triples", vec![c(Portrait, 3), c(Portrait, 3)]));
    v.push(spread("Triple square stacks across the fold", vec![c(Square, 3), c(Square, 3)]));
    v.push(spread("Landscape stacks flanking squares", vec![c(Landscape, 2), c(Square, 1), c(Square, 1), c(Landscape, 2)]));

    // 7 photos
    v.push(page("Triple landscapes beside two square stacks", vec![c(Landscape, 3), c(Square, 2), c(Square, 2)]));
    v.push(page("Two square stacks beside triple landscapes", vec![c(Square, 2), c(Square, 2), c(Landscape, 3)]));
    v.push(page("Triple squares beside two landscape stacks", vec![c(Square, 3), c(Landscape, 2), c(Landscape, 2)]));
    v.push(page("Portrait triple beside two portrait pairs", vec![c(Portrait, 3), c(Portrait, 2), c(Portrait, 2)]));
    v.push(spread("Hero centre, triple squares both sides", vec![c(Square, 3), c(Landscape, 1), c(Square, 3)]));
    v.push(spread("Hero centre, triple portraits both sides", vec![c(Portrait, 3), c(Landscape, 1), c(Portrait, 3)]));

    // 8 photos
    v.push(page("Four square stacks", vec![c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2)]));
    v.push(page("Two triple squares beside square stack", vec![c(Square, 3), c(Square, 3), c(Square, 2)]));
    v.push(page("Square stack beside two triple squares", vec![c(Square, 2), c(Square, 3), c(Square, 3)]));
    v.push(page("Two portrait triples beside portrait pair", vec![c(Portrait, 3), c(Portrait, 3), c(Portrait, 2)]));
    v.push(spread("Four square stacks across the fold", vec![c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2)]));
    v.push(spread("Landscape and square stacks across the fold", vec![c(Landscape, 2), c(Square, 2), c(Square, 2), c(Landscape, 2)]));

    // 9 photos
    v.push(page("Three triple squares", vec![c(Square, 3), c(Square, 3), c(Square, 3)]));
    v.push(page("Triple landscapes beside two triple squares", vec![c(Landscape, 3), c(Square, 3), c(Square, 3)]));
    v.push(page("Four square stacks beside square", vec![c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 1)]));
    v.push(page("Three portrait triples", vec![c(Portrait, 3), c(Portrait, 3), c(Portrait, 3)]));
    v.push(page("Two portrait triples beside square triple", vec![c(Portrait, 3), c(Portrait, 3), c(Square, 3)]));
    v.push(spread("Hero centre, double square stacks both sides", vec![c(Square, 2), c(Square, 2), c(Landscape, 1), c(Square, 2), c(Square, 2)]));

    // 10 photos
    v.push(page("Five square stacks", vec![c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2)]));
    v.push(page("Two triple squares beside two square stacks", vec![c(Square, 3), c(Square, 3), c(Square, 2), c(Square, 2)]));
    v.push(spread("Triple and pair square stacks across the fold", vec![c(Square, 3), c(Square, 2), c(Square, 2), c(Square, 3)]));
    v.push(spread("Landscape triples and square pairs across the fold", vec![c(Landscape, 3), c(Square, 2), c(Square, 2), c(Landscape, 3)]));
    v.push(spread("Hero centre, pair stacks left, pair and triple right", vec![c(Square, 2), c(Square, 2), c(Landscape, 1), c(Square, 2), c(Square, 3)]));

    // 11 photos
    v.push(page("Three triple squares beside square stack", vec![c(Square, 3), c(Square, 3), c(Square, 3), c(Square, 2)]));
    v.push(page("Triple landscapes, triple squares, square stack", vec![c(Landscape, 3), c(Square, 3), c(Square, 3), c(Square, 2)]));
    v.push(spread("Hero centre, triple and pair stacks both sides", vec![c(Square, 3), c(Square, 2), c(Landscape, 1), c(Square, 2), c(Square, 3)]));
    v.push(spread("Hero centre, landscape triples and square pairs", vec![c(Landscape, 3), c(Square, 2), c(Landscape, 1), c(Square, 2), c(Landscape, 3)]));

    v.push(spread("Hero centre, pair and triple squares both sides", vec![c(Square, 2), c(Square, 3), c(Landscape, 1), c(Square, 3), c(Square, 2)]));
    v.push(spread("Hero centre, landscape pairs and triples", vec![c(Landscape, 2), c(Landscape, 3), c(Landscape, 1), c(Landscape, 3), c(Landscape, 2)]));
    v.push(spread("Landscape triples with a pair across the fold", vec![c(Landscape, 3), c(Landscape, 3), c(Landscape, 2), c(Landscape, 3)]));

    // 12 photos
    v.push(page("Four triple squares", vec![c(Square, 3), c(Square, 3), c(Square, 3), c(Square, 3)]));
    v.push(page("Six square stacks", vec![c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2), c(Square, 2)]));
    v.push(spread("Four triple squares across the fold", vec![c(Square, 3), c(Square, 3), c(Square, 3), c(Square, 3)]));
    v.push(spread("Four landscape triples across the fold", vec![c(Landscape, 3), c(Landscape, 3), c(Landscape, 3), c(Landscape, 3)]));

    v
}

/// Solve one column arrangement into a band that fits `canvas_w` (the spread width for a
/// spread, or one page width for a single-page layout).
///
/// Column widths come from `w_i = (H - (n_i - 1) g) / sum(1/aspect)`; when the natural
/// band does not fit the target width the whole band (frames *and* gap) is scaled
/// uniformly, which preserves every aspect and keeps the gap uniform. The band is always
/// centred horizontally so the fold stays on the band's centre line.
fn solve_band(geom: &SpreadGeometry, gap: f64, margin_ratio: f64, columns: &[Column], canvas_w: f64, mode: Mode) -> Option<SolvedBand> {
    let tw = geom.trim_w as f64;
    let th = geom.trim_h as f64;
    let fold_x = tw / 2.0;
    let cols = columns.len();

    let mv = (margin_ratio * th).round();
    let h0 = th - 2.0 * mv;

    let mut widths = Vec::with_capacity(cols);
    for c in columns {
        let n = c.count as f64;
        let w = (h0 - (n - 1.0) * gap) * c.aspect.ratio() / n;
        if w <= 0.0 {
            return None;
        }
        widths.push(w);
    }
    let sum_w: f64 = widths.iter().sum();
    let w_natural = sum_w + (cols as f64 - 1.0) * gap;

    let scale = match mode {
        Mode::Center => {
            let target = canvas_w - 2.0 * mv;
            if w_natural > target && target > 0.0 {
                target / w_natural
            } else {
                1.0
            }
        }
        Mode::Matted => {
            let denom = w_natural - h0;
            if denom <= 0.0 {
                return None;
            }
            let s = (canvas_w - th) / denom;
            if !(s > 0.0 && s <= 1.0) {
                return None;
            }
            s
        }
        Mode::Bleed => {
            if w_natural >= canvas_w {
                return None;
            }
            let s = canvas_w / w_natural;
            if h0 * s > th {
                return None;
            }
            s
        }
    };

    let h = h0 * scale;
    let g = gap * scale;
    let band_w = w_natural * scale;
    let x0 = (canvas_w - band_w) / 2.0;
    let y0 = (th - h) / 2.0;

    let mut frames = Vec::new();
    let mut x = x0;
    for (ci, c) in columns.iter().enumerate() {
        let w = widths[ci] * scale;
        let fh = w / c.aspect.ratio();
        let mut y = y0;
        for _ in 0..c.count {
            frames.push(BandFrame { x, y, w, h: fh, aspect: c.aspect });
            y += fh + g;
        }
        x += w + g;
    }

    let (policy, hero) = fold_policy(&frames, fold_x, g, canvas_w)?;
    Some(SolvedBand { frames, policy, hero })
}

/// Rule 6: only a hero >= 40% of the spread width may cross the fold; every other frame
/// stays at least gap/2 away from it.
fn fold_policy(frames: &[BandFrame], fold_x: f64, g: f64, canvas_w: f64) -> Option<(GutterPolicy, usize)> {
    let tol = 1.0; // 1 um float slack
    let crossing: Vec<usize> = frames.iter().enumerate().filter(|(_, f)| f.x < fold_x && f.x + f.w > fold_x).map(|(i, _)| i).collect();
    match crossing.as_slice() {
        [] => {
            let min_dist = g / 2.0 - tol;
            for f in frames {
                let d = if f.x + f.w <= fold_x { fold_x - (f.x + f.w) } else { f.x - fold_x };
                if d < min_dist {
                    return None;
                }
            }
            let hero = largest_index(frames);
            Some((GutterPolicy::Safe, hero))
        }
        [i] => {
            let w = frames[*i].w;
            if w >= 0.4 * canvas_w - tol {
                Some((GutterPolicy::NoFace, *i))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn largest_index(frames: &[BandFrame]) -> usize {
    let mut best = 0;
    let mut best_area = f64::NEG_INFINITY;
    for (i, f) in frames.iter().enumerate() {
        let a = f.w * f.h;
        if a > best_area {
            best_area = a;
            best = i;
        }
    }
    best
}

fn build_template(geom: &SpreadGeometry, id: String, name: &str, band: &SolvedBand, mode: Mode, kind: Kind) -> Template {
    let tw = geom.trim_w as f64;
    let th = geom.trim_h as f64;
    let mut frames = Vec::with_capacity(band.frames.len());
    let mut orientation = String::with_capacity(band.frames.len());
    for (i, f) in band.frames.iter().enumerate() {
        let a = f.aspect.ratio() as f32;
        let (lo, hi) = aspect_range(a);
        frames.push(TemplateFrame {
            id: format!("f{}", i + 1),
            x: (f.x / tw) as f32,
            y: (f.y / th) as f32,
            w: (f.w / tw) as f32,
            h: (f.h / th) as f32,
            region: Region::Surface,
            role: String::new(),
            min_aspect: (lo * 100.0).round() / 100.0,
            max_aspect: (hi * 100.0).round() / 100.0,
            allow_rotate: false,
            z: 0,
        });
        orientation.push(f.aspect.letter());
    }
    let max_area = frames.iter().map(|f| f.w * f.h).fold(0.0f32, f32::max);
    for (i, f) in frames.iter_mut().enumerate() {
        f.role = if i == band.hero {
            "hero".into()
        } else if f.w * f.h < max_area * 0.3 {
            "detail".into()
        } else {
            "support".into()
        };
    }
    let mut tags = vec!["clean".to_string()];
    match mode {
        Mode::Matted => tags.push("matted".into()),
        Mode::Bleed => tags.push("bleed".into()),
        Mode::Center => {}
    }
    if kind == Kind::Page {
        tags.push("single-page".into());
    }
    Template {
        id,
        name: name.to_string(),
        family: "clean".into(),
        kind: TemplateKind::Spread,
        photo_count: frames.len() as u32,
        orientation,
        tags,
        style: "any".into(),
        gutter_policy: band.policy,
        version: 1,
        custom: false,
        frames,
        texts: Vec::new(),
    }
}

/// Generate every clean template for a real spread geometry.
///
/// Layouts are computed in micrometres on the spread and normalised to `Region::Surface`
/// rects, so `Template::validate` passes on the reference 2:1 spread. `gap_um` is the
/// single gap (default 3000 = 3 mm) and `margin_ratio` is the top/bottom margin
/// (default 0.10 = 10 % of trim height).
pub fn clean_templates(geom: &SpreadGeometry, gap_um: i64, margin_ratio: f32) -> Vec<Template> {
    let gap = gap_um as f64;
    let mratio = margin_ratio as f64;
    let tw = geom.trim_w as f64;
    let page_w = tw / 2.0;

    let mut out = Vec::new();
    let mut next_k = [1u32; 13];
    for layout in catalog() {
        let canvas_w = match layout.kind {
            Kind::Spread => tw,
            Kind::Page => page_w,
        };
        let mut modes = vec![Mode::Center];
        if layout.kind == Kind::Spread {
            modes.push(Mode::Matted);
            let outer_stacks = layout.columns.first().is_some_and(|c| c.count >= 2) && layout.columns.last().is_some_and(|c| c.count >= 2);
            if outer_stacks {
                modes.push(Mode::Bleed);
            }
        }
        for mode in modes {
            if let Some(band) = solve_band(geom, gap, mratio, &layout.columns, canvas_w, mode) {
                if !fill_ok(&band, geom, mratio, layout.kind) {
                    continue;
                }
                let count = band.frames.len();
                let k = next_k[count];
                next_k[count] += 1;
                let id = format!("oto-clean-{count:02}-{k:02}");
                let base = build_template(geom, id.clone(), layout.name, &band, mode, layout.kind);
                out.push(base.clone());
                let mut mirror = mirror_template(&base);
                mirror.id = format!("{id}-m");
                mirror.tags.push("mirrored".into());
                out.push(mirror);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Rule 11: drop layouts that leave the page mostly empty on this album size.
/// Page layouts: >= 70 % of the band height and >= 35 % of the page area. Spread layouts: >= 30 % of the spread area.
fn fill_ok(band: &SolvedBand, geom: &SpreadGeometry, margin_ratio: f64, kind: Kind) -> bool {
    let (tw, th) = (geom.trim_w as f64, geom.trim_h as f64);
    if band.frames.is_empty() || tw <= 0.0 || th <= 0.0 {
        return false;
    }
    let band_h = th - 2.0 * (margin_ratio * th).round();
    let top = band.frames.iter().map(|f| f.y).fold(f64::MAX, f64::min);
    let bottom = band.frames.iter().map(|f| f.y + f.h).fold(f64::MIN, f64::max);
    let area: f64 = band.frames.iter().map(|f| f.w * f.h).sum();
    match kind {
        Kind::Page => (bottom - top) >= 0.70 * band_h && area / (tw / 2.0 * th) >= 0.35,
        Kind::Spread => area / (tw * th) >= 0.30,
    }
}

/// Flip a template horizontally: `x' = 1 - x - w` for every frame and text box.
pub fn mirror_template(t: &Template) -> Template {
    let mut m = t.clone();
    for f in &mut m.frames {
        f.x = 1.0 - f.x - f.w;
    }
    for tx in &mut m.texts {
        tx.x = 1.0 - tx.x - tx.w;
    }
    m
}

/// Built-in templates plus the clean family for a concrete geometry.
pub fn all_templates_for(geom: &SpreadGeometry) -> Vec<Template> {
    let mut v = builtin::all_templates();
    v.extend(clean_templates(geom, 3_000, 0.10));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::mm;

    fn specs() -> Vec<SpreadGeometry> {
        let square = SpreadGeometry { trim_w: mm(508.0), trim_h: mm(254.0), page_w: mm(254.0), bleed: mm(3.0), safe: mm(5.0), trim_outer: mm(3.0), trim_inside: 0, gutter: mm(10.0) };
        let landscape = SpreadGeometry { trim_w: mm(609.6), trim_h: mm(203.2), page_w: mm(304.8), bleed: mm(3.0), safe: mm(5.0), trim_outer: mm(3.0), trim_inside: 0, gutter: mm(10.0) };
        let a4 = SpreadGeometry { trim_w: mm(420.0), trim_h: mm(297.0), page_w: mm(210.0), bleed: mm(3.0), safe: mm(6.0), trim_outer: mm(2.0), trim_inside: 0, gutter: mm(10.0) };
        vec![square, landscape, a4]
    }

    fn frame_rects_um(t: &Template, g: &SpreadGeometry) -> Vec<(f64, f64, f64, f64)> {
        let tw = g.trim_w as f64;
        let th = g.trim_h as f64;
        t.frames.iter().map(|f| (f.x as f64 * tw, f.y as f64 * th, f.w as f64 * tw, f.h as f64 * th)).collect()
    }

    /// Every adjacent gap (horizontal between columns, vertical within columns) in um.
    fn adjacent_gaps_um(t: &Template, g: &SpreadGeometry) -> Vec<f64> {
        let rects = frame_rects_um(t, g);
        let mut gaps = Vec::new();
        let mut cols: std::collections::BTreeMap<i64, Vec<usize>> = std::collections::BTreeMap::new();
        for (i, r) in rects.iter().enumerate() {
            cols.entry(r.0.round() as i64).or_default().push(i);
        }
        let xs: Vec<i64> = cols.keys().copied().collect();
        for w in xs.windows(2) {
            let left = &cols[&w[0]];
            let right = &cols[&w[1]];
            let left_right = left.iter().map(|&i| rects[i].0 + rects[i].2).fold(0.0f64, f64::max);
            let right_left = right.iter().map(|&i| rects[i].0).fold(f64::INFINITY, f64::min);
            gaps.push(right_left - left_right);
        }
        for idxs in cols.values() {
            let mut ys: Vec<usize> = idxs.clone();
            ys.sort_by(|&a, &b| rects[a].1.partial_cmp(&rects[b].1).unwrap());
            for w in ys.windows(2) {
                let top = &rects[w[0]];
                let bottom = &rects[w[1]];
                gaps.push(bottom.1 - (top.1 + top.3));
            }
        }
        gaps
    }

    #[test]
    fn aspects_match_exact_ratios_in_mm() {
        for g in specs() {
            for t in clean_templates(&g, 3000, 0.10) {
                let tw = g.trim_w as f64;
                let th = g.trim_h as f64;
                for f in &t.frames {
                    let w_mm = f.w as f64 * tw / 1000.0;
                    let h_mm = f.h as f64 * th / 1000.0;
                    let a = w_mm / h_mm;
                    let ok = [1.5, 2.0 / 3.0, 1.0].iter().any(|&r| (a - r).abs() <= r * 0.005);
                    assert!(ok, "{} frame {} aspect {a:.5} ({}x{} mm)", t.id, f.id, w_mm, h_mm);
                }
            }
        }
    }

    #[test]
    fn single_uniform_gap() {
        for g in specs() {
            for t in clean_templates(&g, 3000, 0.10) {
                let gaps = adjacent_gaps_um(&t, &g);
                if gaps.is_empty() {
                    continue;
                }
                let mn = gaps.iter().copied().fold(f64::INFINITY, f64::min);
                let mx = gaps.iter().copied().fold(0.0f64, f64::max);
                assert!(mn > 0.0, "{} has a non-positive gap", t.id);
                assert!(mx - mn <= 100.0, "{} gap spread {}um ({}..{})", t.id, mx - mn, mn, mx);
            }
        }
    }

    #[test]
    fn top_equals_bottom_margin() {
        for g in specs() {
            let th = g.trim_h as f64;
            for t in clean_templates(&g, 3000, 0.10) {
                let rects = frame_rects_um(&t, &g);
                let top = rects.iter().map(|r| r.1).fold(f64::INFINITY, f64::min);
                let bottom = rects.iter().map(|r| r.1 + r.3).fold(0.0f64, f64::max);
                assert!((top - (th - bottom)).abs() <= 100.0, "{} top {:.2} bottom {:.2} um", t.id, top, th - bottom);
            }
        }
    }

    #[test]
    fn fold_rule_holds() {
        for g in specs() {
            for t in clean_templates(&g, 3000, 0.10) {
                let fold = g.page_w as f64;
                let tw = g.trim_w as f64;
                let mut crossing = 0;
                for f in &t.frames {
                    let x = f.x as f64 * tw;
                    let w = f.w as f64 * tw;
                    if x < fold && x + w > fold {
                        crossing += 1;
                        assert_eq!(f.role, "hero", "{} {} crosses the fold but is not the hero", t.id, f.id);
                        assert!(w >= 0.4 * tw - 0.5, "{} {} hero is only {:.2} mm wide", t.id, f.id, w / 1000.0);
                        assert_ne!(t.gutter_policy, GutterPolicy::Safe, "{} crosses but policy is safe", t.id);
                    }
                }
                match t.gutter_policy {
                    GutterPolicy::Safe => assert_eq!(crossing, 0, "{} is safe but a frame crosses", t.id),
                    GutterPolicy::NoFace => assert_eq!(crossing, 1, "{} has {crossing} frames crossing the fold", t.id),
                    GutterPolicy::FullBleed => panic!("{} clean layout should not use FullBleed", t.id),
                }
            }
        }
    }

    #[test]
    fn hero_crosses_fold_exists() {
        let g = &specs()[0];
        let all = clean_templates(g, 3000, 0.10);
        assert!(all.iter().any(|t| t.gutter_policy == GutterPolicy::NoFace && t.frames.iter().any(|f| f.role == "hero")));
    }

    #[test]
    fn coverage_counts() {
        for g in specs() {
            let all = clean_templates(&g, 3000, 0.10);
            let base: Vec<&Template> = all.iter().filter(|t| !t.id.ends_with("-m")).collect();
            let mirrored: Vec<&Template> = all.iter().filter(|t| t.id.ends_with("-m")).collect();
            for count in 1..=12u32 {
                let n = base.iter().filter(|t| t.photo_count == count).count();
                let want = if count <= 8 { 3 } else { 2 };
                assert!(n >= want, "count {count}: only {n} base layouts");
            }
            assert_eq!(mirrored.len(), base.len(), "mirrored versions must pair 1:1 with base layouts");
            for t in &base {
                assert!(all.iter().any(|m| m.id == format!("{}-m", t.id)), "missing mirror for {}", t.id);
            }
        }
    }

    #[test]
    fn fill_quality_rule_11() {
        for g in specs() {
            let (tw, th) = (g.trim_w as f64, g.trim_h as f64);
            for t in clean_templates(&g, 3000, 0.10) {
                let r = frame_rects_um(&t, &g);
                let area: f64 = r.iter().map(|f| f.2 * f.3).sum();
                let spread = t.frames.iter().any(|f| f.x < 0.5 && f.x + f.w > 0.5) || (t.frames.iter().any(|f| f.x < 0.5) && t.frames.iter().any(|f| f.x >= 0.5));
                let min = if spread { 0.30 * tw * th } else { 0.35 * tw / 2.0 * th };
                assert!(area >= min - 1.0, "{} covers too little ({:.0}%)", t.id, 100.0 * area / (tw * th));
            }
        }
    }

    #[test]
    fn mirror_is_involution() {
        let g = &specs()[0];
        for t in clean_templates(g, 3000, 0.10).into_iter().take(80) {
            let twice = mirror_template(&mirror_template(&t));
            assert_eq!(twice.id, t.id);
            assert_eq!(twice.frames.len(), t.frames.len());
            for (a, b) in twice.frames.iter().zip(t.frames.iter()) {
                assert!((a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4 && (a.w - b.w).abs() < 1e-4 && (a.h - b.h).abs() < 1e-4, "{}", t.id);
            }
        }
    }

    #[test]
    fn all_clean_validate() {
        for g in specs() {
            for t in clean_templates(&g, 3000, 0.10) {
                let errs = t.validate();
                assert!(errs.is_empty(), "{}: {errs:?}", t.id);
            }
        }
    }

    #[test]
    fn ids_unique_and_formatted() {
        let g = &specs()[0];
        let all = clean_templates(g, 3000, 0.10);
        let mut seen = std::collections::HashSet::new();
        for t in &all {
            assert!(seen.insert(t.id.clone()), "duplicate id {}", t.id);
            assert!(t.id.starts_with("oto-clean-"), "bad id {}", t.id);
        }
        let combined = all_templates_for(g);
        let mut seen2 = std::collections::HashSet::new();
        for t in &combined {
            assert!(seen2.insert(t.id.clone()), "duplicate id {}", t.id);
        }
    }

    #[test]
    fn mirror_moves_left_page_to_right() {
        let g = &specs()[0];
        let all = clean_templates(g, 3000, 0.10);
        let base = all.iter().find(|t| t.id == "oto-clean-01-01").expect("single landscape");
        let m = mirror_template(base);
        let fold = 0.5;
        let base_x = base.frames[0].x as f64;
        let mir_x = m.frames[0].x as f64;
        assert!(base_x < fold && mir_x > fold, "mirror should move the frame across the fold");
    }
}
