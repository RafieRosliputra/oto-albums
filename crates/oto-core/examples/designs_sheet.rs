//! Coverage table + SVG contact sheet of the designed templates.
//! cargo run -p oto-core --example designs_sheet -- [album|photobook] [page_w_mm] [page_h_mm] > sheet.svg
use oto_core::designs::{designed_templates, AlbumKind, LayoutParams};
use oto_core::spec::SpreadGeometry;
fn mm(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}
fn g(w: f64, h: f64) -> SpreadGeometry {
    SpreadGeometry { trim_w: mm(2.0 * w), trim_h: mm(h), page_w: mm(w), bleed: mm(3.0), safe: mm(5.0), trim_outer: mm(3.0), trim_inside: 0, gutter: mm(10.0) }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 2 {
        for (n, s) in [("10x10", g(254.0, 254.0)), ("12x12", g(304.8, 304.8)), ("12x8", g(304.8, 203.2)), ("A4P", g(210.0, 297.0))] {
            for k in AlbumKind::ALL {
                let mut c = [0usize; 11];
                for t in designed_templates(&s, k, &LayoutParams::default()) {
                    if !t.id.ends_with("-m") && t.photo_count <= 10 {
                        c[t.photo_count as usize] += 1;
                    }
                }
                eprintln!("{n} {:?}: {:?}", k, &c[1..]);
            }
        }
        return;
    }
    let kind = if a[1] == "photobook" { AlbumKind::Photobook } else { AlbumKind::Album };
    let pw: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(254.0);
    let ph: f64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(254.0);
    let ts: Vec<_> = designed_templates(&g(pw, ph), kind, &LayoutParams::default()).into_iter().filter(|t| !t.id.ends_with("-m")).collect();
    let cw = 260.0;
    let ch = cw * ph / (2.0 * pw);
    let cols = 5usize;
    let rows = ts.len().div_ceil(cols);
    let mut s = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' style='background:#d9d9d9;font:9px sans-serif'>", cols as f64 * (cw + 16.0), rows as f64 * (ch + 28.0));
    for (i, t) in ts.iter().enumerate() {
        let (x0, y0) = ((i % cols) as f64 * (cw + 16.0) + 8.0, (i / cols) as f64 * (ch + 28.0) + 8.0);
        s += &format!("<rect x='{x0}' y='{y0}' width='{cw}' height='{ch}' fill='white'/><line x1='{0}' y1='{y0}' x2='{0}' y2='{1}' stroke='#c8c8c8'/>", x0 + cw / 2.0, y0 + ch);
        for f in &t.frames {
            let fill = if f.min_aspect <= 0.31 { "#5d7189" } else { "#8fa2b8" };
            s += &format!("<rect x='{}' y='{}' width='{}' height='{}' fill='{fill}'/>", x0 + f.x as f64 * cw, y0 + f.y as f64 * ch, f.w as f64 * cw, f.h as f64 * ch);
        }
        s += &format!("<text x='{x0}' y='{}'>{} · {} [{}]</text>", y0 + ch + 11.0, t.id, t.name.replace('&', "and"), t.family);
    }
    println!("{s}</svg>");
}
