import SwiftUI

struct UsageSectionHeading: View {
    let title: String

    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title)
            .font(.system(size: 13, weight: .medium))
            .foregroundColor(T3Theme.textPrimary)
            .accessibilityAddTraits(.isHeader)
    }
}

/// A tab with nothing to show, in the style of the threads screen's empty state.
struct UsageStateView: View {
    let symbol: String
    let title: String
    var detail: String?

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: symbol)
                .font(.system(size: 20))
                .foregroundColor(T3Theme.textFaint)
                .accessibilityHidden(true)
            Text(title)
                .font(.system(size: 12))
                .foregroundColor(T3Theme.textMuted)
                .multilineTextAlignment(.center)
            if let detail {
                Text(detail)
                    .font(.system(size: 11))
                    .foregroundColor(T3Theme.textMuted)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 24)
        .accessibilityElement(children: .combine)
    }
}

/// The slices of a bar, in T3 Code's order and one hue at several strengths.
struct UsageSegmentedBar: View {
    struct Segment: Identifiable {
        let id: String
        let value: Double
        let color: Color
    }

    let segments: [Segment]
    private let gap: CGFloat = 2

    var body: some View {
        let visible = segments.filter { $0.value > 0 }
        let total = visible.reduce(0) { $0 + $1.value }
        GeometryReader { proxy in
            let room = proxy.size.width - gap * CGFloat(max(visible.count - 1, 0))
            HStack(spacing: gap) {
                ForEach(visible) { segment in
                    Rectangle()
                        .fill(segment.color)
                        .frame(width: max(1, room * CGFloat(segment.value / total)))
                }
            }
        }
        .frame(height: 8)
        .clipShape(Capsule())
    }
}

struct UsageShareBar: View {
    let share: Double
    var color: Color = T3Theme.tokenInput
    var height: CGFloat = 4

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(T3Theme.border)
                Capsule()
                    .fill(color)
                    .frame(width: max(height, proxy.size.width * CGFloat(min(max(share, 0), 1))))
            }
        }
        .frame(height: height)
        .accessibilityHidden(true)
    }
}

/// One slice of a segmented bar and its legend entry.
struct UsageSlice: Identifiable {
    let label: String
    let amount: Double
    /// The amount as T3 Code writes it, for the legend.
    let text: String
    let spoken: String
    let color: Color

    var id: String { label }
}

/// A bar of slices with its legend, as under T3 Code's "Tokens by type", "Cost by type" and "Cost by speed".
/// Zero slices are left out, and the whole block is hidden when none is left.
struct UsageBreakdown: View {
    let title: String
    let slices: [UsageSlice]
    var aside: String?

    var body: some View {
        let visible = slices.filter { $0.amount > 0 }
        if !visible.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .firstTextBaseline) {
                    UsageSectionHeading(title)
                    Spacer(minLength: 8)
                    if let aside {
                        Text(aside)
                            .font(.system(size: 11))
                            .monospacedDigit()
                            .foregroundColor(T3Theme.textMuted)
                    }
                }
                UsageSegmentedBar(segments: visible.map { .init(id: $0.label, value: $0.amount, color: $0.color) })
                    .accessibilityHidden(true)
                LazyVGrid(columns: [GridItem(.flexible(), spacing: 16), GridItem(.flexible())], alignment: .leading, spacing: 8) {
                    ForEach(visible) { slice in
                        HStack(spacing: 6) {
                            RoundedRectangle(cornerRadius: 2, style: .continuous)
                                .fill(slice.color)
                                .frame(width: 8, height: 8)
                            Text(slice.label)
                                .foregroundColor(T3Theme.textMuted)
                                .lineLimit(1)
                            Spacer(minLength: 4)
                            Text(slice.text)
                                .monospacedDigit()
                                .foregroundColor(T3Theme.textPrimary)
                        }
                        .font(.system(size: 11.5))
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel("\(slice.label), \(slice.spoken)")
                    }
                }
            }
        }
    }
}

struct UsageStatRow: View {
    let label: String
    let value: String
    let spoken: String

    var body: some View {
        HStack {
            Text(label).foregroundColor(T3Theme.textMuted)
            Spacer(minLength: 8)
            Text(value).monospacedDigit().foregroundColor(T3Theme.textPrimary)
        }
        .font(.system(size: 11.5))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(label), \(spoken)")
    }
}

/// The four slices of input and output T3 Code draws, in its order and one hue at four strengths.
enum UsageTokenKind: CaseIterable {
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

/// A segmented control in the style of T3 Code's: a hairline frame and a lifted selected cell.
struct UsageSegmentedControl<Value: Hashable>: View {
    struct Option: Identifiable {
        let value: Value
        let title: String
        /// Spoken and shown as a tooltip when `title` is shortened for the popover.
        let fullTitle: String

        var id: Value { value }
    }

    let options: [Option]
    @Binding var selection: Value
    let groupLabel: String
    var fillsWidth = false

    var body: some View {
        HStack(spacing: 2) {
            ForEach(options) { option in
                HoverArea { isHovered in
                    let isSelected = option.value == selection
                    Button { selection = option.value } label: {
                        Text(option.title)
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(isSelected ? T3Theme.textPrimary : isHovered ? T3Theme.textHover : T3Theme.textMuted)
                            .frame(minWidth: 30, maxWidth: fillsWidth ? .infinity : nil)
                            .padding(.vertical, 4)
                            .background(RoundedRectangle(cornerRadius: 5, style: .continuous)
                                .fill(isSelected ? T3Theme.rowHover : Color.clear))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .focusable(false)
                    .help(option.fullTitle)
                    .accessibilityLabel(option.fullTitle)
                    .accessibilityAddTraits(isSelected ? .isSelected : [])
                }
            }
        }
        .padding(2)
        .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).stroke(T3Theme.border, lineWidth: 1))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(groupLabel)
    }
}

enum UsageOptions {
    static var periods: [UsageSegmentedControl<UsagePeriod>.Option] {
        UsagePeriod.allCases.map { .init(value: $0, title: $0.shortLabel, fullTitle: $0.label) }
    }

    static var tabs: [UsageSegmentedControl<UsageTab>.Option] {
        UsageTab.allCases.map { .init(value: $0, title: $0.label, fullTitle: $0.label) }
    }
}
