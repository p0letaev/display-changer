//! User preferences, stored next to the profiles.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Seconds before an unconfirmed change is reverted.
    pub confirm_timeout_secs: u64,
    /// Ask for confirmation also when a profile is applied from the tray,
    /// a hotkey or automatically.
    pub confirm_quick_apply: bool,
    /// Closing the window hides it to the tray instead of quitting.
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub autostart: bool,
    /// Apply profiles marked "auto" when their monitors get connected.
    pub auto_switch: bool,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            confirm_timeout_secs: 15,
            confirm_quick_apply: false,
            close_to_tray: true,
            start_minimized: false,
            autostart: false,
            auto_switch: false,
            theme: Theme::System,
        }
    }
}

impl Settings {
    pub const MIN_TIMEOUT: u64 = 5;
    pub const MAX_TIMEOUT: u64 = 120;

    /// Clamps values a hand-edited file could get wrong.
    pub fn sanitized(mut self) -> Self {
        self.confirm_timeout_secs = self
            .confirm_timeout_secs
            .clamp(Self::MIN_TIMEOUT, Self::MAX_TIMEOUT);
        self
    }

    /// Missing or unreadable files give defaults; settings are not worth failing startup over.
    pub fn load(path: &PathBuf) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<Settings>(&text)
                .map(Settings::sanitized)
                .unwrap_or_else(|e| {
                    log::warn!("settings {}: {e}; using defaults", path.display());
                    Settings::default()
                }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, path: &PathBuf) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_keeps_defaults_and_clamps() {
        let s: Settings =
            serde_json::from_str(r#"{"confirmTimeoutSecs": 1, "theme": "dark"}"#).unwrap();
        let s = s.sanitized();
        assert_eq!(s.confirm_timeout_secs, Settings::MIN_TIMEOUT);
        assert_eq!(s.theme, Theme::Dark);
        assert!(s.close_to_tray);
    }

    #[test]
    fn round_trips_through_disk() {
        let path = std::env::temp_dir().join(format!("dc-settings-{}.json", std::process::id()));
        let s = Settings {
            auto_switch: true,
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        let _ = fs::remove_file(path);
    }
}
