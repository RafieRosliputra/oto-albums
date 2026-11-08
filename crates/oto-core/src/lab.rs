//! Data-driven lab profiles (TOML). Nothing lab-specific is hard-coded in the app.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::expr;
use crate::spec::{AlbumSpec, Binding, ColorProfile, CoverGeometry, CoverType};
use crate::units::{mm, Um, Units};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabProfile {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub country: String,
    #[serde(default = "default_units")]
    pub units: String,
    /// Where the numbers came from (e.g. "example-only", "lab PDF 2026-05").
    pub source: String,
    pub verified_at: String,
    #[serde(default)]
    pub binding: Option<String>,
    pub sizes: Vec<LabSize>,
    #[serde(default)]
    pub color: LabColor,
    #[serde(default)]
    pub cover: Option<LabCover>,
}

fn default_units() -> String {
    "mm".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabSize {
    pub id: String,
    pub page_width_mm: f64,
    pub page_height_mm: f64,
    pub bleed_mm: f64,
    #[serde(default)]
    pub trim_outer_mm: f64,
    #[serde(default)]
    pub trim_inside_mm: f64,
    pub safe_mm: f64,
    #[serde(default)]
    pub gutter_mm: f64,
    pub min_pages: u32,
    pub max_pages: u32,
    pub page_increment: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabColor {
    pub profile: String,
    pub pdf_standard: String,
    #[serde(default = "default_intent")]
    pub rendering_intent: String,
    #[serde(default = "default_dpi")]
    pub dpi: u32,
}

fn default_intent() -> String {
    "relative_colorimetric".into()
}
fn default_dpi() -> u32 {
    300
}

impl Default for LabColor {
    fn default() -> Self {
        Self { profile: "sRGB".into(), pdf_standard: "PDF/X-4".into(), rendering_intent: default_intent(), dpi: 300 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabCover {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub wrap_mm: f64,
    #[serde(default)]
    pub hinge_mm: f64,
    #[serde(default)]
    pub bleed_mm: Option<f64>,
    #[serde(default)]
    pub safe_mm: Option<f64>,
    pub spine_formula: String,
    #[serde(default)]
    pub paper_caliper_mm: f64,
    #[serde(default)]
    pub min_spine_mm: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum LabError {
    #[error("lab profile TOML could not be read: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("lab profile is invalid: {0}")]
    Invalid(String),
}

impl LabProfile {
    pub fn from_toml(s: &str) -> Result<Self, LabError> {
        let p: LabProfile = toml::from_str(s)?;
        let errs = p.validate();
        if errs.is_empty() {
            Ok(p)
        } else {
            Err(LabError::Invalid(errs.join("; ")))
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn is_example(&self) -> bool {
        self.source.contains("example")
    }

    pub fn validate(&self) -> Vec<String> {
        let mut e = Vec::new();
        if self.schema_version != 1 {
            e.push(format!("unsupported schema_version {}", self.schema_version));
        }
        if self.id.trim().is_empty() || !self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            e.push("id must be non-empty ASCII letters, digits, '-' or '_'".into());
        }
        if self.source.trim().is_empty() || self.verified_at.trim().is_empty() {
            e.push("source and verified_at are required so users know how trustworthy the numbers are".into());
        }
        if self.sizes.is_empty() {
            e.push("at least one [[sizes]] entry is required".into());
        }
        for s in &self.sizes {
            let spec = self.spec_for_size(s);
            for issue in spec.validate() {
                // A zero-bleed profile is a warning in the wizard, not an invalid profile.
                if issue.field == "bleed" && s.bleed_mm == 0.0 {
                    continue;
                }
                e.push(format!("size {}: {}", s.id, issue.message));
            }
        }
        if let Some(c) = &self.cover {
            if CoverType::parse(&c.kind).is_none() {
                e.push(format!("unknown cover type '{}'", c.kind));
            }
            if let Err(err) = self.spine_width(c, 40) {
                e.push(format!("cover spine_formula: {err}"));
            }
        }
        e
    }

    pub fn size(&self, id: &str) -> Option<&LabSize> {
        self.sizes.iter().find(|s| s.id == id)
    }

    pub fn spec_for_size(&self, s: &LabSize) -> AlbumSpec {
        let mut spec = AlbumSpec {
            page_width_um: mm(s.page_width_mm),
            page_height_um: mm(s.page_height_mm),
            spread_width_um: 0,
            spread_height_um: 0,
            bleed_um: mm(s.bleed_mm),
            trim_outer_um: mm(s.trim_outer_mm),
            trim_inside_um: mm(s.trim_inside_mm),
            safe_um: mm(s.safe_mm),
            gutter_um: mm(s.gutter_mm),
            min_pages: s.min_pages,
            max_pages: s.max_pages,
            page_increment: s.page_increment,
            output_profile: ColorProfile::default(),
            units: Units::parse(&self.units).unwrap_or_default(),
            cover_type: self.cover.as_ref().and_then(|c| CoverType::parse(&c.kind)).unwrap_or(CoverType::None),
            binding: match self.binding.as_deref() {
                Some("single_pages") => Binding::SinglePages,
                _ => Binding::Layflat,
            },
            target_dpi: self.color.dpi,
        };
        spec.normalize();
        spec
    }

    pub fn spine_width(&self, c: &LabCover, pages: u32) -> Result<Um, expr::ExprError> {
        let mut vars: HashMap<&str, f64> = HashMap::new();
        vars.insert("pages", pages as f64);
        vars.insert("spreads", (pages / 2) as f64);
        vars.insert("paper_caliper_mm", c.paper_caliper_mm);
        let v = expr::eval(&c.spine_formula, &vars)?;
        Ok(mm(v.max(c.min_spine_mm).max(0.0)))
    }

    pub fn cover_geometry(&self, spec: &AlbumSpec, pages: u32) -> Option<CoverGeometry> {
        let c = self.cover.as_ref()?;
        let spine = self.spine_width(c, pages).ok()?;
        Some(CoverGeometry {
            panel_w: spec.page_width_um,
            panel_h: spec.page_height_um,
            spine_w: spine,
            wrap: mm(c.wrap_mm),
            hinge: mm(c.hinge_mm),
            bleed: c.bleed_mm.map(mm).unwrap_or(spec.bleed_um),
            safe: c.safe_mm.map(mm).unwrap_or(spec.safe_um),
        })
    }
}

/// Fallback cover geometry for custom specs (no lab profile): a flat wrap with a computed spine.
pub fn generic_cover_geometry(spec: &AlbumSpec, pages: u32) -> CoverGeometry {
    let (wrap, hinge) = match spec.cover_type {
        CoverType::Hardcover | CoverType::Layflat => (mm(15.0), mm(8.0)),
        CoverType::Wrap => (mm(10.0), 0),
        _ => (0, 0),
    };
    CoverGeometry { panel_w: spec.page_width_um, panel_h: spec.page_height_um, spine_w: mm((pages as f64 * 0.25 / 2.0 + 3.0).max(5.0)), wrap, hinge, bleed: spec.bleed_um, safe: spec.safe_um }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
schema_version = 1
id = "generic-layflat-10x10"
name = "Generic Layflat 10x10 (example)"
country = "MY"
units = "mm"
source = "example-only"
verified_at = "2026-10-25"

[[sizes]]
id = "10x10"
page_width_mm = 254
page_height_mm = 254
bleed_mm = 3
trim_outer_mm = 3
trim_inside_mm = 0
safe_mm = 5
gutter_mm = 0
min_pages = 20
max_pages = 100
page_increment = 2

[color]
profile = "sRGB"
pdf_standard = "PDF/X-4"
rendering_intent = "relative_colorimetric"

[cover]
type = "hardcover"
wrap_mm = 15
hinge_mm = 8
spine_formula = "pages * paper_caliper_mm / 2 + 2.0"
paper_caliper_mm = 0.6
"#;

    #[test]
    fn parses_and_builds_spec() {
        let p = LabProfile::from_toml(SAMPLE).unwrap();
        assert!(p.is_example());
        let s = p.spec_for_size(&p.sizes[0]);
        assert_eq!(s.spread_width_um, mm(508.0));
        assert_eq!(s.cover_type, CoverType::Hardcover);
        let g = p.cover_geometry(&s, 40).unwrap();
        assert_eq!(g.spine_w, mm(14.0));
        assert_eq!(g.trim_w(), mm(254.0 * 2.0 + 14.0 + 30.0));
        let back = LabProfile::from_toml(&p.to_toml()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn rejects_bad_profiles() {
        let bad = SAMPLE.replace("source = \"example-only\"", "source = \"\"");
        assert!(LabProfile::from_toml(&bad).is_err());
        let bad = SAMPLE.replace("pages * paper_caliper_mm", "pages * ?");
        assert!(LabProfile::from_toml(&bad).is_err());
    }
}
