//! Album specification and derived print geometry.

use serde::{Deserialize, Serialize};

use crate::geometry::RectUm;
use crate::units::{mm, um_to_mm, Um, Units};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoverType {
    None,
    Softcover,
    #[default]
    Hardcover,
    Layflat,
    Wrap,
}

impl CoverType {
    pub const ALL: [CoverType; 5] = [CoverType::None, CoverType::Softcover, CoverType::Hardcover, CoverType::Layflat, CoverType::Wrap];
    pub fn key(self) -> &'static str {
        match self {
            CoverType::None => "none",
            CoverType::Softcover => "softcover",
            CoverType::Hardcover => "hardcover",
            CoverType::Layflat => "layflat",
            CoverType::Wrap => "wrap",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == s)
    }
}

/// How the book is bound — decides default export layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    /// Spreads print as one continuous sheet (layflat / flush mount).
    #[default]
    Layflat,
    /// Facing pages printed separately (perfect bound / single page labs).
    SinglePages,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColorProfile {
    pub name: String,
    /// `None` = built-in sRGB profile shipped with the app.
    pub icc_path: Option<String>,
}

impl Default for ColorProfile {
    fn default() -> Self {
        Self { name: "sRGB IEC61966-2.1".into(), icc_path: None }
    }
}

impl ColorProfile {
    pub fn is_builtin_srgb(&self) -> bool {
        self.icc_path.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSpec {
    pub page_width_um: Um,
    pub page_height_um: Um,
    pub spread_width_um: Um,
    pub spread_height_um: Um,
    pub bleed_um: Um,
    /// Cutting tolerance band inside the trim line on outer edges.
    pub trim_outer_um: Um,
    /// Extra hidden area at the binding edge of each page.
    pub trim_inside_um: Um,
    /// Safe distance from the trim line.
    pub safe_um: Um,
    /// Width of the no-go band centred on the fold.
    pub gutter_um: Um,
    pub min_pages: u32,
    pub max_pages: u32,
    pub page_increment: u32,
    pub output_profile: ColorProfile,
    pub units: Units,
    pub cover_type: CoverType,
    pub binding: Binding,
    pub target_dpi: u32,
}

impl Default for AlbumSpec {
    fn default() -> Self {
        AlbumSpec::new_square(mm(254.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecIssue {
    pub field: &'static str,
    pub message: String,
}

impl AlbumSpec {
    pub fn new_square(side: Um) -> Self {
        AlbumSpec {
            page_width_um: side,
            page_height_um: side,
            spread_width_um: side * 2,
            spread_height_um: side,
            bleed_um: mm(3.0),
            trim_outer_um: mm(3.0),
            trim_inside_um: 0,
            safe_um: mm(5.0),
            gutter_um: mm(10.0),
            min_pages: 20,
            max_pages: 100,
            page_increment: 2,
            output_profile: ColorProfile::default(),
            units: Units::Mm,
            cover_type: CoverType::Hardcover,
            binding: Binding::Layflat,
            target_dpi: 300,
        }
    }

    /// Keep derived fields consistent after edits.
    pub fn normalize(&mut self) {
        self.spread_width_um = self.page_width_um * 2;
        self.spread_height_um = self.page_height_um;
        if self.page_increment == 0 {
            self.page_increment = 1;
        }
        if self.target_dpi == 0 {
            self.target_dpi = 300;
        }
    }

    pub fn geometry(&self) -> SpreadGeometry {
        SpreadGeometry::from_spec(self)
    }

    pub fn pages_for_spreads(&self, spreads: u32) -> u32 {
        spreads * 2
    }

    /// Is `pages` a page count this spec accepts?
    pub fn page_count_ok(&self, pages: u32) -> bool {
        let inc = self.page_increment.max(1);
        pages >= self.min_pages && pages <= self.max_pages && (pages - self.min_pages) % inc == 0
    }

    pub fn min_spreads(&self) -> u32 {
        self.min_pages.div_ceil(2).max(1)
    }
    pub fn max_spreads(&self) -> u32 {
        (self.max_pages / 2).max(self.min_spreads())
    }

    /// Explain the spread formula for the wizard diagram.
    pub fn formula(&self) -> String {
        let u = self.units;
        format!(
            "spread = 2 × page ({}) = {}; file = spread + 2 × bleed ({}) = {} × {}",
            u.format(self.page_width_um),
            u.format(self.spread_width_um),
            u.format(self.bleed_um),
            u.format(self.spread_width_um + 2 * self.bleed_um),
            u.format(self.page_height_um + 2 * self.bleed_um),
        )
    }

    pub fn validate(&self) -> Vec<SpecIssue> {
        let mut v = Vec::new();
        let mut push = |field, message: String| v.push(SpecIssue { field, message });
        let page_min = mm(50.0);
        let page_max = mm(1000.0);
        for (f, val) in [("page_width", self.page_width_um), ("page_height", self.page_height_um)] {
            if val < page_min || val > page_max {
                push(f, format!("Page size must be between 50 and 1000 mm (now {:.1} mm).", um_to_mm(val)));
            }
        }
        if self.spread_width_um != self.page_width_um * 2 {
            push("spread_width", "Spread width must be exactly 2 × page width.".into());
        }
        if !(0..=mm(20.0)).contains(&self.bleed_um) {
            push("bleed", "Bleed must be between 0 and 20 mm.".into());
        }
        if self.bleed_um == 0 {
            push("bleed", "Bleed is 0 mm: any trimming error will show white edges. Most labs need 3 mm.".into());
        }
        let short = self.page_width_um.min(self.page_height_um);
        if self.safe_um < 0 || self.safe_um * 4 > short {
            push("safe", "Safe zone must be ≥ 0 and smaller than a quarter of the page.".into());
        }
        if self.trim_outer_um < 0 || self.trim_outer_um > self.safe_um.max(mm(10.0)) {
            push("trim_outer", "Trim tolerance must be ≥ 0 and normally no larger than the safe zone.".into());
        }
        if self.trim_inside_um < 0 || self.trim_inside_um * 4 > self.page_width_um {
            push("trim_inside", "Inside trim must be ≥ 0 and smaller than a quarter of the page.".into());
        }
        if self.gutter_um < 0 || self.gutter_um * 2 > self.page_width_um {
            push("gutter", "Gutter must be ≥ 0 and smaller than half a page.".into());
        }
        if self.min_pages == 0 || self.min_pages > self.max_pages {
            push("pages", "Minimum pages must be ≥ 1 and not above the maximum.".into());
        }
        if self.page_increment == 0 {
            push("pages", "Page increment must be at least 1.".into());
        }
        if !(72..=1200).contains(&self.target_dpi) {
            push("dpi", "Output resolution must be between 72 and 1200 dpi.".into());
        }
        v
    }
}

/// Derived geometry of one spread. Coordinates are micrometres with the origin at the
/// top-left of the **trim** box unless named `media_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpreadGeometry {
    pub trim_w: Um,
    pub trim_h: Um,
    pub page_w: Um,
    pub bleed: Um,
    pub safe: Um,
    pub trim_outer: Um,
    pub trim_inside: Um,
    pub gutter: Um,
}

impl SpreadGeometry {
    pub fn from_spec(s: &AlbumSpec) -> Self {
        SpreadGeometry {
            trim_w: s.page_width_um * 2,
            trim_h: s.page_height_um,
            page_w: s.page_width_um,
            bleed: s.bleed_um,
            safe: s.safe_um,
            trim_outer: s.trim_outer_um,
            trim_inside: s.trim_inside_um,
            gutter: s.gutter_um,
        }
    }
    pub fn media_w(&self) -> Um {
        self.trim_w + 2 * self.bleed
    }
    pub fn media_h(&self) -> Um {
        self.trim_h + 2 * self.bleed
    }
    /// Trim box relative to the media box.
    pub fn trim_in_media(&self) -> RectUm {
        RectUm::new(self.bleed, self.bleed, self.trim_w, self.trim_h)
    }
    pub fn fold_x(&self) -> Um {
        self.page_w
    }
    /// Half-width of the danger band around the fold (never below 4 mm so faces never sit in the fold).
    pub fn gutter_half(&self) -> Um {
        (self.gutter / 2).max(mm(4.0)) + self.trim_inside
    }
    pub fn gutter_band(&self) -> RectUm {
        let h = self.gutter_half();
        RectUm::new(self.fold_x() - h, 0, 2 * h, self.trim_h)
    }
    /// Safe area of each page (left, right), trim coordinates.
    pub fn safe_pages(&self) -> [RectUm; 2] {
        let inner = self.safe.max(self.gutter / 2) + self.trim_inside;
        let w = (self.page_w - self.safe - inner).max(0);
        let h = (self.trim_h - 2 * self.safe).max(0);
        [RectUm::new(self.safe, self.safe, w, h), RectUm::new(self.page_w + inner, self.safe, w, h)]
    }
    pub fn aspect(&self) -> f32 {
        self.trim_w as f32 / self.trim_h.max(1) as f32
    }
}

/// Cover surface geometry, laid out back | spine | front, with wrap all around.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverGeometry {
    pub panel_w: Um,
    pub panel_h: Um,
    pub spine_w: Um,
    pub wrap: Um,
    pub hinge: Um,
    pub bleed: Um,
    pub safe: Um,
}

impl CoverGeometry {
    pub fn trim_w(&self) -> Um {
        2 * self.panel_w + self.spine_w + 2 * self.wrap
    }
    pub fn trim_h(&self) -> Um {
        self.panel_h + 2 * self.wrap
    }
    pub fn media_w(&self) -> Um {
        self.trim_w() + 2 * self.bleed
    }
    pub fn media_h(&self) -> Um {
        self.trim_h() + 2 * self.bleed
    }
    pub fn back(&self) -> RectUm {
        RectUm::new(self.wrap, self.wrap, self.panel_w, self.panel_h)
    }
    pub fn spine(&self) -> RectUm {
        RectUm::new(self.wrap + self.panel_w, self.wrap, self.spine_w, self.panel_h)
    }
    pub fn front(&self) -> RectUm {
        RectUm::new(self.wrap + self.panel_w + self.spine_w, self.wrap, self.panel_w, self.panel_h)
    }
    pub fn safe_rects(&self) -> [RectUm; 2] {
        let s = self.safe + self.hinge;
        let b = self.back();
        let f = self.front();
        [
            RectUm::new(b.x + self.safe, b.y + self.safe, (b.w - self.safe - s).max(0), (b.h - 2 * self.safe).max(0)),
            RectUm::new(f.x + s, f.y + self.safe, (f.w - self.safe - s).max(0), (f.h - 2 * self.safe).max(0)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layflat_geometry() {
        let s = AlbumSpec::new_square(mm(254.0));
        assert!(s.validate().is_empty(), "{:?}", s.validate());
        let g = s.geometry();
        assert_eq!(g.media_w(), mm(514.0));
        assert_eq!(g.media_h(), mm(260.0));
        let [l, r] = g.safe_pages();
        assert_eq!(l.x, mm(5.0));
        assert!(l.right() < g.fold_x());
        assert!(r.x > g.fold_x());
        assert!(s.page_count_ok(40));
        assert!(!s.page_count_ok(41));
        assert!(!s.page_count_ok(18));
    }

    #[test]
    fn invalid_specs_are_explained() {
        let mut s = AlbumSpec::new_square(mm(254.0));
        s.min_pages = 50;
        s.max_pages = 20;
        s.bleed_um = mm(40.0);
        let issues = s.validate();
        assert!(issues.iter().any(|i| i.field == "pages"));
        assert!(issues.iter().any(|i| i.field == "bleed"));
    }
}
