import SwiftUI

/// Processed tokens, cached and uncached input, output and cache savings, on both Cost and Tokens.
struct UsageTotalsSection: View {
    let report: UsageReport

    var body: some View {
        let tokens = report.tokens
        VStack(alignment: .leading, spacing: 12) {
            UsageSectionHeading("Totals")
            VStack(spacing: 8) {
                tokenRow("Processed tokens", report.processedTokens)
                tokenRow("Cached input", tokens.cachedInput)
                tokenRow("Uncached input", tokens.uncachedInput)
                tokenRow("Output", tokens.output)
                UsageStatRow(label: "Cache savings", value: TokenFormat.currency(report.cacheSavingsUsd),
                             spoken: TokenFormat.spokenCurrency(report.cacheSavingsUsd))
            }
        }
    }

    private func tokenRow(_ label: String, _ value: Int) -> some View {
        UsageStatRow(label: label, value: TokenFormat.compact(value), spoken: TokenFormat.spoken(value))
    }
}

/// T3 Code's model breakdown. The popover has no room for its table, so each model is two lines and the
/// rows past the first few sit behind "Show all N models".
struct UsageModelsSection: View {
    let report: UsageReport
    let metric: UsageMetric
    @State private var showsAll = false

    var body: some View {
        let rows = UsagePresentation.modelRows(report, metric: metric)
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                UsageSectionHeading("Breakdown")
                Spacer(minLength: 8)
                Text("Model")
                    .font(.system(size: 11))
                    .foregroundColor(T3Theme.textMuted)
                    .accessibilityHidden(true)
            }
            VStack(spacing: 10) {
                ForEach(UsagePresentation.visibleModels(rows, expanded: showsAll)) { row in
                    ModelRowView(row: row, color: row.kind.color)
                }
            }
            if rows.count > UsagePresentation.collapsedModelCount {
                HoverArea { isHovered in
                    Button { withAnimation(.easeInOut(duration: 0.18)) { showsAll.toggle() } } label: {
                        Text(UsagePresentation.modelToggleTitle(count: rows.count, expanded: showsAll))
                            .font(.system(size: 11.5, weight: .medium))
                            .foregroundColor(isHovered ? T3Theme.textPrimary : T3Theme.textSecondary)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 6)
                            .background(RoundedRectangle(cornerRadius: 6, style: .continuous)
                                .fill(isHovered ? T3Theme.rowHover : Color.clear))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .focusable(false)
                }
            }
        }
    }
}

private struct ModelRowView: View {
    let row: UsageModelRow
    let color: Color

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Text("\(row.rank)")
                .font(.system(size: 10.5))
                .monospacedDigit()
                .foregroundColor(T3Theme.textMuted)
                .frame(width: 16, alignment: .trailing)
                .padding(.top, 1.5)
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 6) {
                    ProviderIconView(providers: [row.kind.style.driver ?? "?"], size: 12)
                    Text(row.name)
                        .font(.system(size: 12))
                        .foregroundColor(T3Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 8)
                    Text(row.costText)
                        .font(.system(size: 12, weight: .medium))
                        .monospacedDigit()
                        .foregroundColor(row.isUnpriced ? T3Theme.textMuted : T3Theme.textPrimary)
                }
                HStack(spacing: 8) {
                    if let share = row.barShare {
                        GeometryReader { proxy in
                            Capsule()
                                .fill(color)
                                .frame(width: max(8, proxy.size.width * CGFloat(share)), height: 2)
                                .frame(maxHeight: .infinity)
                        }
                        .frame(height: 8)
                    } else {
                        Spacer(minLength: 0)
                    }
                    Text([row.shareText, row.tokensText].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.system(size: 10.5))
                        .monospacedDigit()
                        .foregroundColor(T3Theme.textMuted)
                        .fixedSize()
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(row.spoken)
    }
}
