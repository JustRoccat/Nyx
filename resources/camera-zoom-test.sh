#!/usr/bin/env bash
# Nyx camera-zoom visual test: nested compositor (winit) + 6 labeled windows
# scattered in a ring on the FREE canvas. Press L to cycle exact camera zooms
# (1.0 -> 0.7 -> 0.4 -> 0.25 -> 1.5 -> 2.5 -> …) via the `set-zoom` action.
#
# Usage:
#   ./resources/camera-zoom-test.sh            # build (if needed) + validate + run
#   ./resources/camera-zoom-test.sh --no-build # skip cargo build
#   ./resources/camera-zoom-test.sh --validate-only
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG="$ROOT/resources/camera-zoom-test.kdl"
BIN="$ROOT/target/debug/nyx"
LOG="$ROOT/target/camera-zoom-test.log"
# NOTE: the nested compositor derives its own IPC socket at startup
# (discovered from its log below); nothing from the outer session is reused.

# Run from the repo root so the relative `L`-keybind cycle path
# (resources/camera-zoom-cycle.sh) resolves inside the compositor.
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

if command -v kitty >/dev/null 2>&1; then
    TERM_BIN=kitty
elif command -v alacritty >/dev/null 2>&1; then
    TERM_BIN=alacritty
else
    echo "ERROR: need 'kitty' (preferred) or 'alacritty' in PATH" >&2
    exit 3
fi
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

echo "==> viewport unit tests…"
(cd "$ROOT" && cargo test -q -p nyx --lib canvas::viewport 2>&1 | tail -5)

if [[ "$VALIDATE_ONLY" -eq 1 ]]; then
    echo "validate-only: OK (6 zoom markers + L zoom cycle in $CONFIG)"
    exit 0
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" ]]; then
    echo "ERROR: XDG_RUNTIME_DIR is unset (needed for Wayland sockets)" >&2; exit 6
fi
if [[ -z "${WAYLAND_DISPLAY:-}" && -z "${DISPLAY:-}" ]]; then
    echo "WARNING: neither WAYLAND_DISPLAY nor DISPLAY is set; winit may fail to open." >&2
fi

# The nested compositor ALWAYS derives its own IPC socket — never inherit the
# outer session's socket or `nyx msg` talks to the wrong compositor.
unset NYX_SOCKET NIRI_SOCKET
rm -f "$LOG"

echo "==> starting nested compositor (winit; no --session so global env stays untouched)…"
"$BIN" --config "$CONFIG" >"$LOG" 2>&1 &
NYX_PID=$!
cleanup() {
    echo "==> cleanup (compositor pid $NYX_PID)…"
    kill "$NYX_PID" 2>/dev/null || true
    wait "$NYX_PID" 2>/dev/null || true
    # Stop only the NESTED swww-daemon (via pidfile) — never touch the outer one.
    if [[ -f /tmp/nyx-zoom-test-rt/daemon.pid ]]; then
        kill "$(cat /tmp/nyx-zoom-test-rt/daemon.pid)" 2>/dev/null || true
    fi
    rm -rf /tmp/nyx-zoom-test-rt
    rm -f /tmp/nyx-zoom-cycle.idx
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

echo "==> scattering 6 zoom markers (takes ~10s: pan, settle, spawn)…"
"$ROOT/resources/camera-zoom-spawn.sh"

echo "==> waiting for 6 windows…"
OK=0
for _ in $(seq 1 100); do
    count="$("$BIN" msg windows 2>/dev/null | grep -c 'nyx-zoom-' || true)"
    if [[ "$count" -ge 6 ]]; then OK=1; break; fi
    sleep 0.3
done

echo "----- nyx msg windows -----"
"$BIN" msg windows 2>/dev/null | grep -E 'nyx-zoom-|Title' || "$BIN" msg windows
echo "---------------------------"

if [[ "$OK" -ne 1 ]]; then
    echo "ERROR: expected 6 nyx-zoom-* windows (see list above)" >&2
    exit 8
fi

cat <<'EOF'
OK: 6 zoom markers live in a ring on the free canvas.
  Press L to cycle exact zooms: 1.0 -> 0.7 -> 0.4 -> 0.25 -> 1.5 -> 2.5 -> …
  (smooth spring glide between presets; notify-send shows the level if installed)
Try: Mod+wheel zoom around cursor, Mod+Tab overview, Mod+HJKL pan,
     Mod+0 back to 1.0. Ctrl+C to quit + cleanup.
EOF

wait "$NYX_PID"
