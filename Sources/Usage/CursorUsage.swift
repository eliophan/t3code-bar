import Foundation
import CoreFoundation

enum CursorUsage {
    struct Account: Sendable {
        let id: String
        let updated: Date
        let records: [UsageRecord]
    }

    static func read(_ url: URL) throws -> LocalUsageRead {
        guard let document = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any],
              (document["version"] as? Int) == 1, let strings = document["strings"] as? [String],
              let accounts = document["accounts"] as? [String: Any] else { throw LocalUsageError.schema }
        var result = LocalUsageRead()
        for key in accounts.keys.sorted() {
            guard let account = accounts[key] as? [String: Any], let rows = account["records"] as? [[Any]],
                  let fetched = number(account["fetchedAtMs"]), fetched > 0, fetched < 9e15 else { result.malformed += 1; continue }
            let updated = Date(timeIntervalSince1970: fetched / 1000)
            result.updated = min(result.updated ?? updated, updated)
            var records: [UsageRecord] = []
            for row in rows {
                guard row.count >= 9, let time = number(row[0]), time > 0, time < 9e15, let modelIndex = index(row[1], count: strings.count),
                      let sessionIndex = index(row[2], count: strings.count), let input = token(row[3]), let cached = token(row[4]),
                      let write = token(row[5]), let output = token(row[6]), let cost = number(row[7]), cost >= 0,
                      let dedupe = row[8] as? String else { result.malformed += 1; continue }
                let model = strings[modelIndex]
                records.append(.init(timestampMs: time, model: model,
                                            rateModel: model.replacingOccurrences(of: "-thinking$", with: "", options: .regularExpression),
                                            session: strings[sessionIndex], uncachedInput: input, cachedInput: cached, cacheCreation: write,
                                            output: output, reportedCostUsd: cost, dedupeKey: dedupe))
            }
            result.accounts.append(.init(id: key, updated: updated, records: records))
        }
        return result
    }

    private static func number(_ value: Any?) -> Double? {
        guard let value = value as? NSNumber, CFGetTypeID(value) != CFBooleanGetTypeID(), value.doubleValue.isFinite else { return nil }
        return value.doubleValue
    }
    private static func token(_ value: Any) -> Int? {
        guard let number = number(value), number >= 0, number < 9e15 else { return nil }
        return Int(number)
    }
    private static func index(_ value: Any, count: Int) -> Int? {
        guard let number = number(value), let index = Int(exactly: number), index >= 0, index < count else { return nil }
        return index
    }
}
