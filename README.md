# Display Changer

Monitor layout editor with profiles for Linux and Windows. Tauri 2 + Nuxt 4.

## Backends

| Session | Backend | How it was tested |
|---|---|---|
| X11 (XFCE, Cinnamon, MATE, i3, …) | XRandR via `x11rb`, xfconf sync on XFCE | Xorg dummy driver, 3 outputs |
| Wayland: Sway, Hyprland, river, labwc, Wayfire, niri, COSMIC | `wlr-output-management-unstable-v1` | headless Sway |
| Wayland: KDE Plasma 6 | `kde_output_device_v2` / `kde_output_management_v2` (vendored XML, registry for 6.4+) | headless KWin 6.7 |
| Wayland: GNOME | D-Bus `org.gnome.Mutter.DisplayConfig` | headless Mutter 50 |
| Windows 10/11 | CCD API + `ChangeDisplaySettingsEx` | Wine (topology changes need real Windows) |

Selection is automatic and by protocol support, not desktop name, so other
compositors implementing one of these protocols work too.
`DISPLAY_CHANGER_BACKEND=mock|x11|wlroots|kde|gnome` overrides it.

On XFCE the applied layout is also written to xfconf (`displays` channel),
otherwise XFCE restores its own older layout on hotplug or login. Before the
first write the channel is saved to
`~/.config/io.github.displaychanger/xfce-displays-backup.json`.
`DISPLAY_CHANGER_NO_XFCONF=1` disables this.

X11 has no per-output scale, so scale is fixed at 100 % there.

## Structure

- `crates/display-core`: display model, layout validation and normalization, backends.
- `src-tauri`: Tauri app, commands in `src/commands.rs`.
- `app`: Nuxt frontend (SPA).

## Development

Requirements: Rust, Node.js 22+, [Tauri system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev        # run the app
cargo test --workspace   # core tests
npm run typecheck
```

`DISPLAY_CHANGER_BACKEND=mock` forces the demo backend with three fake monitors.
`cargo run -p display-core --example dump` prints what the backend sees (read-only).

Integration tests that change displays run only against throwaway servers:

```sh
scripts/x11-test-server.sh :96          # Xorg + dummy driver, three outputs
DISPLAY_CHANGER_XTEST=:96 cargo test -p display-core --test x11_dummy -- --test-threads=1
scripts/x11-test-server.sh :96 stop
```

`tests/xfconf_private.rs` describes how to test xfconf writes on a private D-Bus session.

```sh
scripts/wayland-test-servers.sh        # headless Sway, KWin and Mutter
```

Windows code is checked from Linux with `cargo xwin clippy --target x86_64-pc-windows-msvc`;
unit tests and the `dump`/`apply` examples run under Wine.

## Features

- Drag-and-snap layout editor; resolution, refresh rate, scale, rotation, primary, on/off
- Profiles matched to monitors by EDID, so they follow a monitor to another port
- Apply with a countdown and automatic revert (5–120 s, configurable)
- Global hotkeys and a tray menu for profiles; optional confirmation for these
- Automatic switching: profiles marked "auto" apply when their monitors get connected
- Changes made elsewhere (system settings, the CLI) show up in the window
- "Identify" shows each monitor's number on that monitor
- Import/export of profiles, autostart, start in tray, light/dark theme
- CLI: `display-changer list | apply <name> | state`

## Profiles and hotkeys

Profiles and settings are stored in `~/.config/io.github.displaychanger/`
(`%APPDATA%\io.github.displaychanger\` on Windows): `profiles.json`,
`settings.json`; `profiles.mock.json` for the demo backend.

- Applying from the UI reverts after 15 s unless confirmed; the timer runs in Rust.
- Hotkeys, the tray and automatic switching apply a profile without confirmation,
  since it was saved from a working setup; a setting turns confirmation on.
- Global hotkeys work on X11 and Windows. Wayland does not allow global grabs:
  bind `display-changer apply "<profile>"` in your compositor instead.

## Building releases

```sh
# Linux: .deb, .rpm, .AppImage
npm run tauri build -- --bundles deb,rpm,appimage

# Windows installer from Linux (needs cargo-xwin, lld, llvm, nsis)
npm run tauri build -- --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
```

Artifacts go to `target/release/bundle/` and
`target/x86_64-pc-windows-msvc/release/bundle/`. The local `builds/` folder
(ignored by git) holds copies that are attached to GitHub releases by hand.
