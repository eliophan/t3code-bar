# bar-limits

This crate provides the read-only Limits cache and the live T3 Code connection used by the desktop shell. It has no Tauri or UI dependency.

`LimitsReader` reads only the sibling `caches` directory under the T3 state directory. Keep one reader per state directory. A malformed file keeps its previous valid value until the file is deleted. `LimitsReader::merge_live` combines cache records with the last live records and returns the typed `LimitsSnapshot` used by the UI.

`LimitsConnection` owns a cancellable loopback session. Construct it with an injected `CredentialStore` and `LiveTransport` for tests, or use `KeyringStore::production()` with `LoopbackTransport` in the desktop shell. Call `restore_saved` once at startup to restore a valid Tauri credential without networking, including when T3 Code is offline. `connect`, `reconnect_saved`, `reconnect_saved_if_available`, `pause`, `resume`, and `disconnect` are async. `snapshot()` returns `LiveStatus`, a secret-free `hasCredential` flag, typed limits data, and `updatedAt`; it never contains the token. A transient socket failure moves to `Retrying` while retaining the last data. Expired or revoked credentials clear storage and become a permanent failure until a new token is supplied.

The default transport connects only to `127.0.0.1`, rejects redirects, limits ticket responses to 1 MiB, and enforces an 8 MiB WebSocket message limit. It uses the ticket endpoint `/api/auth/websocket-ticket/rpc` and the `/ws` Effect JSON RPC stream with protocol version 1 and request id 0.

Saved credential JSON retains reference-epoch seconds for `expiresAt` as its storage format. Tauri uses its own keyring namespace, `com.t3code.bar.tauri.limits` with account `pairing-key`; it never queries the Swift app's keychain item. A new installation therefore starts with manual pairing-key entry. Tests use only in-memory stores, temporary files, and loopback stubs.

Run the crate checks from the repository root:

```sh
cargo test --manifest-path desktop/crates/bar-limits/Cargo.toml
```

Real T3 account access, macOS Keychain state, Linux Secret Service state, packaged app workflows, and native UI acceptance remain separate gates.

`LiveLimitsParser` accepts synthetic or decoded stream events and exposes a `LimitsContribution` for an `EnvironmentId`. `LimitsReader::contribution` exposes cached reports after `read`; `live_contribution` exposes the current live/cache choice. These reports contain account email and credential fingerprints, so callers must treat them as private domain data. Pass contributions and an `EnvironmentSelection` to `merge_contributions` before rendering `LimitsSnapshot::groups`. Unidentified hub accounts are scoped to their environment and source, because upstream source IDs are not known to be globally unique. This intentionally differs from upstream when the same unidentified hub ID appears in two environments.

The account reconciliation and pool behavior derive from T3 Code's `packages/shared/src/usageLimits.ts`. T3 Code is distributed under the MIT license reproduced in `UPSTREAM-LICENSE`. The synthetic parity corpus is generated with `node scripts/generate-limits-parity.mjs` from the local upstream reference. The Rust integration test consumes that corpus.
