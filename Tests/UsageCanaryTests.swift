import Foundation
import Testing
@testable import T3CodeBar

/// The numbers in `Tests/Canary/golden.json` come from the oracle (`repro.py`, a port of T3 Code's Usage page)
/// run on `Tests/Canary/state`. A change that moves any of them fails here.
struct UsageCanaryTests {
    private static let canary = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("Canary")
    private static let zone = TimeZone(identifier: "Asia/Ho_Chi_Minh")!
    private static let now = date("2026-11-06T09:08:00Z")

    private typealias JSON = [String: Any]

    private static func golden() throws -> JSON {
        let data = try Data(contentsOf: canary.appendingPathComponent("golden.json"))
        return try #require(JSONSerialization.jsonObject(with: data) as? JSON)
    }

    private static func window(_ label: String) -> UsageWindow {
        switch label {
        case "Past 24h": return .rolling24h(now: now, timeZone: zone)
        case "7 days": return .calendarDays(7, now: now, timeZone: zone)
        case "30 days": return .calendarDays(30, now: now, timeZone: zone)
        default: return .calendarDays(90, now: now, timeZone: zone)
        }
    }

    private static func report(_ label: String) throws -> UsageReport {
        let state = canary.appendingPathComponent("state")
        guard case .available(let file) = UsageCacheLocator(directory: state).locate() else {
            throw CanaryError.noCache
        }
        let dataset = try UsageCacheReader().load(file)
        return UsageReportBuilder.build(dataset: dataset, window: window(label), prices: UsagePriceSources.load(from: state))
    }

    private enum CanaryError: Error { case noCache }

    private func number(_ json: JSON, _ key: String) throws -> Double {
        try #require((json[key] as? NSNumber)?.doubleValue, "\(key) missing from golden")
    }

    private func expectCents(_ actual: Double, _ expected: Double, _ name: String, _ label: String) {
        #expect(abs(actual - expected) < 1e-9, "\(label) \(name): \(actual) against golden \(expected)")
    }

    @Test(arguments: ["Past 24h", "7 days", "30 days", "90 days"])
    func everyNumberMatchesTheOracle(label: String) throws {
        let periods = try #require(try Self.golden()["periods"] as? JSON)
        let expected = try #require(periods[label] as? JSON)
        let report = try Self.report(label)

        #expect(report.tokens.processed == Int(try number(expected, "totalTokens")), "\(label) processed tokens")
        #expect(report.tokens.uncachedInput == Int(try number(expected, "uncachedInput")))
        #expect(report.tokens.cachedInput == Int(try number(expected, "cachedInput")))
        #expect(report.tokens.cacheCreation == Int(try number(expected, "cacheCreation")))
        #expect(report.tokens.output == Int(try number(expected, "output")))
        #expect(report.tokens.reasoning == Int(try number(expected, "reasoning")))
        #expect(report.records == Int(try number(expected, "records")))
        #expect(report.unpricedRecords == Int(try number(expected, "unpricedRecords")))
        #expect(report.sessions == Int(try number(expected, "sessions")))
        #expect(report.duplicatesDropped == Int(try number(expected, "duplicatesDropped")))
        expectCents(report.costUsd, try number(expected, "costUsd"), "cost", label)
        expectCents(report.cacheSavingsUsd, try number(expected, "cacheSavingsUsd"), "cache savings", label)

        let type = try #require(expected["costByType"] as? JSON)
        expectCents(report.costByType.input, try number(type, "input"), "input cost", label)
        expectCents(report.costByType.cacheRead, try number(type, "cacheRead"), "cache read cost", label)
        expectCents(report.costByType.cacheWrite, try number(type, "cacheWrite"), "cache write cost", label)
        expectCents(report.costByType.output, try number(type, "output"), "output cost", label)
        expectCents(report.costByType.other, try number(type, "other"), "other cost", label)

        let speed = try #require(expected["costBySpeed"] as? JSON)
        expectCents(report.costBySpeed.standard, try number(speed, "standard"), "standard cost", label)
        expectCents(report.costBySpeed.fast, try number(speed, "fast"), "fast cost", label)
        expectCents(report.costBySpeed.ultrafast, try number(speed, "ultrafast"), "ultrafast cost", label)
        expectCents(report.costBySpeed.premium, try number(speed, "premium"), "premium", label)

        let providers = try #require(expected["providers"] as? [String: JSON])
        #expect(Set(report.providers.map(\.kind.raw)) == Set(providers.keys), "\(label) providers shown")
        for provider in report.providers {
            let golden = try #require(providers[provider.kind.raw])
            #expect(provider.tokens.processed == Int(try number(golden, "tokens")), "\(label) \(provider.kind.raw) tokens")
            #expect(provider.sessions == Int(try number(golden, "sessions")), "\(label) \(provider.kind.raw) sessions")
            #expect(provider.records == Int(try number(golden, "records")))
            expectCents(provider.costUsd, try number(golden, "cost"), "\(provider.kind.raw) cost", label)
        }

        let models = try #require(expected["models"] as? [String: JSON])
        #expect(Set(report.models.map { "\($0.kind.raw)/\($0.name)" }) == Set(models.keys), "\(label) models shown")
        for model in report.models {
            let golden = try #require(models["\(model.kind.raw)/\(model.name)"])
            #expect(model.tokens.processed == Int(try number(golden, "tokens")))
            #expect(model.unpricedRecords == Int(try number(golden, "unpriced")))
            expectCents(model.costUsd, try number(golden, "cost"), "\(model.name) cost", label)
        }
    }

