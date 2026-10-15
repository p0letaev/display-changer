//! Polls the display state to notice hotplug and changes made elsewhere
//! (system settings, the CLI), and switches profiles automatically.
//!
//! Polling keeps this identical across X11, Wayland and Windows; reading the
//! state is cheap on every backend.

use std::time::Duration;

use display_core::profile::{resolve, same_layout};
use display_core::{DisplayState, MonitorId, ProfileWarning};
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::apply::{ChangeReason, notify};
use crate::{quick, tray};

const INTERVAL: Duration = Duration::from_secs(2);

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<DisplayState> = None;
        loop {
            std::thread::sleep(INTERVAL);
            let state = app.state::<AppState>();
            let Ok(now) = state.backend.current_state() else {
                continue;
            };
            if let Some(prev) = &last {
                if monitor_set(prev) != monitor_set(&now) {
                    on_monitors_changed(&app, &now);
                    // The auto-applied profile changes the layout again.
                    last = state.backend.current_state().ok();
                    continue;
                }
                if !same_layout(&prev.layout, &now.layout) && !state.apply.is_pending() {
                    notify(&app, now.clone(), ChangeReason::External, None);
                    tray::refresh(&app);
                }
            }
            last = Some(now);
        }
    });
}

fn monitor_set(state: &DisplayState) -> Vec<MonitorId> {
    let mut ids: Vec<MonitorId> = state.monitors.iter().map(|m| m.id.clone()).collect();
    ids.sort_by(|a, b| a.connector.cmp(&b.connector));
    ids
}

fn on_monitors_changed(app: &AppHandle, now: &DisplayState) {
    log::info!(
        "monitors changed: {:?}",
        now.monitors
            .iter()
            .map(|m| &m.id.connector)
            .collect::<Vec<_>>()
    );
    notify(app, now.clone(), ChangeReason::Monitors, None);
    tray::refresh(app);

    let state = app.state::<AppState>();
    if !state.settings.lock().unwrap().auto_switch || state.apply.is_pending() {
        return;
    }
    if let Some(id) = auto_profile(&state, now) {
        quick::run(app, &id, quick::Source::Auto);
        tray::refresh(app);
    }
}

/// First auto-apply profile made for exactly the connected monitors.
fn auto_profile(state: &AppState, now: &DisplayState) -> Option<String> {
    let caps = state.backend.capabilities();
    let profiles = state.profiles.lock().unwrap().all().to_vec();
    profiles
        .into_iter()
        .filter(|p| p.auto_apply)
        .find(|p| {
            let resolved = resolve(&p.layout, now, &caps);
            let exact = !resolved.warnings.iter().any(|w| {
                matches!(
                    w,
                    ProfileWarning::MissingMonitor { .. } | ProfileWarning::ExtraMonitor { .. }
                )
            });
            exact && !same_layout(&resolved.layout, &now.layout)
        })
        .map(|p| p.id)
}
