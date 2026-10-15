//! Windows backend over the CCD API (`QueryDisplayConfig` / `SetDisplayConfig`)
//! plus `ChangeDisplaySettingsEx` for modes and positions.
//!
//! Applying runs in three steps:
//! 1. topology (which monitors are on) with `SetDisplayConfig`;
//! 2. mode, position, rotation and primary with `ChangeDisplaySettingsEx`,
//!    tested with `CDS_TEST` first and committed together;
//! 3. DPI scale through the undocumented `DisplayConfigSetDeviceInfo(-4)`.
//!
//! The Windows desktop is laid out in physical pixels: scale changes content
//! size, not monitor rectangles (`scaled_layout = false`).

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Mutex;

use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{ERROR_SUCCESS, LUID, POINTL};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RegGetValueW};
use windows::core::PCWSTR;

use crate::backend::{Capabilities, DisplayBackend, OriginRule};
use crate::edid;
use crate::error::{Error, Result};
use crate::layout;
use crate::model::{
    DisplayState, Layout, Mode, MonitorId, MonitorInfo, OutputConfig, PhysicalSize, Position,
    Rotation,
};

const PATH_ACTIVE: u32 = 0x1;
const MODE_IDX_INVALID: u32 = 0xffff_ffff;

/// Windows' DPI scale steps; per-monitor scale is stored relative to the
/// recommended step.
const DPI_STEPS: [u32; 12] = [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500];
const GET_DPI_SCALE: i32 = -3;
const SET_DPI_SCALE: i32 = -4;

#[repr(C)]
struct DpiScaleGet {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    min_rel: i32,
    cur_rel: i32,
    max_rel: i32,
}

#[repr(C)]
struct DpiScaleSet {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    rel: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct TargetKey {
    adapter: (u32, i32),
    id: u32,
}

impl TargetKey {
    fn of(path: &DISPLAYCONFIG_PATH_INFO) -> Self {
        let a = path.targetInfo.adapterId;
        Self {
            adapter: (a.LowPart, a.HighPart),
            id: path.targetInfo.id,
        }
    }
}

/// DPI state of one active source.
#[derive(Clone, Copy, Debug)]
struct Dpi {
    /// Index into `DPI_STEPS` of the recommended scale.
    recommended: i32,
    min_rel: i32,
    cur_rel: i32,
    max_rel: i32,
}

impl Dpi {
    fn index_of(&self, scale: f64) -> Option<i32> {
        let percent = (scale * 100.0).round() as u32;
        DPI_STEPS
            .iter()
            .position(|&s| s == percent)
            .map(|i| i as i32)
    }

    fn scales(&self) -> Vec<f64> {
        let lo = (self.recommended + self.min_rel).max(0);
        let hi = (self.recommended + self.max_rel).min(DPI_STEPS.len() as i32 - 1);
        (lo..=hi)
            .map(|i| f64::from(DPI_STEPS[i as usize]) / 100.0)
            .collect()
    }

