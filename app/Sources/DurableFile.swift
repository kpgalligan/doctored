import Foundation
import Darwin

// `publish` takes a staged file or package: `syncTree` a package, `F_BARRIERFSYNC` barrier,
// replace/move, then a best-effort `F_FULLFSYNC` of the directory.
//
// `write` replaces `Data.write(.atomic)` for preference, library and MCP-artifact files (atomic
// write plus the directory flush).
//
// This file uses Foundation and Darwin only, no other app type: the receipt harness compiles it
// standalone.

/// The app's one flush policy for published files, the twin of the
/// core's `durable_file.rs`. A staged file or package is BARRIER-flushed
/// before the rename that publishes it, so the rename can never reach the
/// disk ahead of its contents; the destination's directory is then FULLY
/// flushed, so the rename itself survives power loss. `F_BARRIERFSYNC` and
/// `F_FULLFSYNC` can fail on a volume that does not implement them (SMB,
/// FUSE, exFAT); the fallback is SQLite's, a plain `fsync` whose error is the
/// one reported. Foundation and Darwin only: it runs on `BackgroundWork`
/// workers and on main, and a standalone harness compiles it alone.
enum DurableFile {
    /// Barrier-flushes a file or directory, refusing a symlink (ELOOP).
    static func barrier(_ url: URL) throws {
        let descriptor = open(url.path, O_RDONLY | O_NOFOLLOW)
        guard descriptor >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        defer { Darwin.close(descriptor) }
        let code = flush(descriptor, F_BARRIERFSYNC)
        guard code == 0 else { throw POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO) }
    }

    /// Fully flushes a directory after a rename into it. Best effort: the
    /// rename has already published, so a failure is logged, never thrown.
    static func persistDirectory(_ directory: URL) {
        let descriptor = open(directory.path, O_RDONLY | O_DIRECTORY)
        guard descriptor >= 0 else {
            log(directory, errno)
            return
        }
        defer { Darwin.close(descriptor) }
        let code = flush(descriptor, F_FULLFSYNC)
        if code != 0 { log(directory, code) }
    }

    /// Plain `fsync` of every regular file and directory under `root`,
    /// children before parents and `root` last, so a later barrier on `root`
    /// orders the whole tree. A symlink below `root` is skipped (its parent's
    /// fsync covers the entry); `root` itself as a symlink refuses with ELOOP.
    static func syncTree(_ root: URL) throws {
        try syncChildren(of: root.path)
        try sync(root.path)
    }

    /// Writes `data` to a fresh `<prefix><UUID>` beside `destination`.
    static func writeTemporary(_ data: Data, beside destination: URL,
                               prefix: String) throws -> URL {
        let temporary = destination.deletingLastPathComponent()
            .appendingPathComponent(prefix + UUID().uuidString)
        do {
            try data.write(to: temporary, options: .withoutOverwriting)
        } catch {
            try? FileManager.default.removeItem(at: temporary)
            throw error
        }
        return temporary
    }

    /// Publishes a staged file or package over `destination`: sync a package's
    /// tree, barrier the staged item, replace or move it into place, then
    /// flush the result (logged) and its directory (best effort).
    static func publish(_ staged: URL, to destination: URL) throws {
        if kind(of: staged.path) == S_IFDIR { try syncTree(staged) }
        try barrier(staged)
        let files = FileManager.default
        let result: URL
        if files.fileExists(atPath: destination.path) {
            result = try files.replaceItemAt(destination, withItemAt: staged) ?? destination
        } else {
            try files.moveItem(at: staged, to: destination)
            result = destination
        }
        do {
            try sync(result.path)
        } catch {
            NSLog("DurableFile: could not flush %@: %@", result.path, error.localizedDescription)
        }
        persistDirectory(destination.deletingLastPathComponent())
    }

    /// An `.atomic` write whose rename is then made durable by a full
    /// flush of its directory. Foundation already fsyncs the contents.
    static func write(_ data: Data, to url: URL) throws {
        try data.write(to: url, options: .atomic)
        persistDirectory(url.deletingLastPathComponent())
    }

    private static func syncChildren(of directory: String) throws {
        guard try kindThrowing(of: directory) == S_IFDIR else { return }
        for name in try FileManager.default.contentsOfDirectory(atPath: directory) {
            let path = (directory as NSString).appendingPathComponent(name)
            switch try kindThrowing(of: path) {
            case S_IFDIR:
                try syncChildren(of: path)
                try sync(path)
            case S_IFREG:
                try sync(path)
            default:
                continue
            }
        }
    }

    private static func sync(_ path: String) throws {
        let descriptor = open(path, O_RDONLY | O_NOFOLLOW)
        guard descriptor >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        defer { Darwin.close(descriptor) }
        while fsync(descriptor) != 0 {
            let code = errno
            if code != EINTR { throw POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO) }
        }
    }

    private static func kind(of path: String) -> mode_t? {
        var info = stat()
        guard lstat(path, &info) == 0 else { return nil }
        return info.st_mode & S_IFMT
    }

    private static func kindThrowing(of path: String) throws -> mode_t {
        guard let kind = kind(of: path) else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        return kind
    }

    /// `fcntl(descriptor, command)`, retried on EINTR; any other failure falls
    /// back to `fsync` (also retried on EINTR). Returns 0 or fsync's errno.
    private static func flush(_ descriptor: Int32, _ command: Int32) -> Int32 {
        while fcntl(descriptor, command) == -1 {
            if errno == EINTR { continue }
            while fsync(descriptor) != 0 {
                let code = errno
                if code != EINTR { return code }
            }
            return 0
        }
        return 0
    }

    private static func log(_ directory: URL, _ code: Int32) {
        NSLog("DurableFile: could not flush %@: %@", directory.path,
              String(cString: strerror(code)))
    }
}
