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

Inbox properties such as `settledAt`, `snoozedAt`, `snoozedUntil`, `pinnedAt`, `pinOrderKey`, `activeOrderKey`, `lineage`, and `lastVisitedAt` are read from JSON payloads. The app does not change the schema, create indexes, or migrate T3 Code data. If SQL preparation fails because a table or column is missing, the app reports an incompatibility instead of showing an empty inbox.

SQLite is opened read-only. The app does not use `immutable=1`, so changes in the WAL remain visible. Each poll opens and closes its connection and waits up to 250 ms for a lock. Errors while stepping through results are never treated as partial success.

## Snooze and Woke

Snooze is classified after the row is read; a thread is not filtered in SQL solely because its scheduled time is in the future. Pending approval/input always outranks snooze. A completion after `snoozedAt` or a fresh session error also wakes the thread early. The session is read from the provider-thread payload with the highest `last_run_ordinal` (`status` and `updatedAt`); this is the local projection-v2 mapping for the session data used by the reference implementation. The app does not infer a new error from `thread.updated_at`.

Woke uses the completion timestamp for an early completion wake, the session/snooze event timestamp for another attention event, or the scheduled time for a timer wake. It compares that timestamp with persisted `lastVisitedAt` to determine whether the wake is still unread. Invalid snooze dates never hide a thread. Bar does not change snooze or visit fields and does not call the sidebar's separate acknowledgment API; it reflects persisted data.

## Usage data

The Usage screen no longer reads token counts from SQLite. It reads the files T3 Code's own Usage page reads, so the numbers can match that page.

Checked on 2026-11-06 against T3 Code Nightly `0.1.8-nightly.20261105.4205` (commit `935818e422f9`). Every file is opened read-only, and Bar makes no network call.

| File | What Bar takes from it |
| --- | --- |
| `usage-scan-cache-vN.json` | Per-transcript usage records for Claude, Codex, Grok, Pi and Muse. Bar reads the highest `N` whose `version` field is 4 to 11. |
| `usage-model-rates.json` | The price list T3 Code fetched. Without it, only provider-reported costs are priced. |
| `settings.json` | Two keys, `usagePriceOverrides` and `usageModelAliases`. The file also holds provider credentials, so Bar skips every other key without decoding it. |
| `statev2.sqlite` | The Codex service tier of turns the cache has not resolved yet. This reads `orchestration_events`, `orchestration_v2_projection_provider_turns` and `orchestration_v2_projection_run_attempts`. If the database is missing, those turns count at standard speed. |

### Freshness and coverage

- T3 Code writes the cache only when its Usage page scans. The numbers are as current as the last time that page was open, and the screen says so.
- OpenCode, Antigravity and Cursor are rescanned live by T3 Code and are never in the cache. This build does not show them.
- Remote and linked environments are not included. Bar reads this Mac only.
- T3 Code's per-user list of hidden providers lives in its renderer storage. Bar does not read it.

### Cache format

- Old `-vN` files stay on disk next to the current one. Bar picks by generation and by the `version` field, never by modification time. A newer unsupported format that is also newer on disk makes Bar show "T3 Code changed its usage format" instead of stale numbers.
- A file entry with any row that has fewer than 11 items, a non-number where a number belongs, an unknown model index or an unknown speed index is dropped whole, as T3 Code does. The screen warns when this happens.
- Claude rows for the model `<synthetic>` are dropped. A provider kind this build does not know is kept under its own name and counts in the totals.
- A cache that fails to parse is treated as being rewritten. Bar keeps the last good numbers and tries again.
- The file is 20 MB or more. Bar decodes it off the main actor in about 0.1 to 0.4 seconds, only when its size or modification time changes, and builds each period from the decoded records.

### Counting rules

These follow T3 Code's code and are checked against a port of it (`repro.py`) on a committed fixture and on a real cache.

- Past 24h ends at the current minute and starts 24 hours earlier. 7, 30 and 90 days are calendar days in the Mac's time zone, today included in full. Daylight saving changes are handled.
- The first record with a given key wins, in source order and then file order. Codex sessions that appear in more than one file, which forks and subagents cause, use a key built from the session, time, model and token counts.
- Processed tokens is uncached input plus cached input plus cache writes plus output. Reasoning is part of output and is not added again.
- A session counts once per source when one of its records was counted. A session in two sources counts twice, as in T3 Code.

### Cost rules

Cost is an API estimate from the price list. It is not what a plan bills.

- A model is priced from an exact, case-insensitive match on its name after cutting at the first `[`. There is no fuzzy matching. The bare names `opus`, `sonnet`, `haiku` and `fable` are never priced.
- A provider-reported cost replaces the list price and scales the cost split to it. A price override replaces both and switches off the reported cost for that model.
- Fast and ultrafast turns use their own prices when the list has them, and standard prices otherwise.
- Category costs, speed costs and the speed premium are rounded to a millionth of a dollar per day, hour slot, provider, model and source before they are added, because T3 Code rounds there.
- Cache savings are cached tokens times the gap between the input price and the cache read price.

### What can break

The cache is T3 Code's internal format. Bar checks the version and drops entries it cannot decode, but a new T3 Code can change the meaning of a field without changing the version. `Tests/UsageCanaryTests.swift` runs a recorded cache through the code and compares every number with the oracle, so a change in this code that moves a number fails the build. It cannot see a change on T3 Code's side.

## What the data cannot provide

Limits are not part of the scan cache. The Usage screen shows the Tokens view of T3 Code's Usage page and nothing else from it.

The reference `ThreadBackgroundLivenessService` uses an in-memory registry rather than persistence. It distinguishes an active agent/workflow from a watch loop. T3 Code Bar does not connect to that runtime service and does not infer current work from old task history.

The reference client uses `lastVisitedAt` in client state. The installed nightly also persists this field in `orchestration_v2_projection_threads.payload_json`, so Bar reads it directly. Done appears only when the latest visible run has status `completed` and a valid `completedAt > lastVisitedAt`. After a visit, or when the visit timestamp is missing/invalid, the row shows time. When T3 Code persists a newer visit, Bar updates on its next poll; clicking a link in Bar does not write the visit timestamp. Versions that keep visits only in browser localStorage cannot provide this Done indicator. T3 Code Bar retains its own Goal, Failed, and Woke labels and does not claim complete sidebar parity.

Data is read from one fixed local instance. The app does not merge remote environments, discover every local instance, or access tokens or credential stores.
