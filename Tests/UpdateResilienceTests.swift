import Foundation
import Testing
@testable import T3CodeBar

struct UpdateRetryPolicyTests {
    @Test(arguments: [
        (0, nil), (1, 300), (2, 900), (3, 3600), (4, nil), (50, nil),
    ] as [(Int, TimeInterval?)])
    func delayTable(failures: Int, expected: TimeInterval?) {
        #expect(UpdateRetryPolicy.delay(afterConsecutiveFailures: failures) == expected)
    }

    @Test func onlyFetchFailuresAreRetryable() {
        #expect(UpdateFailure.network.isRetryable)
        #expect(UpdateFailure.badResponse.isRetryable)
        #expect(UpdateFailure.rateLimited.isRetryable)
        #expect(!UpdateFailure.installerExit(1).isRetryable)
    }
}

struct ReleaseFetcherTests {
    private static let url = URL(string: "https://example.test/releases/latest")!
    private static let body = Data(#"{"tag_name": "v1.2.0", "assets": [{"name": "a.zip"}]}"#.utf8)
    private static let release = ReleaseInfo(tag: "v1.2.0", assetNames: ["a.zip"])

    private final class Transport: @unchecked Sendable {
        var requests: [URLRequest] = []
        var replies: [Result<(Int, [String: String], Data), URLError>]

        init(_ replies: [Result<(Int, [String: String], Data), URLError>]) { self.replies = replies }

        func send(_ request: URLRequest) throws -> (Data, URLResponse) {
            requests.append(request)
            switch replies.removeFirst() {
            case .failure(let error): throw error
            case .success(let (status, headers, data)):
                return (data, HTTPURLResponse(url: ReleaseFetcherTests.url, statusCode: status, httpVersion: nil, headerFields: headers)!)
            }
        }
    }

    private func fetcher(_ transport: Transport) -> ReleaseFetcher {
        ReleaseFetcher(url: Self.url, transport: { try transport.send($0) })
    }

    @Test func okStoresTheETagAndTheNextRequestSendsIt() async {
        let transport = Transport([.success((200, ["ETag": "\"abc\""], Self.body)), .success((304, [:], Data()))])
        let sut = fetcher(transport)
        #expect(await sut.fetch() == .success(Self.release))
        #expect(transport.requests[0].value(forHTTPHeaderField: "If-None-Match") == nil)
        #expect(await sut.fetch() == .success(Self.release))
        #expect(transport.requests[1].value(forHTTPHeaderField: "If-None-Match") == "\"abc\"")
    }

    @Test func requestsCarryAcceptAndUserAgent() async {
        let transport = Transport([.success((200, [:], Self.body))])
        _ = await fetcher(transport).fetch()
        #expect(transport.requests[0].value(forHTTPHeaderField: "Accept") == "application/vnd.github+json")
        #expect(transport.requests[0].value(forHTTPHeaderField: "User-Agent") == ReleaseFetcher.userAgent)
    }

    @Test func okWithoutAnETagDoesNotCache() async {
        let transport = Transport([.success((200, [:], Self.body)), .success((200, [:], Self.body))])
        let sut = fetcher(transport)
        _ = await sut.fetch()
        _ = await sut.fetch()
        #expect(transport.requests[1].value(forHTTPHeaderField: "If-None-Match") == nil)
    }

    @Test func notModifiedWithoutACacheIsABadResponse() async {
        let sut = fetcher(Transport([.success((304, [:], Data()))]))
        #expect(await sut.fetch() == .failure(.badResponse))
    }

    @Test(arguments: [
        (403, ["x-ratelimit-remaining": "0"], UpdateFailure.rateLimited),
        (429, [:], .rateLimited),
        (403, ["x-ratelimit-remaining": "12"], .badResponse),
        (403, [:], .badResponse),
        (500, [:], .badResponse),
    ] as [(Int, [String: String], UpdateFailure)])
    func errorStatusMapping(status: Int, headers: [String: String], expected: UpdateFailure) async {
        let sut = fetcher(Transport([.success((status, headers, Data()))]))
        #expect(await sut.fetch() == .failure(expected))
    }

    @Test func malformedBodyIsABadResponse() async {
        let sut = fetcher(Transport([.success((200, [:], Data("nope".utf8)))]))
        #expect(await sut.fetch() == .failure(.badResponse))
    }

    @Test func transportErrorIsANetworkFailure() async {
        let sut = fetcher(Transport([.failure(URLError(.notConnectedToInternet))]))
        #expect(await sut.fetch() == .failure(.network))
    }
}

struct UpdateLogTests {
    private func makeLog(maxBytes: Int = 256 * 1024) -> UpdateLog {
        UpdateLog(
            directory: FileManager.default.temporaryDirectory
                .appendingPathComponent("t3bar-log-\(UUID().uuidString)", isDirectory: true)
                .appendingPathComponent("nested", isDirectory: true),
            maxBytes: maxBytes)
    }

    private func read(_ url: URL) -> String {
        (try? String(contentsOf: url, encoding: .utf8)) ?? ""
    }

    @Test func appendCreatesTheDirectoryAndAppendsTimestampedLines() {
        let log = makeLog()
        let date = Date(timeIntervalSince1970: 1_700_000_000)
        log.append("first", at: date)
        log.append("second", at: date)
        let lines = read(log.fileURL).split(separator: "\n")
        #expect(lines.count == 2)
        #expect(lines[0] == "2023-11-14T22:13:20Z first")
        #expect(lines[1].hasSuffix(" second"))
    }

    @Test func oversizedLogRotatesAndKeepsOnlyOnePreviousFile() {
        let log = makeLog(maxBytes: 100)
        log.append(String(repeating: "a", count: 150))
        log.append("fresh")
        #expect(read(log.rotatedFileURL).contains("aaaa"))
        let current = read(log.fileURL)
        #expect(current.contains("fresh"))
        #expect(!current.contains("aaaa"))

        log.append(String(repeating: "b", count: 150))
        log.append("again")
        #expect(read(log.rotatedFileURL).contains("bbbb"))
        #expect(!read(log.rotatedFileURL).contains("aaaa"))
    }

    @Test func installerHandleStartsARunHeaderAndAppends() throws {
        let log = makeLog()
        log.append("before")
        let handle = try #require(log.openForInstaller(tag: "v1.2.0"))
        try handle.write(contentsOf: Data("child output\n".utf8))
        try handle.close()
        let text = read(log.fileURL)
        #expect(text.contains("before"))
        #expect(text.contains("installer output for v1.2.0"))
        #expect(text.hasSuffix("child output\n"))
    }
}
