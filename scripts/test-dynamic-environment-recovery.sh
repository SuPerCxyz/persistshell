#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
CARGO=${CARGO:-cargo}
TIMEOUT=${PERSIST_TEST_TIMEOUT:-180}

cd "$ROOT"

run() {
    local label=$1
    shift
    printf '==> %s\n' "$label"
    timeout "$TIMEOUT" "$@"
}

run "policy, config and envelope" \
    "$CARGO" test -p persist-core shell_state
run "metadata legacy/v2 codec" \
    "$CARGO" test -p persist-metadata environment
run "shell helper compatibility" \
    "$CARGO" test -p persistd shell_history_tests
run "public connection context" \
    "$CARGO" test -p persist-ipc attach_accepts_legacy_and_round_trips_connection_environment
run "PTY set/unset" \
    "$CARGO" test -p persist-pty launch_environment
run "Holder minor compatibility" \
    "$CARGO" test -p persistd holder::tests
run "Holder offline exit context" \
    "$CARGO" test -p persist-holder --test runtime_exit_context
run "metadata-first crash windows" \
    "$CARGO" test -p persistd --test reconciliation final_cwd_survives_crash
run "metadata/retire idempotency" \
    "$CARGO" test -p persistd --test reconciliation restart_after_metadata_before_retire
run "dynamic set/unset restore" \
    "$CARGO" test -p persistd --test reconciliation closed_attach_restores_set_then_persists_unset
run "cross-client restore" \
    "$CARGO" test -p persistd --test reconciliation second_client_connection_context
run "sensitive leak scan" \
    "$CARGO" test -p persistd --test reconciliation inherited_sensitive_environment

printf 'M55 dynamic environment recovery checks passed.\n'
