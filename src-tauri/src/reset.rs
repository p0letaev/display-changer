//! Emergency reset bound to Space + Alt + →: brings back the layout from
//! before the last change, or a safe layout when there is no history (for
//! example right after the app started on a broken setup).

use display_core::layout::desktop_rect;
use display_core::{Capabilities, DisplayState, Layout, OutputConfig, Position, Rotation};
use tauri::{AppHandle, Manager};

use crate::AppState;
use crate::apply::{ChangeReason, notify};
use crate::tray;

/// What the global shortcut grabs; Space is checked as held on press.
pub const ACCELERATOR: &str = "Alt+ArrowRight";

pub fn spawn(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = run(&app) {
            log::error!("reset failed: {e}");
        }
    });
}

pub fn run(app: &AppHandle) -> crate::error::CommandResult<()> {
    let state = app.state::<AppState>();
    let target = match state.apply.take_before_last() {
        Some(previous) => previous,
        None => safe_layout(
            &state.backend.current_state()?,
            &state.backend.capabilities(),
        ),
    };
    log::info!(
        "reset: restoring layout with {} outputs",
        target.outputs.len()
    );
    // Not through apply_final: the reset itself must not become undo history.
    state.backend.apply(&target)?;
    notify(
        app,
        state.backend.current_state()?,
        ChangeReason::Reset,
        None,
    );
    tray::refresh(app);
    Ok(())
}

/// Every connected monitor on, at its preferred mode, 100 % where allowed,
/// unrotated, side by side; the built-in one (or the first) is primary.
pub fn safe_layout(state: &DisplayState, caps: &Capabilities) -> Layout {
    let primary = state.monitors.iter().position(|m| m.builtin).unwrap_or(0);
    let mut x = 0;
    let mut outputs = Vec::new();
    for (i, m) in state.monitors.iter().enumerate() {
        let Some(mode) = m.preferred_mode.or_else(|| m.modes.first().copied()) else {
            continue;
        };
        let scale = if m.scales.is_empty() || m.scales.iter().any(|s| (*s - 1.0).abs() < 1e-6) {
            1.0
        } else {
            m.scales[0]
        };
        let output = OutputConfig {
            id: m.id.clone(),
            enabled: true,
            position: Position { x, y: 0 },
            mode,
            scale,
            rotation: Rotation::Normal,
            primary: caps.primary && i == primary,
            mirror_of: None,
        };
        x += desktop_rect(&output, caps.scaled_layout).width;
        outputs.push(output);
    }
    Layout { outputs }
}

/// Whether Space is held right now. The grab is on Alt + →, so this tells
/// the reset combination apart from a plain Alt + →.
#[cfg(target_os = "linux")]
pub fn space_held() -> bool {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;

    const XK_SPACE: u32 = 0x20;
    let Ok((conn, _)) = x11rb::connect(None) else {
        return false;
    };
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let Ok(Ok(map)) = conn
        .get_keyboard_mapping(min, max - min + 1)
        .map(|c| c.reply())
    else {
        return false;
    };
    let per = usize::from(map.keysyms_per_keycode).max(1);
    let Some(space) = map
        .keysyms
        .chunks(per)
        .position(|syms| syms.contains(&XK_SPACE))
        .map(|i| min as usize + i)
    else {
        return false;
    };
    let Ok(Ok(keys)) = conn.query_keymap().map(|c| c.reply()) else {
        return false;
    };
    keys.keys[space / 8] & (1 << (space % 8)) != 0
}

#[cfg(windows)]
pub fn space_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SPACE};
    // SAFETY: plain Win32 call without pointers.
    (unsafe { GetAsyncKeyState(i32::from(VK_SPACE.0)) } as u16 & 0x8000) != 0
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn space_held() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use display_core::DisplayBackend;
    use display_core::backends::mock::MockBackend;

    #[test]
    fn safe_layout_is_valid_and_side_by_side() {
        let backend = MockBackend::new();
        let state = backend.current_state().unwrap();
        let caps = backend.capabilities();
        let layout = safe_layout(&state, &caps);
        assert_eq!(layout.outputs.len(), state.monitors.len());
        assert!(
            layout
                .outputs
                .iter()
                .all(|o| o.enabled && o.rotation == Rotation::Normal)
        );
        assert_eq!(layout.outputs.iter().filter(|o| o.primary).count(), 1);
        assert!(layout.outputs[0].primary, "built-in panel is primary");
        backend.validate(&layout).unwrap();
    }
}
