//! "Identify" shows each monitor's number from the editor on that monitor.

use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::AppState;
use crate::error::{CommandError, CommandResult};

const BOX: f64 = 320.0;
const SHOW_FOR: Duration = Duration::from_secs(3);

/// Opens a small borderless window in the middle of every monitor. Numbers
/// match the editor when monitor positions can be matched (X11, Windows);
/// Wayland does not let clients place windows, so there the order is a guess.
pub fn show(app: &AppHandle) -> CommandResult<()> {
    let outputs = app
        .state::<AppState>()
        .backend
        .current_state()?
        .layout
        .outputs;
    let monitors = app
        .available_monitors()
        .map_err(|e| CommandError::Display(display_core::Error::Backend(e.to_string())))?;

    let mut labels = Vec::new();
    for (i, m) in monitors.iter().enumerate() {
        let pos = m.position();
        let found = outputs
            .iter()
            .enumerate()
            .find(|(_, o)| o.enabled && o.position.x == pos.x && o.position.y == pos.y);
        let (number, name) = match found {
            Some((index, o)) => (index + 1, o.id.connector.clone()),
            None => (i + 1, m.name().cloned().unwrap_or_default()),
        };
        let scale = m.scale_factor();
        let size = m.size();
        let x = f64::from(pos.x) / scale + (f64::from(size.width) / scale - BOX) / 2.0;
        let y = f64::from(pos.y) / scale + (f64::from(size.height) / scale - BOX) / 2.0;
        let label = format!("identify-{i}");
        let script = format!(
            "window.__IDENTIFY__ = {};",
            serde_json::json!({ "number": number, "name": name })
        );
        let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
            .initialization_script(&script)
            .title("Display Changer")
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .position(x, y)
            .inner_size(BOX, BOX)
            .build();
        match built {
            Ok(_) => labels.push(label),
            Err(e) => log::warn!("identify window {i}: {e}"),
        }
    }

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_FOR);
        for label in labels {
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.close();
            }
        }
    });
    Ok(())
}
