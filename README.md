# T3 Code Bar

[![Desktop checks](https://github.com/eliophan/t3code-bar/actions/workflows/ci.yml/badge.svg)](https://github.com/eliophan/t3code-bar/actions/workflows/ci.yml)
![macOS 14+](https://img.shields.io/badge/macOS-14%2B-blue)
![Linux x86_64](https://img.shields.io/badge/Linux-x86__64-blue)

Your [T3 Code](https://github.com/pingdotgg/t3code) threads, replies, and usage in the menu bar. Built for macOS and Linux.

<p align="center">
  <img src="docs/screenshots/macos-preview.png" width="350" alt="Thread list and last reply preview on macOS">
  <img src="docs/screenshots/macos-usage.png" width="408" alt="Usage charts and provider breakdown on macOS">
</p>

## Features

- See which threads need attention and open them in T3 Code.
- Hover over a thread to read the latest reply.
- Track estimated cost, tokens, and provider limits.
- Get desktop notifications and reconnect with saved pairing.

## Install

### macOS

```sh
brew install --cask eliophan/tap/t3-code-bar
```

Or [download for Apple Silicon](https://github.com/eliophan/t3code-bar/releases/download/v2.0.0-alpha.1/T3-Code-Bar-2.0.0-alpha.1-darwin-aarch64.app.zip), unzip, and move the app to Applications. Requires macOS 14+.

If Homebrew requests trust, run `brew trust --cask eliophan/tap/t3-code-bar` and retry.

The app is not notarized. If macOS blocks it, try opening it, then use **System Settings → Privacy & Security → Open Anyway**.

### Linux

```sh
curl -fsSL https://eliophan.github.io/t3code-bar/install-linux.sh | bash
```

Installs an AppImage and adds **T3 Code Bar** to your app launcher. Requires curl, Python 3, and `sha256sum`.

Prefer a download? [AppImage](https://github.com/eliophan/t3code-bar/releases/download/v2.0.0-alpha.1/T3-Code-Bar-2.0.0-alpha.1-linux-x86_64.AppImage) · [.deb](https://github.com/eliophan/t3code-bar/releases/download/v2.0.0-alpha.1/T3-Code-Bar-2.0.0-alpha.1-linux-x86_64.deb)

Linux x86_64, built on Ubuntu 24.04. Requires a system tray and an unlocked Secret Service for saved pairing. On GNOME, enable an AppIndicator extension. [Installation details and troubleshooting](docs/USAGE.md#linux-installation).

## Get started

1. Open T3 Code and generate a pairing key.
2. Launch T3 Code Bar and paste the key.
3. Click the menu bar or tray icon to view your threads.

This is an alpha. Updates use manual installation.

## Privacy

Reads local T3 Code data without modifying it. No telemetry or direct provider API calls. Pairing credentials stay in your OS credential store.

[Usage](docs/USAGE.md) · [Development](docs/DEVELOPMENT.md) · [Compatibility](docs/COMPATIBILITY.md) · [Release guide](desktop/RELEASE.md)

[MIT](LICENSE) · [Third-party notices](THIRD_PARTY_NOTICES.md). Independent companion, not affiliated with T3 Tools.
