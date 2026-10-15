//! Writes to xfconf through D-Bus. Skipped unless `DISPLAY_CHANGER_XFCONF_TEST`
//! is set; run it only inside a private bus with its own xfconfd so the real
//! XFCE settings stay untouched:
//!
//! XDG_CONFIG_HOME=$(mktemp -d) dbus-run-session -- sh -c '
//!   /usr/lib/x86_64-linux-gnu/xfce4/xfconf/xfconfd & sleep 0.5
//!   DISPLAY_CHANGER_XFCONF_TEST=1 cargo test -p display-core --test xfconf_private
//!   xfconf-query -c displays -l -v'

#![cfg(target_os = "linux")]

use display_core::backends::xfce::{XfceDisplays, XfceOutput};

#[test]
fn saves_outputs_to_active_profile() {
    if std::env::var_os("DISPLAY_CHANGER_XFCONF_TEST").is_none() {
        eprintln!("DISPLAY_CHANGER_XFCONF_TEST not set, skipping");
        return;
    }
    // SAFETY: set before any other thread reads the environment.
    unsafe { std::env::set_var("XDG_CURRENT_DESKTOP", "XFCE") };
    let xfconf = XfceDisplays::connect().expect("xfconfd on the session bus");
    let output = |connector: &str, active: bool, x: i32| XfceOutput {
        connector: connector.into(),
        name: format!("Test {connector}"),
        edid_hash: "0123456789abcdef0123456789abcdef01234567".into(),
        active,
        x,
        y: 0,
        primary: x == 0,
        rotation: 0,
        resolution: "1920x1080".into(),
        refresh_hz: 59.940_06,
        mode_flags: 5,
    };
    xfconf
        .save(&[
            output("HDMI-A-0", true, 0),
            output("eDP", true, 1920),
            output("DP-1", false, 0),
        ])
        .unwrap();
}
