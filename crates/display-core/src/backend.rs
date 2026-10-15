use serde::Serialize;

use crate::error::Result;
use crate::layout;
use crate::model::{DisplayState, Layout};

/// How a backend expects coordinates to be anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OriginRule {
    /// The bounding box starts at (0, 0); coordinates are never negative (X11).
    TopLeft,
    /// The primary output sits at (0, 0); others may be negative (Windows).
    Primary,
}

/// Feature flags the UI uses to hide or lock controls a backend cannot apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub scale: bool,
    pub fractional_scale: bool,
    pub rotation: bool,
    pub primary: bool,
    pub disable: bool,
    pub origin: OriginRule,
}

pub trait DisplayBackend: Send + Sync {
    /// Short identifier, e.g. `x11-randr` or `mock`.
    fn name(&self) -> &'static str;

    fn capabilities(&self) -> Capabilities;

    fn current_state(&self) -> Result<DisplayState>;

    fn validate(&self, layout: &Layout) -> Result<()> {
        let state = self.current_state()?;
        layout::validate(layout, &state.monitors, &self.capabilities())
    }

    /// Applies the layout. Implementations must call [`DisplayBackend::validate`]
    /// and [`layout::normalize`] first.
    fn apply(&self, layout: &Layout) -> Result<()>;
}
