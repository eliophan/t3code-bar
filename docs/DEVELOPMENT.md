# Development

The application lives in `desktop/`. Swift v1 is retired; its source remains in Git history.

## Run from source

Install Node.js 22+, Rust, and the native [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). macOS requires version 14+ and Xcode command line tools. Rust is pinned in `desktop/rust-toolchain.toml`.

```sh
git clone https://github.com/eliophan/t3code-bar.git
cd t3code-bar/desktop
npm ci
npm run tauri -- dev
```

## Build

From `desktop/`:

```sh
# macOS
npm run tauri -- build --bundles app

# Linux
npm run tauri -- build --config src-tauri/tauri.linux.conf.json
```

A nonzero exit means the build failed. Packages are written under `desktop/src-tauri/target/release/bundle/`, unless `CARGO_TARGET_DIR` is configured. See the [release guide](../desktop/RELEASE.md) for distribution packages.

## Structure

- `src/`: React views, presentation models and frontend tests.
- `src-tauri/`: Tauri IPC, native panel/tray adapters, notifications and updater integration.
- `crates/bar-identity`: environment and thread identity types, and the exported TypeScript contracts.
- `crates/bar-connection`: remote environment discovery, routes, pairing credentials and transports.
- `crates/bar-threads`: read-only thread data and previews.
- `crates/bar-usage`: usage cache and local-provider readers.
- `crates/bar-limits`: cached/live Limits and OS credential storage.
- `crates/bar-core`: aggregate domain state and settings.
- `crates/bar-updater`: update policy and signature helpers.
- `tests/fixtures`: synthetic data and frozen expected outputs used by Rust tests.
- `vendor/notify-rust`: narrowly patched notification backend; see its `PATCH.md`.

## Checks

Run from the repository root:

```sh
bash scripts/check.sh
```

The script checks the install scripts, the frontend tests, that the generated TypeScript contracts match `bar-identity` (`npm run check:contracts`), the production frontend build, all seven Rust crates and the Tauri host using the pinned toolchain. Native Tauri dependencies must be installed even for host tests. Tests use synthetic state and disposable credential namespaces; do not substitute personal database or credential files.

Run a development app from `desktop/`:

```sh
npm ci
npm run tauri -- dev
```

To install the local pre-push check, run `bash scripts/setup_hooks.sh`. Build outputs, private planning, acceptance receipts, credentials and environment files are excluded from Git.

## Native verification

Automated tests do not establish native GUI or release acceptance. Test the actual packaged app for tray actions, panel visibility, hover, pairing, relaunch/reconnect, disconnect, settings persistence and notification routing. On macOS include Spaces and fullscreen; on Linux include tray loss and compositor-specific behavior. Keep private captures and receipts locally.
