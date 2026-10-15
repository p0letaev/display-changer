//! Layout geometry: logical rectangles, validation and normalization.
//!
//! The frontend mirrors these rules for live feedback while dragging; this
//! module stays the source of truth before anything is applied.

use std::collections::HashSet;

use crate::backend::{Capabilities, OriginRule};
use crate::error::{Error, LayoutIssue, Result};
use crate::model::{Layout, MonitorInfo, OutputConfig, Rotation};

pub const MIN_SCALE: f64 = 0.5;
pub const MAX_SCALE: f64 = 4.0;

/// Axis-aligned rectangle in logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    /// True when the interiors intersect. Shared edges do not count.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// True when the rectangles share an edge or a corner without overlapping.
    pub fn touches(&self, other: &Rect) -> bool {
        !self.overlaps(other)
            && self.x <= other.right()
            && other.x <= self.right()
            && self.y <= other.bottom()
            && other.y <= self.bottom()
    }
}

/// Size of an output on the desktop: mode size, rotated, divided by scale.
pub fn logical_rect(output: &OutputConfig) -> Rect {
    desktop_rect(output, true)
}

/// Like [`logical_rect`], but ignores scale when the backend lays out the
/// desktop in physical pixels (see [`Capabilities::scaled_layout`]).
pub fn desktop_rect(output: &OutputConfig, scaled_layout: bool) -> Rect {
    let (w, h) = if output.rotation.swaps_axes() {
        (output.mode.height, output.mode.width)
    } else {
        (output.mode.width, output.mode.height)
    };
    let scale = if scaled_layout { output.scale } else { 1.0 };
    Rect {
        x: output.position.x,
        y: output.position.y,
        width: (f64::from(w) / scale).round() as i32,
        height: (f64::from(h) / scale).round() as i32,
    }
}

/// Bounding box of all enabled outputs, or `None` if none are enabled.
pub fn bounding_box(layout: &Layout) -> Option<Rect> {
    let mut rects = layout.placed().map(logical_rect);
    let first = rects.next()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
    for r in rects {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.right());
        y1 = y1.max(r.bottom());
    }
    Some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// Checks that `layout` can be applied to the connected `monitors`.
