# Pairing and reconnect

## Sub-features

- Enter a fresh T3 Code pairing key.
- Restore a saved pairing from the OS credential store.
- Show a useful error for an expired, rejected, missing, or unreachable credential.

## How to get to it (user POV)

Open the Bar menu bar icon while T3 Code is running. A disconnected Bar shows `Connect T3 Code` or its Settings connection controls.

## Driving it with Computer Use

Use the visible pairing form and its labels. Generate the pairing key in T3 Code on the same machine, enter it, and observe the panel change to the thread list. Restart the Bar and verify that it restores the saved connection without asking for the key again.

## Gotchas

Local pairing is loopback-only. A pairing key from Linux or a remote `omarchy` endpoint is not proof of relay connectivity. Cloud login is a separate path described in `environments.md`.
