#!/usr/bin/env bash
# One-shot build and launch for macOS (Apple Silicon and Intel).
# Checks the macOS-specific prerequisites, then hands over to run-unix.sh,
# so it takes the same options:
#
#   ./run-macos.sh               build everything, start on http://127.0.0.1:8610
#   ./run-macos.sh --demo        also seed a DEMO project on first start
#   ./run-macos.sh --port 9000   use another port
#   ./run-macos.sh --no-build    start the last build without rebuilding
#   ./run-macos.sh --no-open     don't open a browser
set -euo pipefail
cd "$(dirname "$0")"

die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || die "this is the macOS script; on Linux use ./run-unix.sh"

case " $* " in
  *" --no-build "*) ;;
  *)
    # Rust links with Apple's toolchain from the Command Line Tools.
    xcode-select -p >/dev/null 2>&1 \
      || die "Xcode Command Line Tools are missing. Run: xcode-select --install"
    [ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"
    command -v cargo >/dev/null 2>&1 \
      || die "Rust is missing. Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   (or: brew install rustup && rustup-init)"
    command -v node >/dev/null 2>&1 \
      || die "Node.js is missing. Run: brew install node   (or download it from https://nodejs.org)"
    ;;
esac

exec ./run-unix.sh "$@"
