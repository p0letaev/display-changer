//! Profiles: saved layouts and how they map onto currently connected monitors.

use serde::{Deserialize, Serialize};

use crate::backend::{Capabilities, OriginRule};
use crate::layout::{self, bounding_box, logical_rect};
use crate::model::{DisplayState, Layout, Mode, MonitorId, MonitorInfo, OutputConfig, Position};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Accelerator in global-shortcut syntax, e.g. `Ctrl+Alt+Digit1`.
    pub hotkey: Option<String>,
    pub layout: Layout,
    /// Apply automatically when exactly this set of monitors is connected.
    #[serde(default)]
    pub auto_apply: bool,
}

/// Something that differs between the saved profile and what will be applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProfileWarning {
    /// A monitor from the profile is not connected; its entry is skipped.
    MissingMonitor { connector: String, vendor: String },
    /// The saved mode is unavailable; the closest one is used instead.
    ModeSubstituted {
        connector: String,
        wanted: Mode,
        used: Mode,
    },
    /// A connected monitor is not part of the profile.
    ExtraMonitor { connector: String, disabled: bool },
    /// The saved primary monitor is missing; another one became primary.
    PrimaryReassigned { connector: String },
    /// The output was saved as a mirror but its source is missing or the
    /// backend cannot mirror; it gets its own place instead.
    MirrorDropped { connector: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub layout: Layout,
    pub warnings: Vec<ProfileWarning>,
}

/// Maps a saved layout onto the connected monitors.
///
/// Monitors are matched by EDID identity first, so a profile follows a monitor
/// moved to another port. The result still has to pass [`layout::validate`].
pub fn resolve(saved: &Layout, state: &DisplayState, caps: &Capabilities) -> Resolved {
    let mut warnings = Vec::new();
    let mut used = vec![false; state.monitors.len()];
    let mut outputs = Vec::new();
    // Saved connector -> connector now, to re-point mirrors.
    let mut renamed: Vec<(String, String)> = Vec::new();

    for wanted in &saved.outputs {
        let Some(index) = find_monitor(&wanted.id, &state.monitors, &used) else {
            warnings.push(ProfileWarning::MissingMonitor {
                connector: wanted.id.connector.clone(),
                vendor: wanted.id.vendor.clone(),
            });
            continue;
        };
        used[index] = true;
        let monitor = &state.monitors[index];
        renamed.push((wanted.id.connector.clone(), monitor.id.connector.clone()));

        let mode = closest_mode(&wanted.mode, monitor);
        if mode != wanted.mode && wanted.enabled {
            warnings.push(ProfileWarning::ModeSubstituted {
                connector: monitor.id.connector.clone(),
                wanted: wanted.mode,
                used: mode,
            });
        }
        outputs.push(OutputConfig {
            id: monitor.id.clone(),
            mode,
            ..wanted.clone()
        });
    }

    // Monitors the profile does not know about: switch them off when possible,
    // otherwise park them to the right of everything else.
    for (index, monitor) in state.monitors.iter().enumerate() {
        if used[index] {
            continue;
        }
        let current = state.layout.output(&monitor.id.connector);
        let mode = current
            .map(|o| o.mode)
            .or(monitor.preferred_mode)
            .or_else(|| monitor.modes.first().copied());
        let Some(mode) = mode else { continue };
        let x = bounding_box(&Layout {
            outputs: outputs.clone(),
        })
        .map_or(0, |b| b.right());
        outputs.push(OutputConfig {
            id: monitor.id.clone(),
            enabled: !caps.disable,
            position: Position { x, y: 0 },
            mode,
            scale: current.map_or(1.0, |o| o.scale),
            rotation: current.map(|o| o.rotation).unwrap_or_default(),
            primary: false,
            mirror_of: None,
        });
        warnings.push(ProfileWarning::ExtraMonitor {
            connector: monitor.id.connector.clone(),
            disabled: caps.disable,
        });
    }

    remap_mirrors(&mut outputs, &renamed, caps, &mut warnings);

    if caps.primary && !outputs.iter().any(|o| o.enabled && o.primary) {
        for o in &mut outputs {
            o.primary = false;
        }
        if let Some(first) = outputs.iter_mut().find(|o| o.is_placed()) {
            first.primary = true;
            warnings.push(ProfileWarning::PrimaryReassigned {
                connector: first.id.connector.clone(),
            });
        }
    }

    Resolved {
        layout: Layout { outputs },
        warnings,
    }
}

