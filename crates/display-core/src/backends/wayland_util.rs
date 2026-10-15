//! Helpers shared by the Wayland backends.

use wayland_client::protocol::wl_output::Transform;

use crate::model::Rotation;

/// wl_output transforms rotate counter-clockwise; flipped variants are
/// treated as their unflipped rotation.
pub fn rotation_from_transform(t: Transform) -> Rotation {
    match t {
        Transform::_90 | Transform::Flipped90 => Rotation::Left,
        Transform::_180 | Transform::Flipped180 => Rotation::Inverted,
        Transform::_270 | Transform::Flipped270 => Rotation::Right,
        _ => Rotation::Normal,
    }
}

pub fn transform_from_rotation(r: Rotation) -> Transform {
    match r {
        Rotation::Normal => Transform::Normal,
        Rotation::Left => Transform::_90,
        Rotation::Inverted => Transform::_180,
        Rotation::Right => Transform::_270,
    }
}

/// Wayland reports make/model strings instead of EDID codes. A stable 16-bit
/// digest of the model keeps `MonitorId::product` meaningful for matching.
pub fn model_code(model: &str) -> u16 {
    // FNV-1a folded to 16 bits.
    let mut hash: u32 = 0x811c_9dc5;
    for b in model.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    ((hash >> 16) ^ (hash & 0xffff)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_round_trips() {
        for r in Rotation::ALL {
            assert_eq!(rotation_from_transform(transform_from_rotation(r)), r);
        }
    }

    #[test]
    fn model_code_is_stable_and_distinct() {
        assert_eq!(model_code("U2723QE"), model_code("U2723QE"));
        assert_ne!(model_code("U2723QE"), model_code("24MP59G"));
    }
}
