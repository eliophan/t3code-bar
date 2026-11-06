import Foundation
import CryptoKit

public struct LocalUsageStatus: Equatable, Sendable {
    public enum State: Equatable, Sendable { case ready, missing, partial, unreadable }
    public let kind: UsageProviderKind
    public let state: State
    public let checkedAt: Date
    public let dataUpdatedAt: Date?
    public let failedFiles: Int
}

struct LocalUsagePaths: Sendable {
    var openCode: [URL] = []
    var antigravity: [URL] = []
    var cursor: URL?
    static let empty = LocalUsagePaths()

    static func discover(state: URL, home: URL, environment: [String: String]) -> Self {
        func roots(_ key: String, defaults: [URL]) -> [URL] {
            guard let value = environment[key], !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return defaults }
            return value.split(separator: ",").map { URL(fileURLWithPath: (String($0).trimmingCharacters(in: .whitespacesAndNewlines) as NSString).expandingTildeInPath) }
        }
        let xdg = environment["XDG_DATA_HOME"].flatMap { $0.hasPrefix("/") ? URL(fileURLWithPath: $0) : nil }
        let openCode = roots("OPENCODE_DATA_DIR", defaults: [(xdg ?? home.appendingPathComponent(".local/share")).appendingPathComponent("opencode")])
        var antigravity = roots("ANTIGRAVITY_DATA_DIR", defaults: ["antigravity", "antigravity-cli", "antigravity-ide", "antigravity-backup"].map { home.appendingPathComponent(".gemini/\($0)") } + [home.appendingPathComponent(".config/antigravity")])
        for id in antigravityInstances(state.appendingPathComponent("settings.json")) {
            let hash = SHA256.hash(data: Data(id.utf8)).map { String(format: "%02x", $0) }.joined()
            antigravity.append(state.appendingPathComponent("providers/antigravity/\(hash)/antigravity-acp"))
        }
        return .init(openCode: openCode, antigravity: antigravity, cursor: state.appendingPathComponent("usage-cursor-account-cache-v1.json"))
    }

    private static func antigravityInstances(_ url: URL) -> [String] {
        guard let data = try? Data(contentsOf: url) else { return [] }
        return (try? data.withUnsafeBytes { raw -> [String] in
            var scanner = JSONScanner(raw.bindMemory(to: UInt8.self))
            var ids: [String] = []
            try scanner.openObject()
            if scanner.closesObject() { return ids }
            repeat {
                guard try scanner.objectKey() == "providerInstances", scanner.isObject() else { try scanner.skipValue(); continue }
                try scanner.openObject()
                if scanner.closesObject() { continue }
                repeat {
                    let id = try scanner.objectKey()
                    guard scanner.isObject() else { try scanner.skipValue(); continue }
                    try scanner.openObject()
                    if scanner.closesObject() { continue }
                    repeat {
                        if try scanner.objectKey() == "driver", scanner.isString() {
                            if try scanner.string() == "antigravity" { ids.append(id) }
                        } else { try scanner.skipValue() }
                    } while try scanner.nextMember()
                } while try scanner.nextMember()
            } while try scanner.nextMember()
            return ids
        }) ?? []
    }
}

struct LocalUsageSnapshot: Sendable {
    var sources: [UsageDataset.Source] = []
    var files: [UsageDataset.File] = []
    var statuses: [LocalUsageStatus] = []

