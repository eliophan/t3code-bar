import Foundation
import Testing
@testable import T3CodeBar

private func priceTable(_ document: [String: [String: Any]]) -> PriceTable {
    var entries: [PriceTable.Entry] = []
    for (name, fields) in document.sorted(by: { $0.key < $1.key }) {
        var values: [String: Double] = [:]
        var multiplier: Double?
        for (field, value) in fields {
            if field == "provider_specific_entry", let nested = value as? [String: Double] { multiplier = nested["fast"] }
            else if let number = value as? Double { values[field] = number }
        }
        entries.append(.init(name: name, values: values, fastMultiplier: multiplier))
    }
    return PriceTable(entries: entries)
}

private func record(_ model: String = "m", uncached: Int = 0, cached: Int = 0, creation: Int = 0, output: Int = 0,
                    speed: UsageSpeed = .standard, reported: Double? = nil, rateModel: String? = nil) -> UsageRecord {
    UsageRecord(timestampMs: 1, model: model, rateModel: rateModel, uncachedInput: uncached, cachedInput: cached,
                cacheCreation: creation, output: output, reportedCostUsd: reported, speed: speed)
}

struct UsagePricingWorkedExampleTests {
    private let table = priceTable([
        "gpt-6.1-sol": ["input_cost_per_token": 2e-6, "output_cost_per_token": 1e-5],
        "gpt-6-luna": ["input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7,
                       "input_cost_per_token_priority": 2e-7, "output_cost_per_token_priority": 1e-6],
        "claude-sonnet-5-5": ["input_cost_per_token": 2e-6, "output_cost_per_token": 1e-5, "cache_creation_input_token_cost": 2.5e-6],
        "gpt-5.4": ["input_cost_per_token": 2.5e-6, "output_cost_per_token": 1.5e-5, "cache_read_input_token_cost": 2.5e-7],
        "pi-model": ["input_cost_per_token": 2e-5, "output_cost_per_token": 1e-5, "cache_read_input_token_cost": 3e-6],
    ])

    private func price(_ record: UsageRecord) -> PricedRecord { PriceBook(table: table).price(record) }

    @Test func plainInputAndOutputAtListPrice() {
        let priced = price(record("gpt-6.1-sol", uncached: 30_804, output: 61))
        #expect(abs(priced.costUsd - 0.062218) < 1e-12)
        #expect(priced.source == .modelPriced)
        #expect(priced.cacheSavingsUsd == 0)
    }

    @Test func aFastTurnCostsMoreAndTheExtraIsThePremium() {
        let fast = price(record("gpt-6-luna", uncached: 140_552, output: 925, speed: .fast))
        #expect(abs(fast.costUsd - 0.0290354) < 1e-12)
        #expect(abs(fast.speedPremiumUsd - 0.0145177) < 1e-12)
        #expect(price(record("gpt-6-luna", uncached: 140_552, output: 925)).speedPremiumUsd == 0)
    }

    @Test func cacheWritesUseTheirOwnPrice() {
        let priced = price(record("claude-sonnet-5-5", uncached: 2, creation: 27_570, output: 284))
        #expect(abs(priced.costUsd - 0.071769) < 1e-12)
        #expect(abs((priced.categories?.cacheWrite ?? 0) - 0.068925) < 1e-12)
    }

    @Test func cacheSavingsAreCachedTokensTimesTheGapBetweenInputAndCacheReadPrice() {
        let priced = price(record("gpt-5.4", uncached: 100, cached: 2_432))
        #expect(abs(priced.cacheSavingsUsd - 0.005472) < 1e-12)
    }

    @Test func aReportedCostWinsAndTheCategoriesAreScaledToIt() throws {
        let priced = price(record("pi-model", uncached: 1_000, cached: 10_000, output: 3_000, reported: 0.10))
        #expect(abs(priced.costUsd - 0.10) < 1e-12)
        #expect(priced.source == .providerReported)
        let categories = try #require(priced.categories)
        #expect(abs(categories.input - 0.025) < 1e-12)
        #expect(abs(categories.cacheRead - 0.0375) < 1e-12)
        #expect(categories.cacheWrite == 0)
        #expect(abs(categories.output - 0.0375) < 1e-12)
    }
}

