import Foundation
import Testing
@testable import T3CodeBar

/// A temporary stand-in for `~/.t3/caches`.
final class CachesFixture: @unchecked Sendable {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)

    init() { try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true) }
    deinit { try? FileManager.default.removeItem(at: directory) }

    func write(_ name: String, _ text: String) throws {
        try Data(text.utf8).write(to: directory.appendingPathComponent(name), options: .atomic)
    }

    func write(_ name: String, object: [String: Any]) throws {
        try JSONSerialization.data(withJSONObject: object).write(to: directory.appendingPathComponent(name), options: .atomic)
    }

    func remove(_ name: String) throws { try FileManager.default.removeItem(at: directory.appendingPathComponent(name)) }
}

func limitsWindowObject(_ id: String, kind: String = "weekly", used: Double, reset: String? = nil,
                        minutes: Double? = nil) -> [String: Any] {
    var window: [String: Any] = ["id": id, "kind": kind, "label": id, "usedPercent": used]
    if let reset { window["resetsAt"] = reset }
    if let minutes { window["windowDurationMins"] = minutes }
    return window
}

func limitsProviderObject(_ id: String, driver: String = "codex", enabled: Bool = true, installed: Bool = true,
                          email: String? = nil, workspace: String? = nil, plan: String? = nil,
                          status: String = "authenticated", checkedAt: String = "2026-11-06T09:00:00.000Z",
                          windows: [[String: Any]]? = nil, extra: [String: Any] = [:],
                          limits: [String: Any]? = nil) -> [String: Any] {
    var auth: [String: Any] = ["status": status]
    if let email { auth["email"] = email }
    if let workspace { auth["workspaceId"] = workspace }
    if let plan { auth["label"] = plan }
    var object: [String: Any] = ["instanceId": id, "driver": driver, "enabled": enabled, "installed": installed, "auth": auth]
    var usage: [String: Any] = limits ?? ["checkedAt": checkedAt, "windows": windows ?? [limitsWindowObject("w", used: 10)]]
    for (key, value) in extra { usage[key] = value }
    object["usageLimits"] = usage
    return object
}

struct LimitsReaderTests {
    let now = limitDate("2026-11-06T10:00:00Z")

    private func snapshot(_ fixture: CachesFixture) async throws -> LimitsSnapshot {
        try await LimitsReader(cachesDirectory: fixture.directory).read()
    }

    // MARK: decoding

    @Test func decodesTheCodexFixtureFromTheSpec() async throws {
        let fixture = CachesFixture()
        try fixture.write("codex.json", """
        {"instanceId":"codex","driver":"codex","enabled":true,"installed":true,"auth":{"status":"authenticated","label":"Pro"},"usageLimits":{"checkedAt":"2026-11-06T09:19:26.285Z","windows":[{"id":"primary","kind":"session","label":"Session","usedPercent":100,"windowDurationMins":300,"resetsAt":"2026-11-06T09:20:55.000Z"},{"id":"secondary","kind":"weekly","label":"Weekly","usedPercent":16,"windowDurationMins":10080,"resetsAt":"2026-11-13T04:20:55.000Z"}],"resetCredits":{"availableCount":1,"nextExpiresAt":"2026-12-01T21:34:42.000Z"}}}
        """)
        let result = try await snapshot(fixture)
        #expect(result.state == .ok)
        let account = try #require(result.accounts.first)
        #expect(result.accounts.count == 1)
        #expect(account.name == "Codex")
        #expect(account.plan == "Pro")
        #expect(account.checkedAt == limitDate("2026-11-06T09:19:26.285Z"))
        #expect(account.resetCredits == ResetCredits(availableCount: 1, nextExpiresAt: limitDate("2026-12-01T21:34:42Z")))
        #expect(account.windows.map(\.id) == ["primary", "secondary"])
        #expect(account.windows.map(\.kind) == [.session, .weekly])
        #expect(account.windows[0].durationMins == 300)
        #expect(account.windows[1].resetsAt == limitDate("2026-11-13T04:20:55Z"))
        let rows = try #require(result.groups(now: limitDate("2026-11-06T09:30:00Z")).first).rows
        #expect(rows.map(\.id) == ["session", "weekly"])
        #expect(rows.map(\.remainingPercent) == [0, 84])
    }

