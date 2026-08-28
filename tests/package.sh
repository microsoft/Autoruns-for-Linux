#!/bin/sh

set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
BUILD_DIR_INPUT=${1:-target/release}
DEB_ARCHITECTURE=${2:-$(dpkg --print-architecture)}
RPM_ARCHITECTURE=${3:-$(rpm --eval '%{_arch}' 2>/dev/null || true)}
EXTRACT_DIR=$(mktemp -d "${TMPDIR:-/tmp}/autoruns-package-test.XXXXXX")
DEB_PACKAGE=
RPM_PACKAGE=
DEB_INSTALLED=0
REQUIRE_RPM_TESTS=${AUTORUNS_REQUIRE_RPM_TESTS:-1}

case "$REQUIRE_RPM_TESTS" in
    0|1) ;;
    *)
        echo "AUTORUNS_REQUIRE_RPM_TESTS must be 0 or 1" >&2
        exit 1
        ;;
esac

cleanup() {
    if [ "$DEB_INSTALLED" -eq 1 ]; then
        sudo dpkg --remove autoruns >/dev/null 2>&1 || true
    fi
    rm -rf "$EXTRACT_DIR"
    [ -z "$DEB_PACKAGE" ] || rm -f "$DEB_PACKAGE"
    [ -z "$RPM_PACKAGE" ] || rm -f "$RPM_PACKAGE"
}
trap cleanup EXIT HUP INT TERM

assert_equal() {
    LABEL=$1
    EXPECTED=$2
    ACTUAL=$3

    if [ "$ACTUAL" != "$EXPECTED" ]; then
        printf '%s mismatch: expected <%s>, got <%s>\n' "$LABEL" "$EXPECTED" "$ACTUAL" >&2
        exit 1
    fi
}

cd "$REPO_ROOT"
if [ "$#" -eq 0 ]; then
    cargo build --release
fi

[ -d "$BUILD_DIR_INPUT" ] || {
    echo "binary directory does not exist: $BUILD_DIR_INPUT" >&2
    exit 1
}
BUILD_DIR=$(CDPATH= cd -- "$BUILD_DIR_INPUT" && pwd)
VERSION=$(awk '
    /^\[/ { in_package = ($0 == "[package]") }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        gsub(/.*=[[:space:]]*"/, ""); gsub(/".*/, ""); print; exit
    }
' Cargo.toml)
DEB_PACKAGE="$BUILD_DIR/autoruns_${VERSION}_${DEB_ARCHITECTURE}.deb"

./makePackages.sh . "$BUILD_DIR" autoruns "$VERSION" 0 deb "$DEB_ARCHITECTURE"

[ "$(dpkg-deb --field "$DEB_PACKAGE" Package)" = "autoruns" ]
[ "$(dpkg-deb --field "$DEB_PACKAGE" Version)" = "$VERSION" ]
[ "$(dpkg-deb --field "$DEB_PACKAGE" Architecture)" = "$DEB_ARCHITECTURE" ]
[ "$(dpkg-deb --field "$DEB_PACKAGE" Maintainer)" = "Sysinternals <syssite@microsoft.com>" ]
[ "$(dpkg-deb --field "$DEB_PACKAGE" Homepage)" = "https://github.com/microsoft/Autoruns-for-Linux" ]
[ "$(dpkg-deb --field "$DEB_PACKAGE" Depends)" = "libc6, libgcc-s1" ]

PAYLOAD=$(dpkg-deb --contents "$DEB_PACKAGE" | awk '$1 ~ /^-/ { print $6 }')
[ "$PAYLOAD" = "./usr/bin/autoruns" ]

dpkg-deb --extract "$DEB_PACKAGE" "$EXTRACT_DIR"
[ "$(stat -c '%a' "$EXTRACT_DIR/usr/bin/autoruns")" = "755" ]

case "$(uname -m):$DEB_ARCHITECTURE" in
    x86_64:amd64|aarch64:arm64)
        "$EXTRACT_DIR/usr/bin/autoruns" --help | grep -q "Usage: autoruns"

        if [ "${AUTORUNS_PACKAGE_INSTALL_TEST:-0}" -eq 1 ]; then
            command -v sudo >/dev/null 2>&1 || {
                echo "sudo is required for the package install test" >&2
                exit 1
            }
            sudo dpkg --install "$DEB_PACKAGE"
            DEB_INSTALLED=1
            /usr/bin/autoruns --help | grep -q "Usage: autoruns"
            sudo dpkg --remove autoruns
            DEB_INSTALLED=0
            [ ! -e /usr/bin/autoruns ]
        fi
        ;;
