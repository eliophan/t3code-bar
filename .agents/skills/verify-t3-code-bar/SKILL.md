---
name: verify-t3-code-bar
description: Launch and drive the real Tauri T3 Code Bar on Omarchy/Linux with isolated synthetic state, native Cua/AT-SPI actions, screenshots and persisted-settings evidence. Use for native UI verification; does not replace tray pointer, packaged-release or credential acceptance.
---

# Verify T3 Code Bar on native Linux

Read [features/README.md](features/README.md) before choosing a journey. The
primary surface is the native Tauri app with a GTK/WebKit UI and StatusNotifierItem
tray. Vite alone and React tests do not exercise its native host. The companion
reads T3 Code state; it does not own the T3 Code server or `t3code://` scheme.

The Linux product requires a tray. Without one, only the native Retry/Quit
diagnostic may be visible. The separate `verify-linux-tray` skill checks a
bounded lifecycle model, not this GUI.

## Launch

Work from the repository root in the logged-in Omarchy session. Required:
Python 3.11+, `mise`, Node 26.13.0 through mise, the repo's pinned Rust toolchain,
Tauri Linux prerequisites, `busctl`, a functioning tray host, and `cua-driver`.
The initial verified run used Cua 0.41.3, GTK 3.24.52 and WebKitGTK 2.54.1.
Use the installed driver; do not upgrade it or alter desktop security settings
to make a run pass. Consult the `cua-driver` skill for a changed driver schema.

The repository's documented development command is `npm run tauri -- dev`
from `desktop/`, which uses Vite on 127.0.0.1:1420. This skill instead uses the
same Tauri CLI to embed the frontend in a release-mode native binary without
bundling; it needs no development server or port.

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/control.py build
python3 .agents/skills/verify-t3-code-bar/scripts/control.py launch
```

Failure signals: either command exits nonzero. Build must print `BUILD READY`
and produce `.audit/verify-t3-code-bar/build.json`; launch must print `READY`
only after the exact owned process exposes usable `Connect T3 Code` onboarding,
valid settings, and its own registered tray item. A process or window existing
alone is not readiness. Build logs are `.audit/verify-t3-code-bar/build.log`.

`build --target-dir /var/tmp/t3-linux-current-86156d8/target` was the exact
cache override used in the initial end-to-end run. The default uses the
checkout-local `.audit/verify-t3-code-bar/target`; no cache contents are trusted
instead of running the build. Launch verifies the build receipt, commit,
product files including untracked additions, diff and binary hash, then copies
the executable into the run. Old receipts lacking the file manifest require a rebuild.

Launch creates an empty temporary state directory, seeds
`desktop/tests/fixtures/threads.sql` into `statev2.sqlite`, copies the synthetic
Usage fixture files, and lets the real app create its own settings. It sets
`T3_BAR_FIXTURE_STATE`, `T3_BAR_FIXTURE_SETTINGS`, frozen time
`2026-11-06T09:08:00Z`, and UTC. It removes inherited `T3_BAR_*` overrides, so no
keyring namespace or notification opt-in leaks into the run. Do not set HOME
or use a personal T3 Code database.

This existing fixture boundary substitutes the credential store with memory,
disables local personal Usage discovery and notifications by default. It does
**not** simulate successful pairing or intercept native open-link actions.
The fixture has no pairing server/token, so the initial usable surface is
onboarding and Settings, not a populated Threads/Usage page. Do not bypass
that gate through IPC, DevTools or synthetic snapshots.

The owned Cua daemon uses a unique temporary socket and standard permissions
with `CUA_DRIVER_RS_ENABLE_WAYLAND=1`. The app receives per-process
`GTK_MODULES=gail:atk-bridge`, `GTK_A11Y=always`, `NO_AT_BRIDGE=0`; without those
the initial native accessibility read was incomplete. These are launch
environment settings, not changes to Omarchy config or system services.

## Doctor

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/control.py doctor
```

