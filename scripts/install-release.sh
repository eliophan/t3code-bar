#!/usr/bin/env bash
# Install or update T3 Code Bar from a prebuilt GitHub Release. No sudo, no Swift.
#
#   curl -fsSL https://raw.githubusercontent.com/eliophan/t3code-bar/main/scripts/install-release.sh | bash
#   ... | bash -s -- --version v1.2.3   # install a specific version (also downgrades)
#   ... | bash -s -- --force             # reinstall even when already up to date
#
# Running it again updates an existing install. Everything lives in main() so a
# truncated download cannot execute half a script.
set -euo pipefail

REPO="eliophan/t3code-bar"
APP_NAME="T3 Code Bar.app"
# Test hooks: point these at file:// URLs to exercise the script without GitHub.
API_URL="${T3_CODE_BAR_API_URL:-https://api.github.com/repos/${REPO}/releases/latest}"
DOWNLOAD_BASE="${T3_CODE_BAR_RELEASE_BASE_URL:-https://github.com/${REPO}/releases/download}"
INSTALL_DIR="${T3_CODE_BAR_INSTALL_DIR:-${HOME}/Applications}"
DESTINATION="${INSTALL_DIR}/${APP_NAME}"

die() {
    echo "Error: $*" >&2
    exit 1
}

WORK=""
STAGE=""
cleanup() {
    if [[ -n "${STAGE}" ]]; then
        # Interrupted after moving the old app aside: put it back.
        if [[ -d "${STAGE}/previous.app" && ! -e "${DESTINATION}" ]]; then
            mv "${STAGE}/previous.app" "${DESTINATION}" || true
        fi
        rm -rf "${STAGE}"
    fi
    [[ -n "${WORK}" ]] && rm -rf "${WORK}"
    return 0
}

# Prints the app's version, or nothing if the app or its plist is missing.
# PlistBuddy writes "File Doesn't Exist" to stdout, so check the file first.
plist_version() {
    local plist="$1/Contents/Info.plist"
    [[ -f "${plist}" ]] || return 0
    /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "${plist}" 2>/dev/null || true
}

installed_version() {
    plist_version "${DESTINATION}"
}

latest_tag() {
    curl -fsSL --retry 3 "${API_URL}" \
        | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        | head -n 1
}

# Stop only the copy running from DESTINATION, never another instance elsewhere.
stop_running_app() {
    local pids
    pids="$(pgrep -f -- "${DESTINATION}/Contents/MacOS/T3CodeBar" || true)"
    [[ -z "${pids}" ]] && return 0
    echo "==> Quitting the running app..."
    # shellcheck disable=SC2086
    kill -TERM ${pids} 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        pgrep -f -- "${DESTINATION}/Contents/MacOS/T3CodeBar" >/dev/null || return 0
        sleep 0.5
    done
    # shellcheck disable=SC2086
    kill -KILL ${pids} 2>/dev/null || true
}

