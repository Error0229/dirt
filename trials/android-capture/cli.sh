#!/usr/bin/env bash
# The CLI as the second device, with its own data folder and server.
. "$(dirname "$0")/env.sh"
export DIRT_DATA_DIR="$W/target/trial2-cli-data"
export DIRT_API_BASE_URL="http://127.0.0.1:$PORT"
unset MSYS_NO_PATHCONV
"$W/target/debug/dirt.exe" "$@"
