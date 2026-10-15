use display_core::profile::{resolve, same_layout};
use display_core::{Capabilities, DisplayState, Error, Layout, Profile, Resolved};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::apply::{ApplyOutcome, revert_and_notify};
use crate::error::{CommandError, CommandResult};
use crate::hotkeys;
use crate::profiles::new_profile_id;

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
        s.apply.apply_tentative(app, s.backend.as_ref(), &layout)
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
                active: same_layout(
                    &resolve(&profile.layout, &current, &caps).layout,
                    &current.layout,
                ),
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
        s.apply
            .apply_tentative(app, s.backend.as_ref(), &resolved.layout)
    })
    .await
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, AppState>,
    name: String,
    layout: Layout,
) -> CommandResult<Profile> {
    let profile = Profile {
        id: new_profile_id(),
        name,
        hotkey: None,
        layout,
    };
    state.profiles.lock().unwrap().upsert(profile.clone())?;
    Ok(profile)
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    layout: Option<Layout>,
) -> CommandResult<Profile> {
    let mut store = state.profiles.lock().unwrap();
    let mut profile = store.get(&id).cloned().ok_or(CommandError::NotFound(id))?;
    if let Some(name) = name {
        profile.name = name;
    }
    if let Some(layout) = layout {
        profile.layout = layout;
    }
    store.upsert(profile.clone())?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_profile(app: AppHandle, state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let removed = state.profiles.lock().unwrap().remove(&id)?;
    if !removed {
        return Err(CommandError::NotFound(id));
    }
    hotkeys::sync(&app);
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

    let store = state.profiles.lock().unwrap();
    Ok(store.get(&id).cloned().expect("profile exists"))
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
