import Foundation
import Combine

/// Combines T3 Code's transcript cache with read-only local provider sources.
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
        let file: UsageCacheFile?
        let localRevision: Int
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
    private var localScanner = LocalUsageScanner()
    private var localSnapshot = LocalUsageSnapshot()
    private var localScannedAt: Date?
    private var localRevision = 0
    static let localRefreshInterval: TimeInterval = 30
    private(set) var cacheReads = 0

    init(source: UsageSource, clock: @escaping @Sendable () -> Date = Date.init, timeZone: TimeZone = .autoupdatingCurrent) {
        self.source = source
        self.clock = clock
        self.timeZone = timeZone
    }

    func report(for period: UsagePeriod, includeLocal: Bool = true) -> Outcome {
        let now = clock()
        var cacheFile: UsageCacheFile?
        var cacheMissing = false
        switch source.locate() {
        case .missing:
            loaded = nil
            cacheMissing = true
        case .unsupported(let version):
            release()
            return .unsupported(version)
        case .available(let file):
            cacheFile = file
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
        }
        if localScannedAt == nil || (includeLocal && (localScannedAt.map({ now.timeIntervalSince($0) >= Self.localRefreshInterval }) ?? true)) {
            localSnapshot = localScanner.scan(paths: source.localPaths, now: now)
            localScannedAt = now
            localRevision += 1
        }
        let stamp = source.priceStamp()
        if priced?.stamp != stamp { priced = (stamp, source.prices()) }
        let key = Key(file: cacheFile, localRevision: localRevision, prices: stamp, period: period, minute: Int(now.timeIntervalSince1970 / 60))
        if key == lastKey { return .unchanged }
        lastKey = key
        let dataset = localSnapshot.merging(loaded?.dataset)
        guard let book = priced?.book else { return .unreadable }
        if cacheMissing && dataset.files.isEmpty && !localSnapshot.statuses.contains(where: { $0.state == .unreadable || $0.state == .partial }) { return .missing }
        var report = UsageReportBuilder.build(dataset: dataset, window: period.window(now: now, timeZone: timeZone), prices: book)
        report.localSources = localSnapshot.statuses
        report.scanCacheMissing = cacheMissing
        return .report(report)
    }

    func prewarmLocal() {
        guard localScannedAt == nil else { return }
        let now = clock()
        localSnapshot = localScanner.scan(paths: source.localPaths, now: now)
        localScannedAt = now
        localRevision += 1
    }

    /// Drops the transcript cache but keeps the last local-provider snapshot.
    /// Reopening Usage can therefore render the complete report immediately instead of
    /// regressing to the transcript-only total while OpenCode and Antigravity are rescanned.
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

    /// Starts the local-provider scan before the user opens Usage, so the first visible
    /// headline already includes OpenCode and Antigravity instead of briefly showing cache-only data.
    public func prewarmLocal() {
        Task { [engine] in await engine.prewarmLocal() }
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
            let outcome = await engine.report(for: requested, includeLocal: false)
            guard !Task.isCancelled, let self else { return }
            self.refreshTask = nil
            // The period changed while this ran, so its answer is for a question nobody is asking.
            if requested == self.period {
                self.apply(outcome, for: requested)
                self.refreshLocal(for: requested)
            } else { self.refresh() }
        }
    }

    /// Local OpenCode and Antigravity discovery can take seconds on a large home directory.
    /// Publish the cache report first, then replace it with the complete local report.
    private func refreshLocal(for requested: UsagePeriod) {
        guard timer != nil, refreshTask == nil else { return }
        refreshTask = Task { [weak self, engine] in
            let outcome = await engine.report(for: requested, includeLocal: true)
            guard !Task.isCancelled, let self else { return }
            self.refreshTask = nil
            if requested == self.period { self.apply(outcome, for: requested) }
            else { self.refresh() }
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
