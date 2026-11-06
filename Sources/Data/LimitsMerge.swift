import Foundation

/// What the app keeps from one `caches/<instanceId>.json` file. Internal on purpose:
/// it holds the account email and credential fingerprint, which only build the merge key.
struct LimitsRecord: Equatable, Sendable {
    struct Limits: Equatable, Sendable {
        var checkedAt: Date
        var windows: [LimitWindow]
        var credentialFingerprint: String?
        var resetCredits: ResetCredits?
        var externalUsage: ExternalUsage?
        var unavailableReason: String?
        var unavailableMessage: String?

        /// T3's `X`: why an account shows a warning instead of bars, or nil when it has bars.
        var notice: String? {
            switch unavailableReason {
            case "unsupported": return unavailableMessage ?? "This account has no subscription limits."
            case "probeFailed": return unavailableMessage ?? "Could not read limits."
            default: return windows.isEmpty ? "No limits reported." : nil
            }
        }
    }

    var instanceID: String
    var driver: String
    var enabled: Bool
    var installed: Bool
    /// `availability !== "unavailable"`, which is T3's `p(e)`.
    var available: Bool
    var displayName: String?
    var authenticated: Bool
    var plan: String?
    var email: String?
    var workspaceID: String?
    var limits: Limits?
    var isSource = false

    /// T3's `V`: `enabled && installed && p(e) && usageLimits !== undefined`.
    var isVisible: Bool { enabled && installed && available && limits != nil }

    var trimmedName: String? {
        let name = displayName?.trimmingCharacters(in: .whitespacesAndNewlines)
        return name?.isEmpty == false ? name : nil
    }
}

/// T3's account merge (`wt`) for the one environment this app sees.
enum LimitsMerge {
    static func snapshot(_ records: [LimitsRecord], skippedFiles: Int = 0, sourceNotices: [String] = []) -> LimitsSnapshot {
        let visible = records.filter(\.isVisible)
        let accounts = mergedAccounts(visible, mergeKey: mergeKeys(for: records.filter(\.enabled)))
        let notices = sourceNotices + visible.filter { !$0.isSource }.compactMap { record -> String? in
            guard let limits = record.limits, limits.unavailableReason != "unsupported",
                  let notice = limits.notice else { return nil }
            return "\(record.trimmedName ?? record.driver): \(notice)"
        }
        let links = externalLinks(visible)
        let state: LimitsState = accounts.isEmpty && notices.isEmpty && links.isEmpty ? .noLimitsReported : .ok
        return LimitsSnapshot(accounts: accounts, notices: notices, links: links, state: state, skippedFiles: skippedFiles)
    }

    /// T3's `H`: `driver:email`, else `driver:credential:<fingerprint>`.
    private static func baseKey(_ record: LimitsRecord) -> String? {
        if let email = record.email?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(), !email.isEmpty {
            return "\(record.driver):\(email)"
        }
        if let fingerprint = record.limits?.credentialFingerprint, !fingerprint.isEmpty {
            return "\(record.driver):credential:\(fingerprint)"
        }
        return nil
    }

    /// T3's `W`. An account seen under one workspace joins that workspace's key, and one seen
    /// under several stays apart unless its own record names the workspace.
    private static func mergeKeys(for enabled: [LimitsRecord]) -> (LimitsRecord) -> String? {
        var workspaces: [String: Set<String>] = [:]
        for record in enabled {
            if let key = baseKey(record), let workspace = record.workspaceID, !workspace.isEmpty {
                workspaces[key, default: []].insert(workspace)
            }
        }
        return { record in
            guard let key = baseKey(record) else { return nil }
            if let workspace = record.workspaceID, !workspace.isEmpty { return "\(key):\(workspace)" }
            guard let known = workspaces[key] else { return key }
            return known.count == 1 ? "\(key):\(known.first!)" : nil
        }
    }

    private struct Merged {
        var id: String
        var driver: String
        var displayName: String?
        var plan: String?
        var email: String?
        var limits: LimitsRecord.Limits
    }

