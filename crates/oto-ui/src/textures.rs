//! GPU texture cache. Decoding happens on background threads (never on the UI thread); the UI
//! uploads at most a few textures per frame and evicts least-recently-used proxies.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crossbeam_channel::{unbounded, Receiver, Sender};
use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};
use oto_media::Level;

enum Slot {
    Pending,
    Ready(TextureHandle, u64),
    Failed,
}

pub struct TexCache {
    root: PathBuf,
    slots: HashMap<(String, Level), Slot>,
    req_tx: Sender<(PathBuf, String, Level)>,
    done_rx: Receiver<(String, Level, Option<ColorImage>)>,
    frame: u64,
    text: HashMap<u64, (TextureHandle, u64, f32)>,
    patterns: HashMap<u64, TextureHandle>,
    pub uploads_this_frame: usize,
}

fn options() -> TextureOptions {
    TextureOptions {
        magnification: egui::TextureFilter::Linear,
        minification: egui::TextureFilter::Linear,
        wrap_mode: egui::TextureWrapMode::ClampToEdge,
        mipmap_mode: Some(egui::TextureFilter::Linear),
    }
}

impl TexCache {
    pub fn new(cache_root: PathBuf) -> Self {
        let (req_tx, req_rx) = unbounded::<(PathBuf, String, Level)>();
        let (done_tx, done_rx) = unbounded();
        let threads = std::thread::available_parallelism().map(|n| n.get().clamp(2, 6) - 1).unwrap_or(2);
        for i in 0..threads {
            let rx = req_rx.clone();
            let tx = done_tx.clone();
            let spawned = std::thread::Builder::new().name(format!("oto-tex-{i}")).spawn(move || {
                for (path, hash, level) in rx.iter() {
                    let img = image::open(&path).ok().map(|i| {
                        let rgba = i.to_rgba8();
                        ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
                    });
                    if tx.send((hash, level, img)).is_err() {
                        break;
                    }
                }
            });
            if let Err(e) = spawned {
                tracing::error!("texture thread: {e}");
            }
        }
        TexCache { root: cache_root, slots: HashMap::new(), req_tx, done_rx, frame: 0, text: HashMap::new(), patterns: HashMap::new(), uploads_this_frame: 0 }
    }

    fn path(&self, hash: &str, level: Level) -> PathBuf {
        oto_media::CacheLayout::new(self.root.clone()).path(level, hash)
    }

    /// Texture for `hash` at `level`, or the best lower level already loaded (queued meanwhile).
    pub fn get(&mut self, hash: &str, level: Level) -> Option<(TextureId, egui::Vec2)> {
        let order: &[Level] = match level {
            Level::Preview => &[Level::Preview, Level::Proxy, Level::Thumb],
            Level::Proxy => &[Level::Proxy, Level::Thumb],
            Level::Thumb => &[Level::Thumb],
        };
        let key = (hash.to_string(), level);
        if !self.slots.contains_key(&key) {
            self.slots.insert(key.clone(), Slot::Pending);
            let _ = self.req_tx.send((self.path(hash, level), hash.to_string(), level));
        }
        for l in order {
            if let Some(Slot::Ready(t, used)) = self.slots.get_mut(&(hash.to_string(), *l)) {
                *used = self.frame;
                return Some((t.id(), t.size_vec2()));
            }
            if *l != level && !self.slots.contains_key(&(hash.to_string(), *l)) {
                self.slots.insert((hash.to_string(), *l), Slot::Pending);
                let _ = self.req_tx.send((self.path(hash, *l), hash.to_string(), *l));
            }
        }
        None
    }

    pub fn is_failed(&self, hash: &str) -> bool {
        matches!(self.slots.get(&(hash.to_string(), Level::Thumb)), Some(Slot::Failed))
    }