esac

if ./makePackages.sh . "$BUILD_DIR" autoruns "${VERSION}.invalid" 0 deb "$DEB_ARCHITECTURE" >/dev/null 2>&1; then
    echo "package creation unexpectedly accepted a version that differs from Cargo.toml" >&2
    exit 1
fi

case "$DEB_ARCHITECTURE" in
    amd64) WRONG_ARCHITECTURE=arm64 ;;
    arm64) WRONG_ARCHITECTURE=amd64 ;;
    *)
        echo "unsupported test architecture: $DEB_ARCHITECTURE" >&2
        exit 1
        ;;
esac
if ./makePackages.sh . "$BUILD_DIR" autoruns "$VERSION" 0 deb "$WRONG_ARCHITECTURE" >/dev/null 2>&1; then
    echo "package creation unexpectedly accepted architecture $WRONG_ARCHITECTURE for the $DEB_ARCHITECTURE binary" >&2
    exit 1
fi

if command -v rpmbuild >/dev/null 2>&1 && command -v rpm >/dev/null 2>&1; then
    [ -n "$RPM_ARCHITECTURE" ] || {
        echo "could not determine the RPM architecture" >&2
        exit 1
    }

    find "$BUILD_DIR" -maxdepth 1 -type f -name "autoruns-${VERSION}-0*.${RPM_ARCHITECTURE}.rpm" -delete
    ./makePackages.sh . "$BUILD_DIR" autoruns "$VERSION" 0 rpm "$RPM_ARCHITECTURE"

    RPM_PACKAGE=$(find "$BUILD_DIR" -maxdepth 1 -type f -name "autoruns-${VERSION}-0*.${RPM_ARCHITECTURE}.rpm" -print)
    [ -n "$RPM_PACKAGE" ] && [ "$(printf '%s\n' "$RPM_PACKAGE" | wc -l)" -eq 1 ] || {
        echo "expected exactly one RPM package for $RPM_ARCHITECTURE" >&2
        exit 1
    }

    assert_equal "RPM name" "autoruns" "$(rpm -qp --queryformat '%{NAME}' "$RPM_PACKAGE")"
    assert_equal "RPM version" "$VERSION" "$(rpm -qp --queryformat '%{VERSION}' "$RPM_PACKAGE")"
    assert_equal "RPM release" "0" "$(rpm -qp --queryformat '%{RELEASE}' "$RPM_PACKAGE")"
    assert_equal "RPM architecture" "$RPM_ARCHITECTURE" "$(rpm -qp --queryformat '%{ARCH}' "$RPM_PACKAGE")"
    assert_equal "RPM license" "MIT" "$(rpm -qp --queryformat '%{LICENSE}' "$RPM_PACKAGE")"
    assert_equal "RPM URL" "https://github.com/microsoft/Autoruns-for-Linux" "$(rpm -qp --queryformat '%{URL}' "$RPM_PACKAGE")"
    assert_equal "RPM payload" "/usr/bin/autoruns" "$(rpm -qpl "$RPM_PACKAGE")"
    if ! rpm -qplv "$RPM_PACKAGE" | grep -q '^-rwxr-xr-x .* /usr/bin/autoruns$'; then
        echo "RPM executable mode is not 0755" >&2
        rpm -qplv "$RPM_PACKAGE" >&2
        exit 1
    fi
elif [ "$REQUIRE_RPM_TESTS" -eq 1 ]; then
    echo "rpmbuild and rpm are required; set AUTORUNS_REQUIRE_RPM_TESTS=0 only for an explicit DEB-only run" >&2
    exit 1
else
    echo "SKIPPED: RPM package validation explicitly disabled (AUTORUNS_REQUIRE_RPM_TESTS=0)" >&2
fi

if command -v rpmbuild >/dev/null 2>&1 && command -v rpm >/dev/null 2>&1; then
    echo "Package smoke test passed for DEB $DEB_ARCHITECTURE and RPM $RPM_ARCHITECTURE"
else
    echo "DEB package smoke test passed for $DEB_ARCHITECTURE (RPM explicitly skipped)"
fi