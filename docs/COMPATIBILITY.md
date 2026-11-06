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

The Usage screen combines T3 Code's transcript cache with local OpenCode and Antigravity usage and Cursor's saved account cache. Each source is read without changing its contents.

Checked on 2026-11-06 against T3 Code Nightly `0.1.8-nightly.20261105.4205` (commit `935818e422f9`). Every file is opened read-only, and Bar makes no network call.

| File | What Bar takes from it |
| --- | --- |
| `usage-scan-cache-vN.json` | Per-transcript usage records for Claude, Codex, Grok, Pi and Muse. Bar reads the highest `N` whose `version` field is 4 to 11. |
| `usage-model-rates.json` | The price list T3 Code fetched. Without it, only provider-reported costs are priced. |
| `settings.json` | `usagePriceOverrides`, `usageModelAliases`, and the IDs and driver names of Antigravity provider instances. Environment values and credentials are skipped without decoding them. |
| `statev2.sqlite` | The Codex service tier of turns the cache has not resolved yet. This reads `orchestration_events`, `orchestration_v2_projection_provider_turns` and `orchestration_v2_projection_run_attempts`. If the database is missing, those turns count at standard speed. |

### Freshness and coverage

- T3 Code writes the cache only when its Usage page scans. The numbers are as current as the last time that page was open, and the screen says so.
- OpenCode and Antigravity are read from local sources every 30 seconds while Cost or Tokens is visible. Cursor reads only the account cache that T3 Code already fetched. Missing, partial, and unreadable sources are disclosed below the totals.
- Remote and linked environments are not included. Bar reads this Mac only.
- T3 Code's per-user list of hidden providers lives in its renderer storage. Bar does not read it.

### Cache format

- Old `-vN` files stay on disk next to the current one. Bar picks by generation and by the `version` field, never by modification time. A newer unsupported format that is also newer on disk makes Bar show "T3 Code changed its usage format" instead of stale numbers.
- A file entry with any row that has fewer than 11 items, a non-number where a number belongs, an unknown model index or an unknown speed index is dropped whole, as T3 Code does. The screen warns when this happens.
- Claude rows for the model `<synthetic>` are dropped. A provider kind this build does not know is kept under its own name and counts in the totals.
- A cache that fails to parse is treated as being rewritten. Bar keeps the last good numbers and tries again.
- The file is 20 MB or more. Bar decodes it off the main actor in about 0.1 to 0.4 seconds, only when its size or modification time changes, and builds each period from the decoded records.

### Local usage sources

OpenCode reads `opencode.db`, matching `opencode-*.db` files, and legacy `storage/message/**/*.json`. It selects only message tables, never credentials. `OPENCODE_DATA_DIR` accepts comma-separated roots. Otherwise the root is `$XDG_DATA_HOME/opencode`, or `~/.local/share/opencode`. Message IDs deduplicate SQLite and legacy copies. Reasoning is added to OpenCode output once.

Antigravity reads conversation databases from `ANTIGRAVITY_DATA_DIR`, or the standard `~/.gemini/antigravity*` and `~/.config/antigravity` directories. Configured T3 Code Antigravity instances add their hashed `providers/antigravity/<hash>/antigravity-acp` directories. The reader decodes step and generation metadata, retries, and trajectory timestamps. Shared response identities merge across files with the maximum of each token field and the best timestamp. Unknown model IDs count tokens without an invented price. Directory walks skip symlinks.

Cursor reads `usage-cursor-account-cache-v1.json` from the state directory. Its reported costs and model names feed the same price rules as other records. The screen shows the oldest account fetch time. The Cursor reader never refreshes from the network or accesses Keychain. Without this cache, Cursor usage is unavailable.

Local databases are opened with `SQLITE_OPEN_READONLY` in a read transaction. Their size, modification time, attribute change time, and WAL fingerprint cache the decoded records. An unchanged database is not decoded again. A failed read retains its last readable records when available and marks that provider partial. A schema with no supported message or metadata table is unreadable. Bad records are skipped and counted. A missing source adds no provider row. A local source can supply usage before the transcript cache exists, with an explicit warning about missing transcript providers.

The transcript cache timestamp describes only transcript providers. Local check times and Cursor fetch times appear separately. These readers are ports of the same documented T3 Code internals, not a promise of compatibility with future schema changes. Fixture tests cover both OpenCode table layouts, legacy JSON, WAL changes, Antigravity protobuf layouts and identity merges, Cursor cache tuples, and failure recovery. The opt-in real-data test accepts `T3_CODE_BAR_REAL_LOCAL=1` to include local readers alongside the transcript cache.

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

## Limits

Limits are not in SQLite. T3 Code writes a snapshot of every provider instance to `caches/<instanceId>.json` in the folder above the state directory, which is `~/.t3/caches` by default. Bar lists that folder every 5 seconds while the Limits tab is open and decodes only the files whose size or modification time changed. A file it cannot decode is skipped and counted, and a file that decoded before keeps its last good copy.

Bar reads these keys and no others.

- `driver`, `instanceId`, `enabled`, `installed`, `availability`, and `displayName`.
- `auth.status`, `auth.label` (the plan), `auth.email`, and `auth.workspaceId`.
- `usageLimits.checkedAt`, `windows`, `resetCredits`, `externalUsage`, `unavailable`, and `credentialFingerprint`.

The email, the workspace id, and the fingerprint only build the key that merges accounts across files. They are never shown, logged, or kept in the types the screen reads.

