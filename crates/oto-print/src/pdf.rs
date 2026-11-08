//! Low-level PDF builder on top of `pdf-writer`: subset CID fonts with ToUnicode, ICC-based
//! images (JPEG pass-through or Flate), output intent, XMP and page boxes.

use std::collections::{BTreeMap, HashMap};

use pdf_writer::types::{CidFontType, FontFlags, NumberingStyle, OutputIntentSubtype, SystemInfo, TrappingStatus, UnicodeCmap};
use pdf_writer::{Content, Date, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

use oto_render::fonts::{self, SRGB_ICC};
use oto_render::text::TextLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Standard {
    /// PDF/X-4 structure (PDF 1.6, output intent, XMP identification).
    #[default]
    PdfX4,
    /// Plain PDF 1.7.
    Pdf17,
}

impl Standard {
    pub fn label(self) -> &'static str {
        match self {
            Standard::PdfX4 => "PDF/X-4",
            Standard::Pdf17 => "PDF 1.7",
        }
    }
}

pub struct DocInfo {
    pub title: String,
    pub author: String,
    pub standard: Standard,
    pub output_condition: String,
    /// Custom output ICC (bytes); `None` = built-in sRGB.
    pub icc: Option<Vec<u8>>,
    pub id_seed: String,
}

struct FontUse {
    face_index: usize,
    font_ref: Ref,
    remapper: subsetter::GlyphRemapper,
    /// new gid → text
    texts: BTreeMap<u16, String>,
    /// new gid → old gid
    olds: BTreeMap<u16, u16>,
}

/// Image handed to the builder.
pub enum ImageData {
    /// Raw baseline/progressive JPEG with 3 components (embedded unchanged).
    Jpeg { bytes: Vec<u8>, w: u32, h: u32 },
    /// 8-bit RGB pixels plus optional 8-bit alpha. `jpeg_quality` re-encodes as JPEG.
    Rgb { rgb: Vec<u8>, alpha: Option<Vec<u8>>, w: u32, h: u32, jpeg_quality: Option<u8> },
}

pub struct PageBoxes {
    /// Media box size in points (origin 0,0).
    pub media: (f32, f32),
    pub trim: [f32; 4],
    pub bleed: [f32; 4],
    pub label: Option<String>,
}

pub struct PdfBuilder {
    pdf: Pdf,
    next: i32,
    catalog: Ref,
    pages: Ref,
    page_refs: Vec<Ref>,
    labels: Vec<String>,
    icc: Ref,
    fonts: Vec<FontUse>,
    images: HashMap<String, Ref>,
    gstates: HashMap<u16, Ref>,
    pub info: DocInfo,
    pub image_count: usize,
}

/// Resources used by one page's content.
#[derive(Default)]
pub struct PageRes {
    xobjects: BTreeMap<String, Ref>,
    fonts: BTreeMap<String, Ref>,
    gstates: BTreeMap<String, Ref>,
    pub transparency: bool,
}

