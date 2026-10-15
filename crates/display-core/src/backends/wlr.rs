//! wlroots backend over `wlr-output-management-unstable-v1`: Sway, Hyprland,
//! river, labwc, Wayfire, niri, COSMIC.
//!
//! Every call opens a fresh connection: the compositor sends the complete
//! output state on bind, so there is nothing to keep in sync between calls.

use std::collections::HashMap;

use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_output::Transform, wl_registry};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, event_created_child,
};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1,
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};

use super::wayland_util::{model_code, rotation_from_transform, transform_from_rotation};
use crate::backend::{Capabilities, DisplayBackend, OriginRule};
use crate::error::{Error, Result};
use crate::layout;
use crate::model::{
    DisplayState, Layout, Mode, MonitorId, MonitorInfo, OutputConfig, PhysicalSize, Position,
    Rotation,
};

/// Used for outputs that report no modes at all; applied as a custom mode.
const FALLBACK_MODE: Mode = Mode {
    width: 1920,
    height: 1080,
    refresh_mhz: 60_000,
};

#[derive(Default)]
struct HeadState {
    proxy: Option<ZwlrOutputHeadV1>,
    name: String,
    description: String,
    make: String,
    model: String,
    serial: Option<String>,
    physical: (i32, i32),
    modes: Vec<ObjectId>,
    current_mode: Option<ObjectId>,
    enabled: bool,
    position: (i32, i32),
    transform: Option<Transform>,
    scale: f64,
    finished: bool,
}

struct ModeState {
    proxy: ZwlrOutputModeV1,
    width: i32,
    height: i32,
    refresh: i32,
    preferred: bool,
    finished: bool,
}

#[derive(Default)]
struct State {
    heads: Vec<HeadState>,
    modes: HashMap<ObjectId, ModeState>,
    serial: Option<u32>,
    outcome: Option<std::result::Result<(), &'static str>>,
}

impl State {
    fn head_mut(&mut self, head: &ZwlrOutputHeadV1) -> Option<&mut HeadState> {
        self.heads
            .iter_mut()
            .find(|h| h.proxy.as_ref() == Some(head))
    }

    fn mode_of(&self, id: &ObjectId) -> Option<Mode> {
        self.modes.get(id).filter(|m| !m.finished).map(|m| Mode {
            width: m.width as u32,
            height: m.height as u32,
            refresh_mhz: m.refresh.max(0) as u32,
        })
    }

    fn live_heads(&self) -> impl Iterator<Item = &HeadState> {
        self.heads.iter().filter(|h| !h.finished)
    }
}

struct Session {
    queue: EventQueue<State>,
    state: State,
    manager: ZwlrOutputManagerV1,
}

impl Session {
    fn open() -> Result<Self> {
        let conn = Connection::connect_to_env()
            .map_err(|e| Error::Backend(format!("Wayland connect: {e}")))?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn)
            .map_err(|e| Error::Backend(format!("Wayland registry: {e}")))?;
        let qh = queue.handle();
        let manager: ZwlrOutputManagerV1 = globals
            .bind(&qh, 1..=4, ())
            .map_err(|_| Error::Unsupported("compositor lacks wlr-output-management".into()))?;
        let mut state = State::default();
        // Heads, then their modes, then `done`.
        while state.serial.is_none() {
            queue
                .blocking_dispatch(&mut state)
                .map_err(|e| Error::Backend(format!("Wayland dispatch: {e}")))?;
        }
        Ok(Self {
            queue,
            state,
            manager,
        })
    }

    fn to_display_state(&self) -> DisplayState {
        let mut monitors = Vec::new();
        let mut outputs = Vec::new();
        for h in self.state.live_heads() {
            let mut modes: Vec<Mode> = h
                .modes
                .iter()
                .filter_map(|id| self.state.mode_of(id))
                .collect();
            modes.dedup();
            let preferred = h
                .modes
                .iter()
                .find(|id| {
                    self.state
                        .modes
                        .get(*id)
                        .is_some_and(|m| m.preferred && !m.finished)
                })
                .and_then(|id| self.state.mode_of(id));
            let current = h
                .current_mode
                .as_ref()
                .and_then(|id| self.state.mode_of(id));
            // Virtual outputs (headless, VNC) take custom modes and may
            // advertise none while off.
            let custom_modes = modes.len() <= 1 && preferred.is_none();
            if modes.is_empty() {
                modes.push(current.unwrap_or(FALLBACK_MODE));
            }
            let mode = current.or(preferred).unwrap_or(modes[0]);

            let id = MonitorId {
                vendor: h.make.clone(),
                product: model_code(&h.model),
                serial: h.serial.clone().filter(|s| !s.is_empty()),
                connector: h.name.clone(),
            };
            let name = if h.model.is_empty() {
                h.description.clone()
            } else {
                h.model.clone()
            };
            monitors.push(MonitorInfo {
                id: id.clone(),
                name: if name.is_empty() {
                    h.name.clone()
                } else {
                    name
                },
                builtin: ["eDP", "LVDS", "DSI"].iter().any(|p| h.name.starts_with(p)),
                physical_size: (h.physical.0 > 0 && h.physical.1 > 0).then_some(PhysicalSize {
                    width_mm: h.physical.0 as u32,
                    height_mm: h.physical.1 as u32,
                }),
                modes,
                preferred_mode: preferred,
                rotations: Rotation::ALL.to_vec(),
                scales: Vec::new(),
                custom_modes,
            });
            outputs.push(OutputConfig {
                id,
                enabled: h.enabled,
                position: Position {
                    x: h.position.0,
                    y: h.position.1,
                },
                mode,
                scale: if h.scale > 0.0 { h.scale } else { 1.0 },
                rotation: h
                    .transform
                    .map_or(Rotation::Normal, rotation_from_transform),
                primary: false,
                // wlr-output-management cannot express mirroring.
                mirror_of: None,
            });
        }
        DisplayState {
            monitors,
            layout: Layout { outputs },
        }
    }

    fn apply(mut self, target: &Layout) -> Result<()> {
        let qh = self.queue.handle();
        let serial = self.state.serial.expect("set in open");
        let config = self.manager.create_configuration(serial, &qh, ());

        for h in self.state.live_heads() {
            let Some(proxy) = &h.proxy else { continue };
            let Some(wanted) = target.output(&h.name) else {
                // Untouched heads must still be mentioned, in their current state.
                if h.enabled {
                    config.enable_head(proxy, &qh, ());
                } else {
                    config.disable_head(proxy);
                }
                continue;
            };
            if !wanted.enabled {
                config.disable_head(proxy);
                continue;
            }
            let ch = config.enable_head(proxy, &qh, ());
            let mode = h
                .modes
                .iter()
                .find(|id| self.state.mode_of(id) == Some(wanted.mode));
            match mode.and_then(|id| self.state.modes.get(id)) {
                Some(m) => ch.set_mode(&m.proxy),
                None => ch.set_custom_mode(
                    wanted.mode.width as i32,
                    wanted.mode.height as i32,
                    wanted.mode.refresh_mhz as i32,
                ),
            }
            ch.set_position(wanted.position.x, wanted.position.y);
            ch.set_transform(transform_from_rotation(wanted.rotation));
            ch.set_scale(wanted.scale);
        }

        config.apply();
        while self.state.outcome.is_none() {
            self.queue
                .blocking_dispatch(&mut self.state)
                .map_err(|e| Error::Backend(format!("Wayland dispatch: {e}")))?;
        }
        config.destroy();
        let _ = self.queue.flush();
        match self.state.outcome {
            Some(Ok(())) => Ok(()),
            Some(Err(why)) => Err(Error::Backend(format!(
                "compositor {why} the configuration"
            ))),
            None => unreachable!(),
        }
    }
}

