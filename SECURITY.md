# Security policy

## Report a vulnerability privately

Use [GitHub private vulnerability reporting](https://github.com/eliophan/t3code-bar/security/advisories/new). Do not disclose exploitable details, tokens, pairing keys, or personal data in public issues or PRs.

Describe the affected Bar version, T3 Code version, OS, reproduction steps, expected access boundary, and impact. Use synthetic data and redact logs. Do not send working credentials or a personal database. If you exposed a credential, revoke it through its issuer.

Reports are reviewed by the maintainer as time permits. There is no guaranteed response or fix deadline. Coordinate public disclosure through the private advisory.

## Supported versions

Security fixes target the latest published Tauri v2 alpha prerelease. At the time this policy was added, that release is `v2.0.0-alpha.3`. Older alpha builds and retired Swift v1 releases do not receive backported fixes. Update this policy when the supported release changes.

An alpha is not a security certification. Supported platforms and compatibility limits are documented in [Support](docs/SUPPORT.md) and [Compatibility](docs/COMPATIBILITY.md).

## Scope

Relevant reports include unauthorized access to local thread data, credential disclosure, incorrect OS credential-store use, remote environment identity or authorization failures, unsafe destination routing, and installer or update verification bypasses.

Bar reads T3 Code state without modifying its database. Bar-owned settings, caches, and grants are separate. Pairing tokens and remote grants require the same care as other credentials. A vulnerability in T3 Code or another dependency should also be reported privately to its owner when appropriate. Avoid public cross-linking that reveals the vulnerability before disclosure is agreed.
