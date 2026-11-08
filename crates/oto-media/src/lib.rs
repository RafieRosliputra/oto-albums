//! oto-media: folder scan, EXIF, decode, thumbnails/proxies, hashing and image features.
//!
//! Originals are never modified. Everything derived lives in the project's `cache/` and can be
//! rebuilt at any time.
#![forbid(unsafe_code)]

pub mod synth;

use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};

use crossbeam_channel::{unbounded, Receiver, Sender};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, ImageDecoder, ImageReader, RgbImage, RgbaImage};
use oto_core::geometry::Vec2;
use oto_core::media::{ColorLabel, Features, MediaItem};
use oto_core::model::{new_id, now_iso};
use rayon::prelude::*;

pub const SUPPORTED: [&str; 8] = ["jpg", "jpeg", "png", "tif", "tiff", "psd", "webp", "jpe"];

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("cannot read {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("cannot decode {path}: {msg}")]
    Decode { path: String, msg: String },
    #[error("unsupported file type: {0}")]
    Unsupported(String),
}

fn io_err(path: &Path, source: std::io::Error) -> MediaError {
    MediaError::Io { path: path.display().to_string(), source }
}

pub fn is_supported(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| SUPPORTED.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// Recursively collect supported images (files or folders), natural-sorted, no hidden files.
pub fn scan(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = inputs.to_vec();
    while let Some(p) = stack.pop() {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name.ends_with(".otoalbum") {
            continue;
        }
        if p.is_dir() {
            if let Ok(rd) = fs::read_dir(&p) {
                for e in rd.flatten() {
                    stack.push(e.path());
                }
            }
        } else if is_supported(&p) {
            out.push(p);
        }
    }
    out.sort_by_key(|p| oto_core::media::natural_key(&p.to_string_lossy()));
    out.dedup();
    out
}

/// Derived-file layout inside `<project>/cache`.
#[derive(Debug, Clone)]
pub struct CacheLayout {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    Thumb,
    Proxy,
    Preview,
}

impl Level {
    pub fn px(self) -> u32 {
        match self {
            Level::Thumb => 256,
            Level::Proxy => 1024,
            Level::Preview => 2048,
        }
    }
    fn dir(self) -> &'static str {
        match self {
            Level::Thumb => "thumbs",
            Level::Proxy => "proxies",
            Level::Preview => "previews",
        }
    }
}

impl CacheLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        CacheLayout { root: root.into() }
    }
    pub fn path(&self, level: Level, hash: &str) -> PathBuf {
        self.root.join(level.dir()).join(format!("{hash}.jpg"))
    }
    pub fn ensure(&self) -> std::io::Result<()> {
        for l in [Level::Thumb, Level::Proxy, Level::Preview] {
            fs::create_dir_all(self.root.join(l.dir()))?;
        }
        Ok(())
    }
    pub fn has_all(&self, hash: &str) -> bool {
        [Level::Thumb, Level::Proxy, Level::Preview].iter().all(|l| self.path(*l, hash).exists())
    }
}

