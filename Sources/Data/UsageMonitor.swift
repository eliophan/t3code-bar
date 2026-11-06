import Foundation
import Combine

/// Reads token usage while the Usage screen is visible, and not at all otherwise.
///
/// It is separate from `ThreadMonitor` so the one second thread poll carries no extra load,
/// and it keeps the same rule: one read at a time, and a failed read keeps the last good snapshot.
@MainActor
public final class UsageMonitor: ObservableObject {
    static let refreshInterval: TimeInterval = 5
    static let periodKey = "usagePeriodDays"

    @Published public private(set) var snapshot: UsageSnapshot?
    @Published public private(set) var readError: DatabaseReadError?
    @Published public var period: UsagePeriod {
        didSet {
            guard period != oldValue else { return }
            defaults.set(period.rawValue, forKey: Self.periodKey)
            refresh()
        }
    }

    /// True once the shown snapshot belongs to another period than the selected one.
    public var isStale: Bool { snapshot.map { $0.period != period } ?? false }

    private let database: DatabaseManager
    private let defaults: UserDefaults
    private var timer: Timer?
    private var refreshTask: Task<Void, Never>?

    public init(database: DatabaseManager = .shared, defaults: UserDefaults = .standard) {
        self.database = database
        self.defaults = defaults
        period = UsagePeriod(rawValue: defaults.integer(forKey: Self.periodKey)) ?? .default
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
        let requested = period
        refreshTask = Task { [weak self, database] in
            let result: Result<UsageSnapshot, DatabaseReadError>
            do {
                result = .success(try await database.usage(period: requested))
            } catch {
                result = .failure(error as? DatabaseReadError ?? .unavailable)
            }
            guard !Task.isCancelled, let self else { return }
            self.refreshTask = nil
            // The period changed while this read ran, so its answer is for a question nobody is asking.
            if requested == self.period { self.apply(result) } else { self.refresh() }
        }
    }

    func apply(_ result: Result<UsageSnapshot, DatabaseReadError>) {
        switch result {
        case .success(let fresh):
            snapshot = fresh
            readError = nil
        case .failure(let error):
            readError = error
        }
    }
}
