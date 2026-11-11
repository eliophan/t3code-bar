---
name: verify-linux-tray
description: Model-check Linux tray policy, native diagnostic separation, coalesced recovery, and main-thread intents against compiled source. Use when changing platform_linux.rs, tray availability, window visibility, or the no-tray flow. Native Omarchy and package behavior require separate evidence.
---

# Verify the Linux tray lifecycle

Keep the product policy intact. Working content requires an observed registered
tray. Without one, the native Retry and Quit diagnostic is the permitted window.
Do not edit product code or weaken a property to make this check pass.

## Run the source and model checks

From the repository root, run with Python 3.11+, `mise`, and the Rust toolchain
from `desktop/rust-toolchain.toml`:

```sh
python3 .agents/skills/verify-linux-tray/scripts/check.py
```

Failure signals are exit **1** for a policy violation and exit **2** for source
drift, timeout, invalid models, missing observations, or tool errors. Exit **0**
requires 360 unique compiled-source observations, `FOCUS_CHECKS_PASS`, the other
source assertions, all five
safety properties, and fair eventual recovery. TLC must complete with a nonzero
state count. A timeout or incomplete exploration never counts as a pass.

The command prints a temporary artifact directory. Keep its compiled `probe.rs`,
`observations.txt`, `source-checks.txt`, generated `Probe.tla`, configs, and raw TLC
logs. Use `--out /absolute/new-directory` to choose a new or empty directory.

To confirm that the check can reject broken source, run:

```sh
python3 .agents/skills/verify-linux-tray/scripts/check.py --self-test
```

Failure signals are a nonzero exit, missing `SELF-TEST PASS`, or any mutation
without its required property failure. The command compiles eight policy mutations and the exact historical focus-loss
handler in scratch copies of actual product source. It requires each named
property to reject its specific mutation and `FocusPreservesVisiblePanel` to
reject the historical handler, deletes the scratch source, checks the product hash, and
repeats the unmodified baseline. It never edits product source.

If the verifier dependencies are missing, install the pinned tools:

```sh
mise install java@temurin-25.0.5+8.0.LTS 'http:tla2tools[url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar,checksum=sha256:936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88]@1.7.4'
```

A nonzero exit means installation failed. The runner checks the jar SHA-256.
`JAVA` and `TLA2TOOLS` may select preinstalled Java and the same pinned jar.
The probe needs no GTK session, D-Bus session, credentials, or personal data.

Read the results and counterexamples through `/statechart` with the emitted
`LinuxTray.tla`, its `safety.cfg` or `recovery.cfg`, and
[model/view.mjs](model/view.mjs). Give each run a fresh output directory.
Large passing safety graphs exceed the chart budget and produce a report only.
Replay a surprising path through the compiled source before reporting a product
bug. A disagreement is evidence to inspect the verification first.

## Preserve the checked promises

The reviewed source passes these properties:

| Property | Promise |
| --- | --- |
| `TrayRequiredForContent` | After every modeled action, visible working content implies the last applied state is Available. |
| `OnlyDiagnosticWithoutTray` | Unavailable exposes the diagnostic with working content hidden, or the application exits. |
| `HideRequiresObservedTray` | Hide while Unavailable cannot dismiss the diagnostic or reveal content. |
| `SettingsRequiresObservedTray` | Settings cannot reach working content outside Available. |
| `ResizePreservesUnavailableSurface` | Resize outside Available cannot style or change the visible windows. |
| `EventuallyRecovers` | Under fair polling and dispatch, elapsed grace, successful construction after grace, and sustained own-registration after construction, the working panel eventually opens and the diagnostic hides without Retry. |

The current safety run exhausts **224,911 distinct states**. The recovery run
exhausts **104 distinct states**. There is no trace-depth cutoff.

The old verifier reported working content as a no-tray fallback. The reviewed
implementation removes that behavior and passes a stronger content invariant
at every modeled action. The old Hide property required an error return. Hide
now deliberately returns success for an idempotent no-op while unavailable.
The safety promise remains that Hide cannot strand the user. Returning an
error is no longer used as a substitute for observing the windows.

