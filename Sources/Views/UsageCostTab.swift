import SwiftUI

/// T3 Code's Usage page, Cost tab, sized for the popover.
struct UsageCostTab: View {
    let report: UsageReport

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            hero
            providers
            UsageChartSection(report: report, metric: .cost)
            UsageTotalsSection(report: report)
            UsageBreakdown(title: "Cost by type", slices: UsagePresentation.costBySlices(report).map(\.asSlice))
            UsageBreakdown(title: "Cost by speed", slices: UsagePresentation.speedSlices(report).map(\.asSlice),
                           aside: UsagePresentation.premiumText(report))
            UsageModelsSection(report: report, metric: .cost)
            UsageFootnotes(report: report)
        }
    }

    private var hero: some View {
        let note = UsagePresentation.unpricedNote(report)
        let subtitle = UsagePresentation.costSubtitle(report)
        return VStack(alignment: .leading, spacing: 4) {
            Text(UsagePresentation.costHeadline(report))
                .font(.system(size: 34, weight: .semibold))
                .monospacedDigit()
                .foregroundColor(T3Theme.textPrimary)
                .accessibilityLabel("Cost, \(TokenFormat.spokenCurrency(report.costUsd))")
            HStack(spacing: 4) {
                Text(subtitle)
                    .font(.system(size: 11.5))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textMuted)
                if let note {
                    Image(systemName: "info.circle")
                        .font(.system(size: 10.5))
                        .foregroundColor(T3Theme.textMuted)
                        .help(note)
                        .accessibilityLabel("Unpriced usage details")
                        .accessibilityValue(note)
                }
            }
        }
    }

    private var providers: some View {
        VStack(alignment: .leading, spacing: 14) {
            ForEach(report.providers, id: \.kind) { provider in
                providerRow(provider)
            }
        }
    }

    private func providerRow(_ provider: UsageReport.Provider) -> some View {
        let style = provider.kind.style
        let detail = UsagePresentation.costProviderDetail(provider, in: report)
        let sessions = TokenFormat.sessions(provider.sessions)
        return VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 8) {
                Circle().fill(provider.kind.color).frame(width: 7, height: 7)
                ProviderIconView(providers: [style.driver ?? "?"], size: 14)
                Text(style.label)
                    .font(.system(size: 12.5))
                    .foregroundColor(T3Theme.textPrimary)
                    .lineLimit(1)
                Text(sessions)
                    .font(.system(size: 10.5))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textMuted)
                    .lineLimit(1)
                    .layoutPriority(-1)
                Spacer(minLength: 8)
                Text(TokenFormat.currency(provider.costUsd))
                    .font(.system(size: 12.5, weight: .medium))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textPrimary)
            }
            Text(detail)
                .font(.system(size: 11))
                .monospacedDigit()
                .foregroundColor(T3Theme.textMuted)
                .padding(.leading, 29)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(style.label), \(sessions), \(TokenFormat.spokenCurrency(provider.costUsd)), \(detail)")
    }
}
