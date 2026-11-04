import SwiftUI
import AppKit
import Combine

@main
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let monitor = ThreadMonitor()
    private var statusItem: NSStatusItem!
    private var panel: NSPanel!
    private var panelHeight: CGFloat = 160
    private var clickMonitor: Any?
    private var widthObserver: AnyCancellable?
    private let notifier = AttentionNotifier()
    private var refreshObserver: AnyCancellable?
    private var hostingController: NSHostingController<PopoverView>!

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
        installContextMenuMonitor()
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
        hostingController = NSHostingController(rootView: PopoverView(monitor: monitor) { [weak self] height in
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
            self?.closePanel()
        }
    }

    private func closePanel() {
        panel.orderOut(nil)
        if let clickMonitor { NSEvent.removeMonitor(clickMonitor) }
        clickMonitor = nil
    }

    private func resizePanel(to height: CGFloat) {
        panelHeight = min(max(height, 80), 480)
        if panel.isVisible { positionPanel() } else { panel.setContentSize(NSSize(width: 320, height: panelHeight)) }
    }

    private func positionPanel() {
        guard let button = statusItem.button, let window = button.window else { return }
        let anchor = window.convertToScreen(button.convert(button.bounds, to: nil))
        let size = NSSize(width: 320, height: panelHeight)
        var x = anchor.midX - size.width / 2
        if let screen = window.screen ?? NSScreen.main {
            x = min(max(x, screen.visibleFrame.minX + 8), screen.visibleFrame.maxX - size.width - 8)
        }
        panel.setFrame(NSRect(x: x, y: anchor.minY - 6 - size.height, width: size.width, height: size.height), display: true)
    }

    private func addMuteItem(to menu: NSMenu) {
        let item = NSMenuItem(title: "Mute Notifications", action: #selector(toggleMuteNotifications), keyEquivalent: "")
        item.target = self
        item.state = notifier.isMuted ? .on : .off
        menu.addItem(item)
        menu.addItem(.separator())
    }

    @objc private func toggleMuteNotifications() { notifier.toggleMute() }

    private func installContextMenuMonitor() {
        NSEvent.addLocalMonitorForEvents(matching: [.rightMouseDown, .rightMouseUp]) { event in
            guard let window = event.window,
                  (window.level == .statusBar || NSStringFromClass(type(of: window)).localizedCaseInsensitiveContains("statusbar")) else {
                return event
            }

            if event.type == .rightMouseDown {
                let menu = NSMenu()
                MainActor.assumeIsolated { self.addMuteItem(to: menu) }
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
