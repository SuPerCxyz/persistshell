#!/usr/bin/env bash
set -euo pipefail

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
TARGET=${PERSIST_PACKAGE_TARGET:-x86_64-unknown-linux-gnu}
DIST=${PERSIST_PACKAGE_DIST:-dist}
NAME="persistshell-v${VERSION}-${TARGET}"
REPO_ROOT=$(pwd -P)

[[ -n "$VERSION" ]] || { printf 'package: workspace version not found\n' >&2; exit 2; }
[[ -x target/release/persist && -x target/release/persistd ]] || {
    printf 'package: build release binaries first\n' >&2; exit 2;
}

prepare_root() {
    root=$1
    mkdir -p "$root/bin" "$root/completions" "$root/docs/user" "$root/docs/man"
    install -m 0755 target/release/persist "$root/bin/persist"
    install -m 0755 target/release/persistd "$root/bin/persistd"
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
    tar -czf "$DIST/$NAME.tar.gz" -C "$DIST" "$NAME"
    (cd "$DIST" && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256")
}

package_deb() {
    command -v dpkg-deb >/dev/null || { printf 'package: dpkg-deb not found\n' >&2; return 2; }
    root="$DIST/deb-root"
    rm -rf "$root"
    mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/bash-completion/completions" "$root/usr/share/zsh/site-functions" "$root/usr/share/fish/vendor_completions.d" "$root/usr/share/doc/persistshell" "$root/usr/share/man/man1"
    install -m 0755 target/release/persist "$root/usr/bin/persist"
    install -m 0755 target/release/persistd "$root/usr/bin/persistd"
    install -m 0644 completions/persist.bash "$root/usr/share/bash-completion/completions/persist"
    install -m 0644 completions/_persist "$root/usr/share/zsh/site-functions/_persist"
    install -m 0644 completions/persist.fish "$root/usr/share/fish/vendor_completions.d/persist.fish"
    install -m 0644 README.md LICENSE CHANGELOG.md "$root/usr/share/doc/persistshell/"
    install -m 0644 docs/user/*.md "$root/usr/share/doc/persistshell/"
    install -m 0644 docs/man/*.1 "$root/usr/share/man/man1/"
    printf 'Package: persistshell\nVersion: %s\nArchitecture: amd64\nMaintainer: PersistShell contributors\nDescription: Persistent interactive shell runtime\n' "$VERSION" >"$root/DEBIAN/control"
    dpkg-deb --build --root-owner-group "$root" "$DIST/persistshell_${VERSION}_amd64.deb"
    (cd "$DIST" && sha256sum "persistshell_${VERSION}_amd64.deb" >"persistshell_${VERSION}_amd64.deb.sha256")
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
Release: 1
Summary: Persistent interactive shell runtime
License: MIT
BuildArch: x86_64

%description
Persistent interactive shell runtime.

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/share/bash-completion/completions %{buildroot}/usr/share/zsh/site-functions %{buildroot}/usr/share/fish/vendor_completions.d %{buildroot}/usr/share/doc/persistshell %{buildroot}/usr/share/man/man1
install -m 0755 "$REPO_ROOT/target/release/persist" %{buildroot}/usr/bin/persist
install -m 0755 "$REPO_ROOT/target/release/persistd" %{buildroot}/usr/bin/persistd
install -m 0644 "$REPO_ROOT/completions/persist.bash" %{buildroot}/usr/share/bash-completion/completions/persist
install -m 0644 "$REPO_ROOT/completions/_persist" %{buildroot}/usr/share/zsh/site-functions/_persist
install -m 0644 "$REPO_ROOT/completions/persist.fish" %{buildroot}/usr/share/fish/vendor_completions.d/persist.fish
install -m 0644 "$REPO_ROOT/README.md" "$REPO_ROOT/LICENSE" "$REPO_ROOT/CHANGELOG.md" "$REPO_ROOT"/docs/user/*.md %{buildroot}/usr/share/doc/persistshell/
install -m 0644 "$REPO_ROOT"/docs/man/*.1 %{buildroot}/usr/share/man/man1/

%files
/usr/bin/persist
/usr/bin/persistd
/usr/share/bash-completion/completions/persist
/usr/share/zsh/site-functions/_persist
/usr/share/fish/vendor_completions.d/persist.fish
/usr/share/doc/persistshell
/usr/share/man/man1
EOF
    rpmbuild --define "_topdir $(pwd)/$topdir" -bb "$spec"
    rpm_path=$(find "$topdir/RPMS" -name 'persistshell-*.rpm' -print -quit)
    [[ -n "$rpm_path" ]] || { printf 'package: rpm artifact not found\n' >&2; return 1; }
    cp "$rpm_path" "$DIST/"
    (cd "$DIST" && sha256sum "$(basename "$rpm_path")" >"$(basename "$rpm_path").sha256")
}

mkdir -p "$DIST"
for format in "$@"; do
    case "$format" in
        tarball) package_tarball ;;
        deb) package_deb ;;
        rpm) package_rpm ;;
        *) printf 'usage: %s [tarball|deb|rpm]\n' "$0" >&2; exit 2 ;;
    esac
done
