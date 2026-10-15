//! GNOME (Mutter) backend over D-Bus `org.gnome.Mutter.DisplayConfig`.
//!
//! Mutter groups monitors into logical monitors; mirrored monitors share one.
//! This backend exposes each physical monitor separately and gives every
//! enabled one its own logical monitor when applying.

use std::collections::HashMap;

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

use super::wayland_util::model_code;
use crate::backend::{Capabilities, DisplayBackend, OriginRule};
use crate::error::{Error, Result};
use crate::layout;
use crate::model::{
    DisplayState, Layout, Mode, MonitorId, MonitorInfo, OutputConfig, Position, Rotation,
};

const SERVICE: &str = "org.gnome.Mutter.DisplayConfig";
const PATH: &str = "/org/gnome/Mutter/DisplayConfig";

type Props = HashMap<String, OwnedValue>;
/// (connector, vendor, product, serial)
type MonitorSpec = (String, String, String, String);
/// (id, width, height, refresh Hz, preferred scale, supported scales, props)
type ModeT = (String, i32, i32, f64, f64, Vec<f64>, Props);
type MonitorT = (MonitorSpec, Vec<ModeT>, Props);
/// (x, y, scale, transform, primary, monitors, props)
type LogicalT = (i32, i32, f64, u32, bool, Vec<MonitorSpec>, Props);
type CurrentState = (u32, Vec<MonitorT>, Vec<LogicalT>, Props);

/// Mutter's `layout-mode` property: logical (scaled) or physical pixels.
const LAYOUT_MODE_PHYSICAL: u32 = 2;

const METHOD_VERIFY: u32 = 0;
const METHOD_PERSISTENT: u32 = 2;

fn flag(props: &Props, key: &str) -> bool {
    props
        .get(key)
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false)
}

fn refresh_mhz(hz: f64) -> u32 {
    (hz * 1000.0).round() as u32
}

/// Mutter transforms match wl_output: 1 = 90° counter-clockwise.
fn rotation_from(t: u32) -> Rotation {
    match t % 4 {
        1 => Rotation::Left,
        2 => Rotation::Inverted,
        3 => Rotation::Right,
        _ => Rotation::Normal,
    }
}

fn transform_from(r: Rotation) -> u32 {
    match r {
        Rotation::Normal => 0,
        Rotation::Left => 1,
        Rotation::Inverted => 2,
        Rotation::Right => 3,
    }
}

pub struct GnomeBackend {
    conn: Connection,
    scaled_layout: bool,
}

impl GnomeBackend {
    pub fn connect() -> Result<Self> {
        let conn =
            Connection::session().map_err(|e| Error::Backend(format!("session bus: {e}")))?;
        let mut this = Self {
            conn,
            scaled_layout: true,
        };
        let (_, _, _, props) = this.current().map_err(|_| {
            Error::Unsupported("org.gnome.Mutter.DisplayConfig is not available".into())
        })?;
        this.scaled_layout = props
            .get("layout-mode")
            .and_then(|v| u32::try_from(v).ok())
            .is_none_or(|m| m != LAYOUT_MODE_PHYSICAL);
        Ok(this)
    }

    fn current(&self) -> Result<CurrentState> {
        self.conn
            .call_method(Some(SERVICE), PATH, Some(SERVICE), "GetCurrentState", &())
            .and_then(|r| r.body().deserialize::<CurrentState>())
            .map_err(|e| Error::Backend(format!("GetCurrentState: {e}")))
    }

