//! Per-user app settings and the recent-projects list (JSON in the app data folder).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::theme::ThemeMode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
    pub status: String,
    pub due_date: Option<String>,
    pub opened_at: String,
    pub spreads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub lang: Lang,
    pub theme: ThemeMode,
    pub ui_scale: f32,
    pub reduced_motion: bool,
    pub units: oto_core::units::Units,
    pub recent: Vec<RecentProject>,
    pub onboarding_hidden: bool,
    pub show_rulers: bool,
    pub show_grid: bool,
    pub show_guides: bool,
    pub left_open: bool,
    pub right_open: bool,
    pub bottom_open: bool,
    pub last_import_dir: Option<PathBuf>,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            lang: Lang::En,
            theme: ThemeMode::Dark,
            ui_scale: 1.0,
            reduced_motion: false,
            units: oto_core::units::Units::Mm,
            recent: Vec::new(),
            onboarding_hidden: false,
            show_rulers: true,
            show_grid: false,
            show_guides: true,
            left_open: true,
            right_open: true,
            bottom_open: true,
            last_import_dir: None,
        }
    }
}

impl AppSettings {
    pub fn path() -> PathBuf {
        oto_platform::app_data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    pub fn touch_recent(&mut self, r: RecentProject) {
        self.recent.retain(|x| x.path != r.path);
        self.recent.insert(0, r);
        self.recent.truncate(30);
        self.save();
    }
}
