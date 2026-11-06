import SwiftUI
import AppKit

/// T3 Code's Limits tab, sized for the popover: a section per provider, a card per window pool.
struct UsageLimitsTab: View {
    let snapshot: LimitsSnapshot
    let now: Date
    let readFailed: Bool

    var body: some View {
        LazyVStack(alignment: .leading, spacing: 18) {
            if readFailed {
                warning("Could not refresh limits. Showing the last read.", symbol: "exclamationmark.triangle")
            }
            if let stale = LimitsPresentation.staleness(newestCheck: snapshot.newestCheck, now: now) {
                warning(stale, symbol: "clock")
            }
            ForEach(snapshot.groups(now: now)) { group in
                LimitsDriverSection(group: group, now: now)
            }
            ForEach(snapshot.links, id: \.url) { link in
                LimitsLinkCard(link: link)
            }
            if !snapshot.notices.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(snapshot.notices, id: \.self) { notice in
                        Label(notice, systemImage: "exclamationmark.triangle")
                            .foregroundColor(T3Theme.statusApproval)
                    }
                }
                .font(.system(size: 12))
                .fixedSize(horizontal: false, vertical: true)
            }
            if let skipped = LimitsPresentation.skippedFilesText(snapshot.skippedFiles) {
                Text(skipped).font(.system(size: 11)).foregroundColor(T3Theme.textMuted)
            }
        }
    }

    private func warning(_ text: String, symbol: String) -> some View {
        Label(text, systemImage: symbol)
            .font(.system(size: 12))
            .foregroundColor(T3Theme.statusApproval)
            .fixedSize(horizontal: false, vertical: true)
    }
}

private struct LimitsDriverSection: View {
    let group: LimitsDriverGroup
    let now: Date

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                ProviderIconView(providers: [group.driver], size: 16)
                Text(group.label)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundColor(T3Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
            }
            ForEach(group.rows) { row in
                LimitCard(row: row, color: UsageProviderKind.color(forDriver: group.driver), now: now)
            }
        }
    }
}

private struct LimitCard: View {
    let row: LimitRow
    let color: Color
    let now: Date

    var body: some View {
        let members = row.columns.compactMap(\.member)
        let shared = members.count > 1
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(row.label)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundColor(T3Theme.textSecondary)
                    .lineLimit(2)
                Spacer(minLength: 4)
                Text("\(row.remainingPercent)%")
                    .font(.system(size: 18, weight: .semibold))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textPrimary)
                Text("left").font(.system(size: 11)).foregroundColor(T3Theme.textMuted)
                if let pace = row.pace {
                    Image(systemName: pace.symbol).font(.system(size: 10)).foregroundColor(T3Theme.textMuted)
                        .help(pace.label).accessibilityLabel(pace.label)
                }
            }
            if shared {
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 6) {
                    ForEach(members, id: \.account.id) { member in
                        MemberBlock(member: member, reset: row.reset(for: member.account.id),
                                    showsAccount: true, color: color, now: now)
                    }
                }
            } else if let member = members.first {
                MemberBlock(member: member, reset: row.reset(for: member.account.id),
                            showsAccount: false, color: color, now: now)
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(T3Theme.border, lineWidth: 1))
    }
}

private struct MemberBlock: View {
    let member: LimitMember
    let reset: LimitReset?
    let showsAccount: Bool
    let color: Color
    let now: Date

    var body: some View {
        let window = member.window
        let left = LimitsLayout.leftPercent(window)
        VStack(alignment: .leading, spacing: 4) {
            if showsAccount || window.resetsAt != nil {
                HStack(spacing: 4) {
                    if showsAccount {
                    Text(member.account.name)
                        .font(.system(size: 10, weight: .medium))
                        .foregroundColor(T3Theme.textPrimary)
                        .lineLimit(1)
                    Text("\(left)%")
                        .font(.system(size: 10, weight: .semibold))
                        .monospacedDigit()
                        .foregroundColor(T3Theme.textPrimary)
                    }
                    Spacer(minLength: 8)
                    if let resets = LimitsFormat.resetsIn(window, now: now) {
                        Text(resets.replacingOccurrences(of: "resets in ", with: "↻ "))
                            .font(.system(size: 10))
                            .monospacedDigit()
                            .foregroundColor(T3Theme.textMuted)
                            .lineLimit(1)
                    }
                }
            }
            LimitBar(left: left, color: color, evenPaceLeft: LimitsLayout.evenPaceLeft(window, now: now),
                     refill: reset != nil && left < 100 ? member.refillPercent : 0)
        }
        .help(LimitsPresentation.spoken(name: member.account.name, member: member, now: now))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(LimitsPresentation.spoken(name: member.account.name, member: member, now: now))
    }
}

