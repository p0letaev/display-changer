use serde::{Deserialize, Serialize};

/// Identifies a physical monitor across reconnects.
///
/// `vendor`, `product` and `serial` come from EDID and follow the monitor when
/// it moves to another port. `connector` is only stable within a session and
/// is used as a fallback when two identical monitors have no serial.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorId {
    /// Three-letter PNP manufacturer code, e.g. `DEL`.
    pub vendor: String,
    pub product: u16,
    pub serial: Option<String>,
    /// Port name as reported by the platform, e.g. `DP-1` or `HDMI-A-1`.
    pub connector: String,
}

/// A video mode. Refresh rate is stored in millihertz so modes compare exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Rotation {
    #[default]
    Normal,
    /// 90° counter-clockwise.
    Left,
    Inverted,
    /// 90° clockwise.
    Right,
}

impl Rotation {
    pub const ALL: [Rotation; 4] = [
        Rotation::Normal,
        Rotation::Left,
        Rotation::Inverted,
        Rotation::Right,
    ];

    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Left | Rotation::Right)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalSize {
    pub width_mm: u32,
    pub height_mm: u32,
}

/// Static information about a connected monitor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: MonitorId,
    /// Human-readable name, usually the EDID product name.
    pub name: String,
    pub builtin: bool,
    pub physical_size: Option<PhysicalSize>,
    pub modes: Vec<Mode>,
    pub preferred_mode: Option<Mode>,
    /// Rotations the hardware can do for this monitor.
    pub rotations: Vec<Rotation>,
    /// Scales the platform allows for this monitor; empty means any value
    /// permitted by the backend capabilities.
    pub scales: Vec<f64>,
    /// Accepts arbitrary modes (virtual outputs); `modes` are suggestions.
    #[serde(default)]
    pub custom_modes: bool,
}

/// Desired configuration of a single output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    pub id: MonitorId,
    pub enabled: bool,
    /// Top-left corner in logical coordinates.
    pub position: Position,
    pub mode: Mode,
    pub scale: f64,
    pub rotation: Rotation,
    pub primary: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub outputs: Vec<OutputConfig>,
}

impl Layout {
    pub fn output(&self, connector: &str) -> Option<&OutputConfig> {
        self.outputs.iter().find(|o| o.id.connector == connector)
    }

    pub fn enabled(&self) -> impl Iterator<Item = &OutputConfig> {
        self.outputs.iter().filter(|o| o.enabled)
    }
}

/// What the platform currently reports: connected monitors and their layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayState {
    pub monitors: Vec<MonitorInfo>,
    pub layout: Layout,
}

impl DisplayState {
    pub fn monitor(&self, connector: &str) -> Option<&MonitorInfo> {
        self.monitors.iter().find(|m| m.id.connector == connector)
    }
}
