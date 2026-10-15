use display_core::profile::{is_active, resolve};
use display_core::{Capabilities, DisplayState, Error, Layout, Profile, Resolved};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use std::time::Duration;

use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;

use crate::AppState;
use crate::apply::{ApplyOutcome, revert_and_notify};
use crate::error::{CommandError, CommandResult};
use crate::profiles::{merge_imported, new_profile_id};
use crate::settings::Settings;
use crate::{hotkeys, identify, paths, tray};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    name: &'static str,
    capabilities: Capabilities,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEntry {
    #[serde(flatten)]
    profile: Profile,
    /// The current desktop already matches this profile.
    active: bool,
    hotkey_error: Option<String>,
}

/// Runs `f` off the async runtime: real backends talk to X11, Wayland or
/// D-Bus synchronously, and the apply timer needs an owned `AppHandle`.
async fn blocking<T, F>(app: AppHandle, f: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle, &AppState) -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        f(&app, &state)
    })
    .await
    .map_err(|e| Error::Backend(format!("task failed: {e}")))?
}

fn confirm_timeout(state: &AppState) -> Duration {
    Duration::from_secs(state.settings.lock().unwrap().confirm_timeout_secs)
}

pub fn resolve_profile(state: &AppState, id: &str) -> CommandResult<Resolved> {
    let saved = state
        .profiles
        .lock()
        .unwrap()
        .get(id)
        .map(|p| p.layout.clone())
        .ok_or_else(|| CommandError::NotFound(id.into()))?;
    let current = state.backend.current_state()?;
    Ok(resolve(&saved, &current, &state.backend.capabilities()))
}

#[tauri::command]
pub fn backend_info(state: State<'_, AppState>) -> BackendInfo {
    BackendInfo {
        name: state.backend.name(),
        capabilities: state.backend.capabilities(),
    }
}

#[tauri::command]
pub async fn get_display_state(app: AppHandle) -> CommandResult<DisplayState> {
    blocking(app, |_, s| Ok(s.backend.current_state()?)).await
}

#[tauri::command]
pub async fn validate_layout(app: AppHandle, layout: Layout) -> CommandResult<()> {
    blocking(app, move |_, s| Ok(s.backend.validate(&layout)?)).await
}

/// Applies the layout; it is reverted unless `confirm_apply` follows in time.
#[tauri::command]
pub async fn apply_layout(app: AppHandle, layout: Layout) -> CommandResult<ApplyOutcome> {
    blocking(app, move |app, s| {
        let outcome =
            s.apply
                .apply_tentative(app, s.backend.as_ref(), &layout, confirm_timeout(s))?;
        tray::refresh(app);
        Ok(outcome)
    })
    .await
}

#[tauri::command]
pub fn confirm_apply(state: State<'_, AppState>, token: u64) -> bool {
    state.apply.confirm(token)
}

#[tauri::command]
pub async fn revert_apply(app: AppHandle, token: u64) -> CommandResult<DisplayState> {
    blocking(app, move |app, s| {
        if let Some(previous) = s.apply.take(token) {
            revert_and_notify(app, s.backend.as_ref(), &previous);
        }
        Ok(s.backend.current_state()?)
    })
    .await
}

#[tauri::command]
pub async fn list_profiles(app: AppHandle) -> CommandResult<Vec<ProfileEntry>> {
    blocking(app, |_, s| {
        let current = s.backend.current_state()?;
        let caps = s.backend.capabilities();
        let profiles = s.profiles.lock().unwrap().all().to_vec();
        Ok(profiles
            .into_iter()
            .map(|profile| ProfileEntry {
                active: is_active(&profile.layout, &current, &caps),
                hotkey_error: s.hotkeys.error_for(&profile.id),
                profile,
            })
            .collect())
    })
    .await
}

/// What applying the profile would produce on the current monitors.
#[tauri::command]
pub async fn preview_profile(app: AppHandle, id: String) -> CommandResult<Resolved> {
    blocking(app, move |_, s| resolve_profile(s, &id)).await
}

#[tauri::command]
pub async fn apply_profile(app: AppHandle, id: String) -> CommandResult<ApplyOutcome> {
    blocking(app, move |app, s| {
        let resolved = resolve_profile(s, &id)?;
        let outcome = s.apply.apply_tentative(
            app,
            s.backend.as_ref(),
            &resolved.layout,
            confirm_timeout(s),
        )?;
        tray::refresh(app);
        Ok(outcome)
    })
    .await
}

#[tauri::command]
pub fn create_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    layout: Layout,
) -> CommandResult<Profile> {
    let profile = Profile {
        id: new_profile_id(),
        name,
        hotkey: None,
        layout,
        auto_apply: false,
    };
    state.profiles.lock().unwrap().upsert(profile.clone())?;
    tray::refresh(&app);
    Ok(profile)
}

#[tauri::command]
pub fn update_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    layout: Option<Layout>,
    auto_apply: Option<bool>,
) -> CommandResult<Profile> {
    let profile = {
        let mut store = state.profiles.lock().unwrap();
        let mut profile = store.get(&id).cloned().ok_or(CommandError::NotFound(id))?;
        if let Some(name) = name {
            profile.name = name;
        }
        if let Some(layout) = layout {
            profile.layout = layout;
        }
        if let Some(auto_apply) = auto_apply {
            profile.auto_apply = auto_apply;
        }
        store.upsert(profile.clone())?;
        profile
    };
    tray::refresh(&app);
    Ok(profile)
}