impl PdfBuilder {
    pub fn new(info: DocInfo) -> Self {
        let mut pdf = Pdf::new();
        if info.standard == Standard::PdfX4 {
            pdf.set_version(1, 6);
        }
        let mut b = PdfBuilder {
            pdf,
            next: 1,
            catalog: Ref::new(1),
            pages: Ref::new(2),
            page_refs: Vec::new(),
            labels: Vec::new(),
            icc: Ref::new(3),
            fonts: Vec::new(),
            images: HashMap::new(),
            gstates: HashMap::new(),
            info,
            image_count: 0,
        };
        b.next = 4;
        let icc_bytes = b.info.icc.clone().unwrap_or_else(|| SRGB_ICC.to_vec());
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&icc_bytes, 6);
        b.pdf.icc_profile(b.icc, &compressed).n(3).range([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]).filter(Filter::FlateDecode);
        b
    }

    pub fn alloc(&mut self) -> Ref {
        let r = Ref::new(self.next);
        self.next += 1;
        r
    }

    fn font_slot(&mut self, face_index: usize) -> usize {
        if let Some(i) = self.fonts.iter().position(|f| f.face_index == face_index) {
            return i;
        }
        let font_ref = self.alloc();
        self.fonts.push(FontUse { face_index, font_ref, remapper: subsetter::GlyphRemapper::new(), texts: BTreeMap::new(), olds: BTreeMap::new() });
        self.fonts.len() - 1
    }

    /// Opacity graphics state (alpha in 1/1000 steps).
    pub fn gstate(&mut self, res: &mut PageRes, alpha: f32) -> String {
        let key = (alpha.clamp(0.0, 1.0) * 1000.0).round() as u16;
        let r = match self.gstates.get(&key) {
            Some(r) => *r,
            None => {
                let r = self.alloc();
                let a = key as f32 / 1000.0;
                self.pdf.ext_graphics(r).non_stroking_alpha(a).stroking_alpha(a);
                self.gstates.insert(key, r);
                r
            }
        };
        let name = format!("G{key}");
        res.gstates.insert(name.clone(), r);
        if key < 1000 {
            res.transparency = true;
        }
        name
    }

    /// Embed an image once per `key`; returns the resource name.
    pub fn image(&mut self, res: &mut PageRes, key: &str, data: impl FnOnce() -> Option<ImageData>) -> Option<String> {
        let r = match self.images.get(key) {
            Some(r) => *r,
            None => {
                let img = data()?;
                let r = self.alloc();
                match img {
                    ImageData::Jpeg { bytes, w, h } => {
                        let mut x = self.pdf.image_xobject(r, &bytes);
                        x.width(w as i32).height(h as i32).bits_per_component(8);
                        x.filter(Filter::DctDecode);
                        x.color_space().icc_based(self.icc);
                    }
                    ImageData::Rgb { rgb, alpha, w, h, jpeg_quality } => {
                        let smask = match alpha {
                            Some(a) if a.iter().any(|&v| v < 255) => {
                                let sr = self.alloc();
                                let z = miniz_oxide::deflate::compress_to_vec_zlib(&a, 6);
                                let mut m = self.pdf.image_xobject(sr, &z);
                                m.width(w as i32).height(h as i32).bits_per_component(8);
                                m.filter(Filter::FlateDecode);
                                m.color_space().device_gray();
                                res.transparency = true;
                                Some(sr)
                            }
                            _ => None,
                        };
                        let (bytes, filter) = match jpeg_quality {
                            Some(q) => {
                                let mut out = Vec::new();
                                let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q);
                                let ok = image::ImageEncoder::write_image(enc, &rgb, w, h, image::ExtendedColorType::Rgb8).is_ok();
                                if ok {
                                    (out, Filter::DctDecode)
                                } else {
                                    (miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6), Filter::FlateDecode)
                                }
                            }
                            None => (miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6), Filter::FlateDecode),
                        };
                        let mut x = self.pdf.image_xobject(r, &bytes);
                        x.width(w as i32).height(h as i32).bits_per_component(8);
                        x.filter(filter);
                        if let Some(sr) = smask {
                            x.s_mask(sr);
                        }
                        x.color_space().icc_based(self.icc);
                    }
                }
                self.images.insert(key.to_string(), r);
                self.image_count += 1;
                r
            }
        };
        let name = format!("I{}", r.get());
        res.xobjects.insert(name.clone(), r);
        Some(name)
    }

    /// Draw laid-out text with its top-left at (x, y_top) in points (PDF coordinates, y up).
    pub fn text(&mut self, c: &mut Content, res: &mut PageRes, lay: &TextLayout, x: f32, y_top: f32, rgb: [f32; 3]) {
        let slot = self.font_slot(lay.face_index);
        let name = format!("F{}", lay.face_index);
        res.fonts.insert(name.clone(), self.fonts[slot].font_ref);
        c.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
        c.begin_text();
        c.set_font(Name(name.as_bytes()), lay.size);
        for line in &lay.lines {
            for g in &line.glyphs {
                let fu = &mut self.fonts[slot];
                let new = fu.remapper.remap(g.gid);
                fu.olds.insert(new, g.gid);
                if !g.text.is_empty() {
                    fu.texts.entry(new).or_insert_with(|| g.text.clone());
                }
                c.set_text_matrix([1.0, 0.0, 0.0, 1.0, x + g.x, y_top - line.baseline + g.y_offset]);
                c.show(Str(&new.to_be_bytes()));
            }
        }
        c.end_text();
    }

    pub fn add_page(&mut self, content: Content, res: PageRes, boxes: PageBoxes) {
        let page = self.alloc();
        let content_ref = self.alloc();
        let data = content.finish();
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&data, 6);
        self.pdf.stream(content_ref, &z).filter(Filter::FlateDecode);
        let mut p = self.pdf.page(page);
        p.parent(self.pages);
        p.media_box(Rect::new(0.0, 0.0, boxes.media.0, boxes.media.1));
        p.bleed_box(Rect::new(boxes.bleed[0], boxes.bleed[1], boxes.bleed[2], boxes.bleed[3]));
        p.trim_box(Rect::new(boxes.trim[0], boxes.trim[1], boxes.trim[2], boxes.trim[3]));
        p.contents(content_ref);
        if res.transparency {
            p.group().transparency().isolated(false).knockout(false).color_space().icc_based(self.icc);
        }
        let mut r = p.resources();
        {
            let mut x = r.x_objects();
            for (k, v) in &res.xobjects {
                x.pair(Name(k.as_bytes()), *v);
            }
        }
        {
            let mut f = r.fonts();
            for (k, v) in &res.fonts {
                f.pair(Name(k.as_bytes()), *v);
            }
        }
        {
            let mut g = r.ext_g_states();
            for (k, v) in &res.gstates {
                g.pair(Name(k.as_bytes()), *v);
            }
        }
        r.finish();
        p.finish();
        self.page_refs.push(page);
        self.labels.push(boxes.label.unwrap_or_default());
    }

    pub fn page_count(&self) -> usize {
        self.page_refs.len()
    }

    fn write_fonts(&mut self) -> Result<(), String> {
        let fonts = std::mem::take(&mut self.fonts);
        for fu in &fonts {
            let face = &fonts::FACES[fu.face_index];
            let ttf = ttf_parser::Face::parse(face.data, 0).map_err(|e| format!("font {}: {e}", face.postscript))?;
            let sub = subsetter::subset(face.data, 0, &fu.remapper).map_err(|e| format!("font subset {}: {e:?}", face.postscript))?;
            let upem = ttf.units_per_em() as f32;
            let s = |v: f32| v / upem * 1000.0;
            // Subset tag: six capital letters derived from the glyph set.
            let mut h: u64 = 1469598103934665603;
            for (n, o) in &fu.olds {
                h = (h ^ (*n as u64 * 65599 + *o as u64)).wrapping_mul(1099511628211);
            }
            let tag: String = (0..6).map(|i| (b'A' + ((h >> (i * 5)) % 26) as u8) as char).collect();
            let base = format!("{tag}+{}", face.postscript);
            let (cid_ref, desc_ref, file_ref, cmap_ref) = (self.alloc(), self.alloc(), self.alloc(), self.alloc());
            self.pdf.type0_font(fu.font_ref).base_font(Name(base.as_bytes())).encoding_predefined(Name(b"Identity-H")).descendant_font(cid_ref).to_unicode(cmap_ref);
            let sys = SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };
            let max_new = fu.olds.keys().copied().max().unwrap_or(0);
            let widths: Vec<f32> = (0..=max_new)
                .map(|n| fu.olds.get(&n).or(if n == 0 { Some(&0) } else { None }).and_then(|o| ttf.glyph_hor_advance(ttf_parser::GlyphId(*o))).map(|a| s(a as f32)).unwrap_or(0.0))
                .collect();
            {
                let mut cid = self.pdf.cid_font(cid_ref);
                cid.subtype(CidFontType::Type2).base_font(Name(base.as_bytes())).system_info(sys).font_descriptor(desc_ref).default_width(0.0).cid_to_gid_map_predefined(Name(b"Identity"));
                cid.widths().consecutive(0, widths);
            }
            let bbox = ttf.global_bounding_box();
            let mut flags = FontFlags::NON_SYMBOLIC;
            if face.italic {
                flags |= FontFlags::ITALIC;
            }
            if face.family == "Great Vibes" {
                flags |= FontFlags::SCRIPT;
            }
            if ["Lora", "Playfair Display", "Cormorant Garamond"].contains(&face.family) {
                flags |= FontFlags::SERIF;
            }
            self.pdf
                .font_descriptor(desc_ref)
                .name(Name(base.as_bytes()))
                .flags(flags)
                .bbox(Rect::new(s(bbox.x_min as f32), s(bbox.y_min as f32), s(bbox.x_max as f32), s(bbox.y_max as f32)))
                .italic_angle(ttf.italic_angle())
                .ascent(s(ttf.ascender() as f32))
                .descent(s(ttf.descender() as f32))
                .cap_height(s(ttf.capital_height().unwrap_or(ttf.ascender()) as f32))
                .stem_v(if face.bold { 120.0 } else { 80.0 })
                .font_file2(file_ref);
            let z = miniz_oxide::deflate::compress_to_vec_zlib(&sub, 6);
            self.pdf.stream(file_ref, &z).filter(Filter::FlateDecode).pair(Name(b"Length1"), sub.len() as i32);
            let mut cmap = UnicodeCmap::new(Name(b"Custom"), sys);
            for (gid, t) in &fu.texts {
                let cs: Vec<char> = t.chars().collect();
                if cs.len() == 1 {
                    cmap.pair(*gid, cs[0]);
                } else if !cs.is_empty() {
                    cmap.pair_with_multiple(*gid, cs);
                }
            }
            let data = cmap.finish();
            self.pdf.cmap(cmap_ref, &data);
        }
        Ok(())
    }

    /// Finish the document and return the bytes.
    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        self.write_fonts()?;
        let now = time::OffsetDateTime::now_utc();
        let date = Date::new(now.year() as u16).month(now.month() as u8).day(now.day()).hour(now.hour()).minute(now.minute()).second(now.second()).utc_offset_hour(0);
        let iso = now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
        let x4 = self.info.standard == Standard::PdfX4;
        let info_ref = self.alloc();
        {
            let mut di = self.pdf.document_info(info_ref);
            di.title(TextStr(&self.info.title)).author(TextStr(&self.info.author)).creator(TextStr("oto-albums")).producer(TextStr(concat!("oto-albums ", env!("CARGO_PKG_VERSION"))));
            di.creation_date(date).modified_date(date).trapped(TrappingStatus::NotTrapped);
            if x4 {
                di.pair(Name(b"GTS_PDFXVersion"), TextStr("PDF/X-4"));
            }
        }
        let id_hash = blake3_like(&self.info.id_seed);
        self.pdf.set_file_id((id_hash.clone(), id_hash.clone()));
        let doc_id = format!("uuid:{}", uuid::Uuid::from_slice(&id_hash[..16]).unwrap_or_default());
        let inst_id = format!("uuid:{}", uuid::Uuid::now_v7());
        let xmp = xmp_packet(&self.info.title, &self.info.author, &iso, x4, &doc_id, &inst_id);
        let meta_ref = self.alloc();
        self.pdf.metadata(meta_ref, xmp.as_bytes());
        let count = self.page_refs.len() as i32;
        self.pdf.pages(self.pages).kids(self.page_refs.iter().copied()).count(count);
        let labels: Vec<(i32, String)> = self.labels.iter().enumerate().filter(|(_, l)| !l.is_empty()).map(|(i, l)| (i as i32, l.clone())).collect();
        let label_refs: Vec<(i32, Ref, String)> = labels.into_iter().map(|(i, l)| (i, self.alloc(), l)).collect();
        for (_, r, l) in &label_refs {
            let mut pl = self.pdf.indirect(*r).start::<pdf_writer::writers::PageLabel>();
            pl.prefix(TextStr(l)).style(NumberingStyle::Arabic).offset(1);
        }
        {
            let mut cat = self.pdf.catalog(self.catalog);
            cat.pages(self.pages).metadata(meta_ref);
            if !label_refs.is_empty() {
                let mut tree = cat.page_labels();
                let mut nums = tree.nums();
                for (i, r, _) in &label_refs {
                    nums.insert(*i, *r);
                }
            }
            let mut intents = cat.output_intents();
            let mut oi = intents.push();
            oi.subtype(OutputIntentSubtype::PDFX);
            oi.output_condition_identifier(TextStr(&self.info.output_condition))
                .info(TextStr(&self.info.output_condition))
                .registry_name(TextStr("http://www.color.org"))
                .dest_output_profile(self.icc);
        }
        Ok(self.pdf.finish())
    }
}

