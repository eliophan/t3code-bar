import Foundation
import Testing
@testable import T3CodeBar

func limitDate(_ iso: String) -> Date { LimitsDate.parse(iso)! }

func limitWindow(_ id: String, _ kind: LimitWindowKind, used: Double, reset: String? = nil,
                 minutes: Double? = nil, label: String? = nil) -> LimitWindow {
    LimitWindow(id: id, kind: kind, label: label ?? id, usedPercent: used,
                resetsAt: reset.map(limitDate), durationMins: minutes)
}

func limitAccount(_ driver: String, id: String = "a", _ windows: [LimitWindow],
                  checkedAt: String = "2026-11-06T09:00:00Z", credits: ResetCredits? = nil,
                  nameOrder: Int = 0) -> LimitsAccount {
    LimitsAccount(id: id, driver: driver, checkedAt: limitDate(checkedAt), windows: windows,
                  resetCredits: credits, nameOrder: nameOrder)
}

func limitAccount(id: String = "a", _ windows: [LimitWindow], checkedAt: String = "2026-11-06T09:00:00Z",
                  credits: ResetCredits? = nil, nameOrder: Int = 0) -> LimitsAccount {
    limitAccount("codex", id: id, windows, checkedAt: checkedAt, credits: credits, nameOrder: nameOrder)
}

struct LimitsLayoutTests {
    let now = limitDate("2026-11-06T10:00:00Z")

    @Test(arguments: [
        (12.4, 88), (12.5, 88), (12.6, 87), (99.5, 1), (0.5, 100), (100, 0), (0, 100), (-20, 100), (140, 0),
    ] as [(Double, Int)])
    func leftRoundsHalfUpAndClampsUsed(used: Double, left: Int) {
        #expect(LimitsLayout.leftPercent(limitWindow("w", .weekly, used: used)) == left)
    }

    @Test(arguments: [
        (55.0, LimitPace.on), (55.5, .ahead), (45.0, .on), (44.9, .under), (0, .under), (100, .ahead),
    ] as [(Double, LimitPace)])
    func paceChangesOnlyBeyondFivePoints(used: Double, pace: LimitPace) {
        let window = limitWindow("w", .weekly, used: used, reset: "2026-11-06T11:00:00Z", minutes: 120)
        #expect(LimitsLayout.elapsedFraction(window, now: now) == 0.5)
        #expect(LimitsLayout.pace(window, now: now) == pace)
    }

    @Test func evenPaceRoundsHalfUp() {
        let window = limitWindow("w", .weekly, used: 0, reset: "2026-11-06T18:45:00Z", minutes: 600)
        #expect(LimitsLayout.elapsedFraction(window, now: now) == 0.125)
        #expect(LimitsLayout.evenPaceLeft(window, now: now) == 88)
    }

    @Test func aWindowWithoutResetOrDurationHasNoPace() {
        #expect(LimitsLayout.pace(limitWindow("w", .weekly, used: 50), now: now) == nil)
        #expect(LimitsLayout.pace(limitWindow("w", .weekly, used: 50, reset: "2026-11-07T00:00:00Z"), now: now) == nil)
        #expect(LimitsLayout.pace(limitWindow("w", .weekly, used: 50, reset: "2026-11-07T00:00:00Z", minutes: 0), now: now) == nil)
    }

    @Test func aWindowPastItsResetKeepsItsPercentAndReadsResetsNow() throws {
        let window = limitWindow("s", .session, used: 80, reset: "2026-11-06T09:00:00Z", minutes: 300)
        #expect(LimitsLayout.elapsedFraction(window, now: now) == 1)
        #expect(LimitsLayout.evenPaceLeft(window, now: now) == 0)
        #expect(LimitsFormat.resetsIn(window, now: now) == "resets now")
        let row = try #require(LimitsLayout.groups([limitAccount([window])], now: now).first?.rows.first)
        #expect(row.usedPercent == 80)
        #expect(row.remainingPercent == 20)
    }

    @Test func unknownKindsMapToOther() {
        #expect(LimitWindowKind(raw: "daily") == .other)
        #expect(LimitWindowKind(raw: nil) == .other)
        #expect(LimitWindowKind(raw: "monthly") == .monthly)
    }

    // MARK: the session-at-100% rule

