# Compatibility

- macOS minimum deployment target: 14.0. Apple Silicon is the primary tested architecture; Intel and minimum-OS native acceptance remain separate checks.
- Linux x86_64: `.deb` and `.AppImage`, built on Ubuntu 24.04 with glibc 2.39. Requires WebKitGTK 4.1, GTK 3, a StatusNotifier tray host, and Secret Service for saved pairing. Wayland requires layer-shell protocol version 4. Native desktop journeys have been exercised on Omarchy; Ubuntu/Debian desktop behavior, FUSE mounting, and other desktops require separate acceptance.
- Windows: deferred.

The Bar reads T3 Code's internal SQLite projections and usage caches. Schema changes may require an update. A successful build does not prove compatibility with every T3 Code version or provider dataset.

State directory precedence is `T3_CODE_BAR_STATE_DIR`, then `T3CODE_HOME/userdata`, then `~/.t3/userdata`. Custom paths must refer to the intended local T3 Code instance. The database is opened read-only.

Wayland compositors control positioning and decorations. The app applies a process-local WebKit DMA-BUF workaround on Wayland while preserving a user-provided override. Hyprland/Hyprbars may require a scoped compositor rule to suppress plugin-drawn titlebars; application decoration settings alone cannot guarantee it. Never disable decorations globally for unrelated apps.

The Tauri bundle identity and credential namespace differ from Swift v1. Existing Swift preferences and tokens are not migrated. Older GitHub v1 release assets and installers are not Tauri artifacts.


## Multiple environments on Linux

Settings selects All environments or one environment. The selection applies to
threads, Usage, Limits and notifications, and survives restart. A missing
selected environment stays selected. Thread actions use both environment and
thread IDs. Environment icons follow the T3 setting override, server machine
metadata, then the Server fallback. Only threads belonging to another
environment show that icon; the current local environment has no machine icon.
The Bar shows the main sidebar list, excluding settled, snoozed, archived,
deleted, delegated and side-chat threads from both local and remote sources.

Linux discovery projects the installed T3 desktop connection catalog. Discovery
provides metadata and routes; it does not authorize reads. Bar pairs with its
own read grant for direct and SSH routes, or signs into T3 Connect for Relay.
Desktop session tokens are not reused. Credentials stay in Secret Service.
Other platforms currently discover only the configured local source.

Direct HTTP is allowed only on loopback or a numeric Tailscale peer address
confirmed by the local running Tailscale daemon. This requires the Tailscale CLI
in a standard system or Homebrew location. Private LAN addresses and unverified
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

A pairing key entered in Settings replaces any saved direct or SSH grant, and a
rejected key keeps the previous grant. An expired saved grant asks for new
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