    fn current(&self) -> f64 {
        let i = (self.recommended + self.cur_rel).clamp(0, DPI_STEPS.len() as i32 - 1);
        f64::from(DPI_STEPS[i as usize]) / 100.0
    }
}

struct Target {
    key: TargetKey,
    connector: String,
    builtin: bool,
    edid: Vec<u8>,
    friendly_name: String,
    edid_vendor: u16,
    edid_product: u16,
    /// Index of its active path in `Snapshot::active`, if on.
    active: Option<usize>,
    /// GDI device name of the source (`\\.\DISPLAY1`) when active.
    gdi: Option<Vec<u16>>,
    modes: Vec<Mode>,
    current: Option<(Mode, Position, Rotation)>,
    dpi: Option<Dpi>,
    source: Option<(LUID, u32)>,
}

struct Snapshot {
    all_paths: Vec<DISPLAYCONFIG_PATH_INFO>,
    active: Vec<DISPLAYCONFIG_PATH_INFO>,
    targets: Vec<Target>,
}

pub struct WindowsBackend {
    apply_lock: Mutex<()>,
}

impl WindowsBackend {
    pub fn new() -> Result<Self> {
        // Probe once so a missing API fails at startup, not on first use.
        query(QDC_ONLY_ACTIVE_PATHS)?;
        Ok(Self {
            apply_lock: Mutex::new(()),
        })
    }
}

fn win_err(context: &str, code: impl std::fmt::Debug) -> Error {
    Error::Backend(format!("{context}: {code:?}"))
}

fn query(
    flags: QUERY_DISPLAY_CONFIG_FLAGS,
) -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>)> {
    // The configuration can change between the size query and the read.
    for _ in 0..5 {
        let (mut np, mut nm) = (0u32, 0u32);
        let rc = unsafe { GetDisplayConfigBufferSizes(flags, &mut np, &mut nm) };
        if rc != ERROR_SUCCESS {
            return Err(win_err("GetDisplayConfigBufferSizes", rc));
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        let rc = unsafe {
            QueryDisplayConfig(
                flags,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                None,
            )
        };
        if rc == ERROR_SUCCESS {
            paths.truncate(np as usize);
            modes.truncate(nm as usize);
            return Ok((paths, modes));
        }
        if rc.0 != 122 {
            // Anything but ERROR_INSUFFICIENT_BUFFER is final.
            return Err(win_err("QueryDisplayConfig", rc));
        }
    }
    Err(Error::Backend(
        "QueryDisplayConfig: configuration kept changing".into(),
    ))
}

fn header(kind: i32, size: usize, adapter: LUID, id: u32) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(kind),
        size: size as u32,
        adapterId: adapter,
        id,
    }
}

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

fn target_name(adapter: LUID, id: u32) -> Option<DISPLAYCONFIG_TARGET_DEVICE_NAME> {
    let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME {
        header: header(
            DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME.0,
            size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(),
            adapter,
            id,
        ),
        ..Default::default()
    };
    (unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } == 0).then_some(name)
}

fn source_gdi_name(adapter: LUID, id: u32) -> Option<Vec<u16>> {
    let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        header: header(
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME.0,
            size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(),
            adapter,
            id,
        ),
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } != 0 {
        return None;
    }
    let end = name
        .viewGdiDeviceName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(32);
    let mut w = name.viewGdiDeviceName[..end].to_vec();
    w.push(0);
    Some(w)
}

fn preferred_mode(adapter: LUID, id: u32) -> Option<Mode> {
    let mut pref = DISPLAYCONFIG_TARGET_PREFERRED_MODE {
        header: header(
            DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE.0,
            size_of::<DISPLAYCONFIG_TARGET_PREFERRED_MODE>(),
            adapter,
            id,
        ),
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut pref.header) } != 0 || pref.width == 0 {
        return None;
    }
    let v = pref.targetMode.targetVideoSignalInfo.vSyncFreq;
    let hz = if v.Denominator == 0 {
        60
    } else {
        (f64::from(v.Numerator) / f64::from(v.Denominator)).round() as u32
    };
    Some(Mode {
        width: pref.width,
        height: pref.height,
        refresh_mhz: hz * 1000,
    })
}

fn dpi_of(adapter: LUID, source: u32) -> Option<Dpi> {
    let mut get = DpiScaleGet {
        header: header(GET_DPI_SCALE, size_of::<DpiScaleGet>(), adapter, source),
        min_rel: 0,
        cur_rel: 0,
        max_rel: 0,
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut get.header) } != 0 {
        return None;
    }
    Some(Dpi {
        recommended: -get.min_rel,
        min_rel: get.min_rel,
        cur_rel: get.cur_rel,
        max_rel: get.max_rel,
    })
}

fn set_dpi(adapter: LUID, source: u32, rel: i32) -> Result<()> {
    let set = DpiScaleSet {
        header: header(SET_DPI_SCALE, size_of::<DpiScaleSet>(), adapter, source),
        rel,
    };
    match unsafe { DisplayConfigSetDeviceInfo(&set.header) } {
        0 => Ok(()),
        rc => Err(win_err("set DPI scale", rc)),
    }
}

