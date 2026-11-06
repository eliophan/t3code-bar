import Foundation

/// The words of the Limits tab that are this app's own, and T3 Code's aria text for a bar.
public enum LimitsPresentation {
    /// T3 Code re-probes every 5 minutes by default and every 15 at the slowest. Past twice that the file is
    /// most likely not being updated, because T3 Code is closed.
    public static let staleAfter: TimeInterval = 30 * 60

    public static let footnote = "Read from T3 Code's last check of each provider. This app never contacts a provider."

    /// The sentence shown above the limits when the newest check is old, in words and not just a timestamp.
    public static func staleness(newestCheck: Date?, now: Date) -> String? {
        guard let newestCheck, now.timeIntervalSince(newestCheck) > staleAfter else { return nil }
        return "T3 Code last checked \(LimitsFormat.ago(newestCheck, now: now)). "
            + "These numbers may be out of date. Open T3 Code to refresh them."
    }

    /// `Pro · Checked 5m ago`, with the account's name in front when a provider has several.
    public static func accountLine(_ account: LimitsAccount, showsName: Bool, now: Date) -> String {
        [showsName ? account.name : nil, account.plan.flatMap { $0.isEmpty ? nil : $0 },
         "Checked \(LimitsFormat.ago(account.checkedAt, now: now))"]
            .compactMap { $0 }.joined(separator: " · ")
    }

    /// T3 Code's label for a bar: `Codex: 51% left, resets in 3h 17m, 56% of the window left, 1 reset credit banked`.
    public static func spoken(name: String, member: LimitMember, now: Date) -> String {
        let window = member.window
        var parts = ["\(name): \(LimitsLayout.leftPercent(window))% left"]
        if let resets = LimitsFormat.resetsIn(window, now: now) { parts.append(resets) }
        if let even = LimitsLayout.evenPaceLeft(window, now: now) { parts.append("\(even)% of the window left") }
        if let credits = member.account.resetCredits?.availableCount, credits > 0 {
            parts.append("\(credits) reset \(credits == 1 ? "credit" : "credits") banked")
        }
        return parts.joined(separator: ", ")
    }

    /// The card for an external usage link, as T3 Code words it.
    public static func linkBody(_ link: ExternalUsageLink) -> String? {
        link.url == "https://chatgpt.com/#settings/Usage" ? "View usage in ChatGPT with your connected account." : link.message
    }

    /// Only web links open, and in the browser. Nothing here fetches.
    public static func openableURL(_ link: ExternalUsageLink) -> URL? {
        guard let url = URL(string: link.url), url.scheme == "https" else { return nil }
        return url
    }

    public static func skippedFilesText(_ count: Int) -> String? {
        count > 0 ? "\(count) provider \(count == 1 ? "file" : "files") could not be read." : nil
    }
}
