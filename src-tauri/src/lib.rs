mod apply;
mod cli;
mod commands;
mod error;
mod hotkeys;
mod identify;
mod paths;
mod profiles;
mod quick;
mod settings;
mod tray;
mod watcher;

use std::sync::{Arc, Mutex};

use display_core::DisplayBackend;
use display_core::backends;
use tauri::{Manager, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

use crate::apply::ApplyManager;
use crate::hotkeys::HotkeyRegistry;
use crate::profiles::ProfileStore;
use crate::settings::Settings;

pub use cli::run as run_cli;

pub struct AppState {
    /// Shared with blocking tasks and timer threads.
    pub backend: Arc<dyn DisplayBackend>,
    pub profiles: Mutex<ProfileStore>,
    pub settings: Mutex<Settings>,
    pub apply: ApplyManager,
    pub hotkeys: HotkeyRegistry,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let backend: Arc<dyn DisplayBackend> = Arc::from(backends::detect());
    let settings = Settings::load(&paths::settings_file());
    let minimized = settings.start_minimized || std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        // Must be first: a second launch just brings the running window up.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_dialog::init())
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
            let store = profiles::open_store(backend.name())?;
            app.manage(AppState {
                backend,
                profiles: Mutex::new(store),
                settings: Mutex::new(settings),
                apply: ApplyManager::default(),
                hotkeys: HotkeyRegistry::default(),
            });
            hotkeys::sync(app.handle());
            if let Err(e) = tray::build(app.handle()) {
                // No tray (e.g. no StatusNotifier host): keep the window reachable.
                log::warn!("tray unavailable: {e}");
                app.state::<AppState>()
                    .settings
                    .lock()
                    .unwrap()
                    .close_to_tray = false;
            }
            watcher::start(app.handle().clone());
            if !minimized {
                tray::show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.settings.lock().unwrap().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
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
            commands::get_settings,
            commands::update_settings,
            commands::export_profiles,
            commands::import_profiles,
            commands::identify_monitors,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