/// Collects every issue instead of stopping at the first one.
pub fn validate(layout: &Layout, monitors: &[MonitorInfo], caps: &Capabilities) -> Result<()> {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();

    for output in &layout.outputs {
        let connector = &output.id.connector;
        if !seen.insert(connector.as_str()) {
            issues.push(LayoutIssue::DuplicateOutput {
                connector: connector.clone(),
            });
            continue;
        }
        let Some(monitor) = monitors.iter().find(|m| &m.id.connector == connector) else {
            issues.push(LayoutIssue::UnknownMonitor {
                connector: connector.clone(),
            });
            continue;
        };
        if !output.enabled {
            if !caps.disable {
                issues.push(LayoutIssue::DisableUnsupported {
                    connector: connector.clone(),
                });
            }
            continue;
        }
        if !monitor.custom_modes && !monitor.modes.contains(&output.mode) {
            issues.push(LayoutIssue::UnsupportedMode {
                connector: connector.clone(),
                mode: output.mode,
            });
        }
        let listed = monitor.scales.is_empty()
            || monitor
                .scales
                .iter()
                .any(|s| (s - output.scale).abs() < 1e-6);
        if !scale_allowed(output.scale, caps) || !listed {
            issues.push(LayoutIssue::InvalidScale {
                connector: connector.clone(),
                scale: output.scale,
            });
        }
        if (output.rotation != Rotation::Normal && !caps.rotation)
            || !monitor.rotations.contains(&output.rotation)
        {
            issues.push(LayoutIssue::RotationUnsupported {
                connector: connector.clone(),
            });
        }
        if let Some(source) = &output.mirror_of {
            if !caps.mirror {
                issues.push(LayoutIssue::MirrorUnsupported {
                    connector: connector.clone(),
                });
            }
            // The source must be a placed output: no mirrors of mirrors.
            let ok = source != connector && layout.output(source).is_some_and(|s| s.is_placed());
            if !ok {
                issues.push(LayoutIssue::InvalidMirrorSource {
                    connector: connector.clone(),
                    source_name: source.clone(),
                });
            }
            if output.primary {
                issues.push(LayoutIssue::MirrorPrimary {
                    connector: connector.clone(),
                });
            }
        }
    }

    // Mirrors sit on their source and take no part in the geometry checks.
    let enabled: Vec<&OutputConfig> = layout.placed().collect();
    if enabled.is_empty() {
        issues.push(LayoutIssue::NoEnabledOutputs);
        return Err(Error::InvalidLayout(issues));
    }

    let primaries = enabled.iter().filter(|o| o.primary).count();
    if caps.primary && primaries != 1 {
        issues.push(LayoutIssue::PrimaryCount { count: primaries });
    }

    let rects: Vec<Rect> = enabled
        .iter()
        .map(|o| desktop_rect(o, caps.scaled_layout))
        .collect();
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            if rects[i].overlaps(&rects[j]) {
                issues.push(LayoutIssue::Overlap {
                    a: enabled[i].id.connector.clone(),
                    b: enabled[j].id.connector.clone(),
                });
            }
        }
    }
    for i in detached(&rects) {
        issues.push(LayoutIssue::Detached {
            connector: enabled[i].id.connector.clone(),
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(Error::InvalidLayout(issues))
    }
}

fn scale_allowed(scale: f64, caps: &Capabilities) -> bool {
    if !caps.scale {
        return scale == 1.0;
    }
    if !(MIN_SCALE..=MAX_SCALE).contains(&scale) {
        return false;
    }
    caps.fractional_scale || scale.fract() == 0.0
}

/// Indices of rectangles not reachable from the first one through touching edges.
fn detached(rects: &[Rect]) -> Vec<usize> {
    let mut reached = vec![false; rects.len()];
    let mut stack = vec![0];
    reached[0] = true;
    while let Some(i) = stack.pop() {
        for j in 0..rects.len() {
            if !reached[j] && rects[i].touches(&rects[j]) {
                reached[j] = true;
                stack.push(j);
            }
        }
    }
    (0..rects.len()).filter(|&i| !reached[i]).collect()
}

/// Shifts every output so the layout is anchored the way the backend expects.
pub fn normalize(layout: &Layout, rule: OriginRule) -> Layout {
    let anchor = match rule {
        OriginRule::TopLeft => bounding_box(layout).map(|b| (b.x, b.y)),
        OriginRule::Primary => layout
            .enabled()
            .find(|o| o.primary)
            .map(|o| (o.position.x, o.position.y)),
    };
    let (dx, dy) = anchor.unwrap_or((0, 0));
    let mut out = layout.clone();
    for o in &mut out.outputs {
        o.position.x -= dx;
        o.position.y -= dy;
    }
    // A mirror shows its source's area, so it takes the source's position.
    let positions: Vec<(String, crate::model::Position)> = out
        .placed()
        .map(|o| (o.id.connector.clone(), o.position))
        .collect();
    for o in &mut out.outputs {
        if let Some(source) = &o.mirror_of
            && let Some((_, p)) = positions.iter().find(|(c, _)| c == source)
        {
            o.position = *p;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mode, MonitorId, Position};

    const CAPS: Capabilities = Capabilities {
        scale: true,
        fractional_scale: true,
        rotation: true,
        primary: true,
        disable: true,
        origin: OriginRule::TopLeft,
        scaled_layout: true,
        mirror: true,
    };

    fn mode(width: u32, height: u32) -> Mode {
        Mode {
            width,
            height,
            refresh_mhz: 60_000,
        }
    }

    fn id(connector: &str) -> MonitorId {
        MonitorId {
            vendor: "TST".into(),
            product: 1,
            serial: None,
            connector: connector.into(),
        }
    }

    fn monitor(connector: &str) -> MonitorInfo {
        MonitorInfo {
            id: id(connector),
            name: connector.into(),
            builtin: false,
            physical_size: None,
            modes: vec![mode(1920, 1080), mode(2560, 1440)],
            preferred_mode: Some(mode(1920, 1080)),
            rotations: Rotation::ALL.to_vec(),
            scales: Vec::new(),
            custom_modes: false,
        }
    }

    fn output(connector: &str, x: i32, y: i32, primary: bool) -> OutputConfig {
        OutputConfig {
            id: id(connector),
            enabled: true,
            position: Position { x, y },
            mode: mode(1920, 1080),
            scale: 1.0,
            rotation: Rotation::Normal,
            primary,
            mirror_of: None,
        }
    }

    fn issues(result: Result<()>) -> Vec<LayoutIssue> {
        match result {
            Err(Error::InvalidLayout(issues)) => issues,
            other => panic!("expected InvalidLayout, got {other:?}"),
        }
    }

    #[test]
    fn logical_rect_applies_rotation_and_scale() {
        let mut o = output("A", 0, 0, true);
        o.mode = mode(2560, 1440);
        o.scale = 1.25;
        o.rotation = Rotation::Left;
        assert_eq!(
            logical_rect(&o),
            Rect {
                x: 0,
                y: 0,
                width: 1152,
                height: 2048
            }
        );
    }

    #[test]
    fn side_by_side_layout_is_valid() {
        let layout = Layout {
            outputs: vec![output("A", 0, 0, true), output("B", 1920, 0, false)],
        };
        assert!(validate(&layout, &[monitor("A"), monitor("B")], &CAPS).is_ok());
    }

    #[test]
    fn corner_contact_counts_as_adjacent() {
        let layout = Layout {
            outputs: vec![output("A", 0, 0, true), output("B", 1920, 1080, false)],
        };
        assert!(validate(&layout, &[monitor("A"), monitor("B")], &CAPS).is_ok());
    }

    #[test]
    fn detects_overlap_and_gap() {
        let monitors = [monitor("A"), monitor("B"), monitor("C")];
        let layout = Layout {
            outputs: vec![
                output("A", 0, 0, true),
                output("B", 1000, 0, false),
                output("C", 5000, 0, false),
            ],
        };
        let found = issues(validate(&layout, &monitors, &CAPS));
        assert!(found.contains(&LayoutIssue::Overlap {
            a: "A".into(),
            b: "B".into()
        }));
        assert!(found.contains(&LayoutIssue::Detached {
            connector: "C".into()
        }));
    }

    #[test]
    fn requires_exactly_one_primary() {
        let layout = Layout {
            outputs: vec![output("A", 0, 0, true), output("B", 1920, 0, true)],
        };
        let found = issues(validate(&layout, &[monitor("A"), monitor("B")], &CAPS));
        assert_eq!(found, vec![LayoutIssue::PrimaryCount { count: 2 }]);
    }

    #[test]
    fn rejects_unknown_mode_and_fractional_scale_when_unsupported() {
        let mut o = output("A", 0, 0, true);
        o.mode = mode(800, 600);
        o.scale = 1.5;
        let caps = Capabilities {
            fractional_scale: false,
            ..CAPS
        };
        let found = issues(validate(
            &Layout { outputs: vec![o] },
            &[monitor("A")],
            &caps,
        ));
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn disabled_outputs_are_ignored_by_geometry() {
        let mut b = output("B", 0, 0, false);
        b.enabled = false;
        let layout = Layout {
            outputs: vec![output("A", 0, 0, true), b],
        };
        assert!(validate(&layout, &[monitor("A"), monitor("B")], &CAPS).is_ok());
    }

    #[test]
    fn mirror_ignores_geometry_and_follows_source() {
        let mut b = output("B", 5000, 5000, false);
        b.mirror_of = Some("A".into());
        let layout = Layout {
            outputs: vec![output("A", 100, 50, true), b],
        };
        assert!(validate(&layout, &[monitor("A"), monitor("B")], &CAPS).is_ok());
        let n = normalize(&layout, OriginRule::TopLeft);
        assert_eq!(n.outputs[1].position, n.outputs[0].position);
        assert_eq!(bounding_box(&layout).unwrap().width, 1920);
    }

    #[test]
    fn rejects_bad_mirrors() {
        let monitors = [monitor("A"), monitor("B"), monitor("C")];
        // Mirror of a mirror, mirror as primary, unsupported backend.
        let mut b = output("B", 0, 0, true);
        b.mirror_of = Some("A".into());
        let mut c = output("C", 0, 0, false);
        c.mirror_of = Some("B".into());
        let layout = Layout {
            outputs: vec![output("A", 0, 0, false), b, c],
        };
        let caps = Capabilities {
            mirror: false,
            ..CAPS
        };
        let found = issues(validate(&layout, &monitors, &caps));
        assert!(found.contains(&LayoutIssue::MirrorPrimary {
            connector: "B".into()
        }));
        assert!(found.contains(&LayoutIssue::InvalidMirrorSource {
            connector: "C".into(),
            source_name: "B".into()
        }));
        assert!(found.contains(&LayoutIssue::MirrorUnsupported {
            connector: "B".into()
        }));
    }

    #[test]
    fn normalize_anchors_by_rule() {
        let layout = Layout {
            outputs: vec![output("A", 500, 300, false), output("B", 2420, 100, true)],
        };

        let top_left = normalize(&layout, OriginRule::TopLeft);
        assert_eq!(top_left.outputs[0].position, Position { x: 0, y: 200 });
        assert_eq!(top_left.outputs[1].position, Position { x: 1920, y: 0 });

        let primary = normalize(&layout, OriginRule::Primary);
        assert_eq!(primary.outputs[0].position, Position { x: -1920, y: 200 });
        assert_eq!(primary.outputs[1].position, Position { x: 0, y: 0 });
    }
}
