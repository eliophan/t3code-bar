# Using T3 Code Bar

Reference for thread states, configuration, troubleshooting, and known limitations. For installation, see the [README](../README.md).

## Thread states

| State | Meaning |
| --- | --- |
| Approval / Input | The thread is waiting for you to approve an action or answer a question. |
| Request | A guest on a shared link has a message waiting for you to send or dismiss. |
| Working / Goal | A run is starting, running or paused on a wait. T3 Code shows all of these as Working. |
| Waiting | The thread has never run, or its run is only waiting on other threads (`t3_thread_wait`, or `delegate_task` in wait mode). Gray, and not counted as running. |
| Failed | The latest visible run failed. |
| Limited | The latest visible run failed because the provider's usage limit was reached. Shown in amber, as in T3 Code, and not as a failure. When the failure records a reset time, the row shows it (for example `Limited · 2h`) and the tooltip says when it resets; many failures record none. |
| Woke | A snoozed thread has woken and its wake timestamp is newer than the persisted last visit, or no valid visit is available. |
| Done | The latest visible run completed after the last valid visit saved by T3 Code. |
| Relative time | No higher-priority status applies; the row shows time based on the latest user message. |

Done clears when T3 Code saves a newer visit timestamp. If no valid visit timestamp is available, the app shows time rather than assuming a completion is unread. Clicking a link in Bar does not itself mark a thread as read.

Archived, deleted, and settled threads, side chats, and separate subagent threads are excluded. Snoozed threads stay hidden until the timer expires or a new attention event wakes them. An error or completion that already existed when the thread was snoozed does not wake it early.

## Opening threads

Click a thread to open it in T3 Code. Right-click a thread for **Open in T3 Code**, **Open in Browser**, or **Copy Link**.

## Previews and the menu bar icon menu

Rest the pointer on a thread for half a second and its row expands to show the start of the thread's last assistant reply (up to five lines, code blocks left out) and how long ago it was sent. Moving away closes it. Previews read only the `messages` table, never the large turn items, and show real message text, so turn them off with **Show Previews** in the menu you get by right-clicking the menu bar icon. The same menu has **Mute Notifications** and **Update Automatically**.

Browser links use `127.0.0.1` and the port configured by your local T3 Code instance. Your browser session may need sign-in or pairing. Opening links in T3 Code requires its `t3code://` URL scheme to be registered.

## Launch at login

The app has no Dock icon. To launch it at login, add `~/Applications/T3 Code Bar.app` under **System Settings → General → Login Items → Open at Login**.

## Configuration

By default, the app reads `~/.t3/userdata`. Override it using one of these environment variables, in priority order:

| Variable | Expected directory |
| --- | --- |
| `T3_CODE_BAR_STATE_DIR` | Contains `statev2.sqlite`, `server-runtime.json`, and `environment-id` directly. |
| `T3CODE_HOME` | T3 Code's root directory; Bar appends `/userdata`. |

For example, from the source directory:

```bash
T3CODE_HOME="$HOME/.t3-custom" swift run T3CodeBar
```

Apps opened from Finder or Login Items do not automatically inherit your terminal's environment. For custom data directories, launch from Terminal or configure a launcher that passes these variables.

## Troubleshooting

| Problem | What to check |
| --- | --- |
| Data not found | Open T3 Code at least once and check the configured data directory. |
| Unsupported schema | Your T3 Code version may use a different database structure. See the [compatibility notes](COMPATIBILITY.md). Bar does not repair the database. |
| Offline | Reopen T3 Code. Saved threads remain visible, but live activity indicators are disabled. |
| Busy/read error | The app retries every second. Use **Retry** to retry immediately when no read is in progress. |
| Links do not open | Check the T3 Code installation, URL scheme registration, and local runtime configuration. |
| Installation fails | Check `swift --version` for Swift 6 or later and read the Terminal error. If the destination app already exists, quit it and move it aside. |

For a bug report, [open an issue](https://github.com/eliophan/t3code-bar/issues) with your macOS version, architecture, T3 Code version, and steps to reproduce. Redact private information; do not attach your database or credentials.

## Known limitations

- Monitors one local T3 Code instance, not remote machines.
- Some background activity exists only in T3 Code's memory and cannot be inferred from SQLite. Working/Monitoring indicators after the main run ends may be incomplete.
- Unread completion and wake indicators depend on visit timestamps persisted by T3 Code; Bar does not read browser localStorage or call the sidebar's acknowledgment API.
- Server availability uses the runtime PID, not an HTTP health check. After a read error, the last successful list remains visible with live status disabled.
- Full sidebar parity is not guaranteed. Running on Intel, Gatekeeper behavior, and notarized distribution have not been verified.
