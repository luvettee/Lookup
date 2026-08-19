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
    *) fail "This installer supports macOS and Linux only." ;;
esac

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
CARGO_BIN=$(command -v cargo 2>/dev/null || true)
TEMP_DIR=""

cleanup() {
    if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
        rm -rf -- "$TEMP_DIR"
    fi
}
trap cleanup EXIT HUP INT TERM

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

if [ -z "$CARGO_BIN" ]; then
    say "Rust/Cargo was not found; installing Rust via rustup for the current user..."
    TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lookup-setup.XXXXXX")
    INSTALLER="$TEMP_DIR/rustup-init.sh"

    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o "$INSTALLER"
    elif command -v wget >/dev/null 2>&1; then
        wget -q https://sh.rustup.rs -O "$INSTALLER"
    else
        fail "curl or wget is required to download rustup."
    fi

    sh "$INSTALLER" -y --profile minimal --default-toolchain stable

    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        CARGO_BIN="$HOME/.cargo/bin/cargo"
    fi

    [ -n "$CARGO_BIN" ] || fail "Rust installed, but cargo executable could not be located."
fi

say "Using Cargo: $CARGO_BIN"
say "Building Lookup release binary..."

(
    cd "$PROJECT_DIR"
    "$CARGO_BIN" build --release
)

LOOKUP_BIN="$PROJECT_DIR/target/release/lookup"
[ -x "$LOOKUP_BIN" ] || fail "Lookup binary was not found at $LOOKUP_BIN."

say "Running an MCP startup check..."
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    | "$LOOKUP_BIN" >/dev/null

say ""
say "Lookup is ready."
say ""
say "Configuration:"
say '{'
say '  "mcpServers": {'
say '    "Lookup": {'
say "      \"command\": \"$LOOKUP_BIN\","
say '      "args": []'
say '    }'
say '  }'
say '}'
