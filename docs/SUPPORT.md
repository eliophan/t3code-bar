# Product scope and support

T3 Code Bar is an independent, community-maintained companion. It is not affiliated with or endorsed by T3 Tools. Changes to Bar must work without requiring a modified T3 Code installation.

## Product scope

Bar provides thread attention and navigation, reply previews, usage estimates, provider limits, notifications, and environment discovery and pairing. It uses local read-only data and authorized remote reads. T3 Code remains responsible for running agents and managing their work.

New features are evaluated for usefulness, read-access boundaries, upstream compatibility, native platform behavior, and maintenance cost. An issue or prototype does not establish support for a feature.

## Platforms and compatibility

macOS Apple Silicon and Linux x86_64 are the current distribution targets. macOS requires 14 or later. Apple Silicon is the primary tested Mac architecture. Linux packages build on Ubuntu 24.04; native journeys have been exercised on Omarchy. Build coverage on Ubuntu does not establish native acceptance on every Debian or Ubuntu desktop.

Intel Mac, minimum-version macOS acceptance, other Linux desktops, and other CPUs require separate validation. Windows is deferred. See [Compatibility](COMPATIBILITY.md) for runtime requirements, remote access limitations, and the version evidence record.

T3 Code's internal SQLite projections, cache formats, and RPC contracts can change. Bar does not promise compatibility with every upstream release or provider response. Report both product versions when a reader or connection fails.

## Maintenance

[@eliophan](https://github.com/eliophan) is the primary maintainer and makes scope and merge decisions. Support and reviews happen as time permits, without an SLA. Bug fixes and compatibility repairs take priority over expanding platform scope. Security fixes follow [Security](../SECURITY.md).

Use public issues for sanitized bug reports and feature proposals. Use private vulnerability reporting for security problems. Follow [Contributing](../CONTRIBUTING.md) for patches and the [Code of conduct](../CODE_OF_CONDUCT.md) for community participation.
