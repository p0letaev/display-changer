//! Applies layouts on a headless Mutter. Skipped unless
//! `DISPLAY_CHANGER_GNOME_TEST` is set; run inside the private D-Bus session
//! started by `scripts/wayland-test-servers.sh gnome`.

#![cfg(target_os = "linux")]

use display_core::backends::gnome::GnomeBackend;
use display_core::{DisplayBackend, Layout, OutputConfig, Position};

fn set(l: &mut Layout, name: &str, f: impl FnOnce(&mut OutputConfig)) {
    f(l.outputs
        .iter_mut()
        .find(|o| o.id.connector == name)
        .expect(name));
}

#[test]
fn moves_switches_primary_toggles_and_restores() {
    if std::env::var_os("DISPLAY_CHANGER_GNOME_TEST").is_none() {
        eprintln!("DISPLAY_CHANGER_GNOME_TEST not set, skipping");
        return;
    }
    let b = GnomeBackend::connect().unwrap_or_else(|e| panic!("connect to headless Mutter: {e}"));
    let before = b.current_state().unwrap();
    eprintln!(
        "{:?}",
        before
            .layout
            .outputs
            .iter()
            .map(|o| (
                &o.id.connector,
                o.enabled,
                o.position,
                o.mode,
                o.scale,
                o.primary
            ))
            .collect::<Vec<_>>()
    );
    let names: Vec<String> = before
        .layout
        .outputs
        .iter()
        .map(|o| o.id.connector.clone())
        .collect();
    let (a, c) = (names[0].as_str(), names[1].as_str());
    let get = |l: &Layout, n: &str| l.output(n).unwrap().clone();

    // Second monitor to the left and primary.
    let mut l = before.layout.clone();
    let wc = get(&l, c).mode.width as i32;
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
    assert_eq!(get(&s, a).position, Position { x: wc, y: 0 });
    assert!(get(&s, c).primary && !get(&s, a).primary);

    // Off, then back to the start.
    let mut l = s.clone();
    set(&mut l, a, |o| o.enabled = false);
    b.apply(&l).unwrap();
    assert!(!get(&b.current_state().unwrap().layout, a).enabled);

    b.apply(&before.layout).unwrap();
    let after = b.current_state().unwrap().layout;
    for o in &before.layout.outputs {
        let x = get(&after, &o.id.connector);
        assert_eq!(
            (x.enabled, x.position, x.primary),
            (o.enabled, o.position, o.primary),
            "{}",
            o.id.connector
        );
    }
}
