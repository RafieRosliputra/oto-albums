//! Synthetic, copyright-free test photos: soft gradients with simple "people" (head + body
//! discs), a horizon, and a few textures. Used by fixtures, the sample project and tests.

use image::{Rgb, RgbImage};

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

const PALETTES: [([f32; 3], [f32; 3]); 6] = [
    ([238.0, 214.0, 190.0], [146.0, 118.0, 104.0]),
    ([196.0, 220.0, 228.0], [70.0, 104.0, 128.0]),
    ([232.0, 226.0, 205.0], [116.0, 128.0, 92.0]),
    ([244.0, 205.0, 182.0], [186.0, 96.0, 82.0]),
    ([210.0, 206.0, 226.0], [92.0, 84.0, 128.0]),
    ([226.0, 230.0, 232.0], [40.0, 44.0, 48.0]),
];

/// Deterministic synthetic photo. `bw` renders it in black & white.
pub fn photo(w: u32, h: u32, seed: u32, people: u32, bw: bool) -> RgbImage {
    let (top, bottom) = PALETTES[(seed as usize) % PALETTES.len()];
    let horizon = 0.55 + ((seed * 37) % 20) as f32 / 100.0;
    let wf = w as f32;
    let hf = h as f32;
    let unit = wf.min(hf);
    let figures: Vec<(f32, f32, f32)> = (0..people)
        .map(|i| {
            let x = wf * (0.25 + 0.5 * ((i as f32 + 0.5) / people.max(1) as f32)) + ((seed * 13 + i * 29) % 40) as f32 - 20.0;
            let r = unit * (0.07 + ((seed + i) % 3) as f32 * 0.015);
            (x, hf * horizon - r * 2.6, r)
        })
        .collect();
    let skin = [[224.0, 172.0, 140.0], [198.0, 140.0, 104.0], [150.0, 104.0, 78.0]];
    let cloth = [[250.0, 250.0, 246.0], [40.0, 44.0, 60.0], [128.0, 40.0, 52.0], [214.0, 190.0, 140.0]];
    RgbImage::from_fn(w, h, |x, y| {
        let (xf, yf) = (x as f32, y as f32);
        let t = yf / hf;
        let mut c = if t < horizon { mix(top, bottom, t / horizon * 0.6) } else { mix(bottom, top, (t - horizon) * 0.8) };
        // Soft vignette + fine texture.
        let dx = xf / wf - 0.5;
        let dy = yf / hf - 0.5;
        let v = 1.0 - (dx * dx + dy * dy) * 0.5;
        let n = (((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ seed) % 9) as f32) - 4.0;
        c = c.map(|k| k * v + n);
        for (i, &(fx, fy, r)) in figures.iter().enumerate() {
            let d = ((xf - fx).powi(2) + (yf - fy).powi(2)).sqrt();
            if d < r {
                c = skin[(seed as usize + i) % 3];
            }
            // Body: rounded trapezoid below the head.
            let by = yf - (fy + r * 1.1);
            if by > 0.0 && by < r * 4.0 && (xf - fx).abs() < r * (1.0 + by / (r * 4.0) * 0.6) {
                c = cloth[(seed as usize + i) % 4];
            }
        }
        if bw {
            let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            c = [l, l, l];
        }
        Rgb(c.map(|k| k.clamp(0.0, 255.0) as u8))
    })
}

/// Write `n` varied photos into `dir` (JPEG), returning their paths.
pub fn write_set(dir: &std::path::Path, n: u32, long_side: u32) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut out = Vec::new();
    for i in 0..n {
        let portrait = i % 3 == 1;
        let square = i % 11 == 7;
        let (w, h) = if square {
            (long_side * 4 / 5, long_side * 4 / 5)
        } else if portrait {
            (long_side * 2 / 3, long_side)
        } else {
            (long_side, long_side * 2 / 3)
        };
        let img = photo(w, h, i, 1 + i % 3, i % 9 == 4);
        let p = dir.join(format!("OTO_{:04}.jpg", i + 1));
        img.save(&p).map_err(std::io::Error::other)?;
        out.push(p);
    }
    Ok(out)
}
