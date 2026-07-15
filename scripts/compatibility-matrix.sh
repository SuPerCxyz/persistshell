#!/usr/bin/env bash
set -euo pipefail

PERSIST_BIN=${PERSIST_BIN:-target/release/persist}
PERSISTD_BIN=${PERSISTD_BIN:-target/release/persistd}
SHELLS=${PERSIST_COMPAT_SHELLS:-"bash zsh fish"}
TERM_VALUE=${PERSIST_COMPAT_TERM:-"${TERM:-xterm-256color}"}

for binary in "$PERSIST_BIN" "$PERSISTD_BIN"; do
    [[ -x "$binary" ]] || { printf 'compatibility: executable not found: %s\n' "$binary" >&2; exit 2; }
done

cleanup_case() {
    trap - RETURN
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    rm -rf "$root"
}

run_shell() {
    local shell_name=$1 shell_path root pid sid
    shell_path=$(command -v "$shell_name" || true)
    if [[ -z "$shell_path" ]]; then
        printf '%s,%s,%s\n' "$shell_name" "$TERM_VALUE" 'skipped: shell not installed'
        return
    fi
    root=$(mktemp -d "${TMPDIR:-/tmp}/persistshell-compat.XXXXXX")
    export XDG_CONFIG_HOME="$root/config" XDG_DATA_HOME="$root/data"
    export XDG_STATE_HOME="$root/state" XDG_RUNTIME_DIR="$root/runtime"
    export SHELL="$shell_path" TERM="$TERM_VALUE"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
    "$PERSISTD_BIN" foreground >"$root/daemon.log" 2>&1 & pid=$!
    trap cleanup_case RETURN
    for _ in $(seq 1 100); do [[ -S "$XDG_RUNTIME_DIR/persistshell/persist.sock" ]] && break; sleep 0.05; done
    [[ -S "$XDG_RUNTIME_DIR/persistshell/persist.sock" ]] || return 1
    sid=$("$PERSIST_BIN" new)
    "$PERSIST_BIN" ls | grep -E "^${sid}[[:space:]]" >/dev/null
    "$PERSIST_BIN" close "$sid" >/dev/null
    printf '%s,%s,passed\n' "$shell_name" "$TERM_VALUE"
}

printf 'shell,term,status\n'
for shell_name in $SHELLS; do run_shell "$shell_name"; done
