import SwiftUI

/// T3 Code's chart over time for one metric: a line and a faint area per provider, a tooltip on hover.
struct UsageChartSection: View {
    let report: UsageReport
    let metric: UsageMetric

    var body: some View {
        let model = UsageChartModel(timeline: report.timeline, providers: report.providers.map(\.kind), metric: metric)
        VStack(alignment: .leading, spacing: 10) {
            UsageSectionHeading(model.heading)
            UsageChartView(model: model)
        }
    }
}

struct UsageChartView: View {
    let model: UsageChartModel
    static let plotHeight: CGFloat = 112
    private static let axisWidth: CGFloat = 40

    @State private var hover: (index: Int, point: CGPoint)?
    @State private var tooltipSize = CGSize.zero

    var body: some View {
        VStack(spacing: 4) {
            HStack(alignment: .top, spacing: 6) {
                axis
                plot
            }
            HStack {
                ForEach(Array(model.axisLabels.enumerated()), id: \.offset) { index, label in
                    if index > 0 { Spacer(minLength: 0) }
                    Text(label)
                }
            }
            .font(.system(size: 9))
            .foregroundColor(T3Theme.textMuted)
            .padding(.leading, Self.axisWidth + 6)
            .accessibilityHidden(true)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(model.spoken)
    }

    private var axis: some View {
        ZStack(alignment: .topTrailing) {
            Color.clear
            ForEach(Array(model.ticks.enumerated()), id: \.offset) { _, tick in
                Text(tick.label)
                    .font(.system(size: 9))
                    .monospacedDigit()
                    .foregroundColor(T3Theme.textMuted)
                    .fixedSize()
                    .offset(y: Self.y(tick.y) - 5.5)
            }
        }
        .frame(width: Self.axisWidth, height: Self.plotHeight)
        .accessibilityHidden(true)
    }

    private static func y(_ unit: Double) -> CGFloat { CGFloat(unit / UsageChartModel.height) * plotHeight }

    private var plot: some View {
        GeometryReader { proxy in
            let size = proxy.size
            ZStack(alignment: .topLeading) {
                ForEach(Array(model.ticks.enumerated()), id: \.offset) { _, tick in
                    Path { path in
                        path.move(to: CGPoint(x: 0, y: Self.y(tick.y)))
                        path.addLine(to: CGPoint(x: size.width, y: Self.y(tick.y)))
                    }
                    .stroke(T3Theme.border, lineWidth: 1)
                }
                ForEach(model.series, id: \.provider) { series in
                    Path { path in Self.addArea(series, to: &path, size: size) }
                        .fill(series.provider.color.opacity(0.12))
                    Path { path in Self.addLine(series, to: &path, size: size) }
                        .stroke(series.provider.color, style: StrokeStyle(lineWidth: 2, lineJoin: .round))
                }
                if let hover, model.columns.count > 0 {
                    let x = Self.x(hover.index, count: model.columns.count, width: size.width)
                    Path { path in
                        path.move(to: CGPoint(x: x, y: Self.y(8)))
                        path.addLine(to: CGPoint(x: x, y: Self.plotHeight))
                    }
                    .stroke(T3Theme.textMuted, lineWidth: 1)
                    UsageChartTooltip(column: model.columns[hover.index])
                        .background(GeometryReader { Color.clear.preference(key: TooltipSizeKey.self, value: $0.size) })
                        .onPreferenceChange(TooltipSizeKey.self) { tooltipSize = $0 }
                        .offset(tooltipOffset(for: hover.point, in: size))
                        .allowsHitTesting(false)
                }
            }
            .contentShape(Rectangle())
            .onContinuousHover { phase in
                switch phase {
                case .active(let point):
                    guard size.width > 0, let index = model.columnIndex(atFraction: Double(point.x / size.width)) else { return }
                    hover = (index, point)
                case .ended:
                    hover = nil
                }
            }
        }
        .frame(height: Self.plotHeight)
    }

    private static func x(_ index: Int, count: Int, width: CGFloat) -> CGFloat {
        count < 2 ? 0 : width * CGFloat(index) / CGFloat(count - 1)
    }

    private static func point(_ unit: UsageChartModel.Point, size: CGSize) -> CGPoint {
        CGPoint(x: CGFloat(unit.x / UsageChartModel.width) * size.width, y: Self.y(unit.y))
    }

    private static func addLine(_ series: UsageChartModel.Series, to path: inout Path, size: CGSize) {
        guard let first = series.curves.first else { return }
        path.move(to: point(first.from, size: size))
        for curve in series.curves {
            path.addCurve(to: point(curve.to, size: size), control1: point(curve.control1, size: size),
                          control2: point(curve.control2, size: size))
        }
    }

    private static func addArea(_ series: UsageChartModel.Series, to path: inout Path, size: CGSize) {
        guard series.curves.first != nil else { return }
        addLine(series, to: &path, size: size)
        path.addLine(to: CGPoint(x: size.width, y: plotHeight))
        path.addLine(to: CGPoint(x: 0, y: plotHeight))
        path.closeSubpath()
    }

    /// 12 points from the pointer, on the other side when it would run off the plot, as T3 Code places it.
    private func tooltipOffset(for point: CGPoint, in size: CGSize) -> CGSize {
        let right = point.x + 12 + tooltipSize.width <= size.width
        let below = point.y + 12 + tooltipSize.height <= size.height
        let x = right ? point.x + 12 : point.x - 12 - tooltipSize.width
        let y = below ? point.y + 12 : point.y - 12 - tooltipSize.height
        return CGSize(width: min(max(0, x), max(0, size.width - tooltipSize.width)),
                      height: min(max(0, y), max(0, size.height - tooltipSize.height)))
    }
}

private struct TooltipSizeKey: PreferenceKey {
    static let defaultValue = CGSize.zero
    static func reduce(value: inout CGSize, nextValue: () -> CGSize) { value = nextValue() }
}

struct UsageChartTooltip: View {
    let column: UsageChartModel.Column

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(column.title)
                .foregroundColor(T3Theme.textMuted)
                .padding(.bottom, 1)
            ForEach(column.rows, id: \.provider) { row in
                HStack(spacing: 12) {
                    HStack(spacing: 5) {
                        ProviderIconView(providers: [row.provider.style.driver ?? "?"], size: 10)
                        Text(row.provider.style.label).foregroundColor(T3Theme.textMuted)
                    }
                    Spacer(minLength: 0)
                    Text(row.text).monospacedDigit().foregroundColor(T3Theme.textPrimary)
                }
            }
            HStack {
                Text("Total").foregroundColor(T3Theme.textMuted)
                Spacer(minLength: 12)
                Text(column.totalText).monospacedDigit().foregroundColor(T3Theme.textPrimary)
            }
            .padding(.top, 3)
            .overlay(alignment: .top) { T3Theme.border.frame(height: 1) }
        }
        .font(.system(size: 10.5))
        .lineLimit(1)
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .frame(minWidth: 130)
        .fixedSize()
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(T3Theme.tooltip))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(T3Theme.border, lineWidth: 1))
        .shadow(color: .black.opacity(0.5), radius: 8, y: 2)
    }
}
