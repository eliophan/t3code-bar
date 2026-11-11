# CLAUDE.md

Guidance for coding agents working in this repository.

## Project

T3 Code Bar is a Tauri v2 desktop companion for T3 Code on macOS and Linux. The frontend uses React and TypeScript; the host and domains use Rust. Windows is deferred. The retired Swift app is not the current implementation.

## Commands

Run from the repository root:

```sh
bash scripts/check.sh       # frontend tests/build, contract check, seven Rust crates and Tauri host tests
bash scripts/setup_hooks.sh # enable the local pre-push checks
```

Run from `desktop/`:

```sh
npm ci
npm run tauri -- dev
npm test
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run tauri -- build --bundles app # macOS
npm run tauri -- build --config src-tauri/tauri.linux.conf.json # Linux
```

The project verification skill lives at `.agents/skills/verify/`. Use that
directory directly from Codex or Claude Code; do not create a second copy under
`.claude/skills/`.

Rust is pinned by `desktop/rust-toolchain.toml`. Native Tauri prerequisites are required for host tests and builds. See `docs/DEVELOPMENT.md` and `desktop/RELEASE.md`.

## Architecture

- `desktop/src/`: React UI, presentation models and frontend tests.
- `desktop/src-tauri/src/`: typed IPC, native panel/tray adapters, notifications and updater integration.
- `desktop/crates/bar-identity`: environment/thread identity types and exported TypeScript contracts.
- `desktop/crates/bar-connection`: remote environment discovery, routes, pairing credentials and transports.
- `desktop/crates/bar-threads`: read-only thread data and hover previews.
- `desktop/crates/bar-usage`: usage caches, pricing and local-provider readers.
- `desktop/crates/bar-limits`: live/cache Limits, pairing and OS credential storage.
- `desktop/crates/bar-core`: aggregate domain state and persistent settings.
- `desktop/crates/bar-updater`: update policy and signature helpers.
- `desktop/tests/fixtures/`: synthetic fixtures and expected outputs.
- `desktop/vendor/notify-rust/`: patched notification dependency; retain its provenance and `PATCH.md`.

## Boundaries

- T3 Code owns its database and the `t3code://` scheme. Read its database and caches read-only; do not modify its state or register its scheme.
- Pair through the app's current pairing flow. Do not import retired Swift settings or tokens, read unrelated credentials, or expose credentials in UI snapshots or logs.
- Keep transport destination validation and credential access in the host/domain layer. Read `docs/COMPATIBILITY.md` before changing data or protocol behavior.
- Hover previews are enabled by default and wait 500 ms. Preserve cancellation when the row, panel or preference changes.
- Tests use synthetic state and disposable credential namespaces, never personal databases, real pairing keys or accounts.
- Preserve unrelated worktree changes. Keep internal plans, acceptance captures, local references, build artifacts and signing secrets out of Git.
- Local checks do not establish packaged native acceptance. Verify tray, pairing/reconnect/disconnect, notifications and settings on the actual package; include Spaces/fullscreen on macOS and tray-host loss on Linux.
- Release signing, notarization and updater installation are separate gates. Do not publish, push or install a release unless explicitly requested.
