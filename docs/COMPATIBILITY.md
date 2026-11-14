# Compatibility

- macOS minimum deployment target: 14.0. Apple Silicon is the primary tested architecture; Intel and minimum-OS native acceptance remain separate checks.
- Linux x86_64: `.deb` and `.AppImage`, built on Ubuntu 24.04 with glibc 2.39. Requires WebKitGTK 4.1, GTK 3, a StatusNotifier tray host, and Secret Service for saved pairing. Wayland requires layer-shell protocol version 4. Native desktop journeys have been exercised on Omarchy; Ubuntu/Debian desktop behavior, FUSE mounting, and other desktops require separate acceptance.
- Windows: deferred.

The Bar reads T3 Code's internal SQLite projections and usage caches. Schema changes may require an update. A successful build does not prove compatibility with every T3 Code version or provider dataset.

State directory precedence is `T3_CODE_BAR_STATE_DIR`, then `T3CODE_HOME/userdata`, then `~/.t3/userdata`. Custom paths must refer to the intended local T3 Code instance. The database is opened read-only.

Wayland compositors control positioning and decorations. The app applies a process-local WebKit DMA-BUF workaround on Wayland while preserving a user-provided override. Hyprland/Hyprbars may require a scoped compositor rule to suppress plugin-drawn titlebars; application decoration settings alone cannot guarantee it. Never disable decorations globally for unrelated apps.

The Tauri bundle identity and credential namespace differ from Swift v1. Existing Swift preferences and tokens are not migrated. Older GitHub v1 release assets and installers are not Tauri artifacts.


## Multiple environments on Linux and macOS

Settings selects All environments or one environment. The selection applies to
threads, Usage, Limits and notifications, and survives restart. A missing
selected environment stays selected. Thread actions use both environment and
thread IDs. Environment icons follow the T3 setting override, server machine
metadata, then the Server fallback. Only threads belonging to another
environment show that icon; the current local environment has no machine icon.
The Bar shows the main sidebar list, excluding settled, snoozed, archived,
deleted, delegated and side-chat threads from both local and remote sources.

Linux and macOS discovery can project the installed T3 desktop connection catalog
when **Import environments from T3 Code** is enabled in Settings. It is disabled
by default, including for settings written before the preference existed. Local
pairing, manual routes and T3 Connect discovery do not require this import.
Disabling it preserves saved Bar grants; catalog-only routes require opting in
again or adding an independent manual route to reconnect.
Discovery provides metadata and routes; it does not authorize reads. Bar pairs with its
own read grant for direct, public HTTPS tunnel and SSH routes. T3 Connect sign-in
uses Bar's OAuth token to list linked environments and their public tunnel
addresses. These appear in Settings with a pairing field. A fresh code grants
Bar read access to that environment over HTTPS; Tailscale is not required.
The directory refreshes once a minute. A failed refresh keeps same-login metadata
and saved access, and account changes discard the directory cache.

The deployed Relay DPoP exchange accepts a Clerk session JWT, not Bar's OAuth
access token. Bar uses the directory and normal environment pairing instead;
OAuth sign-in alone does not grant thread access. Settings → Add environment also
accepts an HTTPS address or SSH host without cloud sign-in. No route needs to be
added in T3 Code. Synthetic Relay tests cover directory validation and the DPoP
protocol separately.
Desktop session tokens are not reused. Bar grants stay in Secret Service on
Linux and Keychain on macOS. Catalog metadata uses the desktop safeStorage key:
Linux v11 with PBKDF2-SHA1/1, macOS v10 with PBKDF2-SHA1/1003. macOS looks up only
`t3code Safe Storage` / `t3code Key`, with a bounded background lookup and a
one-minute retry delay after denial. The key is zeroized when discarded and
cached only in process memory. A Keychain approval may be required; a denied or
unavailable key leaves Local and any previously discovered sources available.

Direct HTTP is allowed only on loopback or a numeric Tailscale peer address
confirmed by the local running Tailscale daemon. This requires the Tailscale CLI
in a standard system or Homebrew location, or the installed macOS Tailscale app.
Private LAN addresses and unverified
CGNAT addresses are rejected; other direct routes require HTTPS. Bar supports
orchestration protocol versions 1 and 2 and rejects unknown versions.

