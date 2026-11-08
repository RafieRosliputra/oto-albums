//! Crop maths and the automatic crop solver.
//!
//! A crop is stored as the image point shown at the frame centre (`center_norm`), a zoom
//! relative to "exactly cover the frame" and a rotation. Everything is in normalized,
//! *oriented* image coordinates.

use crate::geometry::{Rect, Vec2};
use crate::model::{Crop, FitMode};
use crate::units::{mdeg_to_rad, Um, UM_PER_INCH};

/// Visible part of the image (unrotated, normalized). Width/height may exceed 1 in `Fit` mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Visible {
    pub center: Vec2,
    pub w: f32,
    pub h: f32,
}

impl Visible {
    pub fn rect(&self) -> Rect {
        Rect::new(self.center.x - self.w / 2.0, self.center.y - self.h / 2.0, self.w, self.h)
    }
    /// Map an image point to 0..1 frame coordinates (ignores crop rotation).
    pub fn image_to_frame(&self, p: Vec2) -> Vec2 {
        let r = self.rect();
        Vec2::new((p.x - r.x) / r.w.max(1e-6), (p.y - r.y) / r.h.max(1e-6))
    }
}

/// Fraction of the image visible at zoom 1 for a frame of aspect `af` (Fill).
fn base_fraction(ai: f32, af: f32) -> (f32, f32) {
    if af >= ai {
        (1.0, ai / af)
    } else {
        (af / ai, 1.0)
    }
}

/// Minimum zoom so the rotated frame stays inside the image.
pub fn min_zoom(ai: f32, af: f32, rotation_mdeg: i32) -> f32 {
    if rotation_mdeg == 0 {
        return 1.0;
    }
    let (vw, vh) = base_fraction(ai, af);
    let (fw, fh) = (vw * ai, vh);
    let t = mdeg_to_rad(rotation_mdeg);
    let (c, s) = (t.cos().abs(), t.sin().abs());
    let bw = fw * c + fh * s;
    let bh = fw * s + fh * c;
    (bw / ai).max(bh).max(1.0)
}

pub fn visible(crop: &Crop, ai: f32, af: f32, fit: FitMode) -> Visible {
    match fit {
        FitMode::Fill => {
            let (vw, vh) = base_fraction(ai, af);
            let z = crop.zoom.max(0.05);
            Visible { center: crop.center_norm, w: vw / z, h: vh / z }
        }
        FitMode::Fit => {
            // Whole image inside the frame: the visible window is larger than the image.
            let (vw, vh) = if af >= ai { (af / ai, 1.0) } else { (1.0, ai / af) };
            let z = crop.zoom.max(0.05);
            Visible { center: Vec2::CENTER, w: vw / z, h: vh / z }
        }
    }
}

/// Clamp zoom and centre so the (rotated) crop never leaves the image (Fill mode).
pub fn clamp(crop: &Crop, ai: f32, af: f32) -> Crop {
    let zmin = min_zoom(ai, af, crop.rotation_mdeg);
    let zoom = crop.zoom.clamp(zmin, 20.0);
    let (vw, vh) = base_fraction(ai, af);
    let (vw, vh) = (vw / zoom, vh / zoom);
    let t = mdeg_to_rad(crop.rotation_mdeg);
    let (c, s) = (t.cos().abs(), t.sin().abs());
    // Half extents of the rotated visible window, normalized per axis.
    let fw = vw * ai;
    let fh = vh;
    let hx = (fw * c + fh * s) / 2.0 / ai;
    let hy = (fw * s + fh * c) / 2.0;
    let cx = if hx >= 0.5 { 0.5 } else { crop.center_norm.x.clamp(hx, 1.0 - hx) };
    let cy = if hy >= 0.5 { 0.5 } else { crop.center_norm.y.clamp(hy, 1.0 - hy) };
    Crop { center_norm: Vec2::new(cx, cy), zoom, rotation_mdeg: crop.rotation_mdeg }
}

/// Effective output resolution of an image placed in a frame.
pub fn effective_dpi(img_w: u32, img_h: u32, vis: &Visible, frame_w: Um, frame_h: Um) -> f32 {
    let in_w = frame_w as f32 / UM_PER_INCH as f32;
    let in_h = frame_h as f32 / UM_PER_INCH as f32;
    if in_w <= 0.0 || in_h <= 0.0 {
        return 0.0;
    }
    let dx = vis.w * img_w as f32 / in_w;
    let dy = vis.h * img_h as f32 / in_h;
    dx.min(dy)
}

/// Where the fold band sits relative to a frame (normalized frame x range), if it crosses.
#[derive(Debug, Clone, Copy)]
pub struct GutterInFrame {
    pub x0: f32,
    pub x1: f32,
}

