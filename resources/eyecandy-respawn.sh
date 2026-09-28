#!/usr/bin/env bash
# Eyecandy showcase restart: closes ALL nyx-fx-* windows by ID, then respawns
# the full set. Bound to the `L` key in resources/eyecandy-test.kdl.
#
# Runs inside the nested compositor's environment (NYX_SOCKET set for its
# children), so no address needs hardcoding. Only touches windows whose title
# starts with `nyx-fx-` — everything else is left alone.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/nyx"

if [[ -z "${NYX_SOCKET:-}" && -z "${NIRI_SOCKET:-}" ]]; then
    echo "eyecandy-respawn: NYX_SOCKET/NIRI_SOCKET not set, nothing to do" >&2
    exit 1
fi

ids="$("$BIN" msg --json windows 2>/dev/null | python3 -c '
import json, sys
try:
    wins = json.load(sys.stdin)
except Exception:
    wins = []
for w in wins:
    t = w.get("title") or ""
    if t.startswith("nyx-fx-"):
        print(w["id"])
' || true)"

count=0
for id in $ids; do
    "$BIN" msg action close-window --id "$id" >/dev/null 2>&1 || true
    count=$((count + 1))
done
echo "eyecandy-respawn: closed $count window(s)."

# Wait until they are actually gone (client shutdown takes a moment).
for _ in $(seq 1 50); do
    left="$("$BIN" msg --json windows 2>/dev/null | python3 -c '
import json, sys
try:
    wins = json.load(sys.stdin)
except Exception:
    wins = []
print(sum(1 for w in wins if (w.get("title") or "").startswith("nyx-fx-")))
' || echo 99)"
    if [[ "$left" == "0" ]]; then
        break
    fi
    sleep 0.1
done

exec "$ROOT/resources/eyecandy-spawn.sh"
