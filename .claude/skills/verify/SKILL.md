---
name: verify
description: Drive the T3 Code Bar macOS or Linux tray app through its real pairing, thread, usage, and settings surfaces and capture proof of local T3 Code connectivity.
---

# Verify T3 Code Bar

Use this skill for a real UI check of the T3 Code Bar desktop app. The Bar can read a local server or a linked environment through the T3 relay. Remote access uses a persisted Clerk session, a relay-issued DPoP credential, and the selected environment endpoint; it must never fall back to the local server. Read `features/README.md` and the relevant feature file before driving a journey.

## Launch

For a development instance, run from `desktop/`:

```sh
npm ci
npm run tauri -- dev
```

For the installed macOS app, launch `/Applications/T3 Code Bar Tauri.app` after T3 Code is already running. T3 Code must expose its local pairing port. The installed app stores its state under the selected T3 state directory and the pairing credential in the OS credential store.

The app is ready when the menu bar icon opens a panel with either the thread list or the `Connect T3 Code` pairing form. Do not treat a running process alone as readiness.

Teardown means close the app instance started for the run. Do not kill by process name. Preserve evidence files and the user's existing T3 Code session.

## Doctor

Run these read-only checks before driving the UI:

```sh
pgrep -alf 'T3 Code Bar Tauri|t3-code-bar-desktop'
lsof -nP -iTCP -sTCP:LISTEN | rg 'T3 Code|127\\.0\\.0\\.1'
cat "$HOME/.t3/userdata/tauri-bar-settings.connections.json"
```

The doctor is healthy only when T3 Code owns the expected local port, the Bar process is present, and the connection file is readable. Never print or inspect credential values. A connection file with `environments: []` means the Bar has no remote environment connection of its own.

## Drive

Use Computer Use against the real menu bar app.

1. Open the Bar menu bar icon.
2. Record whether the panel shows `Connect T3 Code`, a pairing form, a thread list, or an offline banner.
3. If pairing is required, create a fresh pairing key in T3 Code on the same machine and enter it in the Bar. Never reuse a key from another machine.
4. Confirm the panel changes to the thread list and record the visible connection state.
5. Open `Usage`, then record whether Cost, Tokens, and Limits render.
6. Open `Settings`, record the connection state, and return to the thread list.
7. If the state directory has `bar-environments.json` with multiple records, verify the environment selector shows each record by `environmentId`, label, machine, and status. Selecting a local record must refresh the local view. Selecting a remote record must show its live shell snapshot and a reachable status; do not count the catalog entry alone as remote connectivity proof.

Prefer accessibility labels and visible text. Do not use coordinate-only clicks when a label is available.

## Evidence

Store proof under `/tmp/t3-code-bar-verify/<timestamp>/`. Capture:

- a screenshot before pairing;
- the pairing action and resulting connected state;
- a screenshot of the thread list;
- a screenshot of Usage and Settings;
- the doctor output with credentials redacted;
- the exact app build and T3 Code version;
- the final process and port ownership check.

The proof must exercise the real menu bar path and verify the resulting visible state. A cached thread list is not proof of a live connection. For a remote environment claim, also record the T3 Code environment ID, relay status, selected endpoint, and live shell snapshot separately. Do not claim that the Bar connected to a remote environment merely because T3 Code lists it. Usage and limits remain separate evidence and must be labelled unavailable when the selected remote API does not expose them.

## Helpers

Run `scripts/verify-environment-catalog.sh <path>` before a catalog journey. It validates the same stable-ID and duplicate-label invariants used by the environment registry and prints a redacted inventory. It is read-only and does not replace the real UI journey.

## Cleanup

Close only the app instance started by the run. Remove scratch state created for an isolated verification profile, but never remove the user's Keychain item, pairing key, T3 Code data, or evidence directory. Keep evidence after cleanup and verify that it still exists.

## Feature map

Read `features/README.md`, then the feature file for the journey being verified.
