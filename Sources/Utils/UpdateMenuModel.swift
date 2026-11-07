import Foundation

public struct UpdateMenuModel: Equatable, Sendable {
    public enum Action: Equatable, Sendable {
        case check
        case checking
        case install(String)
        case installing
        case unavailable

        public var title: String {
            switch self {
            case .check, .unavailable: "Check for Updates…"
            case .checking: "Checking…"
            case .install(let tag): "Install \(tag) and Restart"
            case .installing: "Installing…"
            }
        }

        public var isEnabled: Bool {
            switch self {
            case .check, .install: true
            case .checking, .installing, .unavailable: false
            }
        }
    }

    public let versionLine: String
    public let statusLine: String
    public let action: Action

    public init(status: UpdateStatus, runningVersion: String?, now: Date, calendar: Calendar = .current) {
        versionLine = runningVersion.map { "T3 Code Bar \($0)" } ?? "T3 Code Bar"
        switch status.state {
        case .idle:
            statusLine = status.lastChecked.map { "Up to date, checked \(Self.time($0, now: now, calendar: calendar))" } ?? "Not checked yet"
            action = .check
        case .checking:
            statusLine = "Checking for updates…"
            action = .checking
        case .available(let tag):
            let version = tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
            statusLine = "Version \(version) is available"
            action = .install(tag)
        case .installing(let tag):
            statusLine = "Installing \(tag)…"
            action = .installing
        case .failed(let failure):
            switch failure {
            case .network: statusLine = "Could not reach GitHub"
            case .badResponse: statusLine = "Unexpected response from GitHub"
            case .rateLimited: statusLine = "GitHub rate limit reached, try again later"
            case .installerExit(let code): statusLine = "Installer failed (exit \(code))"
            }
            action = .check
        case .unsupported(let reason):
            switch reason {
            case .notReleaseBuild: statusLine = "Updates are off for builds run from source"
            case .notInstalledInApplications: statusLine = "Move the app to ~/Applications to enable updates"
            case .missingVersion: statusLine = "Version unknown; updates are off"
            }
            action = .unavailable
        }
    }

    private static func time(_ date: Date, now: Date, calendar: Calendar) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = calendar
        formatter.timeZone = calendar.timeZone
        formatter.dateFormat = calendar.isDate(date, inSameDayAs: now) ? "HH:mm" : "MMM d, HH:mm"
        return formatter.string(from: date)
    }
}
