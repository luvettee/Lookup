#!/bin/sh

set -eu

say() {
    printf '%s\n' "$*"
}

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

case "$(uname -s)" in
    Darwin|Linux) ;;
    *) fail "This updater supports macOS and Linux only." ;;
esac

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
CARGO_BIN=$(command -v cargo 2>/dev/null || true)

if [ -z "$CARGO_BIN" ]; then
    for candidate in \
        "$HOME/.cargo/bin/cargo" \
        "/opt/homebrew/bin/cargo" \
        "/usr/local/bin/cargo"
    do
        if [ -x "$candidate" ]; then
            CARGO_BIN=$candidate
            break
        fi
    done
fi

[ -n "$CARGO_BIN" ] || fail "Cargo is required to rebuild Lookup. Run ./setup.sh first."

say "Updating and building Lookup..."
(
    cd "$PROJECT_DIR"
    if [ -d ".git" ] && command -v git >/dev/null 2>&1; then
        say "Pulling latest changes from git..."
        git pull --ff-only || true
    fi
    "$CARGO_BIN" build --release
)

LOOKUP_BIN="$PROJECT_DIR/target/release/lookup"
[ -x "$LOOKUP_BIN" ] || fail "Lookup binary was not found at $LOOKUP_BIN."

say "Validating new binary..."
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    | "$LOOKUP_BIN" >/dev/null

say "Lookup is up to date and ready."
