#!/usr/bin/env bash
set -euo pipefail

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
TARGET=${PERSIST_PACKAGE_TARGET:-x86_64-unknown-linux-gnu}
RPM_RELEASE=${PERSIST_PACKAGE_RPM_RELEASE:-1}
DIST=${PERSIST_PACKAGE_DIST:-dist}
REPO_ROOT=$(pwd -P)
PACKAGE_LIMIT=$((3 * 1024 * 1024))
TARBALL_LIMIT=$((7 * 1024 * 1024 / 2))

case "$TARGET" in
    x86_64-unknown-linux-gnu)
        DEB_ARCH=amd64
        RPM_ARCH=x86_64
        ;;
    aarch64-unknown-linux-gnu)
        DEB_ARCH=arm64
        RPM_ARCH=aarch64
        ;;
    *)
        printf 'package: unsupported target: %s\n' "$TARGET" >&2
        exit 2
        ;;
esac

NAME="persistshell-v${VERSION}-linux-${RPM_ARCH}"
BIN_DIR=${PERSIST_PACKAGE_BIN_DIR:-target/$TARGET/release}
if [[ ! -d "$BIN_DIR" && -z "${PERSIST_PACKAGE_BIN_DIR:-}" && "$TARGET" = "x86_64-unknown-linux-gnu" ]]; then
    BIN_DIR=target/release
fi

mkdir -p "$DIST"
DIST=$(cd "$DIST" && pwd -P)

[[ -n "$VERSION" ]] || { printf 'package: workspace version not found\n' >&2; exit 2; }
[[ -x "$BIN_DIR/persist" && -x "$BIN_DIR/persistd" && -x "$BIN_DIR/persist-holder" ]] || {
    printf 'package: build release binaries for %s first\n' "$TARGET" >&2; exit 2;
}
BIN_DIR=$(cd "$BIN_DIR" && pwd -P)

checksum() {
    artifact=$1
    (cd "$DIST" && sha256sum "$(basename "$artifact")" >"$(basename "$artifact").sha256")
}

check_size() {
    artifact=$1
    limit=$2
    size=$(stat --format=%s "$artifact")
    if (( size > limit )); then
        printf 'package: %s is %d bytes; limit is %d bytes\n' \
            "$(basename "$artifact")" "$size" "$limit" >&2
        return 1
    fi
}

