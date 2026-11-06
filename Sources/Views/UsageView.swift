import SwiftUI

/// T3 Code's Usage page, Tokens view, sized for the popover.
struct UsageView: View {
    static let headerHeight: CGFloat = 44

    @ObservedObject var usage: UsageMonitor
    let onBack: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollView(.vertical, showsIndicators: false) {
                content
                    .padding(.horizontal, 16)
                    .padding(.vertical, 16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(
                        GeometryReader { proxy in
                            Color.clear.preference(
                                key: ContentHeightPreferenceKey.self,
                                value: proxy.size.height + Self.headerHeight
                            )
                        }
                    )
            }
            .background(HideScrollerHelper())
        }
        .onAppear { usage.start() }
        .onDisappear { usage.stop() }
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
            PeriodControl(selection: $usage.period)
        }
        .padding(.horizontal, 12)
        .frame(height: Self.headerHeight)
        .overlay(alignment: .bottom) { T3Theme.border.frame(height: 1) }
    }

    // MARK: Content

    @ViewBuilder
    private var content: some View {
        VStack(alignment: .leading, spacing: 20) {
            if let kind = usage.notice, let message = Self.message(for: kind) {
                noticeRow(message)
            }
            if let report = usage.report {
                if report.isEmpty {
                    emptyState
                } else {
                    hero(report)
                    tokenTypes(report.tokens)
                    totals(report)
                    providers(report)
                }
                footnotes(report)
            } else if usage.notice == nil || usage.notice == .retrying {
                ProgressView("Reading usage…")
                    .controlSize(.small)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 28)
            }
        }
        // A read for another period is still running, so these numbers are not for the selected one.
        .opacity(usage.isStale ? 0.45 : 1)
    }

    private func hero(_ report: UsageReport) -> some View {
        let sessions = TokenFormat.sessions(report.sessions)
        return VStack(alignment: .leading, spacing: 4) {
            Text("Processed tokens")
                .font(.system(size: 11.5))
                .foregroundColor(T3Theme.textMuted)
            Text(TokenFormat.compact(report.processedTokens))
                .font(.system(size: 34, weight: .semibold))
                .monospacedDigit()
                .foregroundColor(T3Theme.textPrimary)
            Text(sessions)
                .font(.system(size: 11.5))
                .monospacedDigit()
                .foregroundColor(T3Theme.textMuted)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Processed tokens, \(TokenFormat.spoken(report.processedTokens))")
        .accessibilityValue(sessions)
    }

    private func tokenTypes(_ tokens: UsageReport.Tokens) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            sectionHeading("Tokens by type")
            SegmentedBar(segments: TokenKind.allCases.map {
                SegmentedBar.Segment(id: $0.label, value: $0.value(in: tokens), color: $0.color)
            })
            .accessibilityHidden(true)
            legendGrid(TokenKind.allCases.map { (label: $0.label, value: TokenFormat.compact($0.value(in: tokens)),
                                                 spoken: TokenFormat.spoken($0.value(in: tokens)), color: $0.color) })
        }
    }

    private func totals(_ report: UsageReport) -> some View {
        let rows: [(label: String, value: String, spoken: String)] = [
            ("Cached input", TokenFormat.compact(report.tokens.cachedInput), TokenFormat.spoken(report.tokens.cachedInput)),
            ("Uncached input", TokenFormat.compact(report.tokens.uncachedInput), TokenFormat.spoken(report.tokens.uncachedInput)),
            ("Output", TokenFormat.compact(report.tokens.output), TokenFormat.spoken(report.tokens.output)),
            ("Cache savings", TokenFormat.currency(report.cacheSavingsUsd), TokenFormat.currency(report.cacheSavingsUsd)),
        ]
        return VStack(alignment: .leading, spacing: 12) {
            sectionHeading("Totals")
            VStack(spacing: 8) {
                ForEach(rows, id: \.label) { row in
                    HStack {
                        Text(row.label).foregroundColor(T3Theme.textMuted)
                        Spacer(minLength: 8)
                        Text(row.value).monospacedDigit().foregroundColor(T3Theme.textPrimary)
                    }
                    .font(.system(size: 11.5))
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("\(row.label), \(row.spoken)")
                }
            }
        }
    }

    private func legendGrid(_ items: [(label: String, value: String, spoken: String, color: Color)]) -> some View {
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 16), GridItem(.flexible())], alignment: .leading, spacing: 8) {
            ForEach(items, id: \.label) { item in
                HStack(spacing: 6) {
                    RoundedRectangle(cornerRadius: 2, style: .continuous)
                        .fill(item.color)
                        .frame(width: 8, height: 8)
                    Text(item.label)
                        .foregroundColor(T3Theme.textMuted)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(item.value)
                        .monospacedDigit()
                        .foregroundColor(T3Theme.textPrimary)
                }
                .font(.system(size: 11.5))
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("\(item.label), \(item.spoken)")
            }
        }
    }

    private func providers(_ report: UsageReport) -> some View {
        let largest = max(report.providers.map(\.tokens.processed).max() ?? 1, 1)
        return VStack(alignment: .leading, spacing: 12) {
            sectionHeading("By provider")
            ForEach(report.providers, id: \.kind) { provider in
                providerRow(provider, total: report.processedTokens,
                            share: Double(provider.tokens.processed) / Double(largest))
            }
        }
    }

    private func providerRow(_ provider: UsageReport.Provider, total: Int, share: Double) -> some View {
        let processed = provider.tokens.processed
        let style = provider.kind.style
        let detail = Self.providerDetail(tokens: processed, total: total, costUsd: provider.costUsd)
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                ProviderIconView(providers: [style.driver ?? "?"], size: 14)
                Text(style.label)
                    .font(.system(size: 12.5))
                    .foregroundColor(T3Theme.textSecondary)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text(TokenFormat.compact(processed))
                    .font(.system(size: 12.5, weight: .medium))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textPrimary)
                Text("· \(detail)")
                    .font(.system(size: 10.5))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textMuted)
            }
            ShareBar(share: share)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(style.label), \(TokenFormat.spoken(processed)) processed tokens, \(detail)")
    }

    private func footnotes(_ report: UsageReport) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if let partial = Self.partialText(report) {
                Text(partial)
                    .help("These are left out of the totals, as T3 Code leaves them out of its own.")
            }
            Text(Self.updatedText(modified: report.cacheModified, now: Date()))
                .help(Self.updatedHelp)
        }
        .font(.system(size: 11))
        .foregroundColor(T3Theme.textMuted)
    }

    private func sectionHeading(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 13, weight: .medium))
            .foregroundColor(T3Theme.textPrimary)
            .accessibilityAddTraits(.isHeader)
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "chart.bar")
                .font(.system(size: 20))
                .foregroundColor(T3Theme.textFaint)
                .accessibilityHidden(true)
            Text("No usage in this period")
                .font(.system(size: 12))
                .foregroundColor(T3Theme.textMuted)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
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
        The numbers are as of T3 Code's last scan, which runs when its Usage page opens. \
        OpenCode, Antigravity and Cursor usage is not included yet, and neither are other computers.
        """

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

/// The four slices of input and output T3 Code draws, in its order and one hue at four strengths.
private enum TokenKind: CaseIterable {
    case input, cacheRead, cacheWrite, output

    var label: String {
        switch self {
        case .input: return "Input"
        case .cacheRead: return "Cache read"
        case .cacheWrite: return "Cache write"
        case .output: return "Output"
        }
    }

    var color: Color {
        switch self {
        case .input: return T3Theme.tokenInput
        case .cacheRead: return T3Theme.tokenCacheRead
        case .cacheWrite: return T3Theme.tokenCacheWrite
        case .output: return T3Theme.tokenOutput
        }
    }

    func value(in tokens: UsageReport.Tokens) -> Int {
        switch self {
        case .input: return tokens.uncachedInput
        case .cacheRead: return tokens.cachedInput
        case .cacheWrite: return tokens.cacheCreation
        case .output: return tokens.output
        }
    }
}

private struct SegmentedBar: View {
    struct Segment: Identifiable {
        let id: String
        let value: Int
        let color: Color
    }

    let segments: [Segment]
    private let gap: CGFloat = 2

    var body: some View {
        let visible = segments.filter { $0.value > 0 }
        let total = Double(visible.reduce(0) { $0 + $1.value })
        GeometryReader { proxy in
            let room = proxy.size.width - gap * CGFloat(max(visible.count - 1, 0))
            HStack(spacing: gap) {
                ForEach(visible) { segment in
                    Rectangle()
                        .fill(segment.color)
                        .frame(width: max(1, room * CGFloat(Double(segment.value) / total)))
                }
            }
        }
        .frame(height: 8)
        .clipShape(Capsule())
    }
}

private struct ShareBar: View {
    let share: Double

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(T3Theme.border)
                Capsule()
                    .fill(T3Theme.tokenInput)
                    .frame(width: max(4, proxy.size.width * CGFloat(min(max(share, 0), 1))))
            }
        }
        .frame(height: 4)
        .accessibilityHidden(true)
    }
}

private struct PeriodControl: View {
    @Binding var selection: UsagePeriod

    var body: some View {
        HStack(spacing: 2) {
            ForEach(UsagePeriod.allCases, id: \.self) { period in
                HoverArea { isHovered in
                    let isSelected = period == selection
                    Button { selection = period } label: {
                        Text(period.shortLabel)
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(isSelected ? T3Theme.textPrimary : isHovered ? T3Theme.textHover : T3Theme.textMuted)
                            .frame(minWidth: 30)
                            .padding(.vertical, 4)
                            .background(RoundedRectangle(cornerRadius: 5, style: .continuous)
                                .fill(isSelected ? T3Theme.rowHover : Color.clear))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .focusable(false)
                    .help(period.label)
                    .accessibilityLabel(period.label)
                    .accessibilityAddTraits(isSelected ? .isSelected : [])
                }
            }
        }
        .padding(2)
        .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).stroke(T3Theme.border, lineWidth: 1))
    }
}
