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

From v1.0.2 the app updates itself. It checks GitHub for a new release about 10 seconds after launch and every 6 hours. A release counts only when it carries the macOS zip, `T3-Code-Bar-<version>-universal.zip`. Older installs run the command above once to get the updater.

By default the app asks before it installs. When an automatic check finds a newer release, a window offers **Install and Restart** or **Later**. Return chooses **Later**, and Escape does not close the window. Choosing **Install and Restart** quits the app, swaps it, and reopens it. The window appears once per version for each launch.

The right-click menu on the menu bar icon shows the running version and a status line, such as "Up to date, checked 14:05" or "Version 1.2.3 is available". Below them is one button.

- **Check for Updates…** checks now and never installs by itself.
- **Install v1.2.3 and Restart** appears after a check finds a release and installs it.
- **Update Automatically** turns the launch and 6 hour checks on or off. It is on by default.
- **Install Without Asking** skips the window and installs a release as soon as an automatic check finds it. It is off by default.

Updates run only for release builds at `~/Applications/T3 Code Bar.app`. Builds from source and copies in other folders show a status line that says why updates are off. For other status lines, see [Troubleshooting](docs/USAGE.md#troubleshooting).

Apple Silicon is the tested architecture. Releases are universal and include an untested Intel build. The app is ad hoc signed and not notarized. To build from source, see the [development guide](docs/DEVELOPMENT.md).

## Features

- View your local T3 Code inbox without switching windows.
- See Working, Approval, Input, Failed, and other thread states.
- Spot unread completions with **Done**; read threads show a relative timestamp.
- Keep pinned threads and manual inbox ordering.
- Bring snoozed threads back when something needs attention.
- Open threads in T3 Code or your browser, or copy their links.
- See what T3 Code's Usage page shows, in three tabs. Cost and Tokens cover the past 24 hours, 7, 30 or 90 days with a chart, providers, models and cache savings. Limits shows each provider's remaining quota, pace and reset times.

## Usage

Click the menu bar icon to view your inbox, and click a thread to open it in T3 Code. Right-click a thread for **Open in T3 Code**, **Open in Browser**, or **Copy Link**. Right-click the menu bar icon to quit.

See the [usage guide](docs/USAGE.md) for thread states, launching at login, custom data directories, troubleshooting, and known limitations. T3 Code Bar reads T3 Code's internal database schema and usage cache, which can change between versions. See the [compatibility notes](docs/COMPATIBILITY.md).

## Privacy

T3 Code Bar collects no telemetry and never calls AI services or provider APIs. It opens T3 Code's SQLite database read-only and never modifies it. A required Limits connection uses a separately created read-only pairing code to read provider and CLI Proxy snapshots from `127.0.0.1`. Its Bar access token stays in memory unless you select **Remember Bar token in Keychain**. The other network request is the update check against GitHub's releases API, which **Update Automatically** controls. It is a conditional request, so GitHub answers "not modified" when nothing changed. Installing an update downloads the install script and the release zip from GitHub. The update log stays on your machine at `~/Library/Logs/T3 Code Bar/update.log`. It holds update events, exit codes, and the installer's output, and the app never uploads it. When sharing screenshots or bug reports, check for private thread names, project names, and paths.

## Contributing

See the [development guide](docs/DEVELOPMENT.md). To report a bug, [open an issue](https://github.com/eliophan/t3code-bar/issues) with your macOS version, T3 Code version, and steps to reproduce. Redact private information.

## License

[MIT](LICENSE). T3 Code-derived logic and artwork, and adapted Lucide icon geometry, are documented in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). T3 Code, OpenAI, Anthropic, Google, and xAI names and logos belong to their respective owners.