Failure signal: nonzero exit. Run it whenever anything looks wrong. It checks
PID start time, binary hash, settings readability, the owned native window,
usable accessibility content, tray-host presence and a tray item whose D-Bus
owner PID matches this exact app. It does not activate controls or repair
anything. Registration is not evidence that the panel visibly displays the
icon. It also reads only the owned process's fixture environment keys to verify
the actual paths and absence of keyring/notification opt-ins. The output records
the revision, version, hash and isolated state path.

## Drive

Use `cua-driver` native accessibility against the owned PID/window. The helper
always refreshes a snapshot, resolves exactly one actionable label, and clicks
its snapshot-bound token through `run_steps`. WebKit can expose blank role
names, so exact accessible labels and advertised actions are used instead of
invented button roles or fixed coordinates.

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/control.py snapshot
python3 .agents/skills/verify-t3-code-bar/scripts/control.py click Settings
python3 .agents/skills/verify-t3-code-bar/scripts/control.py click 'Hover previews'
```

Failure signal for each command: nonzero exit, an incomplete accessibility
tree, an unattested screenshot, or a refused/failed native action. A successful
action acknowledgement is not a feature verdict; check the resulting UI and
stored value. Cua refusals remain failures. Do not fall back automatically to
global input, another window, DOM setters, or direct Tauri command invocation.

Run the native Settings navigation/Quit journey from freshly launched onboarding:

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/control.py navigation
```

Failure signal: nonzero exit or missing `PASS settings-onboarding-navigation-quit`.
This captures onboarding, opens Settings, confirms both preference controls and
Quit, returns through Back to threads, captures onboarding again, reopens
Settings and clicks Quit. It requires the owned PID to terminate and verifies
that navigation did not modify persisted preferences. Cleanup is still
required to stop the owned Cua daemon and remove scratch state.

The separate preference-persistence journey remains an acceptance check:

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/control.py settings
```

Failure signal: nonzero exit or missing `PASS settings-preview-persistence`.
This attempts to open Settings, capture the initial switch, toggle Hover previews through
the real UI, checks the real settings file and native switch action, clicks
Quit, confirms process termination, launches a new process with the same
scratch state, reopens Settings and confirms the saved switch and file agree.
On a pass it leaves the restarted app open for capture/cleanup. No setting is
written directly by the helper. Each failure still requires cleanup before
retrying. **Known failure on the initial run:** Cua 0.41.3 returned
`AT-SPI element click failed: X11 error ... GetWindowAttributes` for the native
Wayland Hover previews switch. This journey exited nonzero and its evidence
was retained. Navigation is a different proof, not a replacement pass for
persistence. Do not bypass this failure with a settings-file write.

## Evidence

Every launch prints its retained evidence directory:
`.audit/verify-t3-code-bar/runs/` followed by the run ID. Artifacts stay local
and ignored by Git. The active pointer is `.audit/verify-t3-code-bar/active.json`.
Capture:

- `run.json` and `build.json`: exact PID/start time, revision, executable hash,
  fixture location and tracked source diff.
- `step-*.json`: before-action trees, requested labels, native action results,
  resulting trees and read-only settings observations.
- `step-*.png`: Cua's attested window screenshots; no desktop crops.
- `app.log`, `driver.log`, journey result JSON on success, and final settings copy.

A proof needs action **and** result. Settings additionally needs the written
file and a second-process readback. Tray needs visible host/menu interaction;
D-Bus registration or calling a menu event directly cannot prove pointer use.
Pairing needs actual transport and separate-process credential checks; the
fixture's in-memory store cannot prove OS credential persistence. Thread
opening and notifications need their destination verified, not just a log.
No browser mock or internal IPC setter counts as native acceptance.

Initial proven scope: native release-mode binary on the actual Omarchy session,
Settings entered from unpaired onboarding, Back to threads, re-entry and native
Quit with process termination. Hover previews persistence is a recorded
driver-route failure. This is not an installed `.deb` or AppImage run, and no
tray icon pointer/menu action, live pairing, Usage/Limits or notification route
was certified. Record each other journey as unverified or failed with its unmet
precondition; do not substitute the Settings proof for those entry points.

## Cleanup

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/control.py cleanup
```

