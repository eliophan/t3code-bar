import Foundation
import Testing
@testable import T3CodeBar

struct LimitsPresentationTests {
    private let now = date("2026-11-06T12:00:00Z")

    private func account(name: String? = nil, plan: String? = nil, checked: String = "2026-11-06T11:55:00Z",
                         credits: ResetCredits? = nil) -> LimitsAccount {
        LimitsAccount(id: "a", driver: "codex", displayName: name, plan: plan, checkedAt: date(checked),
                      windows: [], resetCredits: credits)
    }

    @Test func aRecentCheckIsNotStaleAndAnOldOneSaysSoInWords() {
        #expect(LimitsPresentation.staleness(newestCheck: date("2026-11-06T11:31:00Z"), now: now) == nil)
        #expect(LimitsPresentation.staleness(newestCheck: nil, now: now) == nil)
        #expect(LimitsPresentation.staleness(newestCheck: date("2026-11-06T09:00:00Z"), now: now)
            == "T3 Code last checked 3h ago. These numbers may be out of date. Open T3 Code to refresh them.")
        #expect(LimitsPresentation.staleness(newestCheck: date("2026-11-04T12:00:00Z"), now: now)?.hasPrefix("T3 Code last checked 2d ago.") == true)
    }

    @Test func theAccountLineHasThePlanAndWhenItWasChecked() {
        #expect(LimitsPresentation.accountLine(account(plan: "Pro"), showsName: false, now: now) == "Pro · Checked 5m ago")
        #expect(LimitsPresentation.accountLine(account(), showsName: false, now: now) == "Checked 5m ago")
        #expect(LimitsPresentation.accountLine(account(name: "Work", plan: "Plus", checked: "2026-11-06T09:00:00Z"), showsName: true, now: now)
            == "Work · Plus · Checked 3h ago")
        #expect(LimitsPresentation.accountLine(account(), showsName: true, now: now) == "Codex · Checked 5m ago")
    }

    @Test func aBarIsSpokenLikeT3sAriaLabel() {
        let window = LimitWindow(id: "primary", kind: .session, label: "Session", usedPercent: 49,
                                 resetsAt: date("2026-11-06T15:00:00Z"), durationMins: 360)
        let member = LimitMember(account: account(credits: ResetCredits(availableCount: 1)), window: window, refillPercent: 49)
        #expect(LimitsPresentation.spoken(name: "Codex", member: member, now: now)
            == "Codex: 51% left, resets in 3h 0m, 50% of the window left, 1 reset credit banked")
        let bare = LimitMember(account: account(), window: LimitWindow(id: "w", kind: .weekly, label: "W", usedPercent: 10), refillPercent: 10)
        #expect(LimitsPresentation.spoken(name: "Codex", member: bare, now: now) == "Codex: 90% left")
    }

    @Test func theChatGPTCardUsesT3sSentenceAndOtherCardsTheProvidersMessage() {
        let chatgpt = ExternalUsageLink(label: "ChatGPT usage", url: "https://chatgpt.com/#settings/Usage", message: "ignored", accounts: ["Codex"])
        #expect(LimitsPresentation.linkBody(chatgpt) == "View usage in ChatGPT with your connected account.")
        let other = ExternalUsageLink(label: "Billing", url: "https://example.com/u", message: "See your plan.", accounts: ["X"])
        #expect(LimitsPresentation.linkBody(other) == "See your plan.")
    }

    @Test func onlyWebLinksCanBeOpened() {
        func link(_ url: String) -> ExternalUsageLink { ExternalUsageLink(label: "L", url: url, message: nil, accounts: []) }
        #expect(LimitsPresentation.openableURL(link("https://chatgpt.com/#settings/Usage")) != nil)
        #expect(LimitsPresentation.openableURL(link("http://example.com")) == nil)
        #expect(LimitsPresentation.openableURL(link("file:///etc/passwd")) == nil)
        #expect(LimitsPresentation.openableURL(link("not a url")) == nil)
    }

    @Test func unreadableFilesAreCountedInWords() {
        #expect(LimitsPresentation.skippedFilesText(0) == nil)
        #expect(LimitsPresentation.skippedFilesText(1) == "1 provider file could not be read.")
        #expect(LimitsPresentation.skippedFilesText(3) == "3 provider files could not be read.")
    }

    @Test func theFootnoteSaysNothingContactsAProvider() {
        #expect(LimitsPresentation.footnote.contains("never contacts a provider"))
    }
}
