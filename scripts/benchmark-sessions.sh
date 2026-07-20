#!/usr/bin/env bash
set -euo pipefail

PERSIST_BIN=${PERSIST_BIN:-target/release/persist}
PERSISTD_BIN=${PERSISTD_BIN:-target/release/persistd}
COUNTS=${PERSIST_BENCH_COUNTS:-"100 500 1000"}
RING_SIZE=${PERSIST_BENCH_RING_SIZE:-"1KB"}
KEEP_FAILURE=${PERSIST_BENCH_KEEP_FAILURE:-0}
HOLDER_BIN=${PERSIST_HOLDER_BIN:-}

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

rss_kb() {
    awk '/^VmRSS:/ { print $2; found=1 } END { if (!found) print 0 }' "/proc/$1/status"
}

cpu_ticks() {
    awk '{ print $14 + $15 }' "/proc/$1/stat"
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
    local root daemon_pid holder_pid start_ns create_ms list_ms attach_ms close_ms first_id id
    local daemon_rss holder_rss daemon_ticks holder_ticks
    local created=0 completed=0
    root=$(mktemp -d "${TMPDIR:-/tmp}/persistshell-bench.XXXXXX")
    export XDG_CONFIG_HOME="$root/config"
    export XDG_DATA_HOME="$root/data"
    export XDG_STATE_HOME="$root/state"
    export XDG_RUNTIME_DIR="$root/runtime"
    mkdir -p "$XDG_CONFIG_HOME/persistshell" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
    if [[ -n "$HOLDER_BIN" ]]; then
        export PERSIST_HOLDER_PATH="$HOLDER_BIN"
    else
        unset PERSIST_HOLDER_PATH || true
    fi
    printf '[ring_buffer]\ndefault_size = "%s"\nmax_size = "%s"\nreplay_on_attach = true\nreplay_bytes = "%s"\n\n[logging]\nsession_log = false\n' \
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
    holder_pid=$(<"$XDG_RUNTIME_DIR/persistshell/holder.pid")
    kill -0 "$holder_pid"

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

    daemon_rss=$(rss_kb "$daemon_pid")
    holder_rss=$(rss_kb "$holder_pid")
    daemon_ticks=$(cpu_ticks "$daemon_pid")
    holder_ticks=$(cpu_ticks "$holder_pid")

    "$PERSIST_BIN" new >/dev/null
    start_ns=$(date +%s%N)
    "$PERSIST_BIN" __benchmark-attach "$((count + 1))" >/dev/null
    attach_ms=$(elapsed_ms "$start_ns")
    "$PERSIST_BIN" close "$((count + 1))" >/dev/null

    start_ns=$(date +%s%N)
    for id in $(seq "$first_id" "$count"); do
        "$PERSIST_BIN" close "$id" >/dev/null
    done
    close_ms=$(elapsed_ms "$start_ns")

    printf '%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
        "$count" "$create_ms" "$list_ms" "$close_ms" "$attach_ms" \
        "$daemon_rss" "$holder_rss" "$daemon_ticks" "$holder_ticks"
    completed=1
}

printf 'sessions,create_ms,list_ms,close_ms,attach_writer_ms,daemon_rss_kb,holder_rss_kb,daemon_cpu_ticks,holder_cpu_ticks\n'
printf 'benchmark: ring_buffer=%s, replay_on_attach=true, session_log=false\n' "$RING_SIZE" >&2
for count in $COUNTS; do
    run_case "$count"
done
