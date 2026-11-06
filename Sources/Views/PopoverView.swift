import SwiftUI

public enum PopoverScreen: Equatable, Sendable {
    case threads, usage
}

/// Which screen the popover shows. The app resets it when the popover closes, so it always opens on threads.
@MainActor
public final class PopoverNavigation: ObservableObject {
    @Published public var screen: PopoverScreen = .threads

    public init() {}
}

public struct PopoverView: View {
    /// Tall enough for a full list plus one expanded preview; longer lists scroll.
    public static let maxHeight: CGFloat = 560
    static let footerHeight: CGFloat = 36

    @ObservedObject var monitor: ThreadMonitor
    @ObservedObject var usage: UsageMonitor
    @ObservedObject var navigation: PopoverNavigation
    /// The height the current screen wants, chrome included.
    @State private var contentHeight: CGFloat = 0
    @State private var isRetrying = false

    private let onHeightChange: (CGFloat) -> Void

    public init(monitor: ThreadMonitor, usage: UsageMonitor, navigation: PopoverNavigation,
                onHeightChange: @escaping (CGFloat) -> Void = { _ in }) {
        self.monitor = monitor
        self.usage = usage
        self.navigation = navigation
        self.onHeightChange = onHeightChange
    }

    private var effectiveHeight: CGFloat {
        let maxHeight = Self.maxHeight
        if contentHeight > 0 {
            return min(contentHeight, maxHeight)
        }
        if monitor.activeThreads.isEmpty {
            return 160
        }
        var total: CGFloat = 12 + Self.footerHeight
        for thread in monitor.activeThreads {
            let hasBranch = !(thread.branch?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true)
            total += (hasBranch ? 63 : 47) + 2
        }
        return min(max(total, 80), maxHeight)
    }

    public var body: some View {
        VStack(spacing: 0) {
            switch navigation.screen {
            case .threads: threadsScreen
            case .usage: UsageView(usage: usage) { navigation.screen = .threads }
            }
        }
        .onPreferenceChange(ContentHeightPreferenceKey.self) { height in
            if height > 0 {
                contentHeight = height
                onHeightChange(effectiveHeight)
            }
        }
        // The window is resized by AppDelegate. The content fills it and stays pinned to the top.
        // A fixed height here would be animated separately from the window, and the hosting view
        // centers fixed-size content, so for a moment the rows would slide under a resting pointer:
        // the hovered row loses hover and its preview closes again.
        .frame(width: 320)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(T3Theme.canvas)
        .preferredColorScheme(.dark)
    }

    private var threadsScreen: some View {
        VStack(spacing: 0) {
            // Active Threads List (Pure view-only monitor, clean flat rows, no scrollbars)
            ScrollView(.vertical, showsIndicators: false) {
                VStack(spacing: 2) {
                    if monitor.isLoading {
                        ProgressView("Reading T3 Code data…")
                            .controlSize(.small)
                            .padding()
                    } else if let error = monitor.readError {
                        notice(error.localizedDescription, symbol: "exclamationmark.triangle")
                    } else if !monitor.isServerOnline {
                        notice("T3 Code offline · showing saved threads", symbol: "wifi.slash")
                    }
                    if !monitor.isLoading && monitor.activeThreads.isEmpty && monitor.readError == nil {
                        emptyState
                    } else {
                        ForEach(monitor.activeThreads) { thread in
                            ThreadRowView(thread: thread, showsLiveStatus: monitor.canShowLiveStatus,
                                          loadPreview: { await monitor.preview(for: $0) }) {
                                AppLauncher.openThread(thread: thread)
                            }
                        }
                    }
                }
                .padding(.horizontal, 6)
                .padding(.vertical, 6)
                .background(
                    GeometryReader { proxy in
                        Color.clear.preference(
                            key: ContentHeightPreferenceKey.self,
                            value: proxy.size.height + Self.footerHeight
                        )
                    }
                )
            }
            .background(HideScrollerHelper())
            UsageLinkRow { navigation.screen = .usage }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "tray")
                .font(.system(size: 20))
                .foregroundColor(T3Theme.textFaint)
                .accessibilityHidden(true)
            Text("No active threads")
                .font(.system(size: 12))
                .foregroundColor(T3Theme.textMuted)
            Button("Open T3 Code", action: AppLauncher.activateT3Code)
                .buttonStyle(.borderless)
                .font(.system(size: 12))
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 28)
    }

    private func notice(_ text: String, symbol: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(text, systemImage: symbol)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 12) {
                Button("Open T3 Code", action: AppLauncher.activateT3Code)
                Button(action: retry) {
                    if isRetrying {
                        HStack(spacing: 5) {
                            ProgressView().controlSize(.mini)
                            Text("Retrying…")
                        }
                    } else {
                        Text("Retry")
                    }
                }
                .disabled(isRetrying)
            }
            .buttonStyle(.borderless)
            .font(.system(size: 12))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
    }

    /// Waits for the read that follows the click, then holds the spinner briefly: a read takes
    /// milliseconds, and a spinner that never paints looks like a button that did nothing.
    private func retry() {
        isRetrying = true
        let started = Date()
        monitor.refresh()
        Task { @MainActor in
            let deadline = started.addingTimeInterval(10)
            while (monitor.lastAttemptAt ?? .distantPast) <= started, Date() < deadline {
                try? await Task.sleep(for: .milliseconds(50))
            }
            try? await Task.sleep(for: .milliseconds(500))
            isRetrying = false
        }
    }
}

struct ContentHeightPreferenceKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

struct HideScrollerHelper: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            if let scrollView = view.enclosingScrollView {
                scrollView.hasVerticalScroller = false
                scrollView.verticalScroller = nil
                scrollView.hasHorizontalScroller = false
                scrollView.horizontalScroller = nil
                scrollView.autohidesScrollers = true
            }
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        DispatchQueue.main.async {
            if let scrollView = nsView.enclosingScrollView {
                scrollView.hasVerticalScroller = false
                scrollView.verticalScroller = nil
            }
        }
    }
}

/// Opens the Usage screen. It sits below the thread list, outside the scroll view.
private struct UsageLinkRow: View {
    let action: () -> Void

    var body: some View {
        HoverArea { isHovered in
            Button(action: action) {
                HStack(spacing: 8) {
                    Image(systemName: "chart.bar")
                        .font(.system(size: 12))
                        .frame(width: 16)
                    Text("Usage")
                        .font(.system(size: 12.5))
                    Spacer()
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .semibold))
                }
                .foregroundColor(isHovered ? T3Theme.textPrimary : T3Theme.textSecondary)
                .padding(.horizontal, 16)
                .frame(maxWidth: .infinity)
                .frame(height: PopoverView.footerHeight)
                .background(isHovered ? T3Theme.rowHover : Color.clear)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .focusable(false)
            .accessibilityLabel("Usage")
            .accessibilityHint("Shows token usage")
        }
        .overlay(alignment: .top) { T3Theme.border.frame(height: 1) }
    }
}