main() {
    local requested="" force=0
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --version) requested="${2:-}"; shift 2 || die "--version needs a value like v1.2.3" ;;
            --force) force=1; shift ;;
            -h|--help) sed -n '2,9p' "${BASH_SOURCE[0]:-$0}" 2>/dev/null || true; return 0 ;;
            *) die "Unknown option: $1" ;;
        esac
    done

    [[ "$(uname -s)" == "Darwin" ]] || die "T3 Code Bar requires macOS 14 or later."
    local os_major
    os_major="$(sw_vers -productVersion | cut -d. -f1)"
    (( os_major >= 14 )) || die "macOS 14 or later is required (found $(sw_vers -productVersion))."
    local arch="${T3_CODE_BAR_ARCH:-$(uname -m)}" # the override is a test hook
    case "${arch}" in
        arm64|x86_64) ;;
        *) die "Unsupported architecture: ${arch}." ;;
    esac

    local tag="${requested}"
    if [[ -z "${tag}" ]]; then
        echo "==> Looking up the latest release..."
        tag="$(latest_tag)" || die "Could not reach GitHub to find the latest release."
        [[ -n "${tag}" ]] || die "No release found for ${REPO}."
    fi
    [[ "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "Unexpected version '${tag}'; expected something like v1.2.3."
    local version="${tag#v}"

    local current
    current="$(installed_version)"
    if [[ -n "${current}" && "${current}" == "${version}" && "${force}" -eq 0 ]]; then
        echo "Already up to date (v${current}). Use --force to reinstall."
        return 0
    fi
    if [[ -e "${DESTINATION}" && -z "${current}" ]]; then
        echo "Note: replacing an existing app at ${DESTINATION} whose version could not be read."
    fi

    WORK="$(mktemp -d "${TMPDIR:-/tmp}/t3-code-bar-release.XXXXXX")"
    trap cleanup EXIT

    # New releases ship one universal build. Releases before that are named by
    # architecture and only exist for arm64, so fall back to that name.
    local zip_name="" candidate
    for candidate in "T3-Code-Bar-${version}-universal.zip" "T3-Code-Bar-${version}-${arch}.zip"; do
        echo "==> Downloading ${candidate}..."
        if curl -fSL --progress-bar --retry 3 -o "${WORK}/${candidate}" "${DOWNLOAD_BASE}/${tag}/${candidate}" 2>/dev/null; then
            zip_name="${candidate}"
            break
        fi
    done
    if [[ -z "${zip_name}" ]]; then
        if [[ "${arch}" == "x86_64" ]]; then
            die "Release ${tag} has no build for Intel Macs. Try a newer release, or build from source: git clone https://github.com/${REPO}.git && bash t3code-bar/scripts/install.sh"
        fi
        die "Download failed. Check that release ${tag} exists."
    fi
    curl -fsSL --retry 3 -o "${WORK}/${zip_name}.sha256" "${DOWNLOAD_BASE}/${tag}/${zip_name}.sha256" \
        || die "Checksum download failed."
    (cd "${WORK}" && shasum -a 256 -c "${zip_name}.sha256" >/dev/null 2>&1) \
        || die "Checksum mismatch; nothing was installed."

    ditto -x -k "${WORK}/${zip_name}" "${WORK}/extracted"
    local fresh="${WORK}/extracted/${APP_NAME}"
    [[ -d "${fresh}" ]] || die "The archive did not contain ${APP_NAME}."
    local packaged
    packaged="$(plist_version "${fresh}")"
    [[ "${packaged}" == "${version}" ]] || die "Archive reports version '${packaged}', expected '${version}'."

    mkdir -p "${INSTALL_DIR}"
    # Stage next to the destination so the final move is a same-volume rename.
    STAGE="$(mktemp -d "${INSTALL_DIR}/.t3-code-bar-install.XXXXXX")"
    ditto "${fresh}" "${STAGE}/${APP_NAME}"

    stop_running_app

    local backup=""
    if [[ -e "${DESTINATION}" || -L "${DESTINATION}" ]]; then
        backup="${STAGE}/previous.app"
        mv "${DESTINATION}" "${backup}"
    fi
    if ! mv "${STAGE}/${APP_NAME}" "${DESTINATION}"; then
        # cleanup() restores previous.app because DESTINATION is now missing.
        die "Could not place the new app; the previous version was restored."
    fi
    rm -rf "${STAGE}"
    STAGE=""

    if [[ -n "${current}" ]]; then
        echo "Updated v${current} -> v${version} at ${DESTINATION}"
    else
        echo "Installed v${version} at ${DESTINATION}"
    fi
    echo "Prebuilt app: ad hoc signed, no Developer ID signature, not notarized."
    if [[ "${T3_CODE_BAR_SKIP_OPEN:-0}" != "1" ]]; then
        open "${DESTINATION}"
    fi
}

main "$@"
