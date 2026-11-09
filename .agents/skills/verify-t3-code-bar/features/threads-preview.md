# Threads, hover preview and thread actions

The inbox shows thread attention states and lets users preview the last reply
or open/copy a thread destination.

## Sub-features

- `threads-list`: project, branch and status labels from saved thread data.
- `preview-hover`: hovering a row for 500 ms shows the latest reply.
- `preview-cancel`: leaving, switching rows or disabling previews cancels it.
- `thread-open`: row click opens T3 Code at the right thread.
- `thread-context`: Open in T3 Code, Open in Browser and Copy Thread Link.
- `thread-new`: New thread from an empty inbox.

## How to get to it (user POV)

Pair successfully, then open the main panel. Hover or select a thread row.
Right-click a row for its context menu. Back to threads returns from Usage
or Settings. An empty paired inbox offers New thread.

## Driving it with Cua

Preconditions: the app is paired to a disposable server. The default seed has
threads but no pairing server; onboarding blocks this journey and must be
reported as an unmet precondition, not bypassed.

- **Rows.** Use
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py snapshot`.
  With the seeded state and valid pairing, look for Review a proposed action,
  Answer a question and Working thread, with Fixture project and their states.
  Archived, child and snoozed fixtures must not appear.
- **Hover.** With a fresh attested native screenshot, follow the cua-driver
  hover/move schema on Review a proposed action. Wait at least 500 ms, then
  capture the preview containing The proposed change is ready for review.
  Move away and require the preview to disappear. Repeat with Hover previews
  disabled through Settings and require no preview. Accessibility activation
  or a direct preview IPC call does not simulate hovering.
- **Context.** Use native Cua right-click on the actual row. Resolve the fresh
  Open in T3 Code, Open in Browser and Copy Thread Link menu controls. Choose
  each in separate runs with the same starting state and verify the resulting
  destination or clipboard value, not only menu dismissal.
- **Back.** From a paired Settings page use
  `python3 .agents/skills/verify-t3-code-bar/scripts/control.py click 'Back to threads'`.
  Require the inbox list instead of connection onboarding.

## Gotchas

- Opening synthetic IDs can launch the user's real T3 Code/browser. Use an
  isolated disposable destination for open actions; seeded data alone is not
  an opener sandbox.
- The seeded reply is attached to Review a proposed action only. A different
  row without a reply does not prove preview success.
- The initial proof exercised the preview preference, not hovering a thread.
- Notification routing is an additional native entry into a thread. Default
  fixture mode suppresses it; it remains unverified until a separate synthetic
  notification session captures the notification action and destination.
