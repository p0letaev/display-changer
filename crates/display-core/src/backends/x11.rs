//! X11 backend over XRandR 1.5. Covers XFCE, Cinnamon, MATE, i3 and any
//! other X11 session.
//!
//! X11 has no per-output scale (only transforms), so scale is always 1 and
//! logical size equals pixel size.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use x11rb::connection::Connection as _;
use x11rb::protocol::randr::{self, ConnectionExt as _, ModeFlag, ModeInfo, SetConfig};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use super::xfce::{self, XfceDisplays};
use crate::backend::{Capabilities, DisplayBackend, OriginRule};
use crate::edid;
use crate::error::{Error, Result};
use crate::layout::{self, bounding_box};
use crate::model::{
    DisplayState, Layout, Mode, MonitorId, MonitorInfo, OutputConfig, PhysicalSize, Position,
    Rotation,
};

/// Screen size in millimetres is derived assuming this DPI, like `xrandr --fb`.
const ASSUMED_DPI: f64 = 96.0;

pub struct X11Backend {
    conn: RustConnection,
    root: xproto::Window,
    edid_atom: xproto::Atom,
    /// Serializes applies; RandR reconfiguration is not atomic.
    apply_lock: Mutex<()>,
    xfce: Option<XfceDisplays>,
}

/// One connected output as RandR reports it.
struct OutputSnap {
    id: randr::Output,
    name: String,
    info: randr::GetOutputInfoReply,
    crtc: Option<randr::GetCrtcInfoReply>,
    edid: Vec<u8>,
    /// Our modes for this output and the RandR mode behind each.
    modes: Vec<(Mode, ModeInfo)>,
    /// Rotations supported by its current (or first possible) CRTC.
    rotations: randr::Rotation,
}

struct PlannedOutput {
    /// Index into `Snapshot::outputs`.
    output: usize,
    crtc: randr::Crtc,
    mode: randr::Mode,
    x: i16,
    y: i16,
    rotation: randr::Rotation,
}

struct Snapshot {
    config_timestamp: xproto::Timestamp,
    outputs: Vec<OutputSnap>,
    primary: randr::Output,
}

/// Sends a request and waits for its reply, mapping errors to `Error::Backend`.
macro_rules! x11 {
    ($context:expr, $request:expr) => {
        $request
            .map_err(|e| Error::Backend(format!("{}: {e}", $context)))?
            .reply()
            .map_err(|e| Error::Backend(format!("{}: {e}", $context)))?
    };
}

/// Same for requests without a reply; `check` still round-trips for errors.
macro_rules! x11_void {
    ($context:expr, $request:expr) => {
        $request
            .map_err(|e| Error::Backend(format!("{}: {e}", $context)))?
            .check()
            .map_err(|e| Error::Backend(format!("{}: {e}", $context)))?
    };
}

impl X11Backend {
    /// Connects to `$DISPLAY` and checks for RandR 1.3+ (needed for
    /// `GetScreenResourcesCurrent` and primary output).
    pub fn connect() -> Result<Self> {
        let (conn, screen) = RustConnection::connect(None)
            .map_err(|e| Error::Backend(format!("cannot connect to X server: {e}")))?;
        let root = conn.setup().roots[screen].root;

        let version = x11!("RandR version", conn.randr_query_version(1, 5));
        if (version.major_version, version.minor_version) < (1, 3) {
            return Err(Error::Unsupported(format!(
                "RandR {}.{} (need 1.3+)",
                version.major_version, version.minor_version
            )));
        }
        let edid_atom = x11!("intern EDID", conn.intern_atom(false, b"EDID")).atom;

        Ok(Self {
            conn,
            root,
            edid_atom,
            apply_lock: Mutex::new(()),
            xfce: XfceDisplays::connect(),
        })
    }

