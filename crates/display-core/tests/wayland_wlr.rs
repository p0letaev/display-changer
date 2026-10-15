//! Applies layouts on a headless Sway. Skipped unless
//! `DISPLAY_CHANGER_WLR_TEST` names its socket; see `scripts/wayland-test-servers.sh`.

#![cfg(target_os = "linux")]

use display_core::backends::wlr::WlrBackend;
use display_core::{DisplayBackend, Layout, OutputConfig, Position, Rotation};

fn backend() -> Option<WlrBackend> {
    let socket = std::env::var("DISPLAY_CHANGER_WLR_TEST").ok()?;
    // SAFETY: set before the first Wayland connection in this process.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", socket) };
    Some(WlrBackend::connect().expect("connect to headless sway"))
}

fn set(l: &mut Layout, name: &str, f: impl FnOnce(&mut OutputConfig)) {
    f(l.outputs
        .iter_mut()
        .find(|o| o.id.connector == name)
        .expect(name));
}

#[test]
fn moves_scales_rotates_toggles_and_restores() {
    let Some(b) = backend() else {
        eprintln!("DISPLAY_CHANGER_WLR_TEST not set, skipping");
        return;
    };
    let before = b.current_state().unwrap();
    let get = |l: &Layout, n: &str| l.output(n).unwrap().clone();

    // Second output to the left of the first.
    let mut l = before.layout.clone();
    set(&mut l, "HEADLESS-1", |o| {
        o.position = Position { x: 1280, y: 0 }
    });
    set(&mut l, "HEADLESS-2", |o| {
        o.position = Position { x: 0, y: 0 }
    });
    b.apply(&l).unwrap();
    let s = b.current_state().unwrap().layout;
    assert_eq!(get(&s, "HEADLESS-2").position, Position { x: 0, y: 0 });
    assert_eq!(get(&s, "HEADLESS-1").position, Position { x: 1280, y: 0 });

    // Fractional scale shrinks the logical size: 1920 / 1.5 = 1280.
    let mut l = s.clone();
    set(&mut l, "HEADLESS-1", |o| {
        o.scale = 1.5;
        o.position = Position { x: 0, y: 0 };
    });
    set(&mut l, "HEADLESS-2", |o| {
        o.position = Position { x: 1280, y: 0 }
    });
    b.apply(&l).unwrap();
    let s = b.current_state().unwrap().layout;
    assert!((get(&s, "HEADLESS-1").scale - 1.5).abs() < 1e-3);

    // Portrait.
    let mut l = s.clone();
    set(&mut l, "HEADLESS-2", |o| o.rotation = Rotation::Left);
    b.apply(&l).unwrap();
    assert_eq!(
        get(&b.current_state().unwrap().layout, "HEADLESS-2").rotation,
        Rotation::Left
    );

    // Off and on again.
    let mut l = b.current_state().unwrap().layout;
    set(&mut l, "HEADLESS-2", |o| o.enabled = false);
    b.apply(&l).unwrap();
    assert!(!get(&b.current_state().unwrap().layout, "HEADLESS-2").enabled);

    b.apply(&before.layout).unwrap();
    let after = b.current_state().unwrap().layout;
    for o in &before.layout.outputs {
        let a = get(&after, &o.id.connector);
        assert_eq!(
            (a.enabled, a.position, a.rotation),
            (o.enabled, o.position, o.rotation),
            "{}",
            o.id.connector
        );
        assert!((a.scale - o.scale).abs() < 1e-3);
    }
}
