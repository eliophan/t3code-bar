# T3 Code Bar

[![Swift checks](https://github.com/eliophan/t3code-bar/actions/workflows/ci.yml/badge.svg)](https://github.com/eliophan/t3code-bar/actions/workflows/ci.yml)
![macOS 14+](https://img.shields.io/badge/macOS-14%2B-blue)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Keep your [T3 Code](https://github.com/pingdotgg/t3code) inbox in the macOS menu bar. Check running threads, spot requests that need your attention, and jump back into a conversation with one click.

An independent companion app built with SwiftUI, not affiliated with or endorsed by T3 Tools.

![T3 Code Bar on macOS](docs/images/screenshot.png)

*Some status labels in the screenshot may differ from the current version.*

![Opening a thread from the menu bar popover](docs/assets/demo.gif)

## Installation

Requires macOS 14 or later and T3 Code opened at least once. In Terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/eliophan/t3code-bar/main/scripts/install-release.sh | bash
```

This downloads the latest release, checks its checksum, installs it at `~/Applications/T3 Code Bar.app`, and opens it. No Swift, build step, or `sudo` is needed.

From v1.0.2 the app updates itself. It checks for a new release at launch and every 6 hours, then installs it and reopens. Turn this off with **Update Automatically** in the right-click menu. Older installs run the command above once to get the updater.

Apple Silicon is the tested architecture. Releases are universal and include an untested Intel build. The app is ad hoc signed and not notarized. To build from source, see the [development guide](docs/DEVELOPMENT.md).

## Features

- View your local T3 Code inbox without switching windows.
- See Working, Approval, Input, Failed, and other thread states.
- Spot unread completions with **Done**; read threads show a relative timestamp.
- Keep pinned threads and manual inbox ordering.
- Bring snoozed threads back when something needs attention.
- Open threads in T3 Code or your browser, or copy their links.
- See your token usage by type and provider for the past day, week, month, or quarter.

## Usage

Click the menu bar icon to view your inbox, and click a thread to open it in T3 Code. Right-click a thread for **Open in T3 Code**, **Open in Browser**, or **Copy Link**. Right-click the menu bar icon to quit.

See the [usage guide](docs/USAGE.md) for thread states, launching at login, custom data directories, troubleshooting, and known limitations. T3 Code Bar reads T3 Code's internal database schema, which can change between versions. See the [compatibility notes](docs/COMPATIBILITY.md).

## Privacy

T3 Code Bar needs no API key, collects no telemetry, and never calls AI services. It opens T3 Code's SQLite database read-only and never modifies it. The only network request is the update check against GitHub's releases API, which **Update Automatically** controls. When sharing screenshots or bug reports, check for private thread names, project names, and paths.

## Contributing

See the [development guide](docs/DEVELOPMENT.md). To report a bug, [open an issue](https://github.com/eliophan/t3code-bar/issues) with your macOS version, T3 Code version, and steps to reproduce. Redact private information.

## License

[MIT](LICENSE). T3 Code-derived logic and artwork, and adapted Lucide icon geometry, are documented in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). T3 Code, OpenAI, Anthropic, Google, and xAI names and logos belong to their respective owners.