    private func codex(session: Double, sessionReset: String, weekly: Double, weeklyReset: String,
                       checkedAt: String = "2026-11-06T09:00:00Z") -> LimitsAccount {
        limitAccount([limitWindow("primary", .session, used: session, reset: sessionReset, minutes: 300),
                      limitWindow("secondary", .weekly, used: weekly, reset: weeklyReset, minutes: 10_080)],
                     checkedAt: checkedAt)
    }

    @Test func codexSessionShowsFullWhileWeeklyIsExhausted() throws {
        let account = codex(session: 40, sessionReset: "2026-11-06T12:00:00Z", weekly: 100, weeklyReset: "2026-11-10T00:00:00Z")
        let session = LimitsLayout.resolve(account, account.windows[0], now: now)
        #expect(session.window.usedPercent == 100)
        #expect(session.window.resetsAt == limitDate("2026-11-10T00:00:00Z"))
        #expect(session.window.durationMins == nil)
        #expect(session.refillPercent == 100)
        let weekly = LimitsLayout.resolve(account, account.windows[1], now: now)
        #expect(weekly.window == account.windows[1])
        #expect(weekly.refillPercent == 100)
    }

    @Test func aSessionThatResetsAfterTheWeeklyRestoresOnlyItsFreeShare() {
        let account = codex(session: 40, sessionReset: "2026-11-12T00:00:00Z", weekly: 100, weeklyReset: "2026-11-10T00:00:00Z")
        let session = LimitsLayout.resolve(account, account.windows[0], now: now)
        #expect(session.window.resetsAt == limitDate("2026-11-10T00:00:00Z"))
        #expect(session.refillPercent == 60)
        let exhausted = codex(session: 100, sessionReset: "2026-11-12T00:00:00Z", weekly: 100, weeklyReset: "2026-11-10T00:00:00Z")
        let both = LimitsLayout.resolve(exhausted, exhausted.windows[0], now: now)
        #expect(both.window.resetsAt == limitDate("2026-11-12T00:00:00Z"))
        #expect(both.refillPercent == 100)
    }

    @Test func aResetInThePastLeavesTheFullBarWithoutAResetTime() {
        let account = codex(session: 40, sessionReset: "2026-11-06T09:30:00Z", weekly: 100, weeklyReset: "2026-11-10T00:00:00Z")
        let session = LimitsLayout.resolve(account, account.windows[0], now: now)
        #expect(session.window.usedPercent == 100)
        #expect(session.window.resetsAt == nil)
        #expect(session.refillPercent == 0)
    }

    @Test func aCheckTimeAfterNowMovesTheFloorForward() {
        let account = codex(session: 40, sessionReset: "2026-11-06T12:00:00Z", weekly: 100, weeklyReset: "2026-11-10T00:00:00Z",
                            checkedAt: "2026-11-06T13:00:00Z")
        #expect(LimitsLayout.resolve(account, account.windows[0], now: now).window.resetsAt == nil)
    }

    @Test func claudeUsesTheSevenDayWindowAndOtherDriversUseNothing() {
        let windows = [limitWindow("five_hour", .session, used: 10, reset: "2026-11-06T12:00:00Z", minutes: 300),
                       limitWindow("seven_day", .weekly, used: 100, reset: "2026-11-09T00:00:00Z", minutes: 10_080)]
        let claude = limitAccount("claudeAgent", windows)
        #expect(LimitsLayout.resolve(claude, windows[0], now: now).window.usedPercent == 100)
        let other = limitAccount("grok", windows)
        #expect(LimitsLayout.resolve(other, windows[0], now: now).window.usedPercent == 10)
    }

    // MARK: ordering and pools

    @Test func rowsSortBySessionWeeklyMonthlyOther() throws {
        let account = limitAccount("grok", [
            limitWindow("o", .other, used: 1), limitWindow("m", .monthly, used: 1),
            limitWindow("w", .weekly, used: 1), limitWindow("s", .session, used: 1),
        ])
        let rows = try #require(LimitsLayout.groups([account], now: now).first).rows
        #expect(rows.map(\.id) == ["s", "w", "m", "o"])
    }

