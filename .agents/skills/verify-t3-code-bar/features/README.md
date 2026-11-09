# T3 Code Bar native verification map

Read the feature matching the changed user behavior. These are independent
journeys; a pass at one entry point is not a pass for the others.

Baseline: a freshly built native Linux binary launched by `control.py launch`
on the real Omarchy session, with synthetic SQLite and Usage state, no saved
pairing credential, an available tray host and one owned Cua daemon. The app
starts at `Connect T3 Code`. Settings is usable without pairing.

Use the commands from the parent skill at repository root. All helper commands
fail with a nonzero exit; native snapshots/actions must also have complete
trees and successful postconditions. Capture the label/action, before/after
tree, attested screenshot and relevant side effect in the printed run directory.
Run cleanup after every attempt and retain the evidence.

The seeded state deliberately lacks a T3 Code pairing server and token.
Threads and Usage journeys that need pairing must report that precondition as
unmet; do not set `paired` directly or bypass onboarding. In-memory fixture
credentials cannot establish Secret Service persistence.

## Features

- [Tray and window lifecycle](tray-window.md): native tray Show, Hide, Settings,
  Quit; Escape; launch again; no-host and host-loss policy. Registration-only
  doctor is available; visible tray input is not yet proved.
- [Settings](settings.md): onboarding/header/tray entry points, both preference
  switches, Quit and persistence. The automated initial journey proves
  onboarding entry, Back to threads and Quit; the preference journey records a
  Cua native-Wayland switch-action failure.
- [Pairing and reconnect](pairing.md): onboarding, Settings and Limits entries,
  successful pairing, reconnect and disconnect. Positive journeys lack a
  pairing precondition in the default fixture.
- [Threads and hover preview](threads-preview.md): thread list, hover, context
  actions and destinations. Default fixture is blocked by onboarding.
- [Usage and Limits](usage-limits.md): cost/tokens, periods and live Limits.
  Default fixture is blocked by onboarding; live Limits also needs transport.