    fn snapshot(&self) -> Result<Snapshot> {
        let res = x11!(
            "screen resources",
            self.conn.randr_get_screen_resources_current(self.root)
        );
        let mode_infos: HashMap<randr::Mode, &ModeInfo> =
            res.modes.iter().map(|m| (m.id, m)).collect();
        let primary = x11!(
            "primary output",
            self.conn.randr_get_output_primary(self.root)
        )
        .output;

        let mut outputs = Vec::new();
        for &id in &res.outputs {
            let info = x11!(
                "output info",
                self.conn.randr_get_output_info(id, res.config_timestamp)
            );
            if info.connection != randr::Connection::CONNECTED {
                continue;
            }
            let crtc = if info.crtc == 0 {
                None
            } else {
                Some(x11!(
                    "crtc info",
                    self.conn
                        .randr_get_crtc_info(info.crtc, res.config_timestamp)
                ))
            };
            let edid = x11!(
                "EDID",
                self.conn.randr_get_output_property(
                    id,
                    self.edid_atom,
                    AtomEnum::ANY,
                    0,
                    256,
                    false,
                    false
                )
            )
            .data;

            let mut modes: Vec<(Mode, ModeInfo)> = Vec::new();
            for mode_id in &info.modes {
                let Some(m) = mode_infos.get(mode_id) else {
                    continue;
                };
                let mode = Mode {
                    width: u32::from(m.width),
                    height: u32::from(m.height),
                    refresh_mhz: refresh_mhz(m),
                };
                // Different timings can round to the same mode; keep the first.
                if !modes.iter().any(|(existing, _)| *existing == mode) {
                    modes.push((mode, **m));
                }
            }

            let rotations = match (&crtc, info.crtcs.first()) {
                (Some(c), _) => c.rotations,
                (None, Some(&first)) => {
                    x11!(
                        "crtc info",
                        self.conn.randr_get_crtc_info(first, res.config_timestamp)
                    )
                    .rotations
                }
                (None, None) => randr::Rotation::ROTATE0,
            };

            outputs.push(OutputSnap {
                id,
                rotations,
                name: String::from_utf8_lossy(&info.name).into_owned(),
                info,
                crtc,
                edid,
                modes,
            });
        }

        Ok(Snapshot {
            config_timestamp: res.config_timestamp,
            outputs,
            primary,
        })
    }

    fn state_from(snap: &Snapshot) -> DisplayState {
        let mut monitors = Vec::new();
        let mut outputs = Vec::new();

        for o in &snap.outputs {
            let parsed = edid::parse(&o.edid).unwrap_or_default();
            let id = MonitorId {
                vendor: parsed.vendor,
                product: parsed.product,
                serial: parsed.serial,
                connector: o.name.clone(),
            };
            let modes: Vec<Mode> = o.modes.iter().map(|(m, _)| *m).collect();
            let preferred = (o.info.num_preferred > 0)
                .then(|| o.info.modes.first())
                .flatten()
                .and_then(|pid| o.modes.iter().find(|(_, info)| info.id == *pid))
                .map(|(m, _)| *m);

            let current = o.crtc.as_ref().filter(|c| c.mode != 0).and_then(|c| {
                o.modes
                    .iter()
                    .find(|(_, info)| info.id == c.mode)
                    .map(|(m, _)| (c, *m))
            });
            let fallback = preferred.or_else(|| modes.first().copied());
            let output = match (current, fallback) {
                (Some((crtc, mode)), _) => Some(OutputConfig {
                    id: id.clone(),
                    enabled: true,
                    position: Position {
                        x: i32::from(crtc.x),
                        y: i32::from(crtc.y),
                    },
                    mode,
                    scale: 1.0,
                    rotation: rotation_from_x(crtc.rotation),
                    primary: o.id == snap.primary,
                }),
                (None, Some(mode)) => Some(OutputConfig {
                    id: id.clone(),
                    enabled: false,
                    position: Position::default(),
                    mode,
                    scale: 1.0,
                    rotation: Rotation::Normal,
                    primary: false,
                }),
                // Connected but reports no modes (e.g. a broken EDID): skip it.
                (None, None) => None,
            };
            let Some(output) = output else { continue };

            monitors.push(MonitorInfo {
                name: parsed.name.unwrap_or_else(|| o.name.clone()),
                builtin: is_builtin(&o.name),
                physical_size: (o.info.mm_width > 0 && o.info.mm_height > 0).then_some(
                    PhysicalSize {
                        width_mm: o.info.mm_width,
                        height_mm: o.info.mm_height,
                    },
                ),
                modes,
                preferred_mode: preferred,
                rotations: Rotation::ALL
                    .into_iter()
                    .filter(|r| o.rotations.contains(rotation_to_x(*r)))
                    .collect(),
                scales: vec![1.0],
                custom_modes: false,
                id,
            });
            outputs.push(output);
        }

        // RandR allows "no primary"; the editor expects one, so pick the first enabled.
        if !outputs.iter().any(|o| o.enabled && o.primary)
            && let Some(first) = outputs.iter_mut().find(|o| o.enabled)
        {
            first.primary = true;
        }

        DisplayState {
            monitors,
            layout: Layout { outputs },
        }
    }

