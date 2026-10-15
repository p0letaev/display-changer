//! KDE Plasma (KWin) backend over `kde_output_device_v2` and
//! `kde_output_management_v2`. Primary output comes from `kde_output_order_v1`
//! (first entry) and is set through output priorities.

use std::collections::HashMap;

use super::kde_protocols::device::{
    kde_output_device_mode_v2::{self, KdeOutputDeviceModeV2},
    kde_output_device_registry_v2::{self, KdeOutputDeviceRegistryV2},
    kde_output_device_v2::{self, KdeOutputDeviceV2},
};
use super::kde_protocols::management::{
    kde_output_configuration_v2::{self, KdeOutputConfigurationV2},
    kde_output_management_v2::{self, KdeOutputManagementV2},
};
use super::kde_protocols::order::kde_output_order_v1::{self, KdeOutputOrderV1};
use base64::Engine as _;
use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_output::Transform, wl_registry};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child};

use super::wayland_util::{model_code, rotation_from_transform, transform_from_rotation};
use crate::backend::{Capabilities, DisplayBackend, OriginRule};
use crate::edid;
use crate::error::{Error, Result};
use crate::layout;
use crate::model::{
    DisplayState, Layout, Mode, MonitorId, MonitorInfo, OutputConfig, PhysicalSize, Position,
    Rotation,
};

/// Highest versions of the vendored XML.
const DEVICE_VERSION: u32 = 26;
const MANAGEMENT_VERSION: u32 = 22;

#[derive(Default)]
struct Device {
    proxy: Option<KdeOutputDeviceV2>,
    name: String,
    make: String,
    model: String,
    serial: String,
    edid: Vec<u8>,
    /// Name of the output this one mirrors (protocol v13+), or empty.
    replication_source: String,
    physical: (i32, i32),
    position: (i32, i32),
    transform: i32,
    scale: f64,
    enabled: bool,
    modes: Vec<ObjectId>,
    current_mode: Option<ObjectId>,
    done: bool,
}

struct DeviceMode {
    proxy: KdeOutputDeviceModeV2,
    width: i32,
    height: i32,
    refresh: i32,
    preferred: bool,
    removed: bool,
}

#[derive(Default)]
struct State {
    devices: Vec<Device>,
    modes: HashMap<ObjectId, DeviceMode>,
    order: Vec<String>,
    order_done: bool,
    /// The registry (KWin 6.4+) announces all outputs, then `finished`.
    registry_done: bool,
    outcome: Option<std::result::Result<(), String>>,
    failure_reason: Option<String>,
}

impl State {
    fn device_mut(&mut self, proxy: &KdeOutputDeviceV2) -> Option<&mut Device> {
        self.devices
            .iter_mut()
            .find(|d| d.proxy.as_ref() == Some(proxy))
    }

    fn mode_of(&self, id: &ObjectId) -> Option<Mode> {
        self.modes.get(id).filter(|m| !m.removed).map(|m| Mode {
            width: m.width as u32,
            height: m.height as u32,
            refresh_mhz: m.refresh.max(0) as u32,
        })
    }
}

struct Session {
    queue: EventQueue<State>,
    state: State,
    management: KdeOutputManagementV2,
}

impl Session {
    fn open() -> Result<Self> {
        let conn = Connection::connect_to_env()
            .map_err(|e| Error::Backend(format!("Wayland connect: {e}")))?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn)
            .map_err(|e| Error::Backend(format!("Wayland registry: {e}")))?;
        let qh = queue.handle();
        let management: KdeOutputManagementV2 = globals
            .bind(&qh, 1..=MANAGEMENT_VERSION, ())
            .map_err(|_| Error::Unsupported("compositor lacks kde_output_management_v2".into()))?;

