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

/// Display state changed for a reason other than the UI's own apply.
pub const CHANGED_EVENT: &str = "display://changed";
/// A quick apply is waiting for confirmation; carries an [`ApplyOutcome`].
pub const PENDING_EVENT: &str = "display://pending";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub state: DisplayState,
    pub token: u64,
    pub timeout_secs: u64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeReason {
    /// An unconfirmed change was rolled back.
    Reverted,
    /// A profile was applied from a hotkey or the tray.
    Quick,
    /// A profile was applied because its monitors got connected.
    Auto,
    /// Monitors were connected or disconnected.
    Monitors,
    /// Something else (system settings, the CLI) changed the layout.
    External,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayChanged {
    pub state: DisplayState,
    pub reason: ChangeReason,
    pub profile_name: Option<String>,
}

pub fn notify(
    app: &AppHandle,
    state: DisplayState,
    reason: ChangeReason,
    profile_name: Option<String>,
) {
    let _ = app.emit(
        CHANGED_EVENT,
        DisplayChanged {
            state,
            reason,
            profile_name,
        },
    );
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
    /// Applies `layout` and reverts it after `timeout` unless
    /// [`ApplyManager::confirm`] is called with the returned token.
    pub fn apply_tentative(
        &self,
        app: &AppHandle,
        backend: &dyn DisplayBackend,
        layout: &Layout,
        timeout: Duration,
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
            std::thread::sleep(timeout);
            let state = app.state::<AppState>();
            if let Some(previous) = state.apply.take(token) {
                log::info!("apply {token} not confirmed, reverting");
                revert_and_notify(&app, state.backend.as_ref(), &previous);
            }
        });

        Ok(ApplyOutcome {
            state: backend.current_state()?,
            token,
            timeout_secs: timeout.as_secs(),
        })
    }

    /// Applies without a confirmation window and drops any pending revert.
    pub fn apply_final(&self, backend: &dyn DisplayBackend, layout: &Layout) -> CommandResult<()> {
        backend.apply(layout)?;
        *self.pending.lock().unwrap() = None;
        Ok(())
    }

    pub fn confirm(&self, token: u64) -> bool {
        self.take(token).is_some()
    }

    pub fn is_pending(&self) -> bool {
        self.pending.lock().unwrap().is_some()
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
        Ok(state) => notify(app, state, ChangeReason::Reverted, None),
        Err(e) => log::error!("cannot read state after revert: {e}"),
    }
}
