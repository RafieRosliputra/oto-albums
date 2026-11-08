//! Media library entries and image features used by Auto Build.

use serde::{Deserialize, Serialize};

use crate::geometry::{Rect, Vec2};
use crate::model::MediaId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorLabel {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl ColorLabel {
    pub const ALL: [ColorLabel; 6] = [Self::None, Self::Red, Self::Yellow, Self::Green, Self::Blue, Self::Purple];
    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Red => "red",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Purple => "purple",
        }
    }
    pub fn parse(s: &str) -> Self {
        Self::ALL.into_iter().find(|c| c.key() == s).unwrap_or_default()
    }
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::None => [128, 128, 128],
            Self::Red => [0xC8, 0x3E, 0x3E],
            Self::Yellow => [0xD8, 0xB0, 0x2C],
            Self::Green => [0x4E, 0x9A, 0x5B],
            Self::Blue => [0x3D, 0x6F, 0xB6],
            Self::Purple => [0x86, 0x58, 0xB0],
        }
    }
}

/// Cheap visual features computed from the 256 px thumbnail during import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Features {
    /// Mean luma 0..1.
    pub brightness: f32,
    /// Hasler–Süsstrunk colourfulness, roughly 0..1.
    pub colorfulness: f32,
    /// Normalized Laplacian variance, 0..1 (higher = sharper).
    pub sharpness: f32,
    pub is_bw: bool,
    /// 64-bit difference hash for similarity / bursts.
    pub dhash: u64,
    /// Automatic attention estimate (contrast centroid), 0..1 image coordinates.
    pub focus_auto: Option<Vec2>,
    /// Face boxes in 0..1 image coordinates (empty unless a detector provided them).
    pub faces: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: MediaId,
    pub path: String,
    pub file_name: String,
    /// BLAKE3 of the file bytes (hex).
    pub hash: String,
    pub size: u64,
    /// Modification time, seconds since UNIX epoch.
    pub mtime: i64,
    /// Pixel size **after** EXIF orientation.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1..8 (1 = as stored).
    pub orientation: u8,
    pub format: String,
    pub capture_time: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub aperture: Option<f32>,
    pub shutter: Option<String>,
    pub focal_mm: Option<f32>,
    pub color_profile: Option<String>,
    pub rating: u8,
    pub color_label: ColorLabel,
    pub rejected: bool,
    pub flagged: bool,
    pub client_pick: bool,
    pub bins: Vec<String>,
    pub missing: bool,
    pub features: Features,
    /// Focus point set by the user (wins over `features.focus_auto`).
    pub focus: Option<Vec2>,
    /// Images that are near-duplicates (burst / similar) share a group id.
    pub similar_group: Option<u32>,
    pub imported_at: String,
}

impl MediaItem {
    pub fn aspect(&self) -> f32 {
        self.width.max(1) as f32 / self.height.max(1) as f32
    }
    pub fn is_landscape(&self) -> bool {
        self.aspect() > 1.08
    }
    pub fn is_portrait(&self) -> bool {
        self.aspect() < 0.93
    }
    pub fn megapixels(&self) -> f32 {
        self.width as f32 * self.height as f32 / 1.0e6
    }
    /// Sort key that orders "IMG_2.jpg" before "IMG_10.jpg".
    pub fn natural_key(&self) -> Vec<NaturalChunk> {
        natural_key(&self.file_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NaturalChunk {
    Num(u64, usize),
    Text(String),
}

pub fn natural_key(s: &str) -> Vec<NaturalChunk> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut digits = false;
    let flush = |buf: &mut String, digits: bool, out: &mut Vec<NaturalChunk>| {
        if buf.is_empty() {
            return;
        }
        if digits {
            let n = buf.parse::<u64>().unwrap_or(u64::MAX);
            out.push(NaturalChunk::Num(n, buf.len()));
        } else {
            out.push(NaturalChunk::Text(buf.to_lowercase()));
        }
        buf.clear();
    };
    for c in s.chars() {
        let d = c.is_ascii_digit();
        if d != digits {
            flush(&mut buf, digits, &mut out);
            digits = d;
        }
        buf.push(c);
    }
    flush(&mut buf, digits, &mut out);
    out
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Group near-identical images (dHash distance ≤ `max_dist`) captured close in time
/// (or with unknown time). Returns group id per input index; singletons get `None`.
pub fn similarity_groups(items: &[(u64, Option<i64>)], max_dist: u32, max_secs: i64) -> Vec<Option<u32>> {
    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut c = i;
        while p[c] != r {
            let next = p[c];
            p[c] = r;
            c = next;
        }
        r
    }
    // Sort by time so we only compare neighbours in time (O(n·k)); unknown times compared to all.
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by_key(|&i| items[i].1.unwrap_or(i64::MAX));
    for a in 0..n {
        for b in (a + 1)..n.min(a + 40) {
            let (i, j) = (idx[a], idx[b]);
            let close = match (items[i].1, items[j].1) {
                (Some(x), Some(y)) => (x - y).abs() <= max_secs,
                _ => true,
            };
            if close && hamming(items[i].0, items[j].0) <= max_dist {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut counts = std::collections::HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        *counts.entry(r).or_insert(0u32) += 1;
    }
    let mut ids = std::collections::HashMap::new();
    let mut next = 1u32;
    (0..n)
        .map(|i| {
            let r = find(&mut parent, i);
            if counts[&r] < 2 {
                return None;
            }
            Some(*ids.entry(r).or_insert_with(|| {
                next += 1;
                next - 1
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v = vec!["IMG_10.jpg", "img_2.jpg", "IMG_1.jpg"];
        v.sort_by_key(|s| natural_key(s));
        assert_eq!(v, vec!["IMG_1.jpg", "img_2.jpg", "IMG_10.jpg"]);
    }

    #[test]
    fn groups() {
        let items = vec![(0b1111u64, Some(10)), (0b1110u64, Some(12)), (u64::MAX, Some(13)), (0b1111u64, Some(10_000))];
        let g = similarity_groups(&items, 3, 30);
        assert_eq!(g[0], g[1]);
        assert!(g[0].is_some());
        assert_eq!(g[2], None);
        assert_eq!(g[3], None);
    }
}
