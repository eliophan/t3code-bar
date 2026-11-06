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

## Usage

Click **Usage** at the bottom of the thread list to see what T3 Code's own Usage page shows. Click the back arrow or press Esc to return to the threads. The popover always opens on the threads.

Three tabs sit under the header, in T3 Code's order. **Cost** and **Tokens** combine T3 Code's transcript cache with local OpenCode and Antigravity usage and Cursor's saved account cache. **Limits** reads the connected local T3 Code provider and CLI Proxy snapshots. A read-only T3 connection is required before the main popover opens. Bar remembers the tab you left open. On first launch it opens on Limits and 30 days, as T3 Code does.

The period control on Cost and Tokens offers Past 24h, 7 days, 30 days and 90 days. Limits has no period, so the control is hidden there. Past 24h is a sliding window. The other periods are whole calendar days in your time zone, with today counted in full. Bar remembers your choice. Hover a period for its full name.

### Cost

- The headline is the estimated cost, with the number of sessions and "API estimate" under it. When some records have no price, hover the info icon. It says what share of records the estimate leaves out.
- Each provider row shows its cost, its sessions, its share of the cost and its tokens.
- The chart shows daily cost per provider. Past 24h shows hourly cost. Hover it for the numbers of one day or hour.
- **Totals** lists processed tokens, cached input, uncached input, output and cache savings. Cache savings is what the cached reads would have cost at the full input price.
- **Cost by type** splits the cost into input, cache read, cache write, output and other. Other is cost that has no split, and it hides below half a cent.
- **Cost by speed** appears only when something ran fast or ultrafast. It shows what the faster tiers added as the premium.
- **Breakdown** ranks models by cost. A model whose records all lack a price reads "Unpriced". The list shows the first five models. Click "Show all N models" for the rest.

### Tokens

- The headline is processed tokens, which is uncached input, cached input, cache writes and output added together, with the number of sessions below it.
- **Tokens by type** splits the total into input, cache read, cache write and output.
- **Totals**, **By provider**, the chart and the **Breakdown** follow the same rules as on Cost. Models rank by tokens here.

The small footer shows only warnings and the transcript cache age. OpenCode and Antigravity refresh locally in the background while this tab is open. Transcript providers update when T3 Code's Usage page scans. Cursor uses only its saved account cache, which Bar never fetches. Costs are API estimates from T3 Code's price list. They are not what a plan bills.

### Limits

- Each provider has a section with one line per signed-in account. The line shows the plan and when T3 Code last checked that account.
- Each window pool has a card with the share left, the pace, and a bar of what is left. The line on the bar is where even spending would be. The hatched stretch is what the reset will restore.
- Under the bar, Bar writes when the window resets, the share that even pace would leave by now, what the reset restores, and any banked reset credits.
- When two accounts share a pool, each one gets its own bar.
- A card for an external usage page, such as ChatGPT, has a **Manage usage** button that opens the page in your browser. Bar does not fetch that page or contact the provider.
- If the newest check is more than 30 minutes old, a line above the cards says so. T3 Code probes providers only while it runs, so old numbers usually mean T3 Code is closed.

Limits do not need the transcript usage cache. The first launch requires pairing with the local T3 server. CLI Proxy pooled accounts are held in T3 Code’s live server and are absent from the provider cache.

To include those accounts:

1. In T3 Code, open **Settings → Connections → Authorized clients** for the local environment and click **Create link**. You must be allowed to create pairing links.
2. Give it a label such as `T3 Code Bar Limits`. Under **Permissions**, leave only **View environment** (`orchestration:read`) selected. Clear the other permissions; the **Read only** preset includes additional scopes.
3. Click **Create link**, then **Share → Copy code** on the new card (or **Copy code** directly when offered). Use the new code rather than a provider key, an existing client token, or a full pairing URL.
4. In Bar, open **Usage → Limits → Connect T3 Code…**. Confirm the **Local port**, paste the code, and click **Pair & connect**. The port is prefilled from the local runtime when available; Bar always connects to `127.0.0.1` regardless of any advertised host.
5. Select **Remember Bar token in Keychain** if Bar should reconnect automatically after restart. Without it, the exchanged Bar token lives only until the app quits. The one-time pairing code is never saved. A remembered token is loaded at app launch and must still be valid.

These T3 labels were checked in the `0.1.8-nightly.20261105.4205` client bundle. If your T3 version does not expose a separate scoped pairing code or the RPC ticket endpoint, saved provider data remains available. Live pairing, account counts against a real server, and macOS Keychain prompts still require manual verification; the connection is covered by mock tests.

After pairing, the subscription is maintained while the app is running, including when the popover is closed. If the connection drops after a successful snapshot, Bar keeps the last data and shows when it was received. A pairing-code token expires after one hour and requires pairing again. To revoke a Bar session on the server, use T3 Code’s authorized-client controls.

The connection reads this environment’s providers and configured CLI Proxy sources. Linked environments are excluded. Bar never sends refresh, reset-credit, or settings commands. T3 Code may still perform its own remote/direct-endpoint discovery while building the server-config subscription.

#### Long-lived access token

