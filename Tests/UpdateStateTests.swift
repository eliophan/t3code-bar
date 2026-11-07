import Foundation
import Testing
@testable import T3CodeBar

struct UpdateStateTests {
    private let t0 = Date(timeIntervalSince1970: 1_000)
    private let t1 = Date(timeIntervalSince1970: 2_000)

    private func run(_ start: UpdateStatus, _ events: [UpdateEvent]) -> UpdateStatus {
        events.reduce(start) { UpdateStatus.reduce($0, $1) }
    }

    @Test func checkFinishedRecordsOutcomeAndTime() {
        let checking = UpdateStatus(state: .checking)
        let cases: [(CheckResult, UpdateState)] = [
            (.upToDate, .idle),
            (.available("v1.2.0"), .available("v1.2.0")),
            (.failed(.network), .failed(.network)),
            (.failed(.badResponse), .failed(.badResponse)),
        ]
        for (result, expected) in cases {
            let next = UpdateStatus.reduce(checking, .checkFinished(result, at: t1))
            #expect(next == UpdateStatus(state: expected, lastChecked: t1))
        }
    }

    @Test func failedCheckOverwritesAnOlderLastChecked() {
        let next = run(UpdateStatus(state: .idle, lastChecked: t0), [.checkStarted, .checkFinished(.failed(.network), at: t1)])
        #expect(next == UpdateStatus(state: .failed(.network), lastChecked: t1))
    }

    @Test func checkStartedKeepsLastChecked() {
        for state in [UpdateState.idle, .available("v1.2.0"), .failed(.network)] {
            let next = UpdateStatus.reduce(UpdateStatus(state: state, lastChecked: t0), .checkStarted)
            #expect(next == UpdateStatus(state: .checking, lastChecked: t0))
        }
    }

    @Test func checkDuringInstallIsIgnored() {
        let installing = UpdateStatus(state: .installing("v1.2.0"), lastChecked: t0)
        #expect(UpdateStatus.reduce(installing, .checkStarted) == installing)
        #expect(UpdateStatus.reduce(installing, .checkFinished(.upToDate, at: t1)) == installing)
    }

    @Test func secondInstallWhileInstallingIsIgnored() {
        let installing = UpdateStatus(state: .installing("v1.2.0"))
        #expect(UpdateStatus.reduce(installing, .installStarted(tag: "v1.3.0")) == installing)
    }

    @Test func availableThenInstallingThenIdle() {
        let start = UpdateStatus()
        let events: [UpdateEvent] = [.checkStarted, .checkFinished(.available("v1.2.0"), at: t0), .installStarted(tag: "v1.2.0")]
        #expect(run(start, events).state == .installing("v1.2.0"))
        #expect(run(start, events + [.installFinished(status: 0)]) == UpdateStatus(state: .idle, lastChecked: t0))
    }

    @Test func nonzeroInstallerExitFails() {
        let next = run(UpdateStatus(state: .installing("v1.2.0"), lastChecked: t0), [.installFinished(status: 22)])
        #expect(next == UpdateStatus(state: .failed(.installerExit(22)), lastChecked: t0))
    }

    @Test func installFinishedOutsideInstallingIsIgnored() {
        let idle = UpdateStatus()
        #expect(UpdateStatus.reduce(idle, .installFinished(status: 1)) == idle)
    }

    @Test func staleCheckResultIsIgnored() {
        let idle = UpdateStatus()
        #expect(UpdateStatus.reduce(idle, .checkFinished(.available("v1.2.0"), at: t1)) == idle)
    }

    @Test func unsupportedIsTerminal() {
        let reasons: [UnsupportedReason] = [.notReleaseBuild, .notInstalledInApplications, .missingVersion]
        for reason in reasons {
            let unsupported = UpdateStatus.reduce(UpdateStatus(), .unsupported(reason))
            #expect(unsupported.state == .unsupported(reason))
            let events: [UpdateEvent] = [.checkStarted, .checkFinished(.upToDate, at: t1), .installStarted(tag: "v1.2.0"), .installFinished(status: 0)]
            #expect(run(unsupported, events) == unsupported)
        }
    }
}
