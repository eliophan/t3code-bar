# Development and validation

## Structure

- `Sources/Config/EnvironmentConfig.swift`: resolves the data directory, runtime metadata, and environment ID, then builds URLs.
- `Sources/Data/DatabaseManager.swift`: owns read-only SQLite access and date parsing on an actor; returns `Sendable` data or explicit errors.
- `Sources/Data/ThreadModel.swift`: derives status from persisted projection data.
- `Sources/Data/ThreadMonitor.swift`: allows one asynchronous read at a time and aggregates the badge from the full inbox.
- `Sources/Usage/`: the Usage data layer. It decodes T3 Code's scan cache (`JSONScanner`, `UsageCacheDecoder`, `UsageCacheLocator`), de-duplicates and windows records (`UsageSelection`, `UsageWindow`), prices them (`UsagePricing`, `UsagePriceSources`, `CodexTierResolver`), builds `UsageReport`, and feeds the screen through `UsageMonitor`.
- `Sources/Views/PopoverView.swift`: renders the list and loading/offline/error states; stale data does not display live labels.
- `Sources/Utils/AppUpdater.swift`, `UpdateState.swift`, `UpdateMenuModel.swift`, `UpdatePrompt.swift`, `UpdateRetryPolicy.swift`, `ReleaseFetcher.swift`, and `UpdateLog.swift`: the self-updater. See [The updater](#the-updater).
- `Sources/Utils/AppLauncher.swift`: opens applications and links through macOS and reports failures.
- `Tests/`: isolated SQLite fixtures in temporary directories; no personal database or real server is used.

## Usage canary

`Tests/Canary/state` is a small T3 Code state directory and `Tests/Canary/golden.json` holds what the oracle `repro.py` computes for it. `UsageCanaryTests` compares every number. Regenerate the golden file after changing the fixture with the commands below.

```bash
python3 -I Tests/Canary/make_fixture.py
python3 -I Tests/Canary/golden.py /path/to/repro.py > Tests/Canary/golden.json
```

To compare a real state directory with the oracle, print the Swift report and the oracle's report in the same format and diff them.

```bash
T3_CODE_BAR_REAL_STATE_DIR=~/.t3/userdata swift test -c release --filter UsageRealDataTests
python3 -I Tests/Canary/compare.py /path/to/repro.py ~/.t3/userdata 2026-11-06T09:08:00Z Asia/Ho_Chi_Minh
```

`UsageRealDataTests` is skipped unless `T3_CODE_BAR_REAL_STATE_DIR` is set. Set `T3_CODE_BAR_REAL_NOW` and `T3_CODE_BAR_REAL_TZ` to change the instant and zone.

## Install from source

You need Git and Swift 6 or later. Run this from a directory that does not already contain `t3code-bar`:

```bash
git clone --depth 1 https://github.com/eliophan/t3code-bar.git && bash t3code-bar/scripts/install.sh
```

The installer stops if an app already exists at `~/Applications`, so quit it and move it aside first. Builds from source report version 1.0.0 and never update themselves.

## Run and package locally

From the repository root, run the app from source:

```bash
swift run T3CodeBar
```

Install the tracked pre-push hook once per clone so the automated checks below run before every push (`git push --no-verify` skips it):

```bash
bash scripts/setup_hooks.sh
```

To package a local app without installing it:

```bash
bash scripts/build_app.sh
open "build/T3 Code Bar.app"
```

Build output is written to `build/` and excluded from Git. The package has no external Swift dependencies.

## Releasing

Releases are built by `.github/workflows/release.yml` when a tag like `v1.2.3` is pushed. The workflow runs the tests, packages a universal (`arm64` and `x86_64`) zip with `scripts/package_release.sh`, and attaches the zip and its `.sha256` to a GitHub Release. The tag sets the app's `CFBundleShortVersionString`, which `scripts/install-release.sh` and the in-app updater compare to decide whether an update is needed. Universal builds also set `T3CodeBarReleaseBuild` in `Info.plist`; only those builds update themselves.

```bash
git tag v1.2.3 && git push origin v1.2.3
```

Packaging runs `scripts/smoke_test_app.sh` first, and CI runs it on every push. It launches a copy of the app with the SwiftPM resource bundle and `.build` removed and fails if the app exits, which is how v1.0.0 crashed everywhere except the CI runner. Run it by hand after `bash scripts/build_app.sh`.

To try a package without publishing, run `bash scripts/package_release.sh v1.2.3`, which writes the zip to `build/`. `package_release.sh` builds universal by setting `T3_CODE_BAR_UNIVERSAL=1` for `build_app.sh`, and refuses to package a binary missing either slice. The smoke test runs on the host architecture only. The install script accepts `T3_CODE_BAR_ARCH` (to pretend to be another architecture), `T3_CODE_BAR_RELEASE_BASE_URL` (a directory laid out as `<tag>/<zip>`), `T3_CODE_BAR_API_URL`, `T3_CODE_BAR_INSTALL_DIR`, and `T3_CODE_BAR_SKIP_OPEN=1`, so it can be tested against `file://` URLs and a temporary directory without touching an installed app.

### The updater

`UpdateState.swift` holds the state model. `UpdateState` has six cases, `idle`, `checking`, `available(tag)`, `installing(tag)`, `failed(UpdateFailure)`, and `unsupported(UnsupportedReason)`. `UpdateStatus` pairs the state with `lastChecked`. `UpdateStatus.reduce(_:_:)` is a pure function over `UpdateEvent`. Time arrives inside the event, so tests need no clock. The reducer drops an event that does not fit the current state, so a late check result cannot overwrite an install in flight. `unsupported` is terminal. `UpdateMenuModel` turns a status into the menu's version line, status line, and button. It holds every user-visible status string.

`AppUpdater` is the `@MainActor` object that owns the status and sends events. Checking and installing are separate steps. `checkNow()` and the automatic check both stop at `available`. Only `installAvailable()` and `installAvailable(tag:)` start an install, and the tag form installs only the tag the user was shown. An automatic check goes further than a manual one. If **Install Without Asking** is on, it installs. If not, it calls `onUpdateAvailable`, which opens the ask-first window once per tag per launch. `T3CodeBarApp.swift` builds that window from `UpdatePrompt`, shows it floating without activating the app, and makes Return choose **Later**. **Install and Restart** has no key equivalent. The window has no Escape handler.

`ReleaseFetcher` asks GitHub for the latest release and keeps the last ETag and release in memory. The next request sends `If-None-Match`, and a 304 reuses the cached release. HTTP 429, or 403 with `x-ratelimit-remaining: 0`, is `rateLimited`. A transport error is `network`. Any other status or an unreadable body is `badResponse`.

An automatic check that fails with `network`, `badResponse`, or `rateLimited` schedules a retry through `UpdateRetryPolicy`. The delays are 5 minutes after the first consecutive failure, 15 minutes after the second, and 1 hour after the third. After that the app waits for the regular 6 hour tick. A success clears the count. A manual check never schedules a retry. `installerExit` is not retried, because the script would fail the same way.

A newer release is offered only when its asset list contains `T3-Code-Bar-<version>-universal.zip`. GitHub can publish a tag before its assets finish uploading, and the asset check keeps the app from installing a half-published release. `UpdatePlatform` has one case today, `macOS`, and its `assetName(version:)` returns that name. To port the app, add a case with its asset name and an install script for that OS. `AppUpdater.liveRunInstaller` still runs `scripts/install-release.sh` and needs the same change.

`UpdateLog` writes `~/Library/Logs/T3 Code Bar/update.log` with ISO 8601 timestamps. The app logs failed checks, `install started`, and `install finished` with the exit code. The installer child's stdout and stderr go to the same file, opened with `O_APPEND`. The file rotates to `update.log.1` above 256 KB. Log only event names and exit codes from app code, never tokens or paths from the database.

The updater tests are `AppUpdaterTests`, `UpdateStateTests`, `UpdateMenuModelTests`, `UpdatePromptTests`, and `UpdateResilienceTests`. They inject the fetch, the installer, the settings, the retry scheduler, and the log, so no test touches GitHub or runs an installer. `InstallReleaseScriptTests` runs the real `scripts/install-release.sh` against `file://` URLs. It sets `T3_CODE_BAR_API_URL`, `T3_CODE_BAR_RELEASE_BASE_URL`, `T3_CODE_BAR_INSTALL_DIR`, `T3_CODE_BAR_ARCH`, and `T3_CODE_BAR_SKIP_OPEN=1`, and checks fresh installs, upgrades, checksum mismatches, bad archives, malformed tags, and the restore after a failed move.

To try an update end to end, you need a published release newer than the app you run, because the updater reads `eliophan/t3code-bar` and runs the install script from that release's tag. Package an older version with `bash scripts/package_release.sh v1.0.0`. Unzip `build/T3-Code-Bar-1.0.0-universal.zip` with `ditto -x -k` into `~/Applications`, and open the app. About 10 seconds later the menu shows the new version, or click **Check for Updates…**. Watch `~/Library/Logs/T3 Code Bar/update.log` during the install. Local builds from `scripts/build_app.sh` or `swift run` never update, so they cannot run this test.

The updater has these known limits.

- Releases are ad hoc signed and not notarized. The updater does not check a signature.
- The `.sha256` file sits in the same release as the zip. It detects corruption but not a tampered release.
- A failed check replaces `available` with `failed` until the next successful check. The ask-first window's **Install and Restart** then does nothing, because `installAvailable(tag:)` requires `available`.
- Escape does not close the ask-first window. Return and **Later** do.

## Automated checks

Run from the repository root:

```bash
swift test
swift build -c release
bash -n scripts/build_app.sh scripts/install.sh
```

Every check must return exit code 0. A build, test, or syntax failure returns a nonzero code. With Swift Testing, verify the final summary reports tests actually executed and all passed; the XCTest compatibility summary may separately report zero tests because this project uses Swift Testing.

CI runs tests, a Release build, and script syntax checks. It does not upload an `.app`, sign with Developer ID, notarize, commit, or publish automatically.

## Repository hygiene

Keep generated `.build/`, `build/`, `.local-reference/`, database, and environment files out of commits; `.gitignore` covers these paths. Do not publish screenshots containing private thread names, project paths, or message content without redacting them.
