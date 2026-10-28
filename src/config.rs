use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    /// Follow the Windows light/dark setting.
    #[default]
    System,
    /// The opposite of the Windows setting.
    Opposite,
    Light,
    Dark,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 4] = [ThemeMode::System, ThemeMode::Opposite, ThemeMode::Light, ThemeMode::Dark];
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "Match system",
            ThemeMode::Opposite => "Opposite of system",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        }
    }
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub notes_dir: PathBuf,
    pub theme: ThemeMode,
    pub font_size: f32,
    pub monospace_editor: bool,
}

impl Default for Config {
    fn default() -> Self {
        let base = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
        Self { notes_dir: base.join("nimblenote"), theme: ThemeMode::System, font_size: 15.0, monospace_editor: false }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("nimblenote-rs").join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = config_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, s);
            }
        }
    }
}
