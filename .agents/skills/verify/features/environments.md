# Linked environments

## Sub-features

- Sign in once through the T3 Code hosted connect flow.
- List multiple environments, including records with the same label.
- Connect the selected environment through the relay and read its shell snapshot.
- Keep remote thread identity scoped to the selected environment.
- Report remote Usage and Limits as unavailable when that environment API does not expose them.

## How to get to it (user POV)

Open the Bar with no local catalog, choose `Sign in to T3 Code`, complete the hosted login, then select an environment from the `Environment` selector. Press `Connect` for a disconnected remote record.

## Driving it with Computer Use

Use the environment selector by its accessible label. Verify that two entries with label `omarchy` remain distinct by machine, status, and stable environment ID. Select the remote entry, press `Connect`, and verify a reachable status plus thread rows loaded from that environment. Switch to Usage and verify local data is not shown for the remote selection.

## Gotchas

The real relay journey requires an authenticated T3 Code account and at least one linked environment. A catalog file or a T3 Code list alone is not proof of connectivity. The current remote implementation does not claim remote Usage or Limits support because no matching public remote contract was found.
