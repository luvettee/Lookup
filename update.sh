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
SOURCE_URL=${LOOKUP_UPDATE_URL:-https://raw.githubusercontent.com/luvettee/Lookup/main/Search.py}
TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/lookup-update.XXXXXX")
STAGED_SOURCE="$TEMP_DIR/Search.py"

cleanup() {
    rm -rf -- "$TEMP_DIR"
}
trap cleanup EXIT HUP INT TERM

if [ -x "$PROJECT_DIR/.venv/bin/python" ]; then
    PYTHON_BIN="$PROJECT_DIR/.venv/bin/python"
elif command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN=$(command -v python3)
elif command -v python >/dev/null 2>&1; then
    PYTHON_BIN=$(command -v python)
else
    fail "Python is required to validate the update. Run ./setup.sh first."
fi

say "Downloading the newest Search.py..."
if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -LsSf "$SOURCE_URL" -o "$STAGED_SOURCE"
elif command -v wget >/dev/null 2>&1; then
    wget -q "$SOURCE_URL" -O "$STAGED_SOURCE"
else
    fail "curl or wget is required to download updates."
fi

[ -s "$STAGED_SOURCE" ] || fail "The downloaded Search.py is empty."

say "Validating the new source..."
"$PYTHON_BIN" -c \
    'import pathlib, sys; compile(pathlib.Path(sys.argv[1]).read_bytes(), sys.argv[1], "exec")' \
    "$STAGED_SOURCE"
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    | PYTHONDONTWRITEBYTECODE=1 "$PYTHON_BIN" "$STAGED_SOURCE" >/dev/null

mv -f -- "$STAGED_SOURCE" "$PROJECT_DIR/Search.py"
say "Lookup source is up to date. Local configuration and environment were preserved."
