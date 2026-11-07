import Foundation
import Testing
@testable import T3CodeBar

struct UpdateMenuModelTests {
    private static var utc: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        return calendar
    }

    private static let now = Date(timeIntervalSince1970: 1_700_000_000) // 2023-11-14 22:13 UTC
    private static let earlierToday = Date(timeIntervalSince1970: 1_700_000_000 - 8 * 3600) // 14:13

    private func model(_ state: UpdateState, lastChecked: Date? = nil, version: String? = "1.0.2") -> UpdateMenuModel {
        UpdateMenuModel(status: UpdateStatus(state: state, lastChecked: lastChecked), runningVersion: version, now: Self.now, calendar: Self.utc)
    }

    @Test(arguments: [
        (UpdateState.idle, nil, "Not checked yet", UpdateMenuModel.Action.check),
        (.idle, Self.earlierToday, "Up to date, checked 14:13", .check),
        (.idle, Self.now.addingTimeInterval(-86_400), "Up to date, checked Nov 13, 22:13", .check),
        (.checking, nil, "Checking for updates…", .checking),
        (.available("v1.2.3"), nil, "Version 1.2.3 is available", .install("v1.2.3")),
        (.installing("v1.2.3"), nil, "Installing v1.2.3…", .installing),
        (.failed(.network), nil, "Could not reach GitHub", .check),
        (.failed(.badResponse), nil, "Unexpected response from GitHub", .check),
        (.failed(.installerExit(22)), nil, "Installer failed (exit 22)", .check),
        (.unsupported(.notReleaseBuild), nil, "Updates are off for builds run from source", .unavailable),
        (.unsupported(.notInstalledInApplications), nil, "Move the app to ~/Applications to enable updates", .unavailable),
        (.unsupported(.missingVersion), nil, "Version unknown; updates are off", .unavailable),
    ] as [(UpdateState, Date?, String, UpdateMenuModel.Action)])
    func statusLineAndAction(state: UpdateState, lastChecked: Date?, line: String, action: UpdateMenuModel.Action) {
        let m = model(state, lastChecked: lastChecked)
        #expect(m.statusLine == line)
        #expect(m.action == action)
        #expect(m.versionLine == "T3 Code Bar 1.0.2")
    }

    @Test func unknownVersionOmitsTheNumber() {
        #expect(model(.idle, version: nil).versionLine == "T3 Code Bar")
    }

    @Test(arguments: [
        (UpdateMenuModel.Action.check, "Check for Updates…", true),
        (.checking, "Checking…", false),
        (.install("v1.2.3"), "Install v1.2.3 and Restart", true),
        (.installing, "Installing…", false),
        (.unavailable, "Check for Updates…", false),
    ] as [(UpdateMenuModel.Action, String, Bool)])
    func actionTitleAndEnabled(action: UpdateMenuModel.Action, title: String, enabled: Bool) {
        #expect(action.title == title)
        #expect(action.isEnabled == enabled)
    }
}