/// Points mirrors at their source's current connector; a mirror whose source
/// is gone (or that the backend cannot show) becomes a normal output placed
/// to the right of the rest.
fn remap_mirrors(
    outputs: &mut [OutputConfig],
    renamed: &[(String, String)],
    caps: &Capabilities,
    warnings: &mut Vec<ProfileWarning>,
) {
    for i in 0..outputs.len() {
        let Some(saved_source) = outputs[i].mirror_of.clone() else {
            continue;
        };
        let source = renamed
            .iter()
            .find(|(old, _)| *old == saved_source)
            .map(|(_, new)| new.clone())
            .filter(|s| {
                outputs
                    .iter()
                    .any(|o| o.id.connector == *s && o.is_placed())
            });
        match source {
            Some(s) if caps.mirror => outputs[i].mirror_of = Some(s),
            _ => {
                outputs[i].mirror_of = None;
                let others = Layout {
                    outputs: outputs
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, o)| o.clone())
                        .collect(),
                };
                let x = bounding_box(&others).map_or(0, |b| b.right());
                outputs[i].position = Position { x, y: 0 };
                if outputs[i].enabled {
                    warnings.push(ProfileWarning::MirrorDropped {
                        connector: outputs[i].id.connector.clone(),
                    });
                }
            }
        }
    }
}

fn find_monitor(id: &MonitorId, monitors: &[MonitorInfo], used: &[bool]) -> Option<usize> {
    let free = |i: &usize| !used[*i];
    let same_model = |m: &MonitorInfo| m.id.vendor == id.vendor && m.id.product == id.product;
    let indices = || (0..monitors.len()).filter(free);

    let same_serial = |i: &usize| same_model(&monitors[*i]) && monitors[*i].id.serial == id.serial;
    // A monitor split into several inputs (picture-by-picture) reports the
    // same EDID on every input, so the port tells them apart.
    if id.serial.is_some()
        && let Some(i) = indices()
            .filter(same_serial)
            .find(|&i| monitors[i].id.connector == id.connector)
            .or_else(|| indices().find(same_serial))
    {
        return Some(i);
    }
    if let Some(i) =
        indices().find(|&i| same_model(&monitors[i]) && monitors[i].id.connector == id.connector)
    {
        return Some(i);
    }
    // Same model on another port, but only if it is unambiguous.
    let mut candidates =
        indices().filter(|&i| same_model(&monitors[i]) && monitors[i].id.serial.is_none());
    match (candidates.next(), candidates.next()) {
        (Some(i), None) if id.serial.is_none() => Some(i),
        _ => None,
    }
}

/// Exact mode if available, otherwise the same resolution with the closest
/// refresh rate, otherwise the monitor's preferred mode.
fn closest_mode(wanted: &Mode, monitor: &MonitorInfo) -> Mode {
    if monitor.custom_modes || monitor.modes.contains(wanted) {
        return *wanted;
    }
    monitor
        .modes
        .iter()
        .filter(|m| m.width == wanted.width && m.height == wanted.height)
        .min_by_key(|m| m.refresh_mhz.abs_diff(wanted.refresh_mhz))
        .copied()
        .or(monitor.preferred_mode)
        .or_else(|| monitor.modes.first().copied())
        .unwrap_or(*wanted)
}

