# Usage

Start T3 Code, then launch the Bar. Enter a pairing key from T3 Code to connect. The key is saved in the Bar's own OS credential namespace for later reconnects.

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
sha256sum -c 'T3 Code Bar Tauri_2.0.0-alpha.1_amd64.deb.sha256'
sudo apt install './T3 Code Bar Tauri_2.0.0-alpha.1_amd64.deb'
```

For an AppImage:

```sh
sha256sum -c 'T3 Code Bar Tauri_2.0.0-alpha.1_amd64.AppImage.sha256'
chmod +x 'T3 Code Bar Tauri_2.0.0-alpha.1_amd64.AppImage'
'./T3 Code Bar Tauri_2.0.0-alpha.1_amd64.AppImage'
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
