#!/usr/bin/env bash
set -euo pipefail

PERSIST=${PERSIST_BIN:-/usr/bin/persist}
PERSISTD=${PERSISTD_BIN:-/usr/bin/persistd}
ROOT=${PERSIST_TEST_ROOT:?set PERSIST_TEST_ROOT to an isolated directory}
if [[ "$ROOT" != /* || "$ROOT" == / ]]; then
    echo "PERSIST_TEST_ROOT must be a non-root absolute path" >&2
    exit 2
fi
CURRENT_DAEMON=

cleanup() {
    if test -n "$CURRENT_DAEMON" && kill -0 "$CURRENT_DAEMON" 2>/dev/null; then
        "$PERSIST" daemon stop >/dev/null 2>&1 || kill "$CURRENT_DAEMON" 2>/dev/null || true
        wait "$CURRENT_DAEMON" 2>/dev/null || true
    fi
}
trap cleanup EXIT

wait_status() {
    local expected=$1
    for _ in $(seq 1 160); do
        "$PERSIST" ls --plain 2>/dev/null | grep -q "$expected" && return
        sleep 0.05
    done
    return 1
}

run_shell() {
    local name=$1 shell=$2 set_cmd=$3 unset_cmd=$4
    local base="$ROOT/$name"
    rm -rf "$base"
    mkdir -p "$base"/{home,runtime,config/persistshell,data,state}
    chmod 700 "$base/runtime"
    export HOME="$base/home"
    export XDG_RUNTIME_DIR="$base/runtime"
    export XDG_CONFIG_HOME="$base/config"
    export XDG_DATA_HOME="$base/data"
    export XDG_STATE_HOME="$base/state"
    export SHELL="$shell"
    export STAGE8_API_TOKEN=stage8-secret-must-not-persist
    printf '[recovery.environment]\ninclude = ["EDITOR"]\n' \
        >"$base/config/persistshell/config.toml"

    "$PERSISTD" foreground >"$base/daemon.out" 2>"$base/daemon.err" &
    local daemon=$!
    CURRENT_DAEMON=$daemon
    for _ in $(seq 1 100); do
        test -S "$base/runtime/persistshell/persist.sock" && break
        kill -0 "$daemon"
        sleep 0.05
    done
    "$PERSIST" new >/dev/null
    { sleep 0.5; printf '%s\n' "$set_cmd"; } |
        script -q -e -c "$PERSIST attach 1" /dev/null >/dev/null
    wait_status closed

    local output
    output=$({ sleep 0.5; printf '%s\n' \
        "printf 'stage8-set=%s\\n' \"\$EDITOR\"; $unset_cmd; exit"; } |
        script -q -e -c "$PERSIST attach 1" /dev/null)
    grep -q 'stage8-set=stage8-value' <<<"$output"
    wait_status closed

    output=$({ sleep 0.5; printf '%s\n' \
        "printf 'stage8-unset=%s\\n' \"\$EDITOR\"; exit"; } |
        script -q -e -c "$PERSIST attach 1" /dev/null)
    grep -q 'stage8-unset=' <<<"$output"
    ! grep -q 'stage8-unset=stage8-value' <<<"$output"
    wait_status closed
    ! find "$base/data" "$base/state" "$base/runtime" -type f -print0 |
        xargs -0 -r grep -a -E 'STAGE8_API_TOKEN|stage8-secret-must-not-persist'
    "$PERSIST" daemon stop >/dev/null
    wait "$daemon"
    CURRENT_DAEMON=
    printf '%s dynamic environment passed\n' "$name"
}

default_shell=$(getent passwd "$(id -u)" | cut -d: -f7)
case "$default_shell" in
    */fish)
        run_shell default "$default_shell" \
            'set -gx EDITOR stage8-value; exit' 'set -e EDITOR'
        ;;
    *)
        run_shell default "$default_shell" \
            'export EDITOR=stage8-value; exit' 'unset EDITOR'
        ;;
esac
