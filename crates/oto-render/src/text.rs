//! Text layout shared by screen, raster export and PDF (one engine ⇒ what you see is what prints).
//! Shaping uses rustybuzz (kerning from GPOS/kern), ligatures off so every glyph maps to text.

use oto_core::model::{Align, TextStyle};

use crate::fonts::{self, FontFace};

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedGlyph {
    pub gid: u16,
    /// Pen position from the box's left edge, in points.
    pub x: f32,
    pub y_offset: f32,
    pub advance: f32,
    /// Source text of this glyph (for ToUnicode); empty for continuation glyphs.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub glyphs: Vec<PlacedGlyph>,
    pub width: f32,
    /// Baseline from the box top, in points.
    pub baseline: f32,
}

#[derive(Debug, Clone)]
pub struct TextLayout {
    pub face_index: usize,
    pub size: f32,
    pub lines: Vec<Line>,
    pub height: f32,
    pub overflow: bool,
    pub units_per_em: f32,
}

impl TextLayout {
    pub fn face(&self) -> &'static FontFace {
        &fonts::FACES[self.face_index]
    }
}

struct Shaped {
    gid: u16,
    adv: f32,
    x_off: f32,
    y_off: f32,
    text: String,
    is_space: bool,
}

fn shape_paragraph(face: &rustybuzz::Face, s: &str, size: f32, upem: f32, style: &TextStyle) -> Vec<Shaped> {
    let mut buf = rustybuzz::UnicodeBuffer::new();
    buf.push_str(s);
    let tag = |t: &[u8; 4]| rustybuzz::ttf_parser::Tag::from_bytes(t);
    let feats = [rustybuzz::Feature::new(tag(b"liga"), 0, ..), rustybuzz::Feature::new(tag(b"clig"), 0, ..), rustybuzz::Feature::new(tag(b"kern"), if style.kerning { 1 } else { 0 }, ..)];
    let out = rustybuzz::shape(face, &feats, buf);
    let infos = out.glyph_infos();
    let pos = out.glyph_positions();
    let scale = size / upem;
    let track = style.letter_spacing / 1000.0 * size;
    let mut clusters: Vec<usize> = infos.iter().map(|i| i.cluster as usize).collect();
    clusters.push(s.len());
    let mut sorted: Vec<usize> = clusters.clone();
    sorted.sort_unstable();
    sorted.dedup();
    let mut seen = std::collections::HashSet::new();
    infos
        .iter()
        .zip(pos.iter())
        .map(|(i, p)| {
            let c = i.cluster as usize;
            let next = sorted.iter().copied().find(|&x| x > c).unwrap_or(s.len());
            let text = if seen.insert(c) { s.get(c..next).unwrap_or("").to_string() } else { String::new() };
            let is_space = text.chars().all(char::is_whitespace) && !text.is_empty();
            Shaped { gid: i.glyph_id as u16, adv: p.x_advance as f32 * scale + track, x_off: p.x_offset as f32 * scale, y_off: p.y_offset as f32 * scale, text, is_space }
        })
        .collect()
}

