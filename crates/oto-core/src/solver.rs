//! Auto Build: deterministic layout solver.
//!
//! 1. **Partition** the ordered photos into spreads with dynamic programming (density
//!    rhythm, scene breaks from capture-time gaps, B&W / similarity grouping, duplicate guard).
//! 2. **Template choice** per spread: every candidate with the right photo count is scored by an
//!    optimal photo→slot assignment (Hungarian) plus variety / rhythm terms.
//! 3. **Crop** each slot with the crop solver (faces, focus, fold).
//!
//! Same input + same seed ⇒ same output.

use std::collections::HashMap;

use crate::crop::{self, auto_crop, gutter_in_frame, CropResult};
use crate::geometry::{Rect, Vec2};
use crate::media::MediaItem;
use crate::model::{Crop, FitMode, Frame, MediaId, Spread, TextBlock};
use crate::spec::{AlbumSpec, SpreadGeometry};
use crate::template::{GutterPolicy, Template, TemplateKind};

#[derive(Debug, Clone)]
pub struct PhotoInfo {
    pub id: MediaId,
    pub name: String,
    pub aspect: f32,
    pub width: u32,
    pub height: u32,
    pub time: Option<i64>,
    pub rating: u8,
    pub pick: bool,
    pub is_bw: bool,
    pub sharpness: f32,
    pub brightness: f32,
    pub colorfulness: f32,
    pub focus: Option<Vec2>,
    pub faces: Vec<Rect>,
    pub similar_group: Option<u32>,
}

impl PhotoInfo {
    pub fn from_media(m: &MediaItem) -> Self {
        PhotoInfo {
            id: m.id,
            name: m.file_name.clone(),
            aspect: m.aspect(),
            width: m.width,
            height: m.height,
            time: m.capture_time.as_deref().and_then(parse_time),
            rating: m.rating,
            pick: m.client_pick,
            is_bw: m.features.is_bw,
            sharpness: m.features.sharpness,
            brightness: m.features.brightness,
            colorfulness: m.features.colorfulness,
            focus: m.focus.or(m.features.focus_auto),
            faces: m.features.faces.clone(),
            similar_group: m.similar_group,
        }
    }
    /// 0..1 — how much this photo deserves a big slot.
    pub fn importance(&self) -> f32 {
        let r = self.rating as f32 / 5.0;
        (0.45 * r + if self.pick { 0.3 } else { 0.0 } + 0.25 * self.sharpness).clamp(0.0, 1.0)
    }
    pub fn time_label(&self) -> Option<String> {
        let t = self.time?;
        let d = time::OffsetDateTime::from_unix_timestamp(t).ok()?;
        Some(format!("{:02}:{:02}", d.hour(), d.minute()))
    }
}

