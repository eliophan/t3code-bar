import Foundation

public enum UnsupportedReason: Equatable, Sendable {
    case notReleaseBuild
    case notInstalledInApplications
    case missingVersion
}

public enum UpdateFailure: Error, Equatable, Sendable {
    case network
    case badResponse
    case installerExit(Int32)
}

public enum UpdateState: Equatable, Sendable {
    case idle
    case checking
    case available(String)
    case installing(String)
    case failed(UpdateFailure)
    case unsupported(UnsupportedReason)
}

public enum CheckResult: Equatable, Sendable {
    case upToDate
    case available(String)
    case failed(UpdateFailure)
}

/// Time arrives inside events so `reduce` stays pure.
public enum UpdateEvent: Equatable, Sendable {
    case checkStarted
    case checkFinished(CheckResult, at: Date)
    case installStarted(tag: String)
    case installFinished(status: Int32)
    case unsupported(UnsupportedReason)
}

public struct UpdateStatus: Equatable, Sendable {
    public var state: UpdateState
    public var lastChecked: Date?

    public init(state: UpdateState = .idle, lastChecked: Date? = nil) {
        self.state = state
        self.lastChecked = lastChecked
    }

    /// Events that do not fit the current state are dropped, so a late
    /// check result can never overwrite an install in flight.
    public static func reduce(_ status: UpdateStatus, _ event: UpdateEvent) -> UpdateStatus {
        var next = status
        switch (status.state, event) {
        case (_, .unsupported(let reason)):
            next.state = .unsupported(reason)
        case (.unsupported, _):
            break
        case (.idle, .checkStarted), (.available, .checkStarted), (.failed, .checkStarted):
            next.state = .checking
        case (.checking, .checkFinished(let result, let date)):
            next.lastChecked = date
            switch result {
            case .upToDate: next.state = .idle
            case .available(let tag): next.state = .available(tag)
            case .failed(let failure): next.state = .failed(failure)
            }
        case (.idle, .installStarted(let tag)), (.available, .installStarted(let tag)), (.failed, .installStarted(let tag)):
            next.state = .installing(tag)
        case (.installing, .installFinished(let code)):
            next.state = code == 0 ? .idle : .failed(.installerExit(code))
        default:
            break
        }
        return next
    }
}