struct UsagePricingRuleTests {
    private func book(overrides: [String: PriceOverrides.PerMillion] = [:], aliases: [String: String] = [:]) -> PriceBook {
        PriceBook(table: priceTable([
            "known": ["input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6],
            "claude-opus-9": ["input_cost_per_token": 1.5e-5, "output_cost_per_token": 7.5e-5],
        ]), overrides: PriceOverrides(overrides), aliases: ModelAliases(aliases))
    }

    @Test func aModelWithNoPriceCostsNothingAndIsUnpriced() {
        let priced = book().price(record("mystery", uncached: 1_000))
        #expect(priced.costUsd == 0)
        #expect(priced.source == .unpriced)
        #expect(priced.categories == nil)
    }

    @Test func aProviderReportedCostStandsInForAMissingPrice() {
        let priced = book().price(record("mystery", uncached: 1_000, reported: 0.33))
        #expect(priced.costUsd == 0.33)
        #expect(priced.source == .providerReported)
        #expect(priced.categories == nil)
    }

    @Test func aReportedCostWithNothingToSplitIsKeptWhole() {
        let priced = book().price(record("known", reported: 0.5))
        #expect(priced.costUsd == 0.5)
        #expect(priced.categories == nil)
    }

    @Test func aRateModelIsPricedInsteadOfTheModel() {
        let priced = book().price(record("vendor/odd-name", uncached: 1_000, rateModel: "known"))
        #expect(abs(priced.costUsd - 0.001) < 1e-12)
    }

    @Test(arguments: ["opus", "sonnet", "haiku", "fable", "<synthetic>", "synthetic", "anthropic/opus", "vendor/", ""])
    func bareFamilyNamesAreNeverPriced(model: String) {
        let table = priceTable([model.isEmpty ? " " : model: ["input_cost_per_token": 1e-6, "output_cost_per_token": 1e-6]])
        #expect(table.rate(forModel: model) == nil)
    }

    @Test func aBracketSuffixIsCutBeforeTheLookupAndNamesAreCaseInsensitive() {
        #expect(abs(book().price(record("claude-opus-9[1m]", uncached: 1_000)).costUsd - 0.015) < 1e-12)
        #expect(abs(book().price(record("  KNOWN ", uncached: 1_000)).costUsd - 0.001) < 1e-12)
    }

    @Test func anOverrideBeatsThePriceListAndSwitchesOffTheReportedCost() {
        let overrides = ["known": PriceOverrides.PerMillion(input: 5, output: 20)]
        let priced = book(overrides: overrides).price(record("known", uncached: 1_000_000, output: 1_000_000, reported: 99))
        #expect(abs(priced.costUsd - 25) < 1e-9)
        #expect(priced.source == .modelPriced)
    }

    @Test func anOverrideDefaultsItsCachePricesToTheInputPriceAndHasNoFastTier() {
        let overrides = ["known": PriceOverrides.PerMillion(input: 4, output: 8)]
        let priced = book(overrides: overrides).price(record("known", cached: 1_000_000, creation: 1_000_000, speed: .fast))
        #expect(abs(priced.costUsd - 8) < 1e-9)
        #expect(priced.speedPremiumUsd == 0)
        #expect(priced.cacheSavingsUsd == 0)
    }

    @Test func anOverrideCanPriceAModelTheListLacks() {
        let overrides = ["mystery": PriceOverrides.PerMillion(input: 1, output: 1, cacheRead: 0.1)]
        let priced = book(overrides: overrides).price(record("mystery", uncached: 1_000_000, cached: 1_000_000))
        #expect(abs(priced.costUsd - 1.1) < 1e-9)
        #expect(abs(priced.cacheSavingsUsd - 0.9) < 1e-9)
    }
}

struct PriceTableTests {
    @Test func aFastMultiplierScalesEveryStandardPrice() {
        let table = priceTable(["m": ["input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6, "cache_read_input_token_cost": 1e-7,
                                      "provider_specific_entry": ["fast": 2.0]]])
        #expect(table.rate(forModel: "m")?.fast == TokenRates(input: 2e-6, output: 4e-6, cacheRead: 2e-7, cacheWrite: 2e-6))
    }

    @Test func priorityPricesGiveTheFastTierAndMissingCacheKeysScaleLikeInput() throws {
        let table = priceTable(["m": ["input_cost_per_token": 1e-6, "output_cost_per_token": 4e-6, "cache_read_input_token_cost": 1e-7,
                                      "input_cost_per_token_priority": 2e-6, "output_cost_per_token_priority": 8e-6]])
        let fast = try #require(table.rate(forModel: "m")?.fast)
        #expect(fast.input == 2e-6)
        #expect(abs(fast.cacheRead - 2e-7) < 1e-18)
        #expect(fast.cacheWrite == 2e-6)
    }

    @Test func ultrafastReadsItsOwnKeysAndAMissingTierBillsAtStandard() throws {
        let table = priceTable(["m": ["input_cost_per_token": 1e-6, "output_cost_per_token": 4e-6,
                                      "input_cost_per_token_ultrafast": 4e-6, "output_cost_per_token_ultrafast": 1.6e-5]])
        let rate = try #require(table.rate(forModel: "m"))
        #expect(rate.ultrafast?.output == 1.6e-5)
        #expect(rate.rates(for: .fast) == rate.standard)
    }

    @Test func anEntryWithoutBothStandardPricesIsDropped() {
        let table = priceTable(["half": ["input_cost_per_token": 1e-6], "none": [:]])
        #expect(table.rates.isEmpty)
    }

    @Test func aProviderPrefixedEntryAlsoAnswersToItsBareName() {
        let table = priceTable(["anthropic/claude-haiku-4-5": ["input_cost_per_token": 1e-6, "output_cost_per_token": 5e-6]])
        #expect(table.rate(forModel: "claude-haiku-4-5")?.standard.input == 1e-6)
    }

