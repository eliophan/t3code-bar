import Foundation

public struct UsageCacheFile: Equatable, Sendable {
    public let url: URL
    public let size: Int
    public let modified: Date
}

public enum UsageCacheLocation: Equatable, Sendable {
    /// T3 Code has not scanned yet, or its state directory has no usage cache.
    case missing
    /// The newest cache uses a format this build does not know. The associated value is its version, if readable.
    case unsupported(Int?)
    case available(UsageCacheFile)
}

/// Finds the cache T3 Code is writing now.
///
/// T3 Code keeps older `-vN` files next to the current one, so the choice is by generation and the
/// file's own `version` field, never by modification time. The one use of the time is to notice a newer
/// format that this build cannot read, which would otherwise leave a stale older file looking current.
public struct UsageCacheLocator: Sendable {
    public let directory: URL

    public init(directory: URL) {
        self.directory = directory
    }

    public func locate() -> UsageCacheLocation {
        let candidates = self.candidates()
        guard !candidates.isEmpty else { return .missing }

        var skipped: [(file: UsageCacheFile, version: Int?)] = []
        for candidate in candidates {
            let version = header(of: candidate)
            guard let version else { continue }
            guard UsageCacheDecoder.supportedVersions.contains(version) else {
                skipped.append((candidate, version))
                continue
            }
            if let newer = skipped.first(where: { $0.file.modified > candidate.modified }) {
                return .unsupported(newer.version)
            }
            return .available(candidate)
        }
        return .unsupported(skipped.first?.version)
    }

    /// Highest generation first. The unversioned legacy name comes last.
    private func candidates() -> [UsageCacheFile] {
        let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey]
        guard let urls = try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: keys) else { return [] }
        return urls.compactMap { url -> (generation: Int, file: UsageCacheFile)? in
            guard let generation = Self.generation(of: url.lastPathComponent),
                  let values = try? url.resourceValues(forKeys: Set(keys)),
                  let size = values.fileSize, let modified = values.contentModificationDate else { return nil }
            return (generation, UsageCacheFile(url: url, size: size, modified: modified))
        }
        .sorted { $0.generation > $1.generation }
        .map(\.file)
    }

    /// `usage-scan-cache-v11.json` is 11 and `usage-scan-cache.json` is 0. Anything else is not a cache.
    static func generation(of name: String) -> Int? {
        let prefix = "usage-scan-cache", suffix = ".json"
        guard name.hasPrefix(prefix), name.hasSuffix(suffix), name.count >= prefix.count + suffix.count else { return nil }
        let middle = name.dropFirst(prefix.count).dropLast(suffix.count)
        if middle.isEmpty { return 0 }
        guard middle.hasPrefix("-v"), let number = Int(middle.dropFirst(2)), number > 0 else { return nil }
        return number
    }

    /// The version sits near the top of the file, so the first 64 KB almost always settles it.
    private func header(of file: UsageCacheFile) -> Int? {
        guard let handle = try? FileHandle(forReadingFrom: file.url) else { return nil }
        defer { try? handle.close() }
        guard let prefix = try? handle.read(upToCount: 65_536) else { return nil }
        if let version = prefix.withUnsafeBytes({ UsageCacheDecoder.version(in: $0.bindMemory(to: UInt8.self)) }) {
            return version
        }
        guard file.size > prefix.count, let all = try? Data(contentsOf: file.url) else { return nil }
        return all.withUnsafeBytes { UsageCacheDecoder.version(in: $0.bindMemory(to: UInt8.self)) }
    }
}

public struct UsageCacheReader: Sendable {
    public init() {}

    public func load(_ file: UsageCacheFile) throws -> UsageDataset {
        guard let data = try? Data(contentsOf: file.url) else { throw UsageCacheError.unreadable }
        var dataset = try data.withUnsafeBytes { try UsageCacheDecoder.decode($0.bindMemory(to: UInt8.self)) }
        dataset.size = file.size
        dataset.modified = file.modified
        return dataset
    }
}
