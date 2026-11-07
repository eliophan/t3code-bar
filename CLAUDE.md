# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

T3 Code Bar is a macOS 14+ menu bar app (Swift 6, SwiftUI plus AppKit, no external dependencies) that shows the local T3 Code thread inbox. It is a separate app that reads T3 Code's SQLite database. It never writes to it.

## Commands

```bash
swift test                                   # all tests (Swift Testing, not XCTest)
swift test --filter AppUpdatePolicyTests     # one suite, or use Suite/testName
swift test -c release                        # what CI and the pre-push hook run
swift run T3CodeBar                          # run from source (no .app bundle)
bash scripts/build_app.sh                    # package build/T3 Code Bar.app
bash scripts/smoke_test_app.sh               # launch the packaged app without .build or the SwiftPM bundle
bash scripts/install.sh                      # build and install to ~/Applications (refuses if one exists)
bash scripts/setup_hooks.sh                  # enable .githooks/pre-push (runs swift test -c release and bash -n on scripts)
```

The XCTest summary line may report zero tests. Read the Swift Testing summary instead. CI (`.github/workflows/ci.yml`) runs `swift test -c release`, `build_app.sh` plus the smoke test, and `bash -n` on every script. Markdown-only pushes skip CI.

## Architecture

- **Data flow.** `ThreadMonitor` (`@MainActor`, `ObservableObject`) polls every second and allows one read at a time. Each read calls `DatabaseManager`, an actor that opens T3 Code's `statev2.sqlite` read-only and returns a `ThreadSnapshot`. On a failed read the monitor keeps the last good list but drops live status, so stale data never shows as current. `PopoverView` and the status icon observe the monitor.
- **Status is derived, not stored.** `ThreadModel.swift` and the queries in `DatabaseManager.swift` compute a thread's state (Approval, Input, Working, Done, Woke, Limited and so on) from T3 Code's `orchestration_v2_projection_*` tables and JSON payloads. Snooze, Done and Woke depend on `lastVisitedAt` and related timestamps that T3 Code persists. `docs/COMPATIBILITY.md` documents the schema contract, what SQLite cannot provide, and the exact rules. Read it before changing status logic. The schema is T3 Code's internal projection-v2 and can change under you.
- **Usage.** The Usage screen reads the transcript cache plus read-only local OpenCode and Antigravity SQLite sources and Cursor's saved account cache. `UsageMonitor` asks `UsageEngine` (an actor) to decode T3 Code's `usage-scan-cache-vN.json` once per size or time change, then builds `UsageReport` for the chosen period from the decoded records with `UsageReportBuilder`. The code under `Sources/Usage` ports T3 Code's counting and cost rules, and `Tests/UsageCanaryTests.swift` pins it to the oracle's numbers. The cache's key order decides which duplicate wins, so it is read with `JSONScanner`, never `JSONSerialization`. `settings.json` holds credentials, so only price keys and Antigravity instance IDs and driver names are decoded. Read `docs/COMPATIBILITY.md` before changing any rule.
- **Limits.** The app requires a user-paired `LimitsLiveConnection` before showing its main popover. It requests only `orchestration:read` on validated loopback and subscribes to server-config provider/source snapshots; cache data remains an explicitly labeled fallback during outages. Never read T3 credentials or add refresh/reset/write RPCs. Only an explicit Remember action stores a token in Keychain, either Bar’s own exchanged DPoP token (one hour) or a long-lived Bearer token the user pastes from `t3 auth session issue`; tests inject mock transport and store, never real credentials or a live server. See `docs/COMPATIBILITY.md` and `docs/USAGE.md`.
- **Data location.** `EnvironmentConfig` resolves the state directory from `T3_CODE_BAR_STATE_DIR`, then `T3CODE_HOME` plus `/userdata`, then `~/.t3/userdata`. Tests use isolated SQLite fixtures in temp directories (`Tests/Fixtures.swift`), never a real database.
- **App shell.** `Sources/T3CodeBarApp.swift` holds `AppDelegate` (there is a custom `main`, no SwiftUI `App`). The popover is a borderless `NSPanel`, not an `NSPopover`. It closes through a global mouse monitor, and the status item button toggles it. Any click handling must keep those two paths from fighting, because a click on the icon reaches both. The right-click menu on the status item is built in `installContextMenuMonitor`.
- **Hover previews.** `PreviewController` shows a row's last assistant reply after the pointer rests for 500 ms. It reads only the `messages` table (`ThreadPreview.swift`), and users can turn it off in the right-click menu.
- **Self-update.** `AppUpdater` owns an `UpdateStatus` that the pure `UpdateStatus.reduce` changes, and `UpdateMenuModel` renders it in the right-click menu. `ReleaseFetcher` checks GitHub's latest release about 10 seconds after launch and every 6 hours, with `If-None-Match`. A release counts only when it lists `T3-Code-Bar-<version>-universal.zip` (`UpdatePlatform.assetName`). A check stops at `available`. Install is a separate step. Without **Install Without Asking**, an automatic check opens a non-modal ask-first window (`UpdatePrompt`, Return means Later, no Escape handler). Failed fetches retry after 5 minutes, 15 minutes and 1 hour (`UpdateRetryPolicy`). Events and installer output go to `~/Library/Logs/T3 Code Bar/update.log`, so log only event names and exit codes. To install, it runs that tag's `scripts/install-release.sh` as a detached child that quits, swaps and reopens the app. Releases are ad hoc signed and not notarized, and the `.sha256` sits beside the zip, so it detects corruption only. It only runs when `Info.plist` has `T3CodeBarReleaseBuild` (set by `build_app.sh` only for universal release builds) and the app sits at `~/Applications/T3 Code Bar.app`. Local builds report version 1.0.0, so without that flag they would replace themselves with an older release.

## Releasing

Pushing a tag like `v1.2.3` runs `.github/workflows/release.yml`. It tests, runs `scripts/package_release.sh` (universal build, zip and `.sha256`), and publishes a GitHub Release. The tag becomes `CFBundleShortVersionString`, which the install script and the updater compare. `install-release.sh` accepts test hooks (`T3_CODE_BAR_API_URL`, `T3_CODE_BAR_RELEASE_BASE_URL`, `T3_CODE_BAR_INSTALL_DIR`, `T3_CODE_BAR_ARCH`, `T3_CODE_BAR_SKIP_OPEN=1`) so it can be tested against `file://` URLs.

## Gotchas

- Resources must resolve inside the packaged app. v1.0.0 crashed everywhere except the CI machine because SwiftPM's resource accessor fell back to an absolute `.build` path. Never touch `Bundle.module` inside an `.app`. Load resources through `AppResources.swift` (`MenuBarIconGenerator.swift` has its own equivalent fallback), and run the smoke test after packaging changes.
- `UNUserNotificationCenter` aborts outside an `.app` bundle, so `AttentionNotifier` skips notifications under `swift run`.
- `.local-reference/` and `build/` are git-ignored. Do not commit them.
- Main is protected, so changes go through a PR.