/// Whether the desktop currently is what the profile describes.
///
/// A monitor the profile wants on must actually be present: otherwise two
/// profiles that differ only in which monitor is on could both look active
/// when one monitor is missing from the list.
pub fn is_active(saved: &Layout, state: &DisplayState, caps: &Capabilities) -> bool {
    let resolved = resolve(saved, state, caps);
    let incomplete = resolved.warnings.iter().any(|w| match w {
        ProfileWarning::MissingMonitor { connector, .. } => {
            saved.output(connector).is_some_and(|o| o.enabled)
        }
        ProfileWarning::ModeSubstituted { .. }
        | ProfileWarning::PrimaryReassigned { .. }
        | ProfileWarning::MirrorDropped { .. } => true,
        ProfileWarning::ExtraMonitor { .. } => false,
    });
    !incomplete && same_layout(&resolved.layout, &state.layout)
}

/// True when both layouts would produce the same desktop.
pub fn same_layout(a: &Layout, b: &Layout) -> bool {
    let a = layout::normalize(a, OriginRule::TopLeft);
    let b = layout::normalize(b, OriginRule::TopLeft);
    let key = |o: &OutputConfig| (o.id.connector.clone(), o.enabled);
    let mut ao: Vec<&OutputConfig> = a.outputs.iter().collect();
    let mut bo: Vec<&OutputConfig> = b.outputs.iter().collect();
    ao.sort_by_key(|o| key(o));
    bo.sort_by_key(|o| key(o));
    ao.len() == bo.len()
        && ao.iter().zip(&bo).all(|(x, y)| {
            x.id.connector == y.id.connector
                && x.enabled == y.enabled
                && (!x.enabled
                    || (x.mode == y.mode
                        && (x.scale - y.scale).abs() < 1e-6
                        && x.rotation == y.rotation
                        && x.primary == y.primary
                        && x.mirror_of == y.mirror_of
                        // Mirrors have no position of their own.
                        && (x.mirror_of.is_some() || logical_rect(x) == logical_rect(y))))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::DisplayBackend;
    use crate::backends::mock::{MockBackend, sample_state};

    fn caps() -> Capabilities {
        MockBackend::new().capabilities()
    }

    #[test]
    fn identical_state_resolves_without_warnings() {
        let state = sample_state();
        let resolved = resolve(&state.layout, &state, &caps());
        assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);
        assert!(same_layout(&resolved.layout, &state.layout));
    }

    #[test]
    fn follows_monitor_to_another_port_by_serial() {
        let mut state = sample_state();
        let saved = state.layout.clone();
        state.monitors[1].id.connector = "DP-2".into();
        for o in &mut state.layout.outputs {
            if o.id.connector == "DP-1" {
                o.id.connector = "DP-2".into();
            }
        }
        let resolved = resolve(&saved, &state, &caps());
        assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);
        assert!(resolved.layout.output("DP-2").unwrap().primary);
    }

    #[test]
    fn inputs_of_one_split_monitor_keep_their_ports() {
        // Picture-by-picture: DP-1 and HDMI-A-1 are two halves of one monitor
        // and report the same EDID, HDMI-A-1 listed first.
        let mut state = sample_state();
        let dell = state.monitors[1].clone();
        state.monitors[2] = MonitorInfo {
            id: MonitorId {
                connector: "HDMI-A-1".into(),
                ..dell.id.clone()
            },
            ..dell
        };
        state.monitors.swap(1, 2);
        let mut saved = state.layout.clone();
        for o in &mut saved.outputs {
            if o.id.connector == "HDMI-A-1" {
                o.id = state.monitors[1].id.clone();
                o.mode = state.monitors[1].preferred_mode.unwrap();
            }
        }
        let resolved = resolve(&saved, &state, &caps());
        for (wanted, got) in saved.outputs.iter().zip(&resolved.layout.outputs) {
            assert_eq!(wanted.id.connector, got.id.connector);
            assert_eq!(wanted.position, got.position);
        }
    }

    #[test]
    fn missing_primary_is_reassigned_and_extra_monitor_disabled() {
        // Profile: laptop + primary DELL. Connected now: laptop + LG.
        let mut state = sample_state();
        let saved = Layout {
            outputs: state.layout.outputs[..2].to_vec(),
        };
        state.monitors.remove(1);
        state.layout.outputs.remove(1);

        let resolved = resolve(&saved, &state, &caps());
        let w = &resolved.warnings;
        assert!(
            w.iter()
                .any(|w| matches!(w, ProfileWarning::MissingMonitor { .. }))
        );
        assert!(w.contains(&ProfileWarning::PrimaryReassigned {
            connector: "eDP-1".into()
        }));
        assert!(w.contains(&ProfileWarning::ExtraMonitor {
            connector: "HDMI-A-1".into(),
            disabled: true
        }));
        assert!(!resolved.layout.output("HDMI-A-1").unwrap().enabled);
    }

    #[test]
    fn substitutes_closest_refresh_rate() {
        let state = sample_state();
        let mut saved = state.layout.clone();
        saved.outputs[0].mode.refresh_mhz = 144_000;
        let resolved = resolve(&saved, &state, &caps());
        assert_eq!(resolved.layout.outputs[0].mode.refresh_mhz, 165_000);
        assert!(matches!(
            resolved.warnings[0],
            ProfileWarning::ModeSubstituted { .. }
        ));
    }

    #[test]
    fn mirrors_follow_their_source_to_another_port() {
        let mut state = sample_state();
        let mut saved = state.layout.clone();
        saved.outputs[2].mirror_of = Some("DP-1".into());
        // DELL moved from DP-1 to DP-2.
        state.monitors[1].id.connector = "DP-2".into();
        state.layout.outputs[1].id.connector = "DP-2".into();
        let resolved = resolve(&saved, &state, &caps());
        assert_eq!(
            resolved.layout.outputs[2].mirror_of.as_deref(),
            Some("DP-2")
        );

        // Source gone: the mirror stands on its own.
        state.monitors.remove(1);
        state.layout.outputs.remove(1);
        let resolved = resolve(&saved, &state, &caps());
        let lg = resolved.layout.output("HDMI-A-1").unwrap();
        assert_eq!(lg.mirror_of, None);
        assert!(resolved.warnings.contains(&ProfileWarning::MirrorDropped {
            connector: "HDMI-A-1".into()
        }));
    }

    #[test]
    fn only_the_matching_profile_is_active_when_a_monitor_is_missing() {
        // Profile A: laptop only. Profile B: DELL only. The laptop is off and
        // the platform does not list it at all.
        let full = sample_state();
        let off = |o: &OutputConfig| OutputConfig {
            enabled: false,
            primary: false,
            ..o.clone()
        };
        let on = |o: &OutputConfig| OutputConfig {
            position: Position { x: 0, y: 0 },
            primary: true,
            ..o.clone()
        };
        let a = Layout {
            outputs: vec![
                on(&full.layout.outputs[0]),
                off(&full.layout.outputs[1]),
                off(&full.layout.outputs[2]),
            ],
        };
        let b = Layout {
            outputs: vec![
                off(&full.layout.outputs[0]),
                on(&full.layout.outputs[1]),
                off(&full.layout.outputs[2]),
            ],
        };

        let mut state = full.clone();
        state.monitors.remove(0);
        state.layout = Layout {
            outputs: vec![on(&full.layout.outputs[1]), off(&full.layout.outputs[2])],
        };

        assert!(is_active(&b, &state, &caps()));
        assert!(
            !is_active(&a, &state, &caps()),
            "laptop is not there, A cannot be active"
        );
    }

    #[test]
    fn same_layout_ignores_translation() {
        let a = sample_state().layout;
        let mut b = a.clone();
        for o in &mut b.outputs {
            o.position.x += 500;
        }
        assert!(same_layout(&a, &b));
        b.outputs[0].scale = 1.0;
        assert!(!same_layout(&a, &b));
    }
}
