//! Template library facade: designed catalogue, built-in spreads and covers,
//! filtering for a project, and ranking by photo-to-slot fit.

use std::collections::HashSet;

use crate::builtin;
use crate::designs::{self, AlbumKind, LayoutParams};
use crate::model::{MediaId, ProjectSettings, Spread};
use crate::solver::{self, Ctx, PhotoInfo};
use crate::spec::SpreadGeometry;
use crate::template::{Template, TemplateKind};

/// All templates in the picker: designed catalogue, built-in spreads, and covers.
pub fn library(geom: &SpreadGeometry, layout: &LayoutParams) -> Vec<Template> {
    let mut out = designs::all_designed_templates(geom, layout);

    for mut t in builtin::spread_templates() {
        if !t.tags.iter().any(|tag| tag == "classic") {
            t.tags.push("classic".into());
        }
        out.push(t);
    }

    out.extend(builtin::cover_templates());
    out
}

/// Templates allowed for a project.
///
/// Covers are always available. Designed spread templates must match the album
/// kind and, when the project asks for specific groups, one of those groups.
/// Any template explicitly listed in `template_ids` is added regardless of the
/// filters. The returned order is stable and contains no duplicates.
pub fn project_templates(geom: &SpreadGeometry, s: &ProjectSettings) -> Vec<Template> {
    let all = library(geom, &s.layout);
    let mut out: Vec<Template> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let groups_empty = s.template_groups.is_empty();

    for t in all.iter().filter(|t| t.kind == TemplateKind::Cover) {
        if seen.insert(t.id.clone()) {
            out.push(t.clone());
        }
    }

    for t in all.iter() {
        if t.kind == TemplateKind::Cover {
            continue;
        }

        if let Some(kind) = AlbumKind::of(t) {
            let in_group = groups_empty || s.template_groups.iter().any(|g| g == &t.family);
            if kind == s.album_kind && in_group && seen.insert(t.id.clone()) {
                out.push(t.clone());
            }
        }
    }

    for t in all.iter() {
        if seen.contains(&t.id) {
            continue;
        }

        let base = t.id.strip_suffix("-m").unwrap_or(&t.id);
        if s.template_ids.iter().any(|id| id == &t.id || id == base) && seen.insert(t.id.clone()) {
            out.push(t.clone());
        }
    }

    out
}