fn blake3_like(seed: &str) -> Vec<u8> {
    use sha2::Digest;
    sha2::Sha256::digest(seed.as_bytes()).to_vec()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn xmp_packet(title: &str, author: &str, iso: &str, x4: bool, doc_id: &str, inst_id: &str) -> String {
    let pdfx = if x4 { "\n   <pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion>" } else { "" };
    format!(
        r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
    xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
    xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/">
   <dc:format>application/pdf</dc:format>
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">{t}</rdf:li></rdf:Alt></dc:title>
   <dc:creator><rdf:Seq><rdf:li>{a}</rdf:li></rdf:Seq></dc:creator>
   <xmp:CreateDate>{d}</xmp:CreateDate>
   <xmp:ModifyDate>{d}</xmp:ModifyDate>
   <xmp:MetadataDate>{d}</xmp:MetadataDate>
   <xmp:CreatorTool>oto-albums</xmp:CreatorTool>
   <pdf:Producer>oto-albums {v}</pdf:Producer>
   <pdf:Trapped>False</pdf:Trapped>
   <xmpMM:DocumentID>{doc}</xmpMM:DocumentID>
   <xmpMM:InstanceID>{inst}</xmpMM:InstanceID>
   <xmpMM:VersionID>1</xmpMM:VersionID>
   <xmpMM:RenditionClass>default</xmpMM:RenditionClass>{pdfx}
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#,
        t = esc(title),
        a = esc(author),
        d = iso,
        v = env!("CARGO_PKG_VERSION"),
        doc = doc_id,
        inst = inst_id,
        pdfx = pdfx
    )
}

/// Parse a JPEG's SOF header: (width, height, components, progressive).
pub fn jpeg_info(b: &[u8]) -> Option<(u32, u32, u8)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) || m == 0xFF {
            i += if m == 0xFF { 1 } else { 2 };
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        // SOF0..SOF2 (baseline, extended, progressive) are the ones PDF readers support.
        if matches!(m, 0xC0..=0xC2) {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            let n = b[i + 9];
            return Some((w, h, n));
        }
        if matches!(m, 0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            return None; // lossless/arithmetic: re-encode instead
        }
        i += 2 + len;
    }
    None
}

/// Helper: point conversions in µm → pt.
pub fn pt(um: f64) -> f32 {
    (um * 72.0 / 25_400.0) as f32
}

/// Append a rectangle path with optional rounded corners (PDF coords).
pub fn rrect_path(c: &mut Content, x: f32, y: f32, w: f32, h: f32, r: f32) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.01 {
        c.rect(x, y, w, h);
        return;
    }
    let k = 0.552_284_8 * r;
    c.move_to(x + r, y);
    c.line_to(x + w - r, y);
    c.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    c.line_to(x + w, y + h - r);
    c.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    c.line_to(x + r, y + h);
    c.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    c.line_to(x, y + r);
    c.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    c.close_path();
}

/// Append an ellipse path.
pub fn ellipse_path(c: &mut Content, cx: f32, cy: f32, rx: f32, ry: f32) {
    let (kx, ky) = (0.552_284_8 * rx, 0.552_284_8 * ry);
    c.move_to(cx + rx, cy);
    c.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    c.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    c.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    c.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    c.close_path();
}
