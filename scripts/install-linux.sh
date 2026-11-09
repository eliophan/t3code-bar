#!/usr/bin/env bash
# Install the versioned Linux AppImage without root privileges.
set -euo pipefail

fail() { printf 'T3 Code Bar: %s\n' "$*" >&2; exit 1; }
[[ "$(uname -s)" == Linux ]] || fail 'This installer supports Linux only.'
if [[ "${1:-}" == --help ]]; then
  printf 'Usage: bash install-linux.sh [VERSION]\nDefault: 2.0.0-alpha.2. Installs an AppImage and a desktop launcher for the current user.\n'
  exit 0
fi
[[ $# -le 1 ]] || fail 'Usage: bash install-linux.sh [VERSION]'
version="${1:-2.0.0-alpha.2}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || fail 'Invalid release version.'
case "$(uname -m)" in
  x86_64) architecture=x86_64 ;;
  aarch64|arm64) architecture=aarch64 ;;
  *) fail 'Unsupported Linux architecture.' ;;
esac
for command in curl python3 sha256sum install mktemp; do
  command -v "$command" >/dev/null || fail "Install $command first."
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fetch() {
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location \
    --retry 2 --connect-timeout 10 --max-time 300 "$1" --output "$2"
}
repository='https://github.com/eliophan/t3code-bar'
fetch "https://api.github.com/repos/eliophan/t3code-bar/releases/tags/v${version}" "$work/release.json" || fail "Release v${version} is unavailable. See ${repository}/releases."
# Select an exact version and CPU from the published release.
python3 - "$work/release.json" "$version" "$architecture" > "$work/asset" <<'PY'
import json
import sys

path, version, architecture = sys.argv[1:]
with open(path) as stream:
    release = json.load(stream)
assets = release.get("assets", [])
expected = f"T3-Code-Bar-{version}-linux-{architecture}.AppImage"
base = f"https://github.com/eliophan/t3code-bar/releases/download/v{version}/"
if release.get("tag_name") != f"v{version}" or release.get("draft"):
    sys.exit("Unexpected release identity.")
for name in (expected, expected + ".sha256"):
    matches = [a for a in assets if a.get("name") == name and a.get("state") == "uploaded"]
    if len(matches) != 1 or not matches[0].get("browser_download_url", "").startswith(base):
        sys.exit(f"Linux asset is not published: {name}")
    print(matches[0]["browser_download_url"])
PY
mapfile -t urls < "$work/asset"
fetch "${urls[0]}" "$work/app.AppImage" || fail 'Could not download the AppImage.'
fetch "${urls[1]}" "$work/app.sha256" || fail 'Could not download its checksum.'
read -r expected filename < "$work/app.sha256"
[[ "$expected" =~ ^[0-9a-fA-F]{64}$ && "$filename" == "T3-Code-Bar-${version}-linux-${architecture}.AppImage" ]] || fail 'Invalid checksum file.'
actual="$(sha256sum "$work/app.AppImage")"
[[ "${actual%% *}" == "${expected,,}" ]] || fail 'Checksum mismatch; nothing installed.'

data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
[[ "$data_home" == /* && "$data_home" != *$'\n'* && "$data_home" != *'"'* && "$data_home" != *'`'* && "$data_home" != *'$'* && "$data_home" != *'\'* && "$data_home" != *'%'* ]] || fail 'XDG_DATA_HOME must be an absolute path without desktop launcher escape characters.'
app_dir="$data_home/t3-code-bar"
bin_dir="$HOME/.local/bin"
launcher="$data_home/applications/t3-code-bar.desktop"
# Refuse to replace existing files, including symlinks.
for path in "$app_dir" "$bin_dir/t3-code-bar" "$launcher"; do
  [[ ! -e "$path" && ! -L "$path" ]] || fail "Already exists: $path. Quit and remove the existing installation before reinstalling."
done
mkdir -p "$app_dir" "$bin_dir" "$data_home/applications"
install -m 755 "$work/app.AppImage" "$app_dir/T3-Code-Bar.AppImage"
ln -s "$app_dir/T3-Code-Bar.AppImage" "$bin_dir/t3-code-bar"
cat > "$launcher" <<DESKTOP
[Desktop Entry]
Type=Application
Name=T3 Code Bar
Comment=T3 Code inbox and usage
Exec="$app_dir/T3-Code-Bar.AppImage"
Terminal=false
Categories=Utility;
DESKTOP
printf 'Installed T3 Code Bar %s. Open T3 Code Bar from your app launcher, or run:\n  "%s/t3-code-bar"\n' "$version" "$bin_dir"
printf '%s\n' 'Requires WebKitGTK 4.1, GTK 3, a StatusNotifier tray host, and an unlocked Secret Service.' \
  'If FUSE is unavailable, run: APPIMAGE_EXTRACT_AND_RUN=1 ~/.local/bin/t3-code-bar' \
  'Start T3 Code and enter a pairing key in the Bar.'
