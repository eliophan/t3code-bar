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

    private let reader: LimitsReader
    private let clock: @Sendable () -> Date
    private var timer: Timer?
    private var refreshTask: Task<Void, Never>?

    public init(reader: LimitsReader = LimitsReader(cachesDirectory: EnvironmentConfig.shared.cachesDirectory),
                clock: @escaping @Sendable () -> Date = { Date() }) {
        self.reader = reader
        self.clock = clock
        now = clock()
    }

    public func start() {
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: Self.refreshInterval, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.refresh() }
        }
        refresh()
    }

    public func stop() {
        timer?.invalidate()
        timer = nil
        refreshTask?.cancel()
        refreshTask = nil
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
        } catch {
            result = .failure(error as? LimitsReadError ?? .unreadable)
        }
        guard !Task.isCancelled else { return }
        apply(result)
    }

    func apply(_ result: Result<LimitsSnapshot, LimitsReadError>) {
        now = clock()
        switch result {
        case .success(let fresh):
            if snapshot != fresh { snapshot = fresh }
            readError = nil
        case .failure(let error):
            readError = error
        }
    }
}