/// Full EDID from the registry: the CCD API exposes only vendor and product.
/// `\\?\DISPLAY#GSM5B7F#5&1a2b&0&UID4353#{guid}` maps to
/// `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\GSM5B7F\5&1a2b&0&UID4353\Device Parameters`.
fn registry_edid(device_path: &str) -> Vec<u8> {
    let parts: Vec<&str> = device_path.split('#').collect();
    if parts.len() < 3 {
        return Vec::new();
    }
    let key = format!(
        "SYSTEM\\CurrentControlSet\\Enum\\DISPLAY\\{}\\{}\\Device Parameters",
        parts[1], parts[2]
    );
    let key: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let value: Vec<u16> = "EDID".encode_utf16().chain([0]).collect();
    let mut buf = vec![0u8; 1024];
    let mut len = buf.len() as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if rc != ERROR_SUCCESS {
        return Vec::new();
    }
    buf.truncate(len as usize);
    buf
}

fn current_devmode(gdi: &[u16]) -> Option<DEVMODEW> {
    let mut dm = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    let ok = unsafe {
        EnumDisplaySettingsExW(
            PCWSTR(gdi.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut dm,
            ENUM_DISPLAY_SETTINGS_FLAGS(0),
        )
    };
    ok.as_bool().then_some(dm)
}

fn orientation(dm: &DEVMODEW) -> DEVMODE_DISPLAY_ORIENTATION {
    unsafe { dm.Anonymous1.Anonymous2.dmDisplayOrientation }
}

/// Modes in native (unrotated) orientation, refresh rounded to whole hertz as
/// `EnumDisplaySettings` reports it.
fn list_modes(gdi: &[u16], rotated: bool) -> Vec<Mode> {
    let mut modes = Vec::new();
    for i in 0.. {
        let mut dm = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let ok = unsafe {
            EnumDisplaySettingsExW(
                PCWSTR(gdi.as_ptr()),
                ENUM_DISPLAY_SETTINGS_MODE(i),
                &mut dm,
                ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
        };
        if !ok.as_bool() {
            break;
        }
        // Without EDS_ROTATEDMODE the list is in the current orientation.
        let (w, h) = if rotated {
            (dm.dmPelsHeight, dm.dmPelsWidth)
        } else {
            (dm.dmPelsWidth, dm.dmPelsHeight)
        };
        let mode = Mode {
            width: w,
            height: h,
            refresh_mhz: dm.dmDisplayFrequency.max(1) * 1000,
        };
        if !modes.contains(&mode) {
            modes.push(mode);
        }
    }
    modes.sort_by_key(|m| std::cmp::Reverse((m.width * m.height, m.refresh_mhz)));
    modes
}

fn rotation_from_dm(o: DEVMODE_DISPLAY_ORIENTATION) -> Rotation {
    match o {
        DMDO_90 => Rotation::Left,
        DMDO_180 => Rotation::Inverted,
        DMDO_270 => Rotation::Right,
        _ => Rotation::Normal,
    }
}

/// DMDO_90 turns the picture 90° counter-clockwise ("Portrait" in Settings).
fn rotation_to_dm(r: Rotation) -> DEVMODE_DISPLAY_ORIENTATION {
    match r {
        Rotation::Normal => DMDO_DEFAULT,
        Rotation::Left => DMDO_90,
        Rotation::Inverted => DMDO_180,
        Rotation::Right => DMDO_270,
    }
}

fn connector_name(tech: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY, instance: u32) -> (String, bool) {
    let (prefix, builtin) = match tech {
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI => ("HDMI", false),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL => ("DP", false),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_USB_TUNNEL => ("USB-C", false),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DVI => ("DVI", false),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HD15 => ("VGA", false),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_MIRACAST => ("Miracast", false),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED => ("eDP", true),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS => ("LVDS", true),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL => ("Internal", true),
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED => ("UDI", true),
        _ => ("Display", false),
    };
    (format!("{prefix}-{}", instance + 1), builtin)
}

/// Bumps the trailing number until the name is free ("HDMI-1" -> "HDMI-2").
fn unique_connector(name: String, taken: &[Target]) -> String {
    let free = |n: &str| !taken.iter().any(|t| t.connector == n);
    if free(&name) {
        return name;
    }
    let (prefix, number) = name.rsplit_once('-').unwrap_or((&name, "1"));
    let start: u32 = number.parse().unwrap_or(1);
    (start + 1..)
        .map(|n| format!("{prefix}-{n}"))
        .find(|n| free(n))
        .expect("unbounded range")
}

/// PNP id as stored by CCD: the two EDID bytes, byte-swapped.
fn pnp_vendor(raw: u16) -> String {
    let id = raw.swap_bytes();
    [10, 5, 0]
        .iter()
        .map(|shift| {
            let v = ((id >> shift) & 0x1f) as u8;
            if (1..=26).contains(&v) {
                char::from(b'A' + v - 1)
            } else {
                '?'
            }
        })
        .collect()
}

fn snapshot() -> Result<Snapshot> {
    let (all_paths, _) = query(QDC_ALL_PATHS)?;
    let (active, modes) = query(QDC_ONLY_ACTIVE_PATHS)?;

    let mut targets: Vec<Target> = Vec::new();
    let mut seen = HashSet::new();
    for path in all_paths
        .iter()
        .filter(|p| p.targetInfo.targetAvailable.as_bool())
    {
        let key = TargetKey::of(path);
        if !seen.insert(key) {
            continue;
        }
        let Some(name) = target_name(path.targetInfo.adapterId, path.targetInfo.id) else {
            continue;
        };
        let device_path = wide_to_string(&name.monitorDevicePath);
        let (connector, builtin) = connector_name(name.outputTechnology, name.connectorInstance);
        // Two GPUs can both have an "HDMI-1".
        let connector = unique_connector(connector, &targets);
        targets.push(Target {
            key,
            connector,
            builtin,
            edid: registry_edid(&device_path),
            friendly_name: wide_to_string(&name.monitorFriendlyDeviceName),
            edid_vendor: name.edidManufactureId,
            edid_product: name.edidProductCodeId,
            active: None,
            gdi: None,
            modes: Vec::new(),
            current: None,
            dpi: None,
            source: None,
        });
    }

    for (i, path) in active.iter().enumerate() {
        let key = TargetKey::of(path);
        let Some(t) = targets.iter_mut().find(|t| t.key == key) else {
            continue;
        };
        let adapter = path.sourceInfo.adapterId;
        let source = path.sourceInfo.id;
        t.active = Some(i);
        t.source = Some((adapter, source));
        t.dpi = dpi_of(adapter, source);
        t.gdi = source_gdi_name(adapter, source);

        let idx = unsafe { path.sourceInfo.Anonymous.modeInfoIdx };
        let position = modes
            .get(idx as usize)
            .filter(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE)
            .map(|m| unsafe { m.Anonymous.sourceMode.position })
            .unwrap_or(POINTL { x: 0, y: 0 });

        if let Some(gdi) = &t.gdi
            && let Some(dm) = current_devmode(gdi)
        {
            let rotation = rotation_from_dm(orientation(&dm));
            let (w, h) = if rotation.swaps_axes() {
                (dm.dmPelsHeight, dm.dmPelsWidth)
            } else {
                (dm.dmPelsWidth, dm.dmPelsHeight)
            };
            let mode = Mode {
                width: w,
                height: h,
                refresh_mhz: dm.dmDisplayFrequency.max(1) * 1000,
            };
            t.modes = list_modes(gdi, rotation.swaps_axes());
            if !t.modes.contains(&mode) {
                t.modes.insert(0, mode);
            }
            t.current = Some((
                mode,
                Position {
                    x: position.x,
                    y: position.y,
                },
                rotation,
            ));
        }
    }

    for t in targets.iter_mut().filter(|t| t.modes.is_empty()) {
        let adapter = LUID {
            LowPart: t.key.adapter.0,
            HighPart: t.key.adapter.1,
        };
        if let Some(mode) = preferred_mode(adapter, t.key.id) {
            t.modes.push(mode);
        }
    }
    targets.retain(|t| !t.modes.is_empty());

    Ok(Snapshot {
        all_paths,
        active,
        targets,
    })
}

fn state_from(snap: &Snapshot) -> DisplayState {
    let mut monitors = Vec::new();
    let mut outputs = Vec::new();
    for t in &snap.targets {
        let parsed = edid::parse(&t.edid);
        let id = MonitorId {
            vendor: parsed
                .as_ref()
                .map_or_else(|| pnp_vendor(t.edid_vendor), |e| e.vendor.clone()),
            product: parsed.as_ref().map_or(t.edid_product, |e| e.product),
            serial: parsed.as_ref().and_then(|e| e.serial.clone()),
            connector: t.connector.clone(),
        };
        let name = parsed
            .as_ref()
            .and_then(|e| e.name.clone())
            .or_else(|| Some(t.friendly_name.clone()).filter(|n| !n.is_empty()))
            .unwrap_or_else(|| t.connector.clone());
        monitors.push(MonitorInfo {
            id: id.clone(),
            name,
            builtin: t.builtin,
            physical_size: parsed
                .as_ref()
                .and_then(|e| e.size_mm)
                .map(|(w, h)| PhysicalSize {
                    width_mm: w,
                    height_mm: h,
                }),
            preferred_mode: t.modes.first().copied(),
            modes: t.modes.clone(),
            rotations: Rotation::ALL.to_vec(),
            scales: t.dpi.map(|d| d.scales()).unwrap_or_default(),
        });
        outputs.push(match t.current {
            Some((mode, position, rotation)) => OutputConfig {
                id,
                enabled: true,
                primary: position == Position::default(),
                position,
                mode,
                scale: t.dpi.map_or(1.0, |d| d.current()),
                rotation,
            },
            None => OutputConfig {
                id,
                enabled: false,
                position: Position::default(),
                mode: t.modes[0],
                scale: 1.0,
                rotation: Rotation::Normal,
                primary: false,
            },
        });
    }
    DisplayState {
        monitors,
        layout: Layout { outputs },
    }
}

/// Step 1: turn monitors on and off. Keeps current paths and picks a free
/// source for newly enabled targets. Returns whether anything was requested.
fn apply_topology(snap: &Snapshot, target: &Layout) -> Result<bool> {
    let wanted: HashSet<TargetKey> = snap
        .targets
        .iter()
        .filter(|t| target.output(&t.connector).is_some_and(|o| o.enabled))
        .map(|t| t.key)
        .collect();
    let current: HashSet<TargetKey> = snap
        .targets
        .iter()
        .filter(|t| t.active.is_some())
        .map(|t| t.key)
        .collect();
    if wanted == current {
        return Ok(false);
    }

    let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = snap
        .active
        .iter()
        .filter(|p| wanted.contains(&TargetKey::of(p)))
        .copied()
        .collect();
    let mut used_sources: HashMap<(u32, i32), HashSet<u32>> = HashMap::new();
    for p in &paths {
        let a = p.sourceInfo.adapterId;
        used_sources
            .entry((a.LowPart, a.HighPart))
            .or_default()
            .insert(p.sourceInfo.id);
    }
    for key in wanted.difference(&current) {
        let candidate = snap.all_paths.iter().find(|p| {
            let a = p.sourceInfo.adapterId;
            TargetKey::of(p) == *key
                && !used_sources
                    .get(&(a.LowPart, a.HighPart))
                    .is_some_and(|s| s.contains(&p.sourceInfo.id))
        });
        let Some(path) = candidate else {
            return Err(Error::Unsupported(
                "no free display source for a monitor".into(),
            ));
        };
        let a = path.sourceInfo.adapterId;
        used_sources
            .entry((a.LowPart, a.HighPart))
            .or_default()
            .insert(path.sourceInfo.id);
        paths.push(*path);
    }
    // Let Windows choose modes for the new topology; step 2 sets them.
    for p in &mut paths {
        p.flags |= PATH_ACTIVE;
        p.sourceInfo.Anonymous.modeInfoIdx = MODE_IDX_INVALID;
        p.targetInfo.Anonymous.modeInfoIdx = MODE_IDX_INVALID;
    }

    let supplied =
        SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES | SDC_SAVE_TO_DATABASE;
    let rc = unsafe { SetDisplayConfig(Some(&paths), None, supplied) };
    if rc == 0 {
        return Ok(true);
    }
    // Some drivers only accept a bare topology resolved from the database.
    let topology = SDC_APPLY | SDC_TOPOLOGY_SUPPLIED | SDC_ALLOW_PATH_ORDER_CHANGES;
    match unsafe { SetDisplayConfig(Some(&paths), None, topology) } {
        0 => Ok(true),
        rc2 => Err(win_err("SetDisplayConfig", (rc, rc2))),
    }
}

/// Step 2: modes, positions, rotation, primary — tested, then committed at once.
fn apply_modes(snap: &Snapshot, target: &Layout) -> Result<()> {
    let mut changes = Vec::new();
    for t in &snap.targets {
        let Some(wanted) = target.output(&t.connector).filter(|o| o.enabled) else {
            continue;
        };
        let gdi = t.gdi.clone().ok_or_else(|| {
            Error::Backend(format!("{}: not active after topology change", t.connector))
        })?;
        let swap = wanted.rotation.swaps_axes();
        let mut dm = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        dm.dmFields = DM_PELSWIDTH
            | DM_PELSHEIGHT
            | DM_DISPLAYFREQUENCY
            | DM_POSITION
            | DM_DISPLAYORIENTATION;
        dm.dmPelsWidth = if swap {
            wanted.mode.height
        } else {
            wanted.mode.width
        };
        dm.dmPelsHeight = if swap {
            wanted.mode.width
        } else {
            wanted.mode.height
        };
        dm.dmDisplayFrequency = wanted.mode.refresh_mhz / 1000;
        dm.Anonymous1.Anonymous2.dmPosition = POINTL {
            x: wanted.position.x,
            y: wanted.position.y,
        };
        dm.Anonymous1.Anonymous2.dmDisplayOrientation = rotation_to_dm(wanted.rotation);
        let mut flags = CDS_UPDATEREGISTRY | CDS_NORESET;
        if wanted.primary {
            flags |= CDS_SET_PRIMARY;
        }
        changes.push((t.connector.clone(), gdi, dm, flags));
    }

    for (name, gdi, dm, _) in &changes {
        let rc = unsafe {
            ChangeDisplaySettingsExW(PCWSTR(gdi.as_ptr()), Some(dm), None, CDS_TEST, None)
        };
        if rc != DISP_CHANGE_SUCCESSFUL {
            return Err(Error::Unsupported(format!(
                "{name}: settings rejected ({rc:?})"
            )));
        }
    }
    // The primary must be staged first so others are positioned relative to it.
    changes.sort_by_key(|(.., flags)| !flags.contains(CDS_SET_PRIMARY));
    for (name, gdi, dm, flags) in &changes {
        let rc =
            unsafe { ChangeDisplaySettingsExW(PCWSTR(gdi.as_ptr()), Some(dm), None, *flags, None) };
        if rc != DISP_CHANGE_SUCCESSFUL {
            return Err(win_err(&format!("{name}: stage settings"), rc));
        }
    }
    let rc = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None) };
    if rc != DISP_CHANGE_SUCCESSFUL {
        return Err(win_err("commit display settings", rc));
    }
    Ok(())
}

