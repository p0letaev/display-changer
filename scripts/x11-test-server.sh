#!/usr/bin/env bash
# Starts a throwaway X server with the dummy driver for the X11 backend tests:
# DUMMY0 1920x1080 (primary), DUMMY1 1280x720 to its right, DUMMY2 connected but off.
#
# Needs xserver-xorg-video-dummy. Runs in a systemd user scope so Xorg does not
# attach to your login session via logind.
#
#   scripts/x11-test-server.sh :96
#   DISPLAY_CHANGER_XTEST=:96 cargo test -p display-core --test x11_dummy -- --test-threads=1
#   scripts/x11-test-server.sh :96 stop
set -euo pipefail

display="${1:-:96}"
dir="${XDG_RUNTIME_DIR:-/tmp}/display-changer-xtest"
mkdir -p "$dir/conf.d"

if [[ "${2:-}" == stop ]]; then
  systemctl --user stop "dc-xtest-${display#:}.scope" 2>/dev/null || true
  exit 0
fi

cat > "$dir/dummy.conf" <<'CONF'
Section "ServerFlags"
    Option "AutoAddDevices" "false"
    Option "AutoAddGPU" "false"
    Option "AutoBindGPU" "false"
EndSection

Section "Device"
    Identifier "dummy"
    Driver "dummy"
    VideoRam 256000
EndSection

Section "Monitor"
    Identifier "Monitor0"
    HorizSync 5.0-1000.0
    VertRefresh 5.0-200.0
    Modeline "1920x1080" 148.50 1920 2008 2052 2200 1080 1084 1089 1125 +hsync +vsync
    Modeline "1280x720" 74.25 1280 1390 1430 1650 720 725 730 750 +hsync +vsync
    Modeline "1024x768" 65.00 1024 1048 1184 1344 768 771 777 806 -hsync -vsync
EndSection

Section "Screen"
    Identifier "Screen0"
    Device "dummy"
    Monitor "Monitor0"
    DefaultDepth 24
    SubSection "Display"
        Depth 24
        Virtual 8192 4096
        Modes "1920x1080" "1280x720" "1024x768"
    EndSubSection
EndSection
CONF

systemd-run --user --scope --quiet --unit="dc-xtest-${display#:}" \
  /usr/lib/xorg/Xorg "$display" -noreset -novtswitch -nolisten tcp \
  -logfile "$dir/xorg.log" -config "$dir/dummy.conf" -configdir "$dir/conf.d" \
  >/dev/null 2>&1 &

for _ in $(seq 50); do DISPLAY="$display" xrandr >/dev/null 2>&1 && break; sleep 0.1; done

export DISPLAY="$display"
for mode in 1920x1080 1024x768; do xrandr --addmode DUMMY1 "$mode"; done
xrandr --addmode DUMMY1 1280x720
xrandr --output DUMMY1 --mode 1280x720 --right-of DUMMY0
xrandr --addmode DUMMY2 1024x768
xrandr --output DUMMY2 --mode 1024x768 --right-of DUMMY1
xrandr --output DUMMY2 --off
xrandr | grep -E '^DUMMY[0-2] '
