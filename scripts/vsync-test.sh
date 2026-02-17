#!/bin/bash
# Run vsync-test with correct library paths

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Set library path based on platform
if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS - Homebrew paths
    if [[ -d "/opt/homebrew/lib" ]]; then
        export DYLD_LIBRARY_PATH="/opt/homebrew/lib:$DYLD_LIBRARY_PATH"
    elif [[ -d "/usr/local/lib" ]]; then
        export DYLD_LIBRARY_PATH="/usr/local/lib:$DYLD_LIBRARY_PATH"
    fi
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    # Linux - standard paths should work, but add common locations
    export LD_LIBRARY_PATH="/usr/lib:/usr/local/lib:$LD_LIBRARY_PATH"
fi

# Run the test
exec "$PROJECT_DIR/extension/target/release/vsync-test" "$@"
