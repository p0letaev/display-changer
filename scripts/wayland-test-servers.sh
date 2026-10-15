#!/usr/bin/env bash
# Runs the Wayland backend integration tests against headless compositors:
# Sway (wlroots), KWin (KDE) and Mutter (GNOME). Each runs with its own
# socket, config dir and (for KWin/Mutter) a private D-Bus session, so the
# desktop you are in is never touched.
#
# Needs: sway, kwin-wayland, mutter (Debian/Kali package names).
#   scripts/wayland-test-servers.sh            # all three
#   scripts/wayland-test-servers.sh wlroots    # or kde / gnome
set -uo pipefail

cd "$(dirname "$0")/.."
which="${1:-all}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# Never talk to the real session.
clean_env=(env -u DISPLAY -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS
  QT_QPA_PLATFORMTHEME= GIO_USE_VFS=local XDG_CONFIG_HOME="$work/config")
mkdir -p "$work/config"
status=0

wait_for_socket() { # $1 = name pattern, $2 = existing sockets
  for _ in $(seq 50); do
    local s
    s=$(comm -13 <(echo "$2") <(ls "$XDG_RUNTIME_DIR" | grep -E "$1" | grep -v lock | sort) | head -1)
    [[ -n "$s" ]] && { echo "$s"; return; }
    sleep 0.1
  done
}

if [[ "$which" == all || "$which" == wlroots ]]; then
  printf 'xwayland disable\noutput HEADLESS-1 mode 1920x1080@60Hz position 0 0\noutput HEADLESS-2 mode 1280x720@60Hz position 1920 0\n' > "$work/sway.conf"
  before=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | sort)
  "${clean_env[@]}" WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=2 \
    sway -c "$work/sway.conf" >"$work/sway.log" 2>&1 &
  pid=$!
  socket=$(wait_for_socket '^wayland-[0-9]+$' "$before")
  DISPLAY_CHANGER_WLR_TEST="$socket" cargo test -q -p display-core --test wayland_wlr || status=1
  kill "$pid" 2>/dev/null
fi

if [[ "$which" == all || "$which" == kde ]]; then
  "${clean_env[@]}" dbus-run-session -- kwin_wayland --virtual --output-count 2 \
    --width 1920 --height 1080 --socket dc-kde-test --no-lockscreen >"$work/kwin.log" 2>&1 &
  pid=$!
  for _ in $(seq 50); do [[ -S "$XDG_RUNTIME_DIR/dc-kde-test" ]] && break; sleep 0.1; done
  DISPLAY_CHANGER_KDE_TEST=dc-kde-test cargo test -q -p display-core --test wayland_kde || status=1
  pkill -P "$pid" 2>/dev/null; kill "$pid" 2>/dev/null
fi

if [[ "$which" == all || "$which" == gnome ]]; then
  "${clean_env[@]}" dbus-run-session -- sh -c '
    mutter --headless --virtual-monitor 1920x1080 --virtual-monitor 1280x720 >"$1/mutter.log" 2>&1 &
    for _ in $(seq 40); do
      gdbus introspect --session --dest org.gnome.Mutter.DisplayConfig \
        --object-path /org/gnome/Mutter/DisplayConfig >/dev/null 2>&1 && break
      sleep 0.25
    done
    DISPLAY_CHANGER_GNOME_TEST=1 cargo test -q -p display-core --test wayland_gnome
    rc=$?; kill $! 2>/dev/null; exit $rc' sh "$work" || status=1
fi

exit $status
