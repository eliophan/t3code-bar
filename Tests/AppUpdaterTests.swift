import Foundation
import Testing
@testable import T3CodeBar

struct AppUpdatePolicyTests {
    @Test func newerReleaseIsInstalled() {
        #expect(AppUpdatePolicy.tagToInstall(latestTag: "v1.0.2", runningVersion: "1.0.1") == "v1.0.2")
        #expect(AppUpdatePolicy.tagToInstall(latestTag: "v1.10.0", runningVersion: "1.9.9") == "v1.10.0")
    }

    @Test func sameOrOlderReleaseIsIgnored() {
        #expect(AppUpdatePolicy.tagToInstall(latestTag: "v1.0.1", runningVersion: "1.0.1") == nil)
        #expect(AppUpdatePolicy.tagToInstall(latestTag: "v1.0.1", runningVersion: "1.2.0") == nil)
    }

    @Test func malformedVersionsAreIgnored() {
        #expect(AppUpdatePolicy.tagToInstall(latestTag: "nightly", runningVersion: "1.0.0") == nil)
        #expect(AppUpdatePolicy.tagToInstall(latestTag: "v1.0.2", runningVersion: "dev") == nil)
        #expect(AppUpdatePolicy.tagToInstall(latestTag: "v1.0.2-beta", runningVersion: "1.0.0") == nil)
    }

    @Test func tagIsReadFromTheReleaseJSON() {
        let json = Data(#"{"id": 1, "tag_name": "v1.2.3", "name": "v1.2.3"}"#.utf8)
        #expect(AppUpdatePolicy.latestTag(fromReleaseJSON: json) == "v1.2.3")
        #expect(AppUpdatePolicy.latestTag(fromReleaseJSON: Data("{}".utf8)) == nil)
    }

    @Test func swiftRunNeverSelfUpdates() {
        let home = URL(fileURLWithPath: "/tmp/Applications/T3 Code Bar.app")
        #expect(!AppUpdatePolicy.canSelfUpdate(bundle: .main, installedAt: home))
    }
}
