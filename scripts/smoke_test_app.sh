#!/usr/bin/env bash
# Launch a packaged app where it cannot lean on the build machine, and fail if it dies.
#
# v1.0.0 crashed on launch everywhere but the CI runner: the SwiftPM resource
# accessor fell back to an absolute path inside .build, which only exists on the
# machine that built the app. This test removes both escape hatches on a copy of
# the app: the SwiftPM resource bundle and the .build directory.
#
# Usage: bash scripts/smoke_test_app.sh [path/to/App.app]   (default: build/T3 Code Bar.app)
# SMOKE_SECONDS sets how long the app must stay alive (default 4).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
APP="${1:-${PROJECT_DIR}/build/T3 Code Bar.app}"
SECONDS_ALIVE="${SMOKE_SECONDS:-4}"
BUILD_DIR="${PROJECT_DIR}/.build"
HIDDEN_DIR="${PROJECT_DIR}/.build.smoke-hidden"

[[ -d "${APP}" ]] || { echo "App not found: ${APP}" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/t3-code-bar-smoke.XXXXXX")"
PID=""
cleanup() {
    if [[ -n "${PID}" ]]; then kill "${PID}" 2>/dev/null || true; fi
    # Always put .build back, even when the test fails or is interrupted.
    if [[ -d "${HIDDEN_DIR}" && ! -e "${BUILD_DIR}" ]]; then mv "${HIDDEN_DIR}" "${BUILD_DIR}"; fi
    rm -rf "${WORK}"
}
trap cleanup EXIT

COPY="${WORK}/T3 Code Bar.app"
ditto "${APP}" "${COPY}"
rm -rf "${COPY}/Contents/Resources/T3CodeBar_T3CodeBar.bundle"
if [[ -d "${BUILD_DIR}" ]]; then mv "${BUILD_DIR}" "${HIDDEN_DIR}"; fi

echo "==> Launching a copy without the SwiftPM bundle or .build (${SECONDS_ALIVE}s)..."
"${COPY}/Contents/MacOS/T3CodeBar" >"${WORK}/app.log" 2>&1 &
PID=$!
disown "${PID}" # keeps bash from printing "Terminated" when cleanup kills it
sleep "${SECONDS_ALIVE}"

if kill -0 "${PID}" 2>/dev/null; then
    echo "==> Smoke test passed: the app stayed running."
else
    wait "${PID}" 2>/dev/null || true
    PID=""
    echo "Smoke test FAILED: the app exited within ${SECONDS_ALIVE}s." >&2
    head -n 10 "${WORK}/app.log" >&2
    exit 1
fi
