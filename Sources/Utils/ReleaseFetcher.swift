import Foundation

/// Keeps the last ETag and release in memory only. GitHub does not count a
/// 304 answer against the unauthenticated rate limit.
public actor ReleaseFetcher {
    public typealias Transport = @Sendable (URLRequest) async throws -> (Data, URLResponse)

    public static let userAgent = "T3-Code-Bar-Updater (+https://github.com/\(AppUpdatePolicy.repository))"

    private let url: URL
    private let transport: Transport
    private var cache: (etag: String, release: ReleaseInfo)?

    public init(
        url: URL = URL(string: "https://api.github.com/repos/\(AppUpdatePolicy.repository)/releases/latest")!,
        transport: @escaping Transport = { try await URLSession.shared.data(for: $0) }
    ) {
        self.url = url
        self.transport = transport
    }

    public func fetch() async -> Result<ReleaseInfo, UpdateFailure> {
        var request = URLRequest(url: url)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue(Self.userAgent, forHTTPHeaderField: "User-Agent")
        if let cache { request.setValue(cache.etag, forHTTPHeaderField: "If-None-Match") }

        let data: Data
        let response: URLResponse
        do { (data, response) = try await transport(request) } catch { return .failure(.network) }
        guard let http = response as? HTTPURLResponse else { return .failure(.badResponse) }

        switch http.statusCode {
        case 200:
            guard let release = AppUpdatePolicy.release(fromReleaseJSON: data) else { return .failure(.badResponse) }
            cache = http.value(forHTTPHeaderField: "ETag").map { ($0, release) }
            return .success(release)
        case 304:
            guard let cache else { return .failure(.badResponse) }
            return .success(cache.release)
        case 429:
            return .failure(.rateLimited)
        case 403 where http.value(forHTTPHeaderField: "x-ratelimit-remaining") == "0":
            return .failure(.rateLimited)
        default:
            return .failure(.badResponse)
        }
    }
}