/// Score every spread template with the same photo count and return
/// `(template_index, score)` ordered best-first. The score is deterministic and
/// uses the solver's slot scoring after an optimal photo-to-slot assignment.
pub fn rank_templates(ctx: &Ctx, photos: &[&PhotoInfo], templates: &[Template]) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = templates
        .iter()
        .enumerate()
        .filter_map(|(idx, t)| {
            if t.kind == TemplateKind::Spread && !photos.is_empty() && t.photo_count as usize == photos.len() {
                solver::score_template_slots(ctx, t, photos).map(|score| (idx, score))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored
}

/// Re-layout an existing spread onto `t`, keeping its photos.
pub fn layout_with(ctx: &Ctx, spread: &Spread, t: &Template, lookup: &dyn Fn(MediaId) -> Option<PhotoInfo>) -> Spread {
    let pair = crate::model::font_pairs().into_iter().next().unwrap_or_else(|| crate::model::FontPair { name: "Classic Serif".into(), display: "Playfair Display".into(), text: "Lora".into() });

    let (relaid, _extra) = solver::apply_template(ctx, spread, t, lookup, &pair);
    relaid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Frame;
    use crate::spec::AlbumSpec;

    fn test_photo(aspect: f32) -> PhotoInfo {
        let (width, height) = if aspect >= 1.0 { (6000u32, (6000.0 / aspect) as u32) } else { ((6000.0 * aspect) as u32, 6000u32) };

        PhotoInfo {
            id: crate::model::new_id(),
            name: format!("test-{aspect}"),
            aspect,
            width,
            height,
            time: None,
            rating: 0,
            pick: false,
            is_bw: false,
            sharpness: 1.0,
            brightness: 0.5,
            colorfulness: 0.5,
            focus: None,
            faces: vec![],
            similar_group: None,
        }
    }

    #[test]
    fn rank_templates_is_orientation_aware() {
        let spec = AlbumSpec::default();
        let ctx = Ctx::new(&spec);
        let all = builtin::spread_templates();
        let portrait_t = all.iter().find(|t| t.name == "Portrait right").expect("portrait builtin").clone();
        let landscape_t = all.iter().find(|t| t.name == "Landscape left").expect("landscape builtin").clone();
        let candidates = vec![portrait_t.clone(), landscape_t.clone()];

        let portrait_photo = test_photo(2.0 / 3.0);
        let ranked = rank_templates(&ctx, &[&portrait_photo], &candidates);
        assert!(!ranked.is_empty());
        assert_eq!(ranked[0].0, 0, "portrait template should win for portrait photo, got {ranked:?}");

        let landscape_photo = test_photo(1.5);
        let ranked = rank_templates(&ctx, &[&landscape_photo], &candidates);
        assert!(!ranked.is_empty());
        assert_eq!(ranked[0].0, 1, "landscape template should win for landscape photo, got {ranked:?}");
    }

    #[test]
    fn project_templates_filters_by_kind_group_and_ids() {
        let spec = AlbumSpec::default();
        let ctx = Ctx::new(&spec);
        let geom = ctx.geom;
        let all = library(&geom, &LayoutParams::default());

        let forced = all.iter().find(|t| AlbumKind::of(t) == Some(AlbumKind::Album) && t.photo_count == 2 && !t.id.ends_with("-m")).expect("designed album template").clone();
        let forced_base = forced.id.strip_suffix("-m").map(str::to_string).unwrap_or_else(|| forced.id.clone());

        let settings = ProjectSettings { album_kind: AlbumKind::Photobook, template_groups: vec!["essentials".into()], template_ids: vec![forced_base.clone()], ..ProjectSettings::default() };

        let filtered = project_templates(&geom, &settings);

        assert!(filtered.iter().any(|t| t.kind == TemplateKind::Cover));

        let mut seen: HashSet<String> = HashSet::new();
        for t in &filtered {
            assert!(seen.insert(t.id.clone()), "duplicate {}", t.id);
        }

        for t in filtered.iter().filter(|t| AlbumKind::of(t) == Some(AlbumKind::Album) && t.kind == TemplateKind::Spread) {
            let base = t.id.strip_suffix("-m").unwrap_or(&t.id);
            assert_eq!(base, forced_base.as_str(), "album template leaked {}", t.id);
        }

        for t in filtered.iter().filter(|t| AlbumKind::of(t) == Some(AlbumKind::Photobook) && t.kind == TemplateKind::Spread) {
            assert_eq!(t.family, "essentials", "unexpected photobook family {}", t.family);
        }

        assert!(filtered.iter().any(|t| t.id == forced.id || t.id.strip_suffix("-m") == Some(forced_base.as_str())));
    }

    #[test]
    fn layout_with_keeps_every_photo() {
        let spec = AlbumSpec::default();
        let ctx = Ctx::new(&spec);
        let spread_templates = builtin::spread_templates();
        let template = spread_templates.iter().find(|t| t.photo_count == 2 && t.name == "Twin matted").expect("two-photo builtin").clone();

        let mut spread = Spread::blank();
        let p1 = test_photo(2.0 / 3.0);
        let p2 = test_photo(1.5);

        let mut f1 = Frame::new(crate::geometry::Rect::new(0.09, 0.09, 0.32, 0.82));
        f1.image_id = Some(p1.id);
        let mut f2 = Frame::new(crate::geometry::Rect::new(0.59, 0.09, 0.32, 0.82));
        f2.image_id = Some(p2.id);
        spread.frames = vec![f1, f2];

        let photos = [p1, p2];
        let lookup = |id: MediaId| photos.iter().find(|p| p.id == id).cloned();
        let relaid = layout_with(&ctx, &spread, &template, &lookup);

        let mut before = spread.media_ids();
        let mut after = relaid.media_ids();
        before.sort();
        after.sort();
        assert_eq!(before, after);
    }
}