prepare_root() {
    root=$1
    mkdir -p "$root/bin" "$root/libexec/persistshell" "$root/completions" "$root/docs/user" "$root/docs/man"
    install -m 0755 "$BIN_DIR/persist" "$root/bin/persist"
    install -m 0755 "$BIN_DIR/persistd" "$root/bin/persistd"
    install -m 0755 "$BIN_DIR/persist-holder" "$root/libexec/persistshell/persist-holder"
    install -m 0644 README.md LICENSE CHANGELOG.md "$root/"
    install -m 0644 docs/INDEX.md "$root/docs/INDEX.md"
    install -m 0644 completions/persist.bash completions/_persist completions/persist.fish "$root/completions/"
    install -m 0644 docs/user/*.md "$root/docs/user/"
    install -m 0644 docs/man/*.1 "$root/docs/man/"
}

package_tarball() {
    root="$DIST/$NAME"
    rm -rf "$root"
    prepare_root "$root"
    artifact="$DIST/$NAME.tar.xz"
    tar -cJf "$artifact" -C "$DIST" "$NAME"
    check_size "$artifact" "$TARBALL_LIMIT"
    checksum "$artifact"
}

package_deb() {
    command -v dpkg-deb >/dev/null || { printf 'package: dpkg-deb not found\n' >&2; return 2; }
    root="$DIST/deb-root"
    rm -rf "$root"
    mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/libexec/persistshell" "$root/usr/share/bash-completion/completions" "$root/usr/share/zsh/site-functions" "$root/usr/share/fish/vendor_completions.d" "$root/usr/share/doc/persistshell" "$root/usr/share/man/man1"
    install -m 0755 "$BIN_DIR/persist" "$root/usr/bin/persist"
    install -m 0755 "$BIN_DIR/persistd" "$root/usr/bin/persistd"
    install -m 0755 "$BIN_DIR/persist-holder" "$root/usr/libexec/persistshell/persist-holder"
    install -m 0644 completions/persist.bash "$root/usr/share/bash-completion/completions/persist"
    install -m 0644 completions/_persist "$root/usr/share/zsh/site-functions/_persist"
    install -m 0644 completions/persist.fish "$root/usr/share/fish/vendor_completions.d/persist.fish"
    install -m 0644 README.md LICENSE CHANGELOG.md "$root/usr/share/doc/persistshell/"
    install -m 0644 docs/user/*.md "$root/usr/share/doc/persistshell/"
    install -m 0644 docs/man/*.1 "$root/usr/share/man/man1/"
    printf 'Package: persistshell\nVersion: %s\nArchitecture: %s\nMaintainer: PersistShell contributors\nDepends: libc6 (>= 2.28), libgcc-s1\nDescription: Persistent interactive shell runtime\n' \
        "$VERSION" "$DEB_ARCH" >"$root/DEBIAN/control"
    artifact="$DIST/persistshell_${VERSION}_${DEB_ARCH}.deb"
    dpkg-deb --build --root-owner-group -Zxz -z9 "$root" "$artifact"
    check_size "$artifact" "$PACKAGE_LIMIT"
    checksum "$artifact"
}

package_rpm() {
    command -v rpmbuild >/dev/null || { printf 'package: rpmbuild not found\n' >&2; return 2; }
    topdir="$DIST/rpm-build"
    rm -rf "$topdir"
    mkdir -p "$topdir/BUILD" "$topdir/RPMS" "$topdir/SOURCES" "$topdir/SPECS"
    spec="$topdir/SPECS/persistshell.spec"
    cat >"$spec" <<EOF
Name: persistshell
Version: $VERSION
Release: $RPM_RELEASE
Summary: Persistent interactive shell runtime
License: MIT
BuildArch: $RPM_ARCH

%description
Persistent interactive shell runtime.

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/libexec/persistshell %{buildroot}/usr/share/bash-completion/completions %{buildroot}/usr/share/zsh/site-functions %{buildroot}/usr/share/fish/vendor_completions.d %{buildroot}/usr/share/doc/persistshell %{buildroot}/usr/share/man/man1
install -m 0755 "$BIN_DIR/persist" %{buildroot}/usr/bin/persist
install -m 0755 "$BIN_DIR/persistd" %{buildroot}/usr/bin/persistd
install -m 0755 "$BIN_DIR/persist-holder" %{buildroot}/usr/libexec/persistshell/persist-holder
install -m 0644 "$REPO_ROOT/completions/persist.bash" %{buildroot}/usr/share/bash-completion/completions/persist
install -m 0644 "$REPO_ROOT/completions/_persist" %{buildroot}/usr/share/zsh/site-functions/_persist
install -m 0644 "$REPO_ROOT/completions/persist.fish" %{buildroot}/usr/share/fish/vendor_completions.d/persist.fish
install -m 0644 "$REPO_ROOT/README.md" "$REPO_ROOT/LICENSE" "$REPO_ROOT/CHANGELOG.md" "$REPO_ROOT"/docs/user/*.md %{buildroot}/usr/share/doc/persistshell/
install -m 0644 "$REPO_ROOT"/docs/man/*.1 %{buildroot}/usr/share/man/man1/

%files
/usr/bin/persist
/usr/bin/persistd
/usr/libexec/persistshell/persist-holder
/usr/share/bash-completion/completions/persist
/usr/share/zsh/site-functions/_persist
/usr/share/fish/vendor_completions.d/persist.fish
/usr/share/doc/persistshell
/usr/share/man/man1
EOF
    rpmbuild \
        --define "_topdir $topdir" \
        --define "_binary_payload w9.xzdio" \
        -bb "$spec"
    rpm_path=$(find "$topdir/RPMS" -name 'persistshell-*.rpm' -print -quit)
    [[ -n "$rpm_path" ]] || { printf 'package: rpm artifact not found\n' >&2; return 1; }
    cp "$rpm_path" "$DIST/"
    artifact="$DIST/$(basename "$rpm_path")"
    check_size "$artifact" "$PACKAGE_LIMIT"
    checksum "$artifact"
}

for format in "$@"; do
    case "$format" in
        tarball) package_tarball ;;
        deb) package_deb ;;
        rpm) package_rpm ;;
        *) printf 'usage: %s [tarball|deb|rpm]\n' "$0" >&2; exit 2 ;;
    esac
done
