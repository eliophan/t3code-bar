# Settings and saved preferences

Users can change notification and hover-preview preferences, manage their
connection, inspect update availability, and quit the app.

## Sub-features

- `settings-onboarding`: Settings from the unpaired connection screen.
- `settings-footer`: Settings from the paired thread-list footer.
- `settings-tray`: Settings from the native tray menu.
- `settings-notifications`: Notifications inverts the stored muted flag.
- `settings-previews`: Hover previews changes the saved preview preference.
- `settings-persistence`: Quit and reopen retain the chosen preferences.
- `settings-updates`: update status and enabled/disabled actions reflect the build.

## How to get to it (user POV)

Choose Settings on onboarding, the Settings icon in the Threads footer, or
Settings in the native tray menu. Back to threads returns to the main screen.
Quit T3 Code Bar is at the bottom of Settings.

## Driving it with Cua

Preconditions: a fresh owned run and passing doctor. The initial journey needs
no pairing; footer entry needs a paired app, and tray entry needs host UI access.

- **Onboarding navigation and Quit proof.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py navigation`.
  Require `PASS settings-onboarding-navigation-quit` and
  `navigation-result.json`: Settings opens, Back to threads restores onboarding,
  Settings opens again and native Quit terminates the owned PID. Preference
  values must remain unchanged. This is the initial end-to-end proven journey.
- **Preference-persistence acceptance.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py settings`.
  Require `PASS settings-preview-persistence` and `settings-result.json` showing
  different before/after values and identical after/relaunch values. Screenshots
  and native switch actions must agree with those files.
- **Open individually.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Settings`,
  then `python3 .agents/skills/verify-t3-code-bar/scripts/control.py snapshot`.
  Preferences, Notifications, Hover previews and Quit T3 Code Bar must be visible.
- **Notifications.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Notifications`.
  Capture the switch and read the run's settings file: an enabled Notifications
  switch means `notificationsMuted=false`. Quit and relaunch before calling
  that setting persistent. The automated preview journey does not prove this.
- **Return.** Run
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click 'Back to threads'`.
  On an unpaired run the connection onboarding reappears. On a paired run the
  thread list must appear; these are different preconditions.
- **Tray entry.** Use the host-menu recipe in tray-window.md and choose Settings.
  Confirm the same Preferences page; opening onboarding Settings is not proof
  that the tray event routes correctly.

## Gotchas

- The native WebKit tree can expose blank roles. Resolve a fresh exact label
  and advertised action; never retain a token after navigation.
- Hover previews enabled exposes `uncheck`; disabled exposes `check` in the
  tested native tree. Compare this with `showsPreviews` on disk.
- Updates are unavailable in these manual distribution builds. Disabled Check automatically,
  Install without asking and Check for Updates… are expected; do not claim an
  update installation was exercised.
- The scripted journey must begin at fresh onboarding, not an already open
  Settings page. Cleanup after failures, then launch a new isolated run.
- The initial preference-persistence attempt failed at the Hover previews click:
  Cua 0.41.3 tried X11 GetWindowAttributes on a native Wayland window. Keep it
  failed until a native switch route changes both the visible control and saved
  value. The navigation proof does not cover this switch.
