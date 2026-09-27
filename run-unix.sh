#!/usr/bin/env bash
# One-shot build and launch for Linux and other Unix systems.
# (macOS: use ./run-macos.sh, which checks the Xcode tools and then runs this.)
#
#   ./run-unix.sh               build everything, start on http://127.0.0.1:8610
#   ./run-unix.sh --demo        also seed a DEMO project on first start
#   ./run-unix.sh --port 9000   use another port
#   ./run-unix.sh --no-build    start the last build without rebuilding
#   ./run-unix.sh --no-open     don't open a browser
#
# Data lives in ./data/kanban.db (override with KANBAN_DB). Ctrl+C stops it.
set -euo pipefail
cd "$(dirname "$0")"

DEMO=0
OPEN=1
BUILD=1
PORT="${KANBAN_PORT:-8610}"

while [ $# -gt 0 ]; do
  case "$1" in
    --demo) DEMO=1 ;;
    --no-open) OPEN=0 ;;
    --no-build) BUILD=0 ;;
    --port) PORT="${2:?--port needs a number}"; shift ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
  esac
  shift
done

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 not found. $2"; }

# rustup installs here but only adds it to PATH for new login shells.
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"

BIN="server/target/release/kanban-server"

if [ "$BUILD" = 1 ]; then
  need cargo "Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  need cc "Install a C compiler (Debian/Ubuntu: sudo apt install build-essential; Fedora: sudo dnf install gcc; macOS: xcode-select --install)"
  need node "Install Node.js 20.19 or newer: https://nodejs.org"
  need npm "npm comes with Node.js: https://nodejs.org"
  node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit(a>22||(a===22&&b>=12)||(a===20&&b>=19)?0:1)' \
    || die "Node.js $(node -v) is too old; the frontend build needs 20.19+ or 22.12+"

  say "Building the frontend"
  (
    cd web
    # Reinstall only when the lockfile changed since the last install.
    if [ ! -f node_modules/.package-lock.json ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
      npm ci --no-audit --no-fund
    fi
    npm run build
  )

  say "Building the server (the first build takes a few minutes)"
  (cd server && cargo build --release --locked)
elif [ ! -x "$BIN" ]; then
  die "no build found at $BIN; run without --no-build first"
fi

mkdir -p data
export KANBAN_DB="${KANBAN_DB:-$PWD/data/kanban.db}"
export KANBAN_PORT="$PORT"
URL="http://127.0.0.1:$PORT"

if [ "$OPEN" = 1 ]; then
  (
    for _ in $(seq 1 80); do
      if "$BIN" healthcheck; then
        if [ "$(uname -s)" = Darwin ]; then open "$URL"
        elif command -v xdg-open >/dev/null 2>&1; then xdg-open "$URL" >/dev/null 2>&1 || true
        fi
        exit 0
      fi
      sleep 0.25
    done
  ) &
fi

say "Starting on $URL (database: $KANBAN_DB). Ctrl+C to stop."
ARGS=()
[ "$DEMO" = 1 ] && ARGS+=(--demo)
exec "$BIN" "${ARGS[@]+"${ARGS[@]}"}"