    fn to_state(raw: &CurrentState) -> DisplayState {
        let (_, monitors, logical, _) = raw;
        let mut infos = Vec::new();
        let mut outputs = Vec::new();
        for (spec, modes, props) in monitors {
            let (connector, vendor, product, serial) = spec;
            let current = modes.iter().find(|m| flag(&m.6, "is-current"));
            let preferred = modes.iter().find(|m| flag(&m.6, "is-preferred"));
            let to_mode = |m: &ModeT| Mode {
                width: m.1 as u32,
                height: m.2 as u32,
                refresh_mhz: refresh_mhz(m.3),
            };
            let mut list: Vec<Mode> = modes.iter().map(to_mode).collect();
            list.dedup();
            let Some(base) = current.or(preferred).or(modes.first()) else {
                continue;
            };

            let id = MonitorId {
                vendor: vendor.clone(),
                product: model_code(product),
                serial: Some(serial.clone()).filter(|s| !s.is_empty() && s != "0x00000000"),
                connector: connector.clone(),
            };
            let display_name = props
                .get("display-name")
                .and_then(|v| String::try_from(v.clone()).ok());
            infos.push(MonitorInfo {
                id: id.clone(),
                name: display_name.unwrap_or_else(|| {
                    if product.is_empty() {
                        connector.clone()
                    } else {
                        product.clone()
                    }
                }),
                builtin: flag(props, "is-builtin"),
                physical_size: None,
                modes: list,
                preferred_mode: preferred.map(to_mode),
                rotations: Rotation::ALL.to_vec(),
                scales: base.5.clone(),
                custom_modes: false,
            });

            let lm = logical
                .iter()
                .find(|l| l.5.iter().any(|s| &s.0 == connector));
            outputs.push(match lm {
                Some(l) => {
                    // Several monitors in one logical monitor mirror the first.
                    let mirror_of =
                        l.5.first()
                            .map(|s| s.0.clone())
                            .filter(|first| first != connector);
                    OutputConfig {
                        id,
                        enabled: true,
                        position: Position { x: l.0, y: l.1 },
                        mode: to_mode(base),
                        scale: l.2,
                        rotation: rotation_from(l.3),
                        primary: l.4 && mirror_of.is_none(),
                        mirror_of,
                    }
                }
                None => OutputConfig {
                    id,
                    enabled: false,
                    position: Position::default(),
                    mode: to_mode(base),
                    scale: base.4.max(1.0),
                    rotation: Rotation::Normal,
                    primary: false,
                    mirror_of: None,
                },
            });
        }
        DisplayState {
            monitors: infos,
            layout: Layout { outputs },
        }
    }

    fn apply_with(&self, raw: &CurrentState, target: &Layout, method: u32) -> Result<()> {
        let (serial, monitors, _, _) = raw;
        type LogicalArg<'a> = (
            i32,
            i32,
            f64,
            u32,
            bool,
            Vec<(String, String, HashMap<String, Value<'a>>)>,
        );
        let mode_id = |o: &crate::model::OutputConfig| -> Result<String> {
            let (_, modes, _) = monitors
                .iter()
                .find(|(spec, ..)| spec.0 == o.id.connector)
                .ok_or_else(|| Error::Backend(format!("{} disappeared", o.id.connector)))?;
            modes
                .iter()
                .find(|m| {
                    m.1 as u32 == o.mode.width
                        && m.2 as u32 == o.mode.height
                        && refresh_mhz(m.3) == o.mode.refresh_mhz
                })
                .map(|m| m.0.clone())
                .ok_or_else(|| Error::Backend(format!("{}: mode vanished", o.id.connector)))
        };
        // One logical monitor per placed output; its mirrors join it.
        let mut logical: Vec<LogicalArg> = Vec::new();
        for o in target.placed() {
            let mut members = vec![(o.id.connector.clone(), mode_id(o)?, HashMap::new())];
            for m in target
                .enabled()
                .filter(|m| m.mirror_of.as_deref() == Some(&o.id.connector))
            {
                members.push((m.id.connector.clone(), mode_id(m)?, HashMap::new()));
            }
            logical.push((
                o.position.x,
                o.position.y,
                o.scale,
                transform_from(o.rotation),
                o.primary,
                members,
            ));
        }
        let props: HashMap<String, Value> = HashMap::new();
        self.conn
            .call_method(
                Some(SERVICE),
                PATH,
                Some(SERVICE),
                "ApplyMonitorsConfig",
                &(*serial, method, logical, props),
            )
            .map(|_| ())
            .map_err(|e| Error::Backend(format!("Mutter rejected the configuration: {e}")))
    }
}

impl DisplayBackend for GnomeBackend {
    fn name(&self) -> &'static str {
        "gnome"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            scale: true,
            fractional_scale: true,
            rotation: true,
            primary: true,
            disable: true,
            origin: OriginRule::TopLeft,
            scaled_layout: self.scaled_layout,
            mirror: true,
        }
    }

    fn current_state(&self) -> Result<DisplayState> {
        Ok(Self::to_state(&self.current()?))
    }

    fn apply(&self, layout: &Layout) -> Result<()> {
        let raw = self.current()?;
        layout::validate(layout, &Self::to_state(&raw).monitors, &self.capabilities())?;
        let target = layout::normalize(layout, OriginRule::TopLeft);
        // Verify first: Mutter explains what is wrong without touching anything.
        self.apply_with(&raw, &target, METHOD_VERIFY)?;
        self.apply_with(&raw, &target, METHOD_PERSISTENT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_round_trip() {
        for r in Rotation::ALL {
            assert_eq!(rotation_from(transform_from(r)), r);
        }
        assert_eq!(rotation_from(5), Rotation::Left); // flipped-90
    }

    #[test]
    fn refresh_rounds_to_millihertz() {
        assert_eq!(refresh_mhz(59.940_475_463_867_19), 59_940);
        assert_eq!(refresh_mhz(60.0), 60_000);
    }
}
