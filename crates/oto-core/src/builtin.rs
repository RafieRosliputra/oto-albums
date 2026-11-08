//! Original built-in templates for oto-albums.
//!
//! Spreads are composed from a small vocabulary of *page layouts* (left page + right page)
//! plus a few layouts that deliberately cross the fold. Every layout here was designed for
//! oto-albums on an 8 pt rhythm (margin 8 %, gap 3.5 % of a page); none is copied from
//! another product.

use crate::model::StylePreset;
use crate::spec::SpreadGeometry;
use crate::template::{aspect_range, orientation_letter, GutterPolicy, Region, Template, TemplateFrame, TemplateKind, TemplateText};

const M: f32 = 0.08;
const G: f32 = 0.035;

type Slot = (f32, f32, f32, f32);

#[derive(Clone, Copy)]
enum Page {
    Blank,
    Full,
    Margin,
    Portrait,
    Landscape,
    Small,
    TwoStack,
    TwoSide,
    TopTwo,
    ThreeCols,
    BigLeftTwo,
    Grid4,
    TopThree,
    Grid6,
    Story,
}

fn page_slots(p: Page) -> Vec<Slot> {
    let inner = 1.0 - 2.0 * M;
    match p {
        Page::Blank | Page::Story => vec![],
        Page::Full => vec![(0.0, 0.0, 1.0, 1.0)],
        Page::Margin => vec![(M, M, inner, inner)],
        Page::Portrait => vec![(0.18, 0.1, 0.64, 0.8)],
        Page::Landscape => vec![(0.1, 0.24, 0.8, 0.52)],
        Page::Small => vec![(0.3, 0.27, 0.4, 0.46)],
        Page::TwoStack => {
            let h = (inner - G) / 2.0;
            vec![(M, M, inner, h), (M, M + h + G, inner, h)]
        }
        Page::TwoSide => {
            let w = (inner - G) / 2.0;
            vec![(M, 0.16, w, 0.68), (M + w + G, 0.16, w, 0.68)]
        }
        Page::TopTwo => {
            let h1 = (inner - G) * 0.56;
            let h2 = inner - G - h1;
            let w = (inner - G) / 2.0;
            vec![(M, M, inner, h1), (M, M + h1 + G, w, h2), (M + w + G, M + h1 + G, w, h2)]
        }
        Page::ThreeCols => {
            let w = (inner - 2.0 * G) / 3.0;
            (0..3).map(|i| (M + i as f32 * (w + G), 0.22, w, 0.56)).collect()
        }
        Page::BigLeftTwo => {
            let w1 = (inner - G) * 0.58;
            let w2 = inner - G - w1;
            let h = (inner - G) / 2.0;
            vec![(M, M, w1, inner), (M + w1 + G, M, w2, h), (M + w1 + G, M + h + G, w2, h)]
        }
        Page::Grid4 => {
            let s = (inner - G) / 2.0;
            vec![(M, M, s, s), (M + s + G, M, s, s), (M, M + s + G, s, s), (M + s + G, M + s + G, s, s)]
        }
        Page::TopThree => {
            let h1 = (inner - G) * 0.62;
            let h2 = inner - G - h1;
            let w = (inner - 2.0 * G) / 3.0;
            let mut v = vec![(M, M, inner, h1)];
            for i in 0..3 {
                v.push((M + i as f32 * (w + G), M + h1 + G, w, h2));
            }
            v
        }
        Page::Grid6 => {
            let w = (inner - G) / 2.0;
            let h = (inner - 2.0 * G) / 3.0;
            let mut v = Vec::new();
            for r in 0..3 {
                for c in 0..2 {
                    v.push((M + c as f32 * (w + G), M + r as f32 * (h + G), w, h));
                }
            }
            v
        }
    }
}

fn story_texts(region: Region) -> Vec<TemplateText> {
    vec![
        TemplateText { x: 0.14, y: 0.34, w: 0.72, h: 0.12, region, preset: StylePreset::H2, placeholder: "Chapter title".into(), rotation_mdeg: 0 },
        TemplateText {
            x: 0.14,
            y: 0.5,
            w: 0.72,
            h: 0.2,
            region,
            preset: StylePreset::Body,
            placeholder: "A few lines about this moment, the people in it and why it mattered.".into(),
            rotation_mdeg: 0,
        },
    ]
}

fn reference() -> SpreadGeometry {
    SpreadGeometry { trim_w: 508_000, trim_h: 254_000, page_w: 254_000, bleed: 3_000, safe: 5_000, trim_outer: 3_000, trim_inside: 0, gutter: 10_000 }
}

