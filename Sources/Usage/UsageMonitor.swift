import Foundation
import Combine

/// Decodes T3 Code's scan cache once per change and answers each period from the decoded records.
///
/// It runs off the main actor, so the parse (a fraction of a second for a 20 MB cache) never blocks the UI.
actor UsageEngine {
    enum Outcome: Equatable, Sendable {
        case report(UsageReport)
        /// Nothing changed since the last answer, so the caller keeps what it shows.
        case unchanged
        case missing
        case unsupported(Int?)
        /// The cache could not be decoded, which is normal for a moment while T3 Code rewrites it.
        case unreadable
    }

    private struct Key: Equatable {
        let file: UsageCacheFile
        let prices: UsageSource.PriceStamp
        let period: UsagePeriod
        let minute: Int
    }

    private let source: UsageSource
    private let clock: @Sendable () -> Date
    private let timeZone: TimeZone
    private var loaded: (file: UsageCacheFile, dataset: UsageDataset)?
    private var priced: (stamp: UsageSource.PriceStamp, book: PriceBook)?
    private var lastKey: Key?
    private(set) var cacheReads = 0

    init(source: UsageSource, clock: @escaping @Sendable () -> Date = Date.init, timeZone: TimeZone = .autoupdatingCurrent) {
        self.source = source
        self.clock = clock
        self.timeZone = timeZone
    }

    func report(for period: UsagePeriod) -> Outcome {
        switch source.locate() {
        case .missing:
            release()
            return .missing
        case .unsupported(let version):
            release()
            return .unsupported(version)
        case .available(let file):
            if loaded?.file != file {
                do {
                    cacheReads += 1
                    loaded = (file, try source.dataset(file))
                } catch UsageCacheError.unsupportedVersion(let version) {
                    release()
                    return .unsupported(version)
                } catch {
                    return .unreadable
                }
            }
            let stamp = source.priceStamp()
            if priced?.stamp != stamp { priced = (stamp, source.prices()) }

            let now = clock()
            let key = Key(file: file, prices: stamp, period: period, minute: Int(now.timeIntervalSince1970 / 60))
            if key == lastKey { return .unchanged }
            lastKey = key
            guard let dataset = loaded?.dataset, let book = priced?.book else { return .unreadable }
            return .report(UsageReportBuilder.build(dataset: dataset, window: period.window(now: now, timeZone: timeZone), prices: book))
        }
    }

    /// Drops the decoded records, which are the only large thing held.
    func release() {
        loaded = nil
        lastKey = nil
    }
}

public enum UsageNotice: Equatable, Sendable {
    /// T3 Code has never scanned, so there is no cache.
    case missing
    case unsupported(Int?)
    /// The cache could not be read this time. Numbers shown, if any, are from the last good read.
    case retrying
}

/// Reads token usage while the Usage screen is visible, and not at all otherwise.
///
/// It is separate from `ThreadMonitor` so the one second thread poll carries no extra load. It checks the
/// cache's size and modification time about every five seconds, decodes it only when they change, and
/// builds a period's report from the decoded records. A failed read keeps the last good report.
@MainActor
public final class UsageMonitor: ObservableObject {
    static let refreshInterval: TimeInterval = 5
    static let periodKey = "usagePeriodDays"

    @Published public private(set) var report: UsageReport?
    /// The period `report` was built for.
    @Published public private(set) var reportPeriod: UsagePeriod?
    @Published public private(set) var notice: UsageNotice?
    @Published public var period: UsagePeriod {
        didSet {
            guard period != oldValue else { return }
            defaults.set(period.rawValue, forKey: Self.periodKey)
            refresh()
        }
    }

    /// True while the shown report belongs to another period than the selected one.
    public var isStale: Bool { reportPeriod.map { $0 != period } ?? false }

    private let engine: UsageEngine
    private let defaults: UserDefaults
    private var timer: Timer?
    private var refreshTask: Task<Void, Never>?

    public convenience init(source: UsageSource = UsageSource(), defaults: UserDefaults = .standard) {
        self.init(engine: UsageEngine(source: source), defaults: defaults)
    }

    init(engine: UsageEngine, defaults: UserDefaults) {
        self.engine = engine
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
        Task { [engine] in await engine.release() }
    }

    func refresh() {
        guard timer != nil, refreshTask == nil else { return }
        let requested = period
        refreshTask = Task { [weak self, engine] in
            let outcome = await engine.report(for: requested)
            guard !Task.isCancelled, let self else { return }
            self.refreshTask = nil
            // The period changed while this ran, so its answer is for a question nobody is asking.
            if requested == self.period { self.apply(outcome, for: requested) } else { self.refresh() }
        }
    }

    func apply(_ outcome: UsageEngine.Outcome, for period: UsagePeriod) {
        switch outcome {
        case .report(let fresh):
            report = fresh
            reportPeriod = period
            notice = nil
        case .unchanged:
            if notice == .retrying { notice = nil }
        case .missing:
            report = nil
            reportPeriod = nil
            notice = .missing
        case .unsupported(let version):
            report = nil
            reportPeriod = nil
            notice = .unsupported(version)
        case .unreadable:
            notice = .retrying
        }
    }
}
