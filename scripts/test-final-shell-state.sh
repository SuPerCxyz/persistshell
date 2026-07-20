#!/usr/bin/env bash
set -euo pipefail

CARGO=${CARGO:-cargo}

run_case() {
    local label=$1
    shift
    printf '\n== %s ==\n' "$label"
    "$@"
}

run_case "shell state identity and atomic I/O" \
    "$CARGO" test -p persist-core shell_state
run_case "Holder exit context protocol" \
    "$CARGO" test -p persist-ipc holder
run_case "Holder runtime and offline retention" \
    "$CARGO" test -p persist-holder --tests
run_case "hidden state helper" \
    "$CARGO" test -p persist-cli shell_state
run_case "composable Bash, Zsh and Fish hooks" \
    "$CARGO" test -p persistd shell_history -- --nocapture
run_case "normal exit and Ctrl-D final cwd" \
    "$CARGO" test -p persistd --test reconciliation final_cwd
run_case "invalid state safely falls back" \
    "$CARGO" test -p persistd --test reconciliation invalid_state_file
run_case "metadata-first crash windows" \
    "$CARGO" test -p persistd --test reconciliation final_cwd_survives_crash
run_case "restart after metadata before retire" \
    "$CARGO" test -p persistd --test reconciliation restart_after_metadata

printf '\nFinal shell state suite passed.\n'
