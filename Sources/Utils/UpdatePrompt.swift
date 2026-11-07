import Foundation

/// The consent prompt for an automatically found update. The choices that
/// decide whether a stray keystroke can install live here so a test can pin them.
public struct UpdatePrompt: Equatable, Sendable {
    public enum Choice: Equatable, Sendable { case install, later }

    public let tag: String

    public init(tag: String) { self.tag = tag }

    public var version: String { tag.hasPrefix("v") ? String(tag.dropFirst()) : tag }
    public var messageText: String { "T3 Code Bar \(version) is available" }
    public var informativeText: String { "The app will quit and reopen to finish installing." }
    public var installTitle: String { "Install and Restart" }
    public var laterTitle: String { "Later" }

    /// What Return chooses.
    public var defaultChoice: Choice { .later }
    /// Installing quits the app, so it takes a deliberate click.
    public var installKeyEquivalent: String { "" }
}
