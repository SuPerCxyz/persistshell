#!/usr/bin/env bash
set -euo pipefail

CARGO=${CARGO:-cargo}

run_case() {
    local label=$1
    shift
    printf '\n== %s ==\n' "$label"
    "$@"
}

run_case \
    "private protocol bounds and rejection" \
    "$CARGO" test -p persist-ipc holder

run_case \
    "Holder lifecycle, permissions, PTY and bounded output" \
    "$CARGO" test -p persist-holder --tests

run_case \
    "daemon SIGKILL, Holder survival and recovered attach operations" \
    "$CARGO" test -p persistd --test persistd \
    daemon_crash_leaves_holder_for_next_daemon_claim

run_case \
    "create/commit/exit reconciliation, Holder loss and log degradation" \
    "$CARGO" test -p persistd --test reconciliation

run_case \
    "final cwd survives metadata-first crash windows" \
    "$CARGO" test -p persistd --test reconciliation \
    final_cwd_survives_crash

run_case \
    "metadata commit before retire is idempotent" \
    "$CARGO" test -p persistd --test reconciliation \
    restart_after_metadata

printf '\nHolder recovery fault suite passed.\n'
