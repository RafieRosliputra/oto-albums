//! oto-albums design system: calm, editorial. Charcoal / bone / paper with a vermilion accent and
//! a technical cyan reserved for guides. 8 pt grid, 1 px borders, 44 px hit targets where it counts.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};
use serde::{Deserialize, Serialize};

pub const CHARCOAL: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);
pub const CHARCOAL_2: Color32 = Color32::from_rgb(0x29, 0x29, 0x29);
pub const CHARCOAL_3: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
pub const BONE: Color32 = Color32::from_rgb(0xF3, 0xEF, 0xE6);
pub const PAPER: Color32 = Color32::from_rgb(0xFB, 0xF8, 0xF2);
pub const VERMILION: Color32 = Color32::from_rgb(0xFF, 0xE7, 0x3A);
/// Text drawn on top of the accent fill. The SmartAlbums accent is a light
/// yellow, so on-accent text must be near-black rather than the panel text colour.
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);
pub const CYAN: Color32 = Color32::from_rgb(0x3F, 0xA7, 0xA3);
pub const AMBER: Color32 = Color32::from_rgb(0xC9, 0x8A, 0x1B);
pub const RED: Color32 = Color32::from_rgb(0xC2, 0x3B, 0x30);
pub const GREEN: Color32 = Color32::from_rgb(0x4E, 0x8F, 0x5B);

pub const SPACE: f32 = 8.0;
pub const HIT: f32 = 44.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    HighContrast,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub raised: Color32,
    pub canvas: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub guide: Color32,
    pub warn: Color32,
    pub crit: Color32,
    pub ok: Color32,
    pub selection: Color32,
}

pub fn palette(mode: ThemeMode) -> Palette {
    match mode {
        ThemeMode::Dark => Palette {
            bg: CHARCOAL,
            panel: CHARCOAL_2,
            raised: CHARCOAL_3,
            canvas: Color32::from_rgb(0x54, 0x54, 0x54),
            text: Color32::from_rgb(0xEB, 0xEB, 0xEB),
            muted: Color32::from_rgb(0xA0, 0xA0, 0xA0),
            border: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            accent: VERMILION,
            guide: CYAN,
            warn: AMBER,
            crit: Color32::from_rgb(0xE0, 0x56, 0x4A),
            ok: GREEN,
            selection: VERMILION,
        },
        ThemeMode::Light => Palette {
            bg: BONE,
            panel: PAPER,
            raised: Color32::WHITE,
            canvas: Color32::from_rgb(0xE6, 0xE1, 0xD6),
            text: CHARCOAL,
            muted: Color32::from_rgb(0x6B, 0x67, 0x60),
            border: Color32::from_rgb(0xD8, 0xD1, 0xC3),
            accent: Color32::from_rgb(0xC2, 0x4B, 0x28),
            guide: Color32::from_rgb(0x1E, 0x7F, 0x7B),
            warn: Color32::from_rgb(0x9A, 0x63, 0x00),
            crit: RED,
            ok: Color32::from_rgb(0x2E, 0x6E, 0x3B),
            selection: Color32::from_rgb(0xC2, 0x4B, 0x28),
        },
        ThemeMode::HighContrast => Palette {
            bg: Color32::BLACK,
            panel: Color32::BLACK,
            raised: Color32::from_rgb(0x10, 0x10, 0x10),
            canvas: Color32::BLACK,
            text: Color32::WHITE,
            muted: Color32::from_rgb(0xE0, 0xE0, 0xE0),
            border: Color32::WHITE,
            accent: Color32::from_rgb(0xFF, 0xD4, 0x00),
            guide: Color32::from_rgb(0x00, 0xE5, 0xFF),
            warn: Color32::from_rgb(0xFF, 0xD4, 0x00),
            crit: Color32::from_rgb(0xFF, 0x60, 0x60),
            ok: Color32::from_rgb(0x60, 0xFF, 0x90),
            selection: Color32::from_rgb(0xFF, 0xD4, 0x00),
        },
    }
}