    @Test(arguments: ["Past 24h", "7 days", "30 days", "90 days"])
    func theChartColumnsMatchTheOracleAndAddUpToTheTotals(label: String) throws {
        let periods = try #require(try Self.golden()["periods"] as? JSON)
        let series = try #require((periods[label] as? JSON)?["series"] as? JSON)
        let expected = try #require(series["columns"] as? [String: [String: JSON]])
        let report = try Self.report(label)
        let timeline = report.timeline

        #expect(timeline.resolution == (series["resolution"] as? String == "hour" ? .hour : .day), "\(label) resolution")
        #expect(timeline.columns.count == (timeline.resolution == .hour ? 24 : Int(label.prefix { $0.isNumber }) ?? 0),
                "\(label) draws every slot of the window")

        for (index, column) in timeline.columns.enumerated() {
            let key = timeline.resolution == .hour ? String(index) : UsageTimeline.isoDay(column.day)
            let golden = expected[key] ?? [:]
            let seen = Set(column.byProvider.filter { $0.value.tokens > 0 || $0.value.costUsd > 0 }.map(\.key.raw))
            #expect(seen == Set(golden.keys), "\(label) \(key) providers")
            for (provider, amounts) in golden {
                let ours = column.byProvider[UsageProviderKind(raw: provider)] ?? .init()
                #expect(ours.tokens == Int(try number(amounts, "tokens")), "\(label) \(key) \(provider) tokens")
                expectCents(ours.costUsd, try number(amounts, "cost"), "\(key) \(provider) cost", label)
            }
        }

        let tokens = timeline.columns.reduce(0.0) { $0 + $1.total(.tokens) }
        let cost = timeline.columns.reduce(0.0) { $0 + $1.total(.cost) }
        #expect(Int(tokens) == report.processedTokens, "\(label) chart tokens add up to the headline")
        expectCents(cost, report.costUsd, "chart cost against the headline", label)
    }

    @Test func providersAppearInT3CodesOrder() throws {
        let report = try Self.report("90 days")
        #expect(report.providers.map(\.kind) == [.codex, .claude, .grok, .pi, .muse])
    }

    @Test func theSettingsFileIsReadForTwoKeysOnly() {
        let settings = UsagePriceSources.readSettings(Self.canary.appendingPathComponent("state/settings.json"))
        #expect(settings.overrides.rates.keys.sorted() == ["custom-model"])
        #expect(settings.aliases.target(for: "chain-a") == "gpt-6.1-sol")
        #expect(settings.aliases.target(for: "loop-x") == nil)
    }
}