/// Step 3: per-monitor DPI scale.
fn apply_scale(snap: &Snapshot, target: &Layout) -> Result<()> {
    for t in &snap.targets {
        let Some(wanted) = target.output(&t.connector).filter(|o| o.enabled) else {
            continue;
        };
        let (Some(dpi), Some((adapter, source))) = (t.dpi, t.source) else {
            continue;
        };
        if (dpi.current() - wanted.scale).abs() < 1e-6 {
            continue;
        }
        let index = dpi.index_of(wanted.scale).ok_or_else(|| {
            Error::Unsupported(format!("{}: scale {}", t.connector, wanted.scale))
        })?;
        set_dpi(adapter, source, index - dpi.recommended)?;
    }
    Ok(())
}

/// Fallback for turning monitors off: the classic "detach" by setting an
/// empty mode. Works where `SetDisplayConfig` is unavailable or ignored.
fn detach_legacy(snap: &Snapshot, target: &Layout) -> Result<()> {
    let mut staged = false;
    for t in &snap.targets {
        let off = target.output(&t.connector).is_some_and(|o| !o.enabled);
        let (true, Some(gdi)) = (off, &t.gdi) else {
            continue;
        };
        let mut dm = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        dm.dmFields = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT;
        let rc = unsafe {
            ChangeDisplaySettingsExW(
                PCWSTR(gdi.as_ptr()),
                Some(&dm),
                None,
                CDS_UPDATEREGISTRY | CDS_NORESET,
                None,
            )
        };
        if rc != DISP_CHANGE_SUCCESSFUL {
            return Err(win_err(&format!("{}: detach", t.connector), rc));
        }
        staged = true;
    }
    if staged {
        let rc = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None) };
        if rc != DISP_CHANGE_SUCCESSFUL {
            return Err(win_err("commit detach", rc));
        }
    }
    Ok(())
}