Bar applies T3 Code's own rules. A provider counts when it is enabled, installed, not marked `availability: "unavailable"`, and has `usageLimits`. Files for one `driver:email` merge, split by workspace, and the newest `checkedAt` wins. Windows group by driver and sort session, weekly, monthly, other. "Left" is `round(100 - clamp(usedPercent))`, and pace is ahead or under beyond 5 points. A Codex or Claude session shows 100% used while the weekly window is exhausted. An account that reports `unsupported` shows nothing, and one that failed to probe shows T3's warning text.

The optional live connection supplements the saved-provider fallback with T3’s server snapshots. The contract was inspected in the `0.1.8-nightly.20261105.4205` bundle (`935818e422f9`): `providerUsageLimits`, server-config event schemas, OAuth authorization, Effect RPC JSON socket protocol, and the client’s `usageLimits` merge. This is an internal protocol, not a stable public API.

- A user explicitly supplies a newly provisioned pairing code. Bar posts the form-encoded token exchange to loopback `/oauth/token`, requesting exactly `orchestration:read`. It accepts only a Bearer access-token response with exactly that scope. The pairing code is not persisted, logged, or borrowed from T3’s files or credential stores.
- Bar requests a ticket from `/api/auth/websocket-ticket/rpc` with its own access token, then opens `/ws?wsTicket=…`. It accepts only a validated port on `127.0.0.1`, rejects redirects, and uses an ephemeral session without cookies, credential storage, cache, or configured proxies. Remote destinations and legacy ticket-endpoint fallbacks are not supported.
- The only RPC request is `subscribeServerConfig({usageLimitSources:true, workspaceSnapshotsOnDemand:true})`. Effect Request IDs are numeric; headers are an array. Bar consumes `Chunk` values, acknowledges them with `Ack`, sends `Ping`, and interrupts/closes the stream when hidden. Reconnect obtains a fresh ticket and requires a fresh snapshot before applying updates. Unknown/malformed protocol and authorization failures stop the connection; transport failures retry. No `refreshProviders`, reset-credit, settings, or other mutation RPC is sent.
- Initial `snapshot.config.providers` and `snapshot.config.usageLimitSources` are followed by `providerStatuses.payload.providers` and `usageLimitSourcesUpdated.payload.sources`. Only Limits fields are retained; settings/keybinding/theme updates are ignored. Each source’s `id`, `label`, `accounts`, and `error` are decoded. Source account fields are `id`, `driver`, `email`, `plan`, `workspaceId`, and `usageLimits`.
- Provider and source accounts share T3’s email/fingerprint/workspace merge and newest `checkedAt` rule, including independent newest plan/reset-credit metadata. With a live connection, the full provider list is authoritative, so deleted/disabled providers are not revived from cache. During an outage, last live data merges with newer cached checks and stays explicitly marked as retained data. Source errors and empty-source notices remain visible.
- Tokens are memory-only by default. **Remember Bar token in Keychain** explicitly stores only the exchanged Bar token, its local port, and expiry under Bar’s own service/account. **Reconnect saved** explicitly loads that item. There is no automatic refresh of expired access tokens.

Only this local environment is subscribed. Linked-environment limits are excluded, and all numbers remain as fresh as T3’s last provider/source probe. Subscribing can trigger T3-side remote/direct endpoint discovery even though Bar never contacts those endpoints itself. Real pairing, Keychain behavior, and complete real-account parity remain manual UAT gates; mock tests cover the protocol, 6 Codex/2 Claude/Antigravity account fixture, updates, deduplication, scope refusal, errors, reconnection, cache fallback, and cancellation.

T3 Code restores cached snapshots without `usageLimits` when it starts, so a file can keep old bars until the first probe. T3 Code's own page does not hide a window whose reset time has passed. It shows the stored percent with "resets now". Bar does the same and also exposes `checkedAt`, so the screen can say how old the numbers are.

## What the data cannot provide

The Usage screen shows the Cost, Tokens and Limits tabs of T3 Code's Usage page. Some parts of that page need data Bar does not read.

- Providers without a supported local source or a usable T3 Code cache. No provider API is contacted to fill gaps.
- Remote and linked environments, and T3 Code's list of hidden providers.
- The cost table by day or hour and the model dialog. The popover has no room for them.
- Unpaired CLI Proxy limits, linked-environment limits, and the actions the page offers, such as setting a price or using a reset credit.
- Transcript usage fresher than T3 Code's last Usage page scan, Cursor data fresher than its saved fetch, and limits fresher than the last provider check.

The reference `ThreadBackgroundLivenessService` uses an in-memory registry rather than persistence. It distinguishes an active agent/workflow from a watch loop. T3 Code Bar does not connect to that runtime service and does not infer current work from old task history.

The reference client uses `lastVisitedAt` in client state. The installed nightly also persists this field in `orchestration_v2_projection_threads.payload_json`, so Bar reads it directly. Done appears only when the latest visible run has status `completed` and a valid `completedAt > lastVisitedAt`. After a visit, or when the visit timestamp is missing/invalid, the row shows time. When T3 Code persists a newer visit, Bar updates on its next poll; clicking a link in Bar does not write the visit timestamp. Versions that keep visits only in browser localStorage cannot provide this Done indicator. T3 Code Bar retains its own Goal, Failed, and Woke labels and does not claim complete sidebar parity.

Data is read from one fixed local instance. The app does not merge remote environments, discover every local instance, or read T3 Code’s tokens or credential stores. Only the optional Limits pairing flow handles a new user-supplied code and Bar’s own access token.
