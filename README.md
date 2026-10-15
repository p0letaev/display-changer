# Display Changer

Monitor layout editor with profiles for Linux and Windows. Tauri 2 + Nuxt 4.

## Backends

| Session | Backend | Status |
|---|---|---|
| X11 (XFCE, Cinnamon, MATE, i3, …) | XRandR via `x11rb` | done |
| Wayland (wlroots, KDE, GNOME) | — | planned, falls back to demo |
| Windows 10/11 | CCD API + ChangeDisplaySettingsEx | done (tested under Wine, see below) |

Selection is automatic; `DISPLAY_CHANGER_BACKEND=mock|x11` overrides it.

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

## Profiles and hotkeys

Profiles are stored in `~/.config/io.github.displaychanger/profiles.json`
(`profiles.mock.json` for the demo backend, seeded with four demo profiles).

- Applying from the UI reverts after 15 s unless confirmed; the timer runs in Rust.
- Hotkeys apply a profile without confirmation, since it was saved from a working setup.
- Global hotkeys work on X11 and Windows. Wayland does not allow global grabs.
