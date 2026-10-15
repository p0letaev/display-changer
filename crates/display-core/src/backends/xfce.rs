//! XFCE keeps its own copy of the display layout in xfconf (channel
//! `displays`) and re-applies it on hotplug and login. Without updating it,
//! XFCE would bring back its older layout the next time a cable moves.
//!
//! Properties follow what xfce4-display-settings writes:
//! `/<profile>/<connector>/{Active,EDID,Position/X,Resolution,...}`.

use std::collections::HashMap;
use std::path::PathBuf;

use x11rb::protocol::randr::{ModeFlag, ModeInfo};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

use crate::edid;
use crate::model::{OutputConfig, Rotation};

const SERVICE: &str = "org.xfce.Xfconf";
const PATH: &str = "/org/xfce/Xfconf";
const INTERFACE: &str = "org.xfce.Xfconf";
const CHANNEL: &str = "displays";

/// Set to any value to never touch xfconf (e.g. when testing on a nested X server).
pub const OPT_OUT_ENV: &str = "DISPLAY_CHANGER_NO_XFCONF";

#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Bool(bool),
    Int(i32),
    Double(f64),
    Str(String),
}

impl PropValue {
    fn to_value(&self) -> Value<'_> {
        match self {
            PropValue::Bool(v) => Value::from(*v),
            PropValue::Int(v) => Value::from(*v),
            PropValue::Double(v) => Value::from(*v),
            PropValue::Str(v) => Value::from(v.as_str()),
        }
    }
}

/// One output as XFCE stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct XfceOutput {
    pub connector: String,
    pub name: String,
    pub edid_hash: String,
    pub active: bool,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
    pub rotation: i32,
    pub resolution: String,
    pub refresh_hz: f64,
    pub mode_flags: i32,
}

impl XfceOutput {
    pub fn new(output: &OutputConfig, raw_edid: &[u8], mode: &ModeInfo) -> Self {
        let name = edid::parse(raw_edid)
            .and_then(|e| e.name)
            .unwrap_or_else(|| output.id.connector.clone());
        // XFCE hashes the 128-byte base block only, not the extensions.
        let base = &raw_edid[..raw_edid.len().min(128)];
        let edid_hash = if base.is_empty() {
            String::new()
        } else {
            sha1_smol::Sha1::from(base).digest().to_string()
        };
        Self {
            connector: output.id.connector.clone(),
            name,
            edid_hash,
            active: output.enabled,
            x: output.position.x,
            y: output.position.y,
            primary: output.primary,
            rotation: match output.rotation {
                Rotation::Normal => 0,
                Rotation::Left => 90,
                Rotation::Inverted => 180,
                Rotation::Right => 270,
            },
            resolution: format!("{}x{}", output.mode.width, output.mode.height),
            refresh_hz: exact_refresh(mode),
            mode_flags: u32::from(mode.mode_flags) as i32,
        }
    }

    /// Property paths relative to the profile, with values.
    pub fn properties(&self) -> Vec<(String, PropValue)> {
        let c = &self.connector;
        let mut props = vec![
            (c.clone(), PropValue::Str(self.name.clone())),
            (format!("{c}/EDID"), PropValue::Str(self.edid_hash.clone())),
            (format!("{c}/Active"), PropValue::Bool(self.active)),
        ];
        if self.active {
            props.extend([
                (format!("{c}/Position/X"), PropValue::Int(self.x)),
                (format!("{c}/Position/Y"), PropValue::Int(self.y)),
                (format!("{c}/Primary"), PropValue::Bool(self.primary)),
                (format!("{c}/Rotation"), PropValue::Int(self.rotation)),
                (
                    format!("{c}/Resolution"),
                    PropValue::Str(self.resolution.clone()),
                ),
                (
                    format!("{c}/RefreshRate"),
                    PropValue::Double(self.refresh_hz),
                ),
                (format!("{c}/ModeFlags"), PropValue::Int(self.mode_flags)),
            ]);
        }
        props
    }
}

/// Refresh rate exactly as xrandr and XFCE compute it, so XFCE finds the mode.
fn exact_refresh(m: &ModeInfo) -> f64 {
    let mut vtotal = f64::from(m.vtotal);
    if m.mode_flags.contains(ModeFlag::DOUBLE_SCAN) {
        vtotal *= 2.0;
    }
    if m.mode_flags.contains(ModeFlag::INTERLACE) {
        vtotal /= 2.0;
    }
    if m.htotal == 0 || vtotal == 0.0 {
        return 0.0;
    }
    f64::from(m.dot_clock) / (f64::from(m.htotal) * vtotal)
}

pub struct XfceDisplays {
    conn: Connection,
}

impl XfceDisplays {
    /// Returns a handle only inside an XFCE session with xfconf reachable.
    pub fn connect() -> Option<Self> {
        if std::env::var_os(OPT_OUT_ENV).is_some() {
            return None;
        }
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if !desktop.split(':').any(|d| d.eq_ignore_ascii_case("XFCE")) {
            return None;
        }
        let conn = Connection::session().ok()?;
        let this = Self { conn };
        // Probe: fails when xfconfd is not running.
        this.call::<bool>("PropertyExists", &(CHANNEL, "/ActiveProfile"))
            .ok()?;
        log::info!("xfconf: will keep XFCE display settings in sync");
        Some(this)
    }