/// What is left of a window, as T3 Code draws it: a rounded track, the remainder in the provider's color at
/// 35 percent, a hatched stretch for what a reset will restore, and a line where even spending would be.
private struct LimitBar: View {
    let left: Int
    let color: Color
    let evenPaceLeft: Int?
    let refill: Double

    var body: some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            let tick = evenPaceLeft.map { min(max(CGFloat($0) / 100 * width, 3), width - 3) }
            ZStack(alignment: .leading) {
                ZStack(alignment: .leading) {
                    T3Theme.rowHover
                    color.opacity(0.35).frame(width: width * CGFloat(left) / 100)
                    if refill > 0 {
                        HatchedFill(color: color)
                            .opacity(0.2)
                            .frame(width: width * CGFloat(min(Double(100 - left), refill)) / 100)
                            .frame(maxWidth: .infinity, alignment: .trailing)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                .mask(Gap(center: width >= 6 ? tick : nil).fill(style: FillStyle(eoFill: true)))
                if let tick, width >= 6 {
                    Capsule()
                        .fill(T3Theme.textPrimary)
                        .frame(width: 2, height: 16)
                        .position(x: tick, y: proxy.size.height / 2)
                }
            }
        }
        .frame(height: 12)
        .accessibilityHidden(true)
    }

    /// The whole bar with a 6 point slot cut out around the pace line, so the line reads as a gap in the track.
    private struct Gap: Shape {
        let center: CGFloat?

        func path(in rect: CGRect) -> Path {
            var path = Path(rect)
            if let center { path.addRect(CGRect(x: center - 3, y: rect.minY - 1, width: 6, height: rect.height + 2)) }
            return path
        }
    }
}

private struct HatchedFill: View {
    let color: Color

    var body: some View {
        Canvas { context, size in
            var path = Path()
            var x = -size.height
            while x < size.width {
                path.move(to: CGPoint(x: x, y: 0))
                path.addLine(to: CGPoint(x: x + size.height, y: size.height))
                x += 5
            }
            context.stroke(path, with: .color(color), lineWidth: 1)
        }
        .clipped()
    }
}

private struct LimitsLinkCard: View {
    let link: ExternalUsageLink

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            VStack(alignment: .leading, spacing: 4) {
                Text(link.label)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundColor(T3Theme.textPrimary)
                Text(link.accounts.joined(separator: ", "))
                    .font(.system(size: 11))
                    .foregroundColor(T3Theme.textMuted)
                if let body = LimitsPresentation.linkBody(link) {
                    Text(body)
                        .font(.system(size: 11))
                        .foregroundColor(T3Theme.textMuted)
                }
            }
            .fixedSize(horizontal: false, vertical: true)
            if let url = LimitsPresentation.openableURL(link) {
                HoverArea { isHovered in
                    Button { NSWorkspace.shared.open(url) } label: {
                        HStack(spacing: 4) {
                            Text("Manage usage")
                            Image(systemName: "arrow.up.right.square").font(.system(size: 10))
                        }
                        .font(.system(size: 11.5, weight: .medium))
                        .foregroundColor(isHovered ? T3Theme.textPrimary : T3Theme.textSecondary)
                        .padding(.vertical, 3)
                        .padding(.horizontal, 6)
                        .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(isHovered ? T3Theme.rowHover : Color.clear))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .focusable(false)
                    .accessibilityLabel("Manage usage for \(link.label)")
                }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(T3Theme.border, lineWidth: 1))
    }
}

extension LimitPace {
    /// T3 Code draws a trend-up, trend-down and a neutral icon for the three states.
    var symbol: String {
        switch self {
        case .ahead: return "chart.line.uptrend.xyaxis"
        case .on: return "equal"
        case .under: return "chart.line.downtrend.xyaxis"
        }
    }
}

extension UsageProviderKind {
    /// The color of a provider's bar on the Limits tab, picked by T3 Code's driver name.
    static func color(forDriver driver: String) -> Color {
        known.first { $0.style.driver == driver }?.color ?? T3Theme.textPrimary
    }
}
