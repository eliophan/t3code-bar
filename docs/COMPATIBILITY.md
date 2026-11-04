# T3 Code compatibility

Checked on 2026-11-04.

- Reference repository: `pingdotgg/t3code`, revision `21a3669cee0c6fb54f5afe065cf0eb0348bd232c`.
- Main references: `apps/web/src/components/Sidebar.logic.ts`, `packages/client-runtime/src/state/threadSort.ts`, `packages/client-runtime/src/state/threadSettled.ts`, `apps/server/src/orchestration/ThreadBackgroundLiveness.ts`, and `apps/server/src/config.ts`.
- Local runtime metadata checked: `0.1.7-nightly.20261104.4151`.
- The installed runtime and reference source are not treated as the same revision. Projection-v2 schema was verified directly against the local database; this does not certify compatibility with every T3 Code build.

## SQLite contract

The app requires these tables and queries them from `Sources/Data/DatabaseManager.swift`:

- `orchestration_v2_projection_threads`
- `orchestration_v2_projection_runs`
- `orchestration_v2_projection_runtime_requests`
- `orchestration_v2_projection_plans`
- `orchestration_v2_projection_provider_threads`
- `orchestration_v2_projection_messages`
- `projection_projects`
- `projection_thread_pull_requests`

Optional: `orchestration_v2_projection_turn_items` is read to tell a usage-limit failure (Limited, with its reset time) from other failures, and to tell a run that is only waiting on other threads (Waiting) from one that is working. If it is missing, threads still list, a usage-limit failure shows as Failed, and a thread wait shows as Working. Guest messages (Request) come from `guestShare` in the thread payload.

Inbox properties such as `settledAt`, `snoozedAt`, `snoozedUntil`, `pinnedAt`, `pinOrderKey`, `activeOrderKey`, `lineage`, and `lastVisitedAt` are read from JSON payloads. The app does not change the schema, create indexes, or migrate T3 Code data. If SQL preparation fails because a table or column is missing, the app reports an incompatibility instead of showing an empty inbox.

SQLite is opened read-only. The app does not use `immutable=1`, so changes in the WAL remain visible. Each poll opens and closes its connection and waits up to 250 ms for a lock. Errors while stepping through results are never treated as partial success.

## Snooze and Woke

Snooze is classified after the row is read; a thread is not filtered in SQL solely because its scheduled time is in the future. Pending approval/input always outranks snooze. A completion after `snoozedAt` or a fresh session error also wakes the thread early. The session is read from the provider-thread payload with the highest `last_run_ordinal` (`status` and `updatedAt`); this is the local projection-v2 mapping for the session data used by the reference implementation. The app does not infer a new error from `thread.updated_at`.

Woke uses the completion timestamp for an early completion wake, the session/snooze event timestamp for another attention event, or the scheduled time for a timer wake. It compares that timestamp with persisted `lastVisitedAt` to determine whether the wake is still unread. Invalid snooze dates never hide a thread. Bar does not change snooze or visit fields and does not call the sidebar's separate acknowledgment API; it reflects persisted data.

## What SQLite cannot provide

The reference `ThreadBackgroundLivenessService` uses an in-memory registry rather than persistence. It distinguishes an active agent/workflow from a watch loop. T3 Code Bar does not connect to that runtime service and does not infer current work from old task history.

The reference client uses `lastVisitedAt` in client state. The installed nightly also persists this field in `orchestration_v2_projection_threads.payload_json`, so Bar reads it directly. Done appears only when the latest visible run has status `completed` and a valid `completedAt > lastVisitedAt`. After a visit, or when the visit timestamp is missing/invalid, the row shows time. When T3 Code persists a newer visit, Bar updates on its next poll; clicking a link in Bar does not write the visit timestamp. Versions that keep visits only in browser localStorage cannot provide this Done indicator. T3 Code Bar retains its own Goal, Failed, and Woke labels and does not claim complete sidebar parity.

Data is read from one fixed local instance. The app does not merge remote environments, discover every local instance, or access tokens or credential stores.

## Architecture and distribution

- macOS deployment target: 14.0.
- Locally tested architecture: arm64.
- x86_64 and universal binaries: not tested.
- Local Release binary: linker-signed ad hoc, without Developer ID, and not notarized.
- Any `.app` under `build/` is local output, may be older than the source, is not a release artifact, and is excluded from Git.

A successful build does not replace user-acceptance testing, Gatekeeper testing, or validation on another Mac.
