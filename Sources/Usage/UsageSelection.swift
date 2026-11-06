import Foundation

extension UsageDataset {
    /// A record that survived de-duplication and falls inside the window.
    public struct Counted {
        public let kind: UsageProviderKind
        public let sourceIndex: Int
        public let record: UsageRecord
        public let slot: UsageWindow.Slot
    }

    public struct Tally: Equatable, Sendable {
        public let kind: UsageProviderKind
        public let directory: String
        public let distinctSessions: Int
        public let malformed: Int
    }

    public struct Selection: Equatable, Sendable {
        public let sources: [Tally]
        public let duplicatesDropped: Int
        public let outOfWindow: Int

        /// T3 Code counts a session once per source, so one in two sources counts twice.
        public var sessions: Int { sources.reduce(0) { $0 + $1.distinctSessions } }
    }

    /// Where T3 Code's transcript readers run, in its order. Kinds this build does not know follow.
    private static let readerOrder: [UsageProviderKind] = [.claude, .codex, .grok, .pi, .muse]

    private struct TokenKey: Hashable {
        let uncached, cached, creation, output, reasoning: Int
    }

    /// Codex forks and subagents copy their parent's history into new files.
    private struct SharedCodexKey: Hashable {
        let session: String
        let timestampMs: Double
        let model: String
        let tokens: TokenKey
    }

    private enum DedupeKey: Hashable {
        case text(String)
        case sharedCodex(SharedCodexKey, occurrence: Int)
    }

    /// Streams every counted record to `body`.
    ///
    /// The first record with a given key wins, in source order, then file order. Keys are checked before
    /// the window, so an out-of-window copy can hide an in-window one, exactly as in T3 Code.
    @discardableResult
    public func select(in window: UsageWindow, _ body: (Counted) -> Void) -> Selection {
        let cutoff = window.fileCutoffMs
        let ordered = Self.readerOrder.flatMap { kind in sources.filter { $0.kind == kind } }
            + sources.filter { !Self.readerOrder.contains($0.kind) }
        let groups = ordered.map { source in
            files.filter { $0.kind == source.kind && $0.modifiedMs >= cutoff && Self.isWithin($0.path, source.directory) }
        }
        let shared = Self.sharedCodexSessions(groups)

        var seen = Set<DedupeKey>()
        var duplicates = 0, outside = 0
        var tallies: [Tally] = []
        for (sourceIndex, group) in groups.enumerated() {
            var sessions = Set<String>()
            for file in group {
                var occurrences: [SharedCodexKey: Int] = [:]
                let isCodex = file.kind == .codex && !shared.isEmpty
                for record in file.records {
                    var key = record.dedupeKey.map(DedupeKey.text)
                    if isCodex, shared.contains(record.session) {
                        let base = SharedCodexKey(
                            session: record.session, timestampMs: record.timestampMs, model: record.model,
                            tokens: TokenKey(uncached: record.uncachedInput, cached: record.cachedInput,
                                             creation: record.cacheCreation, output: record.output, reasoning: record.reasoning))
                        let occurrence = occurrences[base, default: 0] + 1
                        occurrences[base] = occurrence
                        key = .sharedCodex(base, occurrence: occurrence)
                    }
                    if let key, !seen.insert(key).inserted {
                        duplicates += 1
                        continue
                    }
                    guard let slot = window.slot(ofMs: record.timestampMs) else {
                        outside += 1
                        continue
                    }
                    if !record.session.isEmpty { sessions.insert(record.session) }
                    body(Counted(kind: file.kind, sourceIndex: sourceIndex, record: record, slot: slot))
                }
            }
            tallies.append(Tally(kind: ordered[sourceIndex].kind, directory: ordered[sourceIndex].directory,
                                 distinctSessions: sessions.count, malformed: group.reduce(0) { $0 + $1.malformed }))
        }
        return Selection(sources: tallies, duplicatesDropped: duplicates, outOfWindow: outside)
    }

    private static func sharedCodexSessions(_ groups: [[File]]) -> Set<String> {
        var firstFile: [String: Int] = [:]
        var shared = Set<String>()
        var index = 0
        for file in groups.joined() {
            defer { index += 1 }
            guard file.kind == .codex else { continue }
            var previous = ""
            for record in file.records where record.session != previous && !record.session.isEmpty {
                previous = record.session
                if let first = firstFile[record.session] {
                    if first != index { shared.insert(record.session) }
                } else {
                    firstFile[record.session] = index
                }
            }
        }
        return shared
    }

    static func isWithin(_ path: String, _ directory: String) -> Bool {
        if hasIrregularSegments(path) || hasIrregularSegments(directory) {
            return isWithin(normalize(path), normalize(directory))
        }
        let prefix = directory.hasSuffix("/") ? directory : directory + "/"
        return path == directory || path.hasPrefix(prefix)
    }

    /// An empty, `.` or `..` segment, which a path T3 Code wrote itself never has.
    private static func hasIrregularSegments(_ path: String) -> Bool {
        var segmentLength = 0, dots = 0, previousWasSlash = false
        for byte in path.utf8 {
            if byte == UInt8(ascii: "/") {
                if previousWasSlash || (segmentLength > 0 && segmentLength == dots && dots <= 2) { return true }
                segmentLength = 0
                dots = 0
                previousWasSlash = true
            } else {
                previousWasSlash = false
                segmentLength += 1
                if byte == UInt8(ascii: ".") { dots += 1 }
            }
        }
        return segmentLength > 0 && segmentLength == dots && dots <= 2
    }

    private static func normalize(_ path: String) -> String {
        var parts: [Substring] = []
        for part in path.split(separator: "/", omittingEmptySubsequences: true) {
            if part == "." { continue }
            if part == ".." { _ = parts.popLast(); continue }
            parts.append(part)
        }
        return "/" + parts.joined(separator: "/")
    }
}
