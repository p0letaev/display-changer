//! Concrete backends and runtime selection.

pub mod mock;
#[cfg(windows)]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod x11;
#[cfg(target_os = "linux")]
pub mod xfce;

use crate::backend::DisplayBackend;

/// Environment variable that forces a backend by name: `mock` or `x11`.
pub const BACKEND_ENV: &str = "DISPLAY_CHANGER_BACKEND";

/// Picks the backend for the current session.
///
/// Wayland compositors will be probed by protocol support (wlr / KDE globals,
/// Mutter on D-Bus); until those exist, Wayland falls back to mock.
pub fn detect() -> Box<dyn DisplayBackend> {
    let forced = std::env::var(BACKEND_ENV).ok();
    if forced.as_deref() == Some("mock") {
        return Box::new(mock::MockBackend::new());
    }

    #[cfg(windows)]
    match windows::WindowsBackend::new() {
        Ok(backend) => return Box::new(backend),
        Err(e) => log::warn!("Windows backend unavailable: {e}"),
    }

    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    if forced.as_deref() == Some("x11") || (x11 && !wayland) {
        match connect_x11() {
            Ok(backend) => return backend,
            Err(e) => log::warn!("X11 backend unavailable: {e}"),
        }
    } else if wayland {
        log::warn!("Wayland backends are not implemented yet");
    }
    if let Some(other) = forced.as_deref().filter(|f| !["mock", "x11"].contains(f)) {
        log::warn!("unknown backend {other:?}");
    }
    log::info!("falling back to the demo backend");
    Box::new(mock::MockBackend::new())
}

#[cfg(target_os = "linux")]
fn connect_x11() -> crate::Result<Box<dyn DisplayBackend>> {
    Ok(Box::new(x11::X11Backend::connect()?))
}

#[cfg(not(target_os = "linux"))]
fn connect_x11() -> crate::Result<Box<dyn DisplayBackend>> {
    Err(crate::Error::Unsupported(
        "X11 backend is Linux-only".into(),
    ))
}
