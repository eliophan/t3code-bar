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

Requires macOS 14+, Swift 6, and T3 Code opened at least once. In Terminal, from a directory that does not already contain `t3code-bar`:

```bash
git clone https://github.com/eliophan/t3code-bar.git && bash t3code-bar/scripts/install.sh
```

This builds the app on your Mac, installs it at `~/Applications/T3 Code Bar.app`, and opens it. No `sudo` is needed. Click the menu bar icon to see your inbox. [Details below](#install).

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
- **Git and Swift 6.0 or later**, available through a compatible Xcode or Xcode Command Line Tools installation.
- **T3 Code installed and opened at least once**, with a compatible local database.

**Apple Silicon (`arm64`) is the tested architecture.** Intel (`x86_64`) is untested. Builds target your local toolchain's architecture; the scripts do not create a universal binary.

T3 Code Bar reads T3 Code's internal projection-v2 schema, which can change between versions, so compatibility with every T3 Code version is not guaranteed. The checked versions are listed in the [compatibility notes](docs/COMPATIBILITY.md).

## Install

The [quick start](#quick-start) command downloads the source, builds a Release version, installs it, and opens it. If you already downloaded or cloned the source, run this from the project directory instead:

```bash
bash scripts/install.sh
```

The installer stops if an app already exists at the destination. To reinstall, quit the app and move the old copy aside first. There is no automatic updater. After installation, you can remove the source directory; keep the installed app in `~/Applications`.

**Distribution status:** there is no prebuilt `.app` download. The locally built app is ad hoc signed, has no Developer ID signature, and is not notarized. The scripts do not perform Developer ID signing or notarization.

## Usage

1. Open T3 Code, then launch T3 Code Bar.
2. Click the menu bar icon to view your inbox.
3. Click a thread to open it in T3 Code. Right-click a thread for **Open in T3 Code**, **Open in Browser**, or **Copy Link**.
4. To quit, right-click the menu bar icon and choose **Quit T3 Code Bar**.

See the [usage guide](docs/USAGE.md) for thread states, launching at login, custom data directories, troubleshooting, and known limitations.

## Privacy

T3 Code Bar reads local data and does not require an API key.

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
