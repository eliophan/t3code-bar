# T3 Code Bar desktop

Production Tauri v2 application for macOS and Linux. See the root [README](../README.md) for setup and builds, [development guide](../docs/DEVELOPMENT.md) for checks, and [release guide](RELEASE.md) for Linux distribution, macOS packaging, and updater signing.

The app uses Rust domains for threads, Usage, Limits, settings and update policy, with React presentation and native tray/panel adapters. IPC accepts typed actions; UI snapshots never contain pairing credentials. T3 Code owns the `t3code://` scheme and its database is read-only.

Synthetic fixtures live in `tests/fixtures`. Personal state, credentials, acceptance captures and internal plans do not belong in the public source tree.
