import Foundation

public struct AppVersion: Comparable, Equatable, Sendable {
    public let parts: [Int]

    public init?(_ text: String) {
        let trimmed = text.hasPrefix("v") ? String(text.dropFirst()) : text
        let numbers = trimmed.split(separator: ".", omittingEmptySubsequences: false).map { Int($0) }
        guard numbers.count == 3, !numbers.contains(where: { $0 == nil }) else { return nil }
        parts = numbers.compactMap { $0 }
    }

    public static func < (lhs: AppVersion, rhs: AppVersion) -> Bool {
        lhs.parts.lexicographicallyPrecedes(rhs.parts)
    }
}

public enum UpdateSettings {
    public static let key = "autoUpdateEnabled"
    public static var isEnabled: Bool {
        get { UserDefaults.standard.object(forKey: key) as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }
}

public enum UpdatePlatform: Sendable {
    case macOS

    public static var current: UpdatePlatform { .macOS }

    public func assetName(version: String) -> String {
        switch self {
        case .macOS: "T3-Code-Bar-\(version)-universal.zip"
        }
    }
}

public struct ReleaseInfo: Equatable, Sendable {
    public let tag: String
    public let assetNames: [String]
}

public enum AppUpdatePolicy {
    public static let repository = "eliophan/t3code-bar"
    public static let releaseBuildKey = "T3CodeBarReleaseBuild"

    /// The tag to install, or nil when the running version is current or newer.
    public static func tagToInstall(latestTag: String, runningVersion: String) -> String? {
        guard let latest = AppVersion(latestTag), let running = AppVersion(runningVersion), latest > running else { return nil }
        return latestTag
    }

    /// A newer tag can be published before its assets finish uploading, so a
    /// release is offered only once it carries the running platform's asset.
    public static func tagToInstall(release: ReleaseInfo, runningVersion: String, platform: UpdatePlatform) -> String? {
        guard let tag = tagToInstall(latestTag: release.tag, runningVersion: runningVersion) else { return nil }
        let version = tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
        return release.assetNames.contains(platform.assetName(version: version)) ? tag : nil
    }

    static func release(fromReleaseJSON data: Data) -> ReleaseInfo? {
        struct Release: Decodable {
            struct Asset: Decodable { let name: String? }
            let tag_name: String
            let assets: [Asset]?
        }
        guard let release = try? JSONDecoder().decode(Release.self, from: data) else { return nil }
        return ReleaseInfo(tag: release.tag_name, assetNames: (release.assets ?? []).compactMap(\.name))
    }

    /// Source builds and `swift run` must never replace themselves with a
    /// release, and a copy outside ~/Applications is not the one the installer replaces.
    static func unsupportedReason(isReleaseBuild: Bool, bundleURL: URL, installedAt: URL, version: String?) -> UnsupportedReason? {
        if !isReleaseBuild { return .notReleaseBuild }
        if bundleURL.standardizedFileURL != installedAt.standardizedFileURL { return .notInstalledInApplications }
        if version == nil { return .missingVersion }
        return nil
    }

    static func unsupportedReason(bundle: Bundle, installedAt: URL) -> UnsupportedReason? {
        unsupportedReason(
            isReleaseBuild: bundle.object(forInfoDictionaryKey: releaseBuildKey) as? Bool == true,
            bundleURL: bundle.bundleURL,
            installedAt: installedAt,
            version: bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        )
    }
}

@MainActor
public final class AppUpdater: ObservableObject {
    private static let interval: TimeInterval = 6 * 60 * 60
    private var timer: Timer?
    @Published public private(set) var status = UpdateStatus()
    public private(set) var runningVersion: String?

    private let fetchRelease: () async -> Result<ReleaseInfo, UpdateFailure>
    /// Calls back with the installer's exit status once it ends.
    private let runInstaller: (String, @escaping @Sendable (Int32) -> Void) -> Void
    private let isAutoUpdateEnabled: () -> Bool
    /// Runs the work after the delay and returns a function that cancels it.
    private let scheduleRetry: (TimeInterval, @escaping @Sendable () -> Void) -> () -> Void
    private let log: UpdateLog
    private var consecutiveFailures = 0
    private var cancelPendingRetry: (() -> Void)?

    public var state: UpdateState { status.state }

    public init(
        runningVersion: String? = nil,
        fetchRelease: (() async -> Result<ReleaseInfo, UpdateFailure>)? = nil,
        runInstaller: ((String, @escaping @Sendable (Int32) -> Void) -> Void)? = nil,
        isAutoUpdateEnabled: @escaping () -> Bool = { UpdateSettings.isEnabled },
        scheduleRetry: @escaping (TimeInterval, @escaping @Sendable () -> Void) -> () -> Void = AppUpdater.liveScheduleRetry(delay:work:),
        log: UpdateLog = .standard
    ) {
        self.runningVersion = runningVersion
        if let fetchRelease {
            self.fetchRelease = fetchRelease
        } else {
            let fetcher = ReleaseFetcher()
            self.fetchRelease = { await fetcher.fetch() }
        }
        self.runInstaller = runInstaller ?? { tag, completion in
            AppUpdater.liveRunInstaller(tag: tag, log: log, completion: completion)
        }
        self.isAutoUpdateEnabled = isAutoUpdateEnabled
        self.scheduleRetry = scheduleRetry
        self.log = log
    }

