#!/usr/bin/env bash
# Rough idle-cost probe (Linux, Xvfb, tiny-skia CPU renderer — pessimistic vs. GPU on Windows).
# Run inside desktop-gui on the desktop_it network after a release build.
set -euo pipefail
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
export DISPLAY=:99; sleep 1
data=$(mktemp -d); mkdir -p "$data/config"
cookie=$(curl -s -i -d "username=alice&password=password123&remember_me=true" http://it-proxy/api/auth/login | tr -d '\r' | sed -n 's/^[Ss]et-[Cc]ookie: messenger_refresh=\([^;]*\).*/\1/p')
printf '{"server":"http://it-proxy","refresh_cookie":"%s","username":"alice"}' "$cookie" > "$data/config/session.bin"
printf '{"server":"http://it-proxy","auto_update":false,"notifications":false}' > "$data/config/settings.json"
start=$(date +%s%N)
ICED_BACKEND=tiny-skia MESSENGER_DATA_DIR="$data" RUST_LOG=warn /target/release/messenger >/dev/null 2>&1 &
pid=$!
sleep 6
xdotool search --name "^Messenger" windowfocus 2>/dev/null || true
ticks() { awk '{print $14+$15}' /proc/$pid/stat; }
hz=$(getconf CLK_TCK)
t0=$(ticks); sleep 10; t1=$(ticks)
echo "idle CPU, chat list only (10 s): $(awk -v d=$((t1-t0)) -v hz=$hz 'BEGIN{printf "%.2f%%", d/hz/10*100}')"
xdotool mousemove 220 150 click 1; sleep 3   # open a chat (focuses the composer: blinking caret)
t0=$(ticks); sleep 10; t1=$(ticks)
echo "idle CPU, chat open + focused composer (10 s): $(awk -v d=$((t1-t0)) -v hz=$hz 'BEGIN{printf "%.2f%%", d/hz/10*100}')"
xdotool mousemove 1000 300 click 1; sleep 2   # click the message area (composer keeps focus?)
t0=$(ticks); sleep 20; t1=$(ticks)
echo "idle CPU after clicking messages (20 s): $(awk -v d=$((t1-t0)) -v hz=$hz 'BEGIN{printf "%.2f%%", d/hz/20*100}')"
echo "RSS: $(grep VmRSS /proc/$pid/status | awk '{printf "%.0f MB", $2/1024}')  threads: $(ls /proc/$pid/task | wc -l)"
# Scroll the chat list/messages for 5 s and measure.
t0=$(ticks); for i in $(seq 1 25); do xdotool mousemove 800 400 click 4; sleep 0.1; xdotool click 5; sleep 0.1; done; t1=$(ticks)
echo "scrolling CPU over 5 s: $(awk -v d=$((t1-t0)) -v hz=$hz 'BEGIN{printf "%.0f%%", d/hz/5*100}')"
ls -la /target/release/messenger | awk '{printf "binary: %.1f MB\n", $5/1048576}'
kill $pid