    func merging(_ cached: UsageDataset?) -> UsageDataset {
        let kinds = Set(sources.map(\.kind))
        var rejected = cached?.rejectedFiles ?? 0
        var tokenBudget = 0
        let combined = (cached?.files.filter { !kinds.contains($0.kind) } ?? []) + files
        let valid = combined.filter { file in
            var budget = tokenBudget
            for record in file.records {
                for value in [record.uncachedInput, record.cachedInput, record.cacheCreation, record.output, record.reasoning] {
                    let (sum, overflow) = budget.addingReportingOverflow(value)
                    guard value >= 0, !overflow else { rejected += 1; return false }
                    budget = sum
                }
            }
            tokenBudget = budget
            return true
        }
        var dataset = UsageDataset(version: cached?.version ?? 11,
                                   sources: (cached?.sources.filter { !kinds.contains($0.kind) } ?? []) + sources,
                                   files: valid, rejectedFiles: rejected)
        dataset.modified = cached?.modified ?? Date(timeIntervalSince1970: 0)
        dataset.size = cached?.size ?? 0
        return dataset
    }
}

struct LocalUsageFile: Sendable {
    let url: URL
    let root: URL
    let kind: UsageProviderKind

    init(url: URL, root: URL, kind: UsageProviderKind) {
        self.url = url.standardizedFileURL.resolvingSymlinksInPath()
        self.root = root.standardizedFileURL.resolvingSymlinksInPath()
        self.kind = kind
    }
}

struct LocalUsageRead: Sendable {
    var records: [UsageRecord] = []
    var candidates: [AntigravityUsage.Candidate] = []
    var accounts: [CursorUsage.Account] = []
    var malformed = 0
    var updated: Date?
}

struct LocalUsageScanner {
    private struct Stamp: Equatable {
        let size: Int
        let modified: Date
        let changed: Date
        init?(_ url: URL) {
            guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
                  let size = attrs[.size] as? Int, let modified = attrs[.modificationDate] as? Date else { return nil }
            self.size = size
            self.modified = modified
            changed = (try? url.resourceValues(forKeys: [.attributeModificationDateKey]))?.attributeModificationDate ?? modified
        }
    }
    private struct Fingerprint: Equatable {
        let file: Stamp?
        let wal: Stamp?
    }
    private struct Cached {
        let stamp: Fingerprint
        let read: LocalUsageRead
    }
    private var cache: [String: Cached] = [:]
    private(set) var reads = 0

    mutating func scan(paths: LocalUsagePaths, now: Date) -> LocalUsageSnapshot {
        var snapshot = LocalUsageSnapshot()
        var retained = Set<String>()
        for (kind, roots) in [(UsageProviderKind.opencode, paths.openCode), (.antigravity, paths.antigravity), (.cursor, paths.cursor.map { [$0] } ?? [])] {
            var files: [LocalUsageFile] = []
            var failures = 0
            var seen = Set<String>()
            for original in roots {
                let root = original.standardizedFileURL.resolvingSymlinksInPath()
                do {
                    let found: [LocalUsageFile]
                    if kind == .cursor {
                        found = FileManager.default.fileExists(atPath: root.path) ? [.init(url: root, root: root, kind: kind)] : []
                    } else {
                        let directory = kind == .antigravity && FileManager.default.fileExists(atPath: root.appendingPathComponent("conversations").path) ? root.appendingPathComponent("conversations") : root
                        found = try Self.discover(directory, kind: kind)
                    }
                    files += found.filter { seen.insert($0.url.path).inserted }
                } catch { failures += 1 }
            }
            var candidates: [(AntigravityUsage.Candidate, Int)] = []
            var providerFiles: [UsageDataset.File] = []
            var updated: Date?
            var malformed = 0
            for file in files {
                retained.insert(file.url.path)
                let fingerprint = Fingerprint(file: Stamp(file.url), wal: Stamp(URL(fileURLWithPath: file.url.path + "-wal")))
                var read = cache[file.url.path]?.read
                if cache[file.url.path]?.stamp != fingerprint {
                    do {
                        reads += 1
                        let fresh = try Self.read(file)
                        let after = Fingerprint(file: Stamp(file.url), wal: Stamp(URL(fileURLWithPath: file.url.path + "-wal")))
                        guard after == fingerprint else { throw LocalUsageError.changed }
                        cache[file.url.path] = Cached(stamp: fingerprint, read: fresh)
                        read = fresh
                    } catch { failures += 1 }
                }
                guard let read else { continue }
                malformed += read.malformed
                if let time = read.updated { updated = max(updated ?? time, time) }
                if kind == .cursor {
                    for account in read.accounts {
                        let path = file.url.appendingPathComponent(account.id).path
                        snapshot.sources.append(.init(kind: kind, directory: path))
                        providerFiles.append(.init(path: path, kind: kind, modifiedMs: account.updated.timeIntervalSince1970 * 1000, malformed: 0, records: account.records))
                    }
                    if read.malformed > 0 {
                        snapshot.sources.append(.init(kind: kind, directory: file.url.path + "/invalid"))
                        providerFiles.append(.init(path: file.url.path + "/invalid", kind: kind, modifiedMs: now.timeIntervalSince1970 * 1000, malformed: read.malformed, records: []))
                    }
                    continue
                }
                let index = providerFiles.count
                candidates += read.candidates.map { ($0, index) }
                let modified = now
                providerFiles.append(.init(path: file.url.path, kind: kind, modifiedMs: modified.timeIntervalSince1970 * 1000, malformed: read.malformed, records: read.records))
                let source = UsageDataset.Source(kind: kind, directory: file.root.path)
                if !snapshot.sources.contains(source) { snapshot.sources.append(source) }
            }
            if kind == .antigravity {
                for (record, index) in AntigravityUsage.merge(candidates) { providerFiles[index].records.append(record) }
            }
            snapshot.files += providerFiles
            let state: LocalUsageStatus.State = failures > 0 ? (providerFiles.isEmpty ? .unreadable : .partial) : malformed > 0 ? .partial : files.isEmpty ? .missing : .ready
            snapshot.statuses.append(.init(kind: kind, state: state, checkedAt: now, dataUpdatedAt: updated, failedFiles: failures))
        }
        cache = cache.filter { retained.contains($0.key) }
        return snapshot
    }