#[allow(clippy::too_many_arguments)]
fn spread(id: &str, name: &str, family: &str, style: &str, left: Page, right: Page, tags: &[&str]) -> Template {
    let mut frames = Vec::new();
    for (region, page) in [(Region::LeftPage, left), (Region::RightPage, right)] {
        for (i, (x, y, w, h)) in page_slots(page).into_iter().enumerate() {
            frames.push(TemplateFrame {
                id: format!("{}{}", if region == Region::LeftPage { "l" } else { "r" }, i + 1),
                x,
                y,
                w,
                h,
                region,
                role: String::new(),
                min_aspect: 0.0,
                max_aspect: 0.0,
                allow_rotate: false,
                z: 0,
            });
        }
    }
    let mut texts = Vec::new();
    if matches!(left, Page::Story) {
        texts.extend(story_texts(Region::LeftPage));
    }
    if matches!(right, Page::Story) {
        texts.extend(story_texts(Region::RightPage));
    }
    let any_full = matches!(left, Page::Full) || matches!(right, Page::Full);
    let mut tags: Vec<String> = tags.iter().map(|s| s.to_string()).collect();
    if any_full {
        tags.push("full-bleed-page".into());
    }
    build(id, name, family, style, GutterPolicy::Safe, frames, texts, tags)
}

#[allow(clippy::too_many_arguments)]
fn build(id: &str, name: &str, family: &str, style: &str, gp: GutterPolicy, frames: Vec<TemplateFrame>, texts: Vec<TemplateText>, tags: Vec<String>) -> Template {
    let t = Template {
        id: id.into(),
        name: name.into(),
        family: family.into(),
        kind: TemplateKind::Spread,
        photo_count: 0,
        orientation: String::new(),
        tags,
        style: style.into(),
        gutter_policy: gp,
        version: 1,
        custom: false,
        frames,
        texts,
    };
    let g = reference();
    finish_roles(t, g.trim_w as f32, g.trim_h as f32, &g)
}

fn finish_roles(mut t: Template, sw: f32, sh: f32, map: &dyn crate::template::RegionMap) -> Template {
    let areas: Vec<f32> = t.frames.iter().map(|f| f.surface_rect(map).area()).collect();
    let max = areas.iter().cloned().fold(0.0f32, f32::max);
    let n = t.frames.len();
    let mut orient = String::new();
    let mut hero_given = false;
    for (i, f) in t.frames.iter_mut().enumerate() {
        let r = f.surface_rect(map);
        let a = (r.w * sw) / (r.h * sh).max(1e-6);
        let (lo, hi) = aspect_range(a);
        f.min_aspect = (lo * 100.0).round() / 100.0;
        f.max_aspect = (hi * 100.0).round() / 100.0;
        let is_max = (areas[i] - max).abs() < 1e-6;
        f.role = if is_max && !hero_given && (n == 1 || areas.iter().filter(|&&x| (x - max).abs() < 1e-6).count() == 1) {
            hero_given = true;
            "hero".into()
        } else if areas[i] < max * 0.3 {
            "detail".into()
        } else {
            "support".into()
        };
        orient.push(orientation_letter(a));
    }
    t.photo_count = n as u32;
    t.orientation = orient;
    t
}

fn surface_frame(id: &str, x: f32, y: f32, w: f32, h: f32) -> TemplateFrame {
    TemplateFrame { id: id.into(), x, y, w, h, region: Region::Surface, role: String::new(), min_aspect: 0.0, max_aspect: 0.0, allow_rotate: false, z: 0 }
}

