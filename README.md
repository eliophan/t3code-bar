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

macOS 14+ · Apple Silicon

```sh
curl -fsSL https://eliophan.github.io/t3code-bar/install.sh | bash
```

Or install with Homebrew:

```sh
brew install --cask eliophan/tap/t3-code-bar
```

[Download ZIP](https://github.com/eliophan/t3code-bar/releases/download/v2.0.0-alpha.3/T3-Code-Bar-2.0.0-alpha.3-darwin-aarch64.app.zip) · Unzip and move the app to Applications.

The app is not notarized. If blocked, try opening it, then use **System Settings → Privacy & Security → Open Anyway**. [Homebrew help](desktop/RELEASE.md#homebrew-macos).

### Linux

x86_64 · AppImage or Debian package

```sh
curl -fsSL https://eliophan.github.io/t3code-bar/install-linux.sh | bash
```

The installer adds the AppImage to your app launcher. Requires curl, Python 3, and `sha256sum`.

[Download AppImage](https://github.com/eliophan/t3code-bar/releases/download/v2.0.0-alpha.3/T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.AppImage) · [Download .deb](https://github.com/eliophan/t3code-bar/releases/download/v2.0.0-alpha.3/T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.deb)

Requires a system tray and an unlocked Secret Service for saved pairing. On GNOME, enable an AppIndicator extension. [Linux setup and troubleshooting](docs/USAGE.md#linux-installation).

## Get started

1. Open T3 Code and generate a pairing key.
2. Launch T3 Code Bar and paste the key.
3. Click the menu bar or tray icon to view your threads.

This is an alpha. Updates use manual installation.

## Privacy

Reads local T3 Code data without modifying it. No telemetry or direct provider API calls. Pairing credentials stay in your OS credential store.

[Usage](docs/USAGE.md) · [Development](docs/DEVELOPMENT.md) · [Compatibility](docs/COMPATIBILITY.md) · [Release guide](desktop/RELEASE.md)

[MIT](LICENSE) · [Third-party notices](THIRD_PARTY_NOTICES.md). Independent companion, not affiliated with T3 Tools.

## Contributing and support

Read [Contributing](CONTRIBUTING.md) for setup and PR guidance, [Support](docs/SUPPORT.md) for product scope, and [Changelog](CHANGELOG.md) for release changes. Community participation follows the [Code of conduct](CODE_OF_CONDUCT.md). Report vulnerabilities through the private channel in [Security](SECURITY.md).
