#!/usr/bin/env bash
# Scatters the 6 camera-zoom markers in a ring on the FREE canvas.
#
# Free canvas spawns every window at the viewport center, so the script pans
# the camera between spawns (900-unit steps, spring settles ~1s) to spread
# them: center -> east -> south-east -> south-west -> north-west -> north.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/nyx"

if command -v kitty >/dev/null 2>&1; then
    TERM_BIN=kitty
elif command -v alacritty >/dev/null 2>&1; then
    TERM_BIN=alacritty
else
    echo "ERROR: need 'kitty' (preferred) or 'alacritty' in PATH" >&2
    exit 3
fi

spawn_here() {
    local title="$1" text="$2"
    if [[ "$TERM_BIN" == kitty ]]; then
        "$BIN" msg action spawn -- "$TERM_BIN" --title "$title" sh -c "printf '%s\n' '$text' 'title: $title'; sleep infinity" >/dev/null
    else
        "$BIN" msg action spawn -- "$TERM_BIN" --title "$title" -e sh -c "printf '%s\n' '$text' 'title: $title'; sleep infinity" >/dev/null
    fi
}

pan_wait() {
    "$BIN" msg action "$@" >/dev/null
    sleep 1.0
}

"$BIN" msg action home-canvas >/dev/null
sleep 0.5

spawn_here "nyx-zoom-01-center"    "ZOOM-01 CENTER red (0, 0)"
pan_wait pan-right
spawn_here "nyx-zoom-02-east"      "ZOOM-02 EAST green (900, 0)"
pan_wait pan-down
spawn_here "nyx-zoom-03-southeast" "ZOOM-03 SOUTH-EAST blue (900, 900)"
pan_wait pan-left
pan_wait pan-left
spawn_here "nyx-zoom-04-southwest" "ZOOM-04 SOUTH-WEST yellow (-900, 900)"
pan_wait pan-up
pan_wait pan-up
spawn_here "nyx-zoom-05-northwest" "ZOOM-05 NORTH-WEST magenta (-900, -900)"
pan_wait pan-right
spawn_here "nyx-zoom-06-north"     "ZOOM-06 NORTH cyan (0, -900)"

"$BIN" msg action home-canvas >/dev/null
echo "scattered 6 zoom markers ($TERM_BIN)."
