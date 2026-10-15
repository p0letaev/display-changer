//! Applying layouts with an automatic revert, like the OS "Keep changes?" dialog.
//!
//! The timer lives here rather than in the UI, so a frozen or hidden window
//! still gets the previous layout back.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use display_core::{DisplayBackend, DisplayState, Layout};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::error::CommandResult;

pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(15);
pub const CHANGED_EVENT: &str = "display://changed";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub state: DisplayState,
    pub token: u64,
    pub timeout_secs: u64,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeReason {
    Reverted,
    Hotkey,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayChanged {
    pub state: DisplayState,
    pub reason: ChangeReason,
    pub profile_name: Option<String>,
}

struct Pending {
    token: u64,
    /// Last confirmed layout; kept across repeated applies.
    previous: Layout,
}

#[derive(Default)]
pub struct ApplyManager {
    pending: Mutex<Option<Pending>>,
    counter: AtomicU64,
}

impl ApplyManager {
    /// Applies `layout` and reverts it after [`CONFIRM_TIMEOUT`] unless
    /// [`ApplyManager::confirm`] is called with the returned token.
    pub fn apply_tentative(
        &self,
        app: &AppHandle,
        backend: &dyn DisplayBackend,
        layout: &Layout,
    ) -> CommandResult<ApplyOutcome> {
        let current = backend.current_state()?.layout;
        backend.apply(layout)?;

        let token = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let mut pending = self.pending.lock().unwrap();
            let previous = pending.take().map_or(current, |p| p.previous);
            *pending = Some(Pending { token, previous });
        }

        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(CONFIRM_TIMEOUT);
            let state = app.state::<AppState>();
            if let Some(previous) = state.apply.take(token) {
                log::info!("apply {token} not confirmed, reverting");
                revert_and_notify(&app, state.backend.as_ref(), &previous);
            }
        });

        Ok(ApplyOutcome {
            state: backend.current_state()?,
            token,
            timeout_secs: CONFIRM_TIMEOUT.as_secs(),
        })
    }

    /// Applies without a confirmation window and drops any pending revert.
    /// Used for hotkeys: profiles were saved from a working setup.
    pub fn apply_final(&self, backend: &dyn DisplayBackend, layout: &Layout) -> CommandResult<()> {
        backend.apply(layout)?;
        *self.pending.lock().unwrap() = None;
        Ok(())
    }

    pub fn confirm(&self, token: u64) -> bool {
        self.take(token).is_some()
    }

    /// Removes the pending apply if `token` is still current.
    pub fn take(&self, token: u64) -> Option<Layout> {
        let mut pending = self.pending.lock().unwrap();
        if pending.as_ref().is_some_and(|p| p.token == token) {
            pending.take().map(|p| p.previous)
        } else {
            None
        }
    }
}

pub fn revert_and_notify(app: &AppHandle, backend: &dyn DisplayBackend, previous: &Layout) {
    if let Err(e) = backend.apply(previous) {
        log::error!("revert failed: {e}");
    }
    match backend.current_state() {
        Ok(state) => {
            let _ = app.emit(
                CHANGED_EVENT,
                DisplayChanged {
                    state,
                    reason: ChangeReason::Reverted,
                    profile_name: None,
                },
            );
        }
        Err(e) => log::error!("cannot read state after revert: {e}"),
    }
}
