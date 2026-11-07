import Foundation
import Testing
@testable import T3CodeBar

struct ScriptResult {
    let status: Int32
    let stdout: String
    let stderr: String
}

final class InstallReleaseFixture {
    static let appName = "T3 Code Bar.app"
    static let scriptURL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("scripts/install-release.sh")

    let root = FileManager.default.temporaryDirectory
        .appendingPathComponent("install-release-\(UUID().uuidString)")
    let installDir: URL
    let releaseBase: URL
    let apiFile: URL
    let tmpDir: URL
    let shimDir: URL
    var destination: URL { installDir.appendingPathComponent(Self.appName) }

    init() throws {
        installDir = root.appendingPathComponent("Applications")
        releaseBase = root.appendingPathComponent("releases")
        apiFile = root.appendingPathComponent("latest.json")
        tmpDir = root.appendingPathComponent("tmp")
        shimDir = root.appendingPathComponent("shims")
        for dir in [installDir, releaseBase, tmpDir, shimDir] {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
    }

    deinit {
        try? FileManager.default.removeItem(at: root)
    }

    func run(_ shell: String, _ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/bash")
        process.arguments = ["-c", shell, "--"] + arguments
        let pipe = Pipe()
        process.standardError = pipe
        try process.run()
        process.waitUntilExit()
        let message = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        try #require(process.terminationStatus == 0, "fixture command failed: \(message)")
    }

    func makeApp(at url: URL, version: String, marker: String = "") throws {
        let contents = url.appendingPathComponent("Contents")
        try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
        let plist: [String: Any] = ["CFBundleShortVersionString": version]
        let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
        try data.write(to: contents.appendingPathComponent("Info.plist"))
        if !marker.isEmpty {
            try marker.write(to: url.appendingPathComponent("marker.txt"), atomically: true, encoding: .utf8)
        }
    }

    func installApp(version: String, marker: String = "old") throws {
        try makeApp(at: destination, version: version, marker: marker)
    }

    func installedVersion() -> String? {
        let plist = destination.appendingPathComponent("Contents/Info.plist")
        guard let data = try? Data(contentsOf: plist),
              let object = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
        else { return nil }
        return object["CFBundleShortVersionString"] as? String
    }

    func installedMarker() -> String? {
        try? String(contentsOf: destination.appendingPathComponent("marker.txt"), encoding: .utf8)
    }

    func installDirEntries() throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: installDir.path).sorted()
    }

    func writeLatest(tag: String) throws {
        try "{\"tag_name\": \"\(tag)\"}".write(to: apiFile, atomically: true, encoding: .utf8)
    }

    enum Checksum { case valid, wrong }

    /// Publishes `<base>/<tag>/<zipName>` plus its `.sha256`. `bundleDirName` lets a test ship an archive
    /// that lacks the expected app bundle, and `packagedVersion` one whose plist disagrees with the tag.
    @discardableResult
    func publish(
        tag: String,
        packagedVersion: String? = nil,
        zipName: String? = nil,
        bundleDirName: String = InstallReleaseFixture.appName,
        checksum: Checksum = .valid
    ) throws -> String {
        let version = String(tag.dropFirst())
        let name = zipName ?? "T3-Code-Bar-\(version)-universal.zip"
        let tagDir = releaseBase.appendingPathComponent(tag)
        let source = root.appendingPathComponent("build-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tagDir, withIntermediateDirectories: true)
        try makeApp(at: source.appendingPathComponent(bundleDirName), version: packagedVersion ?? version, marker: "new")
        let zip = tagDir.appendingPathComponent(name)
        try run(#"ditto -c -k --keepParent "$1" "$2""#, [source.appendingPathComponent(bundleDirName).path, zip.path])
        let hash: String
        switch checksum {
        case .valid:
            let output = Pipe()
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/shasum")
            process.arguments = ["-a", "256", zip.path]
            process.standardOutput = output
            try process.run()
            process.waitUntilExit()
            let line = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            hash = String(line.split(separator: " ").first ?? "")
        case .wrong:
            hash = String(repeating: "0", count: 64)
        }
        try "\(hash)  \(name)\n".write(to: tagDir.appendingPathComponent("\(name).sha256"), atomically: true, encoding: .utf8)
        return name
    }

    /// Makes `mv` fail only for the final move of the staged app into the destination.
    func installFailingMoveShim() throws {
        let shim = shimDir.appendingPathComponent("mv")
        let body = """
        #!/bin/bash
        if [[ "$1" == *"/.t3-code-bar-install."*"/\(Self.appName)" ]]; then
            echo "mv: simulated failure" >&2
            exit 1
        fi
        exec /bin/mv "$@"
        """
        try body.write(to: shim, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: shim.path)
    }

    func runScript(_ arguments: [String] = [], arch: String = "arm64") throws -> ScriptResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/bash")
        process.arguments = [Self.scriptURL.path] + arguments
        var environment = ProcessInfo.processInfo.environment
        environment["T3_CODE_BAR_API_URL"] = apiFile.absoluteString
        environment["T3_CODE_BAR_RELEASE_BASE_URL"] = releaseBase.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        environment["T3_CODE_BAR_INSTALL_DIR"] = installDir.path
        environment["T3_CODE_BAR_ARCH"] = arch
        environment["T3_CODE_BAR_SKIP_OPEN"] = "1"
        environment["TMPDIR"] = tmpDir.path
        environment["PATH"] = shimDir.path + ":" + (environment["PATH"] ?? "/usr/bin:/bin")
        process.environment = environment
        let out = Pipe()
        let err = Pipe()
        process.standardOutput = out
        process.standardError = err
        try process.run()
        let outData = out.fileHandleForReading.readDataToEndOfFile()
        let errData = err.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return ScriptResult(
            status: process.terminationStatus,
            stdout: String(decoding: outData, as: UTF8.self),
            stderr: String(decoding: errData, as: UTF8.self)
        )
    }
}