Failure signal: nonzero exit, surviving owned PID, or missing retained
observation artifacts. Run cleanup after successful **and failed** attempts.
It ends only this Cua session, stops only process groups whose recorded PID and
start time still match, copies settings to evidence, removes temporary runtime
state and the active pointer, and writes `cleanup.json`. Build cache and proof
artifacts remain. It never kills by process name, stops a shared Cua service,
restarts Quickshell, or changes Hyprland configuration.

Cleanup compares every seeded input's SHA-256 with its pre-launch value. A
changed database/cache is a failure, with the changed file retained for review.
The real app's settings file is a separate expected output. This observes
fixture file behavior; it is not a network trace or proof that every external
side effect is absent.

This app uses a single-instance plugin on the shared desktop session. Separate
fixture directories do not isolate that lock or global focus. Launch refuses
another detected Bar process and an existing owned run. Keep one controller;
never drive a user-owned instance. Two worktrees must not run these journeys
at the same time.

## Helpers

The executable [scripts/control.py](scripts/control.py) implements `build`,
`launch`, `doctor`, `snapshot`, `click`, `navigation`, `settings`, and `cleanup`, as invoked
above. It uses Python's standard library and the existing native Cua CLI; no
test-only endpoint, browser server or app code modification is added.

After a relevant UI or lifecycle change, use `/maintain-verification-skill` to
keep the feature map and actual native evidence aligned.

## Verify packages and recovery

