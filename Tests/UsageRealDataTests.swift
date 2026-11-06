import Foundation
import Testing
@testable import T3CodeBar

/// Runs only with `T3_CODE_BAR_REAL_STATE_DIR` set. It reads that directory and prints the report for each period,
/// in the line format `Tests/Canary/compare.py` produces from the oracle. It never writes anything.
struct UsageRealDataTests {
    private static let realDirectory: URL? = ProcessInfo.processInfo.environment["T3_CODE_BAR_REAL_STATE_DIR"]
        .map { URL(fileURLWithPath: ($0 as NSString).expandingTildeInPath) }

    private static let now = date(ProcessInfo.processInfo.environment["T3_CODE_BAR_REAL_NOW"] ?? "2026-11-06T09:08:00Z")
    private static let zone = TimeZone(identifier: ProcessInfo.processInfo.environment["T3_CODE_BAR_REAL_TZ"] ?? "Asia/Ho_Chi_Minh")!

    @Test(.enabled(if: realDirectory != nil))
    func printsTheRealReportForEveryPeriod() throws {
        let directory = try #require(Self.realDirectory)
        let source = UsageSource(config: EnvironmentConfig(stateDirectory: directory))
        guard case .available(let file) = source.locate() else {
            Issue.record("no usable cache in \(directory.path)")
            return
        }
        let loadStart = Date()
        var dataset = try source.dataset(file)
        if ProcessInfo.processInfo.environment["T3_CODE_BAR_REAL_LOCAL"] == "1" {
            var scanner = LocalUsageScanner()
            let local = scanner.scan(paths: source.localPaths, now: Self.now)
            dataset = local.merging(dataset)
            for status in local.statuses {
                print("LOCAL provider=\(status.kind.raw) state=\(status.state) failed=\(status.failedFiles)")
            }
        }
        let prices = source.prices()
        let records = dataset.files.reduce(0) { $0 + $1.records.count }
        print("REAL cache=\(file.url.lastPathComponent) version=\(dataset.version) files=\(dataset.files.count) records=\(records) "
              + "rejected=\(dataset.rejectedFiles) bytes=\(file.size) loadSeconds=\(String(format: "%.3f", Date().timeIntervalSince(loadStart)))")

        let windows: [(String, UsageWindow)] = [
            ("Past 24h", .rolling24h(now: Self.now, timeZone: Self.zone)),
            ("7 days", .calendarDays(7, now: Self.now, timeZone: Self.zone)),
            ("30 days", .calendarDays(30, now: Self.now, timeZone: Self.zone)),
            ("90 days", .calendarDays(90, now: Self.now, timeZone: Self.zone)),
        ]
        for (label, window) in windows {
            let start = Date()
            let report = UsageReportBuilder.build(dataset: dataset, window: window, prices: prices)
            let seconds = String(format: "%.3f", Date().timeIntervalSince(start))
            print("PERIOD \(label) reportSeconds=\(seconds)")
            for line in Self.lines(report) { print(line) }
        }
    }

    private static func lines(_ report: UsageReport) -> [String] {
        func money(_ value: Double) -> String { String(format: "%.6f", value) }
        let tokens = report.tokens
        var lines = [
            "tokens processed=\(tokens.processed) uncached=\(tokens.uncachedInput) cached=\(tokens.cachedInput) "
                + "creation=\(tokens.cacheCreation) output=\(tokens.output) reasoning=\(tokens.reasoning)",
            "cost total=\(money(report.costUsd)) input=\(money(report.costByType.input)) cacheRead=\(money(report.costByType.cacheRead)) "
                + "cacheWrite=\(money(report.costByType.cacheWrite)) output=\(money(report.costByType.output)) other=\(money(report.costByType.other))",
            "speed standard=\(money(report.costBySpeed.standard)) fast=\(money(report.costBySpeed.fast)) "
                + "ultrafast=\(money(report.costBySpeed.ultrafast)) premium=\(money(report.costBySpeed.premium))",
            "savings \(money(report.cacheSavingsUsd))",
            "sessions \(report.sessions) records \(report.records) unpriced \(report.unpricedRecords)",
        ]
        for provider in report.providers {
            lines.append("provider \(provider.kind.raw) tokens=\(provider.tokens.processed) cost=\(money(provider.costUsd)) sessions=\(provider.sessions)")
        }
        return lines
    }
}