/// Apply EXIF orientation 1..8 so pixels are upright.
pub fn apply_orientation(img: DynamicImage, o: u8) -> DynamicImage {
    match o {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// Orientations that swap width and height.
pub fn orientation_swaps(o: u8) -> bool {
    matches!(o, 5..=8)
}

#[derive(Debug, Default, Clone)]
pub struct ExifInfo {
    pub orientation: u8,
    pub capture_time: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub aperture: Option<f32>,
    pub shutter: Option<String>,
    pub focal_mm: Option<f32>,
}

pub fn read_exif(bytes: &[u8]) -> ExifInfo {
    let mut info = ExifInfo { orientation: 1, ..Default::default() };
    let Ok(ex) = exif::Reader::new().read_from_container(&mut BufReader::new(Cursor::new(bytes))) else {
        return info;
    };
    use exif::{In, Tag, Value};
    let get = |t: Tag| ex.get_field(t, In::PRIMARY);
    if let Some(o) = get(Tag::Orientation).and_then(|f| f.value.get_uint(0)) {
        if (1..=8).contains(&o) {
            info.orientation = o as u8;
        }
    }
    let ascii = |t: Tag| -> Option<String> {
        let f = get(t)?;
        if let Value::Ascii(v) = &f.value {
            let s = v.iter().map(|b| String::from_utf8_lossy(b).trim().trim_matches('\0').to_string()).collect::<Vec<_>>().join(" ");
            let s = s.trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        } else {
            None
        }
    };
    if let Some(dt) = ascii(Tag::DateTimeOriginal).or_else(|| ascii(Tag::DateTime)) {
        // "YYYY:MM:DD HH:MM:SS" → "YYYY-MM-DDTHH:MM:SS"
        if dt.len() >= 19 && dt.is_char_boundary(19) {
            let b = dt.as_bytes();
            if b[4] == b':' || b[4] == b'-' {
                info.capture_time = Some(format!("{}-{}-{}T{}", &dt[0..4], &dt[5..7], &dt[8..10], &dt[11..19]));
            }
        }
    }
    let make = ascii(Tag::Make).unwrap_or_default();
    let model = ascii(Tag::Model).unwrap_or_default();
    let cam = if model.starts_with(&make) { model.clone() } else { format!("{make} {model}").trim().to_string() };
    if !cam.is_empty() {
        info.camera = Some(cam);
    }
    info.lens = ascii(Tag::LensModel);
    info.iso = get(Tag::PhotographicSensitivity).and_then(|f| f.value.get_uint(0));
    let rational = |t: Tag| -> Option<f64> {
        match &get(t)?.value {
            Value::Rational(v) => v.first().map(|r| r.to_f64()),
            _ => None,
        }
    };
    info.aperture = rational(Tag::FNumber).map(|v| v as f32);
    info.focal_mm = rational(Tag::FocalLength).map(|v| v as f32);
    info.shutter = rational(Tag::ExposureTime).map(|v| if v > 0.0 && v < 1.0 { format!("1/{:.0}", 1.0 / v) } else { format!("{v:.1}s") });
    info
}

/// Rough human name for an embedded ICC profile.
pub fn icc_name(icc: &[u8]) -> String {
    let text: String = icc.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { ' ' }).collect();
    for (needle, name) in [("sRGB", "sRGB"), ("Adobe RGB", "Adobe RGB (1998)"), ("Display P3", "Display P3"), ("ProPhoto", "ProPhoto RGB"), ("Generic Gray", "Gray"), ("Dot Gain", "Gray")] {
        if text.contains(needle) {
            return name.into();
        }
    }
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    t.chars().take(40).collect()
}

/// Decoded, upright image plus metadata.
pub struct Decoded {
    pub image: DynamicImage,
    pub icc: Option<Vec<u8>>,
    pub exif: ExifInfo,
    pub format: String,
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Decode bytes (any supported format) and apply EXIF orientation.
pub fn decode_bytes(path: &Path, bytes: &[u8]) -> Result<Decoded, MediaError> {
    let ext = ext_of(path);
    let derr = |msg: String| MediaError::Decode { path: path.display().to_string(), msg };
    if ext == "psd" {
        let psd = psd::Psd::from_bytes(bytes).map_err(|e| derr(e.to_string()))?;
        let img = RgbaImage::from_raw(psd.width(), psd.height(), psd.rgba()).ok_or_else(|| derr("PSD composite size mismatch".into()))?;
        return Ok(Decoded { image: DynamicImage::ImageRgba8(img), icc: None, exif: ExifInfo { orientation: 1, ..Default::default() }, format: "psd".into() });
    }
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| io_err(path, e))?;
    let format = reader.format().map(|f| format!("{f:?}").to_ascii_lowercase()).unwrap_or(ext);
    let mut dec = reader.into_decoder().map_err(|e| derr(e.to_string()))?;
    let icc = dec.icc_profile().ok().flatten();
    let img = DynamicImage::from_decoder(dec).map_err(|e| derr(e.to_string()))?;
    let exif = if matches!(format.as_str(), "jpeg" | "tiff" | "webp" | "png") { read_exif(bytes) } else { ExifInfo { orientation: 1, ..Default::default() } };
    let image = apply_orientation(img, exif.orientation);
    Ok(Decoded { image, icc, exif, format })
}

/// Decode a file at full resolution, upright (used by export).
pub fn load_full(path: &Path) -> Result<Decoded, MediaError> {
    let bytes = fs::read(path).map_err(|e| io_err(path, e))?;
    decode_bytes(path, &bytes)
}

/// Load a cached derivative (thumb/proxy/preview) as RGBA.
pub fn load_cached(cache: &CacheLayout, level: Level, hash: &str) -> Option<RgbaImage> {
    let p = cache.path(level, hash);
    let img = image::open(&p).ok()?;
    Some(img.to_rgba8())
}

fn save_jpeg(img: &RgbImage, path: &Path, q: u8) -> std::io::Result<()> {
    // Unique temp name: two identical files imported in parallel share one cache entry.
    let tmp = path.with_extension(format!("{}.tmp", new_id().simple()));
    {
        let f = fs::File::create(&tmp)?;
        let mut w = std::io::BufWriter::new(f);
        JpegEncoder::new_with_quality(&mut w, q).encode_image(img).map_err(std::io::Error::other)?;
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => {
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn fit_size(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w <= max && h <= max {
        return (w, h);
    }
    let s = max as f32 / w.max(h) as f32;
    (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1))
}

/// Write 2048 / 1024 / 256 derivatives (each from the next larger one).
pub fn write_derivatives(img: &DynamicImage, cache: &CacheLayout, hash: &str) -> std::io::Result<RgbImage> {
    let rgb = img.to_rgb8();
    let (w, h) = fit_size(rgb.width(), rgb.height(), 2048);
    let preview = if (w, h) == (rgb.width(), rgb.height()) { rgb } else { image::imageops::resize(&rgb, w, h, FilterType::Triangle) };
    save_jpeg(&preview, &cache.path(Level::Preview, hash), 88)?;
    let (w, h) = fit_size(preview.width(), preview.height(), 1024);
    let proxy = image::imageops::resize(&preview, w, h, FilterType::Triangle);
    save_jpeg(&proxy, &cache.path(Level::Proxy, hash), 85)?;
    let (w, h) = fit_size(proxy.width(), proxy.height(), 256);
    let thumb = image::imageops::resize(&proxy, w, h, FilterType::Triangle);
    save_jpeg(&thumb, &cache.path(Level::Thumb, hash), 82)?;
    Ok(thumb)
}

/// Visual features from a small RGB image.
pub fn features(thumb: &RgbImage) -> Features {
    let (w, h) = thumb.dimensions();
    let n = (w * h).max(1) as f32;
    let mut luma = GrayImage::new(w, h);
    let (mut sum_l, mut chroma) = (0.0f32, 0.0f32);
    let (mut s_rg, mut s_yb, mut ss_rg, mut ss_yb) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (x, y, p) in thumb.enumerate_pixels() {
        let [r, g, b] = p.0.map(|v| v as f32);
        let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        luma.put_pixel(x, y, image::Luma([l as u8]));
        sum_l += l;
        chroma += (r - g).abs() + (g - b).abs();
        let rg = r - g;
        let yb = 0.5 * (r + g) - b;
        s_rg += rg;
        s_yb += yb;
        ss_rg += rg * rg;
        ss_yb += yb * yb;
    }
    let m_rg = s_rg / n;
    let m_yb = s_yb / n;
    let sd_rg = (ss_rg / n - m_rg * m_rg).max(0.0).sqrt();
    let sd_yb = (ss_yb / n - m_yb * m_yb).max(0.0).sqrt();
    let colorfulness = ((sd_rg.powi(2) + sd_yb.powi(2)).sqrt() + 0.3 * (m_rg.powi(2) + m_yb.powi(2)).sqrt()) / 150.0;
    // Laplacian variance + gradient centroid.
    let (mut lap_s, mut lap_ss, mut cnt) = (0.0f64, 0.0f64, 0.0f64);
    let mut grads: Vec<(f32, u32, u32)> = Vec::with_capacity((w * h) as usize);
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let c = luma.get_pixel(x, y).0[0] as f64;
            let v = luma.get_pixel(x - 1, y).0[0] as f64 + luma.get_pixel(x + 1, y).0[0] as f64 + luma.get_pixel(x, y - 1).0[0] as f64 + luma.get_pixel(x, y + 1).0[0] as f64 - 4.0 * c;
            lap_s += v;
            lap_ss += v * v;
            cnt += 1.0;
            let gx = luma.get_pixel(x + 1, y).0[0] as f32 - luma.get_pixel(x - 1, y).0[0] as f32;
            let gy = luma.get_pixel(x, y + 1).0[0] as f32 - luma.get_pixel(x, y - 1).0[0] as f32;
            grads.push(((gx * gx + gy * gy).sqrt(), x, y));
        }
    }
    let var = if cnt > 0.0 { lap_ss / cnt - (lap_s / cnt).powi(2) } else { 0.0 };
    let sharpness = (var / (var + 400.0)) as f32;
    let focus_auto = if grads.len() > 16 {
        grads.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top = &grads[..grads.len() / 10];
        let wsum: f32 = top.iter().map(|g| g.0).sum::<f32>().max(1e-3);
        let cx = top.iter().map(|g| g.0 * g.1 as f32).sum::<f32>() / wsum / w as f32;
        let cy = top.iter().map(|g| g.0 * g.2 as f32).sum::<f32>() / wsum / h as f32;
        // Blend toward the centre: the estimate is a hint, not a detection.
        Some(Vec2::new(0.6 * cx + 0.4 * 0.5, 0.6 * cy + 0.4 * 0.45))
    } else {
        None
    };
    // dHash 9×8.
    let small = image::imageops::resize(&luma, 9, 8, FilterType::Triangle);
    let mut dhash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let bit = small.get_pixel(x, y).0[0] > small.get_pixel(x + 1, y).0[0];
            dhash = (dhash << 1) | bit as u64;
        }
    }
    Features { brightness: sum_l / n / 255.0, colorfulness: colorfulness.clamp(0.0, 1.0), sharpness, is_bw: chroma / n < 8.0, dhash, focus_auto, faces: Vec::new() }
}