A pairing code gives Bar a DPoP token that T3 Code expires after one hour, so the live connection ends and you must pair again. For a connection that lasts, choose **Access token** in the connect form and paste a token from T3 Code's command line:

```bash
t3 auth session issue --scope orchestration:read --ttl 3650d --token-only
```

Select **Remember in Keychain** to reconnect at every launch. Bar sends the token as a Bearer credential to `127.0.0.1` only, and never to the token exchange.

The trade-off is that a Bearer token works for anyone who holds it. Its scope is read-only on this machine, and you can revoke it from T3 Code's authorized-client list. If T3 Code rejects the token, Bar stops and says the token expired or was revoked. Bar does not retry. This path has been tested against a mock server only. Live acceptance of a Bearer token by the ticket route still needs a manual check.

Bar's pairing-code flow requests a DPoP-bound access token and rejects a plain bearer token from the exchange. When remembered, the token and its private key are stored together in Keychain. DPoP reduces reuse of a stolen token but does not authenticate the local server: a process impersonating T3 on the configured port can still receive a pairing code. Only pair when the local T3 server is running and trusted.

### How Bar reads the data

Bar checks the files about every 5 seconds while the screen is open and decodes the usage cache again only when it changes. It scans local providers at launch and keeps their snapshot between visits. The paired Limits subscription stays open while Bar runs.

If T3 Code has never scanned, Cost and Tokens can still show local provider usage and explain that transcript usage is missing. Without any local sources, they ask you to open T3 Code's Usage page once. If T3 Code writes a newer cache format than this build understands, they say so and show no numbers. Update Bar in that case. While T3 Code is rewriting the cache, Bar keeps the last numbers and retries. A warning appears when entries or lines could not be read, because the totals may then read low. A period with no usage says "No usage in this period".

### What is missing

- Usage for a provider without a readable local source or a T3 Code cache. Missing and unreadable OpenCode, Antigravity and Cursor sources are identified below the totals. Bar does not contact provider APIs or read their credentials. The required Limits connection handles only the new pairing code and Bar’s own access token.
- Remote and linked environments. Bar reads this Mac only.
- T3 Code's own list of hidden providers. It lives in T3 Code's renderer storage, which Bar does not read.
- Freshness beyond T3 Code’s last scan, saved Cursor fetch, or provider/source probe. The live Limits stream delivers T3’s snapshots without forcing a new probe. Local OpenCode and Antigravity sources are checked separately.
- The table of cost by day or hour, and the model dialog with its price per million tokens. The popover has no room for them.
- Setting a price, refreshing prices or limits, and using a reset credit. Bar never changes provider data or contacts a provider.

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
| `T3_CODE_BAR_STATE_DIR` | Contains `statev2.sqlite`, `server-runtime.json`, `environment-id`, and the usage files directly. |
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
| Usage says "Open T3 Code's Usage page once" | T3 Code has not written `usage-scan-cache-vN.json` in the data directory yet. Open its Usage page. |
| Usage says "T3 Code changed its usage format" | T3 Code writes a cache version newer than 11. Update T3 Code Bar. |
| Usage numbers differ from T3 Code's page | Compare the same period and time zone, check each source's freshness, and allow a local refresh. Open T3 Code's Usage page to refresh transcript and Cursor caches. Remote environments and unsupported local sources are excluded. |
| Limits say "No limits data yet" | T3 Code has not written its provider files to `caches` yet. Open T3 Code once. |
| Limits say T3 Code last checked hours ago | T3 Code probes providers only while it runs. Open T3 Code and the numbers refresh within minutes. |
| Unsupported schema | Your T3 Code version may use a different database structure. See the [compatibility notes](COMPATIBILITY.md). Bar does not repair the database. |
| Offline | Reopen T3 Code. Saved threads remain visible, but live activity indicators are disabled. |
| Busy/read error | The app retries every second. Use **Retry** to retry immediately when no read is in progress. |
| Links do not open | Check the T3 Code installation, URL scheme registration, and local runtime configuration. |
| Installation fails | Check `swift --version` for Swift 6 or later and read the Terminal error. If the destination app already exists, quit it and move it aside. |

For a bug report, [open an issue](https://github.com/eliophan/t3code-bar/issues) with your macOS version, architecture, T3 Code version, and steps to reproduce. Redact private information; do not attach your database or credentials.

## Known limitations

- Monitors one local T3 Code instance, not remote machines. Usage excludes remote environments. OpenCode and Antigravity refresh from local sources, while other usage depends on T3 Code's saved caches. Limits are only as fresh as T3 Code's last provider check.
- Some background activity exists only in T3 Code's memory and cannot be inferred from SQLite. Working/Monitoring indicators after the main run ends may be incomplete.
- Unread completion and wake indicators depend on visit timestamps persisted by T3 Code; Bar does not read browser localStorage or call the sidebar's acknowledgment API.
- Server availability uses the runtime PID, not an HTTP health check. After a read error, the last successful list remains visible with live status disabled.
- Full sidebar parity is not guaranteed. Running on Intel, Gatekeeper behavior, and notarized distribution have not been verified.
