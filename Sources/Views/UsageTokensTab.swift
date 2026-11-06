import SwiftUI

/// T3 Code's Usage page, Tokens tab, sized for the popover.
struct UsageTokensTab: View {
    let report: UsageReport

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            hero
            UsageBreakdown(title: "Tokens by type", slices: Self.tokenSlices(report.tokens))
            UsageTotalsSection(report: report)
            providers
            UsageChartSection(report: report, metric: .tokens)
            UsageModelsSection(report: report, metric: .tokens)
            UsageFootnotes(report: report)
        }
    }

    private var hero: some View {
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

    private var providers: some View {
        let largest = max(report.providers.map(\.tokens.processed).max() ?? 1, 1)
        return VStack(alignment: .leading, spacing: 12) {
            UsageSectionHeading("By provider")
            ForEach(report.providers, id: \.kind) { provider in
                providerRow(provider, share: Double(provider.tokens.processed) / Double(largest))
            }
        }
    }

    private func providerRow(_ provider: UsageReport.Provider, share: Double) -> some View {
        let processed = provider.tokens.processed
        let style = provider.kind.style
        let detail = UsageView.providerDetail(tokens: processed, total: report.processedTokens, costUsd: provider.costUsd)
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
            UsageShareBar(share: share)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(style.label), \(TokenFormat.spoken(processed)) processed tokens, \(detail)")
    }

    static func tokenSlices(_ tokens: UsageReport.Tokens) -> [UsageSlice] {
        UsageTokenKind.allCases.map {
            let value = $0.value(in: tokens)
            return UsageSlice(label: $0.label, amount: Double(value), text: TokenFormat.compact(value),
                              spoken: TokenFormat.spoken(value), color: $0.color)
        }
    }
}

/// Partial-data warnings and how old the numbers are, under every tab that reads the scan cache.
struct UsageFootnotes: View {
    let report: UsageReport

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let partial = UsageView.partialText(report) {
                Text(partial)
                    .help("These records are left out of the totals.")
            }
            if report.localSources.contains(where: { $0.state == .partial || $0.state == .unreadable }) {
                Text("Some local usage sources could not be read.")
            }
            if !report.scanCacheMissing {
                Text(UsageView.updatedText(modified: report.cacheModified, now: Date()))
                    .help(UsageView.updatedHelp)
            }
        }
        .font(.system(size: 11))
        .foregroundColor(T3Theme.textMuted)
    }
}
