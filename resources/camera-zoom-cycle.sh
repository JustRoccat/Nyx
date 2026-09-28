#!/usr/bin/env bash
# Camera-zoom preset cycler for resources/camera-zoom-test.kdl (the `L` key).
#
# Each press advances to the next exact zoom via `set-zoom`:
#   1.0 -> 0.7 -> 0.4 -> 0.25 -> 1.5 -> 2.5 -> back to 1.0 -> …
# Position persists in /tmp/nyx-zoom-cycle.idx between presses.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/nyx"
IDX_FILE="/tmp/nyx-zoom-cycle.idx"
PRESETS="1.0 0.7 0.4 0.25 1.5 2.5"

if [[ -z "${NYX_SOCKET:-}" && -z "${NIRI_SOCKET:-}" ]]; then
    echo "camera-zoom-cycle: NYX_SOCKET/NIRI_SOCKET not set, nothing to do" >&2
    exit 1
fi

# shellcheck disable=SC2206
PRESET_ARR=($PRESETS)
N=${#PRESET_ARR[@]}

idx=0
if [[ -f "$IDX_FILE" ]]; then
    idx="$(cat "$IDX_FILE" 2>/dev/null || echo 0)"
    [[ "$idx" =~ ^[0-9]+$ ]] || idx=0
fi

zoom="${PRESET_ARR[$((idx % N))]}"
echo "$(((idx + 1) % N))" > "$IDX_FILE"

"$BIN" msg action set-zoom "$zoom" >/dev/null 2>&1 || {
    echo "camera-zoom-cycle: set-zoom $zoom failed" >&2
    exit 1
}

echo "camera zoom -> $zoom"
if command -v notify-send >/dev/null 2>&1; then
    notify-send -t 1200 "Nyx zoom test" "camera zoom -> $zoom" || true
fi
