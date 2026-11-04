import Foundation
import UserNotifications

public struct AttentionAlert: Equatable, Sendable {
    public let threadId: String
    public let title: String
    public let status: ThreadStatus
}

public struct AttentionTracker: Sendable {
    private var known: [String: ThreadStatus]?

    public init() {}

    public static func needsAlert(_ status: ThreadStatus) -> Bool {
        switch status {
        case .approval, .input, .failed: return true
        default: return false
        }
    }

    /// The first snapshot is a baseline and never alerts.
    public mutating func update(with threads: [ThreadItem]) -> [AttentionAlert] {
        let current = Dictionary(uniqueKeysWithValues: threads.map { ($0.id, $0.status) })
        defer { known = current }
        guard let known else { return [] }
        return threads.compactMap { thread in
            let status = thread.status
            guard Self.needsAlert(status), known[thread.id] != status else { return nil }
            return AttentionAlert(threadId: thread.id, title: thread.title, status: status)
        }
    }
}

@MainActor
public final class AttentionNotifier: NSObject, UNUserNotificationCenterDelegate {
    private var tracker = AttentionTracker()
    private static let mutedKey = "notificationsMuted"

    public private(set) var isMuted = UserDefaults.standard.bool(forKey: AttentionNotifier.mutedKey)

    public func toggleMute() {
        isMuted.toggle()
        UserDefaults.standard.set(isMuted, forKey: Self.mutedKey)
    }

    /// UserNotifications aborts the process unless it runs from an `.app`
    /// bundle, which `swift run` does not provide. Skip notifications then.
    private static let canNotify = Bundle.main.bundleURL.pathExtension == "app"

    public override init() {
        super.init()
        guard Self.canNotify else { return }
        UNUserNotificationCenter.current().delegate = self
    }

    public func process(_ threads: [ThreadItem]) {
        let alerts = tracker.update(with: threads)
        guard !alerts.isEmpty, !isMuted, Self.canNotify else { return }
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound]) { granted, _ in
            guard granted else { return }
            for alert in alerts { center.add(Self.request(for: alert)) }
        }
    }

    nonisolated private static func request(for alert: AttentionAlert) -> UNNotificationRequest {
        let content = UNMutableNotificationContent()
        content.title = "\(alert.status.label) needed"
        content.body = alert.title
        content.sound = .default
        content.userInfo = ["threadId": alert.threadId]
        return UNNotificationRequest(identifier: "\(alert.threadId)-\(alert.status.label)", content: content, trigger: nil)
    }

    nonisolated public func userNotificationCenter(_ center: UNUserNotificationCenter,
                                                   willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }

    nonisolated public func userNotificationCenter(_ center: UNUserNotificationCenter,
                                                   didReceive response: UNNotificationResponse) async {
        guard let id = response.notification.request.content.userInfo["threadId"] as? String else { return }
        await MainActor.run { AppLauncher.openThread(threadId: id) }
    }
}
