# Changelog

This log records user-visible changes. GitHub releases contain package downloads and release-specific installation notes.

## Unreleased

### Changed

- Start onboarding with local pairing instructions and put the local connection first in Settings.
- Make importing T3 Code's encrypted environment catalog opt-in; preserve Bar's own pairing credentials and independent manual and T3 Connect connections.
- Distinguish a running local server from an authorized Bar connection instead of showing unpaired installations as connected.

### Maintenance

- Add contribution, security, support, community, and issue-reporting guidance.
- Add an upstream compatibility evidence record and a release-note checklist.

## 2.0.0-alpha.3 - 2026-11-11

### Added

- Environment selection and per-environment thread identity.
- Direct pairing and T3 Connect environment directory discovery.

### Fixed

- Exclude settled, snoozed, and delegated threads from the sidebar list.
- Improve Relay credential diagnostics and SSH test fixtures.

### Compatibility and validation

The published release notes record passing CI on macOS 15 and Ubuntu 24.04 for commit `21310daf0d310ed54bf41685ff149d403357f348`. They do not record an exact tested T3 Code version or commit. That upstream compatibility remains unverified in this log.

### Migration and limitations

This is a Tauri v2 alpha. Swift v1 settings and credentials are not migrated. Updates use manual installation. macOS packages target Apple Silicon and are ad hoc signed, without Apple notarization. Linux packages target x86_64 and require a StatusNotifier tray host.

Source: [v2.0.0-alpha.3 release notes](https://github.com/eliophan/t3code-bar/releases/tag/v2.0.0-alpha.3). Earlier releases remain available in [GitHub releases](https://github.com/eliophan/t3code-bar/releases).