    private static func mergedAccounts(_ visible: [LimitsRecord], mergeKey: (LimitsRecord) -> String?) -> [LimitsAccount] {
        var order: [String] = []
        var merged: [String: Merged] = [:]
        var planHolder: [String: (at: Date, plan: String)] = [:]
        var creditsHolder: [String: (at: Date, credits: ResetCredits)] = [:]
        for record in visible {
            guard let limits = record.limits, limits.notice == nil else { continue }
            let key = mergeKey(record) ?? "local:\(record.instanceID)"
            if let plan = record.plan, !plan.isEmpty, planHolder[key].map({ limits.checkedAt > $0.at }) ?? true {
                planHolder[key] = (limits.checkedAt, plan)
            }
            if let credits = limits.resetCredits, creditsHolder[key].map({ limits.checkedAt > $0.at }) ?? true {
                creditsHolder[key] = (limits.checkedAt, credits)
            }
            guard var current = merged[key] else {
                order.append(key)
                merged[key] = Merged(id: record.instanceID, driver: record.driver, displayName: record.trimmedName,
                                     plan: record.plan, email: record.email, limits: limits)
                continue
            }
            var newest = limits.checkedAt > current.limits.checkedAt ? limits : current.limits
            newest.resetCredits = creditsHolder[key]?.credits
            current.displayName = current.displayName ?? record.trimmedName
            if record.isSource, record.email != nil,
               current.displayName?.lowercased() == LimitsDrivers.label(record.driver).lowercased() {
                current.displayName = nil
            }
            current.plan = planHolder[key]?.plan ?? current.plan ?? record.plan
            current.limits = newest
            merged[key] = current
        }
        let accounts = order.compactMap { merged[$0] }
        let sortKey = { (item: Merged) in (item.displayName ?? item.email ?? item.id).lowercased() }
        let ranked = accounts.sorted { a, b in
            let order = sortKey(a).compare(sortKey(b), locale: Locale(identifier: "en_US"))
            return order != .orderedSame ? order == .orderedAscending
                : a.id.compare(b.id, locale: Locale(identifier: "en_US")) == .orderedAscending
        }
        func initials(_ item: Merged) -> String? {
            item.email.map { email in
                email.split(separator: "@", omittingEmptySubsequences: false).prefix(2)
                    .compactMap(\.first).map { String($0).uppercased() }.joined()
            }
        }
        return accounts.map { item in
            let sameDriver = ranked.filter { $0.driver == item.driver }
            let abbreviation = initials(item)
            let duplicates = sameDriver.filter { $0.displayName == nil && initials($0) == abbreviation }
            let suffix = duplicates.count > 1 ? " \((duplicates.firstIndex { $0.id == item.id } ?? 0) + 1)" : ""
            let name = item.displayName ?? (sameDriver.count > 1 ? abbreviation.map { $0 + suffix } : nil)
            return LimitsAccount(id: item.id, driver: item.driver, displayName: name, plan: item.plan,
                          checkedAt: item.limits.checkedAt, windows: item.limits.windows,
                          resetCredits: item.limits.resetCredits,
                          nameOrder: ranked.firstIndex { $0.id == item.id } ?? 0)
        }
    }

    /// T3's `Ct`: one card per external usage URL, listing the authenticated accounts behind it.
    private static func externalLinks(_ visible: [LimitsRecord]) -> [ExternalUsageLink] {
        var urls: [String] = []
        var links: [String: ExternalUsageLink] = [:]
        for record in visible {
            guard let limits = record.limits, let usage = limits.externalUsage, record.authenticated else { continue }
            let name = record.displayName ?? record.instanceID
            if links[usage.url] == nil { urls.append(usage.url) }
            let accounts = links[usage.url]?.accounts ?? []
            links[usage.url] = ExternalUsageLink(label: usage.label, url: usage.url, message: limits.unavailableMessage,
                                                 accounts: accounts.contains(name) ? accounts : accounts + [name])
        }
        return urls.compactMap { links[$0] }
    }
}
