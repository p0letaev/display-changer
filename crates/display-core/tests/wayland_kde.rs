//! Applies layouts on a headless KWin. Skipped unless
//! `DISPLAY_CHANGER_KDE_TEST` names its socket; see `scripts/wayland-test-servers.sh`.

#![cfg(target_os = "linux")]

use display_core::backends::kde::KdeBackend;
use display_core::{DisplayBackend, Layout, OutputConfig, Position, Rotation};

fn backend() -> Option<KdeBackend> {
    let socket = std::env::var("DISPLAY_CHANGER_KDE_TEST").ok()?;
    // SAFETY: set before the first Wayland connection in this process.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", socket) };
    Some(KdeBackend::connect().unwrap_or_else(|e| panic!("connect to headless KWin: {e}")))
}

fn set(l: &mut Layout, name: &str, f: impl FnOnce(&mut OutputConfig)) {
    f(l.outputs
        .iter_mut()
        .find(|o| o.id.connector == name)
        .expect(name));
}

#[test]
fn moves_scales_rotates_switches_primary_and_restores() {
    let Some(b) = backend() else {
        eprintln!("DISPLAY_CHANGER_KDE_TEST not set, skipping");
        return;
    };
    let before = b.current_state().unwrap();
    eprintln!("{:#?}", before.layout);
    let names: Vec<String> = before
        .layout
        .outputs
        .iter()
        .map(|o| o.id.connector.clone())
        .collect();
    let (a, c) = (names[0].as_str(), names[1].as_str());
    let get = |l: &Layout, n: &str| l.output(n).unwrap().clone();
    let width = |l: &Layout, n: &str| get(l, n).mode.width as i32;

    // Second output to the left of the first, and primary.
    let mut l = before.layout.clone();
    let wc = width(&l, c);
    set(&mut l, a, |o| {
        o.position = Position { x: wc, y: 0 };
        o.primary = false;
    });
    set(&mut l, c, |o| {
        o.position = Position { x: 0, y: 0 };
        o.primary = true;
    });
    b.apply(&l).unwrap();
    let s = b.current_state().unwrap().layout;
    assert_eq!(get(&s, c).position, Position { x: 0, y: 0 });
    assert!(get(&s, c).primary && !get(&s, a).primary);

    // Fractional scale and portrait.
    let mut l = s.clone();
    set(&mut l, c, |o| o.scale = 1.25);
    set(&mut l, a, |o| {
        o.rotation = Rotation::Left;
        o.position = Position {
            x: (wc as f64 / 1.25) as i32,
            y: 0,
        };
    });
    b.apply(&l).unwrap();
    let s = b.current_state().unwrap().layout;
    assert!((get(&s, c).scale - 1.25).abs() < 1e-3);
    assert_eq!(get(&s, a).rotation, Rotation::Left);

    // Off.
    let mut l = s.clone();
    set(&mut l, a, |o| o.enabled = false);
    b.apply(&l).unwrap();
    assert!(!get(&b.current_state().unwrap().layout, a).enabled);

    b.apply(&before.layout).unwrap();
    let after = b.current_state().unwrap().layout;
    for o in &before.layout.outputs {
        let x = get(&after, &o.id.connector);
        assert_eq!(
            (x.enabled, x.position, x.rotation, x.primary),
            (o.enabled, o.position, o.rotation, o.primary),
            "{}",
            o.id.connector
        );
    }
}
