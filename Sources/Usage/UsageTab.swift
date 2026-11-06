import Foundation
import Combine

/// The three metrics of T3 Code's Usage page, in its order.
public enum UsageTab: String, CaseIterable, Sendable {
    case cost, tokens, limits

    /// T3 Code opens on Limits.
    public static let `default`: UsageTab = .limits

    public var label: String {
        switch self {
        case .cost: return "Cost"
        case .tokens: return "Tokens"
        case .limits: return "Limits"
        }
    }

    /// T3 Code hides the period control on Limits, which has no period.
    public var showsPeriod: Bool { self != .limits }
}

/// Which tab of the Usage screen is open. It outlives the screen, so the next visit lands where this one ended.
@MainActor
public final class UsageScreenModel: ObservableObject {
    static let tabKey = "usageTab"

    @Published public var tab: UsageTab {
        didSet {
            guard tab != oldValue else { return }
            defaults.set(tab.rawValue, forKey: Self.tabKey)
        }
    }

    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        tab = defaults.string(forKey: Self.tabKey).flatMap(UsageTab.init(rawValue:)) ?? .default
    }
}

/// What a tab shows in place of its content when it has none to show.
public enum UsageContentState: Equatable, Sendable {
    case loading
    /// T3 Code has not written the file this tab reads.
    case missing
    case unsupported(Int?)
    /// The read failed and there is no earlier good copy.
    case unreadable
    /// The file is fine, and it has nothing for this tab.
    case empty
    case content

    /// Cost and Tokens read the scan cache, which holds the same report for both.
    public static func scan(report: UsageReport?, notice: UsageNotice?) -> UsageContentState {
        if let report { return report.isEmpty ? .empty : .content }
        switch notice {
        case .missing: return .missing
        case .unsupported(let version): return .unsupported(version)
        case .retrying: return .unreadable
        case nil: return .loading
        }
    }

    /// Limits reads the provider status files, so it works without a scan cache.
    public static func limits(snapshot: LimitsSnapshot?, error: LimitsReadError?) -> UsageContentState {
        guard let snapshot else { return error == nil ? .loading : .unreadable }
        switch snapshot.state {
        case .noCacheDirectory: return .missing
        case .noLimitsReported: return .empty
        case .ok: return .content
        }
    }
}