/// All built-in spread templates.
pub fn spread_templates() -> Vec<Template> {
    use Page::*;
    let mut v = Vec::new();
    let mut n = 0usize;
    let mut add = |family: &str, style: &str, name: &str, l: Page, r: Page, tags: &[&str]| {
        n += 1;
        let id = format!("oto-{family}-{n:02}");
        v.push(spread(&id, name, family, style, l, r, tags));
    };
    // 1 photo
    add("full-bleed", "editorial", "Left page bleed", Full, Blank, &["quiet"]);
    add("full-bleed", "editorial", "Right page bleed", Blank, Full, &["quiet"]);
    add("classic", "classic", "Matted left", Margin, Blank, &[]);
    add("classic", "classic", "Matted right", Blank, Margin, &[]);
    add("airy", "airy", "Small left", Small, Blank, &["whitespace"]);
    add("airy", "airy", "Small right", Blank, Small, &["whitespace"]);
    add("airy", "airy", "Portrait right", Blank, Portrait, &["whitespace"]);
    add("classic", "classic", "Landscape left", Landscape, Blank, &[]);
    add("story", "classic", "Story + matted", Story, Margin, &["text"]);
    add("story", "editorial", "Story + bleed", Story, Full, &["text"]);
    // 2 photos
    add("full-bleed", "editorial", "Twin bleed", Full, Full, &[]);
    add("hero-strip", "editorial", "Bleed + matted", Full, Margin, &[]);
    add("hero-strip", "editorial", "Matted + bleed", Margin, Full, &[]);
    add("classic", "classic", "Twin matted", Margin, Margin, &[]);
    add("classic", "classic", "Twin portraits", Portrait, Portrait, &[]);
    add("airy", "airy", "Twin small", Small, Small, &["whitespace"]);
    add("classic", "classic", "Twin landscapes", Landscape, Landscape, &[]);
    add("airy", "airy", "Stacked pair right", Blank, TwoStack, &["whitespace"]);
    add("airy", "airy", "Side pair right", Blank, TwoSide, &["whitespace"]);
    add("editorial", "editorial", "Bleed + detail", Full, Small, &["contrast"]);
    add("editorial", "editorial", "Detail + bleed", Small, Full, &["contrast"]);
    add("story", "classic", "Story + pair", Story, TwoStack, &["text"]);
    // 3 photos
    add("hero-strip", "editorial", "Bleed + stacked pair", Full, TwoStack, &[]);
    add("hero-strip", "editorial", "Stacked pair + bleed", TwoStack, Full, &[]);
    add("hero-strip", "editorial", "Bleed + side pair", Full, TwoSide, &[]);
    add("classic", "classic", "Matted + stacked", Margin, TwoStack, &[]);
    add("classic", "classic", "Stacked + matted", TwoStack, Margin, &[]);
    add("airy", "airy", "Trio right", Blank, TopTwo, &["whitespace"]);
    add("airy", "airy", "Columns right", Blank, ThreeCols, &["whitespace"]);
    add("classic", "classic", "Portrait + stacked", Portrait, TwoStack, &[]);
    add("classic", "classic", "Matted + side pair", Margin, TwoSide, &[]);
    // 4 photos
    add("classic", "classic", "Two stacks", TwoStack, TwoStack, &[]);
    add("classic", "classic", "Four portraits", TwoSide, TwoSide, &[]);
    add("hero-strip", "editorial", "Bleed + trio", Full, TopTwo, &[]);
    add("hero-strip", "editorial", "Bleed + feature", Full, BigLeftTwo, &[]);
    add("classic", "classic", "Trio + matted", TopTwo, Margin, &[]);
    add("classic", "classic", "Matted + columns", Margin, ThreeCols, &[]);
    add("grid", "airy", "Quiet grid right", Blank, Grid4, &["whitespace"]);
    add("classic", "classic", "Side pair + stack", TwoSide, TwoStack, &[]);
    add("classic", "classic", "Portrait + trio", Portrait, TopTwo, &[]);
    // 5 photos
    add("hero-strip", "editorial", "Bleed + grid", Full, Grid4, &[]);
    add("hero-strip", "editorial", "Grid + bleed", Grid4, Full, &[]);
    add("classic", "classic", "Stack + trio", TwoStack, TopTwo, &[]);
    add("classic", "classic", "Feature + side pair", BigLeftTwo, TwoSide, &[]);
    add("classic", "classic", "Matted + strip", Margin, TopThree, &[]);
    add("classic", "classic", "Trio + stack", TopTwo, TwoStack, &[]);
    // 6 photos
    add("grid", "classic", "Six columns", ThreeCols, ThreeCols, &[]);
    add("classic", "classic", "Twin trios", TopTwo, TopTwo, &[]);
    add("grid", "classic", "Stack + grid", TwoStack, Grid4, &[]);
    add("grid", "classic", "Grid + stack", Grid4, TwoStack, &[]);
    add("classic", "classic", "Twin features", BigLeftTwo, BigLeftTwo, &[]);
    // 7 photos
    add("grid", "classic", "Trio + grid", TopTwo, Grid4, &[]);
    add("grid", "classic", "Grid + feature", Grid4, BigLeftTwo, &[]);
    add("grid", "classic", "Matted + six", Margin, Grid6, &["contact"]);
    // 8 photos
    add("grid", "classic", "Eight grid", Grid4, Grid4, &["contact"]);
    add("grid", "classic", "Twin strips", TopThree, TopThree, &[]);
    add("grid", "classic", "Stack + six", TwoStack, Grid6, &["contact"]);
    // text only
    add("story", "any", "Story left", Story, Blank, &["text"]);
    add("story", "any", "Story right", Blank, Story, &["text"]);

    // Layouts that cross the fold on purpose.
    let mut k = 0usize;
    let mut cross = |family: &str, style: &str, name: &str, gp: GutterPolicy, frames: Vec<TemplateFrame>| {
        k += 1;
        let id = format!("oto-{family}-x{k:02}");
        v.push(build(&id, name, family, style, gp, frames, vec![], vec!["crosses-fold".into()]));
    };
    cross("panorama", "editorial", "Panorama bleed", GutterPolicy::FullBleed, vec![surface_frame("p1", 0.0, 0.0, 1.0, 1.0)]);
    cross("panorama", "classic", "Panorama matted", GutterPolicy::NoFace, vec![surface_frame("p1", 0.05, 0.1, 0.9, 0.8)]);
    cross("panorama", "editorial", "Panorama band", GutterPolicy::NoFace, vec![surface_frame("p1", 0.0, 0.18, 1.0, 0.64)]);
    cross(
        "panorama",
        "classic",
        "Panorama + pair",
        GutterPolicy::NoFace,
        vec![surface_frame("p1", 0.04, 0.08, 0.92, 0.52), surface_frame("p2", 0.04, 0.64, 0.43, 0.28), surface_frame("p3", 0.53, 0.64, 0.43, 0.28)],
    );
    cross(
        "hero-strip",
        "editorial",
        "Wide hero + column",
        GutterPolicy::NoFace,
        vec![surface_frame("p1", 0.0, 0.0, 0.64, 1.0), surface_frame("p2", 0.68, 0.08, 0.28, 0.26), surface_frame("p3", 0.68, 0.37, 0.28, 0.26), surface_frame("p4", 0.68, 0.66, 0.28, 0.26)],
    );
    cross(
        "editorial",
        "editorial",
        "Offset panorama",
        GutterPolicy::NoFace,
        vec![surface_frame("p1", 0.12, 0.12, 0.76, 0.56), surface_frame("p2", 0.12, 0.72, 0.2, 0.2), surface_frame("p3", 0.68, 0.72, 0.2, 0.2)],
    );
    v
}

