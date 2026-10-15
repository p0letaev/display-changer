//! File locations shared by the GUI and the CLI, which runs before Tauri
//! starts and so cannot ask it for `app_config_dir`.

use std::path::PathBuf;

/// Must match `identifier` in `tauri.conf.json`.
pub const IDENTIFIER: &str = "io.github.displaychanger";

/// `~/.config/io.github.displaychanger` on Linux, `%APPDATA%\io.github.displaychanger` on Windows.
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(IDENTIFIER)
}

/// Demo profiles live in their own file so they never mix with real ones.
pub fn profiles_file(backend: &str) -> PathBuf {
    config_dir().join(if backend == "mock" {
        "profiles.mock.json"
    } else {
        "profiles.json"
    })
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}
