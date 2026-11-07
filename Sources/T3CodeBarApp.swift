import SwiftUI
import AppKit
import Combine

@main
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let monitor = ThreadMonitor()
    private let usage = UsageMonitor()
    private let limits = LimitsMonitor()
    private let usageScreen = UsageScreenModel()
    private let navigation = PopoverNavigation()
    private var statusItem: NSStatusItem!
    private var panel: NSPanel!
    private var panelHeight: CGFloat = 160
    private var clickMonitor: Any?
    private var widthObserver: AnyCancellable?
    private let notifier = AttentionNotifier()
    private let updater = AppUpdater()
    private var refreshObserver: AnyCancellable?
    private var hostingController: NSHostingController<PopoverView>!
    private var updatePrompt: (window: NSWindow, actions: UpdatePromptActions)?

    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.run()
        withExtendedLifetime(delegate) {}
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        configureStatusItem()
        configurePopover()
        limits.start()
        usage.prewarmLocal()
        installContextMenuMonitor()
        updater.onUpdateAvailable = { [weak self] tag in self?.askToInstall(tag: tag) ?? false }
        updater.start()
        refreshObserver = monitor.$lastRefreshedAt.compactMap { $0 }.sink { [weak self] _ in
            guard let self else { return }
            self.notifier.process(self.monitor.activeThreads)
        }
    }

    func applicationDidResignActive(_ notification: Notification) {
        if panel.isVisible { closePanel() }
    }

    private func configureStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        guard let button = statusItem.button else { return }

        let iconView = ClickThroughHostingView(rootView: StatusItemView(monitor: monitor))
        iconView.translatesAutoresizingMaskIntoConstraints = false
        button.addSubview(iconView)
        NSLayoutConstraint.activate([
            iconView.leadingAnchor.constraint(equalTo: button.leadingAnchor),
            iconView.trailingAnchor.constraint(equalTo: button.trailingAnchor),
            iconView.topAnchor.constraint(equalTo: button.topAnchor),
            iconView.bottomAnchor.constraint(equalTo: button.bottomAnchor)
        ])
        let updateWidth = { [weak self, weak iconView] in
            guard let self, let iconView else { return }
            self.statusItem.length = max(iconView.fittingSize.width, 24)
        }
        updateWidth()
        widthObserver = monitor.objectWillChange.sink { _ in
            DispatchQueue.main.async { updateWidth() }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { updateWidth() }
        }
        button.action = #selector(togglePopover(_:))
        button.target = self
    }

    private func configurePopover() {
        hostingController = NSHostingController(rootView: PopoverView(monitor: monitor, usage: usage, limits: limits, usageScreen: usageScreen, navigation: navigation) { [weak self] height in
            self?.resizePanel(to: height)
        })
        let view = hostingController.view
        view.wantsLayer = true
        view.layer?.backgroundColor = NSColor.black.cgColor
        view.layer?.cornerRadius = 12
        view.layer?.masksToBounds = true

        panel = PopoverPanel(contentRect: NSRect(x: 0, y: 0, width: 320, height: 160),
                             styleMask: [.borderless, .nonactivatingPanel],
                             backing: .buffered, defer: false)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .popUpMenu
        panel.appearance = NSAppearance(named: .darkAqua)
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = view
        panelHeight = 160
    }

    @objc private func togglePopover(_ sender: Any?) {
        if panel.isVisible { closePanel() } else { showPanel() }
    }

    private func showPanel() {
        positionPanel()
        // Cursor rects and pointer styles only apply to the key window, and a
        // borderless panel is never key unless asked, so the hand cursor over
        // rows would not appear. A nonactivating panel becomes key without
        // activating this app or stealing the frontmost app's menu bar.
        panel.makeKeyAndOrderFront(nil)
        clickMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] _ in
            // A click on the status icon is left to togglePopover. Closing here
            // too would hide the panel first, and the toggle would reopen it.
            guard let self, !(self.buttonScreenFrame()?.contains(NSEvent.mouseLocation) ?? false) else { return }
            self.closePanel()
        }
    }

    private func closePanel() {
        // Reopening lands on threads, and leaving the Usage screen stops its reads.
        navigation.screen = .threads
        panel.orderOut(nil)
        if let clickMonitor { NSEvent.removeMonitor(clickMonitor) }
        clickMonitor = nil
    }

    private func resizePanel(to height: CGFloat) {
        let target = min(max(height, 80), PopoverView.maxHeight)
        // Tabs have different heights, so the Usage screen glides between them. The thread list resizes
        // at once: it changes every second and its hover previews rely on the window not lagging the rows.
        let animates = panel.isVisible && navigation.screen == .usage && target != panelHeight
        panelHeight = target
        if animates {
            positionPanel(animated: true)
        } else if panel.isVisible {
            positionPanel()
        } else {
            panel.setContentSize(NSSize(width: 320, height: panelHeight))
        }
    }

    private func buttonScreenFrame() -> NSRect? {
        guard let button = statusItem.button, let window = button.window else { return nil }
        return window.convertToScreen(button.convert(button.bounds, to: nil))
    }

    private func positionPanel(animated: Bool = false) {
        guard let button = statusItem.button, let window = button.window, let anchor = buttonScreenFrame() else { return }
        let size = NSSize(width: 320, height: panelHeight)
        var x = anchor.midX - size.width / 2
        if let screen = window.screen ?? NSScreen.main {
            x = min(max(x, screen.visibleFrame.minX + 8), screen.visibleFrame.maxX - size.width - 8)
        }
        let frame = NSRect(x: x, y: anchor.minY - 6 - size.height, width: size.width, height: size.height)
        if animated {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.18
                context.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
                panel.animator().setFrame(frame, display: true)
            }
        } else {
            panel.setFrame(frame, display: true)
        }
    }

    private func addToggleItems(to menu: NSMenu) {
        menu.autoenablesItems = false
        let update = UpdateMenuModel(status: updater.status, runningVersion: updater.runningVersion, now: Date())
        for line in [update.versionLine, update.statusLine] {
            let item = NSMenuItem(title: line, action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
        }
        let actionItem = NSMenuItem(title: update.action.title, action: #selector(runUpdateAction), keyEquivalent: "")
        actionItem.target = self
        actionItem.isEnabled = update.action.isEnabled
        menu.addItem(actionItem)
        menu.addItem(.separator())

        let mute = NSMenuItem(title: "Mute Notifications", action: #selector(toggleMuteNotifications), keyEquivalent: "")
        mute.target = self
        mute.state = notifier.isMuted ? .on : .off
        menu.addItem(mute)

        let previews = NSMenuItem(title: "Show Previews", action: #selector(togglePreviews), keyEquivalent: "")
        previews.target = self
        previews.state = PreviewSettings.isEnabled ? .on : .off
        menu.addItem(previews)

        let updates = NSMenuItem(title: "Update Automatically", action: #selector(toggleAutoUpdate), keyEquivalent: "")
        updates.target = self
        updates.state = UpdateSettings.isEnabled ? .on : .off
        menu.addItem(updates)

        let withoutAsking = NSMenuItem(title: "Install Without Asking", action: #selector(toggleInstallWithoutAsking), keyEquivalent: "")
        withoutAsking.target = self
        withoutAsking.indentationLevel = 1
        withoutAsking.state = UpdateSettings.installsWithoutAsking ? .on : .off
        menu.addItem(withoutAsking)
        menu.addItem(.separator())
    }

    /// Shows the prompt as a floating window without activating the app, so a
    /// keystroke meant for another app cannot answer it and the run loop keeps
    /// running (runModal would stall ThreadMonitor's poll and the heartbeat).
    /// Returns true once a prompt is on screen, replacing any earlier one.
    private func askToInstall(tag: String) -> Bool {
        closeUpdatePrompt()
        let prompt = UpdatePrompt(tag: tag)
        let alert = NSAlert()
        alert.messageText = prompt.messageText
        alert.informativeText = prompt.informativeText
        let later = alert.addButton(withTitle: prompt.laterTitle)
        let install = alert.addButton(withTitle: prompt.installTitle)
        later.keyEquivalent = "\r"
        install.keyEquivalent = prompt.installKeyEquivalent

        // Without runModal nothing lays the alert out, which leaves a blank button row and the suppression checkbox showing.
        alert.layout()
        let window = alert.window
        let actions = UpdatePromptActions(
            install: { [weak self] in
                self?.closeUpdatePrompt()
                self?.updater.installAvailable(tag: prompt.tag)
            },
            later: { [weak self] in self?.closeUpdatePrompt() }
        )
        later.target = actions
        later.action = #selector(UpdatePromptActions.chooseLater)
        install.target = actions
        install.action = #selector(UpdatePromptActions.chooseInstall)

        window.level = .floating
        window.isReleasedWhenClosed = false
        window.center()
        window.orderFrontRegardless()
        updatePrompt = (window, actions)
        return true
    }

    private func closeUpdatePrompt() {
        updatePrompt?.window.orderOut(nil)
        updatePrompt = nil
    }

    @objc private func runUpdateAction() {
        switch UpdateMenuModel(status: updater.status, runningVersion: updater.runningVersion, now: Date()).action {
        case .check: Task { await updater.checkNow() }
        case .install: updater.installAvailable()
        case .checking, .installing, .unavailable: break
        }
    }

    @objc private func toggleAutoUpdate() { UpdateSettings.isEnabled.toggle() }

    @objc private func toggleInstallWithoutAsking() { UpdateSettings.installsWithoutAsking.toggle() }

    @objc private func toggleMuteNotifications() { notifier.toggleMute() }

    @objc private func togglePreviews() {
        UserDefaults.standard.set(!PreviewSettings.isEnabled, forKey: PreviewSettings.key)
    }

    private func installContextMenuMonitor() {
        NSEvent.addLocalMonitorForEvents(matching: [.rightMouseDown, .rightMouseUp]) { event in
            guard let window = event.window,
                  (window.level == .statusBar || NSStringFromClass(type(of: window)).localizedCaseInsensitiveContains("statusbar")) else {
                return event
            }

            if event.type == .rightMouseDown {
                let menu = NSMenu()
                MainActor.assumeIsolated { self.addToggleItems(to: menu) }
                let quitItem = NSMenuItem(
                    title: "Quit T3 Code Bar",
                    action: #selector(NSApplication.terminate(_:)),
                    keyEquivalent: "q"
                )
                quitItem.target = NSApp
                menu.addItem(quitItem)
                let location = window.contentView?.convert(event.locationInWindow, from: nil) ?? event.locationInWindow
                menu.popUp(positioning: quitItem, at: location, in: window.contentView)
            }
            return nil
        }
    }
}


@MainActor
private final class UpdatePromptActions: NSObject {
    private let install: () -> Void
    private let later: () -> Void

    init(install: @escaping () -> Void, later: @escaping () -> Void) {
        self.install = install
        self.later = later
    }

    @objc func chooseInstall() { install() }
    @objc func chooseLater() { later() }
}

/// Borderless panels refuse key status by default; see `showPanel()`.
private final class PopoverPanel: NSPanel {
    override var canBecomeKey: Bool { true }
}

private final class ClickThroughHostingView<Content: View>: NSHostingView<Content> {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

private struct StatusItemView: View {
    @ObservedObject var monitor: ThreadMonitor

    var body: some View {
        MenuBarIconView(
            status: monitor.overallStatus,
            activeCount: monitor.badgeCount
        )
    }
}