    /// Works out every CRTC assignment and checks modes and rotations before
    /// anything changes, so most failures leave the desktop untouched.
    fn plan(&self, target: &Layout, snap: &Snapshot) -> Result<Vec<PlannedOutput>> {
        let mut planned = Vec::new();
        for wanted in target.enabled() {
            let output = snap
                .outputs
                .iter()
                .position(|o| o.name == wanted.id.connector)
                .ok_or_else(|| Error::Backend(format!("{} disappeared", wanted.id.connector)))?;
            let o = &snap.outputs[output];
            let mode = o
                .modes
                .iter()
                .find(|(m, _)| *m == wanted.mode)
                .map(|(_, info)| info.id)
                .ok_or_else(|| Error::Backend(format!("{}: mode vanished", o.name)))?;
            planned.push(PlannedOutput {
                output,
                crtc: 0,
                mode,
                x: wanted.position.x as i16,
                y: wanted.position.y as i16,
                rotation: rotation_to_x(wanted.rotation),
            });
        }

        // Outputs we leave alone keep their CRTCs; outputs we turn off free theirs.
        let mut taken: HashSet<randr::Crtc> = snap
            .outputs
            .iter()
            .filter(|o| target.output(&o.name).is_none() && o.info.crtc != 0)
            .map(|o| o.info.crtc)
            .collect();
        // First pass: every output keeps its current CRTC.
        for p in &mut planned {
            let current = snap.outputs[p.output].info.crtc;
            if current != 0 && taken.insert(current) {
                p.crtc = current;
            }
        }
        // Second pass: the rest take any free CRTC that can drive them.
        for p in planned.iter_mut().filter(|p| p.crtc == 0) {
            let o = &snap.outputs[p.output];
            p.crtc = *o
                .info
                .crtcs
                .iter()
                .find(|c| !taken.contains(c))
                .ok_or_else(|| Error::Unsupported(format!("{}: no free CRTC", o.name)))?;
            taken.insert(p.crtc);
        }

        for p in &planned {
            let info = x11!(
                "crtc info",
                self.conn.randr_get_crtc_info(p.crtc, snap.config_timestamp)
            );
            if !info.rotations.contains(p.rotation) {
                return Err(Error::Unsupported(format!(
                    "{}: this rotation is not supported",
                    snap.outputs[p.output].name
                )));
            }
        }
        Ok(planned)
    }

