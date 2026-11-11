# Updates and notifications

## Sub-features

- Show a notification when a new or changed thread needs attention.
- Open the current thread from a notification action.
- Check for updates and show an install prompt only for a verified offer.
- Quit the Bar from Settings.

## How to get to it (user POV)

Leave the Bar paired with a working T3 Code server, then open Settings for update controls. Use a fixture or a real thread state that changes while the Bar is running to exercise notification routing.

## Driving it with Computer Use

Use Settings by its accessible label. Trigger `Check for Updates…` only when the build has updater configuration and verify the resulting status. For notifications, use the existing notification fixture path, click the notification action, and verify that only a currently listed thread opens.

## Gotchas

An updater-disabled development build correctly leaves installation unavailable. A notification is not proof of a current thread; the action must be checked against the current snapshot. Do not kill the app by process name during cleanup.
