#!/usr/bin/env bash
# Build the app at a given version and zip it for a GitHub Release.
# Usage: bash scripts/package_release.sh v1.2.3
# Output: build/T3-Code-Bar-<version>-universal.zip (Apple Silicon and Intel) and a matching .sha256 file.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

RAW_VERSION="${1:-}"
VERSION="${RAW_VERSION#v}"
if [[ ! "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Usage: bash scripts/package_release.sh v1.2.3" >&2
    exit 1
fi

T3_CODE_BAR_UNIVERSAL=1 T3_CODE_BAR_VERSION="${VERSION}" bash "${SCRIPT_DIR}/build_app.sh"

APP="${PROJECT_DIR}/build/T3 Code Bar.app"
# Never ship an app that cannot launch away from the build machine.
bash "${SCRIPT_DIR}/smoke_test_app.sh" "${APP}"
ARCHS="$(lipo -archs "${APP}/Contents/MacOS/T3CodeBar")"
if [[ " ${ARCHS} " != *" arm64 "* || " ${ARCHS} " != *" x86_64 "* ]]; then
    echo "Expected a universal binary with arm64 and x86_64, got: ${ARCHS}" >&2
    exit 1
fi

ZIP_NAME="T3-Code-Bar-${VERSION}-universal.zip"
cd "${PROJECT_DIR}/build"
rm -f "${ZIP_NAME}" "${ZIP_NAME}.sha256"
ditto -c -k --keepParent "T3 Code Bar.app" "${ZIP_NAME}"
shasum -a 256 "${ZIP_NAME}" > "${ZIP_NAME}.sha256"

echo "==> Packaged ${PROJECT_DIR}/build/${ZIP_NAME}"
cat "${ZIP_NAME}.sha256"