    @Test func decodesFourAntigravityWindows() async throws {
        let fixture = CachesFixture()
        try fixture.write("antigravity.json", object: limitsProviderObject("antigravity", driver: "antigravity", plan: "Google account", windows: [
            limitsWindowObject("gemini-5h", kind: "session", used: 0),
            limitsWindowObject("3p-weekly", used: 100, reset: "2026-11-09T17:51:02.000Z"),
            limitsWindowObject("gemini-argon-weekly", used: 98, reset: "2026-11-08T02:02:15.000Z"),
            limitsWindowObject("gemini-weekly", used: 55, reset: "2026-11-08T02:25:07.000Z"),
        ], extra: ["credentialFingerprint": "abc"]))
        let account = try #require(try await snapshot(fixture).accounts.first)
        #expect(account.windows.count == 4)
        #expect(account.plan == "Google account")
        let group = try #require(try await snapshot(fixture).groups(now: now).first)
        #expect(group.rows.map(\.id) == ["gemini-5h", "gemini-weekly", "gemini-argon-weekly", "3p-weekly"])
    }

    @Test func anUnsupportedAccountShowsNeitherBarsNorAWarning() async throws {
        let fixture = CachesFixture()
        try fixture.write("claude.json", object: limitsProviderObject("claudeAgent", driver: "claudeAgent", limits: [
            "checkedAt": "2026-11-06T09:00:00Z", "windows": [], "unavailable": ["reason": "unsupported"],
        ]))
        let result = try await snapshot(fixture)
        #expect(result.accounts.isEmpty)
        #expect(result.notices.isEmpty)
        #expect(result.state == .noLimitsReported)
    }