fn cover(id: &str, name: &str, frames: Vec<TemplateFrame>, texts: Vec<TemplateText>) -> Template {
    let geom = crate::spec::CoverGeometry { panel_w: 254_000, panel_h: 254_000, spine_w: 14_000, wrap: 15_000, hinge: 8_000, bleed: 3_000, safe: 5_000 };
    let t = Template {
        id: id.into(),
        name: name.into(),
        family: "cover".into(),
        kind: TemplateKind::Cover,
        photo_count: 0,
        orientation: String::new(),
        tags: vec!["cover".into()],
        style: "any".into(),
        gutter_policy: GutterPolicy::FullBleed,
        version: 1,
        custom: false,
        frames,
        texts,
    };
    finish_roles(t, geom.trim_w() as f32, geom.trim_h() as f32, &geom)
}

fn rf(id: &str, region: Region, x: f32, y: f32, w: f32, h: f32) -> TemplateFrame {
    TemplateFrame { id: id.into(), x, y, w, h, region, role: String::new(), min_aspect: 0.0, max_aspect: 0.0, allow_rotate: false, z: 0 }
}

fn tt(region: Region, x: f32, y: f32, w: f32, h: f32, preset: StylePreset, text: &str) -> TemplateText {
    TemplateText { x, y, w, h, region, preset, placeholder: text.into(), rotation_mdeg: 0 }
}

fn spine_title() -> TemplateText {
    TemplateText { x: 0.0, y: 0.15, w: 1.0, h: 0.7, region: Region::Spine, preset: StylePreset::CoverSubtitle, placeholder: "AINA & DANIAL".into(), rotation_mdeg: 90_000 }
}

