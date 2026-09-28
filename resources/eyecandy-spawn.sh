#!/usr/bin/env bash
# Spawns all eyecandy showcase terminals into the RUNNING nested compositor.
#
# Requires NYX_SOCKET (or NIRI_SOCKET) pointing at the nested instance in the
# environment — set automatically for children spawned by the compositor
# (keybinds, spawn-at-startup) and by resources/eyecandy-test.sh.
# Used by eyecandy-test.sh on startup and by the `L` keybind (respawn).
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

spawn_fx() {
    local title="$1" text="$2"
    if [[ "$TERM_BIN" == kitty ]]; then
        "$BIN" msg action spawn -- "$TERM_BIN" --title "$title" sh -c "printf '%s\n' '$text' 'title: $title'; sleep infinity" >/dev/null
    else
        "$BIN" msg action spawn -- "$TERM_BIN" --title "$title" -e sh -c "printf '%s\n' '$text' 'title: $title'; sleep infinity" >/dev/null
    fi
}

spawn_fx "nyx-fx-01-rounding"      "01 ROUNDING + POWER (r=24, power=3.5)"
spawn_fx "nyx-fx-02-opacity"       "02 OPACITY (inactive 0.55 — focus another window)"
spawn_fx "nyx-fx-03-dim"           "03 DIM (inactive, strength 0.5)"
spawn_fx "nyx-fx-04-blur"          "04 BLUR (translucent 0.85 + vibrancy 0.2)"
spawn_fx "nyx-fx-05-shadow"        "05 SHADOW (soft 60, power 4, scale 1.5)"
spawn_fx "nyx-fx-06-gradient"      "06 GRADIENT BORDER (static linear)"
spawn_fx "nyx-fx-07-borderangle"   "07 BORDERANGLE LOOP (rotating gradient)"
spawn_fx "nyx-fx-08-popin"         "08 POPIN+SLIDE open/close (close+reopen me: Mod+Q / Mod+T)"
spawn_fx "nyx-fx-09-move"          "09 MOVE SPRING (Mod+Ctrl+arrows swaps slots)"
spawn_fx "nyx-fx-10-camera"        "10 CAMERA (pan Mod+HJKL, zoom Mod+wheel, overview Mod+Tab)"
spawn_fx "nyx-fx-11-blur-vibrancy" "11 BLUR VIBRANCY 0.7 (strong color bleed-through)"
spawn_fx "nyx-fx-12-blur-noise"    "12 BLUR NOISE 0.3 (film grain)"
spawn_fx "nyx-fx-13-blur-grade"    "13 BLUR GRADE (contrast 1.6, brightness +0.12, sat 2.0)"

echo "spawned 13 eyecandy windows ($TERM_BIN)."
