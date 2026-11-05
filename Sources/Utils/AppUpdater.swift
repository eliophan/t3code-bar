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

public enum AppUpdatePolicy {
    public static let repository = "eliophan/t3code-bar"
    public static let releaseBuildKey = "T3CodeBarReleaseBuild"

    /// The tag to install, or nil when the running version is current or newer.
    public static func tagToInstall(latestTag: String, runningVersion: String) -> String? {
        guard let latest = AppVersion(latestTag), let running = AppVersion(runningVersion), latest > running else { return nil }
        return latestTag
    }

    static func latestTag(fromReleaseJSON data: Data) -> String? {
        struct Release: Decodable { let tag_name: String }
        return (try? JSONDecoder().decode(Release.self, from: data))?.tag_name
    }

    /// Source builds and `swift run` must never replace themselves with a
    /// release, and a copy outside ~/Applications is not the one the installer replaces.
    static func canSelfUpdate(bundle: Bundle, installedAt: URL) -> Bool {
        bundle.object(forInfoDictionaryKey: releaseBuildKey) as? Bool == true
            && bundle.bundleURL.standardizedFileURL == installedAt.standardizedFileURL
    }
}

@MainActor
public final class AppUpdater {
    private static let interval: TimeInterval = 6 * 60 * 60
    private var timer: Timer?
    private var installing = false

    public init() {}

    public func start() {
        let installedAt = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Applications/T3 Code Bar.app")
        guard AppUpdatePolicy.canSelfUpdate(bundle: .main, installedAt: installedAt),
              let running = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String else { return }
        let check = { [weak self] in
            guard UpdateSettings.isEnabled else { return }
            Task { await self?.checkAndInstall(running: running) }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 10, execute: check)
        timer = Timer.scheduledTimer(withTimeInterval: Self.interval, repeats: true) { _ in check() }
    }

    private func checkAndInstall(running: String) async {
        guard !installing,
              let url = URL(string: "https://api.github.com/repos/\(AppUpdatePolicy.repository)/releases/latest"),
              let (data, response) = try? await URLSession.shared.data(from: url),
              (response as? HTTPURLResponse)?.statusCode == 200,
              let latest = AppUpdatePolicy.latestTag(fromReleaseJSON: data),
              let tag = AppUpdatePolicy.tagToInstall(latestTag: latest, runningVersion: running) else { return }
        install(tag: tag)
    }

    /// The installer quits this app, swaps the bundle, and reopens it, so it
    /// runs as a detached child with no pipes back to us.
    private func install(tag: String) {
        installing = true
        let script = "https://raw.githubusercontent.com/\(AppUpdatePolicy.repository)/\(tag)/scripts/install-release.sh"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/bash")
        process.arguments = ["-c", "curl -fsSL --retry 3 \"$0\" | bash -s -- --version \"$1\"", script, tag]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        process.terminationHandler = { [weak self] _ in
            Task { @MainActor in self?.installing = false }
        }
        do { try process.run() } catch { installing = false }
    }
}
