#!/usr/bin/env bash
# Headless screenshots of the real app against the integration stack (run inside the
# desktop-gui image on the desktop_it network). Writes PNGs to /src/shots.
set -euo pipefail
W=${SHOT_WIDTH:-1280}; H=${SHOT_HEIGHT:-800}
mkdir -p /src/shots && rm -f /src/shots/*.png
Xvfb :99 -screen 0 ${W}x${H}x24 -nolisten tcp >/dev/null 2>&1 &
export DISPLAY=:99
sleep 1
data=$(mktemp -d)
mkdir -p "$data/config"
login() {
  curl -s -i -d "username=$1&password=password123&remember_me=true" http://it-proxy/api/auth/login \
    | tr -d '\r' | sed -n 's/^[Ss]et-[Cc]ookie: messenger_refresh=\([^;]*\).*/\1/p'
}
cookie=$(login "${SHOT_USER:-alice}")
printf '{"server":"http://it-proxy","refresh_cookie":"%s","username":"%s"}' "$cookie" "${SHOT_USER:-alice}" > "$data/config/session.bin"
printf '{"server":"http://it-proxy","auto_update":false,"notifications":false,"window_width":%s,"window_height":%s}' "$W" "$H" > "$data/config/settings.json"
shot() { sleep "${2:-1.5}"; import -window root "/src/shots/$1.png"; echo "shot $1"; }
click() { xdotool mousemove "$1" "$2" click "${3:-1}"; }
# tiny-skia only repaints damaged regions; a 1px resize forces a full frame.
repaint() { xdotool search --name "^Messenger" windowsize $((W - 1)) $H; sleep 0.5; xdotool search --name "^Messenger" windowsize $W $H; }
ICED_BACKEND=${ICED_BACKEND:-tiny-skia} MESSENGER_DATA_DIR="$data" RUST_LOG=warn /target/debug/messenger > /src/shots/app.log 2>&1 &
app=$!
sleep 6
xdotool search --sync --name "^Messenger" windowfocus 2>/dev/null || true
shot 01-chat-list 1
# Own media messages at the bottom of the first chat.
click 220 150; repaint; shot 00-own-media 2.5
# Smooth wheel scrolling: three notches up, captured after the animation settled.
xdotool mousemove 800 400; for i in 1 2 3; do xdotool click 4; sleep 0.05; done; sleep 0.8; repaint; shot 00-wheel 0.5
# Hovered "New group" tile.
xdotool mousemove 34 101; sleep 0.6; repaint; xdotool mousemove 34 101; shot 00-rail-hover 0.6
convert /src/shots/00-rail-hover.png -crop 70x800+0+0 -scale 300% /src/shots/rail-zoom.png
# Open the first chat in the list (rail 68px + sidebar; first item ≈ y 140).
click 220 150; shot 02-conversation 2.5
# Right-click the newest message bubble (bottom right of the conversation).
click 1000 $((H - 153)) 3; shot 03-context-menu 1.5
xdotool key Escape
# Settings via Ctrl+,
xdotool key ctrl+comma; shot 04-settings 2
xdotool key Escape
# Second chat (group) and its info dialog (click the header title).
click 220 210; shot 05-group 2.5
click 520 34; shot 06-group-info 1.5
xdotool key Escape
# Image viewer: open the first chat again and click the photo.
click 220 150; sleep 2
click 610 520; shot 07-image-viewer 2.5
xdotool key Escape
# New group dialog (rail "+").
click 20 102; sleep 1; xdotool mousemove 640 400; repaint; shot 08-new-group 2
xdotool key Escape
# Narrow window layout.
xdotool search --name "^Messenger" windowsize 700 760; shot 09-narrow 2
kill $app 2>/dev/null || true
