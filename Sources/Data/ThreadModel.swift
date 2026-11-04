import Foundation
import SwiftUI

public enum ThreadStatus: Equatable, Hashable, Sendable {
    case connecting
    case working(startedAt: Date?)
    case goal(startedAt: Date?)
    case approval
    case input
    /// A guest on a shared link has a message waiting for the owner.
    case request
    case woke
    case planReady
    case failed
    /// The thread is idle or is blocked waiting on another thread, so nothing is running here.
    case waiting
    /// The latest run failed because the provider's usage limit was reached.
    case limited
    case done
    case settled

    public var label: String {
        switch self {
        case .connecting: return "Connecting"
        case .working: return "Working"
        case .goal: return "Goal"
        case .approval: return "Approval"
        case .input: return "Input"
        case .request: return "Request"
        case .woke: return "Woke"
        case .planReady: return "Plan Ready"
        case .failed: return "Failed"
        case .limited: return "Limited"
        case .waiting: return "Waiting"
        case .done: return "Done"
        case .settled: return "Settled"
        }
    }

    public var color: Color {
        switch self {
        case .connecting, .working, .goal: return T3Theme.statusWorking
        case .approval, .woke, .limited: return T3Theme.statusApproval
        case .input: return T3Theme.statusInput
        case .request: return T3Theme.statusRequest
        case .planReady: return Color(hex: "#c084fc")
        case .failed: return T3Theme.statusFailed
        case .done: return T3Theme.statusDone
        case .settled, .waiting: return T3Theme.statusSettled
        }
    }

    public var showsLabelInMenuBar: Bool {
        switch self {
        case .approval, .input, .request, .woke, .planReady, .failed, .limited: return true
        default: return false
        }
    }

    public var isRunning: Bool {
        switch self {
        case .connecting, .working, .goal: return true
        default: return false
        }
    }

    public var shouldPulse: Bool {
        switch self {
        case .connecting, .working, .goal:
            return true
        default:
            return false
        }
    }

    public var isAttention: Bool {
        switch self {
        case .connecting, .working, .goal, .approval, .input, .request, .planReady, .failed, .limited, .woke:
            return true
        default:
            return false
        }
    }

    public var priority: Int {
        switch self {
        case .approval: return 8
        case .input, .request: return 7
        case .goal: return 6
        case .connecting, .working: return 5
        case .failed, .limited: return 4
        case .woke: return 3
        case .planReady: return 2
        case .done: return 1
        case .settled, .waiting: return 0
        }
    }
}

public struct ThreadItem: Identifiable, Equatable, Sendable {
    public let id: String
    public let projectId: String
    public let projectTitle: String
    public let title: String
    public let createdAt: Date
    public let updatedAt: Date
    public let latestUserMessageAt: Date?
    public let activeRunStatus: String?
    public let activeRunStartedAt: Date?
    public let latestRunStatus: String?
    public let latestRunStartedAt: Date?
    public let latestRunCompletedAt: Date?
    public let pendingUserInputCount: Int
    public let pendingApprovalCount: Int
    public let branch: String?
    public let worktreePath: String?
    public let modelName: String?
    public let providers: [String]
    public let interactionMode: String?
    public let hasActionableProposedPlan: Bool
    public let hasGoal: Bool
    public let isPinned: Bool
    public let isSettled: Bool
    public let isSnoozed: Bool
    public let isWoke: Bool
    public let pinOrderKey: String?
    public let activeOrderKey: String?
    public let prNumber: Int?
    public let projectIconJson: String?
    public let projectFaviconPath: String?
    public let workspaceRoot: String?
    public let unsettledAt: Date?
    public let lastVisitedAt: Date?
    /// `failure.class` of the latest run's root error, such as `usage_limit`.
    public var latestFailureClass: String? = nil
    /// When the provider's usage limit resets, if the failure says.
    public var usageLimitResetAt: Date? = nil
    /// Guest messages the owner has not sent or dismissed yet.
    public var pendingGuestMessageCount: Int = 0
    /// The active run is only waiting on other threads (`t3_thread_wait`).
    public var isBlockedOnThreadWait: Bool = false
    /// False for a thread that has never run anything.
    public var hasRuns: Bool = true

    public var status: ThreadStatus {
        if pendingApprovalCount > 0 {
            return .approval
        }
        if pendingUserInputCount > 0 {
            return .input
        }
        if pendingGuestMessageCount > 0 {
            return .request
        }
        if activeRunStatus == "starting" || activeRunStatus == "preparing" {
            return .connecting
        }
        if activeRunStatus == "running" && isBlockedOnThreadWait {
            return .waiting
        }
        if activeRunStatus == "running" || activeRunStatus == "waiting" {
            if hasGoal {
                return .goal(startedAt: activeRunStartedAt ?? latestRunStartedAt ?? updatedAt)
            }
            return .working(startedAt: activeRunStartedAt ?? latestRunStartedAt ?? updatedAt)
        }
        if latestRunStatus == "failed" || latestRunStatus == "error" {
            return latestFailureClass == "usage_limit" ? .limited : .failed
        }
        if isWoke {
            return .woke
        }
        if interactionMode == "plan" && hasActionableProposedPlan {
            return .planReady
        }
        if !hasRuns {
            return .waiting
        }
        // Like the sidebar: a completion is unseen only after a known last visit.
        if latestRunStatus == "completed", let completedAt = latestRunCompletedAt,
           let lastVisitedAt, completedAt > lastVisitedAt {
            return .done
        }
        return .settled
    }

    public var isActiveAttention: Bool {
        return !isSettled && !isSnoozed
    }
}
