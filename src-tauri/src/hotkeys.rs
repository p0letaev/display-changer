//! Global hotkeys that apply profiles. Works on X11 and Windows; Wayland
//! compositors do not allow global grabs, so registration fails there.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::AppState;
use crate::error::{CommandError, CommandResult};
use crate::quick;

#[derive(Default)]
pub struct HotkeyRegistry {
    /// Shortcut id -> profile id.
    bindings: Mutex<HashMap<u32, String>>,
    /// Profile id -> why its hotkey could not be registered.
    errors: Mutex<HashMap<String, String>>,
}

impl HotkeyRegistry {
    pub fn error_for(&self, profile_id: &str) -> Option<String> {
        self.errors.lock().unwrap().get(profile_id).cloned()
    }

    fn profile_for(&self, shortcut: &Shortcut) -> Option<String> {
        self.bindings.lock().unwrap().get(&shortcut.id()).cloned()
    }
}

/// Releases every global hotkey.
///
/// global-hotkey 0.8 on X11 queues `XUngrabKey` without flushing the
/// connection, so combinations stay grabbed until the next request that waits
/// for a reply. Registering a throwaway combination is such a request.
pub fn unregister_all(app: &AppHandle) -> CommandResult<()> {
    let shortcuts = app.global_shortcut();
    shortcuts
        .unregister_all()
        .map_err(|e| CommandError::Hotkey(e.to_string()))?;
    if cfg!(target_os = "linux") {
        let flush = parse("Ctrl+Alt+Shift+Super+F24")?;
        if shortcuts.register(flush).is_ok() {
            let _ = shortcuts.unregister(flush);
        }
    }
    Ok(())
}

pub fn parse(accelerator: &str) -> CommandResult<Shortcut> {
    accelerator
        .parse::<Shortcut>()
        .map_err(|e| CommandError::Hotkey(format!("{accelerator}: {e}")))
}

/// Re-registers hotkeys for all profiles. Failures are recorded per profile
/// instead of aborting, so one taken combination does not disable the rest.
pub fn sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    let shortcuts = app.global_shortcut();
    if let Err(e) = unregister_all(app) {
        log::warn!("unregister hotkeys: {e}");
    }

    let mut bindings = HashMap::new();
    let mut errors = HashMap::new();
    let profiles = state.profiles.lock().unwrap().all().to_vec();
    for profile in profiles {
        let Some(accelerator) = &profile.hotkey else {
            continue;
        };
        let result = parse(accelerator).and_then(|shortcut| {
            if bindings.contains_key(&shortcut.id()) {
                return Err(CommandError::Hotkey("уже назначено другому профилю".into()));
            }
            shortcuts.register(shortcut).map_err(|e| {
                let text = e.to_string();
                if text.contains("already registered") {
                    CommandError::Hotkey("сочетание занято другим приложением".into())
                } else {
                    CommandError::Hotkey(text)
                }
            })?;
            Ok(shortcut)
        });
        match result {
            Ok(shortcut) => {
                bindings.insert(shortcut.id(), profile.id);
            }
            Err(e) => {
                log::warn!("hotkey {accelerator} for {:?}: {e}", profile.name);
                errors.insert(profile.id, e.to_string());
            }
        }
    }

    *state.hotkeys.bindings.lock().unwrap() = bindings;
    *state.hotkeys.errors.lock().unwrap() = errors;
}

/// Plugin handler: applies the bound profile on a background thread.
pub fn on_pressed(app: &AppHandle, shortcut: &Shortcut) {
    if let Some(profile_id) = app.state::<AppState>().hotkeys.profile_for(shortcut) {
        quick::spawn(app, profile_id, quick::Source::Hotkey);
    }
}