fn enabled_set(snap: &Snapshot) -> HashSet<String> {
    snap.targets
        .iter()
        .filter(|t| t.active.is_some())
        .map(|t| t.connector.clone())
        .collect()
}

fn apply_all(target: &Layout) -> Result<()> {
    let wanted: HashSet<String> = target.enabled().map(|o| o.id.connector.clone()).collect();
    let snap = snapshot()?;
    apply_topology(&snap, target)?;
    // New sources mean new GDI names and DPI handles.
    let mut snap = snapshot()?;
    if enabled_set(&snap).iter().any(|c| !wanted.contains(c)) {
        detach_legacy(&snap, target)?;
        snap = snapshot()?;
    }
    // SetDisplayConfig can report success without switching anything (and
    // does under Wine); never pretend it worked.
    let now = enabled_set(&snap);
    if now != wanted {
        let mut diff: Vec<&String> = now.symmetric_difference(&wanted).collect();
        diff.sort();
        return Err(Error::Unsupported(format!(
            "Windows did not switch these monitors on/off: {}",
            diff.into_iter().cloned().collect::<Vec<_>>().join(", ")
        )));
    }
    apply_modes(&snap, target)?;
    apply_scale(&snapshot()?, target)
}

impl DisplayBackend for WindowsBackend {
    fn name(&self) -> &'static str {
        "windows-ccd"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            scale: true,
            fractional_scale: true,
            rotation: true,
            primary: true,
            disable: true,
            origin: OriginRule::Primary,
            scaled_layout: false,
        }
    }

    fn current_state(&self) -> Result<DisplayState> {
        Ok(state_from(&snapshot()?))
    }

    fn apply(&self, layout: &Layout) -> Result<()> {
        let _guard = self
            .apply_lock
            .lock()
            .map_err(|_| Error::Backend("apply lock poisoned".into()))?;
        self.validate(layout)?;
        let target = layout::normalize(layout, OriginRule::Primary);
        let previous = self.current_state()?.layout;

        let result = apply_all(&target);
        if let Err(e) = &result {
            log::error!("windows apply failed, restoring: {e}");
            if let Err(e2) = apply_all(&previous) {
                log::error!("windows restore failed: {e2}");
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_steps_relative_to_recommended() {
        // Recommended 150 %, allowed 100–200 %, currently 125 %.
        let dpi = Dpi {
            recommended: 2,
            min_rel: -2,
            cur_rel: -1,
            max_rel: 2,
        };
        assert_eq!(dpi.scales(), vec![1.0, 1.25, 1.5, 1.75, 2.0]);
        assert_eq!(dpi.current(), 1.25);
        assert_eq!(dpi.index_of(1.75), Some(3));
        assert_eq!(dpi.index_of(1.1), None);
    }

    #[test]
    fn decodes_ccd_vendor_id() {
        // "DEL" = 0x10AC in EDID byte order; CCD returns it swapped.
        assert_eq!(pnp_vendor(0xAC10), "DEL");
    }

    #[test]
    fn names_connectors() {
        assert_eq!(
            connector_name(DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI, 0),
            ("HDMI-1".into(), false)
        );
        assert_eq!(
            connector_name(DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, 0),
            ("Internal-1".into(), true)
        );
    }
}
