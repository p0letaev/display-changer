//! Applying a profile without the editor: hotkeys, the tray menu and
//! automatic switching. Runs on a background thread, applies at once (no
//! countdown — Space + Alt + → undoes it) and ignores repeats while busy.

use std::sync::atomic::{AtomicBool, Ordering};

use display_core::profile::is_active;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::apply::{ChangeReason, notify};
use crate::commands::resolve_profile;
use crate::tray;

pub const FAILED_EVENT: &str = "display://quick-failed";

/// Held keys and key repeat fire the same hotkey several times; one apply at
/// a time keeps the screens from flickering through a queue of requests.
static BUSY: AtomicBool = AtomicBool::new(false);

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

pub fn run(app: &AppHandle, profile_id: &str, source: Source) {
    if BUSY.swap(true, Ordering::AcqRel) {
        log::debug!("{source:?}: another apply is running, ignored");
        return;
    }
    run_exclusive(app, profile_id, source);
    BUSY.store(false, Ordering::Release);
    // Also after a failure: the tray must show what is really active.
    tray::refresh(app);
}

fn run_exclusive(app: &AppHandle, profile_id: &str, source: Source) {
    let state = app.state::<AppState>();
    let name = state
        .profiles
        .lock()
        .unwrap()
        .get(profile_id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let reason = if matches!(source, Source::Auto) {
        ChangeReason::Auto
    } else {
        ChangeReason::Quick
    };

    let saved = state
        .profiles
        .lock()
        .unwrap()
        .get(profile_id)
        .map(|p| p.layout.clone());
    let result = resolve_profile(&state, profile_id).and_then(|resolved| {
        let current = state.backend.current_state()?;
        // Already there: confirm the press without touching the screens.
        if saved.is_some_and(|s| is_active(&s, &current, &state.backend.capabilities())) {
            return Ok(current);
        }
        state
            .apply
            .apply_final(state.backend.as_ref(), &resolved.layout)?;
        Ok(state.backend.current_state()?)
    });

    match result {
        Ok(display) => {
            log::info!("{source:?} applied profile {name:?}");
            notify(app, display, reason, Some(name));
        }
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
