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
- `orchestration_v2_projection_provider_threads`
- `orchestration_v2_projection_messages`
- `projection_projects`
- `projection_thread_pull_requests`

Optional: `orchestration_v2_projection_turn_items` is read to tell a usage-limit failure (Limited, with its reset time) from other failures, and to tell a run that is only waiting on other threads (Waiting) from one that is working. If it is missing, threads still list, a usage-limit failure shows as Failed, and a thread wait shows as Working. Guest messages (Request) come from `guestShare` in the thread payload.

Optional: `orchestration_v2_projection_provider_turns` feeds the Usage screen. If it is missing, the Usage screen shows a notice and the thread list is not affected.

Inbox properties such as `settledAt`, `snoozedAt`, `snoozedUntil`, `pinnedAt`, `pinOrderKey`, `activeOrderKey`, `lineage`, and `lastVisitedAt` are read from JSON payloads. The app does not change the schema, create indexes, or migrate T3 Code data. If SQL preparation fails because a table or column is missing, the app reports an incompatibility instead of showing an empty inbox.

SQLite is opened read-only. The app does not use `immutable=1`, so changes in the WAL remain visible. Each poll opens and closes its connection and waits up to 250 ms for a lock. Errors while stepping through results are never treated as partial success.

## Snooze and Woke

Snooze is classified after the row is read; a thread is not filtered in SQL solely because its scheduled time is in the future. Pending approval/input always outranks snooze. A completion after `snoozedAt` or a fresh session error also wakes the thread early. The session is read from the provider-thread payload with the highest `last_run_ordinal` (`status` and `updatedAt`); this is the local projection-v2 mapping for the session data used by the reference implementation. The app does not infer a new error from `thread.updated_at`.

Woke uses the completion timestamp for an early completion wake, the session/snooze event timestamp for another attention event, or the scheduled time for a timer wake. It compares that timestamp with persisted `lastVisitedAt` to determine whether the wake is still unread. Invalid snooze dates never hide a thread. Bar does not change snooze or visit fields and does not call the sidebar's separate acknowledgment API; it reflects persisted data.

## Token usage

The Usage screen reads one row per provider turn from `orchestration_v2_projection_provider_turns` with a single aggregate query.

- `started_at` is ISO-8601 with milliseconds and a `Z`, so it is compared as text against a cutoff. The periods are rolling windows of 1, 7, 30, and 90 days back from now. A turn without `started_at` belongs to no period.
- `payload_json.nativeTurnRef.driver` names the provider. It maps to a provider the same way thread rows do, and an unknown or missing driver counts as Other.
- `payload_json.turnTokenUsage` holds the per-turn totals `inputTokens`, `cachedInputTokens`, `cacheCreationTokens`, `outputTokens`, and `usageStatus`. The sibling `tokenUsage` object is a context-window gauge and is ignored.

`inputTokens` already includes cached reads and cache writes. Checked against about 7,000 turns from five drivers, `cachedInputTokens` never exceeded `inputTokens`. For Claude, `inputTokens` minus `cachedInputTokens` matched `cacheCreationTokens` within 0.003 percent. Three numbers follow from that, and they are T3 Code's own.

- Uncached input is `max(0, input - cached - cacheCreation)`.
- Processed tokens is `input + output`.
- Cache hit is `cached / (uncachedInput + cached + cacheCreation)`, and is not shown when that denominator is 0.

A turn counts as having usage when `inputTokens` is present. Other turns are left out of the sums and counted as turns with no usage. That covers turns with no `turnTokenUsage`, turns with `usageStatus` `unavailable`, and payloads without token fields. A turn with `usageStatus` `partial` carries real but incomplete counts. Bar keeps it in the sums, so totals can read slightly low, and mentions it in a footnote.

Reasoning tokens are not shown on their own. T3 Code counts them inside output.

The query runs about 30 ms on an 8.6 GB database once the pages are cached, and about half a second the first time. It reads only while the Usage screen is open.

## What SQLite cannot provide

Cost needs the model of each turn, which only part of the turns record. Limits are not stored in SQLite. Per-model breakdowns are not available for the same reason as cost. The Usage screen therefore shows the Tokens view of T3 Code's Usage page and nothing else from it.

The reference `ThreadBackgroundLivenessService` uses an in-memory registry rather than persistence. It distinguishes an active agent/workflow from a watch loop. T3 Code Bar does not connect to that runtime service and does not infer current work from old task history.

The reference client uses `lastVisitedAt` in client state. The installed nightly also persists this field in `orchestration_v2_projection_threads.payload_json`, so Bar reads it directly. Done appears only when the latest visible run has status `completed` and a valid `completedAt > lastVisitedAt`. After a visit, or when the visit timestamp is missing/invalid, the row shows time. When T3 Code persists a newer visit, Bar updates on its next poll; clicking a link in Bar does not write the visit timestamp. Versions that keep visits only in browser localStorage cannot provide this Done indicator. T3 Code Bar retains its own Goal, Failed, and Woke labels and does not claim complete sidebar parity.

Data is read from one fixed local instance. The app does not merge remote environments, discover every local instance, or access tokens or credential stores.
