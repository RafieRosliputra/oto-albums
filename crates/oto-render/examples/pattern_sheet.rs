//! SVG swatch sheet of the formal pattern collection in the pastel inks (visual review).
use oto_core::model::{Pattern, PatternKind, PASTELS};
use oto_render::pattern::{tile, Prim};
fn main() {
    let (sw, cols) = (180.0f32, 6usize);
    let kinds = PatternKind::FORMAL;
    let rows = kinds.len().div_ceil(cols);
    let mut s = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' style='background:#eee;font:11px sans-serif'>", cols as f32 * (sw + 14.0), rows as f32 * (sw + 30.0));
    for (i, k) in kinds.iter().enumerate() {
        let mut p = Pattern::new(*k);
        p.color = PASTELS[i % PASTELS.len()].1;
        let (ts, prims) = tile(&p);
        let px = 3.0; // px per mm
        let (x0, y0) = ((i % cols) as f32 * (sw + 14.0) + 7.0, (i / cols) as f32 * (sw + 30.0) + 7.0);
        let c = format!("rgb({},{},{})", p.color.r, p.color.g, p.color.b);
        s += &format!("<svg x='{x0}' y='{y0}' width='{sw}' height='{sw}'><rect width='{sw}' height='{sw}' fill='white'/><g fill='{c}' stroke='{c}' opacity='{}'>", p.opacity);
        let n = (sw / (ts * px)).ceil() as i32 + 1;
        for ty in -1..n {
            for tx in -1..n {
                let (ox, oy) = (tx as f32 * ts * px, ty as f32 * ts * px);
                for pr in &prims {
                    match pr {
                        Prim::Circle { cx, cy, r } => s += &format!("<circle cx='{}' cy='{}' r='{}' stroke='none'/>", ox + cx * px, oy + cy * px, r * px),
                        Prim::Rect { x, y, w, h } => s += &format!("<rect x='{}' y='{}' width='{}' height='{}' stroke='none'/>", ox + x * px, oy + y * px, w * px, h * px),
                        Prim::Poly { pts, width } => {
                            let d: Vec<String> = pts.iter().map(|(x, y)| format!("{},{}", ox + x * px, oy + y * px)).collect();
                            s += &format!("<polyline points='{}' fill='none' stroke-width='{}'/>", d.join(" "), width * px);
                        }
                    }
                }
            }
        }
        s += &format!("</g></svg><text x='{x0}' y='{}'>{} · {}</text>", y0 + sw + 14.0, k.label(), PASTELS[i % PASTELS.len()].0);
    }
    println!("{s}</svg>");
}