        let mut state = State::default();
        let mut has_order = false;
        let mut has_registry = false;
        for g in globals.contents().clone_list() {
            match g.interface.as_str() {
                // KWin 6.4+: one registry global creates the device objects.
                "kde_output_device_registry_v2" => {
                    let _: KdeOutputDeviceRegistryV2 =
                        globals
                            .registry()
                            .bind(g.name, g.version.min(DEVICE_VERSION), &qh, ());
                    has_registry = true;
                }
                // Older KWin: one global per output.
                "kde_output_device_v2" => {
                    let proxy: KdeOutputDeviceV2 =
                        globals
                            .registry()
                            .bind(g.name, g.version.min(DEVICE_VERSION), &qh, ());
                    state.devices.push(Device {
                        proxy: Some(proxy),
                        scale: 1.0,
                        ..Default::default()
                    });
                }
                "kde_output_order_v1" => {
                    let _: KdeOutputOrderV1 = globals.registry().bind(g.name, 1, &qh, ());
                    has_order = true;
                }
                _ => {}
            }
        }
        if state.devices.is_empty() && !has_registry {
            return Err(Error::Unsupported("no kde_output_device_v2 outputs".into()));
        }
        // With a registry, devices arrive as events; wait for at least one
        // roundtrip so they are announced before checking `done`.
        queue
            .roundtrip(&mut state)
            .map_err(|e| Error::Backend(format!("Wayland roundtrip: {e}")))?;
        while !(state.devices.iter().all(|d| d.done) && (state.order_done || !has_order)) {
            queue
                .blocking_dispatch(&mut state)
                .map_err(|e| Error::Backend(format!("Wayland dispatch: {e}")))?;
        }
        Ok(Self {
            queue,
            state,
            management,
        })
    }

    fn to_display_state(&self) -> DisplayState {
        let primary = self.state.order.first().cloned();
        let mut monitors = Vec::new();
        let mut outputs = Vec::new();
        for d in &self.state.devices {
            let modes: Vec<Mode> = d
                .modes
                .iter()
                .filter_map(|id| self.state.mode_of(id))
                .collect();
            let preferred = d
                .modes
                .iter()
                .find(|id| {
                    self.state
                        .modes
                        .get(*id)
                        .is_some_and(|m| m.preferred && !m.removed)
                })
                .and_then(|id| self.state.mode_of(id));
            let current = d
                .current_mode
                .as_ref()
                .and_then(|id| self.state.mode_of(id));
            let Some(mode) = current.or(preferred).or_else(|| modes.first().copied()) else {
                continue;
            };

            let parsed = edid::parse(&d.edid);
            let id = MonitorId {
                vendor: parsed
                    .as_ref()
                    .map_or_else(|| d.make.clone(), |e| e.vendor.clone()),
                product: parsed
                    .as_ref()
                    .map_or_else(|| model_code(&d.model), |e| e.product),
                serial: parsed
                    .as_ref()
                    .and_then(|e| e.serial.clone())
                    .or_else(|| Some(d.serial.clone()).filter(|s| !s.is_empty())),
                connector: d.name.clone(),
            };
            let name = parsed
                .as_ref()
                .and_then(|e| e.name.clone())
                .or_else(|| Some(d.model.clone()).filter(|m| !m.is_empty()))
                .unwrap_or_else(|| d.name.clone());
            monitors.push(MonitorInfo {
                id: id.clone(),
                name,
                builtin: ["eDP", "LVDS", "DSI"].iter().any(|p| d.name.starts_with(p)),
                physical_size: (d.physical.0 > 0 && d.physical.1 > 0).then_some(PhysicalSize {
                    width_mm: d.physical.0 as u32,
                    height_mm: d.physical.1 as u32,
                }),
                modes,
                preferred_mode: preferred,
                rotations: Rotation::ALL.to_vec(),
                scales: Vec::new(),
                custom_modes: false,
            });
            outputs.push(OutputConfig {
                id,
                enabled: d.enabled,
                position: Position {
                    x: d.position.0,
                    y: d.position.1,
                },
                mode,
                scale: d.scale,
                rotation: Transform::try_from(d.transform as u32)
                    .map_or(Rotation::Normal, rotation_from_transform),
                primary: primary.as_deref() == Some(d.name.as_str())
                    && d.replication_source.is_empty(),
                mirror_of: Some(d.replication_source.clone())
                    .filter(|s| !s.is_empty() && *s != d.name),
            });
        }
        if !outputs.iter().any(|o| o.is_placed() && o.primary)
            && let Some(first) = outputs.iter_mut().find(|o| o.is_placed())
        {
            first.primary = true;
        }
        DisplayState {
            monitors,
            layout: Layout { outputs },
        }
    }

    fn apply(mut self, target: &Layout) -> Result<()> {
        let qh = self.queue.handle();
        let config = self.management.create_configuration(&qh, ());
        let version = config.version();

        // Primary first, then the rest in their current order.
        let mut priority = 1u32;
        let primary = target
            .enabled()
            .find(|o| o.primary)
            .map(|o| o.id.connector.clone());
        let mut order: Vec<&Device> = self.state.devices.iter().collect();
        order.sort_by_key(|d| {
            (
                Some(&d.name) != primary.as_ref(),
                self.state.order.iter().position(|n| *n == d.name),
            )
        });

        for d in order {
            let Some(proxy) = &d.proxy else { continue };
            let Some(wanted) = target.output(&d.name) else {
                continue;
            };
            config.enable(proxy, i32::from(wanted.enabled));
            if !wanted.enabled {
                continue;
            }
            let mode = d
                .modes
                .iter()
                .find(|id| self.state.mode_of(id) == Some(wanted.mode));
            if let Some(m) = mode.and_then(|id| self.state.modes.get(id)) {
                config.mode(proxy, &m.proxy);
            }
            config.transform(proxy, transform_from_rotation(wanted.rotation) as i32);
            config.position(proxy, wanted.position.x, wanted.position.y);
            config.scale(proxy, wanted.scale);
            if version >= 13 {
                // Empty clears a previous mirror.
                config.set_replication_source(proxy, wanted.mirror_of.clone().unwrap_or_default());
            }
            if version >= 3 {
                config.set_priority(proxy, priority);
                priority += 1;
            } else if version >= 2 && wanted.primary {
                config.set_primary_output(proxy);
            }
        }

        config.apply();
        while self.state.outcome.is_none() {
            self.queue
                .blocking_dispatch(&mut self.state)
                .map_err(|e| Error::Backend(format!("Wayland dispatch: {e}")))?;
        }
        config.destroy();
        let _ = self.queue.flush();
        match self.state.outcome.take() {
            Some(Ok(())) => Ok(()),
            Some(Err(_)) => Err(Error::Backend(format!(
                "KWin rejected the configuration{}",
                self.state
                    .failure_reason
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default()
            ))),
            None => unreachable!(),
        }
    }
}