/// Lay out `text` in a box `box_w × box_h` points.
pub fn layout(text: &str, style: &TextStyle, box_w: f32, box_h: f32) -> TextLayout {
    let (face_index, ff) = fonts::face(&style.font_family, style.bold, style.italic);
    let size = style.size_pt.max(1.0);
    let Some(face) = rustybuzz::Face::from_slice(ff.data, 0) else {
        return TextLayout { face_index, size, lines: vec![], height: 0.0, overflow: !text.is_empty(), units_per_em: 1000.0 };
    };
    let upem = face.units_per_em() as f32;
    let asc = face.ascender() as f32 / upem * size;
    let desc = -(face.descender() as f32) / upem * size;
    let line_h = size * style.line_height.max(0.5);
    let mut raw_lines: Vec<Vec<Shaped>> = Vec::new();
    for para in text.split('\n') {
        let glyphs = shape_paragraph(&face, para, size, upem, style);
        if glyphs.is_empty() {
            raw_lines.push(Vec::new());
            continue;
        }
        let mut line: Vec<Shaped> = Vec::new();
        let mut width = 0.0f32;
        let mut last_break: Option<usize> = None;
        for g in glyphs {
            let adv = g.adv;
            if width + adv > box_w && !line.is_empty() && !g.is_space {
                if let Some(b) = last_break {
                    let rest: Vec<Shaped> = line.drain(b + 1..).collect();
                    raw_lines.push(std::mem::take(&mut line));
                    line = rest;
                } else {
                    raw_lines.push(std::mem::take(&mut line));
                }
                width = line.iter().map(|g| g.adv).sum();
                last_break = None;
            }
            if g.is_space {
                last_break = Some(line.len());
            }
            width += adv;
            line.push(g);
        }
        raw_lines.push(line);
    }
    let mut lines = Vec::with_capacity(raw_lines.len());
    for (i, mut l) in raw_lines.into_iter().enumerate() {
        while l.last().map(|g| g.is_space).unwrap_or(false) {
            l.pop();
        }
        while l.first().map(|g| g.is_space).unwrap_or(false) && i > 0 {
            l.remove(0);
        }
        let track = style.letter_spacing / 1000.0 * size;
        let width: f32 = l.iter().map(|g| g.adv).sum::<f32>() - if l.is_empty() { 0.0 } else { track };
        let x0 = match style.align {
            Align::Left => 0.0,
            Align::Center => (box_w - width) / 2.0,
            Align::Right => box_w - width,
        };
        let top = i as f32 * line_h;
        let baseline = top + (line_h - (asc + desc)) / 2.0 + asc;
        let mut x = x0;
        let glyphs = l
            .into_iter()
            .map(|g| {
                let pg = PlacedGlyph { gid: g.gid, x: x + g.x_off, y_offset: g.y_off, advance: g.adv, text: g.text };
                x += g.adv;
                pg
            })
            .collect();
        lines.push(Line { glyphs, width, baseline });
    }
    let height = lines.len() as f32 * line_h;
    let widest = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    TextLayout { face_index, size, overflow: height > box_h + 0.5 || widest > box_w + 0.5, lines, height, units_per_em: upem }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_aligns() {
        let style = TextStyle { font_family: "Lora".into(), size_pt: 12.0, align: Align::Center, ..Default::default() };
        let l = layout("The quick brown fox jumps over the lazy dog", &style, 100.0, 200.0);
        assert!(l.lines.len() >= 3, "{}", l.lines.len());
        assert!(!l.overflow);
        for line in &l.lines {
            assert!(line.width <= 100.5);
            let first = line.glyphs.first().unwrap();
            assert!(first.x > 0.0, "centred lines start inside the box");
        }
        let text: String = l.lines.iter().flat_map(|l| l.glyphs.iter().map(|g| g.text.clone())).collect();
        assert_eq!(text.replace(' ', ""), "Thequickbrownfoxjumpsoverthelazydog");
    }

    #[test]
    fn overflow_and_fallback() {
        let style = TextStyle { font_family: "No Such Font".into(), size_pt: 30.0, ..Default::default() };
        let l = layout("Overflowing\nlines\nhere", &style, 300.0, 20.0);
        assert!(l.overflow);
        assert_eq!(l.face().family, "Inter");
    }

    #[test]
    fn kerning_changes_advances() {
        let mut style = TextStyle { font_family: "Playfair Display".into(), size_pt: 40.0, ..Default::default() };
        let a = layout("AVATAR", &style, 1000.0, 100.0).lines[0].width;
        style.kerning = false;
        let b = layout("AVATAR", &style, 1000.0, 100.0).lines[0].width;
        assert!(a < b, "kerned {a} should be tighter than {b}");
    }
}
