export const title = "Linux tray availability gates working content";
export function state(v) {
  const s = v.surface;
  if (s.exited) return "Application exited";
  if (v.badHide) return "Hide dismissed the diagnostic";
  if (v.badSettings) return "Settings reached content without a tray";
  if (v.badResize) return "Resize changed an unavailable window";
  if (s.content && s.state !== 1) return "Working content visible without observed tray";
  if (s.state === 0) return "Starting with content hidden";
  if (s.state === 2) return s.diagnostic ? "Retry and Quit diagnostic" : "Diagnostic missing";
  return s.content ? "Working panel visible" : "Working panel hidden with tray available";
}
export function step(action, from) {
  if (action !== "Apply") return {
    ExpireGrace: "startup grace expires",
    ReadHost: "sample tray registration",
    Publish: "publish the latest registration observation",
    Enqueue: "queue a user request",
    Dequeue: "start the queued callback",
  }[action] ?? action;
  return {
    observe00: "apply missing registration before startup grace",
    observe01: "apply missing registration after startup grace",
    observe10: "apply observed registration",
    observe11: "apply observed registration",
    observe01_fail: "automatic tray reconstruction fails; apply missing registration",
    observe11_fail: "automatic tray reconstruction fails; apply registration observation",
    activate: "activate the tray",
    show: "show the current permitted window",
    settings: "open Settings if tray is available",
    hide: "hide the working panel if tray is available",
    resize: "resize the working panel if tray is available",
    retry_ok: "Retry creates the tray and wakes polling",
    retry_fail: "Retry fails and retains the diagnostic",
    close: "close the diagnostic",
    quit: "press Quit",
  }[from.active] ?? action;
}