fn mtime_secs(md: &fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Import a single file: hash, decode, EXIF, derivatives, features. Never touches the original.
pub fn import_one(path: &Path, cache: &CacheLayout) -> Result<MediaItem, MediaError> {
    let md = fs::metadata(path).map_err(|e| io_err(path, e))?;
    let bytes = fs::read(path).map_err(|e| io_err(path, e))?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let d = decode_bytes(path, &bytes)?;
    let thumb = if cache.has_all(&hash) { load_cached(cache, Level::Thumb, &hash).map(|i| DynamicImage::ImageRgba8(i).to_rgb8()) } else { None };
    let thumb = match thumb {
        Some(t) => t,
        None => write_derivatives(&d.image, cache, &hash).map_err(|e| io_err(path, e))?,
    };
    let feats = features(&thumb);
    Ok(MediaItem {
        id: new_id(),
        path: path.display().to_string(),
        file_name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        hash,
        size: md.len(),
        mtime: mtime_secs(&md),
        width: d.image.width(),
        height: d.image.height(),
        orientation: d.exif.orientation,
        format: d.format,
        capture_time: d.exif.capture_time,
        camera: d.exif.camera,
        lens: d.exif.lens,
        iso: d.exif.iso,
        aperture: d.exif.aperture,
        shutter: d.exif.shutter,
        focal_mm: d.exif.focal_mm,
        color_profile: d.icc.as_deref().map(icc_name),
        rating: 0,
        color_label: ColorLabel::None,
        rejected: false,
        flagged: false,
        client_pick: false,
        bins: Vec::new(),
        missing: false,
        features: feats,
        focus: None,
        similar_group: None,
        imported_at: now_iso(),
    })
}

#[derive(Debug, Clone)]
pub enum ImportMsg {
    Started { total: usize },
    Progress { done: usize, total: usize, current: String },
    Item(Box<MediaItem>),
    Duplicate { path: String },
    Failed { path: String, error: String },
    Finished { imported: usize, duplicates: usize, failed: usize, cancelled: bool, elapsed_ms: u128 },
}

/// Import in the background on the rayon pool. Exact duplicates (same BLAKE3) are skipped.
pub fn spawn_import(inputs: Vec<PathBuf>, cache: CacheLayout, known_hashes: HashSet<String>, cancel: Arc<AtomicBool>) -> Receiver<ImportMsg> {
    let (tx, rx) = unbounded();
    let spawned = std::thread::Builder::new().name("oto-import".into()).spawn(move || run_import(inputs, cache, known_hashes, cancel, tx));
    if let Err(e) = spawned {
        tracing::error!("cannot start import thread: {e}");
    }
    rx
}

fn run_import(inputs: Vec<PathBuf>, cache: CacheLayout, known: HashSet<String>, cancel: Arc<AtomicBool>, tx: Sender<ImportMsg>) {
    let t0 = Instant::now();
    if let Err(e) = cache.ensure() {
        let _ = tx.send(ImportMsg::Failed { path: cache.root.display().to_string(), error: e.to_string() });
    }
    let files = scan(&inputs);
    let total = files.len();
    let _ = tx.send(ImportMsg::Started { total });
    let done = std::sync::atomic::AtomicUsize::new(0);
    let seen = std::sync::Mutex::new(known);
    let counts = std::sync::Mutex::new((0usize, 0usize, 0usize));
    files.par_iter().for_each(|p| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let res = import_one(p, &cache);
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let mut c = counts.lock().unwrap_or_else(|e| e.into_inner());
            match res {
                Ok(item) => {
                    let fresh = seen.lock().map(|mut s| s.insert(item.hash.clone())).unwrap_or(true);
                    if fresh {
                        c.0 += 1;
                        let _ = tx.send(ImportMsg::Item(Box::new(item)));
                    } else {
                        c.1 += 1;
                        let _ = tx.send(ImportMsg::Duplicate { path: p.display().to_string() });
                    }
                }
                Err(e) => {
                    c.2 += 1;
                    let _ = tx.send(ImportMsg::Failed { path: p.display().to_string(), error: e.to_string() });
                }
            }
        }
        let _ = tx.send(ImportMsg::Progress { done: n, total, current: p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() });
    });
    let (imported, duplicates, failed) = *counts.lock().unwrap_or_else(|e| e.into_inner());
    let _ = tx.send(ImportMsg::Finished { imported, duplicates, failed, cancelled: cancel.load(Ordering::Relaxed), elapsed_ms: t0.elapsed().as_millis() });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Ok,
    Missing,
    Changed,
}

