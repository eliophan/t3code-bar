# Pairing, reconnect and disconnect

Users connect to their local T3 Code instance and can reconnect or remove the
saved credential without exposing the pairing key in screenshots or logs.

## Sub-features

- `pair-onboarding`: enter Pairing key and Connect on first launch.
- `pair-settings`: connect or Pair again… from Settings.
- `pair-limits`: expand Connect T3 Code… in Limits.
- `reconnect`: reconnect a saved credential after transport interruption.
- `disconnect`: Cancel preserves pairing; Disconnect and remove key removes it.
- `credential-relaunch`: a separate packaged process restores saved credentials.

## How to get to it (user POV)

First launch shows Connect T3 Code and the Pairing key field. The connection
section also appears in Settings and on Limits when disconnected. A paired
connection exposes Disconnect… in Settings; confirmation offers Cancel and
Disconnect and remove key.

## Driving it with Cua

Preconditions: the positive journey needs a disposable local T3 Code server,
its genuine pairing key and an approved disposable credential namespace for
credential-persistence acceptance. The default seed supplies none of these.

- **Unpaired state.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py snapshot`.
  Require Pairing key and disabled Connect with an empty field. Do not report
  a successful connection from that screen.
- **Connect.** In a suitably isolated session, obtain the key through the
  available secret-input mechanism, refresh Cua's Pairing key control, and use
  native text entry. Do not put a real key in CLI arguments or retained action
  logs. The current helper intentionally has no credential-entry command.
  Choose Connect through native Cua and require Connected to T3 Code plus the
  Threads surface. If secure entry/transport is unavailable, fail with that
  unmet precondition instead of injecting `paired` into app state.
- **Reconnect entry.** With a saved credential and interrupted transport, run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Reconnect`.
  Capture the old state and the restored Connected to T3 Code state after the
  server returns. A click acknowledgement alone is not reconnection.
- **Disconnect.** From Settings run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click 'Disconnect…'`,
  then `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Cancel`.
  Confirm pairing remains. Repeat Disconnect… then choose
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click 'Disconnect and remove key'`.
  Confirm unpaired onboarding, then confirm it again after a separate process.

## Gotchas

- Default fixture mode uses memory credentials, so even successful synthetic
  pairing would not prove Secret Service persistence through process restart.
- The helper disables production keyring access on purpose; packaged credential
  acceptance needs a separate disposable namespace and real package journey.
- Pairing transport, revoked/expired keys and reconnect have not been driven by
  this skill's initial Settings proof. Their absence is an unmet precondition.
- Never reuse a personal pairing credential, expose it in evidence, or use
  OS credential-adapter tests as proof that the packaged GUI saved it.
