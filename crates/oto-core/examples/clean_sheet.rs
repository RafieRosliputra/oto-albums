//! Contact sheet of the generated "clean" templates (SVG) for visual review.
use oto_core::clean::clean_templates;
use oto_core::spec::AlbumSpec;
fn main() {
    let g = AlbumSpec::default().geometry();
    let ts: Vec<_> = clean_templates(&g, 3000, 0.10).into_iter().filter(|t| !t.id.ends_with("-m")).collect();
    let (cw, ch, cols) = (300.0, 150.0, 6usize);
    let rows = ts.len().div_ceil(cols);
    let mut s = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' style='background:#ddd;font:10px sans-serif'>", cols as f64 * (cw + 20.0), rows as f64 * (ch + 34.0));
    for (i, t) in ts.iter().enumerate() {
        let (x0, y0) = ((i % cols) as f64 * (cw + 20.0) + 10.0, (i / cols) as f64 * (ch + 34.0) + 10.0);
        s += &format!("<rect x='{x0}' y='{y0}' width='{cw}' height='{ch}' fill='white'/><line x1='{}' y1='{y0}' x2='{}' y2='{}' stroke='#bbb'/>", x0 + cw / 2.0, x0 + cw / 2.0, y0 + ch);
        for f in &t.frames {
            s += &format!("<rect x='{}' y='{}' width='{}' height='{}' fill='#7a8fa6'/>", x0 + f.x as f64 * cw, y0 + f.y as f64 * ch, f.w as f64 * cw, f.h as f64 * ch);
        }
        s += &format!("<text x='{x0}' y='{}'>{} · {}</text>", y0 + ch + 13.0, t.id, t.name.replace('&', "and"));
    }
    s += "</svg>";
    println!("{s}");
}
