import Foundation
import Combine

/// Keeps the Limits tab current while it is visible, and does nothing otherwise.
///
/// Like `UsageMonitor` it is separate from `ThreadMonitor`, so the one second thread poll carries no extra load.
/// Each tick lists the caches directory, and `LimitsReader` decodes only the files whose size or modification time changed.
/// A failed read keeps the last good snapshot.
@MainActor
public final class LimitsMonitor: ObservableObject {
    static let refreshInterval: TimeInterval = 5

    @Published public private(set) var snapshot: LimitsSnapshot?
    @Published public private(set) var readError: LimitsReadError?
    /// The time of the last tick. The view derives "resets in" and "checked ago" from it.
    @Published public private(set) var now: Date

    let live: LimitsLiveConnection
    private var cachedSnapshot: LimitsSnapshot?
    private var cachedRecords: [LimitsRecord] = []

    private let reader: LimitsReader
    private let clock: @Sendable () -> Date
    private var timer: Timer?
    private var refreshTask: Task<Void, Never>?
    private var didAttemptSavedReconnect = false

    public convenience init(reader: LimitsReader = LimitsReader(cachesDirectory: EnvironmentConfig.shared.cachesDirectory),
                            clock: @escaping @Sendable () -> Date = { Date() }) {
        self.init(reader: reader, live: LimitsLiveConnection(), clock: clock)
    }

    init(reader: LimitsReader, live: LimitsLiveConnection, clock: @escaping @Sendable () -> Date = { Date() }) {
        self.reader = reader
        self.live = live
        self.clock = clock
        now = clock()
        live.onChange = { [weak self] in self?.updateSnapshot() }
    }

    public func start() {
        if !didAttemptSavedReconnect {
            didAttemptSavedReconnect = true
            if let port = EnvironmentConfig.shared.serverRuntime?.port {
                live.reconnectSavedIfAvailable(port: port)
            }
        }
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: Self.refreshInterval, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.refresh() }
        }
        live.resume()
        refresh()
    }

    /// Stops cache polling when Usage leaves the screen. The app-level live connection stays up
    /// so the mandatory pairing remains valid while the popover is closed.
    public func stop() {
        timer?.invalidate()
        timer = nil
        refreshTask?.cancel()
        refreshTask = nil
    }

    public func shutdown() {
        live.stop()
        stop()
    }

    func refresh() {
        guard timer != nil, refreshTask == nil else { return }
        refreshTask = Task { [weak self] in
            await self?.readOnce()
            self?.refreshTask = nil
        }
    }

    func readOnce() async {
        let result: Result<LimitsSnapshot, LimitsReadError>
        do {
            result = .success(try await reader.read())
            cachedRecords = await reader.records()
        } catch {
            result = .failure(error as? LimitsReadError ?? .unreadable)
        }
        guard !Task.isCancelled else { return }
        apply(result)
    }

    private func updateSnapshot() {
        guard let data = live.data else { snapshot = cachedSnapshot; return }
        // Connected config is authoritative (including disabled/deleted providers). During an outage,
        // merge last live sources with newly written cache records, using T3's newest-check rule.
        let records = live.status == .live ? data.records : data.providers + cachedRecords + data.sources
        snapshot = LimitsMerge.snapshot(records, skippedFiles: cachedSnapshot?.skippedFiles ?? 0, sourceNotices: data.notices)
    }

    func apply(_ result: Result<LimitsSnapshot, LimitsReadError>) {
        now = clock()
        switch result {
        case .success(let fresh):
            cachedSnapshot = fresh
            updateSnapshot()
            readError = nil
        case .failure(let error):
            readError = error
        }
    }
}
