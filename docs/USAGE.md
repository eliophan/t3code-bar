# Usage

Launch the Bar. In the T3 Code environment you want to connect, open **Settings → Sharing & access → Authorized clients → Create link** and choose read-only permissions. Choose **Copy link**, then **Paste and connect** in the Bar. Alternatively, paste the complete link into **Pairing link** and click **Connect**. The same entry connects this computer or a remote machine; a remote connection does not require a local T3 Code installation. The clipboard is read only after an explicit action, never in the background. Bar saves its own grant for later reconnects.

Bar validates the destination and identifies the environment before exchanging the code. A verified local identity uses the local connection; other identities are paired and saved as remote environments. Bar selects the connected environment automatically. Remote addresses must be reachable and follow the existing HTTPS, loopback or verified Tailscale transport policy. A localhost link copied from another computer still points to this computer: create a link with that server's reachable address. Supported formats include direct `/pair#token=…` links, `t3code://pair?pairingUrl=…` wrappers, and hosted `https://app.t3.codes/pair?host=…#token=…` links. Link format evidence: T3 Code v0.1.9, `apps/web/src/components/settings/pairingUrls.ts`.

First setup has one action: **Paste and connect** when the field is empty, or **Connect** after entering a link. A bare pairing code is insufficient here because it does not identify the target. Settings is available after pairing, not during first setup. Previously paired installations keep recovery and Settings available while offline.

Local setup does not require T3 Connect sign-in or access to T3 Code's saved environment key. Settings shows a compact **Local T3 Code connection** status. Choose **Manage connection** to open its dedicated pairing, reconnect and disconnect page; the back button returns to Settings. Local never appears in the remote pairing controls. Add a remote environment manually by address or SSH host, or sign in to T3 Connect to find linked environments.

**Import environments from T3 Code** in Settings is optional and off by default, including after upgrading from a version without this preference. Turn it on only if you want the Bar to discover T3 Code's saved connections. Reading that encrypted list may require approval for T3 Code's Keychain or Secret Service entry; it discovers metadata and routes, not read authorization. Pair separately with each environment. Turning import off stops catalog discovery without deleting Bar's saved credentials. Imported-only environments may become unavailable until you turn import back on or add their route manually. Manual connections and T3 Connect discovery are independent of this preference.

## Threads and usage

Click the tray/menu bar icon to show the thread list. Click a row to open its thread in T3 Code. Right-click a row to open it in a browser or copy its link. Hover for 500 ms to view the latest assistant reply. Enable this in Settings if a previous choice turned it off.

The footer's Usage button opens Cost, Tokens and Limits. Cost and Tokens support 24h, 7d, 30d and 90d periods. Limits shows T3 Code's provider quota snapshots; the Bar does not call providers directly.

## Settings

- Notifications and hover previews are enabled by default.
- Connected installations hide the pairing form. When disconnected, use Reconnect or Pair again.
- Disconnect asks for confirmation before removing the saved key.
- Update controls are unavailable when the build has no updater configuration.
- Quit is available from Settings or the tray menu.

The app stores settings in `tauri-bar-settings.json` under the selected T3 state directory. It does not import Swift settings or credentials.

## Troubleshooting

Open T3 Code if the Bar reports offline. If reconnect fails because a key has expired or been revoked, pair again using a fresh key. On Linux ensure Secret Service is available and unlocked. A StatusNotifier tray host must accept the app icon. Without a usable tray, the app shows a native Retry/Quit diagnostic. Start a tray host, then click Retry. GNOME needs an AppIndicator extension.

If AppImage extraction fails on a quota-limited `/tmp`, prefer direct FUSE launch. Where extraction mode is required, choose a writable temporary directory with sufficient quota:

```sh
APPIMAGE_EXTRACT_AND_RUN=1 TMPDIR=/var/tmp '/path/to/T3 Code Bar Tauri.AppImage'
```

No launch-at-login setting is currently provided; configure it through the operating system if needed.

## Linux installation

Download a `.deb` or `.AppImage` and its matching `.sha256` file from [GitHub Releases](https://github.com/eliophan/t3code-bar/releases).

For Debian or Ubuntu, run these commands in the download directory. Replace the version and CPU in the filenames when needed:

```sh
sha256sum -c 'T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.deb.sha256'
sudo apt install './T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.deb'
```

For an AppImage:

```sh
sha256sum -c 'T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.AppImage.sha256'
chmod +x 'T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.AppImage'
'./T3-Code-Bar-2.0.0-alpha.3-linux-x86_64.AppImage'
```

Stop if verification exits nonzero or reports `FAILED`. Installation and launch also report failure with a nonzero exit.

Or use the command-line installer. Requires curl, Python 3, and `sha256sum`:

```sh
curl -fsSL https://eliophan.github.io/t3code-bar/install-linux.sh | bash
```

The installer checks SHA-256, creates an application launcher, and refuses to overwrite an existing installation. A nonzero exit means installation failed. Checksums detect corruption; they do not independently authenticate the publisher.

The `.deb` installs runtime dependencies through `apt`. AppImages still need a compatible host with WebKitGTK 4.1 and GTK 3. Saved pairing needs an unlocked Secret Service, such as GNOME Keyring or a Secret Service enabled KWallet. On Wayland, the compositor must support layer-shell protocol version 4. X11 does not use layer-shell.

If an AppImage reports a FUSE error, use `APPIMAGE_EXTRACT_AND_RUN=1` when launching it. If it reports missing shared libraries or an unsupported glibc version, use a package built for your distribution. See [compatibility](COMPATIBILITY.md).

The command-line installer puts the AppImage under `${XDG_DATA_HOME:-$HOME/.local/share}/t3-code-bar`, the desktop launcher under that data directory's `applications/t3-code-bar.desktop`, and a symlink at `~/.local/bin/t3-code-bar`. If that bin directory is absent from `PATH`, run `~/.local/bin/t3-code-bar` directly.

To update an AppImage installed by the script, quit the Bar, remove those three installer paths, and run the installer again. Use the same `XDG_DATA_HOME` as the original installation. To uninstall, remove those paths without reinstalling. For a `.deb`, quit the Bar and remove its package through your system package manager. These steps keep settings and saved pairing credentials.