    /// Upload finished decodes and evict old proxies. Returns true if something changed.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        self.frame += 1;
        self.uploads_this_frame = 0;
        let mut changed = false;
        while self.uploads_this_frame < 12 {
            let Ok((hash, level, img)) = self.done_rx.try_recv() else { break };
            let slot = match img {
                Some(ci) => {
                    self.uploads_this_frame += 1;
                    Slot::Ready(ctx.load_texture(format!("{hash}-{level:?}"), ci, options()), self.frame)
                }
                None => Slot::Failed,
            };
            self.slots.insert((hash, level), slot);
            changed = true;
        }
        if self.frame % 120 == 0 {
            self.evict();
        }
        let frame = self.frame;
        self.text.retain(|_, (_, used, _)| frame - *used < 600);
        changed
    }

    fn evict(&mut self) {
        for (level, keep) in [(Level::Preview, 10usize), (Level::Proxy, 70), (Level::Thumb, 1500)] {
            let mut ready: Vec<(u64, (String, Level))> = self
                .slots
                .iter()
                .filter_map(|(k, v)| {
                    if k.1 == level {
                        if let Slot::Ready(_, u) = v {
                            Some((*u, k.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();
            if ready.len() > keep {
                ready.sort_by_key(|(u, _)| *u);
                let n = ready.len() - keep;
                for (_, k) in ready.into_iter().take(n) {
                    self.slots.remove(&k);
                }
            }
        }
    }

    pub fn has_pending(&self) -> bool {
        self.slots.values().any(|s| matches!(s, Slot::Pending))
    }

    pub fn stats(&self) -> (usize, f32) {
        let mut n = 0;
        let mut bytes = 0f32;
        for s in self.slots.values() {
            if let Slot::Ready(t, _) = s {
                n += 1;
                let sz = t.size();
                bytes += (sz[0] * sz[1] * 4) as f32 * 1.33;
            }
        }
        (n, bytes / 1_048_576.0)
    }

    /// Text rendered by the print engine, cached per content + size bucket.
    pub fn text(&mut self, ctx: &egui::Context, t: &oto_core::model::TextBlock, box_w_um: f64, box_h_um: f64, px_per_um: f64) -> Option<(TextureId, egui::Vec2, f32)> {
        // Bucket the scale so zooming does not re-rasterize every frame.
        let bucket = (px_per_um * 1e5).log2().mul_add(4.0, 0.0).round();
        let ppu = 2f64.powf(bucket / 4.0) / 1e5;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        t.text.hash(&mut h);
        format!("{:?}", t.style).hash(&mut h);
        ((box_w_um as i64) / 50, (box_h_um as i64) / 50, bucket as i64).hash(&mut h);
        let key = h.finish();
        if let Some((tex, used, pad_um)) = self.text.get_mut(&key) {
            *used = self.frame;
            return Some((tex.id(), tex.size_vec2(), *pad_um));
        }
        let (img, pad) = oto_render::raster::text_rgba(t, box_w_um, box_h_um, ppu)?;
        let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let tex = ctx.load_texture(format!("text-{key}"), ci, TextureOptions::LINEAR);
        let pad_um = pad as f32 / ppu as f32;
        let out = (tex.id(), tex.size_vec2(), pad_um);
        self.text.insert(key, (tex, self.frame, pad_um));
        Some(out)
    }

    /// Repeating pattern tile (straight alpha), sized for the current zoom.
    pub fn pattern(&mut self, ctx: &egui::Context, p: &oto_core::model::Pattern, px_per_mm: f32) -> TextureId {
        let bucket = (px_per_mm.max(0.5).log2() * 2.0).round();
        let ppm = 2f32.powf(bucket / 2.0).clamp(1.0, 24.0);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}{:?}{}{}", p.kind, p.color, p.scale_mm, p.opacity).hash(&mut h);
        (bucket as i64).hash(&mut h);
        let key = h.finish();
        if let Some(t) = self.patterns.get(&key) {
            return t.id();
        }
        let (n, mask) = oto_render::pattern::tile_mask(p, ppm);
        let a = p.opacity.clamp(0.0, 1.0);
        let px: Vec<egui::Color32> = mask.iter().map(|&m| egui::Color32::from_rgba_unmultiplied(p.color.r, p.color.g, p.color.b, (m as f32 * a) as u8)).collect();
        let ci = ColorImage { size: [n, n], pixels: px };
        let opts = TextureOptions { magnification: egui::TextureFilter::Linear, minification: egui::TextureFilter::Linear, wrap_mode: egui::TextureWrapMode::Repeat, mipmap_mode: None };
        let tex = ctx.load_texture(format!("pattern-{key}"), ci, opts);
        let id = tex.id();
        if self.patterns.len() > 64 {
            self.patterns.clear();
        }
        self.patterns.insert(key, tex);
        id
    }
}
