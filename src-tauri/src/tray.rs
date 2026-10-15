//! Tray icon with one menu entry per profile.

use display_core::profile::{resolve, same_layout};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::AppState;
use crate::quick;

const TRAY_ID: &str = "main";
const PROFILE_PREFIX: &str = "profile:";

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let icon = app
        .default_window_icon()
        .cloned()
        .expect("bundled window icon");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Display Changer")
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuilds the menu after profiles or the active layout changed.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match menu(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => log::warn!("tray menu: {e}"),
    }
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let current = state.backend.current_state().ok();
    let caps = state.backend.capabilities();
    let profiles = state.profiles.lock().unwrap().all().to_vec();

    let menu = Menu::new(app)?;
    for p in &profiles {
        let active = current
            .as_ref()
            .is_some_and(|c| same_layout(&resolve(&p.layout, c, &caps).layout, &c.layout));
        let label = match &p.hotkey {
            Some(h) => format!(
                "{}    {}",
                p.name,
                h.replace("Digit", "").replace("Key", "")
            ),
            None => p.name.clone(),
        };
        let id = format!("{PROFILE_PREFIX}{}", p.id);
        menu.append(&CheckMenuItem::with_id(
            app,
            id,
            label,
            true,
            active,
            None::<&str>,
        )?)?;
    }
    if profiles.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "none",
            "Профилей пока нет",
            false,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "show",
        "Открыть Display Changer",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Выход",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "show" => show_main_window(app),
        "quit" => app.exit(0),
        _ => {
            if let Some(profile) = id.strip_prefix(PROFILE_PREFIX) {
                quick::spawn(app, profile.to_string(), quick::Source::Tray);
            }
        }
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
