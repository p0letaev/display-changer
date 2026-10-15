//! Global hotkeys that apply profiles. Works on X11 and Windows; Wayland
//! compositors do not allow global grabs, so registration fails there.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::AppState;
use crate::error::{CommandError, CommandResult};
use crate::{quick, reset};

/// Binding id of the emergency reset in the shortcut table.
const RESET_ID: &str = "__reset";
pub const RESERVED: &str = "занято аварийным сбросом (Пробел + Alt + →)";

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

    // The emergency reset goes first so no profile can take its keys.
    if state.settings.lock().unwrap().reset_combo {
        match parse(reset::ACCELERATOR).and_then(|s| {
            shortcuts
                .register(s)
                .map_err(|e| CommandError::Hotkey(e.to_string()))?;
            Ok(s)
        }) {
            Ok(s) => {
                bindings.insert(s.id(), RESET_ID.to_string());
            }
            Err(e) => log::warn!("reset combination unavailable: {e}"),
        }
    }

    let profiles = state.profiles.lock().unwrap().all().to_vec();
    for profile in profiles {
        let Some(accelerator) = &profile.hotkey else {
            continue;
        };
        let result = parse(accelerator).and_then(|shortcut| {
            match bindings.get(&shortcut.id()).map(String::as_str) {
                Some(RESET_ID) => return Err(CommandError::Hotkey(RESERVED.into())),
                Some(_) => {
                    return Err(CommandError::Hotkey("уже назначено другому профилю".into()));
                }
                None => {}
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
    let Some(target) = app.state::<AppState>().hotkeys.profile_for(shortcut) else {
        return;
    };
    if target == RESET_ID {
        // The grab is Alt + →; only with Space held is it the reset.
        if reset::space_held() {
            reset::spawn(app);
        }
        return;
    }
    quick::spawn(app, target, quick::Source::Hotkey);
}

/// Whether `accelerator` collides with the reset combination.
pub fn is_reserved(app: &AppHandle, accelerator: &str) -> bool {
    app.state::<AppState>().settings.lock().unwrap().reset_combo
        && matches!((parse(accelerator), parse(reset::ACCELERATOR)), (Ok(a), Ok(b)) if a.id() == b.id())
}
