//! In-memory backend with three fake monitors, for UI work and tests.

use std::sync::Mutex;

use crate::backend::{Capabilities, DisplayBackend, OriginRule};
use crate::error::{Error, Result};
use crate::layout;
use crate::model::{
    DisplayState, Layout, Mode, MonitorId, MonitorInfo, OutputConfig, PhysicalSize, Position,
    Rotation,
};
use crate::profile::Profile;

pub struct MockBackend {
    state: Mutex<DisplayState>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(sample_state()),
        }
    }

    pub fn with_state(state: DisplayState) -> Self {
        Self {
            state: Mutex::new(state),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, DisplayState>> {
        self.state
            .lock()
            .map_err(|_| Error::Backend("mock state poisoned".into()))
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayBackend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
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
        }
    }

    fn current_state(&self) -> Result<DisplayState> {
        Ok(self.lock()?.clone())
    }

    fn apply(&self, layout: &Layout) -> Result<()> {
        self.validate(layout)?;
        let normalized = layout::normalize(layout, self.capabilities().origin);
        log::info!(
            "mock: applying layout with {} outputs",
            normalized.outputs.len()
        );
        self.lock()?.layout = normalized;
        Ok(())
    }
}

fn modes(list: &[(u32, u32, &[u32])]) -> Vec<Mode> {
    list.iter()
        .flat_map(|&(width, height, rates)| {
            rates.iter().map(move |&refresh_mhz| Mode {
                width,
                height,
                refresh_mhz,
            })
        })
        .collect()
}

/// Laptop on the left, 4K monitor as primary in the middle, portrait monitor on the right.
pub fn sample_state() -> DisplayState {
    let laptop = MonitorInfo {
        id: MonitorId {
            vendor: "BOE".into(),
            product: 0x0a8c,
            serial: None,
            connector: "eDP-1".into(),
        },
        name: "Built-in Display".into(),
        builtin: true,
        physical_size: Some(PhysicalSize {
            width_mm: 344,
            height_mm: 215,
        }),
        modes: modes(&[
            (2560, 1600, &[165_000, 120_000, 60_000]),
            (1920, 1200, &[165_000, 60_000]),
            (1280, 800, &[60_000]),
        ]),
        preferred_mode: Some(Mode {
            width: 2560,
            height: 1600,
            refresh_mhz: 165_000,
        }),
        rotations: Rotation::ALL.to_vec(),
        scales: Vec::new(),
        custom_modes: false,
    };
    let dell = MonitorInfo {
        id: MonitorId {
            vendor: "DEL".into(),
            product: 0x4291,
            serial: Some("7XQ2Y93".into()),
            connector: "DP-1".into(),
        },
        name: "DELL U2723QE".into(),
        builtin: false,
        physical_size: Some(PhysicalSize {
            width_mm: 597,
            height_mm: 336,
        }),
        modes: modes(&[
            (3840, 2160, &[60_000, 59_940, 30_000]),
            (2560, 1440, &[60_000]),
            (1920, 1080, &[60_000, 50_000]),
        ]),
        preferred_mode: Some(Mode {
            width: 3840,
            height: 2160,
            refresh_mhz: 60_000,
        }),
        rotations: Rotation::ALL.to_vec(),
        scales: Vec::new(),
        custom_modes: false,
    };
    let lg = MonitorInfo {
        id: MonitorId {
            vendor: "GSM".into(),
            product: 0x5b7f,
            serial: Some("012NTKF4B331".into()),
            connector: "HDMI-A-1".into(),
        },
        name: "LG 24MP59G".into(),
        builtin: false,
        physical_size: Some(PhysicalSize {
            width_mm: 527,
            height_mm: 296,
        }),
        modes: modes(&[
            (1920, 1080, &[75_000, 60_000]),
            (1680, 1050, &[60_000]),
            (1280, 720, &[60_000]),
        ]),
        preferred_mode: Some(Mode {
            width: 1920,
            height: 1080,
            refresh_mhz: 75_000,
        }),
        rotations: Rotation::ALL.to_vec(),
        scales: Vec::new(),
        custom_modes: false,
    };

    let layout = Layout {
        outputs: vec![
            OutputConfig {
                id: laptop.id.clone(),
                enabled: true,
                position: Position { x: 0, y: 160 },
                mode: Mode {
                    width: 2560,
                    height: 1600,
                    refresh_mhz: 165_000,
                },
                scale: 1.25,
                rotation: Rotation::Normal,
                primary: false,
            },
            OutputConfig {
                id: dell.id.clone(),
                enabled: true,
                position: Position { x: 2048, y: 0 },
                mode: Mode {
                    width: 3840,
                    height: 2160,
                    refresh_mhz: 60_000,
                },
                scale: 1.5,
                rotation: Rotation::Normal,
                primary: true,
            },
            OutputConfig {
                id: lg.id.clone(),
                enabled: true,
                position: Position { x: 4608, y: 0 },
                mode: Mode {
                    width: 1920,
                    height: 1080,
                    refresh_mhz: 75_000,
                },
                scale: 1.0,
                rotation: Rotation::Left,
                primary: false,
            },
        ],
    };

    DisplayState {
        monitors: vec![laptop, dell, lg],
        layout,
    }
}

