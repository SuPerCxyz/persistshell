#!/usr/bin/env bash
set -euo pipefail

PERSIST_BIN=${PERSIST_BIN:-target/release/persist}
PERSISTD_BIN=${PERSISTD_BIN:-target/release/persistd}
COUNTS=${PERSIST_BENCH_COUNTS:-"100 500 1000"}
RING_SIZE=${PERSIST_BENCH_RING_SIZE:-"1KB"}
KEEP_FAILURE=${PERSIST_BENCH_KEEP_FAILURE:-0}

for binary in "$PERSIST_BIN" "$PERSISTD_BIN"; do
    if [[ ! -x "$binary" ]]; then
        printf 'benchmark: executable not found: %s\n' "$binary" >&2
        exit 2
    fi
done

elapsed_ms() {
    local start_ns=$1
    local end_ns
    end_ns=$(date +%s%N)
    printf '%s' $(((end_ns - start_ns) / 1000000))
}

cleanup_case() {
    trap - RETURN
    kill -TERM "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
    if [[ $completed -eq 1 || $KEEP_FAILURE -ne 1 ]]; then
        rm -rf "$root"
    else
        printf 'benchmark: failed data kept at %s\n' "$root" >&2
    fi
}

run_case() {
    local count=$1
    local root daemon_pid start_ns create_ms list_ms close_ms first_id id created=0 completed=0
    root=$(mktemp -d "${TMPDIR:-/tmp}/persistshell-bench.XXXXXX")
    export XDG_CONFIG_HOME="$root/config"
    export XDG_DATA_HOME="$root/data"
    export XDG_STATE_HOME="$root/state"
    export XDG_RUNTIME_DIR="$root/runtime"
    mkdir -p "$XDG_CONFIG_HOME/persistshell" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
    printf '[ring_buffer]\ndefault_size = "%s"\nmax_size = "%s"\nreplay_on_attach = false\nreplay_bytes = "%s"\n\n[logging]\nsession_log = false\n' \
        "$RING_SIZE" "$RING_SIZE" "$RING_SIZE" >"$XDG_CONFIG_HOME/persistshell/config.toml"

    "$PERSISTD_BIN" foreground >"$root/daemon.log" 2>&1 &
    daemon_pid=$!
    trap cleanup_case RETURN

    for _ in $(seq 1 100); do
        [[ -S "$XDG_RUNTIME_DIR/persistshell/persist.sock" ]] && break
        sleep 0.05
    done
    [[ -S "$XDG_RUNTIME_DIR/persistshell/persist.sock" ]] || {
        cat "$root/daemon.log" >&2
        return 1
    }

    first_id=1
    start_ns=$(date +%s%N)
    for _ in $(seq 1 "$count"); do
        if ! "$PERSIST_BIN" new >/dev/null; then
            printf 'benchmark: creation failed after %s sessions\n' "$created" >&2
            return 1
        fi
        created=$((created + 1))
    done
    create_ms=$(elapsed_ms "$start_ns")

    start_ns=$(date +%s%N)
    "$PERSIST_BIN" ls >/dev/null
    list_ms=$(elapsed_ms "$start_ns")

    start_ns=$(date +%s%N)
    for id in $(seq "$first_id" "$count"); do
        "$PERSIST_BIN" close "$id" >/dev/null
    done
    close_ms=$(elapsed_ms "$start_ns")

    printf '%s,%s,%s,%s\n' "$count" "$create_ms" "$list_ms" "$close_ms"
    completed=1
}

printf 'sessions,create_ms,list_ms,close_ms\n'
printf 'benchmark: ring_buffer=%s, session_log=false\n' "$RING_SIZE" >&2
for count in $COUNTS; do
    run_case "$count"
done
