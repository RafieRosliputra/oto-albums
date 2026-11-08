//! Prints how many base "clean" layouts survive per photo count on three album sizes.
use oto_core::clean::clean_templates;
use oto_core::spec::SpreadGeometry;
fn mm(v: f64) -> i64 { (v * 1000.0).round() as i64 }
fn main() {
    let g = |w: f64, h: f64| SpreadGeometry { trim_w: mm(2.0 * w), trim_h: mm(h), page_w: mm(w), bleed: mm(3.0), safe: mm(5.0), trim_outer: mm(3.0), trim_inside: 0, gutter: mm(10.0) };
    for (name, s) in [("10x10", g(254.0, 254.0)), ("12x8", g(304.8, 203.2)), ("A4P", g(210.0, 297.0))] {
        let mut n = [0usize; 13];
        for t in clean_templates(&s, 3000, 0.10) { if !t.id.ends_with("-m") { n[t.photo_count as usize] += 1; } }
        println!("{name}: {:?}", &n[1..]);
    }
}
