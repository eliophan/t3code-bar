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
            if let error = usage.readError {
                notice(Self.noticeText(for: error))
            }
            if let snapshot = usage.snapshot {
                if snapshot.isEmpty {
                    emptyState
                } else {
                    hero(snapshot)
                    tokenTypes(snapshot.total)
                    providers(snapshot)
                }
                if let footnote = Self.footnote(partial: snapshot.partialTurns, withoutUsage: snapshot.turnsWithoutUsage) {
                    Text(footnote.text)
                        .font(.system(size: 11))
                        .foregroundColor(T3Theme.textMuted)
                        .help(footnote.help)
                }
            } else if usage.readError == nil {
                ProgressView("Reading usage…")
                    .controlSize(.small)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 28)
            }
        }
        // A read for another period is still running, so these numbers are not for the selected one.
        .opacity(usage.isStale ? 0.45 : 1)
    }

    private func hero(_ snapshot: UsageSnapshot) -> some View {
        let total = snapshot.total
        let summary = Self.summary(turns: snapshot.turns, cacheHit: total.cacheHit)
        return VStack(alignment: .leading, spacing: 4) {
            Text("Processed tokens")
                .font(.system(size: 11.5))
                .foregroundColor(T3Theme.textMuted)
            Text(TokenFormat.compact(total.processed))
                .font(.system(size: 34, weight: .semibold))
                .monospacedDigit()
                .foregroundColor(T3Theme.textPrimary)
            Text(summary)
                .font(.system(size: 11.5))
                .monospacedDigit()
                .foregroundColor(T3Theme.textMuted)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Processed tokens, \(TokenFormat.spoken(total.processed))")
        .accessibilityValue(summary)
    }

    private func tokenTypes(_ total: TokenBreakdown) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            sectionHeading("Tokens by type")
            SegmentedBar(segments: TokenKind.allCases.map {
                SegmentedBar.Segment(id: $0.label, value: $0.value(in: total), color: $0.color)
            })
            .accessibilityHidden(true)
            LazyVGrid(columns: [GridItem(.flexible(), spacing: 16), GridItem(.flexible())],
                      alignment: .leading, spacing: 8) {
                ForEach(TokenKind.allCases, id: \.self) { kind in
                    legendItem(kind, value: kind.value(in: total))
                }
            }
        }
    }

    private func legendItem(_ kind: TokenKind, value: Int) -> some View {
        HStack(spacing: 6) {
            RoundedRectangle(cornerRadius: 2, style: .continuous)
                .fill(kind.color)
                .frame(width: 8, height: 8)
            Text(kind.label)
                .foregroundColor(T3Theme.textMuted)
                .lineLimit(1)
            Spacer(minLength: 4)
            Text(TokenFormat.compact(value))
                .monospacedDigit()
                .foregroundColor(T3Theme.textPrimary)
        }
        .font(.system(size: 11.5))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(kind.label), \(TokenFormat.spoken(value))")
    }

    private func providers(_ snapshot: UsageSnapshot) -> some View {
        let shown = snapshot.providers.filter { $0.tokens.processed > 0 }
        let largest = max(shown.first?.tokens.processed ?? 1, 1)
        return VStack(alignment: .leading, spacing: 12) {
            sectionHeading("By provider")
            ForEach(shown, id: \.provider) { provider in
                providerRow(provider, share: Double(provider.tokens.processed) / Double(largest))
            }
        }
    }

    private func providerRow(_ provider: ProviderUsage, share: Double) -> some View {
        let processed = provider.tokens.processed
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                ProviderIconView(providers: [provider.provider == .unknown ? "?" : provider.provider.rawValue], size: 14)
                Text(provider.provider.displayName)
                    .font(.system(size: 12.5))
                    .foregroundColor(T3Theme.textSecondary)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text(TokenFormat.compact(processed))
                    .font(.system(size: 12.5, weight: .medium))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textPrimary)
                Text("· \(TokenFormat.turns(provider.turns))")
                    .font(.system(size: 10.5))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textMuted)
            }
            ShareBar(share: share)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "\(provider.provider.displayName), \(TokenFormat.spoken(processed)) processed tokens, \(TokenFormat.turns(provider.turns))")
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

    private func notice(_ text: String) -> some View {
        Label(text, systemImage: "exclamationmark.triangle")
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    // MARK: Text

    nonisolated static func noticeText(for error: DatabaseReadError) -> String {
        error == .incompatibleSchema ? "This T3 Code version does not record token usage." : error.localizedDescription
    }

    nonisolated static func summary(turns: Int, cacheHit: Double?, locale: Locale = .current) -> String {
        var parts = [TokenFormat.turns(turns, locale: locale)]
        if let cacheHit { parts.append("\(TokenFormat.percent(cacheHit)) cache hit") }
        return parts.joined(separator: " · ")
    }

    nonisolated static func footnote(partial: Int, withoutUsage: Int) -> (text: String, help: String)? {
        var text: [String] = []
        var help: [String] = []
        if partial > 0 {
            text.append("\(TokenFormat.turns(partial)) reported partial usage.")
            help.append("A provider sent incomplete token counts for these turns. They are included, so totals can read slightly low.")
        }
        if withoutUsage > 0 {
            text.append("\(TokenFormat.turns(withoutUsage)) reported no usage.")
            help.append("A provider sent no token counts for these turns, so they are left out of the totals.")
        }
        return text.isEmpty ? nil : (text.joined(separator: " "), help.joined(separator: " "))
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

    func value(in tokens: TokenBreakdown) -> Int {
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
