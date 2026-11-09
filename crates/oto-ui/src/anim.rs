//! Layout animation: when a spread switches template (variation, add/remove photo), photos glide from
//! their old place to the new one (220 ms, ease-out); photos that are new to the spread fade in.
//! Only the painted copy is animated — the document and hit-testing use the final layout.

use std::collections::HashMap;
use std::time::Instant;

use oto_core::geometry::Rect as NRect;
use oto_core::model::{MediaId, Spread, SpreadId};

const DURATION: f32 = 0.22;

#[derive(Default)]
pub struct Anim {
    spread: Option<SpreadId>,
    start: Option<Instant>,
    from: HashMap<MediaId, NRect>,
}

impl Anim {
    /// Remember where every photo of `before` is drawn now; call just before committing the new layout.
    pub fn begin(&mut self, before: &Spread, enabled: bool) {
        if !enabled {
            self.start = None;
            return;
        }
        self.spread = Some(before.id);
        self.start = Some(Instant::now());
        self.from = before.frames.iter().filter_map(|f| f.image_id.map(|id| (id, f.rect_norm))).collect();
    }

    pub fn active(&self) -> bool {
        self.start.map(|s| s.elapsed().as_secs_f32() < DURATION).unwrap_or(false)
    }

    /// A copy of `s` with every frame at its in-between position (and new photos partly faded in).
    pub fn apply(&self, s: &Spread) -> Spread {
        let Some(start) = self.start else { return s.clone() };
        if self.spread != Some(s.id) {
            return s.clone();
        }
        let t = (start.elapsed().as_secs_f32() / DURATION).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        let mut out = s.clone();
        for f in &mut out.frames {
            match f.image_id.and_then(|id| self.from.get(&id)) {
                Some(a) => {
                    let b = f.rect_norm;
                    f.rect_norm = NRect::new(a.x + (b.x - a.x) * e, a.y + (b.y - a.y) * e, a.w + (b.w - a.w) * e, a.h + (b.h - a.h) * e);
                }
                None => f.opacity *= e,
            }
        }
        out
    }
}
