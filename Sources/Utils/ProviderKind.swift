import Foundation

/// The provider behind a thread, as shown by its logo.
///
/// T3 Code names providers by driver (`codex`, `claudeAgent`, `opencode`, ...).
/// A custom instance of a driver is named `<driver>_<id>`, for example
/// `codex_6a18363d-...`, so the driver is matched as an exact name or a prefix.
/// Model names are matched too, because a thread without run history falls back
/// to its model name.
public enum ProviderKind: String, Hashable, CaseIterable, Sendable {
    case openAI, claude, gemini, grok, cursor, openCode, antigravity, fx, kiro, pi, muse, unknown

    /// Driver ids that have their own logo in T3 Code.
    private static let drivers: [(prefix: String, kind: ProviderKind)] = [
        ("codex", .openAI), ("claudeagent", .claude), ("cursor", .cursor), ("grok", .grok),
        ("opencode", .openCode), ("antigravity", .antigravity), ("fx", .fx),
        ("kiro", .kiro), ("pi", .pi), ("muse", .muse),
    ]

    public static func resolve(_ raw: String) -> ProviderKind {
        let name = raw.lowercased()
        for (prefix, kind) in drivers where name == prefix || name.hasPrefix(prefix + "_") {
            return kind
        }
        if name.contains("grok") || name.contains("xai") { return .grok }
        if ["claude", "anthropic", "sonnet", "opus", "haiku"].contains(where: name.contains) { return .claude }
        if name.contains("gemini") || name.contains("google") { return .gemini }
        if ["gpt", "openai", "codex"].contains(where: name.contains) || name.hasPrefix("o3") || name.hasPrefix("o4") {
            return .openAI
        }
        return .unknown
    }

    /// Distinct kinds in first-seen order. An empty list shows the OpenAI logo,
    /// as before, because Codex is by far the most common provider.
    public static func resolveUnique(_ raws: [String]) -> [ProviderKind] {
        var result: [ProviderKind] = []
        for raw in raws {
            let kind = resolve(raw)
            if !result.contains(kind) { result.append(kind) }
        }
        return result.isEmpty ? [.openAI] : result
    }
}
