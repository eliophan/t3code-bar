import Testing
@testable import T3CodeBar

struct UpdatePromptTests {
    private let prompt = UpdatePrompt(tag: "v1.2.3")

    @Test func returnChoosesLater() {
        #expect(prompt.defaultChoice == .later)
    }

    @Test func installingHasNoKeyEquivalent() {
        #expect(prompt.installKeyEquivalent.isEmpty)
    }

    @Test func namesTheVersionWithoutThePrefix() {
        #expect(prompt.version == "1.2.3")
        #expect(prompt.messageText == "T3 Code Bar 1.2.3 is available")
        #expect(prompt.installTitle == "Install and Restart")
        #expect(prompt.laterTitle == "Later")
    }
}
