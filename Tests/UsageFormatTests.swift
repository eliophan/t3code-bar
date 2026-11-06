import Foundation
import Testing
@testable import T3CodeBar

struct TokenFormatTests {
    @Test(arguments: [
        (0, "0"), (7, "7"), (999, "999"), (1_000, "1K"),(4_200, "4.20K"), (12_345, "12.3K"), (999_000, "999K"),
        (1_500_000, "1.50M"), (10_000_000, "10M"), (193_000_000, "193M"), (16_000_000_000, "16B"),
        (1_100_000_000, "1.10B"), (16_238_000_000, "16.2B"), (12_345_678_901_234, "12.3T"),
    ] as [(Int, String)])
    func compactMatchesT3CodesFormat(count: Int, expected: String) {
        #expect(TokenFormat.compact(count) == expected)
    }

    @Test func aFractionOfAllZerosIsDroppedButATrailingZeroIsKept() {
        #expect(TokenFormat.compact(16_000_000_000) == "16B")
        #expect(TokenFormat.compact(1_000_000) == "1M")
        #expect(TokenFormat.compact(1_500_000) == "1.50M")
        #expect(TokenFormat.compact(10_500_000) == "10.5M")
    }

    @Test func aValueThatRoundsUpToAThousandStaysInItsUnit() {
        #expect(TokenFormat.compact(999_960_000) == "1000M")
        #expect(TokenFormat.compact(999_960) == "1000K")
    }

    @Test func halfwayValuesRoundUpLikeJavaScriptNotToEven() {
        #expect(TokenFormat.compact(150_500_000) == "151M")
        #expect(TokenFormat.compact(1_125_000) == "1.13M")
        #expect(TokenFormat.compact(1_005_000) == "1M")
    }

    @Test func negativeValuesKeepTheirSign() {
        #expect(TokenFormat.compact(-5) == "-5")
        #expect(TokenFormat.compact(-1_500) == "-1.50K")
    }

    @Test func spokenFormUsesWordsSoVoiceOverReadsItNaturally() {
        #expect(TokenFormat.spoken(193_000_000) == "193 million")
        #expect(TokenFormat.spoken(1_100_000_000) == "1.10 billion")
        #expect(TokenFormat.spoken(4_200) == "4.20 thousand")
        #expect(TokenFormat.spoken(12) == "12")
    }

    @Test func currencyHasGroupingAndTwoDecimalsWhateverTheMacLocale() {
        #expect(TokenFormat.currency(0) == "$0.00")
        #expect(TokenFormat.currency(1_234.5) == "$1,234.50")
        #expect(TokenFormat.currency(1_234_567.891) == "$1,234,567.89")
        #expect(TokenFormat.currency(0.005) == "$0.01")
        #expect(TokenFormat.currency(1.005) == "$1.01")
        #expect(TokenFormat.spokenCurrency(1.005) == "1 dollar and 1 cent")
        #expect(TokenFormat.currency(-3.2) == "-$3.20")
    }

    @Test func percentHasOneDecimalAndTinySharesReadAsLessThanATenth() {
        #expect(TokenFormat.percent(0) == "0.0%")
        #expect(TokenFormat.percent(0.0004) == "<0.1%")
        #expect(TokenFormat.percent(0.001) == "0.1%")
        #expect(TokenFormat.percent(0.1234) == "12.3%")
        #expect(TokenFormat.percent(1) == "100.0%")
    }

    @Test func countsAreGroupedAndSingularForOne() {
        #expect(TokenFormat.sessions(0) == "0 sessions")
        #expect(TokenFormat.sessions(1) == "1 session")
        #expect(TokenFormat.sessions(1_234_567) == "1,234,567 sessions")
    }
}

struct UsageProviderKindTests {
    @Test func knownKindsKeepT3CodesOrderAndLabels() {
        #expect(UsageProviderKind.known.map(\.raw) == ["codex", "claude", "grok", "cursor", "opencode", "antigravity", "pi", "muse"])
        #expect(UsageProviderKind.known.map(\.style.label)
                == ["Codex", "Claude Code", "Grok Build", "Cursor", "OpenCode", "Antigravity", "Pi", "Muse Code"])
        #expect(UsageProviderKind.claude.style.driver == "claudeAgent")
    }

    @Test func aRawNameRoundTripsAndAnUnknownOneKeepsItsName() {
        #expect(UsageProviderKind(raw: "pi") == .pi)
        let unknown = UsageProviderKind(raw: "newcomer")
        #expect(unknown == .other("newcomer"))
        #expect(unknown.raw == "newcomer")
        #expect(unknown.style.label == "newcomer")
        #expect(unknown.style.driver == nil)
    }

    @Test func displayOrderPutsKnownKindsFirstThenUnknownByName() {
        let shuffled: [UsageProviderKind] = [.other("zeta"), .muse, .other("alpha"), .codex, .pi]
        #expect(shuffled.sorted(by: UsageProviderKind.displayOrder) == [.codex, .pi, .muse, .other("alpha"), .other("zeta")])
    }
}
