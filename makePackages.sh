#!/bin/sh

set -eu

usage() {
    echo "Usage: $0 <SourceDir> <BinaryDir> <package name> <package version> <package release> <PackageType> <architecture>" >&2
}

fail() {
    echo "makePackages.sh: $*" >&2
    exit 1
}

if [ "$#" -ne 7 ]; then
    usage
    exit 1
fi

SOURCE_DIR=$1
BUILD_DIR=$2
PACKAGE_NAME=$3
PACKAGE_VERSION=$4
PACKAGE_RELEASE=$5
PACKAGE_TYPE=$6
ARCHITECTURE=$7

[ -d "$SOURCE_DIR" ] || fail "source directory does not exist: $SOURCE_DIR"
[ -d "$BUILD_DIR" ] || fail "binary directory does not exist: $BUILD_DIR"

SOURCE_DIR=$(CDPATH= cd -- "$SOURCE_DIR" && pwd)
BUILD_DIR=$(CDPATH= cd -- "$BUILD_DIR" && pwd)

[ "$PACKAGE_NAME" = "autoruns" ] || fail "unsupported package name: $PACKAGE_NAME"

case "$PACKAGE_VERSION" in
    ''|*[!0-9A-Za-z.+~-]*) fail "invalid package version: $PACKAGE_VERSION" ;;
esac

MANIFEST="$SOURCE_DIR/Cargo.toml"
[ -f "$MANIFEST" ] || fail "Cargo.toml does not exist: $MANIFEST"

# Read the [package] version with POSIX awk so packaging needs no cargo/python3.
MANIFEST_VERSION=$(awk '
    /^\[/ { in_package = ($0 == "[package]") }
    in_package && /^[[:space:]]*version[[:space:]]*=/ {
        gsub(/.*=[[:space:]]*"/, ""); gsub(/".*/, ""); print; exit
    }
' "$MANIFEST")
[ -n "$MANIFEST_VERSION" ] || fail "could not determine the autoruns version from Cargo.toml"
[ "$PACKAGE_VERSION" = "$MANIFEST_VERSION" ] || fail "package version $PACKAGE_VERSION does not match Cargo.toml version $MANIFEST_VERSION"

case "$PACKAGE_RELEASE" in
    ''|*[!0-9]*) fail "invalid package release: $PACKAGE_RELEASE" ;;
esac

BINARY="$BUILD_DIR/autoruns"
[ -f "$BINARY" ] || fail "binary does not exist: $BINARY"
[ -x "$BINARY" ] || fail "binary is not executable: $BINARY"
command -v readelf >/dev/null 2>&1 || fail "readelf is required to verify the binary architecture"

ELF_HEADER=$(LC_ALL=C readelf -h "$BINARY") || fail "could not read ELF header: $BINARY"
ELF_CLASS=$(printf '%s\n' "$ELF_HEADER" | awk -F: '
    /^[[:space:]]*Class:/ {
        value = $2
        sub(/^[[:space:]]*/, "", value)
        sub(/[[:space:]]*$/, "", value)
        print value
        exit
    }
')
ELF_MACHINE=$(printf '%s\n' "$ELF_HEADER" | awk -F: '
    /^[[:space:]]*Machine:/ {
        value = $2
        sub(/^[[:space:]]*/, "", value)
        sub(/[[:space:]]*$/, "", value)
        print value
        exit
    }
')
[ "$ELF_CLASS" = "ELF64" ] || fail "binary class mismatch: expected ELF64, got ${ELF_CLASS:-unknown}"
[ -n "$ELF_MACHINE" ] || fail "could not determine ELF machine type: $BINARY"

case "$ARCHITECTURE" in
    amd64|x86_64) EXPECTED_MACHINE="Advanced Micro Devices X86-64" ;;
    arm64|aarch64) EXPECTED_MACHINE="AArch64" ;;
    *) EXPECTED_MACHINE= ;;
esac
[ -n "$EXPECTED_MACHINE" ] || fail "unsupported package architecture: $ARCHITECTURE"
[ "$ELF_MACHINE" = "$EXPECTED_MACHINE" ] || fail "binary architecture mismatch: requested $ARCHITECTURE, ELF machine is $ELF_MACHINE"

WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/autoruns-package.XXXXXX")
cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT HUP INT TERM

umask 022

case "$PACKAGE_TYPE" in
    deb)
        case "$ARCHITECTURE" in
            amd64|arm64) ;;
            *) fail "unsupported Debian architecture: $ARCHITECTURE" ;;
        esac

        command -v dpkg-deb >/dev/null 2>&1 || fail "dpkg-deb is required to build a Debian package"

        CONTROL_TEMPLATE="$SOURCE_DIR/dist/DEBIAN.in/control.in"
        [ -f "$CONTROL_TEMPLATE" ] || fail "Debian control template does not exist: $CONTROL_TEMPLATE"

        DEB_ROOT="$WORK_DIR/${PACKAGE_NAME}_${PACKAGE_VERSION}_${ARCHITECTURE}"
        mkdir -p "$DEB_ROOT/DEBIAN" "$DEB_ROOT/usr/bin"
        install -m 0755 "$BINARY" "$DEB_ROOT/usr/bin/autoruns"
        sed \
            -e "s/@PACKAGE_VERSION@/$PACKAGE_VERSION/g" \
            -e "s/@ARCHITECTURE@/$ARCHITECTURE/g" \
            "$CONTROL_TEMPLATE" > "$DEB_ROOT/DEBIAN/control"

        OUTPUT="$BUILD_DIR/${PACKAGE_NAME}_${PACKAGE_VERSION}_${ARCHITECTURE}.deb"
        dpkg-deb --build --root-owner-group "$DEB_ROOT" "$OUTPUT"
        ;;
    rpm)
        case "$ARCHITECTURE" in
            x86_64|aarch64) ;;
            *) fail "unsupported RPM architecture: $ARCHITECTURE" ;;
        esac

        command -v rpmbuild >/dev/null 2>&1 || fail "rpmbuild is required to build an RPM package"

        SPEC_TEMPLATE="$SOURCE_DIR/dist/SPECS.in/spec.in"
        [ -f "$SPEC_TEMPLATE" ] || fail "RPM spec template does not exist: $SPEC_TEMPLATE"

        RPM_ROOT="$WORK_DIR/rpmbuild"
        mkdir -p "$RPM_ROOT/BUILD" "$RPM_ROOT/BUILDROOT" "$RPM_ROOT/RPMS" "$RPM_ROOT/SOURCES" "$RPM_ROOT/SPECS" "$RPM_ROOT/SRPMS"
        install -m 0755 "$BINARY" "$RPM_ROOT/BUILD/autoruns"
        sed \
            -e "s/@PACKAGE_VERSION@/$PACKAGE_VERSION/g" \
            -e "s/@PACKAGE_RELEASE@/$PACKAGE_RELEASE/g" \
            "$SPEC_TEMPLATE" > "$RPM_ROOT/SPECS/autoruns.spec"

        rpmbuild --define "_topdir $RPM_ROOT" --target "$ARCHITECTURE" -bb "$RPM_ROOT/SPECS/autoruns.spec"

        set -- "$RPM_ROOT/RPMS/$ARCHITECTURE"/*.rpm
        [ "$#" -eq 1 ] && [ -f "$1" ] || fail "expected exactly one RPM output for $ARCHITECTURE"
        cp "$1" "$BUILD_DIR/$(basename "$1")"
        ;;
    *)
        fail "unsupported package type: $PACKAGE_TYPE"
        ;;
esac

echo "Created package in $BUILD_DIR"