    @Test func warningsUseT3sWordsAndTheProviderName() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: withName(limitsProviderObject("a", driver: "grok", limits: [
            "checkedAt": "2026-11-06T09:00:00Z", "windows": [], "unavailable": ["reason": "probeFailed"],
        ]), "  Grok  "))
        try fixture.write("b.json", object: limitsProviderObject("b", driver: "kiro", limits: [
            "checkedAt": "2026-11-06T09:00:00Z", "windows": [], "unavailable": ["reason": "probeFailed", "message": "Token expired."],
        ]))
        try fixture.write("c.json", object: limitsProviderObject("c", driver: "pi", windows: []))
        let result = try await snapshot(fixture)
        #expect(result.notices == ["Grok: Could not read limits.", "kiro: Token expired.", "pi: No limits reported."])
        #expect(result.accounts.isEmpty)
        #expect(result.state == .ok)
    }

    private func withName(_ object: [String: Any], _ name: String) -> [String: Any] {
        object.merging(["displayName": name]) { $1 }
    }

    @Test func aFileWithoutUsageLimitsIsIgnoredButNotSkipped() async throws {
        let fixture = CachesFixture()
        try fixture.write("muse.json", #"{"instanceId":"muse","driver":"muse","enabled":true,"installed":true,"auth":{"status":"authenticated"}}"#)
        let result = try await snapshot(fixture)
        #expect(result.accounts.isEmpty)
        #expect(result.skippedFiles == 0)
        #expect(result.state == .noLimitsReported)
    }

    @Test func aCorruptOrForeignFileIsSkippedAndCounted() async throws {
        let fixture = CachesFixture()
        try fixture.write("broken.json", "{not json")
        try fixture.write("list.json", "[1,2]")
        try fixture.write("nodriver.json", #"{"instanceId":"x"}"#)
        try fixture.write("notes.txt", "ignore me")
        try fixture.write("codex.json", object: limitsProviderObject("codex"))
        let result = try await snapshot(fixture)
        #expect(result.accounts.count == 1)
        #expect(result.skippedFiles == 3)
    }

    @Test func unknownKindsAndDriversStillRender() async throws {
        let fixture = CachesFixture()
        try fixture.write("x.json", object: limitsProviderObject("fancy_1", driver: "fancyai", windows: [
            limitsWindowObject("daily", kind: "daily", used: 30), limitsWindowObject("s", kind: "session", used: 5),
        ]))
        let result = try await snapshot(fixture)
        let group = try #require(result.groups(now: now).first)
        #expect(group.label == "fancyai")
        #expect(result.accounts.first?.name == "fancyai")
        #expect(group.rows.map(\.kind) == [.session, .other])
    }

    @Test func aWindowWithoutAUsableNumberIsDropped() async throws {
        let fixture = CachesFixture()
        try fixture.write("x.json", object: limitsProviderObject("x", windows: [
            limitsWindowObject("good", used: 10),
            ["id": "bool", "kind": "weekly", "usedPercent": true],
            ["id": "text", "kind": "weekly", "usedPercent": "50"],
            ["kind": "weekly", "usedPercent": 5],
        ]))
        #expect(try await snapshot(fixture).accounts.first?.windows.map(\.id) == ["good"])
    }

    @Test func percentsOutsideTheRangeKeepTheirValueButShowClamped() async throws {
        let fixture = CachesFixture()
        try fixture.write("x.json", object: limitsProviderObject("x", driver: "grok", windows: [
            limitsWindowObject("over", used: 130), limitsWindowObject("under", kind: "session", used: -10),
        ]))
        let windows = try #require(try await snapshot(fixture).accounts.first).windows
        #expect(windows.map(\.usedPercent) == [130, -10])
        #expect(windows.map(LimitsLayout.leftPercent) == [0, 100])
    }

    @Test func externalUsageBecomesALinkOnlyForAuthenticatedAccounts() async throws {
        let fixture = CachesFixture()
        let usage: [String: Any] = ["checkedAt": "2026-11-06T09:00:00Z", "windows": [],
                                    "externalUsage": ["label": "ChatGPT usage", "url": "https://chatgpt.com/#settings/Usage"],
                                    "unavailable": ["reason": "unsupported", "message": "Open Usage settings."]]
        try fixture.write("codex_a.json", object: withName(limitsProviderObject("codex_a", limits: usage), "Work"))
        try fixture.write("codex_b.json", object: withName(limitsProviderObject("codex_b", limits: usage), "Home"))
        try fixture.write("codex_c.json", object: limitsProviderObject("codex_c", status: "unauthenticated", limits: usage))
        let result = try await snapshot(fixture)
        #expect(result.links == [ExternalUsageLink(label: "ChatGPT usage", url: "https://chatgpt.com/#settings/Usage",
                                                   message: "Open Usage settings.", accounts: ["Work", "Home"])])
        #expect(result.notices.isEmpty)
        #expect(result.state == .ok)
    }

    @Test func aMissingDirectoryIsAStateNotAnError() async throws {
        let reader = LimitsReader(cachesDirectory: FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString))
        #expect(try await reader.read() == LimitsSnapshot(state: .noCacheDirectory))
    }

    @Test func aPlainFileInPlaceOfTheDirectoryIsAnError() async throws {
        let fixture = CachesFixture()
        try fixture.write("not-a-dir", "x")
        let reader = LimitsReader(cachesDirectory: fixture.directory.appendingPathComponent("not-a-dir"))
        await #expect(throws: LimitsReadError.unreadable) { try await reader.read() }
    }

    @Test func thePathFollowsTheStateDirectory() {
        let config = EnvironmentConfig(stateDirectory: URL(fileURLWithPath: "/tmp/t3/userdata"))
        #expect(config.cachesDirectory.path == "/tmp/t3/caches")
    }

    // MARK: T3's visibility rule

    @Test(arguments: [
        ("disabled", false, true, nil as String?), ("not installed", true, false, nil), ("unavailable", true, true, "unavailable"),
    ] as [(String, Bool, Bool, String?)])
    func aHiddenProviderIsNotShown(name: String, enabled: Bool, installed: Bool, availability: String?) async throws {
        let fixture = CachesFixture()
        var object = limitsProviderObject("x", enabled: enabled, installed: installed)
        if let availability { object["availability"] = availability }
        try fixture.write("x.json", object: object)
        let result = try await snapshot(fixture)
        #expect(result.accounts.isEmpty, "\(name)")
        #expect(result.state == .noLimitsReported)
    }

    @Test func availabilityAvailableOrAbsentIsShown() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", driver: "grok").merging(["availability": "available"]) { $1 })
        try fixture.write("b.json", object: limitsProviderObject("b", driver: "kiro"))
        #expect(try await snapshot(fixture).accounts.count == 2)
    }

    // MARK: merge

    @Test func sameDriverAndEmailMergeAndTheNewestCheckWins() async throws {
        let fixture = CachesFixture()
        try fixture.write("codex.json", object: limitsProviderObject("codex", email: "Me@Example.com", windows: [limitsWindowObject("old", used: 10)], limits: nil)
            .merging(["displayName": "Codex"]) { $1 })
        try fixture.write("codex_2.json", object: limitsProviderObject("codex_2", email: " me@example.COM ", plan: "Plus",
                                                                    checkedAt: "2026-11-06T09:30:00.000Z",
                                                                    windows: [limitsWindowObject("new", used: 20)]))
        let result = try await snapshot(fixture)
        let account = try #require(result.accounts.first)
        #expect(result.accounts.count == 1)
        #expect(account.windows.map(\.id) == ["new"])
        #expect(account.checkedAt == limitDate("2026-11-06T09:30:00Z"))
        #expect(account.plan == "Plus")
        #expect(account.name == "Codex")
        #expect(account.id == "codex")
    }

    @Test func resetCreditsComeFromTheNewestRecordThatHasThem() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", email: "me@x.io", checkedAt: "2026-11-06T09:00:00Z",
                                                                  extra: ["resetCredits": ["availableCount": 2]]))
        try fixture.write("b.json", object: limitsProviderObject("b", email: "me@x.io", checkedAt: "2026-11-06T09:30:00Z"))
        #expect(try await snapshot(fixture).accounts.first?.resetCredits == ResetCredits(availableCount: 2))
    }

    @Test func differentWorkspacesOfOneEmailStayApart() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", email: "me@x.io", workspace: "w1"))
        try fixture.write("b.json", object: limitsProviderObject("b", email: "me@x.io", workspace: "w2"))
        try fixture.write("c.json", object: limitsProviderObject("c", email: "me@x.io", workspace: "w1", checkedAt: "2026-11-06T09:40:00Z"))
        let result = try await snapshot(fixture)
        #expect(result.accounts.map(\.id).sorted() == ["a", "b"])
        #expect(result.accounts.first { $0.id == "a" }?.checkedAt == limitDate("2026-11-06T09:40:00Z"))
    }

    @Test func aRecordWithoutAWorkspaceJoinsTheOnlyKnownOne() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", email: "me@x.io", workspace: "w1"))
        try fixture.write("b.json", object: limitsProviderObject("b", email: "me@x.io"))
        #expect(try await snapshot(fixture).accounts.map(\.id) == ["a"])
    }

    @Test func aRecordWithoutAWorkspaceStaysAloneWhenSeveralAreKnown() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", email: "me@x.io", workspace: "w1"))
        try fixture.write("b.json", object: limitsProviderObject("b", email: "me@x.io", workspace: "w2"))
        try fixture.write("c.json", object: limitsProviderObject("c", email: "me@x.io"))
        #expect(try await snapshot(fixture).accounts.map(\.id).sorted() == ["a", "b", "c"])
    }

    @Test func recordsWithoutEmailMergeOnTheCredentialFingerprint() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", driver: "antigravity", extra: ["credentialFingerprint": "f1"]))
        try fixture.write("b.json", object: limitsProviderObject("b", driver: "antigravity", extra: ["credentialFingerprint": "f1"]))
        try fixture.write("c.json", object: limitsProviderObject("c", driver: "antigravity", extra: ["credentialFingerprint": "f2"]))
        try fixture.write("d.json", object: limitsProviderObject("d", driver: "antigravity"))
        try fixture.write("e.json", object: limitsProviderObject("e", driver: "antigravity"))
        #expect(try await snapshot(fixture).accounts.map(\.id) == ["a", "c", "d", "e"])
    }

    @Test func theSameEmailOnTwoDriversDoesNotMerge() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", driver: "codex", email: "me@x.io"))
        try fixture.write("b.json", object: limitsProviderObject("b", driver: "claudeAgent", email: "me@x.io"))
        #expect(try await snapshot(fixture).accounts.count == 2)
    }

    @Test func aHiddenRecordStillTellsTheMergeWhichWorkspacesExist() async throws {
        let fixture = CachesFixture()
        try fixture.write("a.json", object: limitsProviderObject("a", email: "me@x.io", workspace: "w1"))
        try fixture.write("b.json", object: limitsProviderObject("b", installed: false, email: "me@x.io", workspace: "w2"))
        try fixture.write("c.json", object: limitsProviderObject("c", email: "me@x.io"))
        #expect(try await snapshot(fixture).accounts.map(\.id).sorted() == ["a", "c"])
    }

    @Test func accountsRankByNameThenId() async throws {
        let fixture = CachesFixture()
        try fixture.write("1.json", object: withName(limitsProviderObject("1", driver: "grok"), "Zed"))
        try fixture.write("2.json", object: withName(limitsProviderObject("2", driver: "grok"), "alpha"))
        try fixture.write("3.json", object: withName(limitsProviderObject("3", driver: "grok"), "Alpha"))
        let ranks = Dictionary(uniqueKeysWithValues: try await snapshot(fixture).accounts.map { ($0.id, $0.nameOrder) })
        #expect(ranks == ["2": 0, "3": 1, "1": 2])
    }

    // MARK: staleness

    @Test func theCheckTimeIsExposedSoTheViewCanSayHowOldTheBarsAre() async throws {
        let fixture = CachesFixture()
        try fixture.write("codex.json", object: limitsProviderObject("codex", checkedAt: "2026-11-06T07:00:00.000Z"))
        let account = try #require(try await snapshot(fixture).accounts.first)
        #expect(account.age(now: now) == 3 * 3600)
        #expect(LimitsFormat.ago(account.checkedAt, now: now) == "3h ago")
    }

    // MARK: real data

    @Test(.enabled(if: ProcessInfo.processInfo.environment["T3_CODE_BAR_REAL_CACHES_DIR"] != nil))
    func printsTheRealSnapshot() async throws {
        let path = try #require(ProcessInfo.processInfo.environment["T3_CODE_BAR_REAL_CACHES_DIR"])
        let result = try await LimitsReader(cachesDirectory: URL(fileURLWithPath: path)).read()
        let now = Date()
        print("state \(result.state) skipped \(result.skippedFiles)")
        for group in result.groups(now: now) {
            for account in group.accounts {
                print("-- \(group.driver) [\(account.id)] \(account.name) plan=\(account.plan ?? "-") checked \(LimitsFormat.ago(account.checkedAt, now: now))")
                for window in account.windows {
                    let left = LimitsLayout.leftPercent(window)
                    print("     \(window.kind) \(window.label) \(left)% left resets \(window.resetsAt.map { "\($0)" } ?? "-")")
                }
                if let credits = account.resetCredits { print("     \(LimitsFormat.credits(credits, now: now))") }
            }
            for row in group.rows {
                print("   row \(row.kind) \(row.id) \(row.remainingPercent)% left pace \(row.pace.map { "\($0)" } ?? "-")")
            }
        }
        for notice in result.notices { print("notice: \(notice)") }
        for link in result.links { print("link: \(link.label) \(link.accounts)") }
    }
}
