#!/usr/bin/env bash
# Compares the egui and GTK Linux apps on the --demo fixtures:
#   - time to first window (launch to a mapped, visible window)
#   - resident memory after opening the public.users table tab
#   - idle CPU % over IDLE seconds with that tab open
#   - CPU % while wheel-scrolling the users grid for SCROLL seconds
# Each metric is the median of RUNS launches.
#
# Usage, from the repository root after release builds of both apps:
#   cargo build --release --manifest-path apps/native/workbench/Cargo.toml
#   cargo build --release --manifest-path apps/native/gtk/Cargo.toml
#   apps/native/gtk/bench.sh            # on the current X11 display
#   BENCH_XVFB=1 apps/native/gtk/bench.sh  # on a private Xvfb (no GPU)
#
# Needs xdotool, and Xvfb for BENCH_XVFB=1. Both apps run on X11 (XWayland
# under a Wayland session) at scale 1, so xdotool drives them the same way
# and the click targets below hold on HiDPI screens. BENCH_SHOTS=<dir> saves
# a screenshot of each app with the table open, to check those targets.
# Settings, profiles and keyring are untouched: each app runs with a
# throwaway HOME and demo fixtures.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
EGUI=${EGUI:-$ROOT/apps/native/workbench/target/release/dbm-workbench}
GTK=${GTK:-$ROOT/apps/native/gtk/target/release/dbm-gtk}
RUNS=${RUNS:-3}
IDLE=${IDLE:-60}
SCROLL=${SCROLL:-10}
WIDTH=1280
HEIGHT=800

for bin in "$EGUI" "$GTK"; do
  [[ -x $bin ]] || { echo "missing $bin; build it first (see the header)" >&2; exit 1; }
done
command -v xdotool >/dev/null || { echo "xdotool is required" >&2; exit 1; }

if [[ ${BENCH_XVFB:-0} == 1 || -z ${DISPLAY:-} ]]; then
  export DISPLAY=:97
  Xvfb "$DISPLAY" -screen 0 ${WIDTH}x${HEIGHT}x24 -nolisten tcp >/dev/null 2>&1 &
  XVFB=$!
  sleep 1
fi

TICKS=$(getconf CLK_TCK)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"; [[ -n ${XVFB:-} ]] && kill $XVFB 2>/dev/null || true' EXIT

now() { date +%s.%N; }

# utime + stime of a process and its threads, in clock ticks.
cpu_ticks() { awk '{print $14 + $15}' "/proc/$1/stat"; }

rss_mb() { awk '/^VmRSS:/ {printf "%.1f", $2 / 1024}' "/proc/$1/status"; }

cpu_percent() { # pid seconds [action...]
  local pid=$1 seconds=$2 start end t0 t1
  shift 2
  start=$(cpu_ticks "$pid"); t0=$(now)
  if (($#)); then "$@"; else sleep "$seconds"; fi
  end=$(cpu_ticks "$pid"); t1=$(now)
  awk -v a="$start" -v b="$end" -v t0="$t0" -v t1="$t1" -v hz="$TICKS" \
    'BEGIN { printf "%.2f", (b - a) / hz / (t1 - t0) * 100 }'
}

click() { # window x y
  xdotool mousemove --window "$1" "$2" "$3" click 1
}

scroll_grid() { # window x y seconds
  local window=$1 x=$2 y=$3 until
  until=$(awk -v t="$(now)" -v s="$4" 'BEGIN { printf "%.3f", t + s }')
  xdotool mousemove --window "$window" "$x" "$y"
  while awk -v t="$(now)" -v u="$until" 'BEGIN { exit !(t < u) }'; do
    xdotool click --repeat 15 --delay 16 5
    xdotool click --repeat 15 --delay 16 4
  done
}

# Window-relative click targets for each app at 1280×800: the Production
# profile, then public.users in its schema tree, then a point in the grid.
coords() {
  case $1 in
    egui) echo "$EGUI_COORDS" ;;
    gtk) echo "$GTK_COORDS" ;;
  esac
}
EGUI_COORDS=${EGUI_COORDS:-"100 62 100 450 600 550"}
GTK_COORDS=${GTK_COORDS:-"100 62 100 428 500 550"}

run_once() { # name binary
  local name=$1 bin=$2 home=$WORK/$1-home pid window start mapped
  mkdir -p "$home"
  start=$(now)
  env -u WAYLAND_DISPLAY HOME="$home" XDG_DATA_HOME="$home/.local/share" \
    XDG_CONFIG_HOME="$home/.config" XDG_CACHE_HOME="$home/.cache" \
    GDK_BACKEND=x11 GDK_SCALE=1 WINIT_X11_SCALE_FACTOR=1 \
    "$bin" --demo >"$WORK/$name.log" 2>&1 &
  pid=$!
  # Poll every 10 ms; xdotool's own --sync waits in 0.5 s steps.
  window=
  for ((tries = 0; tries < 3000; tries++)); do
    window=$(xdotool search --onlyvisible --pid "$pid" 2>/dev/null | head -n 1) || true
    [[ -n $window ]] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.01
  done
  if [[ -z $window ]]; then
    echo "$name did not open a window; its log:" >&2
    tail -n 20 "$WORK/$name.log" >&2
    exit 1
  fi
  mapped=$(now)
  xdotool windowmove "$window" 0 0 windowsize "$window" $WIDTH $HEIGHT
  sleep 3
  read -r px py tx ty gx gy <<<"$(coords "$name")"
  click "$window" "$px" "$py"; sleep 2
  click "$window" "$tx" "$ty"; sleep 3
  [[ -n ${BENCH_SHOTS:-} ]] && import -window root "$BENCH_SHOTS/$name.png"
  local rss idle scroll
  rss=$(rss_mb "$pid")
  idle=$(cpu_percent "$pid" "$IDLE")
  scroll=$(cpu_percent "$pid" "$SCROLL" scroll_grid "$window" "$gx" "$gy" "$SCROLL")
  kill "$pid"; wait "$pid" 2>/dev/null || true
  awk -v a="$start" -v b="$mapped" -v r="$rss" -v i="$idle" -v s="$scroll" \
    'BEGIN { printf "%.3f %s %s %s\n", b - a, r, i, s }'
}

median() { sort -g | awk '{ v[NR] = $1 } END { print (NR % 2) ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2 }'; }

for name in egui gtk; do
  bin=$EGUI; [[ $name == gtk ]] && bin=$GTK
  : >"$WORK/$name.results"
  # One warm-up launch so font caches and shader caches don't count.
  run_once "$name" "$bin" >/dev/null
  for ((i = 1; i <= RUNS; i++)); do
    line=$(run_once "$name" "$bin")
    echo "$name run $i: window ${line%% *} s, then RSS/idle/scroll: ${line#* }" >&2
    echo "$line" >>"$WORK/$name.results"
  done
done

col() { awk -v c="$2" '{ print $c }' "$WORK/$1.results" | median; }
printf '\n| Median of %s runs | egui | GTK |\n| --- | --- | --- |\n' "$RUNS"
printf '| Time to first window | %s s | %s s |\n' "$(col egui 1)" "$(col gtk 1)"
printf '| Resident memory, users table open | %s MB | %s MB |\n' "$(col egui 2)" "$(col gtk 2)"
printf '| Idle CPU over %s s | %s%% | %s%% |\n' "$IDLE" "$(col egui 3)" "$(col gtk 3)"
printf '| CPU while scrolling the grid (%s s) | %s%% | %s%% |\n' "$SCROLL" "$(col egui 4)" "$(col gtk 4)"