    fn apply_locked(&self, target: &Layout, snap: &Snapshot) -> Result<()> {
        let size =
            bounding_box(target).ok_or_else(|| Error::Backend("nothing to enable".into()))?;
        let range = x11!(
            "screen size range",
            self.conn.randr_get_screen_size_range(self.root)
        );
        if size.width > i32::from(range.max_width) || size.height > i32::from(range.max_height) {
            return Err(Error::Unsupported(format!(
                "screen {}x{} exceeds X limit {}x{}",
                size.width, size.height, range.max_width, range.max_height
            )));
        }
        let width = size.width.max(i32::from(range.min_width)) as u16;
        let height = size.height.max(i32::from(range.min_height)) as u16;
        let planned = self.plan(target, snap)?;

        // 1. Switch off CRTCs that must go dark, change owner, or would not fit
        //    the new screen size.
        for o in &snap.outputs {
            let Some(crtc) = &o.crtc else { continue };
            let keeps_crtc = planned
                .iter()
                .any(|p| snap.outputs[p.output].id == o.id && p.crtc == o.info.crtc);
            let untouched = target.output(&o.name).is_none();
            let outside = i32::from(crtc.x) + i32::from(crtc.width) > i32::from(width)
                || i32::from(crtc.y) + i32::from(crtc.height) > i32::from(height);
            if (!keeps_crtc && !untouched) || outside {
                self.set_crtc(snap, o.info.crtc, 0, 0, 0, randr::Rotation::ROTATE0, &[])?;
            }
        }

        // 2. Resize the screen to the new bounding box.
        let mm = |px: u16| (f64::from(px) * 25.4 / ASSUMED_DPI).round() as u32;
        x11_void!(
            "set screen size",
            self.conn
                .randr_set_screen_size(self.root, width, height, mm(width), mm(height))
        );

        // 3. Configure every enabled output.
        for p in &planned {
            let output = snap.outputs[p.output].id;
            self.set_crtc(snap, p.crtc, p.x, p.y, p.mode, p.rotation, &[output])?;
        }

        // 4. Primary output.
        if let Some(primary) = target.enabled().find(|o| o.primary)
            && let Some(o) = snap.outputs.iter().find(|o| o.name == primary.id.connector)
        {
            x11_void!(
                "set primary",
                self.conn.randr_set_output_primary(self.root, o.id)
            );
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn set_crtc(
        &self,
        snap: &Snapshot,
        crtc: randr::Crtc,
        x: i16,
        y: i16,
        mode: randr::Mode,
        rotation: randr::Rotation,
        outputs: &[randr::Output],
    ) -> Result<()> {
        let reply = x11!(
            "set crtc",
            self.conn.randr_set_crtc_config(
                crtc,
                x11rb::CURRENT_TIME,
                snap.config_timestamp,
                x,
                y,
                mode,
                rotation,
                outputs
            )
        );
        if reply.status != SetConfig::SUCCESS {
            return Err(Error::Backend(format!(
                "set crtc {crtc}: {:?}",
                reply.status
            )));
        }
        Ok(())
    }

    /// Records the applied layout in xfconf so XFCE restores it on hotplug
    /// and login instead of its own older layout. Failures are only logged.
    fn persist_to_xfce(&self, snap: &Snapshot, applied: &Layout) {
        let Some(xfce) = &self.xfce else { return };
        let outputs: Vec<xfce::XfceOutput> = applied
            .outputs
            .iter()
            .filter_map(|o| {
                let s = snap.outputs.iter().find(|s| s.name == o.id.connector)?;
                let (_, mode) = s.modes.iter().find(|(m, _)| *m == o.mode)?;
                Some(xfce::XfceOutput::new(o, &s.edid, mode))
            })
            .collect();
        if let Err(e) = xfce.save(&outputs) {
            log::warn!("xfconf: {e}");
        }
    }
}

impl DisplayBackend for X11Backend {
    fn name(&self) -> &'static str {
        "x11-randr"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            scale: false,
            fractional_scale: false,
            rotation: true,
            primary: true,
            disable: true,
            origin: OriginRule::TopLeft,
            scaled_layout: true,
        }
    }

    fn current_state(&self) -> Result<DisplayState> {
        Ok(Self::state_from(&self.snapshot()?))
    }

