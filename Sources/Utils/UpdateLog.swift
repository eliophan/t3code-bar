import Foundation

/// A plain-text log that stays on this machine. Callers must pass only
/// event names and exit codes, never tokens or database paths, because users
/// may share the file.
public struct UpdateLog: Sendable {
    public let directory: URL
    public let maxBytes: Int

    public init(directory: URL, maxBytes: Int = 256 * 1024) {
        self.directory = directory
        self.maxBytes = maxBytes
    }

    public static var standard: UpdateLog {
        UpdateLog(directory: FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Logs/T3 Code Bar", isDirectory: true))
    }

    public var fileURL: URL { directory.appendingPathComponent("update.log") }
    public var rotatedFileURL: URL { directory.appendingPathComponent("update.log.1") }

    public func append(_ line: String, at date: Date = Date()) {
        guard let handle = openForAppend() else { return }
        defer { try? handle.close() }
        try? handle.write(contentsOf: Data("\(date.formatted(.iso8601)) \(line)\n".utf8))
    }

    /// The installer's stdout and stderr go to this handle. The caller closes it.
    public func openForInstaller(tag: String, at date: Date = Date()) -> FileHandle? {
        guard let handle = openForAppend() else { return nil }
        try? handle.write(contentsOf: Data("\(date.formatted(.iso8601)) installer output for \(tag)\n".utf8))
        return handle
    }

    /// O_APPEND keeps the app's lines and the child's output from overwriting
    /// each other while both hold the file open.
    private func openForAppend() -> FileHandle? {
        let fileManager = FileManager.default
        do { try fileManager.createDirectory(at: directory, withIntermediateDirectories: true) } catch { return nil }
        rotateIfNeeded(fileManager)
        let descriptor = open(fileURL.path, O_WRONLY | O_APPEND | O_CREAT, 0o600)
        guard descriptor >= 0 else { return nil }
        return FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
    }

    private func rotateIfNeeded(_ fileManager: FileManager) {
        let size = (try? fileManager.attributesOfItem(atPath: fileURL.path)[.size] as? Int) ?? 0
        guard size > maxBytes else { return }
        try? fileManager.removeItem(at: rotatedFileURL)
        try? fileManager.moveItem(at: fileURL, to: rotatedFileURL)
    }
}
