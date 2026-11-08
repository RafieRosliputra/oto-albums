//! Structural PDF validation (independent parser: lopdf). This is *not* a PDF/X certification;
//! it verifies what oto-albums promises: page count, boxes, embedded fonts, images and ICC.

use std::path::Path;

use lopdf::{Dictionary, Document, Object};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PdfCheck {
    pub ok: bool,
    pub version: String,
    pub pages: usize,
    pub fonts: usize,
    pub images: usize,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Expect {
    pub pages: usize,
    /// Expected TrimBox size per page (points).
    pub trim_pt: Vec<(f32, f32)>,
    pub pdfx4: bool,
}

/// 0.1 mm in points.
const TOL_PT: f32 = 0.2835;

fn rect(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<[f32; 4]> {
    let o = d.get(key).ok()?;
    let (_, o) = doc.dereference(o).ok()?;
    let a = o.as_array().ok()?;
    if a.len() != 4 {
        return None;
    }
    let v: Vec<f32> = a.iter().filter_map(|x| x.as_float().ok()).collect();
    if v.len() == 4 {
        Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
    } else {
        None
    }
}

fn deref_dict<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    doc.dereference(o).ok()?.1.as_dict().ok()
}

fn inside(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] >= b[0] - 0.01 && a[1] >= b[1] - 0.01 && a[2] <= b[2] + 0.01 && a[3] <= b[3] + 0.01
}

pub fn check(path: &Path, exp: &Expect) -> PdfCheck {
    let mut c = PdfCheck::default();
    let doc = match Document::load(path) {
        Ok(d) => d,
        Err(e) => {
            c.problems.push(format!("cannot parse PDF: {e}"));
            return c;
        }
    };
    c.version = doc.version.clone();
    let pages = doc.get_pages();
    c.pages = pages.len();
    if c.pages != exp.pages {
        c.problems.push(format!("expected {} pages, found {}", exp.pages, c.pages));
    }
    let mut seen_fonts = std::collections::HashSet::new();
    let mut seen_imgs = std::collections::HashSet::new();
    for (n, (_, id)) in pages.iter().enumerate() {
        let Some(page) = doc.get_object(*id).ok().and_then(|o| o.as_dict().ok()) else {
            c.problems.push(format!("page {}: unreadable", n + 1));
            continue;
        };
        let media = rect(&doc, page, b"MediaBox");
        let trim = rect(&doc, page, b"TrimBox");
        let bleed = rect(&doc, page, b"BleedBox");
        match (media, trim, bleed) {
            (Some(m), Some(t), Some(b)) => {
                if !inside(&t, &b) || !inside(&b, &m) {
                    c.problems.push(format!("page {}: TrimBox ⊄ BleedBox ⊄ MediaBox", n + 1));
                }
                if let Some(&(w, h)) = exp.trim_pt.get(n).or(exp.trim_pt.last()) {
                    if ((t[2] - t[0]) - w).abs() > TOL_PT || ((t[3] - t[1]) - h).abs() > TOL_PT {
                        c.problems.push(format!("page {}: TrimBox {:.2}×{:.2} pt, expected {:.2}×{:.2}", n + 1, t[2] - t[0], t[3] - t[1], w, h));
                    }
                }
            }
            _ => c.problems.push(format!("page {}: missing MediaBox/TrimBox/BleedBox", n + 1)),
        }
        let Some(res) = page.get(b"Resources").ok().and_then(|o| deref_dict(&doc, o)) else {
            continue;
        };
        if let Some(fonts) = res.get(b"Font").ok().and_then(|o| deref_dict(&doc, o)) {
            for (_, fo) in fonts.iter() {
                let key = fo.as_reference().ok();
                let Some(f) = deref_dict(&doc, fo) else { continue };
                if key.map(|k| !seen_fonts.insert(k)).unwrap_or(false) {
                    continue;
                }
                let desc = f
                    .get(b"DescendantFonts")
                    .ok()
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_array().ok())
                    .and_then(|a| a.first())
                    .and_then(|o| deref_dict(&doc, o))
                    .and_then(|cid| cid.get(b"FontDescriptor").ok())
                    .and_then(|o| deref_dict(&doc, o));
                let embedded = desc.map(|d| d.has(b"FontFile2") || d.has(b"FontFile3") || d.has(b"FontFile")).unwrap_or(false);
                if !embedded {
                    c.problems.push(format!("page {}: font not embedded", n + 1));
                }
                if !f.has(b"ToUnicode") {
                    c.problems.push(format!("page {}: font without ToUnicode", n + 1));
                }
            }
        }
        if let Some(xo) = res.get(b"XObject").ok().and_then(|o| deref_dict(&doc, o)) {
            for (name, o) in xo.iter() {
                let id = o.as_reference().ok();
                match doc.dereference(o).ok().and_then(|(_, o)| o.as_stream().ok()) {
                    Some(s) => {
                        let is_img = s.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Image".as_slice());
                        if is_img {
                            if let Some(id) = id {
                                seen_imgs.insert(id);
                            }
                        }
                    }
                    None => c.problems.push(format!("page {}: XObject {} missing", n + 1, String::from_utf8_lossy(name))),
                }
            }
        }
    }
    c.fonts = seen_fonts.len();
    c.images = seen_imgs.len();
    // Output intent + ICC.
    let catalog = doc.catalog().ok();
    let icc_ok = catalog
        .and_then(|cat| cat.get(b"OutputIntents").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .and_then(|a| a.first())
        .and_then(|o| deref_dict(&doc, o))
        .and_then(|oi| oi.get(b"DestOutputProfile").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_stream().ok())
        .map(|s| s.dict.get(b"N").ok().and_then(|n| n.as_i64().ok()) == Some(3))
        .unwrap_or(false);
    if !icc_ok {
        c.problems.push("no OutputIntent with an RGB ICC profile".into());
    }
    if exp.pdfx4 {
        if c.version != "1.6" {
            c.problems.push(format!("PDF/X-4 needs PDF 1.6, file is {}", c.version));
        }
        let meta =
            catalog.and_then(|cat| cat.get(b"Metadata").ok()).and_then(|o| doc.dereference(o).ok()).and_then(|(_, o)| o.as_stream().ok()).map(|s| String::from_utf8_lossy(&s.content).to_string());
        if !meta.map(|m| m.contains("<pdfxid:GTS_PDFXVersion>PDF/X-4<")).unwrap_or(false) {
            c.problems.push("XMP metadata lacks pdfxid:GTS_PDFXVersion = PDF/X-4".into());
        }
        let trailer_info = doc.trailer.get(b"Info").ok().and_then(|o| deref_dict(&doc, o));
        if !trailer_info.map(|i| i.has(b"Trapped")).unwrap_or(false) {
            c.problems.push("Info dictionary lacks /Trapped".into());
        }
        if doc.trailer.get(b"ID").is_err() {
            c.problems.push("trailer lacks /ID".into());
        }
    }
    c.ok = c.problems.is_empty();
    c
}