/// Built-in cover templates (front | spine | back).
pub fn cover_templates() -> Vec<Template> {
    use Region::*;
    use StylePreset::*;
    vec![
        cover(
            "oto-cover-01",
            "Type only",
            vec![],
            vec![tt(Front, 0.12, 0.4, 0.76, 0.12, CoverTitle, "Aina & Danial"), tt(Front, 0.12, 0.54, 0.76, 0.06, CoverSubtitle, "25 · 10 · 2026"), spine_title()],
        ),
        cover("oto-cover-02", "Front photo", vec![rf("f1", Front, 0.0, 0.0, 1.0, 1.0)], vec![spine_title()]),
        cover("oto-cover-03", "Full wrap photo", vec![rf("f1", Surface, 0.0, 0.0, 1.0, 1.0)], vec![]),
        cover(
            "oto-cover-04",
            "Window + title",
            vec![rf("f1", Front, 0.18, 0.14, 0.64, 0.5)],
            vec![tt(Front, 0.12, 0.7, 0.76, 0.1, CoverTitle, "Aina & Danial"), tt(Front, 0.12, 0.82, 0.76, 0.05, CoverSubtitle, "THE WEDDING"), spine_title()],
        ),
        cover(
            "oto-cover-05",
            "Small square",
            vec![rf("f1", Front, 0.32, 0.22, 0.36, 0.36)],
            vec![tt(Front, 0.12, 0.64, 0.76, 0.1, H1, "Our Story"), tt(Front, 0.12, 0.76, 0.76, 0.05, CoverSubtitle, "2026"), spine_title()],
        ),
        cover(
            "oto-cover-06",
            "Top photo",
            vec![rf("f1", Front, 0.0, 0.0, 1.0, 0.62)],
            vec![tt(Front, 0.1, 0.7, 0.8, 0.1, CoverTitle, "Aina & Danial"), tt(Front, 0.1, 0.82, 0.8, 0.05, CoverSubtitle, "25 · 10 · 2026")],
        ),
        cover("oto-cover-07", "Front + back photos", vec![rf("f1", Front, 0.0, 0.0, 1.0, 1.0), rf("b1", Back, 0.0, 0.0, 1.0, 1.0)], vec![spine_title()]),
        cover(
            "oto-cover-08",
            "Stacked front",
            vec![rf("f1", Front, 0.14, 0.1, 0.72, 0.38), rf("f2", Front, 0.14, 0.52, 0.72, 0.28)],
            vec![tt(Front, 0.14, 0.84, 0.72, 0.06, CoverSubtitle, "AINA & DANIAL")],
        ),
        cover("oto-cover-09", "Band across", vec![rf("f1", Surface, 0.0, 0.3, 1.0, 0.4)], vec![tt(Front, 0.12, 0.08, 0.76, 0.1, CoverTitle, "Aina & Danial")]),
        cover(
            "oto-cover-10",
            "Back detail",
            vec![rf("f1", Front, 0.1, 0.1, 0.8, 0.6), rf("b1", Back, 0.3, 0.32, 0.4, 0.36)],
            vec![tt(Front, 0.1, 0.76, 0.8, 0.08, H2, "Aina & Danial"), spine_title()],
        ),
        cover("oto-cover-11", "Stamp style", vec![], vec![tt(Front, 0.2, 0.45, 0.6, 0.1, H1, "A & D"), spine_title()]),
    ]
}

pub fn all_templates() -> Vec<Template> {
    let mut v = spread_templates();
    v.extend(cover_templates());
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_builtin_round_trips_through_toml() {
        for t in all_templates() {
            let back = crate::template::Template::from_toml(&t.to_toml()).unwrap_or_else(|e| panic!("{}: {e}", t.id));
            assert_eq!(back.id, t.id);
            assert_eq!(back.frames.len(), t.frames.len());
            for (a, b) in back.frames.iter().zip(t.frames.iter()) {
                assert!((a.x - b.x).abs() < 1e-4 && (a.w - b.w).abs() < 1e-4, "{}", t.id);
            }
        }
    }

    #[test]
    fn builtins_are_valid_and_plentiful() {
        let all = all_templates();
        let spreads: Vec<_> = all.iter().filter(|t| t.kind == TemplateKind::Spread).collect();
        let covers: Vec<_> = all.iter().filter(|t| t.kind == TemplateKind::Cover).collect();
        assert!(spreads.len() >= 60, "only {} spread templates", spreads.len());
        assert!(covers.len() >= 10);
        let mut ids = HashSet::new();
        for t in &all {
            assert!(ids.insert(t.id.clone()), "duplicate id {}", t.id);
            let errs = t.validate();
            assert!(errs.is_empty(), "{}: {errs:?}", t.id);
        }
        for n in 1..=8 {
            assert!(spreads.iter().any(|t| t.photo_count == n), "no template with {n} photos");
        }
        let hero = spreads.iter().filter(|t| t.frames.iter().any(|f| f.role == "hero")).count();
        assert!(hero > 30);
    }
}
