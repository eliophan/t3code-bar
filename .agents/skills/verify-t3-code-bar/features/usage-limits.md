# Usage and provider Limits

Users inspect estimated cost, token breakdowns and provider limits without
switching to the full T3 Code window.

## Sub-features

- `usage-open`: Usage from the Threads footer.
- `usage-metrics`: Cost and Tokens tabs.
- `usage-periods`: 24h, 7d, 30d and 90d.
- `limits-saved`: Limits with checked/updated state and saved quota values.
- `limits-live`: a new server value replaces the displayed older value.
- `limits-manage`: Manage usage ↗ opens the expected provider destination.

## How to get to it (user POV)

From a paired Threads panel choose Usage. Choose Cost, Tokens or Limits.
Cost/Tokens expose the period buttons. Limits can offer Connect T3 Code…
when disconnected and Manage usage ↗ for supported provider links.

## Driving it with Cua

Preconditions: pairing is established with a disposable local server. The
default seed has saved Usage caches but lacks pairing and a live Limits source;
report those unmet preconditions instead of forcing navigation through IPC.

- **Open.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Usage`,
  then `python3 .agents/skills/verify-t3-code-bar/scripts/control.py snapshot`.
  Require Usage, metric tabs and a populated or explicitly empty saved result,
  not a lingering Reading usage… state.
- **Metrics.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Tokens`,
  capture Processed tokens, then
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Cost`.
  Capture the cost estimate and breakdown. Compare with the synthetic dataset
  and frozen time; do not invent expected totals from the screen itself.
- **Periods.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click 7d`,
  then repeat for 30d, 90d and 24h, capturing selected periods and resulting
  values. An unchanged screenshot while the request is pending is not proof.
- **Limits.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Limits`.
  With a supplied disposable quota source, capture one known value, deliver a
  different value through the real server transport, then capture the changed
  percentage and updated timestamp. No limits data yet is a failed data
  precondition, not evidence of live updates.

## Gotchas

- Frozen time is 2026-11-06T09:08:00Z. The fixture cache is synthetic; no live
  provider account or personal local Usage directory is read in fixture mode.
- The UI is gated by pairing even when cache files exist.
- Limits groups, provider links and stale-data behavior need explicit source
  preconditions. The default seed does not establish them.
- Do not exercise Manage usage ↗ against a user's authenticated provider
  browser without including that destination in the verification scope.
