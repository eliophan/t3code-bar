import Foundation
import Testing
@testable import T3CodeBar

@MainActor
struct AppUpdaterTests {
    private let newer = ReleaseInfo(tag: "v1.2.0", assetNames: ["T3-Code-Bar-1.2.0-universal.zip"])

    private final class Recorder: @unchecked Sendable {
        var installed: [String] = []
        var fetches = 0
    }

    private func updater(
        _ recorder: Recorder,
        fetch: Result<ReleaseInfo, UpdateFailure>,
        autoUpdate: Bool = false
    ) -> AppUpdater {
        AppUpdater(
            runningVersion: "1.1.0",
            fetchRelease: { recorder.fetches += 1; return fetch },
            runInstaller: { tag, _ in recorder.installed.append(tag) },
            isAutoUpdateEnabled: { autoUpdate },
            log: UpdateLog(directory: Self.tempDirectory())
        )
    }

    static func tempDirectory() -> URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("t3bar-updater-\(UUID().uuidString)", isDirectory: true)
    }

    @Test func manualCheckWithAutoUpdateOffStopsAtAvailable() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer), autoUpdate: false)
        await sut.checkNow()
        #expect(sut.state == .available("v1.2.0"))
        #expect(sut.status.lastChecked != nil)
        #expect(recorder.installed.isEmpty)
    }

    @Test func installAvailableRunsTheInstallerOnce() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer))
        await sut.checkNow()
        sut.installAvailable()
        sut.installAvailable()
        #expect(sut.state == .installing("v1.2.0"))
        #expect(recorder.installed == ["v1.2.0"])
    }

    @Test func installAvailableWithoutAnAvailableReleaseDoesNothing() {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer))
        sut.installAvailable()
        #expect(sut.state == .idle)
        #expect(recorder.installed.isEmpty)
    }

    @Test func automaticCheckInstallsWhenEnabled() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer), autoUpdate: true)
        await sut.checkAutomatically()
        #expect(sut.state == .installing("v1.2.0"))
        #expect(recorder.installed == ["v1.2.0"])
    }

    @Test func automaticCheckDoesNothingWhenDisabled() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer), autoUpdate: false)
        await sut.checkAutomatically()
        #expect(sut.state == .idle)
        #expect(recorder.fetches == 0)
    }

    @Test func manualCheckWhileUnsupportedDoesNothing() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer))
        sut.send(.unsupported(.notReleaseBuild))
        await sut.checkNow()
        #expect(sut.state == .unsupported(.notReleaseBuild))
        #expect(recorder.fetches == 0)
    }

    @Test func manualCheckWhileInstallingDoesNothing() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer))
        sut.send(.installStarted(tag: "v1.2.0"))
        await sut.checkNow()
        #expect(sut.state == .installing("v1.2.0"))
        #expect(recorder.fetches == 0)
    }

    @Test func manualCheckWhileCheckingDoesNotFetchAgain() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .success(newer))
        sut.send(.checkStarted)
        await sut.checkNow()
        #expect(sut.state == .checking)
        #expect(recorder.fetches == 0)
    }

    @Test func failedFetchRecordsFailureAndTime() async {
        let recorder = Recorder()
        let sut = updater(recorder, fetch: .failure(.network))
        await sut.checkNow()
        #expect(sut.state == .failed(.network))
        #expect(sut.status.lastChecked != nil)
    }

    @Test func installerExitIsReportedBack() async {
        let recorder = Recorder()
        var finish: (@Sendable (Int32) -> Void)?
        let sut = AppUpdater(
            runningVersion: "1.1.0",
            fetchRelease: { .success(newer) },
            runInstaller: { tag, completion in recorder.installed.append(tag); finish = completion },
            isAutoUpdateEnabled: { false },
            log: UpdateLog(directory: Self.tempDirectory())
        )
        await sut.checkNow()
        sut.installAvailable()
        finish?(22)
        await Task.yield()
        #expect(sut.state == .failed(.installerExit(22)))
    }

    private final class Scheduler: @unchecked Sendable {
        var delays: [TimeInterval] = []
        var active = 0
        var maxActive = 0

        func schedule(_ delay: TimeInterval, _ work: @escaping @Sendable () -> Void) -> () -> Void {
            delays.append(delay)
            active += 1
            maxActive = max(maxActive, active)
            return { [self] in active -= 1 }
        }
    }

    private final class Script: @unchecked Sendable {
        var results: [Result<ReleaseInfo, UpdateFailure>]
        init(_ results: [Result<ReleaseInfo, UpdateFailure>]) { self.results = results }
        func next() -> Result<ReleaseInfo, UpdateFailure> { results.count > 1 ? results.removeFirst() : results[0] }
    }

    private func retrying(_ script: Script, _ scheduler: Scheduler) -> AppUpdater {
        AppUpdater(
            runningVersion: "1.1.0",
            fetchRelease: { script.next() },
            runInstaller: { _, _ in },
            isAutoUpdateEnabled: { true },
            scheduleRetry: { scheduler.schedule($0, $1) },
            log: UpdateLog(directory: Self.tempDirectory())
        )
    }

    @Test func automaticFailuresBackOffWithoutStackingTimers() async {
        let scheduler = Scheduler()
        let sut = retrying(Script([.failure(.network)]), scheduler)
        for _ in 0..<5 { await sut.checkAutomatically() }
        #expect(scheduler.delays == [300, 900, 3600])
        #expect(scheduler.maxActive == 1)
        #expect(scheduler.active == 0)
    }

    @Test func successResetsTheBackoff() async {
        let scheduler = Scheduler()
        let upToDate = ReleaseInfo(tag: "v1.1.0", assetNames: [])
        let sut = retrying(Script([.failure(.network), .failure(.badResponse), .success(upToDate), .failure(.rateLimited)]), scheduler)
        for _ in 0..<4 { await sut.checkAutomatically() }
        #expect(scheduler.delays == [300, 900, 300])
        #expect(scheduler.active == 1)
    }

    @Test func manualCheckNeverSchedulesARetry() async {
        let scheduler = Scheduler()
        let sut = retrying(Script([.failure(.network)]), scheduler)
        await sut.checkNow()
        #expect(sut.state == .failed(.network))
        #expect(scheduler.delays.isEmpty)
    }

    @Test func manualSuccessCancelsThePendingRetry() async {
        let scheduler = Scheduler()
        let upToDate = ReleaseInfo(tag: "v1.1.0", assetNames: [])
        let sut = retrying(Script([.failure(.network), .success(upToDate)]), scheduler)
        await sut.checkAutomatically()
        await sut.checkNow()
        #expect(scheduler.delays == [300])
        #expect(scheduler.active == 0)
    }

    @Test func installerExitIsNotRetried() async {
        let scheduler = Scheduler()
        let sut = retrying(Script([.failure(.installerExit(1))]), scheduler)
        await sut.checkAutomatically()
        #expect(scheduler.delays.isEmpty)
    }

    @Test func rateLimitIsRetried() async {
        let scheduler = Scheduler()
        let sut = retrying(Script([.failure(.rateLimited)]), scheduler)
        await sut.checkAutomatically()
        #expect(scheduler.delays == [300])
    }

    @Test func failedCheckIsLoggedWithoutDetails() async {
        let log = UpdateLog(directory: Self.tempDirectory())
        let sut = AppUpdater(
            runningVersion: "1.1.0",
            fetchRelease: { .failure(.rateLimited) },
            runInstaller: { _, _ in },
            isAutoUpdateEnabled: { true },
            scheduleRetry: { _, _ in {} },
            log: log
        )
        await sut.checkAutomatically()
        let text = (try? String(contentsOf: log.fileURL, encoding: .utf8)) ?? ""
        #expect(text.contains("update check failed: rate limited"))
    }
}

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

    @Test func releaseIsReadFromTheJSON() {
        let json = Data(#"{"id": 1, "tag_name": "v1.2.3", "assets": [{"name": "a.zip", "size": 3}, {"id": 9}, {"name": "b.sha256"}]}"#.utf8)
        #expect(AppUpdatePolicy.release(fromReleaseJSON: json) == ReleaseInfo(tag: "v1.2.3", assetNames: ["a.zip", "b.sha256"]))
    }

    @Test func missingAssetsKeyMeansNoAssets() {
        let json = Data(#"{"tag_name": "v1.2.3"}"#.utf8)
        #expect(AppUpdatePolicy.release(fromReleaseJSON: json) == ReleaseInfo(tag: "v1.2.3", assetNames: []))
    }

    @Test func malformedReleaseJSONIsRejected() {
        #expect(AppUpdatePolicy.release(fromReleaseJSON: Data("{}".utf8)) == nil)
        #expect(AppUpdatePolicy.release(fromReleaseJSON: Data("not json".utf8)) == nil)
        #expect(AppUpdatePolicy.release(fromReleaseJSON: Data(#"{"tag_name": "v1.0.0", "assets": "x"}"#.utf8)) == nil)
    }

    @Test func assetNameMatchesThePackagedZip() {
        #expect(UpdatePlatform.current.assetName(version: "1.2.3") == "T3-Code-Bar-1.2.3-universal.zip")
    }

    @Test(arguments: [
        ("v1.2.0", ["T3-Code-Bar-1.2.0-universal.zip"], "1.1.0", "v1.2.0"),
        ("v1.2.0", [], "1.1.0", nil),
        ("v1.2.0", ["T3-Code-Bar-1.1.0-universal.zip"], "1.1.0", nil),
        ("v1.1.0", ["T3-Code-Bar-1.1.0-universal.zip"], "1.1.0", nil),
        ("v1.0.0", ["T3-Code-Bar-1.0.0-universal.zip"], "1.1.0", nil),
    ] as [(String, [String], String, String?)])
    func offersOnlyNewerReleasesThatCarryTheAsset(tag: String, assets: [String], running: String, expected: String?) {
        let release = ReleaseInfo(tag: tag, assetNames: assets)
        #expect(AppUpdatePolicy.tagToInstall(release: release, runningVersion: running, platform: .macOS) == expected)
    }

    @Test(arguments: [
        (false, "/a/App.app", "/a/App.app", "1.0.0", UnsupportedReason.notReleaseBuild),
        (true, "/b/App.app", "/a/App.app", "1.0.0", UnsupportedReason.notInstalledInApplications),
        (true, "/a/App.app", "/a/App.app", nil, UnsupportedReason.missingVersion),
    ] as [(Bool, String, String, String?, UnsupportedReason)])
    func unsupportedReasons(release: Bool, bundle: String, installed: String, version: String?, expected: UnsupportedReason) {
        let reason = AppUpdatePolicy.unsupportedReason(
            isReleaseBuild: release, bundleURL: URL(fileURLWithPath: bundle),
            installedAt: URL(fileURLWithPath: installed), version: version)
        #expect(reason == expected)
    }

    @Test func supportedReleaseBuildHasNoReason() {
        let url = URL(fileURLWithPath: "/a/App.app")
        #expect(AppUpdatePolicy.unsupportedReason(isReleaseBuild: true, bundleURL: url, installedAt: url, version: "1.0.0") == nil)
    }

    @Test func swiftRunNeverSelfUpdates() {
        let home = URL(fileURLWithPath: "/tmp/Applications/T3 Code Bar.app")
        #expect(AppUpdatePolicy.unsupportedReason(bundle: .main, installedAt: home) == .notReleaseBuild)
    }
}
