//! First-run sample project: 12 synthetic (copyright-free) photos laid out on 6 spreads.

use std::path::{Path, PathBuf};

use oto_core::builtin::all_templates;
use oto_core::geometry::Rect;
use oto_core::model::{font_pairs, Cover, Pattern, PatternKind, Project, StylePreset, TextBlock};
use oto_core::solver::{auto_build, cover_from_template, BuildParams, PhotoInfo};
use oto_core::spec::{AlbumSpec, CoverType};
use oto_core::template::TemplateKind;
use oto_media::CacheLayout;

use crate::{Package, Result, StoreError};

/// Create (or reuse) the sample project inside `parent`. Returns the package path.
pub fn create_sample(parent: &Path) -> Result<PathBuf> {
    let root = parent.join("oto-albums Sample.otoalbum");
    if Package::db_path(&root).exists() {
        return Ok(root);
    }
    let photos_dir = parent.join("oto-albums Sample Photos");
    let files = oto_media::synth::write_set(&photos_dir, 12, 2400).map_err(|e| StoreError::Io { path: photos_dir.display().to_string(), source: e })?;
    let cache = CacheLayout::new(root.join("cache"));
    cache.ensure().map_err(|e| StoreError::Io { path: root.display().to_string(), source: e })?;
    let mut media: Vec<_> = files.iter().filter_map(|p| oto_media::import_one(p, &cache).ok()).collect();
    oto_media::assign_similarity(&mut media);
    for (i, m) in media.iter_mut().enumerate() {
        m.rating = [5, 3, 4, 2, 5, 3, 4, 3, 2, 4, 3, 5][i % 12];
        m.bins = vec![["Preparation", "Ceremony", "Portraits", "Reception"][i * 4 / 12].to_string()];
    }
    let spec = AlbumSpec { min_pages: 12, cover_type: CoverType::Hardcover, ..AlbumSpec::default() };
    let mut p = Project::new("Sample — Aina & Danial", spec.clone(), 0);
    p.client = "Sample client".into();
    let templates = all_templates();
    let infos: Vec<PhotoInfo> = media.iter().map(PhotoInfo::from_media).collect();
    let r = auto_build(&infos, &templates, &spec, &BuildParams { spreads: Some(6), min_per: 1, max_per: 3, ..Default::default() });
    p.spreads = r.spreads;
    let pair = font_pairs().into_iter().next().unwrap_or(oto_core::model::FontPair { name: "Classic".into(), display: "Lora".into(), text: "Lora".into() });
    if let Some(s) = p.spreads.first_mut() {
        if !s.frames.iter().any(|f| f.rect_norm.x >= 0.5 && f.rect_norm.y < 0.6) {
            s.texts.push(TextBlock::new(Rect::new(0.58, 0.44, 0.34, 0.1), "Aina & Danial", StylePreset::H1.style(&pair)));
        }
    }
    if let Some(s) = p.spreads.get_mut(1) {
        s.background.pattern = Some(Pattern::new(PatternKind::Dots));
    }
    let cg = oto_core::lab::generic_cover_geometry(&p.spec, p.page_count());
    if let Some(t) = templates.iter().find(|t| t.kind == TemplateKind::Cover && t.id == "oto-cover-04") {
        let first: Vec<&PhotoInfo> = infos.iter().take(1).collect();
        p.cover = Some(Cover { content: cover_from_template(t, &cg, &first, &pair, None), material_id: None, stamp: None });
    }
    p.timeline = oto_core::model::Timeline::from_spreads(&p.spreads);
    let pkg = Package::create(&root, &p, &media)?;
    pkg.close()?;
    Ok(root)
}