## Review the source correspondence

The sync commit is `6adb75126be61f2bc43c68f4eb41b711768911a1`.
This review checks the changed working tree over that commit. The exact snapshot
SHA-256 and covered functions are recorded in [coverage.json](coverage.json).
Do not describe the uncommitted implementation as content of the base commit.

Source is `desktop/src-tauri/src/platform_linux.rs`.

| Product code | Verification representation |
| --- | --- |
| `TrayState`, `LinuxUi`, `Observation`, `Mailbox`, `UI` | Actual type definitions and thread-local ownership compiled in the probe. |
| `next_state`, `observe_on_main` | Actual post-grace reconstruction, state changes, forced loss hide, recovery hide, and presentation compiled verbatim. Automatic reconstruction stays inside Apply and does not wake the worker. |
| `present`, `present_diagnostic`, `hide_on_main` | Actual content gate, diagnostic construction, native delete callback, button callbacks, and Hide behavior compiled verbatim. |
| `on_window_event` | Actual handler compiled verbatim; direct source assertions cover focus sequences, queued tray activation, explicit Hide, and diagnostic preservation. Focus events are outside the TLC transition table. |
| `dispatch`, public `show`, `hide`, `resize` | Actual closures compiled with queued main-thread API doubles. Show reads state when the closure runs. |
| `rebuild_missing_tray`, `retry_on_main` | Actual missing-tray construction guard and state update compiled; Retry also sends on its bounded channel. Construction success or failure is supplied by an API double. |
| `build_tray` primary-click callback | Exact callback body compiled with typed tray-event doubles; Left Up dispatches the existing gated Show. The native builder and event transport remain API boundaries. |
| `publish` | Actual mutex mailbox, coalescing, dispatch, and drain closure compiled and exercised by source assertions. |
| `initialize_surface`, `layer_bar.c` | Reviewed startup boundary before tray construction and worker initialization. Configures the GTK layer surface without showing content. Native Wayland support, layer placement, keyboard mode, and realization timing are outside the model. |
| `initialize` worker | Manually reviewed scheduler mapping to `ReadHost`, `Publish`, and the one-way grace timer. Probe and publication remain separate steps. |
| Main-thread queue | At most two pending callbacks and one active callback. `Dequeue` releases the mailbox lock before `Apply`; the worker can publish between those steps. |
| `apply` | API double records invocation and can fail. The reviewed real function changes geometry and decoration but never shows a window. |
| `focus_grab::presented` | Typed no-op API double; presentation still compiles from actual source. No native grab lifecycle or outside-click delivery claim. |
| `build_tray`, D-Bus, GTK, Tauri, telemetry | API boundaries, not compiled real implementations. Their native behavior requires separate acceptance. |

The probe executes 24 input combinations across 15 actions. Observation actions
distinguish constructor success and failure after grace. Tray activation uses the
actual primary-click callback. The probe generates the
model's complete effect table from actual Rust execution. The table distinguishes
working content, visible diagnostic, tray construction, exit, Settings, and
geometry invocation. An already queued native delete event retains its actual
callback after programmatic recovery hide.

Direct focus assertions replay all 126 nonempty true/false sequences through
length six across visible Available content, hidden Available content, and the
Unavailable diagnostic (378 sequences total). They drain queued work after each
event and require unchanged window effects; explicit Hide must still hide
Available content and preserve the Unavailable diagnostic. A separate tray
activation replay delivers focus loss before and after its queued Show and
requires that focus queues no Hide. `FOCUS_CHECKS_PASS` is required alongside
`SOURCE_CHECKS_PASS`; these are direct compiled-source assertions, not an added
TLA+ invariant or proof of native focus delivery. The self-test restores the
exact prior handler and checks that exact handler against its recorded
SHA-256 before requiring the visible-panel assertion to fail.

