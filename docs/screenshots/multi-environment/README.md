# Multi-environment macOS screenshots

Native macOS captures of the T3 Code Bar panel while connected to several
environments (envA, envB, envC), exported so they can be viewed from the Linux
workspace.

All data is synthetic: fixture databases, local test servers and disposable
credentials. No personal threads, accounts or pairing keys appear.

Captured on 2026-11-10 from the verification run in
`/tmp/verify-multienv-macos-final/`. These are verification artifacts, not
packaged-release acceptance captures.

| File | Scenario |
| --- | --- |
| `ui-10-all.png` | All environments shown |
| `ui-20-one-envc.png` | Filtered to a single environment (envC) |
| `ui-11-hover-envb.png` | Hover preview on an envB thread |
| `ui-12-hover-envc.png` | Hover preview on an envC thread |
| `ui-30-stale-menu.png` | Stale environment menu state |
| `ui-40-envb-offline.png` | envB offline |
| `ui-50-restart-envb-offline.png` | App restarted while envB is offline |
| `ui-51-after-online.png` | After envB returns online |
