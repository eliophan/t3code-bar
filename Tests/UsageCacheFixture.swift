import Foundation
@testable import T3CodeBar

/// Writes a scan cache the way T3 Code lays it out: `sources, version, models, sessions, files`.
struct CacheFixture {
    struct Row {
        var timestampMs: Double
        var model = 0
        var session = 0
        var uncached = 0, cached = 0, creation = 0, output = 0, reasoning = 0
        var key: String?
        var cost: Double?
        var speed = 0
        var rateModel: Int?
        var turn: String?
        var includeRateModel = true

        var json: String {
            var items: [String] = [
                Self.number(timestampMs), "\(model)", "\(session)", "\(uncached)", "\(cached)", "\(creation)", "\(output)", "\(reasoning)",
                key.map(CacheFixture.string) ?? "null", cost.map(Self.number) ?? "null", "\(speed)", rateModel.map(String.init) ?? "null",
            ]
            if let turn { items.append(CacheFixture.string(turn)) }
            return "[" + items.joined(separator: ",") + "]"
        }

        static func number(_ value: Double) -> String {
            value == value.rounded() && abs(value) < 1e15 ? String(Int64(value)) : String(value)
        }
    }

    struct FileEntry {
        var path: String
        var kind: String
        var modifiedMs: Double = 1_793_900_000_000
        var rows: [String] = []
        var tail: [String] = []
        var malformed = 0
    }

    var sources: [(kind: String, directory: String)] = []
    var models: [String] = []
    var sessions: [String] = []
    var files: [FileEntry] = []
    var version = 11
    var filesFirst = false

    static func string(_ value: String) -> String {
        let data = (try? JSONEncoder().encode(value)) ?? Data("\"\"".utf8)
        return String(decoding: data, as: UTF8.self)
    }

    var json: String {
        let sourcesJSON = sources.map { "\(Self.string("\($0.kind)\u{0}\($0.directory)")):{\"dir\":\(Self.string($0.directory)),\"volumeId\":\"1:2\"}" }
            .joined(separator: ",")
        let filesJSON = files.map { file in
            "\(Self.string(file.path)):{\"s\":1,\"m\":\(Row.number(file.modifiedMs)),\"p\":\(Self.string(file.kind)),"
                + "\"r\":[\(file.rows.joined(separator: ","))],\"t\":[\(file.tail.joined(separator: ","))],"
                + "\"mr\":\(file.malformed),\"o\":0,\"gl\":0,\"gh\":0,\"cs\":null}"
        }.joined(separator: ",")
        let modelsJSON = "\"models\":[\(models.map(Self.string).joined(separator: ","))]"
        let sessionsJSON = "\"sessions\":[\(sessions.map(Self.string).joined(separator: ","))]"
        let filesBlock = "\"files\":{\(filesJSON)}"
        if filesFirst {
            return "{\(filesBlock),\"sources\":{\(sourcesJSON)},\"version\":\(version),\(modelsJSON),\(sessionsJSON)}"
        }
        return "{\"sources\":{\(sourcesJSON)},\"version\":\(version),\(modelsJSON),\(sessionsJSON),\(filesBlock)}"
    }

    @discardableResult
    func write(to directory: URL, name: String? = nil) throws -> URL {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent(name ?? "usage-scan-cache-v\(version).json")
        try Data(json.utf8).write(to: url)
        return url
    }

    func decoded() throws -> UsageDataset {
        try Data(json.utf8).withUnsafeBytes { try UsageCacheDecoder.decode($0.bindMemory(to: UInt8.self)) }
    }
}

func temporaryDirectory() -> URL {
    FileManager.default.temporaryDirectory.appendingPathComponent("t3bar-usage-\(UUID().uuidString)")
}

/// Epoch milliseconds for an ISO-8601 instant such as `2026-11-06T09:08:00Z` or `...00.500Z`.
func ms(_ iso: String) -> Double {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    if let date = formatter.date(from: iso) { return date.timeIntervalSince1970 * 1000 }
    formatter.formatOptions = [.withInternetDateTime]
    return (formatter.date(from: iso)?.timeIntervalSince1970 ?? .nan) * 1000
}

func date(_ iso: String) -> Date { Date(timeIntervalSince1970: ms(iso) / 1000) }

/// Days since 1970-01-01 of a calendar date such as `2026-10-31`.
func dayNumber(_ isoDate: String) -> Int { Int(ms(isoDate + "T00:00:00Z") / 86_400_000) }