/// Demo profiles for the sample monitors, seeded on first run of the mock backend.
pub fn demo_profiles() -> Vec<Profile> {
    let state = sample_state();
    let [laptop, dell, lg] = [0, 1, 2].map(|i| state.layout.outputs[i].clone());
    let off = |o: &OutputConfig| OutputConfig {
        enabled: false,
        primary: false,
        ..o.clone()
    };
    let profile =
        |id: &str, name: &str, hotkey: Option<&str>, outputs: Vec<OutputConfig>| Profile {
            id: id.into(),
            name: name.into(),
            hotkey: hotkey.map(Into::into),
            layout: Layout { outputs },
            auto_apply: false,
        };

    vec![
        profile(
            "demo-desk",
            "Рабочий стол",
            Some("Ctrl+Alt+Shift+Digit1"),
            state.layout.outputs.clone(),
        ),
        profile(
            "demo-gaming",
            "Игры",
            Some("Ctrl+Alt+Shift+Digit2"),
            vec![
                OutputConfig {
                    position: Position { x: 0, y: 0 },
                    primary: true,
                    ..laptop.clone()
                },
                off(&dell),
                off(&lg),
            ],
        ),
        profile(
            "demo-external",
            "Только внешние",
            Some("Ctrl+Alt+Shift+Digit3"),
            vec![
                off(&laptop),
                OutputConfig {
                    position: Position { x: 0, y: 0 },
                    ..dell.clone()
                },
                OutputConfig {
                    position: Position { x: 2560, y: 180 },
                    rotation: Rotation::Normal,
                    ..lg.clone()
                },
            ],
        ),
        profile(
            "demo-presentation",
            "Презентация",
            None,
            vec![
                OutputConfig {
                    position: Position { x: 0, y: 0 },
                    mode: Mode {
                        width: 1920,
                        height: 1200,
                        refresh_mhz: 60_000,
                    },
                    scale: 1.0,
                    ..laptop
                },
                OutputConfig {
                    position: Position { x: 1920, y: 0 },
                    mode: Mode {
                        width: 1920,
                        height: 1080,
                        refresh_mhz: 60_000,
                    },
                    scale: 1.0,
                    ..dell
                },
                off(&lg),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_state_is_valid() {
        let backend = MockBackend::new();
        let state = backend.current_state().unwrap();
        backend.validate(&state.layout).unwrap();
    }

    #[test]
    fn apply_normalizes_and_persists() {
        let backend = MockBackend::new();
        let mut layout = backend.current_state().unwrap().layout;
        for o in &mut layout.outputs {
            o.position.x += 100;
            o.position.y -= 50;
        }
        backend.apply(&layout).unwrap();
        let applied = backend.current_state().unwrap().layout;
        assert_eq!(applied.outputs[0].position, Position { x: 0, y: 160 });
    }

    #[test]
    fn apply_rejects_invalid_layout_and_keeps_state() {
        let backend = MockBackend::new();
        let before = backend.current_state().unwrap();
        let mut layout = before.layout.clone();
        layout.outputs[1].position = Position { x: 0, y: 0 };
        assert!(matches!(
            backend.apply(&layout),
            Err(Error::InvalidLayout(_))
        ));
        assert_eq!(backend.current_state().unwrap(), before);
    }

    #[test]
    fn demo_profiles_are_valid() {
        let backend = MockBackend::new();
        let state = backend.current_state().unwrap();
        for p in demo_profiles() {
            let resolved = crate::profile::resolve(&p.layout, &state, &backend.capabilities());
            assert!(
                resolved.warnings.is_empty(),
                "{}: {:?}",
                p.name,
                resolved.warnings
            );
            backend
                .validate(&resolved.layout)
                .unwrap_or_else(|e| panic!("{}: {e}", p.name));
        }
    }

    #[test]
    fn errors_serialize_with_kind_tag() {
        let err = Error::InvalidLayout(vec![crate::LayoutIssue::PrimaryCount { count: 0 }]);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "invalidLayout");
        assert_eq!(json["detail"][0]["type"], "primaryCount");
    }
}
