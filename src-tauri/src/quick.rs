//! Applying a profile without the editor: hotkeys, the tray menu and
//! automatic switching. Runs on a background thread.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::apply::{ChangeReason, PENDING_EVENT, notify};
use crate::commands::resolve_profile;

pub const FAILED_EVENT: &str = "display://quick-failed";

#[derive(Clone, Copy, Debug)]
pub enum Source {
    Hotkey,
    Tray,
    Auto,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QuickFailed {
    profile_name: String,
    error: String,
}

pub fn spawn(app: &AppHandle, profile_id: String, source: Source) {
    let app = app.clone();
    std::thread::spawn(move || run(&app, &profile_id, source));
}

/// Profiles were saved from a working setup, so by default they apply at
/// once; with `confirm_quick_apply` the window opens with the usual countdown.
pub fn run(app: &AppHandle, profile_id: &str, source: Source) {
    let state = app.state::<AppState>();
    let name = state
        .profiles
        .lock()
        .unwrap()
        .get(profile_id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let settings = state.settings.lock().unwrap().clone();

    let result = resolve_profile(&state, profile_id).and_then(|resolved| {
        if settings.confirm_quick_apply {
            let timeout = Duration::from_secs(settings.confirm_timeout_secs);
            let outcome = state.apply.apply_tentative(
                app,
                state.backend.as_ref(),
                &resolved.layout,
                timeout,
            )?;
            crate::tray::show_main_window(app);
            let _ = app.emit(PENDING_EVENT, outcome);
            Ok(None)
        } else {
            state
                .apply
                .apply_final(state.backend.as_ref(), &resolved.layout)?;
            Ok(Some(state.backend.current_state()?))
        }
    });

    match result {
        Ok(Some(display)) => {
            log::info!("{source:?} applied profile {name:?}");
            let reason = if matches!(source, Source::Auto) {
                ChangeReason::Auto
            } else {
                ChangeReason::Quick
            };
            notify(app, display, reason, Some(name));
        }
        Ok(None) => log::info!("{source:?} applied profile {name:?}, waiting for confirmation"),
        Err(error) => {
            log::warn!("{source:?} profile {name:?} failed: {error}");
            let _ = app.emit(
                FAILED_EVENT,
                QuickFailed {
                    profile_name: name,
                    error: error.to_string(),
                },
            );
        }
    }
}
