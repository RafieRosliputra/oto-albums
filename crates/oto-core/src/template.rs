//! Slot-based templates (TOML/JSON). Coordinates are normalized 0..1 inside a *region*.

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::model::StylePreset;
use crate::spec::{CoverGeometry, SpreadGeometry};
use crate::units::MilliDeg;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GutterPolicy {
    /// No photo crosses the fold.
    #[default]
    Safe,
    /// A photo may run across the fold edge to edge.
    FullBleed,
    /// Photo crosses the fold, but faces/focus must stay out of the gutter band.
    NoFace,
}

impl GutterPolicy {
    pub fn key(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::FullBleed => "full_bleed",
            Self::NoFace => "no_face",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TemplateKind {
    #[default]
    Spread,
    Cover,
}

/// Which part of the surface the normalized coordinates refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    /// Whole trimmed surface (spread, or cover incl. wrap).
    #[default]
    Surface,
    LeftPage,
    RightPage,
    Front,
    Back,
    Spine,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateFrame {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub region: Region,
    /// "hero" | "support" | "detail" | "background"
    pub role: String,
    /// Acceptable photo aspect range for this slot (width / height).
    pub min_aspect: f32,
    pub max_aspect: f32,
    #[serde(default)]
    pub allow_rotate: bool,
    #[serde(default)]
    pub z: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateText {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub region: Region,
    pub preset: StylePreset,
    pub placeholder: String,
    #[serde(default)]
    pub rotation_mdeg: MilliDeg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub family: String,
    #[serde(default)]
    pub kind: TemplateKind,
    pub photo_count: u32,
    /// Summary of slot orientations, e.g. "LLP".
    pub orientation: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// "classic" | "airy" | "editorial" | "any"
    pub style: String,
    pub gutter_policy: GutterPolicy,
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub custom: bool,
    pub frames: Vec<TemplateFrame>,
    #[serde(default)]
    pub texts: Vec<TemplateText>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("template TOML could not be read: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("template is invalid: {0}")]
    Invalid(String),
}

/// Resolves a region to a normalized rect of the full surface.
pub trait RegionMap {
    fn region(&self, r: Region) -> Rect;
}

impl RegionMap for SpreadGeometry {
    fn region(&self, r: Region) -> Rect {
        match r {
            Region::LeftPage => Rect::new(0.0, 0.0, 0.5, 1.0),
            Region::RightPage => Rect::new(0.5, 0.0, 0.5, 1.0),
            _ => Rect::UNIT,
        }
    }
}

impl RegionMap for CoverGeometry {
    fn region(&self, r: Region) -> Rect {
        let (w, h) = (self.trim_w(), self.trim_h());
        let to = |rr: crate::geometry::RectUm| rr.to_norm(w, h);
        match r {
            Region::Front | Region::RightPage => to(self.front()),
            Region::Back | Region::LeftPage => to(self.back()),
            Region::Spine => to(self.spine()),
            Region::Surface => Rect::UNIT,
        }
    }
}

pub fn map_rect(r: Rect, region: Rect) -> Rect {
    Rect::new(region.x + r.x * region.w, region.y + r.y * region.h, r.w * region.w, r.h * region.h)
}

impl TemplateFrame {
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
    pub fn surface_rect(&self, map: &dyn RegionMap) -> Rect {
        map_rect(self.rect(), map.region(self.region))
    }
}

impl TemplateText {
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
    pub fn surface_rect(&self, map: &dyn RegionMap) -> Rect {
        map_rect(self.rect(), map.region(self.region))
    }
}

impl Template {
    pub fn from_toml(s: &str) -> Result<Self, TemplateError> {
        let t: Template = toml::from_str(s)?;
        let errs = t.validate();
        if errs.is_empty() {
            Ok(t)
        } else {
            Err(TemplateError::Invalid(errs.join("; ")))
        }
    }

    /// Human-friendly TOML: f32 fractions are rounded to 4 decimals (0.1 mm on a 1 m page) so files stay readable.
    pub fn to_toml(&self) -> String {
        fn tidy(v: &mut toml::Value) {
            match v {
                toml::Value::Float(f) => *f = (*f * 10_000.0).round() / 10_000.0,
                toml::Value::Array(a) => a.iter_mut().for_each(tidy),
                toml::Value::Table(t) => t.iter_mut().for_each(|(_, v)| tidy(v)),
                _ => {}
            }
        }
        match toml::Value::try_from(self) {
            Ok(mut v) => {
                tidy(&mut v);
                toml::to_string_pretty(&v).unwrap_or_default()
            }
            Err(_) => toml::to_string_pretty(self).unwrap_or_default(),
        }
    }

    /// Structural validation in a reference 2:1 spread.
    pub fn validate(&self) -> Vec<String> {
        let mut e = Vec::new();
        if self.id.trim().is_empty() {
            e.push("id is empty".into());
        }
        let photo_frames: Vec<&TemplateFrame> = self.frames.iter().filter(|f| f.role != "background").collect();
        if photo_frames.len() as u32 != self.photo_count {
            e.push(format!("photo_count {} but {} photo frames", self.photo_count, photo_frames.len()));
        }
        let reference = SpreadGeometry { trim_w: 508_000, trim_h: 254_000, page_w: 254_000, bleed: 3_000, safe: 5_000, trim_outer: 3_000, trim_inside: 0, gutter: 10_000 };
        let cover_ref = CoverGeometry { panel_w: 254_000, panel_h: 254_000, spine_w: 14_000, wrap: 15_000, hinge: 8_000, bleed: 3_000, safe: 5_000 };
        let map: &dyn RegionMap = if self.kind == TemplateKind::Cover { &cover_ref } else { &reference };
        let rects: Vec<Rect> = photo_frames.iter().map(|f| f.surface_rect(map)).collect();
        for (i, f) in photo_frames.iter().enumerate() {
            if !f.rect().within_unit() {
                e.push(format!("frame {} is outside 0..1", f.id));
            }
            if !(f.min_aspect > 0.0 && f.min_aspect <= f.max_aspect) {
                e.push(format!("frame {} has invalid aspect range", f.id));
            }
            for (j, other) in rects.iter().enumerate().skip(i + 1) {
                if rects[i].overlaps(other) {
                    e.push(format!("frames {} and {} overlap", f.id, photo_frames[j].id));
                }
            }
            if self.kind == TemplateKind::Spread && self.gutter_policy == GutterPolicy::Safe {
                let r = rects[i];
                if r.x < 0.5 - 1e-3 && r.right() > 0.5 + 1e-3 {
                    e.push(format!("frame {} crosses the fold but gutter_policy is 'safe'", f.id));
                }
            }
        }
        for t in &self.texts {
            if !t.rect().within_unit() {
                e.push("text box outside 0..1".into());
            }
        }
        e
    }

    /// Physical aspect of each photo frame in the given geometry.
    pub fn frame_aspects(&self, map: &dyn RegionMap, surface_w: i64, surface_h: i64) -> Vec<f32> {
        self.frames
            .iter()
            .filter(|f| f.role != "background")
            .map(|f| {
                let r = f.surface_rect(map);
                (r.w * surface_w as f32) / (r.h * surface_h as f32).max(1.0)
            })
            .collect()
    }

    pub fn photo_frames(&self) -> impl Iterator<Item = &TemplateFrame> {
        self.frames.iter().filter(|f| f.role != "background")
    }

    pub fn crosses_fold(&self) -> bool {
        self.gutter_policy != GutterPolicy::Safe
    }
}

/// Derive an aspect range for a slot from its physical aspect.
pub fn aspect_range(a: f32) -> (f32, f32) {
    (a / 1.75, a * 1.75)
}

pub fn orientation_letter(a: f32) -> char {
    if a > 1.08 {
        'L'
    } else if a < 0.93 {
        'P'
    } else {
        'S'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tf(id: &str, x: f32, y: f32, w: f32, h: f32) -> TemplateFrame {
        TemplateFrame { id: id.into(), x, y, w, h, region: Region::Surface, role: "support".into(), min_aspect: 0.3, max_aspect: 3.0, allow_rotate: false, z: 0 }
    }

    #[test]
    fn validation_catches_problems() {
        let mut t = Template {
            id: "t".into(),
            name: "T".into(),
            family: "classic".into(),
            kind: TemplateKind::Spread,
            photo_count: 2,
            orientation: "PP".into(),
            tags: vec![],
            style: "any".into(),
            gutter_policy: GutterPolicy::Safe,
            version: 1,
            custom: false,
            frames: vec![tf("a", 0.05, 0.1, 0.4, 0.8), tf("b", 0.55, 0.1, 0.4, 0.8)],
            texts: vec![],
        };
        assert!(t.validate().is_empty(), "{:?}", t.validate());
        t.frames[1].x = 0.3;
        assert!(!t.validate().is_empty());
        t.frames[1].x = 0.55;
        t.frames[0].w = 0.6; // crosses fold
        assert!(t.validate().iter().any(|e| e.contains("fold")));
        let back = Template::from_toml(&{
            t.frames[0].w = 0.4;
            t.to_toml()
        })
        .unwrap();
        assert_eq!(back, t);
    }
}