Remote thread lists use the server's shell snapshot without downloading every
conversation history. Hovering a listed thread fetches only that thread's
preview.

SSH reads metadata for an already running remote T3 server and owns a local
loopback tunnel. It does not provision or start a remote server. It uses
OpenSSH with BatchMode and the existing host-key policy. A locked agent, unknown
host key or unavailable server appears offline. SSH grant storage is scoped to
the exact target and environment, independently of the tunnel's temporary port.

Settings → Add environment accepts an HTTPS server origin, a pasted pairing link,
or an SSH host with optional user and port. Bar checks the server descriptor
before asking for a key, verifies its environment ID again before redemption,
and requests only `orchestration:read diagnostics:read`. Supported pasted links
include server `/pair#token=…`, `t3code://pair?pairingUrl=…`, and the hosted
`app.t3.codes/pair?host=…#token=…` form. Bar does not register T3's scheme.

Bar-owned metadata is stored beside the settings file in a separate
`.connections.json` file, with credentials kept in the OS credential store.
Manual routes merge with the desktop catalog by environment ID and destination;
removing them never writes the desktop catalog. If the same destination serves a
different environment, Bar refuses it before submitting a key. Independent remote
access works even when Local is unpaired. Pairing codes are single-use and expire;
create a fresh code for Bar instead of reusing one redeemed by T3 Code.

A pairing key entered in Settings replaces any saved direct or SSH grant, and a
rejected key keeps the previous grant and its current live session. An expired saved grant asks for new
access and lets a later route such as Relay serve; a server denial does not.
"Forget access" in Settings removes only Bar's own direct and SSH grants for
that environment, offline and without starting a tunnel. It leaves T3 Connect
and other environments alone, clears that source's data and drops its tunnel.
Notification and menu destinations are `t3code://threads/{environment}/{thread}`
(browser form `{server}/{environment}/{thread}`), matching the upstream
`/threads/$environmentId/$threadId` route, and open only for listed threads.

Usage uses source fingerprints and bucket ownership before aggregation. Limits
uses account and workspace identities to avoid duplicating quota. Local Usage
without verified provenance is excluded. These rules follow the upstream
synthetic parity corpus; they do not establish compatibility with every live
provider response.

The bounded Bar cache stores metadata and last-good sections without routes or
credentials. Remote sections remain quarantined after restart until the same
principal is established. A fully offline remote restart can therefore show
metadata without its previous data. Revoked access clears protected sections.

Synthetic protocol tests cover direct, Relay and SSH transport boundaries.
Native Omarchy tests cover All/One threads, Usage, Limits, environment icons,
offline retention and selection across a separate process. Real cloud consent,
OpenSSH agent and host-key behavior, OS credential persistence, native
notification destination navigation, and packaged multi-environment acceptance
remain separate checks.

## T3 Code version evidence

Record exact upstream versions separately from build and native acceptance. An inspected source checkout is not a tested compatibility claim.

| Bar revision or release | T3 Code version or commit | Evidence | Status |
| --- | --- | --- | --- |
| `v2.0.0-alpha.3` | Not recorded in the published release notes | [Release notes](https://github.com/eliophan/t3code-bar/releases/tag/v2.0.0-alpha.3) record macOS 15 and Ubuntu 24.04 CI for `21310daf0d310ed54bf41685ff149d403357f348` | Exact upstream compatibility not established |
| Documentation baseline, 2026-11-14 | Local desktop manifest `0.1.7`, source commit `60514be91175f648634a7b8dd00fe45a3483feee` | Local upstream revision and manifest inspected while adding this record | Source identification only; no compatibility tests run against this revision |

For each compatibility validation, append the date, exact Bar revision, T3 Code release channel and version or commit, OS and architecture, tested features, evidence reference, and unresolved limits. Distinguish synthetic contract checks, native app checks, and packaged installation checks. Keep private captures outside the public repository. A passing row covers only its listed features and environment.
