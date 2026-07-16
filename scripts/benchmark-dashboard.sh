#!/usr/bin/env bash
set -euo pipefail

PERSIST_BIN=${PERSIST_BIN:-target/release/persist}
PERSISTD_BASELINE_BIN=${PERSISTD_BASELINE_BIN:-}
PERSISTD_DASHBOARD_BIN=${PERSISTD_DASHBOARD_BIN:-target/release/persistd}
COUNTS=${PERSIST_DASHBOARD_BENCH_COUNTS:-"100 1000"}
WARMUP_SECONDS=${PERSIST_DASHBOARD_BENCH_WARMUP_SECONDS:-10}
DURATION_SECONDS=${PERSIST_DASHBOARD_BENCH_DURATION_SECONDS:-30}
CPU_LIMIT_MILLI_PERCENT=${PERSIST_DASHBOARD_CPU_LIMIT_MILLI_PERCENT:-1000}
RING_SIZE=${PERSIST_DASHBOARD_BENCH_RING_SIZE:-"1KB"}
KEEP_FAILURE=${PERSIST_BENCH_KEEP_FAILURE:-0}

if [[ -z "$PERSISTD_BASELINE_BIN" ]]; then
    printf 'benchmark: PERSISTD_BASELINE_BIN is required\n' >&2
    exit 2
fi
for binary in "$PERSIST_BIN" "$PERSISTD_BASELINE_BIN" "$PERSISTD_DASHBOARD_BIN"; do
    if [[ ! -x "$binary" ]]; then
        printf 'benchmark: executable not found: %s\n' "$binary" >&2
        exit 2
    fi
done
for value in $COUNTS "$WARMUP_SECONDS" "$DURATION_SECONDS"; do
    if [[ ! "$value" =~ ^[1-9][0-9]*$ ]]; then
        printf 'benchmark: counts and durations must be positive integers\n' >&2
        exit 2
    fi
done

clock_ticks=$(getconf CLK_TCK)

read_cpu_ticks() {
    awk '{ print $14 + $15 }' "/proc/$1/stat"
}

read_rss_kib() {
    awk '/^VmRSS:/ { print $2; found=1 } END { if (!found) print 0 }' "/proc/$1/status"
}

read_threads() {
    awk '/^Threads:/ { print $2; found=1 } END { if (!found) print 0 }' "/proc/$1/status"
}

stop_daemon() {
    local pid=$1
    kill -TERM "$pid" 2>/dev/null || return 0
    for _ in $(seq 1 40); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.25
    done
    if kill -0 "$pid" 2>/dev/null; then
        kill -KILL "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
}

run_case() (
    local mode=$1 daemon_bin=$2 count=$3
    local root daemon_pid=0 completed=0 created=0
    local start_ns end_ns start_ticks end_ticks elapsed_ns cpu_milli
    local rss rss_sum=0 rss_max=0 rss_samples=0 threads metrics_bytes

    root=$(mktemp -d "${TMPDIR:-/tmp}/persistshell-dashboard-bench.XXXXXX")
    cleanup() {
        if [[ $daemon_pid -gt 0 ]]; then
            stop_daemon "$daemon_pid"
        fi
        if [[ $completed -eq 1 || $KEEP_FAILURE -ne 1 ]]; then
            rm -rf "$root"
        else
            printf 'benchmark: failed data kept at %s\n' "$root" >&2
        fi
    }
    trap cleanup EXIT

    export XDG_CONFIG_HOME="$root/config"
    export XDG_DATA_HOME="$root/data"
    export XDG_STATE_HOME="$root/state"
    export XDG_RUNTIME_DIR="$root/runtime"
    mkdir -p "$XDG_CONFIG_HOME/persistshell" "$XDG_DATA_HOME" \
        "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
    printf '[ring_buffer]\ndefault_size = "%s"\nmax_size = "%s"\nreplay_on_attach = false\nreplay_bytes = "%s"\n\n[logging]\nsession_log = false\n' \
        "$RING_SIZE" "$RING_SIZE" "$RING_SIZE" \
        >"$XDG_CONFIG_HOME/persistshell/config.toml"

    "$daemon_bin" foreground >"$root/daemon.log" 2>&1 &
    daemon_pid=$!
    for _ in $(seq 1 100); do
        [[ -S "$XDG_RUNTIME_DIR/persistshell/persist.sock" ]] && break
        sleep 0.05
    done
    if [[ ! -S "$XDG_RUNTIME_DIR/persistshell/persist.sock" ]]; then
        tail -100 "$root/daemon.log" >&2
        return 1
    fi

    for _ in $(seq 1 "$count"); do
        if ! "$PERSIST_BIN" new >/dev/null; then
            printf 'benchmark: %s creation failed after %s sessions\n' "$mode" "$created" >&2
            return 1
        fi
        created=$((created + 1))
    done
    sleep "$WARMUP_SECONDS"
    "$PERSIST_BIN" ls --plain >/dev/null

    start_ns=$(date +%s%N)
    start_ticks=$(read_cpu_ticks "$daemon_pid")
    for _ in $(seq 1 "$DURATION_SECONDS"); do
        rss=$(read_rss_kib "$daemon_pid")
        rss_sum=$((rss_sum + rss))
        rss_samples=$((rss_samples + 1))
        ((rss > rss_max)) && rss_max=$rss
        sleep 1
    done
    end_ticks=$(read_cpu_ticks "$daemon_pid")
    end_ns=$(date +%s%N)
    elapsed_ns=$((end_ns - start_ns))
    cpu_milli=$(((end_ticks - start_ticks) * 100000 * 1000000000 / clock_ticks / elapsed_ns))
    threads=$(read_threads "$daemon_pid")
    metrics_bytes=$(du -sb "$XDG_STATE_HOME/persistshell/metrics" 2>/dev/null | awk '{print $1}')
    metrics_bytes=${metrics_bytes:-0}
    "$PERSIST_BIN" ls --plain >/dev/null

    printf '%s,%s,%s,%s,%s,%s,%s\n' "$mode" "$count" "$cpu_milli" \
        "$((rss_sum / rss_samples))" "$rss_max" "$threads" "$metrics_bytes"
    completed=1
)

printf 'mode,sessions,cpu_milli_percent,rss_avg_kib,rss_peak_kib,threads,metrics_bytes\n'
printf 'benchmark: warmup=%ss duration=%ss ring_buffer=%s\n' \
    "$WARMUP_SECONDS" "$DURATION_SECONDS" "$RING_SIZE" >&2

for count in $COUNTS; do
    baseline=$(run_case baseline "$PERSISTD_BASELINE_BIN" "$count")
    dashboard=$(run_case dashboard "$PERSISTD_DASHBOARD_BIN" "$count")
    printf '%s\n%s\n' "$baseline" "$dashboard"

    if [[ "$count" -eq 100 ]]; then
        baseline_cpu=$(cut -d, -f3 <<<"$baseline")
        dashboard_cpu=$(cut -d, -f3 <<<"$dashboard")
        overhead=$((dashboard_cpu - baseline_cpu))
        ((overhead < 0)) && overhead=0
        printf 'benchmark: 100-session CPU overhead=%s milli-percent, limit=%s\n' \
            "$overhead" "$CPU_LIMIT_MILLI_PERCENT" >&2
        if ((overhead > CPU_LIMIT_MILLI_PERCENT)); then
            printf 'benchmark: dashboard CPU overhead exceeds limit\n' >&2
            exit 1
        fi
    fi
done