    private static func discover(_ root: URL, kind: UsageProviderKind) throws -> [LocalUsageFile] {
        guard FileManager.default.fileExists(atPath: root.path) else { return [] }
        var result: [LocalUsageFile] = []
        func walk(_ directory: URL) throws {
            for url in try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.isSymbolicLinkKey, .isDirectoryKey]).sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
                let values = try url.resourceValues(forKeys: [.isSymbolicLinkKey, .isDirectoryKey])
                if values.isSymbolicLink == true { continue }
                if values.isDirectory == true { try walk(url) }
                else if kind == .antigravity ? url.pathExtension == "db" : url.pathExtension == "json" {
                    result.append(.init(url: url, root: root, kind: kind))
                }
            }
        }
        if kind == .antigravity { try walk(root) }
        else {
            let dbs = try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.isSymbolicLinkKey])
                .filter { $0.lastPathComponent.range(of: "^opencode(?:-[a-zA-Z0-9_-]+)?\\.db$", options: .regularExpression) != nil }
                .sorted { ($0.lastPathComponent == "opencode.db" ? "" : $0.lastPathComponent) < ($1.lastPathComponent == "opencode.db" ? "" : $1.lastPathComponent) }
            for url in dbs where (try url.resourceValues(forKeys: [.isSymbolicLinkKey])).isSymbolicLink != true {
                result.append(.init(url: url, root: root, kind: kind))
            }
            let legacy = root.appendingPathComponent("storage/message")
            if FileManager.default.fileExists(atPath: legacy.path) { try walk(legacy) }
        }
        return result
    }

    private static func read(_ file: LocalUsageFile) throws -> LocalUsageRead {
        switch file.kind {
        case .opencode: return try OpenCodeUsage.read(file.url)
        case .antigravity: return try AntigravityUsage.read(file.url)
        case .cursor: return try CursorUsage.read(file.url)
        default: throw LocalUsageError.schema
        }
    }
}

enum LocalUsageError: Error { case schema, malformed, changed, sqlite }