pub fn gutter_in_frame(frame: &Rect, band_x0: f32, band_x1: f32) -> Option<GutterInFrame> {
    if frame.x < band_x1 && frame.right() > band_x0 {
        Some(GutterInFrame { x0: (band_x0 - frame.x) / frame.w, x1: (band_x1 - frame.x) / frame.w })
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropResult {
    pub crop: Crop,
    /// All faces inside the crop with the safety inset.
    pub face_safe: bool,
    /// Focus/faces outside the fold band.
    pub gutter_safe: bool,
    /// 0..1, how much of the image survives the crop (1 = nothing lost).
    pub kept: f32,
}

/// Inset from the crop edge that faces must respect (fraction of the visible window).
pub const FACE_INSET: f32 = 0.06;

/// Choose a crop that keeps the subject: faces first, then the focus point.
pub fn auto_crop(ai: f32, af: f32, focus: Option<Vec2>, faces: &[Rect], gutter: Option<GutterInFrame>) -> CropResult {
    let (vw, vh) = base_fraction(ai, af);
    let subject = if !faces.is_empty() {
        let x0 = faces.iter().map(|f| f.x).fold(1.0f32, f32::min);
        let y0 = faces.iter().map(|f| f.y).fold(1.0f32, f32::min);
        let x1 = faces.iter().map(|f| f.right()).fold(0.0f32, f32::max);
        let y1 = faces.iter().map(|f| f.bottom()).fold(0.0f32, f32::max);
        Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0)
    } else {
        // Without a known subject keep a little more headroom than dead centre.
        focus.unwrap_or(Vec2::new(0.5, 0.45))
    };
    let mut crop = clamp(&Crop { center_norm: subject, zoom: 1.0, rotation_mdeg: 0 }, ai, af);

    // Keep the subject out of the fold by sliding the window horizontally.
    let mut gutter_safe = true;
    if let Some(g) = gutter {
        let vis = visible(&crop, ai, af, FitMode::Fill);
        let points: Vec<Vec2> = if faces.is_empty() { vec![subject] } else { faces.iter().map(|f| f.center()).collect() };
        let in_band = |vis: &Visible| {
            points.iter().any(|p| {
                let fx = vis.image_to_frame(*p).x;
                fx > g.x0 && fx < g.x1
            })
        };
        if in_band(&vis) {
            gutter_safe = false;
            // Try moving so the subject lands just left or right of the band.
            let r = vis.rect();
            for target in [g.x0 - 0.04, g.x1 + 0.04] {
                if !(0.0..=1.0).contains(&target) {
                    continue;
                }
                let new_left = subject.x - target * r.w;
                let cand = clamp(&Crop { center_norm: Vec2::new(new_left + r.w / 2.0, crop.center_norm.y), ..crop }, ai, af);
                if !in_band(&visible(&cand, ai, af, FitMode::Fill)) {
                    crop = cand;
                    gutter_safe = true;
                    break;
                }
            }
        }
    }

    let vis = visible(&crop, ai, af, FitMode::Fill);
    let face_safe = faces.iter().all(|f| {
        let a = vis.image_to_frame(Vec2::new(f.x, f.y));
        let b = vis.image_to_frame(Vec2::new(f.right(), f.bottom()));
        a.x >= FACE_INSET && a.y >= FACE_INSET && b.x <= 1.0 - FACE_INSET && b.y <= 1.0 - FACE_INSET
    });
    CropResult { crop, face_safe, gutter_safe, kept: (vw * vh).min(1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn landscape_in_portrait_frame() {
        let r = auto_crop(1.5, 0.75, None, &[], None);
        let v = visible(&r.crop, 1.5, 0.75, FitMode::Fill);
        assert!((v.h - 1.0).abs() < 1e-5);
        assert!((v.w - 0.5).abs() < 1e-5);
        assert!(r.kept < 0.6);
    }

    #[test]
    fn face_moves_out_of_gutter() {
        // Wide frame across the fold, face exactly in the middle of the image.
        let face = Rect::new(0.47, 0.3, 0.06, 0.1);
        let g = GutterInFrame { x0: 0.46, x1: 0.54 };
        let r = auto_crop(1.5, 2.0, None, &[face], Some(g));
        // 1.5 image into 2.0 frame: only vertical slack, so the face cannot move sideways.
        assert!(!r.gutter_safe);
        let r = auto_crop(3.0, 1.2, None, &[face], Some(g));
        assert!(r.gutter_safe, "{r:?}");
    }

    #[test]
    fn dpi() {
        let v = Visible { center: Vec2::CENTER, w: 1.0, h: 1.0 };
        let d = effective_dpi(3000, 2000, &v, UM_PER_INCH * 10, UM_PER_INCH * 20 / 3);
        assert!((d - 300.0).abs() < 0.5);
    }

    proptest! {
        #[test]
        fn clamped_crop_stays_inside(ai in 0.3f32..3.5, af in 0.2f32..4.0, cx in -1.0f32..2.0, cy in -1.0f32..2.0, zoom in 0.1f32..6.0, rot in -45_000i32..45_000) {
            let c = clamp(&Crop { center_norm: Vec2::new(cx, cy), zoom, rotation_mdeg: rot }, ai, af);
            let v = visible(&c, ai, af, FitMode::Fill);
            let t = mdeg_to_rad(rot);
            let (co, si) = (t.cos().abs(), t.sin().abs());
            let fw = v.w * ai;
            let hx = (fw * co + v.h * si) / 2.0 / ai;
            let hy = (fw * si + v.h * co) / 2.0;
            prop_assert!(c.center_norm.x - hx >= -1e-3 && c.center_norm.x + hx <= 1.0 + 1e-3);
            prop_assert!(c.center_norm.y - hy >= -1e-3 && c.center_norm.y + hy <= 1.0 + 1e-3);
            prop_assert!(c.zoom >= 1.0 - 1e-6);
        }
    }
}