@Suite struct InstallReleaseScriptTests {
    @Test func freshInstallIntoEmptyDirectory() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3")

        let result = try fixture.runScript()

        #expect(result.status == 0, "\(result.stderr)")
        #expect(result.stdout.contains("Installed v1.2.3"))
        #expect(fixture.installedVersion() == "1.2.3")
        #expect(fixture.installedMarker() == "new")
        #expect(try fixture.installDirEntries() == [InstallReleaseFixture.appName])
    }

    @Test func upgradeReplacesOlderVersion() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.installApp(version: "1.0.0")
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3")

        let result = try fixture.runScript()

        #expect(result.status == 0, "\(result.stderr)")
        #expect(result.stdout.contains("Updated v1.0.0 -> v1.2.3"))
        #expect(fixture.installedVersion() == "1.2.3")
        #expect(fixture.installedMarker() == "new")
        #expect(try fixture.installDirEntries() == [InstallReleaseFixture.appName])
    }

    @Test func alreadyUpToDateLeavesAppUntouchedAndForceReinstalls() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.installApp(version: "1.2.3")
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3")

        let skipped = try fixture.runScript()
        #expect(skipped.status == 0, "\(skipped.stderr)")
        #expect(skipped.stdout.contains("Already up to date (v1.2.3)"))
        #expect(fixture.installedMarker() == "old")

        let forced = try fixture.runScript(["--force"])
        #expect(forced.status == 0, "\(forced.stderr)")
        #expect(fixture.installedVersion() == "1.2.3")
        #expect(fixture.installedMarker() == "new")
    }

    @Test func checksumMismatchLeavesExistingAppUntouched() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.installApp(version: "1.0.0")
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3", checksum: .wrong)

        let result = try fixture.runScript()

        #expect(result.status != 0)
        #expect(result.stderr.contains("Checksum mismatch; nothing was installed."))
        #expect(fixture.installedVersion() == "1.0.0")
        #expect(fixture.installedMarker() == "old")
        #expect(try fixture.installDirEntries() == [InstallReleaseFixture.appName])
    }

    @Test func archiveWithoutAppBundleInstallsNothing() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3", bundleDirName: "Other.app")

        let result = try fixture.runScript()

        #expect(result.status != 0)
        #expect(result.stderr.contains("The archive did not contain \(InstallReleaseFixture.appName)."))
        #expect(try fixture.installDirEntries().isEmpty)
    }

    @Test func packagedVersionDifferingFromTagFailsAndKeepsExistingApp() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.installApp(version: "1.0.0")
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3", packagedVersion: "9.9.9")

        let result = try fixture.runScript()

        #expect(result.status != 0)
        #expect(result.stderr.contains("Archive reports version '9.9.9', expected '1.2.3'."))
        #expect(fixture.installedVersion() == "1.0.0")
        #expect(fixture.installedMarker() == "old")
    }

    @Test(arguments: ["nightly", "1.2.3", "v1.2", "v1.2.3-beta"])
    func malformedTagIsRejected(tag: String) throws {
        let fixture = try InstallReleaseFixture()
        try fixture.writeLatest(tag: tag)

        let result = try fixture.runScript()

        #expect(result.status != 0)
        #expect(result.stderr.contains("Unexpected version '\(tag)'"))
        #expect(try fixture.installDirEntries().isEmpty)
    }

    @Test func malformedRequestedVersionIsRejected() throws {
        let fixture = try InstallReleaseFixture()

        let result = try fixture.runScript(["--version", "nightly"])

        #expect(result.status != 0)
        #expect(result.stderr.contains("Unexpected version 'nightly'"))
    }

    @Test func legacyArchNamedAssetIsUsedWhenNoUniversalZipExists() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3", zipName: "T3-Code-Bar-1.2.3-arm64.zip")

        let result = try fixture.runScript(arch: "arm64")

        #expect(result.status == 0, "\(result.stderr)")
        #expect(result.stdout.contains("T3-Code-Bar-1.2.3-arm64.zip"))
        #expect(fixture.installedVersion() == "1.2.3")
    }

    @Test func intelWithoutAnyAssetGetsIntelSpecificMessage() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3", zipName: "T3-Code-Bar-1.2.3-arm64.zip")

        let result = try fixture.runScript(arch: "x86_64")

        #expect(result.status != 0)
        #expect(result.stderr.contains("has no build for Intel Macs"))
        #expect(try fixture.installDirEntries().isEmpty)
    }

    @Test func missingReleaseOnArm64ReportsDownloadFailure() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.writeLatest(tag: "v1.2.3")

        let result = try fixture.runScript(arch: "arm64")

        #expect(result.status != 0)
        #expect(result.stderr.contains("Download failed. Check that release v1.2.3 exists."))
    }

    @Test func failedMoveRestoresPreviousApp() throws {
        let fixture = try InstallReleaseFixture()
        try fixture.installApp(version: "1.0.0")
        try fixture.writeLatest(tag: "v1.2.3")
        try fixture.publish(tag: "v1.2.3")
        try fixture.installFailingMoveShim()

        let result = try fixture.runScript()

        #expect(result.status != 0)
        #expect(result.stderr.contains("Could not place the new app; the previous version was restored."))
        #expect(fixture.installedVersion() == "1.0.0")
        #expect(fixture.installedMarker() == "old")
        #expect(try fixture.installDirEntries() == [InstallReleaseFixture.appName])
    }
}