/// Register the UI font (Inter, OFL) and every album family for previews.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let inter = oto_render::fonts::FACES.iter().find(|f| f.postscript == "Inter-Regular").map(|f| f.data);
    if let Some(d) = inter {
        fonts.font_data.insert("inter".into(), Arc::new(FontData::from_static(d)));
        if let Some(v) = fonts.families.get_mut(&FontFamily::Proportional) {
            v.insert(0, "inter".into());
        }
    }
    fonts.font_data.insert("inter-semibold".into(), Arc::new(FontData::from_static(oto_render::fonts::INTER_SEMIBOLD)));
    // Symbol fallbacks: egui's bundled Ubuntu, Noto Emoji, icon font and Hack (arrows, boxes, shapes).
    let mut fallbacks: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default().into_iter().filter(|f| f != "inter").collect();
    if let Some(mono) = fonts.families.get(&FontFamily::Monospace) {
        for f in mono {
            if !fallbacks.contains(f) {
                fallbacks.push(f.clone());
            }
        }
    }
    if let Some(v) = fonts.families.get_mut(&FontFamily::Proportional) {
        for f in &fallbacks {
            if !v.contains(f) {
                v.push(f.clone());
            }
        }
    }
    let mut strong = vec!["inter-semibold".to_string(), "inter".to_string()];
    strong.extend(fallbacks.iter().cloned());
    fonts.families.insert(FontFamily::Name("strong".into()), strong);
    for fam in oto_core::model::ALBUM_FONTS {
        if let Some(face) = oto_render::fonts::FACES.iter().find(|f| f.family == fam && !f.bold && !f.italic) {
            let key = format!("album:{fam}");
            fonts.font_data.insert(key.clone(), Arc::new(FontData::from_static(face.data)));
            let mut v = vec![key, "inter".into()];
            v.extend(fallbacks.iter().cloned());
            fonts.families.insert(FontFamily::Name(fam.into()), v);
        }
    }
    ctx.set_fonts(fonts);
}

pub fn strong_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("strong".into()))
}


/// Palette for content drawn on the white inspector surface. The editor chrome is
/// dark while the inspector stays white, so that panel always needs dark text.
pub fn panel_palette(mode: ThemeMode) -> Palette {
    let base = palette(mode);
    let mut p = palette(ThemeMode::Light);
    p.panel = Color32::WHITE;
    p.raised = Color32::from_rgb(0xF2, 0xF2, 0xF2);
    p.border = Color32::from_rgb(0xDC, 0xDC, 0xDC);
    p.accent = base.accent;
    p.selection = base.accent;
    p
}

/// Visuals for widgets inside the white inspector panel.
pub fn panel_visuals(mode: ThemeMode) -> Visuals {
    let p = panel_palette(mode);
    let mut v = Visuals::light();
    v.panel_fill = Color32::WHITE;
    v.window_fill = Color32::WHITE;
    v.extreme_bg_color = Color32::from_rgb(0xF6, 0xF6, 0xF6);
    v.faint_bg_color = Color32::from_rgb(0xF0, 0xF0, 0xF0);
    v.override_text_color = Some(p.text);
    v.widgets.noninteractive.bg_fill = Color32::WHITE;
    v.widgets.inactive.bg_fill = Color32::from_rgb(0xEC, 0xEC, 0xEC);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0xE0, 0xE0, 0xE0);
    v.widgets.active.bg_fill = Color32::from_rgb(0xD4, 0xD4, 0xD4);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0_f32, p.text);
    v
}

pub fn apply(ctx: &egui::Context, mode: ThemeMode, reduced_motion: bool) {
    let p = palette(mode);
    let mut v = if mode == ThemeMode::Light { Visuals::light() } else { Visuals::dark() };
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.raised;
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.crit;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if mode == ThemeMode::Light { 40 } else { 120 }) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(60) };
    v.selection.bg_fill = p.selection.gamma_multiply(if mode == ThemeMode::HighContrast { 0.6 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0_f32, p.selection);
    let w = &mut v.widgets;
    let hc = mode == ThemeMode::HighContrast;
    for (st, bg) in [(&mut w.noninteractive, p.panel), (&mut w.inactive, p.raised), (&mut w.hovered, p.raised), (&mut w.active, p.raised), (&mut w.open, p.raised)] {
        st.bg_fill = bg;
        st.weak_bg_fill = bg;
        st.corner_radius = CornerRadius::same(6);
        st.fg_stroke = Stroke::new(if hc { 1.5_f32 } else { 1.0_f32 }, p.text);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.bg_stroke = Stroke::new(1.0_f32, if hc { p.text } else { p.border });
    w.hovered.bg_stroke = Stroke::new(if hc { 2.0_f32 } else { 1.0_f32 }, p.accent);
    w.active.bg_stroke = Stroke::new(if hc { 2.0_f32 } else { 1.5_f32 }, p.accent);
    w.hovered.weak_bg_fill = p.raised.gamma_multiply(1.2);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(SPACE, SPACE * 0.75);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.window_margin = Margin::same(16);
        s.spacing.menu_margin = Margin::same(8);
        s.text_styles.insert(TextStyle::Heading, strong_font(20.0));
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.5));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.5));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(11.0));
        s.animation_time = if reduced_motion { 0.0 } else { 0.12 };
        // Visible keyboard focus.
        s.visuals.widgets.hovered.expansion = 1.0;
    });
}