pub struct KdeBackend {
    /// `set_replication_source` needs kde_output_management_v2 v13.
    mirror: bool,
}

impl KdeBackend {
    pub fn connect() -> Result<Self> {
        let session = Session::open()?;
        Ok(Self {
            mirror: session.management.version() >= 13,
        })
    }
}

impl DisplayBackend for KdeBackend {
    fn name(&self) -> &'static str {
        "kde"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            scale: true,
            fractional_scale: true,
            rotation: true,
            primary: true,
            disable: true,
            origin: OriginRule::TopLeft,
            scaled_layout: true,
            mirror: self.mirror,
        }
    }

    fn current_state(&self) -> Result<DisplayState> {
        Ok(Session::open()?.to_display_state())
    }

    fn apply(&self, layout: &Layout) -> Result<()> {
        self.validate(layout)?;
        Session::open()?.apply(&layout::normalize(layout, OriginRule::TopLeft))
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<KdeOutputDeviceRegistryV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &KdeOutputDeviceRegistryV2,
        event: kde_output_device_registry_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            kde_output_device_registry_v2::Event::Output { output } => {
                state.devices.push(Device {
                    proxy: Some(output),
                    scale: 1.0,
                    ..Default::default()
                });
            }
            kde_output_device_registry_v2::Event::Finished => state.registry_done = true,
        }
    }

    event_created_child!(State, KdeOutputDeviceRegistryV2, [
        kde_output_device_registry_v2::EVT_OUTPUT_OPCODE => (KdeOutputDeviceV2, ()),
    ]);
}

impl Dispatch<KdeOutputManagementV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &KdeOutputManagementV2,
        _: kde_output_management_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<KdeOutputDeviceV2, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &KdeOutputDeviceV2,
        event: kde_output_device_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use kde_output_device_v2::Event;
        if let Event::Mode { mode } = &event {
            state.modes.insert(
                mode.id(),
                DeviceMode {
                    proxy: mode.clone(),
                    width: 0,
                    height: 0,
                    refresh: 0,
                    preferred: false,
                    removed: false,
                },
            );
        }
        let Some(d) = state.device_mut(proxy) else {
            return;
        };
        match event {
            Event::Geometry {
                x,
                y,
                physical_width,
                physical_height,
                make,
                model,
                transform,
                ..
            } => {
                d.position = (x, y);
                d.physical = (physical_width, physical_height);
                d.make = make;
                d.model = model;
                d.transform = transform;
            }
            Event::CurrentMode { mode } => d.current_mode = Some(mode.id()),
            Event::Mode { mode } => d.modes.push(mode.id()),
            Event::Done => d.done = true,
            Event::Scale { factor } => d.scale = factor,
            Event::Edid { raw } => {
                d.edid = base64::engine::general_purpose::STANDARD
                    .decode(raw.trim())
                    .unwrap_or_default();
            }
            Event::Enabled { enabled } => d.enabled = enabled != 0,
            Event::SerialNumber { serialNumber } => d.serial = serialNumber,
            Event::Name { name } => d.name = name,
            Event::ReplicationSource { source } => d.replication_source = source,
            _ => {}
        }
    }

    event_created_child!(State, KdeOutputDeviceV2, [
        kde_output_device_v2::EVT_MODE_OPCODE => (KdeOutputDeviceModeV2, ()),
    ]);
}

impl Dispatch<KdeOutputDeviceModeV2, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &KdeOutputDeviceModeV2,
        event: kde_output_device_mode_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(m) = state.modes.get_mut(&proxy.id()) else {
            return;
        };
        match event {
            kde_output_device_mode_v2::Event::Size { width, height } => {
                (m.width, m.height) = (width, height)
            }
            kde_output_device_mode_v2::Event::Refresh { refresh } => m.refresh = refresh,
            kde_output_device_mode_v2::Event::Preferred => m.preferred = true,
            kde_output_device_mode_v2::Event::Removed => m.removed = true,
            _ => {}
        }
    }
}

impl Dispatch<KdeOutputOrderV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &KdeOutputOrderV1,
        event: kde_output_order_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            kde_output_order_v1::Event::Output { output_name } => {
                if state.order_done {
                    state.order.clear();
                    state.order_done = false;
                }
                state.order.push(output_name);
            }
            kde_output_order_v1::Event::Done => state.order_done = true,
        }
    }
}

impl Dispatch<KdeOutputConfigurationV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &KdeOutputConfigurationV2,
        event: kde_output_configuration_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            kde_output_configuration_v2::Event::Applied => state.outcome = Some(Ok(())),
            kde_output_configuration_v2::Event::Failed => state.outcome = Some(Err(String::new())),
            kde_output_configuration_v2::Event::FailureReason { reason } => {
                state.failure_reason = Some(reason)
            }
        }
    }
}
