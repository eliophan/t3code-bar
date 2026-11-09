# Compatibility

- macOS minimum deployment target: 14.0. Apple Silicon is the primary tested architecture; Intel and minimum-OS native acceptance remain separate checks.
- Linux x86_64: `.deb` and `.AppImage`, built on Ubuntu 24.04 with glibc 2.39. Requires WebKitGTK 4.1, GTK 3, a StatusNotifier tray host, and Secret Service for saved pairing. Wayland requires layer-shell protocol version 4. Native desktop journeys have been exercised on Omarchy; Ubuntu/Debian desktop behavior, FUSE mounting, and other desktops require separate acceptance.
- Windows: deferred.

The Bar reads T3 Code's internal SQLite projections and usage caches. Schema changes may require an update. A successful build does not prove compatibility with every T3 Code version or provider dataset.

State directory precedence is `T3_CODE_BAR_STATE_DIR`, then `T3CODE_HOME/userdata`, then `~/.t3/userdata`. Custom paths must refer to the intended local T3 Code instance. The database is opened read-only.

Wayland compositors control positioning and decorations. The app applies a process-local WebKit DMA-BUF workaround on Wayland while preserving a user-provided override. Hyprland/Hyprbars may require a scoped compositor rule to suppress plugin-drawn titlebars; application decoration settings alone cannot guarantee it. Never disable decorations globally for unrelated apps.

The Tauri bundle identity and credential namespace differ from Swift v1. Existing Swift preferences and tokens are not migrated. Older GitHub v1 release assets and installers are not Tauri artifacts.
