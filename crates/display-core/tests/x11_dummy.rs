//! Applies layouts on a throwaway X server with the dummy driver. Skipped
//! unless `DISPLAY_CHANGER_XTEST` names that server, so it never touches a
//! real desktop. Setup is in `scripts/x11-test-server.sh`.

#![cfg(target_os = "linux")]

use display_core::backends::x11::X11Backend;
use display_core::{DisplayBackend, Error, Layout, Mode, OutputConfig, Position, Rotation};

const FHD: Mode = Mode {
    width: 1920,
    height: 1080,
    refresh_mhz: 60_000,
};
const XGA: Mode = Mode {
    width: 1024,
    height: 768,
    refresh_mhz: 60_004,
};

fn backend() -> Option<X11Backend> {
    let display = std::env::var("DISPLAY_CHANGER_XTEST").ok()?;
    // SAFETY: set before the only X connection in this process is opened.
    unsafe {
        std::env::set_var("DISPLAY", display);
        std::env::set_var("DISPLAY_CHANGER_NO_XFCONF", "1");
    }
    Some(X11Backend::connect().expect("connect to test X server"))
}

fn out<'a>(layout: &'a mut Layout, name: &str) -> &'a mut OutputConfig {
    layout
        .outputs
        .iter_mut()
        .find(|o| o.id.connector == name)
        .expect(name)
}

fn get<'a>(layout: &'a Layout, name: &str) -> &'a OutputConfig {
    layout.output(name).expect(name)
}

#[test]
fn reconfigures_three_outputs_and_restores() {
    let Some(x) = backend() else {
        eprintln!("DISPLAY_CHANGER_XTEST not set, skipping");
        return;
    };
    let before = x.current_state().unwrap();
    assert_eq!(
        before.layout.enabled().count(),
        2,
        "expected DUMMY0 + DUMMY1 on, DUMMY2 off"
    );
    let xga = before
        .monitor("DUMMY2")
        .unwrap()
        .modes
        .iter()
        .copied()
        .find(|m| m.width == 1024);
    let xga = xga.unwrap_or(XGA);

    // A: DUMMY1 to the left of DUMMY0.
    let mut l = before.layout.clone();
    out(&mut l, "DUMMY1").position = Position { x: -1280, y: 0 };
    x.apply(&l).unwrap();
    let s = x.current_state().unwrap().layout;
    assert_eq!(get(&s, "DUMMY1").position, Position { x: 0, y: 0 });
    assert_eq!(get(&s, "DUMMY0").position, Position { x: 1280, y: 0 });

    // B: DUMMY1 below at 1080p and primary.
    let mut l = s.clone();
    *out(&mut l, "DUMMY0") = OutputConfig {
        position: Position { x: 0, y: 0 },
        primary: false,
        ..get(&s, "DUMMY0").clone()
    };
    *out(&mut l, "DUMMY1") = OutputConfig {
        position: Position { x: 0, y: 1080 },
        mode: FHD,
        primary: true,
        ..get(&s, "DUMMY1").clone()
    };
    x.apply(&l).unwrap();
    let s = x.current_state().unwrap().layout;
    assert_eq!(get(&s, "DUMMY1").position, Position { x: 0, y: 1080 });
    assert_eq!(get(&s, "DUMMY1").mode, FHD);
    assert!(get(&s, "DUMMY1").primary && !get(&s, "DUMMY0").primary);

    // C: DUMMY1 off, DUMMY2 on to the right of DUMMY0.
    let mut l = s.clone();
    *out(&mut l, "DUMMY0") = OutputConfig {
        primary: true,
        ..get(&s, "DUMMY0").clone()
    };
    *out(&mut l, "DUMMY1") = OutputConfig {
        enabled: false,
        primary: false,
        ..get(&s, "DUMMY1").clone()
    };
    *out(&mut l, "DUMMY2") = OutputConfig {
        enabled: true,
        position: Position { x: 1920, y: 0 },
        mode: xga,
        ..get(&s, "DUMMY2").clone()
    };
    x.apply(&l).unwrap();
    let s = x.current_state().unwrap().layout;
    assert!(!get(&s, "DUMMY1").enabled);
    assert!(get(&s, "DUMMY2").enabled);
    assert_eq!(get(&s, "DUMMY2").position, Position { x: 1920, y: 0 });

    // D: the dummy driver cannot rotate; validation refuses, nothing changes.
    assert!(
        !x.current_state()
            .unwrap()
            .monitor("DUMMY0")
            .unwrap()
            .rotations
            .contains(&Rotation::Left)
    );
    let mut l = s.clone();
    out(&mut l, "DUMMY0").rotation = Rotation::Left;
    out(&mut l, "DUMMY2").position = Position { x: 1080, y: 0 };
    assert!(matches!(x.apply(&l), Err(Error::InvalidLayout(_))));
    assert_eq!(x.current_state().unwrap().layout, s);

    // Back to the start.
    x.apply(&before.layout).unwrap();
    assert_eq!(x.current_state().unwrap().layout, before.layout);
}

#[test]
fn rejects_scale_without_changes() {
    let Some(x) = backend() else { return };
    let before = x.current_state().unwrap();
    let mut l = before.layout.clone();
    l.outputs[0].scale = 1.5;
    assert!(matches!(x.apply(&l), Err(Error::InvalidLayout(_))));
    assert_eq!(x.current_state().unwrap(), before);
}