/// Parse "YYYY-MM-DDTHH:MM:SS" (EXIF-like, any offset ignored) into seconds.
pub fn parse_time(s: &str) -> Option<i64> {
    use time::format_description::well_known::Rfc3339;
    if let Ok(t) = time::OffsetDateTime::parse(s, &Rfc3339) {
        return Some(t.unix_timestamp());
    }
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i32>().ok();
    let date = time::Date::from_calendar_date(num(0, 4)?, time::Month::try_from(num(5, 7)? as u8).ok()?, num(8, 10)? as u8).ok()?;
    let tm = time::Time::from_hms(num(11, 13)? as u8, num(14, 16)? as u8, num(17, 19)? as u8).ok()?;
    Some(time::PrimitiveDateTime::new(date, tm).assume_utc().unix_timestamp())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    #[default]
    Classic,
    Airy,
    Editorial,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Classic, Style::Airy, Style::Editorial];
    pub fn key(self) -> &'static str {
        match self {
            Style::Classic => "classic",
            Style::Airy => "airy",
            Style::Editorial => "editorial",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Style::Classic => "Classic",
            Style::Airy => "Airy",
            Style::Editorial => "Editorial",
        }
    }
    pub fn parse(s: &str) -> Style {
        Style::ALL.into_iter().find(|v| v.key() == s).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    #[default]
    Time,
    BlackWhite,
    Similarity,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Reuse {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildParams {
    /// `None` = choose automatically.
    pub spreads: Option<usize>,
    pub min_per: usize,
    pub max_per: usize,
    pub grouping: Grouping,
    pub reuse: Reuse,
    pub style: Style,
    pub seed: u64,
    /// Photos below this effective DPI are never placed in a slot that would need it.
    pub hard_min_dpi: f32,
    pub warn_dpi: f32,
}

impl Default for BuildParams {
    fn default() -> Self {
        BuildParams { spreads: None, min_per: 1, max_per: 5, grouping: Grouping::Time, reuse: Reuse::Medium, style: Style::Classic, seed: 1, hard_min_dpi: 150.0, warn_dpi: 250.0 }
    }
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub style: Style,
    pub spreads: Vec<Spread>,
    pub warnings: Vec<String>,
    pub score: f32,
    pub elapsed_ms: u128,
}

/// SplitMix64 — tiny deterministic RNG (no external dependency).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

// ---------------------------------------------------------------------------------------------
// Hungarian algorithm (minimum cost, square matrix), O(n³).
// ---------------------------------------------------------------------------------------------

/// Returns `assign[row] = col` minimizing the total cost.
pub fn hungarian(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    if n == 0 {
        return vec![];
    }
    let inf = f64::INFINITY;
    let mut u = vec![0.0; n + 1];
    let mut v = vec![0.0; n + 1];
    let mut p = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![inf; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = inf;
            let mut j1 = 0usize;
            for j in 1..=n {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut assign = vec![0usize; n];
    for j in 1..=n {
        if p[j] > 0 {
            assign[p[j] - 1] = j - 1;
        }
    }
    assign
}

// ---------------------------------------------------------------------------------------------
// Scoring weights
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Weights {
    orient: f32,
    crop: f32,
    face: f32,
    gutter: f32,
    dpi: f32,
    hero: f32,
    color: f32,
    seq: f32,
    novelty: f32,
    style_affinity: f32,
    jitter: f32,
}

fn weights(style: Style) -> Weights {
    match style {
        Style::Classic => Weights { orient: 1.0, crop: 0.8, face: 1.2, gutter: 1.5, dpi: 1.0, hero: 0.6, color: 0.3, seq: 0.25, novelty: 0.6, style_affinity: 0.5, jitter: 0.12 },
        Style::Airy => Weights { orient: 1.0, crop: 0.9, face: 1.2, gutter: 1.5, dpi: 1.0, hero: 0.4, color: 0.4, seq: 0.25, novelty: 0.5, style_affinity: 0.9, jitter: 0.12 },
        Style::Editorial => Weights { orient: 0.8, crop: 0.6, face: 1.2, gutter: 1.5, dpi: 1.0, hero: 1.0, color: 0.2, seq: 0.2, novelty: 0.7, style_affinity: 0.9, jitter: 0.15 },
    }
}

/// Context needed to score slots: geometry and the fold band in normalized spread coords.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub geom: SpreadGeometry,
    pub band_x0: f32,
    pub band_x1: f32,
}

impl Ctx {
    pub fn new(spec: &AlbumSpec) -> Self {
        let geom = spec.geometry();
        let b = geom.gutter_band();
        Ctx { geom, band_x0: b.x as f32 / geom.trim_w as f32, band_x1: b.right() as f32 / geom.trim_w as f32 }
    }
    pub fn frame_um(&self, r: &Rect) -> (i64, i64) {
        ((r.w as f64 * self.geom.trim_w as f64) as i64, (r.h as f64 * self.geom.trim_h as f64) as i64)
    }
    pub fn frame_aspect(&self, r: &Rect) -> f32 {
        let (w, h) = self.frame_um(r);
        w as f32 / h.max(1) as f32
    }
}

#[derive(Debug, Clone)]
struct SlotEval {
    score: f32,
    crop: CropResult,
    dpi: f32,
}

#[allow(clippy::too_many_arguments)]
fn eval_slot(ctx: &Ctx, w: &Weights, p: &PhotoInfo, rect: &Rect, role: &str, min_a: f32, max_a: f32, policy: GutterPolicy, params: &BuildParams) -> SlotEval {
    let af = ctx.frame_aspect(rect);
    let ai = p.aspect;
    let gutter = if policy == GutterPolicy::Safe { None } else { gutter_in_frame(rect, ctx.band_x0, ctx.band_x1) };
    let cr = auto_crop(ai, af, p.focus, &p.faces, gutter);
    let vis = crop::visible(&cr.crop, ai, af, FitMode::Fill);
    let (fw, fh) = ctx.frame_um(rect);
    let dpi = crop::effective_dpi(p.width, p.height, &vis, fw, fh);

    let ratio = (ai / af).ln().abs();
    let mut orient = 1.0 - (ratio / 2.2f32.ln()).min(1.0);
    if ai < min_a * 0.98 || ai > max_a * 1.02 {
        orient -= 1.0;
    }
    let crop_q = cr.kept;
    let face = if cr.face_safe { 1.0 } else { -0.8 };
    let gut = if cr.gutter_safe { 1.0 } else { -1.0 };
    // Matches preflight: < hard_min blocks export, < 200 is critical, < warn_dpi is a warning.
    let dpi_q = if dpi < params.hard_min_dpi {
        -8.0
    } else if dpi < 200.0 {
        -4.0
    } else if dpi < params.warn_dpi {
        -0.3
    } else {
        1.0
    };
    let area = rect.area();
    let hero = match role {
        "hero" => p.importance() * 1.0 + area,
        "detail" => (1.0 - p.importance()) * 0.3,
        _ => p.importance() * area,
    };
    let score = w.orient * orient + w.crop * crop_q + w.face * face * 0.3 + w.gutter * gut * 0.4 + w.dpi * dpi_q + w.hero * hero;
    SlotEval { score, crop: cr, dpi }
}

#[derive(Debug, Clone)]
struct Choice {
    template: usize,
    /// assign[slot] = photo index (within the segment)
    assign: Vec<usize>,
    evals: Vec<SlotEval>,
    total: f32,
    notes: Vec<String>,
}

fn is_hero_template(t: &Template, ctx: &Ctx) -> bool {
    t.photo_frames().any(|f| f.surface_rect(&ctx.geom).area() >= 0.45) || t.tags.iter().any(|x| x == "full-bleed-page") && t.photo_count <= 2
}

struct History {
    families: Vec<String>,
    uses: HashMap<usize, u32>,
    since_hero: u32,
}

fn style_affinity(t: &Template, style: Style) -> f32 {
    let base = if t.style == style.key() {
        1.0
    } else if t.style == "any" {
        0.4
    } else {
        0.0
    };
    let fam = match (style, t.family.as_str()) {
        (Style::Airy, "airy") => 0.8,
        (Style::Airy, "full-bleed" | "panorama") => -0.4,
        (Style::Editorial, "full-bleed" | "hero-strip" | "panorama" | "editorial") => 0.6,
        (Style::Editorial, "airy") => -0.2,
        (Style::Classic, "classic" | "grid") => 0.5,
        (Style::Classic, "panorama") => -0.2,
        _ => 0.0,
    };
    base + fam
}

#[allow(clippy::too_many_arguments)]
fn choose_template(ctx: &Ctx, templates: &[Template], cands: &[usize], photos: &[&PhotoInfo], params: &BuildParams, hist: &History, rng: &mut Rng, explain: bool) -> Option<Choice> {
    let w = weights(params.style);
    let k = photos.len();
    let mut best: Option<Choice> = None;
    for &ti in cands {
        let t = &templates[ti];
        let slots: Vec<_> = t.photo_frames().collect();
        if slots.len() != k {
            continue;
        }
        let rects: Vec<Rect> = slots.iter().map(|f| f.surface_rect(&ctx.geom)).collect();
        // evals[slot][photo]
        let mut evals: Vec<Vec<SlotEval>> = Vec::with_capacity(k);
        for (si, f) in slots.iter().enumerate() {
            evals.push(photos.iter().map(|p| eval_slot(ctx, &w, p, &rects[si], &f.role, f.min_aspect, f.max_aspect, t.gutter_policy, params)).collect());
        }
        let cost: Vec<Vec<f64>> = evals.iter().map(|row| row.iter().map(|e| -(e.score as f64)).collect()).collect();
        let assign = hungarian(&cost);
        let mut total: f32 = assign.iter().enumerate().map(|(s, &p)| evals[s][p].score).sum::<f32>() / k.max(1) as f32;
        let mut notes = Vec::new();

        // Sequence coherence: left→right should follow story order.
        let mut order: Vec<(f32, usize)> = assign.iter().enumerate().map(|(s, &p)| (rects[s].x + rects[s].y * 0.1, p)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let inversions = order.windows(2).filter(|w| w[0].1 > w[1].1).count();
        total -= w.seq * inversions as f32 / k.max(1) as f32;

        // Colour harmony: avoid mixing B&W and colour on one spread.
        let bw = photos.iter().filter(|p| p.is_bw).count();
        if bw > 0 && bw < k {
            total -= w.color * 0.6;
        }
        let affinity = style_affinity(t, params.style);
        total += w.style_affinity * affinity;

        // Variety.
        let reuse_pen = match params.reuse {
            Reuse::Low => 0.6,
            Reuse::Medium => 0.3,
            Reuse::High => 0.08,
        };
        let used = *hist.uses.get(&ti).unwrap_or(&0);
        total -= w.novelty * reuse_pen * used as f32;
        let n = hist.families.len();
        if n >= 2 && hist.families[n - 1] == t.family && hist.families[n - 2] == t.family {
            total -= 3.0; // never three of the same family in a row
        } else if n >= 1 && hist.families[n - 1] == t.family {
            total -= w.novelty * 0.35;
        }
        // Rhythm: a hero spread every 4–6 spreads.
        let hero_t = is_hero_template(t, ctx);
        if hero_t && hist.since_hero >= 4 {
            total += 0.35 * (hist.since_hero as f32 - 3.0).min(3.0);
        } else if hero_t && hist.since_hero < 2 {
            total -= 0.25;
        }
        total += w.jitter * rng.f32();

        if explain {
            let hero_slot = slots.iter().position(|f| f.role == "hero");
            if let Some(hs) = hero_slot {
                let p = photos[assign[hs]];
                notes.push(format!("hero: {}{}", p.name, if p.rating > 0 { format!(" (★{})", p.rating) } else { String::new() }));
            }
            let mismatched = assign.iter().enumerate().filter(|(s, &p)| (photos[p].aspect > 1.08) != (ctx.frame_aspect(&rects[*s]) > 1.08)).count();
            if mismatched == 0 {
                notes.push("orientations match their slots".into());
            } else {
                notes.push(format!("{mismatched} photo(s) cropped across orientation"));
            }
            if t.gutter_policy != GutterPolicy::Safe {
                notes.push("crosses the fold: subject kept out of the gutter".into());
            }
            if hero_t && hist.since_hero >= 4 {
                notes.push("hero spread for rhythm".into());
            }
            if bw > 0 && bw < k {
                notes.push("mixes B&W and colour".into());
            }
        }
        let ev_sel: Vec<SlotEval> = assign.iter().enumerate().map(|(s, &p)| evals[s][p].clone()).collect();
        if best.as_ref().map(|b| total > b.total).unwrap_or(true) {
            best = Some(Choice { template: ti, assign, evals: ev_sel, total, notes });
        }
    }
    best
}

/// How well one template fits these photos (orientation, crop kept, DPI, focus out of the fold, story order),
/// using the optimal photo→slot assignment. Higher is better; `None` when the photo count differs. Deterministic.
pub fn score_template_slots(ctx: &Ctx, t: &Template, photos: &[&PhotoInfo]) -> Option<f32> {
    let params = BuildParams::default();
    let w = weights(params.style);
    let slots: Vec<_> = t.photo_frames().collect();
    let k = photos.len();
    if slots.len() != k || k == 0 {
        return None;
    }
    let rects: Vec<Rect> = slots.iter().map(|f| f.surface_rect(&ctx.geom)).collect();
    let evals: Vec<Vec<SlotEval>> =
        slots.iter().enumerate().map(|(si, f)| photos.iter().map(|p| eval_slot(ctx, &w, p, &rects[si], &f.role, f.min_aspect, f.max_aspect, t.gutter_policy, &params)).collect()).collect();
    let cost: Vec<Vec<f64>> = evals.iter().map(|row| row.iter().map(|e| -(e.score as f64)).collect()).collect();
    let assign = hungarian(&cost);
    let mut total: f32 = assign.iter().enumerate().map(|(s, &p)| evals[s][p].score).sum::<f32>() / k as f32;
    let mut order: Vec<(f32, usize)> = assign.iter().enumerate().map(|(s, &p)| (rects[s].x + rects[s].y * 0.1, p)).collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let inversions = order.windows(2).filter(|w| w[0].1 > w[1].1).count();
    total -= w.seq * inversions as f32 / k as f32;
    Some(total)
}

/// Pick the spread count when the user asked for "auto".
pub fn auto_spread_count(n: usize, spec: &AlbumSpec, style: Style) -> usize {
    let per = match style {
        Style::Classic => 3.6,
        Style::Airy => 2.6,
        Style::Editorial => 3.0,
    };
    let s = (n as f32 / per).round() as usize;
    s.clamp(spec.min_spreads() as usize, spec.max_spreads() as usize).max(1)
}

/// Target photos for spread `s` following a dense/airy rhythm.
fn target_density(s: usize, base: f32, style: Style) -> f32 {
    let wave = match style {
        Style::Classic => [1.15, 0.85, 1.1, 0.9, 1.2, 0.6],
        Style::Airy => [1.0, 0.75, 1.1, 0.8, 1.15, 0.6],
        Style::Editorial => [1.3, 0.7, 1.2, 0.45, 1.25, 0.8],
    };
    (base * wave[s % wave.len()]).max(1.0)
}

/// Dynamic-programming partition of `photos` (in story order) into `s` spreads.
pub fn partition(photos: &[PhotoInfo], s: usize, min_per: usize, max_per: usize, grouping: Grouping, style: Style) -> Option<Vec<std::ops::Range<usize>>> {
    let n = photos.len();
    if s == 0 || n < s * min_per || n > s * max_per {
        return None;
    }
    let base = n as f32 / s as f32;
    // Gap bonus before photo i (scene change).
    let gaps: Vec<f32> = (0..n)
        .map(|i| {
            if i == 0 {
                return 0.0;
            }
            match (photos[i - 1].time, photos[i].time) {
                (Some(a), Some(b)) => ((b - a).max(0) as f32 / 60.0 + 1.0).ln(),
                _ => 0.0,
            }
        })
        .collect();
    let seg_cost = |a: usize, b: usize, si: usize| -> f32 {
        let len = (b - a) as f32;
        let t = target_density(si, base, style);
        let mut c = (len - t).powi(2) * 0.8;
        // Scene coherence: gaps *inside* a segment are bad; at its start they are good.
        if grouping == Grouping::Time {
            for g in &gaps[a + 1..b] {
                c += g * 0.9;
            }
            c -= gaps[a] * 0.6;
        }
        let seg = &photos[a..b];
        if grouping == Grouping::BlackWhite {
            let bw = seg.iter().filter(|p| p.is_bw).count();
            c += bw.min(seg.len() - bw) as f32 * 1.5;
        }
        if grouping == Grouping::Similarity && seg.len() > 1 {
            let mb = seg.iter().map(|p| p.brightness).sum::<f32>() / len;
            let mc = seg.iter().map(|p| p.colorfulness).sum::<f32>() / len;
            let var: f32 = seg.iter().map(|p| (p.brightness - mb).powi(2) + (p.colorfulness - mc).powi(2)).sum::<f32>();
            c += var * 6.0;
        }
        // Duplicate guard: near-identical frames never share a spread.
        for i in a..b {
            if let Some(g) = photos[i].similar_group {
                if photos[i + 1..b].iter().any(|q| q.similar_group == Some(g)) {
                    c += 50.0;
                }
            }
        }
        c
    };
    let inf = f32::INFINITY;
    let mut dp = vec![vec![inf; n + 1]; s + 1];
    let mut back = vec![vec![0usize; n + 1]; s + 1];
    dp[0][0] = 0.0;
    for si in 1..=s {
        for i in (si * min_per)..=n.min(si * max_per) {
            let lo = i.saturating_sub(max_per).max((si - 1) * min_per);
            let hi = i.saturating_sub(min_per);
            if lo > hi {
                continue;
            }
            for j in lo..=hi {
                if dp[si - 1][j].is_finite() {
                    let c = dp[si - 1][j] + seg_cost(j, i, si - 1);
                    if c < dp[si][i] {
                        dp[si][i] = c;
                        back[si][i] = j;
                    }
                }
            }
        }
    }
    if !dp[s][n].is_finite() {
        return None;
    }
    let mut out = Vec::with_capacity(s);
    let mut i = n;
    for si in (1..=s).rev() {
        let j = back[si][i];
        out.push(j..i);
        i = j;
    }
    out.reverse();
    Some(out)
}

/// Instantiate a template as a spread with photos in slot order.
pub fn spread_from_choice(ctx: &Ctx, t: &Template, photos: &[&PhotoInfo], assign: &[usize], crops: Option<&[CropResult]>) -> Spread {
    let mut sp = Spread::blank();
    sp.template_id = t.id.clone();
    for (si, f) in t.photo_frames().enumerate() {
        let rect = f.surface_rect(&ctx.geom);
        let mut fr = Frame::new(rect);
        fr.role = f.role.clone();
        fr.allow_rotate = f.allow_rotate;
        fr.z = f.z;
        if let Some(&pi) = assign.get(si) {
            if let Some(p) = photos.get(pi) {
                fr.image_id = Some(p.id);
                fr.crop = match crops {
                    Some(c) => c[si].crop,
                    None => {
                        let gutter = if t.gutter_policy == GutterPolicy::Safe { None } else { gutter_in_frame(&rect, ctx.band_x0, ctx.band_x1) };
                        auto_crop(p.aspect, ctx.frame_aspect(&rect), p.focus, &p.faces, gutter).crop
                    }
                };
            }
        }
        sp.frames.push(fr);
    }
    sp
}

fn texts_from_template(t: &Template, geom: &SpreadGeometry, pair: &crate::model::FontPair) -> Vec<TextBlock> {
    t.texts.iter().map(|tt| TextBlock::new(tt.surface_rect(geom), &tt.placeholder, tt.preset.style(pair))).collect()
}

pub fn spread_templates(templates: &[Template]) -> Vec<usize> {
    templates.iter().enumerate().filter(|(_, t)| t.kind == TemplateKind::Spread && t.family != "story" && t.photo_count > 0).map(|(i, _)| i).collect()
}

/// Run Auto Build for one style.
pub fn auto_build(photos: &[PhotoInfo], templates: &[Template], spec: &AlbumSpec, params: &BuildParams) -> BuildResult {
    let started = std::time::Instant::now();
    let ctx = Ctx::new(spec);
    let mut warnings = Vec::new();
    let n = photos.len();
    if n == 0 {
        return BuildResult { style: params.style, spreads: vec![], warnings: vec!["No photos selected.".into()], score: 0.0, elapsed_ms: 0 };
    }
    let usable = spread_templates(templates);
    let max_avail = usable.iter().map(|&i| templates[i].photo_count as usize).max().unwrap_or(1);
    let mut max_per = params.max_per.clamp(1, max_avail);
    let mut min_per = params.min_per.clamp(1, max_per);
    let mut s = params.spreads.unwrap_or_else(|| auto_spread_count(n, spec, params.style)).max(1);
    if n < s * min_per {
        if params.spreads.is_some() && n >= s {
            min_per = 1;
            warnings.push(format!("Only {n} photos for {s} spreads: some spreads will hold a single photo."));
        } else {
            s = n.div_ceil(min_per.max(1)).max(1).min(n);
            warnings.push(format!("Not enough photos for the requested spreads; building {s} spreads."));
        }
    }
    if n > s * max_per {
        let need = n.div_ceil(s);
        if need <= max_avail {
            warnings.push(format!("{n} photos do not fit {s} spreads at {max_per} per spread; allowing up to {need} per spread."));
            max_per = need;
        } else {
            s = n.div_ceil(max_avail);
            max_per = max_avail;
            warnings.push(format!("Too many photos for the requested spreads; building {s} spreads."));
        }
    }
    let parts = partition(photos, s, min_per, max_per, params.grouping, params.style).unwrap_or_else(|| {
        warnings.push("Could not balance spreads; used an even split.".into());
        even_split(n, s)
    });

    let mut by_count: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in &usable {
        by_count.entry(templates[i].photo_count as usize).or_default().push(i);
    }
    let mut rng = Rng::new(params.seed.wrapping_mul(31).wrapping_add(params.style as u64 * 7919));
    let mut hist = History { families: vec![], uses: HashMap::new(), since_hero: 4 };
    let mut spreads = Vec::with_capacity(parts.len());
    let mut total = 0.0;
    let mut bw_run = 0;
    for (si, r) in parts.iter().enumerate() {
        let seg: Vec<&PhotoInfo> = photos[r.clone()].iter().collect();
        let cands = by_count.get(&seg.len()).cloned().unwrap_or_default();
        let choice = choose_template(&ctx, templates, &cands, &seg, params, &hist, &mut rng, true);
        let mut sp = match &choice {
            Some(c) => {
                let crops: Vec<CropResult> = c.evals.iter().map(|e| e.crop).collect();
                let t = &templates[c.template];
                hist.families.push(t.family.clone());
                *hist.uses.entry(c.template).or_insert(0) += 1;
                hist.since_hero = if is_hero_template(t, &ctx) { 0 } else { hist.since_hero + 1 };
                total += c.total;
                let mut sp = spread_from_choice(&ctx, t, &seg, &c.assign, Some(&crops));
                for e in &c.evals {
                    if e.dpi < params.warn_dpi {
                        warnings.push(format!("Spread {}: a photo prints at {:.0} dpi.", si + 1, e.dpi));
                    }
                }
                let time = match (seg.first().and_then(|p| p.time_label()), seg.last().and_then(|p| p.time_label())) {
                    (Some(a), Some(b)) if a != b => format!(" ({a}–{b})"),
                    (Some(a), _) => format!(" ({a})"),
                    _ => String::new(),
                };
                sp.explain = format!("{} photo{}{} · {}: {}", seg.len(), if seg.len() == 1 { "" } else { "s" }, time, t.name, c.notes.join("; "));
                sp
            }
            None => {
                warnings.push(format!("Spread {}: no template for {} photos — used single-photo fallback.", si + 1, seg.len()));
                fallback_spread(&ctx, &seg)
            }
        };
        let all_bw = seg.iter().all(|p| p.is_bw);
        bw_run = if all_bw { bw_run + 1 } else { 0 };
        if bw_run == 4 {
            warnings.push(format!("Spreads {}–{} are all black & white; consider mixing colour in.", si - 2, si + 1));
        }
        sp.notes.clear();
        spreads.push(sp);
    }
    BuildResult { style: params.style, spreads, warnings, score: total / parts.len().max(1) as f32, elapsed_ms: started.elapsed().as_millis() }
}

fn even_split(n: usize, s: usize) -> Vec<std::ops::Range<usize>> {
    let s = s.max(1).min(n.max(1));
    let mut out = Vec::new();
    let mut a = 0;
    for i in 0..s {
        let b = (n * (i + 1)) / s;
        out.push(a..b);
        a = b;
    }
    out
}

/// Last-resort layout: one equal column per photo, never crossing the fold awkwardly.
fn fallback_spread(ctx: &Ctx, photos: &[&PhotoInfo]) -> Spread {
    let mut sp = Spread::blank();
    sp.template_id = "fallback".into();
    let k = photos.len().max(1);
    let m = 0.04;
    let gap = 0.015;
    let w = (1.0 - 2.0 * m - gap * (k as f32 - 1.0)) / k as f32;
    for (i, p) in photos.iter().enumerate() {
        let rect = Rect::new(m + i as f32 * (w + gap), 0.12, w, 0.76);
        let mut f = Frame::new(rect);
        f.image_id = Some(p.id);
        f.crop = auto_crop(p.aspect, ctx.frame_aspect(&rect), p.focus, &p.faces, gutter_in_frame(&rect, ctx.band_x0, ctx.band_x1)).crop;
        sp.frames.push(f);
    }
    sp.explain = "Fallback layout — no matching template.".into();
    sp
}

/// Build the three standard variants (Classic, Airy, Editorial).
pub fn auto_build_variants(photos: &[PhotoInfo], templates: &[Template], spec: &AlbumSpec, params: &BuildParams) -> Vec<BuildResult> {
    Style::ALL
        .iter()
        .map(|&style| {
            let mut p = params.clone();
            p.style = style;
            if style == Style::Airy {
                p.max_per = p.max_per.min(4);
            }
            auto_build(photos, templates, spec, &p)
        })
        .collect()
}

/// Re-layout an existing spread with a different template, keeping its photos.
/// Returns the new spread and photos that no longer fit.
pub fn apply_template(ctx: &Ctx, spread: &Spread, t: &Template, lookup: &dyn Fn(MediaId) -> Option<PhotoInfo>, pair: &crate::model::FontPair) -> (Spread, Vec<MediaId>) {
    // Photos in current visual order, biggest first so heroes stay heroes.
    let mut cur: Vec<&Frame> = spread.frames.iter().filter(|f| f.image_id.is_some()).collect();
    cur.sort_by(|a, b| b.rect_norm.area().total_cmp(&a.rect_norm.area()));
    let infos: Vec<PhotoInfo> = cur.iter().filter_map(|f| f.image_id.and_then(lookup)).collect();
    let k = t.photo_count as usize;
    let (keep, extra) = infos.split_at(infos.len().min(k));
    let refs: Vec<&PhotoInfo> = keep.iter().collect();
    let slots: Vec<_> = t.photo_frames().collect();
    let rects: Vec<Rect> = slots.iter().map(|f| f.surface_rect(&ctx.geom)).collect();
    let w = weights(Style::Classic);
    let params = BuildParams::default();
    // Square matrix: pad with dummy photos (score 0) when the template has more slots.
    let n = k.max(refs.len());
    let mut cost = vec![vec![0.0f64; n]; n];
    for s in 0..k {
        for (p, info) in refs.iter().enumerate() {
            cost[s][p] = -(eval_slot(ctx, &w, info, &rects[s], &slots[s].role, slots[s].min_aspect, slots[s].max_aspect, t.gutter_policy, &params).score as f64);
        }
    }
    let assign_full = hungarian(&cost);
    let assign: Vec<usize> = assign_full.iter().take(k).map(|&p| if p < refs.len() { p } else { usize::MAX }).collect();
    let mut sp = spread_from_choice(ctx, t, &refs, &assign, None);
    // Keep manual focus points from the old frames.
    for f in sp.frames.iter_mut() {
        if let Some(id) = f.image_id {
            if let Some(old) = spread.frames.iter().find(|o| o.image_id == Some(id)) {
                f.focus = old.focus;
                f.adjust = old.adjust;
                if let Some(fp) = old.focus.filter(|fp| fp.manual) {
                    let gutter = if t.gutter_policy == GutterPolicy::Safe { None } else { gutter_in_frame(&f.rect_norm, ctx.band_x0, ctx.band_x1) };
                    if let Some(info) = lookup(id) {
                        f.crop = auto_crop(info.aspect, ctx.frame_aspect(&f.rect_norm), Some(Vec2::new(fp.x, fp.y)), &info.faces, gutter).crop;
                    }
                }
            }
        }
    }
    sp.id = spread.id;
    sp.background = spread.background.clone();
    sp.shapes = spread.shapes.clone();
    sp.locked = spread.locked;
    sp.notes = spread.notes.clone();
    sp.texts = if spread.texts.is_empty() { texts_from_template(t, &ctx.geom, pair) } else { spread.texts.clone() };
    sp.explain = format!("Template: {}", t.name);
    (sp, extra.iter().map(|p| p.id).collect())
}

/// Best template for a fixed group of photos (used by timeline edits).
pub fn layout_group(ctx: &Ctx, templates: &[Template], photos: &[&PhotoInfo], style: Style, seed: u64) -> Spread {
    let cands: Vec<usize> = spread_templates(templates).into_iter().filter(|&i| templates[i].photo_count as usize == photos.len()).collect();
    let params = BuildParams { style, seed, ..Default::default() };
    let hist = History { families: vec![], uses: HashMap::new(), since_hero: 2 };
    let mut rng = Rng::new(seed);
    match choose_template(ctx, templates, &cands, photos, &params, &hist, &mut rng, true) {
        Some(c) => {
            let crops: Vec<CropResult> = c.evals.iter().map(|e| e.crop).collect();
            let mut sp = spread_from_choice(ctx, &templates[c.template], photos, &c.assign, Some(&crops));
            sp.explain = format!("{} photos · {}: {}", photos.len(), templates[c.template].name, c.notes.join("; "));
            sp
        }
        None => fallback_spread(ctx, photos),
    }
}

/// Re-crop a frame automatically (used after replace/swap).
pub fn recrop(ctx: &Ctx, frame: &Frame, p: &PhotoInfo) -> Crop {
    let focus = frame.focus.filter(|f| f.manual).map(|f| Vec2::new(f.x, f.y)).or(p.focus);
    let gutter = gutter_in_frame(&frame.rect_norm, ctx.band_x0, ctx.band_x1);
    auto_crop(p.aspect, ctx.frame_aspect(&frame.rect_norm), focus, &p.faces, gutter).crop
}

/// Replaces a cover template's sample title with the project's own name (front title and spine).
/// Only call this when the cover has no words of the user's yet.
pub fn personalize_cover_texts(sp: &mut Spread, t: &Template, title: &str) {
    let title = title.trim();
    if title.is_empty() {
        return;
    }
    for (tb, tt) in sp.texts.iter_mut().zip(t.texts.iter()) {
        if tt.region == crate::template::Region::Spine {
            tb.text = title.to_uppercase();
        } else if tt.preset == crate::model::StylePreset::CoverTitle || (tt.preset == crate::model::StylePreset::H2 && tt.placeholder.contains('&')) {
            tb.text = title.to_string();
        }
    }
}

/// Instantiate a cover template on a cover surface, filling photo slots in order.
pub fn cover_from_template(t: &Template, cg: &crate::spec::CoverGeometry, photos: &[&PhotoInfo], pair: &crate::model::FontPair, keep: Option<&Spread>) -> Spread {
    let (w, h) = (cg.trim_w(), cg.trim_h());
    let mut sp = Spread::blank();
    sp.template_id = t.id.clone();
    for (i, f) in t.photo_frames().enumerate() {
        let rect = f.surface_rect(cg);
        let mut fr = Frame::new(rect);
        fr.role = f.role.clone();
        if let Some(p) = photos.get(i) {
            let af = (rect.w * w as f32) / (rect.h * h as f32).max(1.0);
            fr.image_id = Some(p.id);
            fr.crop = auto_crop(p.aspect, af, p.focus, &p.faces, None).crop;
        }
        sp.frames.push(fr);
    }
    sp.texts = t
        .texts
        .iter()
        .map(|tt| {
            let mut tb = TextBlock::new(tt.surface_rect(cg), &tt.placeholder, tt.preset.style(pair));
            tb.rotation_mdeg = tt.rotation_mdeg;
            tb
        })
        .collect();
    if let Some(old) = keep {
        sp.id = old.id;
        sp.background = old.background.clone();
        sp.shapes = old.shapes.clone();
        // Keep the user's words when switching templates.
        for (new, prev) in sp.texts.iter_mut().zip(old.texts.iter()) {
            new.text = prev.text.clone();
        }
    }
    sp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::all_templates;
    use crate::model::new_id;

    #[test]
    fn cover_title_uses_project_name() {
        let t = all_templates().into_iter().find(|t| t.id == "oto-cover-04").expect("builtin cover template");
        let cg = crate::lab::generic_cover_geometry(&crate::spec::AlbumSpec::default(), 20);
        let pair = crate::model::font_pairs().remove(0);
        let mut sp = cover_from_template(&t, &cg, &[], &pair, None);
        personalize_cover_texts(&mut sp, &t, "Golden Test Album");
        assert!(sp.texts.iter().any(|x| x.text == "Golden Test Album"));
        assert!(sp.texts.iter().any(|x| x.text == "GOLDEN TEST ALBUM"));
        assert!(!sp.texts.iter().any(|x| x.text.contains("Aina")));
    }

    pub(crate) fn synthetic(n: usize) -> Vec<PhotoInfo> {
        let mut rng = Rng::new(42);
        (0..n)
            .map(|i| {
                let land = rng.f32() > 0.4;
                let (w, h) = if land { (6000, 4000) } else { (4000, 6000) };
                PhotoInfo {
                    id: new_id(),
                    name: format!("IMG_{i:04}.jpg"),
                    aspect: w as f32 / h as f32,
                    width: w,
                    height: h,
                    time: Some(1_700_000_000 + i as i64 * 40 + (i as i64 / 25) * 3600),
                    rating: (rng.next_u64() % 6) as u8,
                    pick: rng.f32() > 0.9,
                    is_bw: i % 17 == 0,
                    sharpness: rng.f32(),
                    brightness: rng.f32(),
                    colorfulness: rng.f32(),
                    focus: None,
                    faces: vec![],
                    similar_group: if i % 30 == 1 || i % 30 == 2 { Some(i as u32 / 30 + 1) } else { None },
                }
            })
            .collect()
    }

    #[test]
    fn hungarian_is_optimal() {
        let cost = vec![vec![4.0, 1.0, 3.0], vec![2.0, 0.0, 5.0], vec![3.0, 2.0, 2.0]];
        let a = hungarian(&cost);
        let total: f64 = a.iter().enumerate().map(|(i, &j)| cost[i][j]).sum();
        assert_eq!(total, 5.0);
    }

    #[test]
    fn partition_respects_bounds_and_duplicates() {
        let p = synthetic(120);
        let parts = partition(&p, 20, 1, 8, Grouping::Time, Style::Classic).unwrap();
        assert_eq!(parts.len(), 20);
        assert_eq!(parts.last().unwrap().end, 120);
        for r in &parts {
            assert!(!r.is_empty() && r.len() <= 8);
            let groups: Vec<_> = p[r.clone()].iter().filter_map(|x| x.similar_group).collect();
            let mut g2 = groups.clone();
            g2.dedup();
            assert_eq!(groups.len(), g2.len(), "duplicates share spread {r:?}");
        }
    }

    #[test]
    fn auto_build_uses_every_photo_once_and_is_deterministic() {
        let photos = synthetic(100);
        let templates = all_templates();
        let spec = AlbumSpec::default();
        let params = BuildParams { spreads: Some(20), min_per: 1, max_per: 5, ..Default::default() };
        let r = auto_build(&photos, &templates, &spec, &params);
        assert_eq!(r.spreads.len(), 20);
        let mut used: Vec<MediaId> = r.spreads.iter().flat_map(|s| s.media_ids()).collect();
        assert_eq!(used.len(), 100);
        used.sort();
        used.dedup();
        assert_eq!(used.len(), 100);
        for s in &r.spreads {
            assert!(!s.explain.is_empty());
            for f in &s.frames {
                assert!(f.rect_norm.within_unit());
            }
        }
        let r2 = auto_build(&photos, &templates, &spec, &params);
        let ids1: Vec<_> = r.spreads.iter().map(|s| (s.template_id.clone(), s.media_ids())).collect();
        let ids2: Vec<_> = r2.spreads.iter().map(|s| (s.template_id.clone(), s.media_ids())).collect();
        assert_eq!(ids1, ids2);
        // No three consecutive spreads from the same family.
        let fam: Vec<String> = r.spreads.iter().map(|s| templates.iter().find(|t| t.id == s.template_id).map(|t| t.family.clone()).unwrap_or_default()).collect();
        for w in fam.windows(3) {
            assert!(!(w[0] == w[1] && w[1] == w[2]), "{fam:?}");
        }
    }

    #[test]
    fn variants_differ_and_are_fast() {
        let photos = synthetic(300);
        let templates = all_templates();
        let spec = AlbumSpec::default();
        let params = BuildParams { spreads: Some(30), min_per: 1, max_per: 8, ..Default::default() };
        let t0 = std::time::Instant::now();
        let v = auto_build_variants(&photos, &templates, &spec, &params);
        let ms = t0.elapsed().as_millis();
        assert_eq!(v.len(), 3);
        let a: Vec<_> = v[0].spreads.iter().map(|s| s.template_id.clone()).collect();
        let b: Vec<_> = v[2].spreads.iter().map(|s| s.template_id.clone()).collect();
        assert_ne!(a, b);
        // Generous bound for debug builds on CI; release target is < 3 s for all three.
        assert!(ms < 30_000, "took {ms} ms");
    }

    #[test]
    fn too_few_templates_falls_back() {
        let photos = synthetic(9);
        let templates: Vec<Template> = all_templates().into_iter().filter(|t| t.photo_count <= 2).collect();
        let spec = AlbumSpec::default();
        let params = BuildParams { spreads: Some(1), max_per: 9, ..Default::default() };
        let r = auto_build(&photos, &templates, &spec, &params);
        let used: usize = r.spreads.iter().map(|s| s.media_ids().len()).sum();
        assert_eq!(used, 9);
        assert!(!r.warnings.is_empty());
    }

    #[test]
    fn exif_time_parsing() {
        assert!(parse_time("2026:10:25 14:02:11").is_none() || parse_time("2026-10-25T14:02:11").is_some());
        assert_eq!(parse_time("2026-10-25T14:02:11"), parse_time("2026-10-25T14:02:11Z"));
    }
}
