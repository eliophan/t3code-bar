import Foundation

/// The provider kinds T3 Code's Usage page counts, plus whatever a newer T3 Code might add.
///
/// An unknown kind keeps its raw name, is labelled with it, and still counts in every total.
public enum UsageProviderKind: Hashable, Sendable {
    case codex, claude, grok, cursor, opencode, antigravity, pi, muse
    case other(String)

    /// T3 Code's registry order, which is also the order its provider rows appear in.
    public static let known: [UsageProviderKind] = [.codex, .claude, .grok, .cursor, .opencode, .antigravity, .pi, .muse]

    public init(raw: String) {
        self = Self.known.first { $0.raw == raw } ?? .other(raw)
    }

    public var raw: String {
        switch self {
        case .codex: return "codex"
        case .claude: return "claude"
        case .grok: return "grok"
        case .cursor: return "cursor"
        case .opencode: return "opencode"
        case .antigravity: return "antigravity"
        case .pi: return "pi"
        case .muse: return "muse"
        case .other(let raw): return raw
        }
    }

    public var style: UsageProviderStyle {
        switch self {
        case .codex: return .init(label: "Codex", driver: "codex", accentHex: nil)
        case .claude: return .init(label: "Claude Code", driver: "claudeAgent", accentHex: "#d97757")
        case .grok: return .init(label: "Grok Build", driver: "grok", accentHex: nil)
        case .cursor: return .init(label: "Cursor", driver: "cursor", accentHex: "#8b8b8b")
        case .opencode: return .init(label: "OpenCode", driver: "opencode", accentHex: "#5b9bbd")
        case .antigravity: return .init(label: "Antigravity", driver: "antigravity", accentHex: "#8c7bd1")
        case .pi: return .init(label: "Pi", driver: "pi", accentHex: "#4d9f8a")
        case .muse: return .init(label: "Muse Code", driver: "muse", accentHex: "#0866ff")
        case .other(let raw): return .init(label: raw, driver: nil, accentHex: "#8b8b8b")
        }
    }

    /// Known kinds in registry order, then unknown ones by name.
    public static func displayOrder(_ lhs: UsageProviderKind, _ rhs: UsageProviderKind) -> Bool {
        switch (known.firstIndex(of: lhs), known.firstIndex(of: rhs)) {
        case let (left?, right?): return left < right
        case (.some, nil): return true
        case (nil, .some): return false
        case (nil, nil): return lhs.raw < rhs.raw
        }
    }
}

public struct UsageProviderStyle: Equatable, Sendable {
    public let label: String
    /// The T3 Code driver kind, which picks the logo. Nil for a kind this build does not know.
    public let driver: String?
    /// Nil means the foreground color.
    public let accentHex: String?
}
