import Foundation
import Testing
@testable import T3CodeBar

struct UsageTabTests {
    private func freshDefaults() -> UserDefaults {
        UserDefaults(suiteName: "usage-tab-\(UUID().uuidString)")!
    }

    @Test func tabsAreInT3sOrderWithItsLabels() {
        #expect(UsageTab.allCases.map(\.label) == ["Cost", "Tokens", "Limits"])
    }

    @Test @MainActor func firstLaunchOpensOnLimitsAndThirtyDaysLikeT3() {
        let defaults = freshDefaults()
        #expect(UsageScreenModel(defaults: defaults).tab == .limits)
        #expect(UsagePeriod.default == .month)
    }

    @Test @MainActor func theChosenTabIsRememberedForTheNextVisit() {
        let defaults = freshDefaults()
        UsageScreenModel(defaults: defaults).tab = .cost
        #expect(UsageScreenModel(defaults: defaults).tab == .cost)
        let second = UsageScreenModel(defaults: defaults)
        second.tab = .tokens
        #expect(UsageScreenModel(defaults: defaults).tab == .tokens)
    }

    @Test @MainActor func anUnknownStoredTabFallsBackToTheDefault() {
        let defaults = freshDefaults()
        defaults.set("budget", forKey: UsageScreenModel.tabKey)
        #expect(UsageScreenModel(defaults: defaults).tab == .limits)
    }

    @Test func onlyCostAndTokensShowThePeriodControl() {
        #expect(UsageTab.cost.showsPeriod)
        #expect(UsageTab.tokens.showsPeriod)
        #expect(!UsageTab.limits.showsPeriod)
    }

    @Test @MainActor func thePeriodChoiceIsRememberedAlongsideTheTab() {
        let defaults = freshDefaults()
        let engine = UsageEngine(source: UsageSource(config: EnvironmentConfig(stateDirectory: temporaryDirectory()), localPaths: .empty))
        UsageMonitor(engine: engine, defaults: defaults).period = .quarter
        #expect(UsageMonitor(engine: engine, defaults: defaults).period == .quarter)
    }
}

struct UsageContentStateTests {
    @Test func scanTabsWaitWhileNothingHasBeenReadYet() {
        #expect(UsageContentState.scan(report: nil, notice: nil) == .loading)
    }

    @Test func scanTabsExplainAMissingCacheAndAnUnsupportedVersionSeparately() {
        #expect(UsageContentState.scan(report: nil, notice: .missing) == .missing)
        #expect(UsageContentState.scan(report: nil, notice: .unsupported(12)) == .unsupported(12))
        #expect(UsageContentState.scan(report: nil, notice: .retrying) == .unreadable)
    }

    @Test func aPeriodWithoutRecordsIsEmptyAndOneWithRecordsHasContent() {
        #expect(UsageContentState.scan(report: UsageReport(), notice: nil) == .empty)
        var report = UsageReport()
        report.records = 3
        #expect(UsageContentState.scan(report: report, notice: nil) == .content)
    }

    @Test func aRetryNeverHidesNumbersThatWereAlreadyRead() {
        var report = UsageReport()
        report.records = 1
        #expect(UsageContentState.scan(report: report, notice: .retrying) == .content)
    }

    @Test func limitsWorkWithoutAScanCache() {
        let snapshot = LimitsSnapshot(accounts: [], notices: ["Could not read limits."], state: .ok)
        #expect(UsageContentState.limits(snapshot: snapshot, error: nil) == .content)
    }

    @Test func limitsSeparateNoDirectoryFromNothingReported() {
        #expect(UsageContentState.limits(snapshot: LimitsSnapshot(state: .noCacheDirectory), error: nil) == .missing)
        #expect(UsageContentState.limits(snapshot: LimitsSnapshot(state: .noLimitsReported), error: nil) == .empty)
    }

    @Test func limitsShowLoadingUntilTheFirstReadAndAnErrorOnlyWithoutAGoodCopy() {
        #expect(UsageContentState.limits(snapshot: nil, error: nil) == .loading)
        #expect(UsageContentState.limits(snapshot: nil, error: .unreadable) == .unreadable)
        let good = LimitsSnapshot(notices: ["No limits reported."], state: .ok)
        #expect(UsageContentState.limits(snapshot: good, error: .unreadable) == .content)
    }
}
