#!/usr/bin/env bash
set -euo pipefail

TARGET=${1:?usage: check-linux-release.sh <target> [binary-dir] [max-glibc]}
BIN_DIR=${2:-target/$TARGET/release}
MAX_GLIBC=${3:-2.28}

case "$TARGET" in
    x86_64-unknown-linux-gnu)
        MACHINE_PATTERN='Advanced Micro Devices X86-64'
        ;;
    aarch64-unknown-linux-gnu)
        MACHINE_PATTERN='AArch64'
        ;;
    *)
        printf 'release-check: unsupported target: %s\n' "$TARGET" >&2
        exit 2
        ;;
esac

for name in persist persistd persist-holder; do
    binary="$BIN_DIR/$name"
    [[ -x "$binary" ]] || {
        printf 'release-check: missing executable: %s\n' "$binary" >&2
        exit 2
    }
    header=$(readelf -h "$binary")
    grep -F "Machine:" <<<"$header" | grep -Fq "$MACHINE_PATTERN" || {
        printf 'release-check: wrong architecture: %s\n' "$binary" >&2
        exit 1
    }
    sections=$(readelf --sections "$binary")
    if grep -q '\.debug_' <<<"$sections"; then
        printf 'release-check: debug sections remain: %s\n' "$binary" >&2
        exit 1
    fi
    max_found=$(
        objdump -T "$binary" |
            grep -oE 'GLIBC_[0-9.]+' |
            cut -d_ -f2 |
            sort -Vu |
            tail -n 1
    )
    [[ -n "$max_found" ]] || {
        printf 'release-check: no GLIBC symbols found: %s\n' "$binary" >&2
        exit 1
    }
    if [[ "$(printf '%s\n' "$max_found" "$MAX_GLIBC" | sort -V | tail -n 1)" != "$MAX_GLIBC" ]]; then
        printf 'release-check: %s requires GLIBC_%s, maximum is %s\n' \
            "$binary" "$max_found" "$MAX_GLIBC" >&2
        exit 1
    fi
    printf '%s: GLIBC_%s, %s bytes\n' "$name" "$max_found" "$(stat --format=%s "$binary")"
done