Build with `scripts/package-linux.py`, then inspect/extract the package without
installing it. Launch the executable from that extraction using its receipt:

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/control.py launch --package-receipt /var/tmp/t3-linux-tray-required-final/receipt.json --package-binary /var/tmp/t3-linux-tray-required-final/deb-extracted/usr/bin/t3-code-bar-desktop
```

Failure signal: nonzero exit or absent READY. The helper checks all archive
hashes and uniquely matches the extracted executable to a package-specific
hash. Tauri patches the bundle type, so the deb, AppImage and original build
binary may have different hashes. For the actual AppImage launcher, add
`--appimage` with the original verified `.AppImage` and use its extracted
binary for `--package-binary`. The helper sets `APPIMAGE_EXTRACT_AND_RUN=1`,
finds the exact verified child in its owned process group, and retains launcher
ownership for cleanup. This proves that mode, not FUSE mounting.

`--bus-address` selects an owned private D-Bus for the app. Cua stays on the
desktop accessibility bus. Never use it to disrupt the user's tray service.
[scripts/watcher.py](scripts/watcher.py) is a synthetic SNI protocol fixture:
run `/usr/bin/python3 .agents/skills/verify-t3-code-bar/scripts/watcher.py
--address PRIVATE_ADDRESS --host-file OWNED_FILE`, with an owned file containing
`0` or `1`. Nonzero exit or absent READY is failure. It requires Python Gio,
reports registrations, and changes host presence when that file changes. It
provides no visible icon and cannot certify pointer/menu behavior. Track and
stop its PID/start time and its private dbus-daemon separately after cleanup.

With no host, doctor accepts only Retry/Quit and rejects working content. With
a registered own item it requires working content. Capture diagnostic, Retry,
second launch, automatic recovery, host loss, Quit and compositor close. Use
the native titlebar close dispatcher for the exact owned window if Omarchy
does not draw an X; label that evidence as a close request, not a pointer X.

For the actual Omarchy tray, use a fresh Cua desktop capture, expand the tray,
right-click the observed Bar icon, then capture the resulting menu. Use the
exact installed click schema (`button: "right"` with a desktop target worked
on Cua 0.41.3). Capture each menu action and resulting visibility or process
exit. Never reuse these run's pixel positions as a portable recipe.

The 2026-11-09 packaged deb run `4ba84cf0afe0` proved real Omarchy tray Show,
Hide, Settings and Quit, second launch, and Hover previews persistence across
a separate process. Window foreground input refused because the production
Hyprland plugin was unavailable. The authorized desktop input route changed
`showsPreviews` from true to false and the native tree/file agreed after restart.
The earlier semantic input failure remains recorded. Private-bus packaged run
`ff99917f6e8b` proved diagnostic, Retry, second launch, automatic recovery, host
loss, diagnostic Quit and compositor close. Both cleanups retained evidence.
Pairing, OS credentials, populated Threads/Usage and notifications remain
unverified by these unpaired synthetic journeys.

Actual AppImage extract-and-run `a60a82b88834` passed Settings/Back/Quit.
`cdb074fad238` additionally passed disappearance and recreation of the private
watcher service, automatic own-item re-registration, and diagnostic recovery
without Retry. Both runs checked the launched child executable hash and cleaned
up launcher, app, and owned Cua daemon. FUSE mounting remains unverified.

Linux focus events preserve the panel. On Hyprland, `hyprland_focus_grab_v1`
provides compositor-driven dismissal. An actual outside press dismisses; pointer
movement across other windows and internal entry clicks preserve the panel.
The protocol is optional; other compositors retain explicit Hide controls.

`control.py primary --icon-x X --icon-y Y --outside-x X --outside-y Y` verifies
three tray-open, outside-hover, outside-click-dismiss cycles. Coordinates must
come from a fresh 1568-wide desktop capture. For a tray that automatically
collapses after each outside click, supply `--expand-x X --expand-y Y` from the
observed collapsed arrow. Begin with that tray collapsed. The helper captures
every action; it does not discover tray geometry or assert icon identity.

The 2026-11-09 release receipt at
`/var/tmp/t3-linux-outside-click-release/receipt.json` identifies both tested
packages and their extracted executable hashes. Actual AppImage extract-and-run
`1613764bceba` and extracted deb executable `fc002c584e72` each passed three
primary/hover/outside-dismiss cycles. Both also accepted a native click and
synthetic typing in the masked pairing field without closing; Connect became
enabled and was never pressed. Window screenshots and action records are in
those run directories. The first attempt wrongly expected the password value
in the accessibility tree; the screenshot verifies masked input instead. Another
initial primary loop failed because Omarchy automatically collapsed its tray;
the corrected loop explicitly expands the observed tray each cycle.

Private-bus packaged deb run `44cefcb5fb68` passed host loss with an already
mapped panel and automatic recovery. This uses a synthetic SNI watcher and does
not certify visible icon behavior. All three runs cleaned up owned processes
and verified unchanged seeded data. Package archives contain the exact vendored
protocol license. Deb installation, FUSE mounting, other desktops, populated
paired data and OS credential storage remain outside this acceptance.


## Omarchy layer bar acceptance

The main Linux surface now uses gtk-layer-shell on supported Wayland compositors.
It is not an ordinary client in `hyprctl clients`. `control.py` identifies its
layer by the exact owned PID in `hyprctl -j layers`. Cua 0.41.3 cannot discover
this layer as a window, so it cannot issue a truthful window handle or element
token for it. The helper uses [layer_accessibility.py](scripts/layer_accessibility.py)
for bounded, read-only AT-SPI data from that same PID. It never invents Cua tokens.
Use fresh Cua desktop captures and observed desktop pointer coordinates for
input. `click`, `navigation`, and `settings` semantic journeys explicitly refuse
a layer surface; their older ordinary-window evidence remains historical.

Layer snapshots pair the native AT-SPI tree and compositor geometry with a full
Cua desktop capture. These are full desktop images, not attested individual
window screenshots or crops. Native diagnostic windows still use the original
Cua window route. No permission profile or desktop service is changed.

```sh
python3 .agents/skills/verify-t3-code-bar/scripts/layer_geometry.py --pid OWNED_PID --out /absolute/evidence.json
python3 .agents/skills/verify-t3-code-bar/scripts/layer_accessibility.py --pid OWNED_PID
```

Failure signals are nonzero exit, absent unique owned visible layer/tree, an
ordinary client for the working panel, clipping, unexpected width, or placement
outside the upper-right region. The geometry check was red on the old app
(`geometry-before.json`) and green on the delivered layer surface. Neither read
alone proves input or tray interaction.

The final source receipt is `/var/tmp/t3-linux-omarchy-bar-verified/receipt.json`.
Actual AppImage extract-and-run `261a70d7180f` proved owned bundled library
resolution without the development prefix, upper-right layer geometry,
hover/internal typing, dynamic Settings height, and three tray-open/outside-hide
cycles. Extracted deb run `274869b34cf5` proved the same tray/input behavior and
live appearance updates from an isolated XDG theme fixture. Theme background
pixels changed and restored, while larger text increased the native panel
height and restored it. The real desktop theme was untouched. Private-bus deb
run `cfd5e9c11517` proved tray loss and automatic recovery with a mapped layer.
All fixture runs cleaned up owned processes and kept seeded data unchanged.

The deb executable was tested with the isolated gtk-layer-shell runtime from
`/var/tmp/t3-layer-deps/usr/lib`; no system package was installed. Its archive
declares `libgtk-layer-shell0`, and the AppImage bundles the shared library.
Both carry the required notices. Evidence is collected under
`.audit/linux-layer-bar/`. Other monitor arrangements, other compositors, deb
installation, FUSE mounting and paired production data remain outside this run.

## Linux color and preview acceptance

The current color module is `desktop/src-tauri/src/linux_appearance.rs` and the
CSS namespace is `linux-desktop` / `--linux-*`. Omarchy and GTK supply color
inputs to one contrast-normalization step. Neither uses the T3 Code sidebar.
The current receipt is `/var/tmp/t3-linux-readable-ui-final/receipt.json`.

AppImage run `0c37ad76afd8` used an isolated loopback pairing fixture through
native input to reach the real Threads UI. Native hover showed Last reply in
both dark and light theme screenshots; an isolated XDG theme changed live and
restored. Settings in light mode was inspected. Static preview text was absent
from AT-SPI, so the screenshot and native preview fetch are the evidence rather
than a fabricated tree label. Usage reached only its empty state.

Run `da3f84ae0cfd` omitted Omarchy palette files and showed the app's GTK font
and Adwaita dark colors. AppImage's standard GTK hook overrides GTK_THEME;
use its supported APPIMAGE_GTK_THEME option for a per-process theme test. Run
`fa58b7dcecb3` used HighContrast and showed the GTK light palette. These are GTK
source tests on Omarchy, not separate GNOME/KDE/XFCE acceptance. All fixture
runs cleaned up with unchanged seed hashes. Details and retained limitations
are in `.audit/linux-readable-ui/completion.md`.

## Native launch environment and thread opening

Never inherit `ELECTRON_RUN_AS_NODE` into a GUI Bar launch from an Electron-based
agent host. `app_env` removes it. Even though Bar is Tauri, its external URL
handler can be Electron, and the inherited flag makes that handler run the URI
as a Node module. An xdg-open exit of zero still does not prove navigation.

The same packaged AppImage failed native thread opening with that inherited
flag and passed after removing only the flag at Bar startup. Reverify evidence
is `.audit/linux-thread-reverify/completion.md`: native clicks switched between
two existing threads and restored the original; screenshots confirm T3 Code
header/content, not just launcher success. That diagnosis used normal app data
under the user's explicit bug-verification request; default fixture journeys
still require isolated destinations and must not silently open personal threads.