    @Test func accountsOfOneDriverShareRowsAndSortBySoonestReset() throws {
        let later = limitAccount(id: "later", [limitWindow("p", .session, used: 20, reset: "2026-11-06T14:00:00Z", minutes: 300)], nameOrder: 0)
        let sooner = limitAccount(id: "sooner", [limitWindow("p", .session, used: 41, reset: "2026-11-06T12:00:00Z", minutes: 300)], nameOrder: 1)
        let group = try #require(LimitsLayout.groups([later, sooner], now: now).first)
        #expect(group.accounts.map(\.id) == ["sooner", "later"])
        let row = try #require(group.rows.first)
        #expect(group.rows.count == 1)
        #expect(row.id == "session")
        #expect(row.usedPercent == 31)
        #expect(row.remainingPercent == 70)
        #expect(row.resets.map(\.accountID) == ["sooner", "later"])
        #expect(row.resets.map(\.restoresPercent) == [21, 10])
    }

    @Test func equalResetsFallBackToTheNameOrder() throws {
        let windows = [limitWindow("p", .session, used: 0, reset: "2026-11-06T12:00:00Z", minutes: 300)]
        let first = limitAccount(id: "b", windows, nameOrder: 0)
        let second = limitAccount(id: "a", windows, nameOrder: 1)
        #expect(try #require(LimitsLayout.groups([second, first], now: now).first).accounts.map(\.id) == ["b", "a"])
    }

    @Test func driversKeepTheirFirstSeenOrder() {
        let w = [limitWindow("x", .weekly, used: 1)]
        let groups = LimitsLayout.groups([limitAccount("grok", id: "1", w), limitAccount("codex", id: "2", w),
                                          limitAccount("grok", id: "3", w)], now: now)
        #expect(groups.map(\.driver) == ["grok", "codex"])
        #expect(groups[0].accounts.map(\.id).sorted() == ["1", "3"])
    }

    @Test func aDriverWithTheSamePoolTwiceInOneAccountKeepsBothRows() throws {
        let account = limitAccount("grok", [limitWindow("a", .weekly, used: 10), limitWindow("b", .weekly, used: 20)])
        let rows = try #require(LimitsLayout.groups([account], now: now).first).rows
        #expect(rows.map(\.id) == ["a", "b"])
    }

    @Test func antigravityListsGeminiFirstThenByStemThenKind() throws {
        let account = limitAccount("antigravity", [
            limitWindow("3p-weekly", .weekly, used: 1), limitWindow("gemini-pro-weekly", .weekly, used: 1),
            limitWindow("gemini-pro-5h", .session, used: 1), limitWindow("gemini-flash-weekly", .weekly, used: 1),
        ])
        let rows = try #require(LimitsLayout.groups([account], now: now).first).rows
        #expect(rows.map(\.id) == ["gemini-flash-weekly", "gemini-pro-5h", "gemini-pro-weekly", "3p-weekly"])
    }

    @Test func cursorDropsOverallWhenBothPoolsExistAndOrdersThePools() throws {
        func ids(_ names: [String]) throws -> [String] {
            let account = limitAccount("cursor", names.map { limitWindow($0, .monthly, used: 1) })
            return try #require(LimitsLayout.groups([account], now: now).first).rows.map(\.id)
        }
        #expect(try ids(["apiPercentUsed", "totalPercentUsed", "autoPercentUsed"]) == ["autoPercentUsed", "apiPercentUsed"])
        #expect(try ids(["totalPercentUsed", "autoPercentUsed"]) == ["totalPercentUsed", "autoPercentUsed"])
    }

    @Test func aRowReportsPaceFromTheMembersThatHaveOne() throws {
        let paced = limitWindow("p", .session, used: 80, reset: "2026-11-06T11:00:00Z", minutes: 120)
        let plain = limitWindow("p", .session, used: 0)
        let group = try #require(LimitsLayout.groups([limitAccount(id: "1", [paced]), limitAccount(id: "2", [plain])], now: now).first)
        #expect(group.rows.first?.pace == .ahead)
        #expect(group.rows.first?.usedPercent == 40)
        #expect(group.rows.first?.columns.count == 2)
    }

    @Test func aMissingWindowLeavesAnEmptyColumn() throws {
        let one = limitAccount(id: "1", [limitWindow("p", .session, used: 10), limitWindow("q", .weekly, used: 10)])
        let two = limitAccount(id: "2", [limitWindow("p", .session, used: 10)])
        let group = try #require(LimitsLayout.groups([one, two], now: now).first)
        let weekly = try #require(group.rows.first { $0.kind == .weekly })
        #expect(weekly.columns.map { $0.member == nil } == [false, true])
    }
}
