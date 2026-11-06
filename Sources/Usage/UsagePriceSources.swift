import Foundation

/// Reads the two files that price a record: T3 Code's fetched price list and two keys of its settings.
enum UsagePriceSources {
    static let ratesFile = "usage-model-rates.json"
    static let settingsFile = "settings.json"

    private static let rateFields: Set<String> = {
        let bases = ["input_cost_per_token", "output_cost_per_token", "cache_read_input_token_cost", "cache_creation_input_token_cost"]
        return Set(bases.flatMap { base in ["", "_priority", "_ultrafast"].map { base + $0 } })
    }()

    static func load(from directory: URL) -> PriceBook {
        let settings = readSettings(directory.appendingPathComponent(settingsFile))
        return PriceBook(table: readTable(directory.appendingPathComponent(ratesFile)), overrides: settings.overrides, aliases: settings.aliases)
    }

    /// A missing or unreadable price list leaves every model unpriced, which is also what T3 Code shows.
    static func readTable(_ url: URL) -> PriceTable {
        guard let data = try? Data(contentsOf: url) else { return PriceTable() }
        let entries = try? data.withUnsafeBytes { raw -> [PriceTable.Entry] in
            var scanner = JSONScanner(raw.bindMemory(to: UInt8.self))
            var entries: [PriceTable.Entry] = []
            try scanner.openObject()
            if scanner.closesObject() { return entries }
            repeat {
                if try scanner.objectKey() == "document", scanner.isObject() {
                    entries = try readDocument(&scanner)
                } else {
                    try scanner.skipValue()
                }
            } while try scanner.nextMember()
            return entries
        }
        return entries.map(PriceTable.init(entries:)) ?? PriceTable()
    }

    private static func readDocument(_ scanner: inout JSONScanner) throws -> [PriceTable.Entry] {
        var entries: [PriceTable.Entry] = []
        try scanner.openObject()
        if scanner.closesObject() { return entries }
        repeat {
            let name = try scanner.objectKey()
            guard scanner.isObject() else {
                try scanner.skipValue()
                continue
            }
            var entry = PriceTable.Entry(name: name, values: [:], fastMultiplier: nil)
            try scanner.openObject()
            if !scanner.closesObject() {
                repeat {
                    let field = try scanner.objectKey()
                    if rateFields.contains(field), scanner.isNumber() {
                        entry.values[field] = try scanner.number()
                    } else if field == "provider_specific_entry", scanner.isObject() {
                        entry.fastMultiplier = try readFastMultiplier(&scanner)
                    } else {
                        try scanner.skipValue()
                    }
                } while try scanner.nextMember()
            }
            entries.append(entry)
        } while try scanner.nextMember()
        return entries
    }

    private static func readFastMultiplier(_ scanner: inout JSONScanner) throws -> Double? {
        var multiplier: Double?
        try scanner.openObject()
        if scanner.closesObject() { return nil }
        repeat {
            if try scanner.objectKey() == "fast", scanner.isNumber() {
                multiplier = try scanner.number()
            } else {
                try scanner.skipValue()
            }
        } while try scanner.nextMember()
        return multiplier
    }

    /// Settings hold provider credentials. The scanner skips every key but the two it wants, so nothing else
    /// is ever turned into a value, let alone kept.
    static func readSettings(_ url: URL) -> (overrides: PriceOverrides, aliases: ModelAliases) {
        guard let data = try? Data(contentsOf: url) else { return (PriceOverrides(), ModelAliases()) }
        let parsed = try? data.withUnsafeBytes { raw -> (PriceOverrides, ModelAliases) in
            var scanner = JSONScanner(raw.bindMemory(to: UInt8.self))
            var overrides = PriceOverrides()
            var aliases = ModelAliases()
            try scanner.openObject()
            if scanner.closesObject() { return (overrides, aliases) }
            repeat {
                switch try scanner.objectKey() {
                case "usagePriceOverrides" where scanner.isObject(): overrides = PriceOverrides(try readOverrides(&scanner))
                case "usageModelAliases" where scanner.isObject(): aliases = ModelAliases(try readAliases(&scanner))
                default: try scanner.skipValue()
                }
            } while try scanner.nextMember()
            return (overrides, aliases)
        }
        return parsed ?? (PriceOverrides(), ModelAliases())
    }

    private static func readOverrides(_ scanner: inout JSONScanner) throws -> [String: PriceOverrides.PerMillion] {
        var overrides: [String: PriceOverrides.PerMillion] = [:]
        try scanner.openObject()
        if scanner.closesObject() { return overrides }
        repeat {
            let model = try scanner.objectKey()
            guard scanner.isObject() else {
                try scanner.skipValue()
                continue
            }
            var input: Double?, output: Double?, cacheRead: Double?, cacheWrite: Double?
            try scanner.openObject()
            if !scanner.closesObject() {
                repeat {
                    let field = try scanner.objectKey()
                    guard scanner.isNumber() else {
                        try scanner.skipValue()
                        continue
                    }
                    let value = try scanner.number()
                    switch field {
                    case "inputCostPerMillionTokens": input = value
                    case "outputCostPerMillionTokens": output = value
                    case "cacheReadCostPerMillionTokens": cacheRead = value
                    case "cacheWriteCostPerMillionTokens": cacheWrite = value
                    default: break
                    }
                } while try scanner.nextMember()
            }
            if let input, let output {
                overrides[model] = .init(input: input, output: output, cacheRead: cacheRead, cacheWrite: cacheWrite)
            }
        } while try scanner.nextMember()
        return overrides
    }

    private static func readAliases(_ scanner: inout JSONScanner) throws -> [String: String] {
        var aliases: [String: String] = [:]
        try scanner.openObject()
        if scanner.closesObject() { return aliases }
        repeat {
            let model = try scanner.objectKey()
            if scanner.isString() { aliases[model] = try scanner.string() } else { try scanner.skipValue() }
        } while try scanner.nextMember()
        return aliases
    }
}