/// Cheap check (stat only) whether the original is still where we left it.
pub fn file_status(item: &MediaItem) -> FileStatus {
    match fs::metadata(&item.path) {
        Err(_) => FileStatus::Missing,
        Ok(md) => {
            if md.len() != item.size || mtime_secs(&md) != item.mtime {
                FileStatus::Changed
            } else {
                FileStatus::Ok
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelinkMatch {
    pub media_id: uuid::Uuid,
    /// Same content (hash verified) — safe to relink automatically.
    pub exact: Option<PathBuf>,
    /// Same file name, different content — needs user confirmation.
    pub candidates: Vec<PathBuf>,
}

/// Find missing originals inside `folder`, matching by name and verifying by hash.
pub fn find_relinks(missing: &[MediaItem], folder: &Path) -> Vec<RelinkMatch> {
    let files = scan(&[folder.to_path_buf()]);
    let mut by_name: std::collections::HashMap<String, Vec<PathBuf>> = std::collections::HashMap::new();
    let mut by_size: std::collections::HashMap<u64, Vec<PathBuf>> = std::collections::HashMap::new();
    for f in &files {
        if let Some(n) = f.file_name() {
            by_name.entry(n.to_string_lossy().to_lowercase()).or_default().push(f.clone());
        }
        if let Ok(md) = fs::metadata(f) {
            by_size.entry(md.len()).or_default().push(f.clone());
        }
    }
    let hash_of = |p: &Path| fs::read(p).ok().map(|b| blake3::hash(&b).to_hex().to_string());
    missing
        .par_iter()
        .map(|m| {
            let mut exact = None;
            let mut candidates = Vec::new();
            for p in by_name.get(&m.file_name.to_lowercase()).cloned().unwrap_or_default() {
                if hash_of(&p).as_deref() == Some(m.hash.as_str()) {
                    exact = Some(p);
                    break;
                }
                candidates.push(p);
            }
            if exact.is_none() {
                for p in by_size.get(&m.size).cloned().unwrap_or_default() {
                    if hash_of(&p).as_deref() == Some(m.hash.as_str()) {
                        exact = Some(p);
                        break;
                    }
                }
            }
            RelinkMatch { media_id: m.id, exact, candidates }
        })
        .collect()
}

/// Copy originals into `dest` and verify each copy by hash. Returns (media id, new path).
pub fn consolidate(items: &[MediaItem], dest: &Path) -> Result<Vec<(uuid::Uuid, PathBuf)>, MediaError> {
    fs::create_dir_all(dest).map_err(|e| io_err(dest, e))?;
    items
        .par_iter()
        .filter(|m| !m.missing)
        .map(|m| {
            let src = PathBuf::from(&m.path);
            let mut target = dest.join(&m.file_name);
            if target.exists() && fs::read(&target).ok().map(|b| blake3::hash(&b).to_hex().to_string()) != Some(m.hash.clone()) {
                target = dest.join(format!("{}_{}", &m.hash[..8.min(m.hash.len())], m.file_name));
            }
            if !target.exists() {
                fs::copy(&src, &target).map_err(|e| io_err(&src, e))?;
            }
            let bytes = fs::read(&target).map_err(|e| io_err(&target, e))?;
            if blake3::hash(&bytes).to_hex().as_str() != m.hash {
                return Err(MediaError::Decode { path: target.display().to_string(), msg: "copy verification failed (hash mismatch)".into() });
            }
            Ok((m.id, target))
        })
        .collect()
}

/// Group burst / near-duplicate frames across a library (writes `similar_group`).
pub fn assign_similarity(items: &mut [MediaItem]) {
    let data: Vec<(u64, Option<i64>)> = items.iter().map(|m| (m.features.dhash, m.capture_time.as_deref().and_then(oto_core::solver::parse_time))).collect();
    let groups = oto_core::media::similarity_groups(&data, 5, 20);
    for (m, g) in items.iter_mut().zip(groups) {
        m.similar_group = g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oto-media-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn gradient(w: u32, h: u32, seed: u8) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 255 / w) as u8 ^ seed, (y * 255 / h) as u8, seed]))
    }

    #[test]
    fn orientation_all_eight() {
        let img = DynamicImage::ImageRgb8(gradient(40, 20, 3));
        for o in 1..=8u8 {
            let r = apply_orientation(img.clone(), o);
            if orientation_swaps(o) {
                assert_eq!((r.width(), r.height()), (20, 40), "orientation {o}");
            } else {
                assert_eq!((r.width(), r.height()), (40, 20), "orientation {o}");
            }
        }
        // 6 = rotate 90° CW: top-left pixel moves to top-right.
        let r = apply_orientation(img.clone(), 6).to_rgb8();
        assert_eq!(r.get_pixel(19, 0), img.to_rgb8().get_pixel(0, 0));
    }

    #[test]
    fn import_dedupe_and_features() {
        let d = tmpdir("import");
        let src = d.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        gradient(600, 400, 10).save(src.join("a.jpg")).unwrap();
        gradient(400, 600, 99).save(src.join("sub/b.png")).unwrap();
        fs::copy(src.join("a.jpg"), src.join("sub/a-copy.jpg")).unwrap();
        fs::write(src.join("notes.txt"), "x").unwrap();
        let cache = CacheLayout::new(d.join("cache"));
        let rx = spawn_import(vec![src.clone()], cache.clone(), HashSet::new(), Arc::new(AtomicBool::new(false)));
        let mut items = Vec::new();
        let mut dups = 0;
        for m in rx.iter() {
            match m {
                ImportMsg::Item(i) => items.push(*i),
                ImportMsg::Duplicate { .. } => dups += 1,
                ImportMsg::Finished { .. } => break,
                _ => {}
            }
        }
        assert_eq!(items.len(), 2);
        assert_eq!(dups, 1);
        for i in &items {
            assert!(cache.has_all(&i.hash));
            assert!(load_cached(&cache, Level::Thumb, &i.hash).unwrap().width() <= 256);
            assert_eq!(file_status(i), FileStatus::Ok);
        }
        let b = items.iter().find(|i| i.file_name == "b.png").unwrap();
        assert_eq!((b.width, b.height), (400, 600));
        assert!(!b.features.is_bw);
        // Relink after moving a file.
        let moved = d.join("moved");
        fs::create_dir_all(&moved).unwrap();
        fs::rename(src.join("sub/b.png"), moved.join("b.png")).unwrap();
        let mut missing = b.clone();
        assert_eq!(file_status(&missing), FileStatus::Missing);
        missing.missing = true;
        let r = find_relinks(&[missing], &moved);
        assert_eq!(r[0].exact.as_deref(), Some(moved.join("b.png").as_path()));
    }

    #[test]
    fn bw_detection_and_dhash() {
        let gray = RgbImage::from_fn(64, 64, |x, _| image::Rgb([(x * 4) as u8; 3]));
        let f = features(&gray);
        assert!(f.is_bw);
        let f2 = features(&gray);
        assert_eq!(f.dhash, f2.dhash);
    }

    #[test]
    fn unsupported_and_broken() {
        let d = tmpdir("broken");
        let p = d.join("x.jpg");
        fs::write(&p, b"not a jpeg").unwrap();
        assert!(import_one(&p, &CacheLayout::new(d.join("c"))).is_err());
        assert!(!is_supported(Path::new("a.cr2")));
    }
}
