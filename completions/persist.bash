_persist_completions() {
    local cur prev words cword
    _init_completion || return

    local subcmds="-h --help -V --version help version doctor config daemon new ls ps stats snapshot metrics top attach close kill log rename detach note tag pin unpin lock unlock replay install uninstall"

    if [[ $cword -eq 1 ]]; then
        COMPREPLY=($(compgen -W "$subcmds" -- "$cur"))
        return
    fi

    case "${words[1]}" in
        help|version|doctor|new|metrics|top|install)
            COMPREPLY=()
            ;;
        config)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "show" -- "$cur"))
            fi
            ;;
        daemon)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "start stop status" -- "$cur"))
            fi
            ;;
        ls)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "-t --tag" -- "$cur"))
            fi
            ;;
        attach)
            if [[ $cword -ge 2 ]]; then
                COMPREPLY=($(compgen -W "$(_persist_session_ids) -r --readonly" -- "$cur"))
            fi
            ;;
        ps|stats|snapshot|close|kill|rename|detach|note|pin|unpin|lock|unlock)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "$(_persist_session_ids)" -- "$cur"))
            fi
            ;;
        log)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "$(_persist_session_ids) export search" -- "$cur"))
            elif [[ ${words[2]} == "export" && $cword -eq 3 ]]; then
                COMPREPLY=($(compgen -W "$(_persist_session_ids)" -- "$cur"))
            elif [[ ${words[2]} == "export" ]]; then
                COMPREPLY=($(compgen -W "-o --output" -- "$cur"))
            elif [[ ${words[2]} == "search" && $cword -gt 3 ]]; then
                COMPREPLY=($(compgen -W "-s --session -i --ignore-case" -- "$cur"))
            fi
            ;;
        replay)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "$(_persist_session_ids)" -- "$cur"))
            else
                COMPREPLY=($(compgen -W "-t --tail -h --head -s --speed -f --follow" -- "$cur"))
            fi
            ;;
        tag)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "$(_persist_session_ids)" -- "$cur"))
            elif [[ $cword -eq 3 ]]; then
                COMPREPLY=($(compgen -W "add remove list" -- "$cur"))
            fi
            ;;
        uninstall)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "--purge" -- "$cur"))
            fi
            ;;
        *)
            COMPREPLY=()
            ;;
    esac
}

_persist_session_ids() {
    # Try to list session IDs from the daemon if available
    if command -v persist &>/dev/null; then
        persist ls 2>/dev/null | awk 'NR > 1 && $1 ~ /^[0-9]+$/ {print $1}'
    fi
}

complete -F _persist_completions persist