#[tauri::command]
pub fn delete_profile(app: AppHandle, state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let removed = state.profiles.lock().unwrap().remove(&id)?;
    if !removed {
        return Err(CommandError::NotFound(id));
    }
    hotkeys::sync(&app);
    tray::refresh(&app);
    Ok(())
}

/// Binds or clears a profile hotkey. If the system refuses the combination,
/// the previous binding is restored and the error is returned.
#[tauri::command]
pub fn set_profile_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    hotkey: Option<String>,
) -> CommandResult<Profile> {
    let previous = {
        let mut store = state.profiles.lock().unwrap();
        let mut profile = store
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::NotFound(id.clone()))?;

        if let Some(accelerator) = &hotkey {
            if hotkeys::is_reserved(&app, accelerator) {
                return Err(CommandError::Hotkey(hotkeys::RESERVED.into()));
            }
            let wanted = hotkeys::parse(accelerator)?.id();
            let taken_by = store.all().iter().find(|p| {
                p.id != id
                    && p.hotkey
                        .as_deref()
                        .and_then(|h| hotkeys::parse(h).ok())
                        .map(|s| s.id())
                        == Some(wanted)
            });
            if let Some(other) = taken_by {
                return Err(CommandError::Hotkey(format!(
                    "уже назначено профилю «{}»",
                    other.name
                )));
            }
        }

        let previous = std::mem::replace(&mut profile.hotkey, hotkey);
        store.upsert(profile)?;
        previous
    };

    hotkeys::sync(&app);
    if let Some(error) = state.hotkeys.error_for(&id) {
        let mut store = state.profiles.lock().unwrap();
        if let Some(mut profile) = store.get(&id).cloned() {
            profile.hotkey = previous;
            store.upsert(profile)?;
        }
        drop(store);
        hotkeys::sync(&app);
        return Err(CommandError::Hotkey(error));
    }

    let profile = state
        .profiles
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .expect("profile exists");
    tray::refresh(&app);
    Ok(profile)
}

/// Suspends global hotkeys while the UI records a new combination, so
/// pressing an already bound one does not switch profiles.
#[tauri::command]
pub fn set_hotkeys_paused(app: AppHandle, paused: bool) -> CommandResult<()> {
    if paused {
        hotkeys::unregister_all(&app)
    } else {
        hotkeys::sync(&app);
        Ok(())
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle, state: State<'_, AppState>) -> Settings {
    let mut settings = state.settings.lock().unwrap().clone();
    // The OS entry is the source of truth; it can be removed outside the app.
    if let Ok(enabled) = app.autolaunch().is_enabled() {
        settings.autostart = enabled;
    }
    settings
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CommandResult<Settings> {
    let settings = settings.sanitized();
    let autolaunch = app.autolaunch();
    let result = if settings.autostart {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    if let Err(e) = result {
        log::warn!("autostart: {e}");
    }
    settings.save(&paths::settings_file())?;
    *state.settings.lock().unwrap() = settings.clone();
    // The reset combination may have been switched on or off.
    hotkeys::sync(&app);
    Ok(settings)
}

/// Writes all profiles to a JSON file chosen by the user. Returns the path,
/// or `None` if the dialog was cancelled.
#[tauri::command]
pub async fn export_profiles(app: AppHandle) -> CommandResult<Option<String>> {
    blocking(app, |app, s| {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name("display-changer-profiles.json")
            .add_filter("JSON", &["json"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|e| CommandError::Storage(std::io::Error::other(e.to_string())))?;
        let profiles = s.profiles.lock().unwrap().all().to_vec();
        let json = serde_json::to_string_pretty(&profiles).map_err(std::io::Error::other)?;
        std::fs::write(&path, json)?;
        Ok(Some(path.display().to_string()))
    })
    .await
}

/// Adds profiles from a JSON file chosen by the user. Returns how many were added.
#[tauri::command]
pub async fn import_profiles(app: AppHandle) -> CommandResult<usize> {
    blocking(app, |app, s| {
        let Some(path) = app
            .dialog()
            .file()
            .add_filter("JSON", &["json"])
            .blocking_pick_file()
        else {
            return Ok(0);
        };
        let path = path
            .into_path()
            .map_err(|e| CommandError::Storage(std::io::Error::other(e.to_string())))?;
        let text = std::fs::read_to_string(path)?;
        let imported: Vec<Profile> = serde_json::from_str(&text).map_err(|e| {
            CommandError::Storage(std::io::Error::other(format!("not a profile file: {e}")))
        })?;
        let count = merge_imported(&mut s.profiles.lock().unwrap(), imported)?;
        hotkeys::sync(app);
        tray::refresh(app);
        Ok(count)
    })
    .await
}

#[tauri::command]
pub async fn identify_monitors(app: AppHandle) -> CommandResult<()> {
    blocking(app, |app, _| identify::show(app)).await
}

/// The emergency reset, also reachable from the UI and the tray.
#[tauri::command]
pub async fn reset_layout(app: AppHandle) -> CommandResult<()> {
    blocking(app, |app, _| crate::reset::run(app)).await
}
