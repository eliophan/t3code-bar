import SwiftUI

/// T3 Code's Usage page, sized for the popover: Cost, Tokens and Limits under one header.
struct UsageView: View {
    static let headerHeight: CGFloat = 44
    static let tabBarHeight: CGFloat = 38
    /// Everything above the scrolling content.
    static let chromeHeight = headerHeight + tabBarHeight

    @ObservedObject var usage: UsageMonitor
    @ObservedObject var limits: LimitsMonitor
    @ObservedObject var screen: UsageScreenModel
    let onBack: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            header
            tabBar
            ScrollView(.vertical, showsIndicators: false) {
                content
                    .padding(.horizontal, 16)
                    .padding(.vertical, 16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(
                        GeometryReader { proxy in
                            Color.clear.preference(
                                key: ContentHeightPreferenceKey.self,
                                value: proxy.size.height + Self.chromeHeight
                            )
                        }
                    )
            }
            .background(HideScrollerHelper())
        }
        .onAppear { monitor(for: screen.tab) }
        .onChange(of: screen.tab) { _, tab in monitor(for: tab) }
        .onDisappear {
            usage.stop()
            limits.stop()
        }
    }

    /// Each tab reads only its own file, and only while it is the one on screen.
    private func monitor(for tab: UsageTab) {
        if tab == .limits {
            usage.stop()
            limits.start()
        } else {
            limits.stop()
            usage.start()
        }
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: 6) {
            HoverArea { isHovered in
                Button(action: onBack) {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundColor(isHovered ? T3Theme.textPrimary : T3Theme.textSecondary)
                        .frame(width: 24, height: 24)
                        .background(RoundedRectangle(cornerRadius: 6, style: .continuous)
                            .fill(isHovered ? T3Theme.rowHover : Color.clear))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .focusable(false)
                .keyboardShortcut(.cancelAction)
                .accessibilityLabel("Back to threads")
            }
            Text("Usage")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(T3Theme.textPrimary)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: 8)
            if screen.tab.showsPeriod {
                UsageSegmentedControl(options: UsageOptions.periods, selection: $usage.period, groupLabel: "Usage period")
            }
        }
        .padding(.horizontal, 12)
        .frame(height: Self.headerHeight)
    }

    private var tabBar: some View {
        UsageSegmentedControl(options: UsageOptions.tabs, selection: $screen.tab, groupLabel: "Usage metric", fillsWidth: true)
            .padding(.horizontal, 12)
            .frame(height: Self.tabBarHeight, alignment: .top)
            .overlay(alignment: .bottom) { T3Theme.border.frame(height: 1) }
    }

    // MARK: Content

    @ViewBuilder
    private var content: some View {
        switch screen.tab {
        case .cost, .tokens: scanContent
        case .limits: limitsContent
        }
    }

    @ViewBuilder
    private var limitsContent: some View {
        VStack(alignment: .leading, spacing: 16) {
            LimitsConnectionView(connection: limits.live)
            limitsDataContent
        }
    }

    @ViewBuilder
    private var limitsDataContent: some View {
        switch UsageContentState.limits(snapshot: limits.snapshot, error: limits.readError) {
        case .loading:
            ProgressView("Reading limits…")
                .controlSize(.small)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 28)
        case .missing:
            UsageStateView(symbol: "gauge.with.dots.needle.50percent", title: "No limits data yet",
                           detail: "Open T3 Code once so it can save the status of its providers.")
        case .unreadable, .unsupported:
            UsageStateView(symbol: "exclamationmark.triangle", title: "Could not read limits",
                           detail: "T3 Code's provider status files could not be read. Trying again.")
        case .empty:
            UsageStateView(symbol: "gauge.with.dots.needle.50percent", title: "No provider reports subscription limits.")
        case .content:
            if let snapshot = limits.snapshot {
                UsageLimitsTab(snapshot: snapshot, now: limits.now, readFailed: limits.readError != nil && limits.live.status != .live)
            }
        }
    }

    @ViewBuilder
    private var scanContent: some View {
        VStack(alignment: .leading, spacing: 20) {
            if usage.report != nil, usage.notice == .retrying, let message = Self.message(for: .retrying) {
                noticeRow(message)
            }
            switch UsageContentState.scan(report: usage.report, notice: usage.notice) {
            case .loading:
                ProgressView("Reading usage…")
                    .controlSize(.small)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 28)
            case .missing:
                UsageStateView(symbol: "chart.bar", title: "No usage data yet", detail: Self.message(for: .missing))
            case .unsupported(let version):
                UsageStateView(symbol: "exclamationmark.triangle", title: "T3 Code changed its usage format",
                               detail: Self.message(for: .unsupported(version)))
            case .unreadable:
                UsageStateView(symbol: "exclamationmark.triangle", title: "Could not read usage",
                               detail: Self.message(for: .retrying))
            case .empty:
                UsageStateView(symbol: "chart.bar", title: "No usage in this period")
                if let report = usage.report { UsageFootnotes(report: report) }
            case .content:
                if let report = usage.report {
                    if screen.tab == .tokens {
                        UsageTokensTab(report: report)
                    } else {
                        UsageCostTab(report: report)
                    }
                }
            }
        }
        // A read for another period is still running, so these numbers are not for the selected one.
        .opacity(usage.isStale ? 0.45 : 1)
    }

    private func noticeRow(_ text: String) -> some View {
        Label(text, systemImage: "exclamationmark.triangle")
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    // MARK: Text

    nonisolated static func message(for notice: UsageNotice) -> String? {
        switch notice {
        case .missing: return "No usage data yet. Open T3 Code's Usage page once so it can scan your sessions."
        case .unsupported: return "T3 Code changed its usage format. Update T3 Code Bar to read it."
        case .retrying: return "T3 Code is rewriting its usage data. Trying again."
        }
    }

    /// `29.5% of tokens · $682.88`, as T3 Code writes a provider row on the Tokens tab.
    nonisolated static func providerDetail(tokens: Int, total: Int, costUsd: Double) -> String {
        let share = total > 0 ? Double(tokens) / Double(total) : 0
        return "\(TokenFormat.percent(share)) · \(TokenFormat.currency(costUsd))"
    }

    /// T3 Code's wording for skipped transcript lines, plus a line for cache entries this build could not decode.
    nonisolated static func partialText(_ report: UsageReport) -> String? {
        var lines: [String] = []
        if report.malformedRecords > 0 {
            lines.append("\(report.malformedRecords) malformed or inconsistent usage record(s) were skipped; usage may be incomplete.")
        }
        if report.rejectedFiles > 0 {
            lines.append("\(report.rejectedFiles) cache entries could not be read; usage may be incomplete.")
        }
        return lines.isEmpty ? nil : lines.joined(separator: " ")
    }

    nonisolated static let updatedHelp = """
        Transcript usage is as of T3 Code's last Usage page scan. OpenCode and Antigravity are read locally \
        every 30 seconds while this tab is open. Cursor uses T3 Code's saved account cache. Other computers are excluded.
        """

    nonisolated static func localSourceText(_ source: LocalUsageStatus, now: Date = Date()) -> String {
        let name = source.kind.style.label
        switch source.state {
        case .missing: return "\(name) · No local usage source found"
        case .unreadable: return "\(name) · Could not read local usage. Trying again."
        case .partial: return "\(name) · Partial local data. Unreadable files or records may be missing or show older values. Trying again."
        case .ready:
            if source.kind == .cursor {
                guard let updated = source.dataUpdatedAt else { return "Cursor · Saved account cache is empty" }
                return "Cursor · Saved account data · " + updatedText(modified: updated, now: now)
            }
            return "\(name) · Local check · " + updatedText(modified: source.checkedAt, now: now)
        }
    }

    nonisolated static func updatedText(modified: Date, now: Date) -> String {
        let seconds = Int(now.timeIntervalSince(modified))
        switch seconds {
        case ..<60: return "Updated just now"
        case ..<3_600: return "Updated \(seconds / 60) min ago"
        case ..<86_400: return "Updated \(seconds / 3_600) h ago"
        default: return "Updated \(seconds / 86_400) d ago"
        }
    }
}
