# AGENTS.md

Guidance for coding agents working in this repository. `CLAUDE.md` imports this file; edit this one.

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

Agent skills live in `.agents/skills/`. Use them from there; do not create
copies under `.claude/skills/` or another agent's directory.

- `verify/`: drive the real Bar app through pairing, threads, usage and settings.
- `verify-t3-code-bar/`: native Linux UI checks with isolated synthetic state.
- `verify-linux-tray/`: model-check the Linux tray lifecycle against source.

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

## T3 Code boundary

Bar is an independent companion. Every change must work with a released, unmodified T3 Code.

- Do not change T3 Code, and do not build a feature that needs a new T3 Code endpoint, field, route, scheme or setting. Do not open upstream changes as a prerequisite for a Bar change.
- Use only what T3 Code already ships: its database and caches (read-only), the server descriptor, pairing and read-scoped grants, the orchestration and shell RPCs, the opt-in desktop catalog, and `t3code://threads/{environment}/{thread}` links. Name the T3 Code version or commit that provides the behavior you rely on.
- Internal renderer routes, unexported source and unreleased builds are not supported interfaces. Reading upstream source can identify behavior; it does not establish support.
- Bar reads; T3 Code acts. Do not add controls that create, edit, run or delete T3 Code threads, tasks or settings, or that write to its state.
- When T3 Code does not expose what a feature needs, ship the part that works and document the limitation. Do not invent deep links, guess at schemas, or silently fall back to another behavior. Missing or changed upstream data must produce an explicit unavailable state, not an empty or wrong result.

## Boundaries

- T3 Code owns its database and the `t3code://` scheme. Read its database and caches read-only; do not modify its state or register its scheme.
- Pair through the app's current pairing flow. Do not import retired Swift settings or tokens, read unrelated credentials, or expose credentials in UI snapshots or logs.
- Keep transport destination validation and credential access in the host/domain layer. Read `docs/COMPATIBILITY.md` before changing data or protocol behavior.
- Hover previews are enabled by default and wait 500 ms. Preserve cancellation when the row, panel or preference changes.
- Tests use synthetic state and disposable credential namespaces, never personal databases, real pairing keys or accounts.
- Preserve unrelated worktree changes. Keep internal plans, acceptance captures, local references, build artifacts and signing secrets out of Git.
- Local checks do not establish packaged native acceptance. Verify tray, pairing/reconnect/disconnect, notifications and settings on the actual package; include Spaces/fullscreen on macOS and tray-host loss on Linux.
- Release signing, notarization and updater installation are separate gates. Do not publish, push or install a release unless explicitly requested.

## Contributing

Follow `CONTRIBUTING.md`: branch from current `main`, add regression coverage, update the Unreleased section of `CHANGELOG.md` for user-visible changes, and fill in `.github/pull_request_template.md`. Report security problems through private vulnerability reporting (`SECURITY.md`), not public issues. Large features and compatibility changes start as an issue; scope decisions belong to the maintainer.
