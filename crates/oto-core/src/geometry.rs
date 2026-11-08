//! Small geometry helpers. `Rect` is normalized (0..1) unless stated; `RectUm` is micrometres.

use serde::{Deserialize, Serialize};

use crate::units::Um;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub const CENTER: Vec2 = Vec2 { x: 0.5, y: 0.5 };
}

/// Normalized rectangle (x, y = top-left).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub const UNIT: Rect = Rect::new(0.0, 0.0, 1.0, 1.0);

    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn area(&self) -> f32 {
        self.w.max(0.0) * self.h.max(0.0)
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }
    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 > x0 && y1 > y0 {
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }
    /// Overlap area above a tiny epsilon (shared edges do not count).
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.intersection(o).map(|r| r.area() > 1e-6).unwrap_or(false)
    }
    pub fn within_unit(&self) -> bool {
        const E: f32 = 1e-4;
        self.x >= -E && self.y >= -E && self.right() <= 1.0 + E && self.bottom() <= 1.0 + E && self.w > 0.0 && self.h > 0.0
    }
    pub fn clamp_to_unit(&self) -> Rect {
        let w = self.w.clamp(0.005, 1.0);
        let h = self.h.clamp(0.005, 1.0);
        Rect::new(self.x.clamp(0.0, 1.0 - w), self.y.clamp(0.0, 1.0 - h), w, h)
    }
    pub fn inset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, (self.w - 2.0 * dx).max(0.0), (self.h - 2.0 * dy).max(0.0))
    }
}

/// Rectangle in micrometres, origin top-left of the trimmed spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RectUm {
    pub x: Um,
    pub y: Um,
    pub w: Um,
    pub h: Um,
}

impl RectUm {
    pub const fn new(x: Um, y: Um, w: Um, h: Um) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> Um {
        self.x + self.w
    }
    pub fn bottom(&self) -> Um {
        self.y + self.h
    }
    pub fn inset(&self, d: Um) -> RectUm {
        RectUm::new(self.x + d, self.y + d, (self.w - 2 * d).max(0), (self.h - 2 * d).max(0))
    }
    pub fn contains_rect(&self, o: &RectUm) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }
    /// Convert a normalized rect inside a `w × h` surface to micrometres.
    pub fn from_norm(r: &Rect, w: Um, h: Um) -> RectUm {
        let x0 = (r.x as f64 * w as f64).round() as Um;
        let y0 = (r.y as f64 * h as f64).round() as Um;
        let x1 = (r.right() as f64 * w as f64).round() as Um;
        let y1 = (r.bottom() as f64 * h as f64).round() as Um;
        RectUm::new(x0, y0, x1 - x0, y1 - y0)
    }
    pub fn to_norm(&self, w: Um, h: Um) -> Rect {
        let (w, h) = (w.max(1) as f64, h.max(1) as f64);
        Rect::new((self.x as f64 / w) as f32, (self.y as f64 / h) as f32, (self.w as f64 / w) as f32, (self.h as f64 / h) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_and_conversion() {
        let a = Rect::new(0.0, 0.0, 0.5, 1.0);
        let b = Rect::new(0.5, 0.0, 0.5, 1.0);
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&Rect::new(0.4, 0.1, 0.2, 0.2)));
        let um = RectUm::from_norm(&a, 508_000, 254_000);
        assert_eq!(um, RectUm::new(0, 0, 254_000, 254_000));
        let back = um.to_norm(508_000, 254_000);
        assert!((back.w - 0.5).abs() < 1e-6);
        assert!(Rect::new(1.2, 0.0, 0.5, 0.5).clamp_to_unit().within_unit());
    }
}