Additional source assertions cover queued Show after loss, positive then negative
mailbox coalescing, failed scheduling, failed hide, failed style or show, focus
failure after successful show, repeated Retry during a pending probe, and close
after recovery. They also cover initial constructor failure, a failed
automatic attempt, later construction followed by own registration, no additional
wake or publication from automatic rebuilding, no rebuild once created, ignored
non-primary events, and queued activation after registration loss. Retry and Quit
invoke the actual diagnostic button callbacks.
Programmatic hide must not invoke the diagnostic delete callback.

The scheduler is still a reviewed hand-written TLA+ description. A source hash
is a drift alarm, not proof that arbitrary future code matches that scheduler.

## Respect the bounds and exclusions

Safety includes arbitrary ordered registration samples, startup grace,
construction failure, successful and failed automatic rebuild and Retry, queued
activation, Show and Settings,
Hide, resize, diagnostic close, Quit, and mailbox overwrite. At most one drain is
queued, with two total queued callbacks and one active main-thread callback.
The old queue bound of two is retained. The callback shapes changed because the
source now coalesces observations and accepts user intents through dispatch.

Exhaustive safety assumes successful native window calls, a working main-thread
queue, an unpoisoned mailbox mutex, and the normal single polling worker. Source
assertions separately exercise reported API failures. An exit request is treated
as terminal. Process termination itself is not proved.

Recovery checks disable user actions and manual Retry. They assume elapsed grace,
a successful native constructor on future post-grace attempts, and sustained
own-registration after construction. Before construction, recovery samples remain
negative; a later polling cycle supplies the positive registration. Weak fairness
covers grace expiry, polling, publication, dequeue, and application. This describes
the suffix after the host permits construction; safety separately permits any
number of failed attempts, and source assertions replay failure then success.
The property is unchanged; its recovery environment now exercises automatic
rebuilding without a manual Retry escape. It proves eventual recovery under these
conditions, not a time deadline or recovery after arbitrary user actions.

Availability means the last applied registration observation. A host can vanish
between polls. This model does not prove continuous physical icon visibility,
D-Bus ownership or timeout behavior, native constructor success or automatic
native tray re-registration,
unbounded dispatch backlogs, GTK focus or pointer behavior, diagnostic mapping
failure, compositor geometry, WebKit rendering, second-instance transport,
telemetry counters, persistence, pairing, notifications, or packaging. The early
`initialize_surface` call and `layer_bar.c` are reviewed native boundaries, not
compiled into the policy probe. The model assumes successful startup; it does
not prove layer-shell protocol support, GTK realization order, surface role,
anchor/margin geometry, keyboard focus, or startup error presentation. Main close,
Escape, and toggle use the reviewed Hide gate; their event routing remains
outside the generated transition table. Focus events now preserve panel state
and are covered by the direct assertions above. Native focus and pointer
delivery still require live acceptance. The Hyprland focus-grab Rust/C backend,
Wayland cleared delivery, idle callback generation guard, draw scheduling, and
unmap cleanup are outside this model and its focus-sequence assertions. Those
assertions prove that Tauri focus changes themselves preserve content; they do
not prove that an outside click hides it. Check both delivered package
formats on Omarchy separately.

## Update after source changes

1. Run the check before editing verification and record its result.
2. Compare every changed covered function and all callers that can show content.
3. Preserve each concurrent boundary in the scheduler. Keep GTK ownership and
   worker publication separate. Model a diagnostic separately from content.
4. Update extraction, doubles, and actions to match the code. Retain properties
   unless an intentional product promise changed and you can cite its evidence.
5. Review correspondence before updating the hash and sync records. Record a
   working-tree snapshot explicitly if the reviewed source is uncommitted.
6. Run the ordinary check, then `--self-test`. Require a real-source mutation
   failure for each passing property and an unchanged restored baseline.
7. Report model drift separately from source bugs, and keep native and package
   acceptance separate from the formal result.
