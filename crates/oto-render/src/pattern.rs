//! Background patterns as resolution-independent primitives inside one square tile.
//! The PDF writer emits them as vectors; the raster renderer samples them per pixel.

use oto_core::model::{Pattern, PatternKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// Stroked open polyline.
    Poly {
        pts: Vec<(f32, f32)>,
        width: f32,
    },
}

/// Primitives for one tile of size `s` (mm), coordinates in mm, y down. Shapes may spill over the
/// tile edge; callers draw neighbouring copies so tiles join seamlessly.
pub fn tile(p: &Pattern) -> (f32, Vec<Prim>) {
    let s = p.scale_mm.max(1.0);
    let prims = match p.kind {
        PatternKind::Dots => vec![Prim::Circle { cx: s / 2.0, cy: s / 2.0, r: s * 0.16 }],
        PatternKind::Stripes => vec![Prim::Rect { x: 0.0, y: 0.0, w: s * 0.32, h: s }],
        PatternKind::Grid => vec![Prim::Rect { x: 0.0, y: 0.0, w: s * 0.06, h: s }, Prim::Rect { x: 0.0, y: 0.0, w: s, h: s * 0.06 }],
        PatternKind::Diagonal => vec![Prim::Poly { pts: vec![(-s * 0.1, s * 1.1), (s * 1.1, -s * 0.1)], width: s * 0.14 }],
        PatternKind::Chevron => vec![Prim::Poly { pts: vec![(-s * 0.01, s * 0.55), (s * 0.5, s * 0.25), (s * 1.01, s * 0.55)], width: s * 0.09 }],
        PatternKind::Waves => {
            let pts = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    (t * s, s * 0.5 + (t * std::f32::consts::TAU).sin() * s * 0.14)
                })
                .collect();
            vec![Prim::Poly { pts, width: s * 0.07 }]
        }
        PatternKind::Confetti => {
            // Fixed pseudo-random layout so the pattern is stable across exports.
            let pts = [(0.12, 0.2, 0.05), (0.62, 0.12, 0.035), (0.4, 0.55, 0.045), (0.85, 0.7, 0.03), (0.2, 0.82, 0.03)];
            pts.iter().map(|&(x, y, r)| Prim::Circle { cx: x * s, cy: y * s, r: r * s }).collect()
        }
    };
    (s, prims)
}

fn seg_dist2(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (a.0 + t * dx - px, a.1 + t * dy - py);
    qx * qx + qy * qy
}

/// Is tile-space point (x, y) (mm, already wrapped into 0..s) covered?
pub fn covered(prims: &[Prim], s: f32, x: f32, y: f32) -> bool {
    for dx in [-s, 0.0, s] {
        for dy in [-s, 0.0, s] {
            let (px, py) = (x + dx, y + dy);
            for p in prims {
                let hit = match p {
                    Prim::Circle { cx, cy, r } => (px - cx).powi(2) + (py - cy).powi(2) <= r * r,
                    Prim::Rect { x, y, w, h } => px >= *x && px < x + w && py >= *y && py < y + h,
                    Prim::Poly { pts, width } => pts.windows(2).any(|w| seg_dist2(px, py, w[0], w[1]) <= (width / 2.0).powi(2)),
                };
                if hit {
                    return true;
                }
            }
        }
    }
    false
}

/// Rasterize one tile with 4×4 supersampling. Returns coverage 0..255, `n × n` pixels.
pub fn tile_mask(p: &Pattern, px_per_mm: f32) -> (usize, Vec<u8>) {
    let (s, prims) = tile(p);
    let n = ((s * px_per_mm).round() as usize).clamp(2, 4096);
    let step = s / n as f32;
    let mut out = vec![0u8; n * n];
    for j in 0..n {
        for i in 0..n {
            let mut c = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (i as f32 + (sx as f32 + 0.5) / 4.0) * step;
                    let y = (j as f32 + (sy as f32 + 0.5) / 4.0) * step;
                    if covered(&prims, s, x, y) {
                        c += 1;
                    }
                }
            }
            out[j * n + i] = (c * 255 / 16) as u8;
        }
    }
    (n, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_has_ink_and_gaps() {
        for k in PatternKind::ALL {
            let (n, m) = tile_mask(&Pattern::new(k), 8.0);
            let ink = m.iter().filter(|&&v| v > 128).count();
            assert!(ink > 0 && ink < n * n, "{k:?} ink {ink}");
        }
    }
}