    fn call<R>(
        &self,
        method: &str,
        body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> Result<R, String>
    where
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        self.conn
            .call_method(Some(SERVICE), PATH, Some(INTERFACE), method, body)
            .and_then(|reply| reply.body().deserialize::<R>())
            .map_err(|e| format!("{method}: {e}"))
    }

    fn active_profile(&self) -> String {
        self.call::<OwnedValue>("GetProperty", &(CHANNEL, "/ActiveProfile"))
            .ok()
            .and_then(|v| String::try_from(v).ok())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Default".into())
    }

    pub fn save(&self, outputs: &[XfceOutput]) -> Result<(), String> {
        self.backup_once();
        let profile = self.active_profile();
        for output in outputs {
            for (path, value) in output.properties() {
                let property = format!("/{profile}/{path}");
                self.call::<()>(
                    "SetProperty",
                    &(CHANNEL, property.as_str(), value.to_value()),
                )?;
            }
        }
        log::info!(
            "xfconf: saved {} outputs to profile {profile:?}",
            outputs.len()
        );
        Ok(())
    }

    /// Saves the channel as it was before our first write, for manual recovery.
    fn backup_once(&self) {
        let Some(path) = backup_path() else { return };
        if path.exists() {
            return;
        }
        let props =
            match self.call::<HashMap<String, OwnedValue>>("GetAllProperties", &(CHANNEL, "/")) {
                Ok(props) => props,
                Err(e) => return log::warn!("xfconf backup: {e}"),
            };
        let json: serde_json::Map<String, serde_json::Value> =
            props.into_iter().map(|(k, v)| (k, to_json(&v))).collect();
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(path.parent().expect("has parent"))?;
            std::fs::write(&path, serde_json::to_string_pretty(&json)?)
        };
        match write() {
            Ok(()) => log::info!("xfconf: backup written to {}", path.display()),
            Err(e) => log::warn!("xfconf backup: {e}"),
        }
    }
}

fn backup_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(
        base.join("io.github.displaychanger")
            .join("xfce-displays-backup.json"),
    )
}

fn to_json(v: &Value<'_>) -> serde_json::Value {
    match v {
        Value::Bool(b) => (*b).into(),
        Value::I32(n) => (*n).into(),
        Value::U32(n) => (*n).into(),
        Value::I64(n) => (*n).into(),
        Value::U64(n) => (*n).into(),
        Value::F64(n) => (*n).into(),
        Value::Str(s) => s.as_str().into(),
        Value::Value(inner) => to_json(inner),
        other => format!("{other:?}").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edid::tests::sample;
    use crate::model::{Mode, MonitorId, Position};

    fn output(enabled: bool) -> OutputConfig {
        OutputConfig {
            id: MonitorId {
                vendor: "AOC".into(),
                product: 1,
                serial: None,
                connector: "HDMI-A-0".into(),
            },
            enabled,
            position: Position { x: 0, y: 1080 },
            mode: Mode {
                width: 1920,
                height: 1080,
                refresh_mhz: 60_000,
            },
            scale: 1.0,
            rotation: Rotation::Right,
            primary: true,
            mirror_of: None,
        }
    }

    fn mode_info() -> ModeInfo {
        ModeInfo {
            id: 1,
            width: 1920,
            height: 1080,
            dot_clock: 148_500_000,
            hsync_start: 0,
            hsync_end: 0,
            htotal: 2200,
            hskew: 0,
            vsync_start: 0,
            vsync_end: 0,
            vtotal: 1125,
            name_len: 0,
            mode_flags: ModeFlag::from(5u32),
        }
    }

    #[test]
    fn hashes_base_block_only_and_names_from_edid() {
        let mut raw = sample("AOC", 1, 0, Some("24G2"), None);
        let base_hash = sha1_smol::Sha1::from(&raw).digest().to_string();
        raw.extend([0xAB; 128]); // CEA extension block
        let x = XfceOutput::new(&output(true), &raw, &mode_info());
        assert_eq!(x.edid_hash, base_hash);
        assert_eq!(x.name, "24G2");
        assert_eq!(x.rotation, 270);
        assert_eq!(x.mode_flags, 5);
        assert!((x.refresh_hz - 60.0).abs() < 1e-9);
    }

    #[test]
    fn inactive_outputs_only_write_identity() {
        let x = XfceOutput::new(&output(false), &[], &mode_info());
        let paths: Vec<String> = x.properties().into_iter().map(|(p, _)| p).collect();
        assert_eq!(paths, ["HDMI-A-0", "HDMI-A-0/EDID", "HDMI-A-0/Active"]);
        assert_eq!(x.name, "HDMI-A-0");
    }

    #[test]
    fn active_outputs_write_geometry() {
        let props = XfceOutput::new(&output(true), &[], &mode_info()).properties();
        assert!(props.contains(&("HDMI-A-0/Position/Y".into(), PropValue::Int(1080))));
        assert!(props.contains(&(
            "HDMI-A-0/Resolution".into(),
            PropValue::Str("1920x1080".into())
        )));
        assert!(props.contains(&("HDMI-A-0/Primary".into(), PropValue::Bool(true))));
    }
}