    fn apply(&self, layout: &Layout) -> Result<()> {
        let _guard = self
            .apply_lock
            .lock()
            .map_err(|_| Error::Backend("apply lock poisoned".into()))?;
        self.validate(layout)?;
        let target = layout::normalize(layout, OriginRule::TopLeft);
        let snap = self.snapshot()?;
        let previous = Self::state_from(&snap).layout;

        x11_void!("grab server", self.conn.grab_server());
        let result = self.apply_locked(&target, &snap);
        if let Err(e) = &result {
            // Leave the desktop as it was rather than half-configured.
            log::error!("x11 apply failed, restoring: {e}");
            if let Ok(fresh) = self.snapshot()
                && let Err(e2) = self.apply_locked(&previous, &fresh)
            {
                log::error!("x11 restore failed: {e2}");
            }
        }
        let _ = self.conn.ungrab_server();
        let _ = self.conn.flush();

        if result.is_ok() {
            self.persist_to_xfce(&snap, &target);
        }
        result
    }
}

/// Refresh rate in millihertz from mode timings.
fn refresh_mhz(m: &ModeInfo) -> u32 {
    let mut vtotal = f64::from(m.vtotal);
    if m.mode_flags.contains(ModeFlag::DOUBLE_SCAN) {
        vtotal *= 2.0;
    }
    if m.mode_flags.contains(ModeFlag::INTERLACE) {
        vtotal /= 2.0;
    }
    if m.htotal == 0 || vtotal == 0.0 {
        return 0;
    }
    (f64::from(m.dot_clock) * 1000.0 / (f64::from(m.htotal) * vtotal)).round() as u32
}

fn rotation_from_x(r: randr::Rotation) -> Rotation {
    if r.contains(randr::Rotation::ROTATE90) {
        Rotation::Left
    } else if r.contains(randr::Rotation::ROTATE180) {
        Rotation::Inverted
    } else if r.contains(randr::Rotation::ROTATE270) {
        Rotation::Right
    } else {
        Rotation::Normal
    }
}

fn rotation_to_x(r: Rotation) -> randr::Rotation {
    match r {
        Rotation::Normal => randr::Rotation::ROTATE0,
        Rotation::Left => randr::Rotation::ROTATE90,
        Rotation::Inverted => randr::Rotation::ROTATE180,
        Rotation::Right => randr::Rotation::ROTATE270,
    }
}

fn is_builtin(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"]
        .iter()
        .any(|p| connector.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(dot_clock: u32, htotal: u16, vtotal: u16, flags: ModeFlag) -> ModeInfo {
        ModeInfo {
            id: 1,
            width: 1920,
            height: 1080,
            dot_clock,
            hsync_start: 0,
            hsync_end: 0,
            htotal,
            hskew: 0,
            vsync_start: 0,
            vsync_end: 0,
            vtotal,
            name_len: 0,
            mode_flags: flags,
        }
    }

    #[test]
    fn computes_refresh_from_timings() {
        // CEA 1080p60 and 1080p59.94.
        assert_eq!(
            refresh_mhz(&mode(148_500_000, 2200, 1125, ModeFlag::from(0u32))),
            60_000
        );
        assert_eq!(
            refresh_mhz(&mode(148_352_000, 2200, 1125, ModeFlag::from(0u32))),
            59_940
        );
        assert_eq!(
            refresh_mhz(&mode(74_250_000, 2200, 1125, ModeFlag::INTERLACE)),
            60_000
        );
        assert_eq!(refresh_mhz(&mode(1, 0, 0, ModeFlag::from(0u32))), 0);
    }

    #[test]
    fn rotation_round_trips() {
        for r in [
            Rotation::Normal,
            Rotation::Left,
            Rotation::Inverted,
            Rotation::Right,
        ] {
            assert_eq!(rotation_from_x(rotation_to_x(r)), r);
        }
    }

    #[test]
    fn detects_builtin_panels() {
        assert!(is_builtin("eDP"));
        assert!(is_builtin("eDP-1"));
        assert!(is_builtin("LVDS1"));
        assert!(!is_builtin("HDMI-A-0"));
    }
}