    @Test func aBareNameThatHasItsOwnPriceIsNotReplaced() {
        let table = priceTable(["x": ["input_cost_per_token": 1e-6, "output_cost_per_token": 1e-6],
                                "azure/x": ["input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6]])
        #expect(table.rate(forModel: "x")?.standard.input == 1e-6)
    }

    @Test func aFirstPartyProviderWinsTheBareName() {
        let table = priceTable(["openrouter/dual": ["input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6],
                                "anthropic/dual": ["input_cost_per_token": 4e-6, "output_cost_per_token": 6e-6]])
        #expect(table.rate(forModel: "dual")?.standard.input == 4e-6)
    }

    @Test func providersOfTheSameRankWithDifferentPricesCancelTheBareName() {
        let table = priceTable(["openrouter/foo": ["input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6],
                                "azure/foo": ["input_cost_per_token": 2e-6, "output_cost_per_token": 3e-6]])
        #expect(table.rate(forModel: "foo") == nil)
        #expect(table.rate(forModel: "azure/foo") != nil)
    }

    @Test func providersOfTheSameRankWithTheSamePriceKeepTheBareName() {
        let table = priceTable(["openrouter/bar": ["input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6],
                                "azure/bar": ["input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6]])
        #expect(table.rate(forModel: "bar")?.standard.output == 2e-6)
    }
}

struct ModelAliasTests {
    @Test func aChainResolvesToItsEnd() {
        let aliases = ModelAliases(["a": "b", "b": "c"])
        #expect(aliases.target(for: "a") == "c")
        #expect(aliases.target(for: "b") == "c")
        #expect(aliases.target(for: "c") == nil)
    }

    @Test func aCycleIsDroppedSoEveryMemberKeepsItsName() {
        let aliases = ModelAliases(["x": "y", "y": "x", "self": "self", "ok": "known"])
        #expect(aliases.target(for: "x") == nil)
        #expect(aliases.target(for: "y") == nil)
        #expect(aliases.target(for: "self") == nil)
        #expect(aliases.target(for: "ok") == "known")
    }

    @Test func aChainThatLeadsIntoACycleIsDroppedToo() {
        let aliases = ModelAliases(["in": "x", "x": "y", "y": "x"])
        #expect(aliases.target(for: "in") == nil)
    }
}

struct UsagePriceSourceTests {
    private func directory(rates: String? = nil, settings: String? = nil) throws -> URL {
        let directory = temporaryDirectory()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        if let rates { try Data(rates.utf8).write(to: directory.appendingPathComponent("usage-model-rates.json")) }
        if let settings { try Data(settings.utf8).write(to: directory.appendingPathComponent("settings.json")) }
        return directory
    }

    @Test func theRatesFileFeedsThePriceTable() throws {
        let rates = """
        {"fetchedAtMs": 1, "document": {"gpt-x": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6, "supports_vision": true,
        "provider_specific_entry": {"fast": 3}, "nested": {"input_cost_per_token": 5}}, "sample_spec": "text"}}
        """
        let book = UsagePriceSources.load(from: try directory(rates: rates))
        let rate = try #require(book.table.rate(forModel: "gpt-x"))
        #expect(rate.standard.input == 1e-6)
        #expect(rate.fast?.input == 3e-6)
    }

    @Test func aMissingOrTruncatedRatesFileLeavesEverythingUnpriced() throws {
        #expect(UsagePriceSources.load(from: try directory()).table.rates.isEmpty)
        #expect(UsagePriceSources.load(from: try directory(rates: "{\"document\":{\"a\":{\"input_cost")).table.rates.isEmpty)
    }

    @Test func onlyTheOverrideAndAliasKeysAreReadFromSettings() throws {
        let settings = """
        {"providerInstances": {"x": {"environment": {"ANTHROPIC_AUTH_TOKEN": "not-a-real-token"}}},
         "usagePriceOverrides": {"m": {"inputCostPerMillionTokens": 5, "outputCostPerMillionTokens": 20, "cacheWriteCostPerMillionTokens": 7},
                                 "bad": {"inputCostPerMillionTokens": 1}, "worse": 3},
         "usageModelAliases": {"old": "new", "number": 4}, "other": [1, 2, {"a": "b"}]}
        """
        let book = UsagePriceSources.load(from: try directory(settings: settings))
        #expect(book.overrides.rates.keys.sorted() == ["m"])
        #expect(book.overrides.rates["m"]?.standard.cacheWrite == 7e-6)
        #expect(book.aliases.target(for: "old") == "new")
        #expect(book.aliases.target(for: "number") == nil)
    }

    @Test func missingKeysOrAnUnreadableSettingsFileMeanNoOverrides() throws {
        for settings in [nil, "{}", "{\"usageModelAliases\": 5}", "{broken"] {
            let book = UsagePriceSources.load(from: try directory(settings: settings))
            #expect(book.overrides.rates.isEmpty)
            #expect(book.aliases.target(for: "x") == nil)
        }
    }
}
