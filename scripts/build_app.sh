#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Local builds keep 1.0.0. Release packaging passes the tag's version so the
# installed app reports it and install-release.sh can compare versions.
VERSION="${T3_CODE_BAR_VERSION:-1.0.0}"
if [[ ! "${VERSION}" =~ ^[0-9]+(\.[0-9]+){1,2}$ ]]; then
    echo "T3_CODE_BAR_VERSION must look like 1.2.3, got: ${VERSION}" >&2
    exit 1
fi

echo "==> Building T3CodeBar ${VERSION} in Release mode..."
cd "${PROJECT_DIR}"
swift build -c release

BIN_PATH="$(swift build -c release --show-bin-path)/T3CodeBar"
APP_DIR="${PROJECT_DIR}/build/T3 Code Bar.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
RESOURCE_BUNDLE="$(dirname "${BIN_PATH}")/T3CodeBar_T3CodeBar.bundle"

if [ ! -d "${RESOURCE_BUNDLE}" ]; then
    echo "Required SwiftPM resource bundle not found: ${RESOURCE_BUNDLE}" >&2
    exit 1
fi

echo "==> Packaging into ${APP_DIR}..."
rm -rf "${APP_DIR}"
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

cp "${BIN_PATH}" "${MACOS_DIR}/T3CodeBar"
cp "${PROJECT_DIR}/LICENSE" "${PROJECT_DIR}/THIRD_PARTY_NOTICES.md" "${RESOURCES_DIR}/"
cp -r "${PROJECT_DIR}/Sources/Resources/"* "${RESOURCES_DIR}/"
cp -r "${RESOURCE_BUNDLE}" "${RESOURCES_DIR}/"

cat << EOF > "${CONTENTS_DIR}/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>T3CodeBar</string>
    <key>CFBundleIdentifier</key>
    <string>com.t3code.bar</string>
    <key>CFBundleName</key>
    <string>T3 Code Bar</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
EOF

codesign --force --sign - "${APP_DIR}"

echo "==> Build and package completed successfully!"
echo "App created at: ${APP_DIR}"
echo "Architecture: $(lipo -archs "${MACOS_DIR}/T3CodeBar")"
echo "Local build only. No Developer ID signing or notarization is performed."
