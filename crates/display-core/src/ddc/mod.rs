//! DDC/CI: reading and writing monitor settings (VCP codes) over the video
//! cable. Input source is VCP 0x60; vendors put their own features at
//! 0xE0..0xFF.

use std::io;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

/// VCP code of the input source.
pub const INPUT_SOURCE: u8 = 0x60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VcpValue {
    pub current: u16,
    pub maximum: u16,
    /// 0 = set parameter, 1 = momentary (MCCS).
    pub kind: u8,
}

/// A monitor reachable over DDC/CI.
pub struct DdcMonitor {
    /// DRM connector on Linux (`DP-1`), GDI device and description on Windows.
    pub name: String,
    inner: platform::Handle,
}

impl DdcMonitor {
    /// `Ok(None)` when the monitor answers but does not support `code`.
    pub fn get(&mut self, code: u8) -> io::Result<Option<VcpValue>> {
        self.inner.get(code)
    }

    pub fn set(&mut self, code: u8, value: u16) -> io::Result<()> {
        self.inner.set(code, value)
    }

    /// MCCS capabilities string, e.g. `(prot(monitor)...vcp(10 12 60(0F 11 12)))`.
    pub fn capabilities(&mut self) -> io::Result<String> {
        self.inner.capabilities()
    }
}

/// Connected monitors, each with a handle or the reason it is unreachable.
#[cfg(any(target_os = "linux", windows))]
pub fn enumerate() -> Vec<(String, io::Result<DdcMonitor>)> {
    platform::enumerate()
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn enumerate() -> Vec<(String, io::Result<DdcMonitor>)> {
    Vec::new()
}

/// Name of an MCCS input source value; vendors may use others.
pub fn input_name(value: u16) -> Option<&'static str> {
    Some(match value & 0xff {
        0x01 => "VGA-1",
        0x02 => "VGA-2",
        0x03 => "DVI-1",
        0x04 => "DVI-2",
        0x0f => "DisplayPort-1",
        0x10 => "DisplayPort-2",
        0x11 => "HDMI-1",
        0x12 => "HDMI-2",
        0x1b => "USB-C",
        _ => return None,
    })
}
