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
UV_BIN=$(command -v uv 2>/dev/null || true)
TEMP_DIR=""

cleanup() {
    if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
        rm -rf -- "$TEMP_DIR"
    fi
}
trap cleanup EXIT HUP INT TERM

if [ -z "$UV_BIN" ]; then
    say "uv was not found; installing it for the current user..."
    TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lookup-setup.XXXXXX")
    INSTALLER="$TEMP_DIR/uv-installer.sh"

    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -LsSf \
            https://astral.sh/uv/install.sh -o "$INSTALLER"
    elif command -v wget >/dev/null 2>&1; then
        wget -q https://astral.sh/uv/install.sh -O "$INSTALLER"
    else
        fail "curl or wget is required to download uv."
    fi

    UV_NO_MODIFY_PATH=1 sh "$INSTALLER"

    for candidate in \
        "$HOME/.local/bin/uv" \
        "${XDG_BIN_HOME:-$HOME/.local/bin}/uv" \
        "$HOME/.cargo/bin/uv"
    do
        if [ -x "$candidate" ]; then
            UV_BIN=$candidate
            break
        fi
    done

    [ -n "$UV_BIN" ] || fail "uv installed, but its executable could not be located."
fi

say "Using uv: $UV_BIN"
say "Creating Lookup's Python environment..."

(
    cd "$PROJECT_DIR"
    UV_PYTHON_DOWNLOADS=automatic "$UV_BIN" venv --python 3.12 --allow-existing .venv
)

PYTHON_BIN="$PROJECT_DIR/.venv/bin/python"
LOOKUP_SCRIPT="$PROJECT_DIR/Search.py"
[ -x "$PYTHON_BIN" ] || fail "Python environment was not created at $PYTHON_BIN."
[ -f "$LOOKUP_SCRIPT" ] || fail "Lookup server was not found at $LOOKUP_SCRIPT."

say "Running an MCP startup check..."
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    | "$PYTHON_BIN" "$LOOKUP_SCRIPT" >/dev/null

say ""
say "Lookup is ready."
say ""
say "Configuration:"
say '{'
say '  "mcpServers": {'
say '    "Lookup": {'
say "      \"command\": \"$PYTHON_BIN\","
say "      \"args\": [\"$LOOKUP_SCRIPT\"]"
say '    }'
say '  }'
say '}'
