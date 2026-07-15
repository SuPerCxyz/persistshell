#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
BASH_COMPLETION=/usr/share/bash-completion/bash_completion

[[ -r "$BASH_COMPLETION" ]] || { printf 'test: bash-completion not found\n' >&2; exit 2; }
command -v zsh >/dev/null || { printf 'test: zsh not found\n' >&2; exit 2; }
command -v fish >/dev/null || { printf 'test: fish not found\n' >&2; exit 2; }

bash -n "$ROOT/completions/persist.bash"
zsh -n "$ROOT/completions/_persist"
fish -n "$ROOT/completions/persist.fish"

bash -c '
    set -eo pipefail
    source "$1"
    source "$2"
    persist() {
        printf "ID NAME STATUS\n41 demo running\n77 closed closed\n"
    }
    complete_for() {
        COMP_WORDS=("$@")
        COMP_CWORD=$((${#COMP_WORDS[@]} - 1))
        COMP_LINE="${COMP_WORDS[*]}"
        COMP_POINT=${#COMP_LINE}
        COMPREPLY=()
        _persist_completions
        printf "%s\n" "${COMPREPLY[@]}"
    }
    complete_for persist "" | grep -qx snapshot
    complete_for persist attach "" | grep -qx 41
    complete_for persist replay "" | grep -qx 77
    complete_for persist ls "--" | grep -qx -- --tag
    complete_for persist log "" | grep -qx export
' bash "$BASH_COMPLETION" "$ROOT/completions/persist.bash"

COMPLETION_FILE="$ROOT/completions/persist.fish" fish -c '
    source "$COMPLETION_FILE"
    complete -C "persist " | string match --quiet --entire snapshot
    complete -C "persist daemon " | string match --quiet --entire start
'

zsh -fc 'fpath=("$1" $fpath); autoload -Uz +X _persist; (( $+functions[_persist] ))' zsh "$ROOT/completions"

printf 'completion checks: passed\n'
