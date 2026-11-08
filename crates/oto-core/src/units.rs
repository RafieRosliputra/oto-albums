//! Physical units. All stored geometry is integer micrometres (`i64`) to avoid float drift.

use serde::{Deserialize, Serialize};

/// Micrometres.
pub type Um = i64;

pub const UM_PER_MM: i64 = 1_000;
pub const UM_PER_INCH: i64 = 25_400;

#[inline]
pub fn mm(v: f64) -> Um {
    (v * UM_PER_MM as f64).round() as Um
}

#[inline]
pub fn um_to_mm(v: Um) -> f64 {
    v as f64 / UM_PER_MM as f64
}

#[inline]
pub fn um_to_in(v: Um) -> f64 {
    v as f64 / UM_PER_INCH as f64
}

#[inline]
pub fn inch(v: f64) -> Um {
    (v * UM_PER_INCH as f64).round() as Um
}

/// PostScript points (1/72 in), used by PDF.
#[inline]
pub fn um_to_pt(v: Um) -> f32 {
    (v as f64 * 72.0 / UM_PER_INCH as f64) as f32
}

#[inline]
pub fn pt_to_um(v: f64) -> Um {
    (v * UM_PER_INCH as f64 / 72.0).round() as Um
}

/// Pixels needed to cover `len` at `dpi`.
#[inline]
pub fn um_to_px(len: Um, dpi: f64) -> f64 {
    um_to_in(len) * dpi
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Mm,
    Cm,
    In,
}

impl Units {
    pub const ALL: [Units; 3] = [Units::Mm, Units::Cm, Units::In];

    pub fn label(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Cm => "cm",
            Units::In => "in",
        }
    }

    pub fn from_um(self, v: Um) -> f64 {
        match self {
            Units::Mm => um_to_mm(v),
            Units::Cm => um_to_mm(v) / 10.0,
            Units::In => um_to_in(v),
        }
    }

    pub fn to_um(self, v: f64) -> Um {
        match self {
            Units::Mm => mm(v),
            Units::Cm => mm(v * 10.0),
            Units::In => inch(v),
        }
    }

    /// Sensible number of decimals for display.
    pub fn decimals(self) -> usize {
        match self {
            Units::Mm => 1,
            Units::Cm => 2,
            Units::In => 3,
        }
    }

    pub fn format(self, v: Um) -> String {
        format!("{:.*} {}", self.decimals(), self.from_um(v), self.label())
    }

    pub fn parse(s: &str) -> Option<Units> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mm" => Some(Units::Mm),
            "cm" => Some(Units::Cm),
            "in" | "inch" | "inches" => Some(Units::In),
            _ => None,
        }
    }
}

/// Angles are stored as integer millidegrees.
pub type MilliDeg = i32;

#[inline]
pub fn mdeg_to_rad(v: MilliDeg) -> f32 {
    (v as f32 / 1000.0).to_radians()
}

#[inline]
pub fn deg_to_mdeg(v: f32) -> MilliDeg {
    (v * 1000.0).round() as MilliDeg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(mm(254.0), 254_000);
        assert_eq!(inch(10.0), 254_000);
        for u in Units::ALL {
            let v = u.to_um(u.from_um(123_456));
            assert!((v - 123_456).abs() <= 1, "{u:?} {v}");
        }
        assert!((um_to_pt(UM_PER_INCH) - 72.0).abs() < 1e-4);
        assert_eq!(pt_to_um(72.0), UM_PER_INCH);
        assert_eq!(Units::parse("IN"), Some(Units::In));
        assert_eq!(Units::Mm.format(3_000), "3.0 mm");
    }
}
