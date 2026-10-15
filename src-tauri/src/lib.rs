mod apply;
mod commands;
mod error;
mod hotkeys;
mod profiles;

use std::sync::{Arc, Mutex};

use display_core::backends::{self, mock};
use display_core::{DisplayBackend, Profile};
use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::apply::ApplyManager;
use crate::hotkeys::HotkeyRegistry;
use crate::profiles::ProfileStore;

pub struct AppState {
    /// Shared with blocking tasks and timer threads.
    pub backend: Arc<dyn DisplayBackend>,
    pub profiles: Mutex<ProfileStore>,
    pub apply: ApplyManager,
    pub hotkeys: HotkeyRegistry,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let backend: Arc<dyn DisplayBackend> = Arc::from(backends::detect());

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        hotkeys::on_pressed(app, shortcut);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            log::info!("display backend: {}", backend.name());

            // Demo profiles live in their own file so they never mix with real ones.
            let is_mock = backend.name() == "mock";
            let file = if is_mock {
                "profiles.mock.json"
            } else {
                "profiles.json"
            };
            let seed: fn() -> Vec<Profile> = if is_mock {
                mock::demo_profiles
            } else {
                Vec::new
            };
            let store = ProfileStore::load(app.path().app_config_dir()?.join(file), seed)?;

            app.manage(AppState {
                backend,
                profiles: Mutex::new(store),
                apply: ApplyManager::default(),
                hotkeys: HotkeyRegistry::default(),
            });
            hotkeys::sync(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::backend_info,
            commands::get_display_state,
            commands::validate_layout,
            commands::apply_layout,
            commands::confirm_apply,
            commands::revert_apply,
            commands::list_profiles,
            commands::preview_profile,
            commands::apply_profile,
            commands::create_profile,
            commands::update_profile,
            commands::delete_profile,
            commands::set_profile_hotkey,
            commands::set_hotkeys_paused,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
