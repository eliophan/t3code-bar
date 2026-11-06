import Foundation
import Testing
@testable import T3CodeBar

/// Runs only with `T3_CODE_BAR_REAL_STATE_DIR` set, and prints what it finds. It never writes anything.
struct UsageRealDataTests {
    private static let realDirectory: URL? = ProcessInfo.processInfo.environment["T3_CODE_BAR_REAL_STATE_DIR"]
        .map { URL(fileURLWithPath: ($0 as NSString).expandingTildeInPath) }

    @Test(.enabled(if: realDirectory != nil))
    func decodesTheRealCache() throws {
        let directory = try #require(Self.realDirectory)
        guard case .available(let file) = UsageCacheLocator(directory: directory).locate() else {
            Issue.record("no usable cache in \(directory.path)")
            return
        }
        let start = Date()
        let dataset = try UsageCacheReader().load(file)
        let seconds = Date().timeIntervalSince(start)
        let records = dataset.files.reduce(0) { $0 + $1.records.count }
        print("real cache \(file.url.lastPathComponent) v\(dataset.version): \(dataset.files.count) files, \(records) records, "
              + "\(dataset.rejectedFiles) rejected, \(file.size) bytes, decoded in \(String(format: "%.3f", seconds)) s")
    }
}
