# Tray and window lifecycle

The tray is the required Linux entry point. It opens or hides the panel,
opens Settings, and lets the user quit without leaving an inaccessible process.

## Sub-features

- `tray-registration`: the host contains this process's tray item.
- `tray-show-hide`: Show T3 Code Bar and Hide T3 Code Bar change panel visibility.
- `tray-settings-quit`: Settings routes to preferences; Quit T3 Code Bar exits.
- `panel-escape`: Escape navigates back, dismisses a context menu or hides Threads.
- `second-launch`: launching again reopens the existing process's panel.
- `tray-required`: no host exposes only a diagnostic Retry/Quit surface.
- `tray-host-loss`: loss and return do not strand the app invisibly.

## How to get to it (user POV)

Open the T3 Code Bar icon's native menu on the desktop panel. Choose
Show T3 Code Bar, Hide T3 Code Bar, Settings or Quit T3 Code Bar. Inside the app,
Escape acts on the current screen. Opening the application again should reach
the running instance.

## Driving it with Cua

Preconditions: an owned app, passing doctor and observable tray-host UI. Input
on Quickshell is desktop input, not input on the main Tauri window. Follow the
installed cua-driver skill for admitted desktop scope and live tool schemas.

- **Registration.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py doctor`.
  Require the exact app PID's registered item. This is supporting evidence only.
- **Open the real menu.** Take a fresh Cua desktop/host snapshot and locate the
  T3 Code Bar icon by its current accessible label or attested pixels. Use a
  snapshot-bound Cua action on that host surface. If the host exposes neither
  a semantic control nor an admitted screenshot/input route, report this entry
  as blocked. Never guess coordinates or send a D-Bus menu event as a substitute.
- **Exercise menu labels.** In the visible menu choose Show T3 Code Bar, then
  Hide T3 Code Bar, then Show again. Observe the owned app PID/window after
  each action and capture the host menu before selection. Choose Settings and
  require Preferences. Choose Quit T3 Code Bar and require that PID to exit.
  `control.py click` intentionally targets the main app, so it is not a tray
  host action helper.
- **Quit from Settings separately.** From the owned app's Settings use
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click 'Quit T3 Code Bar'`.
  Require process termination. This proves a separate entry, not tray Quit.
- **Loss/no-host.** Use a separately owned graphical test session/tray host to
  withdraw and restore the host. Do not stop the user's Quickshell or change
  their shell config. With no such isolated session, record these journeys as
  blocked. Run `verify-linux-tray` for bounded model evidence separately.

## Gotchas

- Without a host, require only the native Retry/Quit diagnostic. Working
  content must stay hidden. An owned private bus can test protocol recovery
  without changing Quickshell; see the parent skill for the watcher fixture.
- Tray registration does not prove icon visibility or pointer delivery.
- Tauri Linux does not supply macOS-like tray click/hover events. The native
  menu is the supported interaction to verify.
- Linux keeps the panel open across focus changes so pointer travel into an
  input cannot dismiss it. Use tray Hide, Escape, or window close to hide it.
- A second process shares the single-instance lock. Never attempt this journey
  while another controller or user-owned Bar is running.

Packaged evidence on 2026-11-09: `4ba84cf0afe0` covers the real Omarchy menu,
second launch and saved previews. `ff99917f6e8b` covers the deb diagnostic and
recovery. `cdb074fad238` launches the original AppImage, terminates its private
watcher service, recreates it, and confirms automatic item re-registration and
working-panel recovery without Retry. All runs cleaned up their owned apps.

The packaged AppImage primary-click regression run `11f378cb7154` passed three
cycles of outside-click hide followed by the exact fresh tray icon left-click.
The panel was visible at 200 ms and 1 s after every click.

Focus regression proof `4be1aa81f7d9` uses the actual focus-fixed AppImage.
After primary tray activation, moving the real desktop pointer across another
window into Pairing key, clicking it, and typing synthetic text kept the panel
visible. Three pointer cycles passed; Connect became enabled and was not clicked.
Use `/var/tmp/t3-linux-focus-final/receipt.json` for that package.