    public func start() {
        runningVersion = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        let installedAt = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Applications/T3 Code Bar.app")
        if let reason = AppUpdatePolicy.unsupportedReason(bundle: .main, installedAt: installedAt) {
            send(.unsupported(reason))
            return
        }
        let check = automaticCheck
        DispatchQueue.main.asyncAfter(deadline: .now() + 10, execute: check)
        timer = Timer.scheduledTimer(withTimeInterval: Self.interval, repeats: true) { _ in check() }
    }

    private var automaticCheck: @Sendable () -> Void {
        { [weak self] in Task { @MainActor in await self?.checkAutomatically() } }
    }

    /// Stops at `.available`; installing is a separate, explicit step.
    /// A manual check never schedules a retry, but a success clears a pending one.
    public func checkNow() async {
        guard let result = await check(installIfAvailable: false) else { return }
        if case .failed = result { return }
        resetRetries()
    }

    public func installAvailable() {
        guard case .available(let tag) = state else { return }
        install(tag: tag)
    }

    func checkAutomatically() async {
        guard isAutoUpdateEnabled() else { return }
        guard let result = await check(installIfAvailable: true) else { return }
        guard case .failed(let failure) = result else {
            resetRetries()
            return
        }
        resetRetries(keepingCount: true)
        consecutiveFailures += 1
        guard failure.isRetryable,
              let delay = UpdateRetryPolicy.delay(afterConsecutiveFailures: consecutiveFailures) else { return }
        cancelPendingRetry = scheduleRetry(delay, automaticCheck)
    }

    /// Cancelling the timer without the count lets the next failure back off further.
    private func resetRetries(keepingCount: Bool = false) {
        cancelPendingRetry?()
        cancelPendingRetry = nil
        if !keepingCount { consecutiveFailures = 0 }
    }

    func send(_ event: UpdateEvent) {
        status = UpdateStatus.reduce(status, event)
    }

    /// Nil when no fetch happened because the state does not allow one.
    private func check(installIfAvailable: Bool) async -> CheckResult? {
        guard let running = runningVersion else { return nil }
        // The reducer accepts .checkStarted only from these states. Checking here
        // too keeps a second caller from fetching while a check is in flight.
        switch state {
        case .idle, .available, .failed: break
        case .checking, .installing, .unsupported: return nil
        }
        send(.checkStarted)
        let result = await fetchRelease()
        let outcome: CheckResult
        switch result {
        case .failure(let failure):
            outcome = .failed(failure)
        case .success(let release):
            if let tag = AppUpdatePolicy.tagToInstall(release: release, runningVersion: running, platform: .current) {
                outcome = .available(tag)
            } else {
                outcome = .upToDate
            }
        }
        send(.checkFinished(outcome, at: Date()))
        if case .failed(let failure) = outcome { log.append("update check failed: \(failure.logLabel)") }
        if installIfAvailable, case .available(let tag) = state { install(tag: tag) }
        return outcome
    }

    private func install(tag: String) {
        send(.installStarted(tag: tag))
        guard case .installing = state else { return }
        let log = log
        log.append("install started: \(tag)")
        runInstaller(tag) { [weak self] code in
            log.append("install finished: \(tag), exit \(code)")
            Task { @MainActor in self?.send(.installFinished(status: code)) }
        }
    }

    public nonisolated static func liveScheduleRetry(delay: TimeInterval, work: @escaping @Sendable () -> Void) -> () -> Void {
        let task = Task {
            try? await Task.sleep(for: .seconds(delay))
            if !Task.isCancelled { work() }
        }
        return { task.cancel() }
    }

    /// The installer quits this app, swaps the bundle, and reopens it, so it
    /// runs as a detached child with no pipes back to us.
    public nonisolated static func liveRunInstaller(tag: String, log: UpdateLog, completion: @escaping @Sendable (Int32) -> Void) {
        let script = "https://raw.githubusercontent.com/\(AppUpdatePolicy.repository)/\(tag)/scripts/install-release.sh"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/bash")
        process.arguments = ["-c", "curl -fsSL --retry 3 \"$0\" | bash -s -- --version \"$1\"", script, tag]
        let output = log.openForInstaller(tag: tag)
        process.standardOutput = output ?? FileHandle.nullDevice
        process.standardError = output ?? FileHandle.nullDevice
        process.terminationHandler = {
            try? output?.close()
            completion($0.terminationStatus)
        }
        do { try process.run() } catch {
            try? output?.close()
            completion(-1)
        }
    }
}
