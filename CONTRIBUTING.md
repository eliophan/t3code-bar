# Contributing

T3 Code Bar is an independent companion to T3 Code. Read the [support policy](docs/SUPPORT.md) before proposing a feature.

## Set up and check your change

Follow [Development](docs/DEVELOPMENT.md) for prerequisites, setup, native builds, and the repository layout. Run `bash scripts/check.sh` from the repository root before submitting a PR. Report checks you could not run, including the reason.

Use synthetic fixtures. Never submit personal databases, pairing keys, tokens, credentials, environment files, or unredacted logs and screenshots. Follow [Security](SECURITY.md) for sensitive reports.

## Submit a change

1. Search existing issues and PRs. Discuss large features or compatibility changes in an issue first.
2. Create a focused branch from current `main`. Keep unrelated cleanup in a separate PR.
3. Add regression coverage for changed behavior. Record the exact T3 Code version or commit when changing an upstream adapter.
4. Update affected documentation and the Unreleased section of [CHANGELOG.md](CHANGELOG.md) for user-visible changes.
5. Open a PR describing the problem, change, and validation. Include UI captures when relevant, with private data removed.

Both macOS and Linux CI checks must pass before merge. A passing build does not replace native acceptance. For platform changes, name the actual package, OS, and architecture tested.

## Licenses and review

Contributions are provided under the repository's [MIT license](LICENSE). Only submit material you have permission to contribute. When adding dependencies, copied code, fonts, or artwork, check the license, preserve copyright notices, and update [Third-party notices](THIRD_PARTY_NOTICES.md) where required. Identify the source and license in the PR.

The primary maintainer, [@eliophan](https://github.com/eliophan), decides whether a PR fits the product, compatibility requirements, and maintenance budget. Reviews happen as time permits. There is no response or merge SLA. Follow the [Code of conduct](CODE_OF_CONDUCT.md).
