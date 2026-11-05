# T3 Code Bar

[![Swift checks](https://github.com/eliophan/t3code-bar/actions/workflows/ci.yml/badge.svg)](https://github.com/eliophan/t3code-bar/actions/workflows/ci.yml)
![macOS 14+](https://img.shields.io/badge/macOS-14%2B-blue)
![Swift 6](https://img.shields.io/badge/Swift-6.0-orange)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Keep your [T3 Code](https://github.com/pingdotgg/t3code) inbox in the macOS menu bar. Check running threads, spot requests that need your attention, and jump back into a conversation with one click.

Built with SwiftUI. An independent companion app, not affiliated with or endorsed by T3 Tools.

![T3 Code Bar on macOS](docs/images/screenshot.png)

*Some status labels in the screenshot may differ from the current version.*

![Opening a thread from the menu bar popover](docs/assets/demo.gif)

*Demo: opening the popover, hovering a thread, and jumping into T3 Code.*

## Quick start

Requires a Mac on macOS 14+ (Apple Silicon is the tested architecture) and T3 Code opened at least once. In Terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/eliophan/t3code-bar/main/scripts/install-release.sh | bash
```

This downloads the latest prebuilt release, checks its checksum, installs it at `~/Applications/T3 Code Bar.app`, and opens it. No Swift, build step, or `sudo` is needed. Click the menu bar icon to see your inbox.

**To update, run the same command again.** It installs a newer release if there is one, restarts the app, and keeps your current version if anything fails. [Details below](#install-and-update).

**From the first release that includes the updater, the app updates itself.** It checks for a new release at launch and every 6 hours, installs it with the same script, and reopens. Turn it off with **Update Automatically** in the right-click menu. Builds from source never update themselves. Older installs need to run the command above once.

## Features

- View your local T3 Code inbox without switching windows.
- See Working, Approval, Input, Failed, and other thread states.
- Identify unread completions with **Done**; read threads show a relative timestamp.
- Keep pinned threads and manual inbox ordering.
- Bring snoozed threads back when an approval, input request, fresh session error, or new completion needs attention.
- Open threads in T3 Code or your browser, or copy their links.
- Read local data without modifying T3 Code's database.

## Requirements

- **macOS 14 or later.**
- **T3 Code installed and opened at least once**, with a compatible local database.
- **Only to build from source:** Git and Swift 6.0 or later, available through a compatible Xcode or Xcode Command Line Tools installation.

**Apple Silicon (`arm64`) is the tested architecture.** Prebuilt releases are universal and include an Intel (`x86_64`) build, but Intel has not been tested. Releases before v1.0.2 are `arm64` only. Builds from source target your local toolchain's architecture; the source scripts do not create a universal binary.

T3 Code Bar reads T3 Code's internal projection-v2 schema, which can change between versions, so compatibility with every T3 Code version is not guaranteed. The checked versions are listed in the [compatibility notes](docs/COMPATIBILITY.md).

## Install and update

### From a prebuilt release

The [quick start](#quick-start) command installs the latest [release](https://github.com/eliophan/t3code-bar/releases). Run it again at any time to update. If the app is running, the script quits it, swaps in the new version, and reopens it. If the download, checksum, or swap fails, your installed version stays in place.

Options go after `bash -s --`:

```bash
# Install a specific version, including an older one
curl -fsSL https://raw.githubusercontent.com/eliophan/t3code-bar/main/scripts/install-release.sh | bash -s -- --version v1.2.3

# Reinstall even when already up to date
curl -fsSL https://raw.githubusercontent.com/eliophan/t3code-bar/main/scripts/install-release.sh | bash -s -- --force
```

There is no in-app updater, and the app never checks for updates on its own. Only this script contacts GitHub, and only when you run it.

### From source

If you prefer to build it yourself, you need Git and Swift 6 or later. Run this from a directory that does not already contain `t3code-bar`:

```bash
git clone --depth 1 https://github.com/eliophan/t3code-bar.git && bash t3code-bar/scripts/install.sh
```

If you already downloaded or cloned the source, run `bash scripts/install.sh` from the project directory. The source installer stops if an app already exists at the destination, so to reinstall, quit the app and move the old copy aside first. After installing, you can remove the source directory.

**Distribution status:** prebuilt releases and local builds are ad hoc signed, have no Developer ID signature, and are not notarized. The scripts do not perform Developer ID signing or notarization. See [known limitations](docs/USAGE.md#known-limitations).

## Usage

1. Open T3 Code, then launch T3 Code Bar.
2. Click the menu bar icon to view your inbox.
3. Click a thread to open it in T3 Code. Right-click a thread for **Open in T3 Code**, **Open in Browser**, or **Copy Link**.
4. To quit, right-click the menu bar icon and choose **Quit T3 Code Bar**.

See the [usage guide](docs/USAGE.md) for thread states, launching at login, custom data directories, troubleshooting, and known limitations.

## Privacy

T3 Code Bar reads local data and does not require an API key. The app itself makes no network requests; only the optional [install script](#from-a-prebuilt-release) downloads a release from GitHub when you run it.

- Opens T3 Code's SQLite database in **read-only** mode to read thread, project, run, request, and visit metadata.
- Reads `server-runtime.json` and `environment-id` to identify the local instance and construct links.
- May read project favicon/logo files from local workspace paths.
- Does not modify or migrate the database, answer approvals, or send messages on your behalf.
- Does not collect telemetry, access provider credential stores, or call AI services.

When you open a thread, macOS hands the URL to T3 Code or your browser. That application handles its own connections and authentication. When sharing screenshots or bug reports, check for private thread names, project names, and paths.

## Contributing

Run the checks before opening a pull request:

```bash
swift test
swift build -c release
bash -n scripts/build_app.sh scripts/install.sh
```

The package has no external Swift dependencies. For running from source, packaging a local app, and the pre-push hook, see the [development guide](docs/DEVELOPMENT.md). To report a bug, [open an issue](https://github.com/eliophan/t3code-bar/issues) with your macOS version, architecture, T3 Code version, and steps to reproduce; redact private information and do not attach your database or credentials.

## License and acknowledgments

Project source is licensed under [MIT](LICENSE). T3 Code-derived logic and artwork, and adapted Lucide icon geometry, are documented in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

T3 Code, OpenAI, Anthropic, Google, and xAI names and logos belong to their respective owners. Their use does not imply endorsement or an official relationship.