pub struct WlrBackend;

impl WlrBackend {
    /// Succeeds only when the compositor offers wlr-output-management.
    pub fn connect() -> Result<Self> {
        Session::open()?;
        Ok(Self)
    }
}

impl DisplayBackend for WlrBackend {
    fn name(&self) -> &'static str {
        "wlroots"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            scale: true,
            fractional_scale: true,
            rotation: true,
            // wlroots has no notion of a primary output.
            primary: false,
            disable: true,
            origin: OriginRule::TopLeft,
            scaled_layout: true,
            mirror: false,
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

impl Dispatch<ZwlrOutputManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrOutputManagerV1,
        event: zwlr_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Head { head } => {
                state.heads.push(HeadState {
                    proxy: Some(head),
                    scale: 1.0,
                    ..Default::default()
                });
            }
            zwlr_output_manager_v1::Event::Done { serial } => state.serial = Some(serial),
            _ => {}
        }
    }

    event_created_child!(State, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for State {
    fn event(
        state: &mut Self,
        head: &ZwlrOutputHeadV1,
        event: zwlr_output_head_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_output_head_v1::Event;
        if let Event::Mode { mode } = &event {
            state.modes.insert(
                mode.id(),
                ModeState {
                    proxy: mode.clone(),
                    width: 0,
                    height: 0,
                    refresh: 0,
                    preferred: false,
                    finished: false,
                },
            );
        }
        let Some(h) = state.head_mut(head) else {
            return;
        };
        match event {
            Event::Name { name } => h.name = name,
            Event::Description { description } => h.description = description,
            Event::Make { make } => h.make = make,
            Event::Model { model } => h.model = model,
            Event::SerialNumber { serial_number } => h.serial = Some(serial_number),
            Event::PhysicalSize { width, height } => h.physical = (width, height),
            Event::Mode { mode } => h.modes.push(mode.id()),
            Event::Enabled { enabled } => h.enabled = enabled != 0,
            Event::CurrentMode { mode } => h.current_mode = Some(mode.id()),
            Event::Position { x, y } => h.position = (x, y),
            Event::Transform {
                transform: WEnum::Value(t),
            } => h.transform = Some(t),
            Event::Scale { scale } => h.scale = scale,
            Event::Finished => h.finished = true,
            _ => {}
        }
    }

    event_created_child!(State, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for State {
    fn event(
        state: &mut Self,
        mode: &ZwlrOutputModeV1,
        event: zwlr_output_mode_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(m) = state.modes.get_mut(&mode.id()) else {
            return;
        };
        match event {
            zwlr_output_mode_v1::Event::Size { width, height } => {
                (m.width, m.height) = (width, height)
            }
            zwlr_output_mode_v1::Event::Refresh { refresh } => m.refresh = refresh,
            zwlr_output_mode_v1::Event::Preferred => m.preferred = true,
            zwlr_output_mode_v1::Event::Finished => m.finished = true,
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrOutputConfigurationV1,
        event: zwlr_output_configuration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.outcome = match event {
            zwlr_output_configuration_v1::Event::Succeeded => Some(Ok(())),
            zwlr_output_configuration_v1::Event::Failed => Some(Err("rejected")),
            zwlr_output_configuration_v1::Event::Cancelled => Some(Err("cancelled")),
            _ => state.outcome,
        };
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrOutputConfigurationHeadV1,
        _: <ZwlrOutputConfigurationHeadV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
