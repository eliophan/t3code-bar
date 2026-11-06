import Foundation

/// Where T3 Code's usage data lives, and how to read it. Every access is read-only.
public struct UsageSource: Sendable {
    /// Changes whenever the price list or the settings file does, so prices are re-read only then.
    public struct PriceStamp: Equatable, Sendable {
        let rates: FileStamp?
        let settings: FileStamp?
    }

    struct FileStamp: Equatable, Sendable {
        let size: Int
        let modified: Date
    }

    public let directory: URL
    private let database: URL
    private var pathsOverride: LocalUsagePaths?

    var localPaths: LocalUsagePaths {
        pathsOverride ?? .discover(state: directory, home: FileManager.default.homeDirectoryForCurrentUser, environment: ProcessInfo.processInfo.environment)
    }

    public init(config: EnvironmentConfig = .shared) {
        directory = config.baseDir
        database = config.stateDbUrl
    }

    init(config: EnvironmentConfig, localPaths: LocalUsagePaths) {
        directory = config.baseDir
        database = config.stateDbUrl
        pathsOverride = localPaths
    }

    public func locate() -> UsageCacheLocation {
        UsageCacheLocator(directory: directory).locate()
    }

    /// The decoded cache with Codex service tiers looked up in T3 Code's database.
    public func dataset(_ file: UsageCacheFile) throws -> UsageDataset {
        CodexTierResolver.resolve(try UsageCacheReader().load(file), database: database)
    }

    public func prices() -> PriceBook {
        UsagePriceSources.load(from: directory)
    }

    public func priceStamp() -> PriceStamp {
        PriceStamp(rates: stamp(UsagePriceSources.ratesFile), settings: stamp(UsagePriceSources.settingsFile))
    }

    private func stamp(_ name: String) -> FileStamp? {
        let url = directory.appendingPathComponent(name)
        guard let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey]),
              let size = values.fileSize, let modified = values.contentModificationDate else { return nil }
        return FileStamp(size: size, modified: modified)
    }
}
