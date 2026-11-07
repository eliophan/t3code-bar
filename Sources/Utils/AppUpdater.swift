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

    public var state: UpdateState { status.state }

    public init(
        runningVersion: String? = nil,
        fetchRelease: @escaping () async -> Result<ReleaseInfo, UpdateFailure> = AppUpdater.liveFetchRelease,
        runInstaller: @escaping (String, @escaping @Sendable (Int32) -> Void) -> Void = AppUpdater.liveRunInstaller,
        isAutoUpdateEnabled: @escaping () -> Bool = { UpdateSettings.isEnabled }
    ) {
        self.runningVersion = runningVersion
        self.fetchRelease = fetchRelease
        self.runInstaller = runInstaller
        self.isAutoUpdateEnabled = isAutoUpdateEnabled
    }

    public func start() {
        runningVersion = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        let installedAt = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Applications/T3 Code Bar.app")
        if let reason = AppUpdatePolicy.unsupportedReason(bundle: .main, installedAt: installedAt) {
            send(.unsupported(reason))
            return
        }
        let check: @Sendable () -> Void = { [weak self] in
            Task { @MainActor in await self?.checkAutomatically() }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 10, execute: check)
        timer = Timer.scheduledTimer(withTimeInterval: Self.interval, repeats: true) { _ in check() }
    }

    /// Stops at `.available`; installing is a separate, explicit step.
    public func checkNow() async {
        await check(installIfAvailable: false)
    }

    public func installAvailable() {
        guard case .available(let tag) = state else { return }
        install(tag: tag)
    }

    func checkAutomatically() async {
        guard isAutoUpdateEnabled() else { return }
        await check(installIfAvailable: true)
    }

    func send(_ event: UpdateEvent) {
        status = UpdateStatus.reduce(status, event)
    }

    private func check(installIfAvailable: Bool) async {
        guard let running = runningVersion else { return }
        // The reducer accepts .checkStarted only from these states. Checking here
        // too keeps a second caller from fetching while a check is in flight.
        switch state {
        case .idle, .available, .failed: break
        case .checking, .installing, .unsupported: return
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
        if installIfAvailable, case .available(let tag) = state { install(tag: tag) }
    }

    public nonisolated static func liveFetchRelease() async -> Result<ReleaseInfo, UpdateFailure> {
        guard let url = URL(string: "https://api.github.com/repos/\(AppUpdatePolicy.repository)/releases/latest"),
              let (data, response) = try? await URLSession.shared.data(from: url) else { return .failure(.network) }
        guard (response as? HTTPURLResponse)?.statusCode == 200,
              let release = AppUpdatePolicy.release(fromReleaseJSON: data) else { return .failure(.badResponse) }
        return .success(release)
    }

    private func install(tag: String) {
        send(.installStarted(tag: tag))
        guard case .installing = state else { return }
        runInstaller(tag) { [weak self] code in
            Task { @MainActor in self?.send(.installFinished(status: code)) }
        }
    }

    /// The installer quits this app, swaps the bundle, and reopens it, so it
    /// runs as a detached child with no pipes back to us.
    public nonisolated static func liveRunInstaller(tag: String, completion: @escaping @Sendable (Int32) -> Void) {
        let script = "https://raw.githubusercontent.com/\(AppUpdatePolicy.repository)/\(tag)/scripts/install-release.sh"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/bash")
        process.arguments = ["-c", "curl -fsSL --retry 3 \"$0\" | bash -s -- --version \"$1\"", script, tag]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        process.terminationHandler = { completion($0.terminationStatus) }
        do { try process.run() } catch { completion(-1) }
    }
}
