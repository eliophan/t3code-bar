import Foundation

public enum LimitsReadError: Error, Equatable, Sendable {
    case unreadable
}

extension EnvironmentConfig {
    /// T3 writes provider snapshots to `<base>/caches`, next to the state directory rather than inside it.
    public var cachesDirectory: URL {
        baseDir.deletingLastPathComponent().appendingPathComponent("caches", isDirectory: true)
    }
}

/// Reads the provider snapshots T3 Code keeps in `caches/*.json`. It opens nothing else,
/// makes no network call, and decodes only the keys the Limits tab needs.
public actor LimitsReader {
    private struct Signature: Equatable {
        let size: Int
        let modified: Date
    }

    private struct Entry {
        var signature: Signature
        var record: LimitsRecord?
    }

    private let directory: URL
    private var entries: [String: Entry] = [:]
    /// How many files were decoded so far. A poll that finds nothing changed leaves it alone.
    public private(set) var parseCount = 0

    public init(cachesDirectory: URL) {
        directory = cachesDirectory
    }

    public func read() throws -> LimitsSnapshot {
        let files: [(name: String, url: URL, signature: Signature)]
        do {
            files = try listFiles()
        } catch let error as CocoaError where error.code == .fileReadNoSuchFile || error.code == .fileNoSuchFile {
            entries = [:]
            return LimitsSnapshot(state: .noCacheDirectory)
        } catch {
            throw LimitsReadError.unreadable
        }
        entries = entries.filter { name, _ in files.contains { $0.name == name } }
        for file in files where entries[file.name]?.signature != file.signature {
            guard let data = try? Data(contentsOf: file.url) else { continue }
            parseCount += 1
            let record = Self.decode(data, fallbackID: file.url.deletingPathExtension().lastPathComponent)
            entries[file.name] = Entry(signature: file.signature, record: record ?? entries[file.name]?.record)
        }
        let records = files.compactMap { entries[$0.name]?.record }
        return LimitsMerge.snapshot(records, skippedFiles: files.count - records.count)
    }

    func records() -> [LimitsRecord] { entries.keys.sorted().compactMap { entries[$0]?.record } }

    private func listFiles() throws -> [(name: String, url: URL, signature: Signature)] {
        let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey, .isRegularFileKey]
        let urls = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: keys)
        return urls.compactMap { url in
            guard url.pathExtension.lowercased() == "json",
                  let values = try? url.resourceValues(forKeys: Set(keys)), values.isRegularFile == true,
                  let size = values.fileSize, let modified = values.contentModificationDate else { return nil }
            return (url.lastPathComponent, url, Signature(size: size, modified: modified))
        }.sorted { $0.name < $1.name }
    }

    // MARK: decoding

    static func decode(_ data: Data, fallbackID: String) -> LimitsRecord? {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let driver = root["driver"] as? String, !driver.isEmpty else { return nil }
        let auth = root["auth"] as? [String: Any]
        return LimitsRecord(
            instanceID: (root["instanceId"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? fallbackID,
            driver: driver,
            enabled: root["enabled"] as? Bool ?? false,
            installed: root["installed"] as? Bool ?? false,
            available: root["availability"] as? String != "unavailable",
            displayName: root["displayName"] as? String,
            authenticated: auth?["status"] as? String == "authenticated",
            plan: (auth?["label"] as? String).flatMap { $0.isEmpty ? nil : $0 },
            email: auth?["email"] as? String,
            workspaceID: auth?["workspaceId"] as? String,
            limits: (root["usageLimits"] as? [String: Any]).map(decodeLimits))
    }

    static func decodeLimits(_ object: [String: Any]) -> LimitsRecord.Limits {
        let unavailable = object["unavailable"] as? [String: Any]
        return LimitsRecord.Limits(
            checkedAt: (object["checkedAt"] as? String).flatMap(LimitsDate.parse) ?? .distantPast,
            windows: (object["windows"] as? [Any] ?? []).compactMap { ($0 as? [String: Any]).flatMap(decodeWindow) },
            credentialFingerprint: object["credentialFingerprint"] as? String,
            resetCredits: (object["resetCredits"] as? [String: Any]).flatMap(decodeCredits),
            externalUsage: (object["externalUsage"] as? [String: Any]).flatMap { usage in
                guard let label = usage["label"] as? String, let url = usage["url"] as? String else { return nil }
                return ExternalUsage(label: label, url: url)
            },
            unavailableReason: unavailable.map { $0["reason"] as? String ?? "" },
            unavailableMessage: unavailable?["message"] as? String)
    }

    private static func decodeWindow(_ object: [String: Any]) -> LimitWindow? {
        guard let id = object["id"] as? String, let used = number(object["usedPercent"]) else { return nil }
        return LimitWindow(
            id: id, kind: LimitWindowKind(raw: object["kind"] as? String), label: object["label"] as? String ?? id,
            usedPercent: used, resetsAt: (object["resetsAt"] as? String).flatMap(LimitsDate.parse),
            durationMins: number(object["windowDurationMins"]))
    }

    private static func decodeCredits(_ object: [String: Any]) -> ResetCredits? {
        guard let count = number(object["availableCount"]) else { return nil }
        return ResetCredits(availableCount: Int(max(0, min(count, 1_000_000))),
                            nextExpiresAt: (object["nextExpiresAt"] as? String).flatMap(LimitsDate.parse))
    }

    /// JSON numbers only. `true` and `false` also bridge to `NSNumber`, and a bool is not a percent.
    private static func number(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        let double = number.doubleValue
        return double.isFinite ? double : nil
    }
}
