//! Concrete backends and runtime selection.

#[cfg(target_os = "linux")]
pub mod gnome;
#[cfg(target_os = "linux")]
pub mod kde;
#[cfg(target_os = "linux")]
mod kde_protocols;
pub mod mock;
#[cfg(target_os = "linux")]
mod wayland_util;
#[cfg(windows)]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod wlr;
#[cfg(target_os = "linux")]
pub mod x11;
#[cfg(target_os = "linux")]
pub mod xfce;

use crate::backend::DisplayBackend;

/// Environment variable that forces a backend by name: `mock`, `x11`,
/// `wlroots`, `kde` or `gnome`.
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
        match connect_wayland(forced.as_deref()) {
            Ok(backend) => return backend,
            Err(e) => log::warn!("no Wayland backend for this compositor: {e}"),
        }
    }
    if let Some(other) = forced
        .as_deref()
        .filter(|f| !["mock", "x11", "wlroots", "kde", "gnome"].contains(f))
    {
        log::warn!("unknown backend {other:?}");
    }
    log::info!("falling back to the demo backend");
    Box::new(mock::MockBackend::new())
}

/// Probes by protocol support rather than by desktop name, so unknown
/// compositors that implement a supported protocol just work.
#[cfg(target_os = "linux")]
fn connect_wayland(forced: Option<&str>) -> crate::Result<Box<dyn DisplayBackend>> {
    let wants = |name: &str| forced.is_none() || forced == Some(name);
    if wants("wlroots") {
        match wlr::WlrBackend::connect() {
            Ok(b) => return Ok(Box::new(b)),
            Err(e) => log::debug!("wlroots: {e}"),
        }
    }
    if wants("kde") {
        match kde::KdeBackend::connect() {
            Ok(b) => return Ok(Box::new(b)),
            Err(e) => log::debug!("kde: {e}"),
        }
    }
    if wants("gnome") {
        match gnome::GnomeBackend::connect() {
            Ok(b) => return Ok(Box::new(b)),
            Err(e) => log::debug!("gnome: {e}"),
        }
    }
    Err(crate::Error::Unsupported(
        "no supported output-management protocol".into(),
    ))
}

#[cfg(not(target_os = "linux"))]
fn connect_wayland(_: Option<&str>) -> crate::Result<Box<dyn DisplayBackend>> {
    Err(crate::Error::Unsupported("Wayland is Linux-only".into()))
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
