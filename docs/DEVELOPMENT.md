# Development and validation

## Structure

- `Sources/Config/EnvironmentConfig.swift`: resolves the data directory, runtime metadata, and environment ID, then builds URLs.
- `Sources/Data/DatabaseManager.swift`: owns read-only SQLite access and date parsing on an actor; returns `Sendable` data or explicit errors.
- `Sources/Data/ThreadModel.swift`: derives status from persisted projection data.
- `Sources/Data/ThreadMonitor.swift`: allows one asynchronous read at a time and aggregates the badge from the full inbox independently of search filtering.
- `Sources/Views/PopoverView.swift`: renders the list and loading/offline/error states; stale data does not display live labels.
- `Sources/Utils/AppLauncher.swift`: opens applications and links through macOS and reports failures.
- `Tests/`: isolated SQLite fixtures in temporary directories; no personal database or real server is used.

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

Releases are built by `.github/workflows/release.yml` when a tag like `v1.2.3` is pushed. The workflow runs the tests, packages a universal (`arm64` and `x86_64`) zip with `scripts/package_release.sh`, and attaches the zip and its `.sha256` to a GitHub Release. The tag sets the app's `CFBundleShortVersionString`, which `scripts/install-release.sh` compares to decide whether an update is needed.

```bash
git tag v1.2.3 && git push origin v1.2.3
```

Packaging runs `scripts/smoke_test_app.sh` first, and CI runs it on every push. It launches a copy of the app with the SwiftPM resource bundle and `.build` removed and fails if the app exits, which is how v1.0.0 crashed everywhere except the CI runner. Run it by hand after `bash scripts/build_app.sh`.

To try a package without publishing, run `bash scripts/package_release.sh v1.2.3`, which writes the zip to `build/`. `package_release.sh` builds universal by setting `T3_CODE_BAR_UNIVERSAL=1` for `build_app.sh`, and refuses to package a binary missing either slice. The smoke test runs on the host architecture only. The install script accepts `T3_CODE_BAR_ARCH` (to pretend to be another architecture), `T3_CODE_BAR_RELEASE_BASE_URL` (a directory laid out as `<tag>/<zip>`), `T3_CODE_BAR_API_URL`, `T3_CODE_BAR_INSTALL_DIR`, and `T3_CODE_BAR_SKIP_OPEN=1`, so it can be tested against `file://` URLs and a temporary directory without touching an installed app.

## Automated checks

Run from the repository root:

```bash
swift test
swift build -c release
bash -n scripts/build_app.sh scripts/install.sh
```

The tests cover inboxes larger than 50 threads, pin ordering, hidden threads, silent/cancelled/queued runs, status priority, unread completion, persisted visits and later completions, missing/invalid visits, offline and read-error recovery, search without losing global badge counts, custom data directories, runtime ports, URL encoding, and missing configuration. Snooze regressions cover pending approval/input, completion and session errors before/equal/after snoozing, resolved requests, Woke after a visit, and invalid dates.

Every check must return exit code 0. A build, test, or syntax failure returns a nonzero code. With Swift Testing, verify the final summary reports tests actually executed and all passed; the XCTest compatibility summary may separately report zero tests because this project uses Swift Testing.

CI runs tests, a Release build, and script syntax checks. It does not upload an `.app`, sign with Developer ID, notarize, commit, or publish automatically.

## Local validation result — 2026-11-04

- `swift test`: 30 tests in 5 suites passed, including SQLite locking, malformed JSON during result stepping, and Done returning to a timestamp after T3 Code persists a newer visit.
- `swift build -c release`: passed; the earlier deprecated activation warning was removed.
- `bash -n scripts/build_app.sh scripts/install.sh`: passed. A local `.app` was packaged and opened for review; no download artifact was released.
- `scripts/install.sh` installed successfully into a temporary directory with `T3_CODE_BAR_SKIP_OPEN=1`. The resource bundle and license/notices were present. A second install was refused with a nonzero exit code and did not modify the existing binary. The installer’s normal auto-open branch has not been separately tested.
- Local Markdown links were valid. The README uses the author-provided PNG at its original size, including the menu bar; no generated illustration is used. Workflow syntax and `.gitignore` rules were checked earlier.
- The current SQL query successfully reads the local projection database through a read-only connection without printing thread titles or content. This is a compatibility check, not a benchmark or full application UAT.
- Release binary: arm64, linker-signed ad hoc, without Developer ID; notarization was not performed.
- The user confirmed that approval/input appears correctly. The snooze regression has automated coverage; a dedicated manual snooze UAT has not been completed. CI runs on GitHub-hosted macOS and has passed; the manual checklist below remains unverified.

## Manual checks before wider sharing

1. Start T3 Code and run `swift run T3CodeBar`; confirm the menu bar icon appears.
2. Start a run, request approval and user input, and compare the row and label with T3 Code. Also verify that a snoozed thread wakes for a request or new completion, then check Woke against the persisted visit data.
3. Click a thread, copy its link, and open it in the browser; try a different runtime port if available.
4. Quit T3 Code: the Offline banner should appear and the activity badge should turn off. Reopen T3 Code and confirm data updates again.
5. Set `T3_CODE_BAR_STATE_DIR` to a temporary directory without a database and confirm the missing-data error; do not modify or delete the real database to create an error.
6. Scroll a long list, inspect title/branch/provider data, and right-click the menu bar icon to quit.
7. If using local packaging, open the app from Finder outside the source directory and confirm the resource bundle loads.

This is a checklist, not a claim that all UAT is complete. Intel, Gatekeeper, notarization, and RAM/CPU/latency benchmarks have not been verified.

## Repository hygiene

Keep generated `.build/`, `build/`, `.local-reference/`, database, and environment files out of commits; `.gitignore` covers these paths. Do not publish screenshots containing private thread names, project paths, or message content without redacting them.
