#!/usr/bin/env bash
# Nyx eyecandy visual test: opens the compositor nested (winit) and spawns
# 10 terminals (kitty, fallback: alacritty), each demonstrating ONE Hyprland-style
# effect in isolation. Fails loudly if anything is missing.
#
# Usage:
#   ./resources/eyecandy-test.sh            # build (if needed) + validate + run test
#   ./resources/eyecandy-test.sh --no-build # skip cargo build
#   ./resources/eyecandy-test.sh --validate-only
#
# Requirements: built nyx binary, kitty (or alacritty), an existing Wayland/X11
# session for the nested winit window. Cleans up compositor + terminals on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG="$ROOT/resources/eyecandy-test.kdl"
BIN="$ROOT/target/debug/nyx"
# NOTE: the nested compositor derives its own IPC socket at startup
# (discovered from its log below); nothing from the outer session is reused.

# Run from the repo root so the relative `L`-keybind respawn path
# (resources/eyecandy-respawn.sh) resolves inside the compositor.
cd "$ROOT"

NO_BUILD=0
VALIDATE_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --no-build) NO_BUILD=1 ;;
        --validate-only) VALIDATE_ONLY=1 ;;
        *) echo "unknown arg: $arg" >&2; exit 2 ;;
    esac
done

pick_terminal() {
    if command -v kitty >/dev/null 2>&1; then
        echo kitty
    elif command -v alacritty >/dev/null 2>&1; then
        echo alacritty
    else
        echo "ERROR: need 'kitty' (preferred) or 'alacritty' in PATH" >&2
        exit 3
    fi
}

TERM_BIN="$(pick_terminal)"
echo "terminal: $TERM_BIN"
echo "config:   $CONFIG"

if [[ ! -f "$CONFIG" ]]; then
    echo "ERROR: missing $CONFIG" >&2; exit 4
fi

if [[ "$NO_BUILD" -eq 0 ]]; then
    echo "==> cargo build (debug)…"
    (cd "$ROOT" && cargo build 2>&1 | tail -5)
fi

if [[ ! -x "$BIN" ]]; then
    echo "ERROR: $BIN not executable (build failed?)" >&2; exit 5
fi

echo "==> validate config…"
"$BIN" validate --config "$CONFIG"

echo "==> effects unit tests…"
(cd "$ROOT" && cargo test -q -p nyx --lib effects:: 2>&1 | tail -8)

if [[ "$VALIDATE_ONLY" -eq 1 ]]; then
    echo "validate-only: OK (13 eyecandy windows defined in $CONFIG)"
    exit 0
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" ]]; then
    echo "ERROR: XDG_RUNTIME_DIR is unset (needed for Wayland sockets)" >&2; exit 6
fi
if [[ -z "${WAYLAND_DISPLAY:-}" && -z "${DISPLAY:-}" ]]; then
    echo "WARNING: neither WAYLAND_DISPLAY nor DISPLAY is set; winit may fail to open." >&2
fi

# The nested compositor ALWAYS derives its own IPC socket
# (niri.<wayland-socket>.<pid>.sock) and ignores NYX_SOCKET/NIRI_SOCKET for
# binding. The outer session usually exports NIRI_SOCKET, so it must NOT be
# inherited here — otherwise `nyx msg` below would talk to the OUTER
# compositor. The real socket is discovered from the compositor log.
unset NYX_SOCKET NIRI_SOCKET
LOG="$ROOT/target/eyecandy-test.log"
rm -f "$LOG"

echo "==> starting nested compositor (winit; no --session so global env stays untouched)…"
"$BIN" --config "$CONFIG" >"$LOG" 2>&1 &
NYX_PID=$!
cleanup() {
    echo "==> cleanup (compositor pid $NYX_PID)…"
    kill "$NYX_PID" 2>/dev/null || true
    wait "$NYX_PID" 2>/dev/null || true
    # Stop only the NESTED swww-daemon (via pidfile) — never touch the outer one.
    if [[ -f /tmp/nyx-eyecandy-rt/daemon.pid ]]; then
        kill "$(cat /tmp/nyx-eyecandy-rt/daemon.pid)" 2>/dev/null || true
    fi
    rm -rf /tmp/nyx-eyecandy-rt
}
trap cleanup EXIT

echo "==> waiting for nested IPC socket (log: $LOG)…"
SOCK=""
for _ in $(seq 1 150); do
    if ! kill -0 "$NYX_PID" 2>/dev/null; then
        echo "ERROR: compositor died on startup, log tail:" >&2
        tail -25 "$LOG" >&2
        exit 7
    fi
    line="$(grep -a -m1 'IPC listening on: ' "$LOG" 2>/dev/null || true)"
    if [[ -n "$line" ]]; then
        SOCK="${line##*IPC listening on: }"
        SOCK="${SOCK%$'\r'}"
        if [[ -S "$SOCK" ]]; then
            break
        fi
    fi
    sleep 0.2
done
if [[ -z "$SOCK" || ! -S "$SOCK" ]]; then
    echo "ERROR: no IPC socket appeared, log tail:" >&2
    tail -25 "$LOG" >&2
    exit 7
fi
export NYX_SOCKET="$SOCK"
export NIRI_SOCKET="$SOCK"
echo "nested IPC socket: $SOCK"
if ! "$BIN" msg windows >/dev/null 2>&1; then
    echo "ERROR: IPC handshake failed at $SOCK, log tail:" >&2
    tail -25 "$LOG" >&2
    exit 7
fi
echo "compositor is up."

echo "==> spawning 13 eyecandy terminals…"
"$ROOT/resources/eyecandy-spawn.sh"

echo "==> waiting for 13 windows…"
OK=0
for _ in $(seq 1 100); do
    count="$("$BIN" msg windows 2>/dev/null | grep -c 'nyx-fx-' || true)"
    if [[ "$count" -ge 13 ]]; then OK=1; break; fi
    sleep 0.3
done

echo "----- nyx msg windows -----"
"$BIN" msg windows 2>/dev/null | grep -E 'nyx-fx-|Title' || "$BIN" msg windows
echo "---------------------------"

if [[ "$OK" -ne 1 ]]; then
    echo "ERROR: expected 13 nyx-fx-* windows (see list above)" >&2
    exit 8
fi

cat <<'EOF'
OK: 13 eyecandy windows live, tiled in a grid on the canvas (24px gaps).
  01 rounding/power   02 opacity   03 dim   04 blur         05 shadow
  06 gradient border  07 borderangle loop  08 popin/slide  09 move spring  10 camera
  11 blur vibrancy    12 blur noise        13 blur grade
Try: press L to close + respawn them all,
     focus through windows (02/03 dim+fade), Mod+wheel zoom,
     Mod+Tab overview, close+reopen 08 (popin),
     Mod+Ctrl+arrows swaps 09 with neighbours,
     Mod+Y drops tiling for the free canvas. Ctrl+C to quit + cleanup.
EOF

wait "$NYX_PID"
