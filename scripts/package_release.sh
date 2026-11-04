#!/usr/bin/env bash
# Build the app at a given version and zip it for a GitHub Release.
# Usage: bash scripts/package_release.sh v1.2.3
# Output: build/T3-Code-Bar-<version>-<arch>.zip and a matching .sha256 file.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

RAW_VERSION="${1:-}"
VERSION="${RAW_VERSION#v}"
if [[ ! "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Usage: bash scripts/package_release.sh v1.2.3" >&2
    exit 1
fi

T3_CODE_BAR_VERSION="${VERSION}" bash "${SCRIPT_DIR}/build_app.sh"

APP="${PROJECT_DIR}/build/T3 Code Bar.app"
ARCH="$(lipo -archs "${APP}/Contents/MacOS/T3CodeBar")"
if [[ "${ARCH}" != "arm64" && "${ARCH}" != "x86_64" ]]; then
    echo "Expected a single-architecture binary, got: ${ARCH}" >&2
    exit 1
fi

ZIP_NAME="T3-Code-Bar-${VERSION}-${ARCH}.zip"
cd "${PROJECT_DIR}/build"
rm -f "${ZIP_NAME}" "${ZIP_NAME}.sha256"
ditto -c -k --keepParent "T3 Code Bar.app" "${ZIP_NAME}"
shasum -a 256 "${ZIP_NAME}" > "${ZIP_NAME}.sha256"

echo "==> Packaged ${PROJECT_DIR}/build/${ZIP_NAME}"
cat "${ZIP_NAME}.sha256"
