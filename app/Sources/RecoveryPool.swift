import Foundation

/// One recovery folder's tile pool (13C-3): every generation's states and session snapshots
/// are written as skeletons plus payloads in `<recovery>/pool`, so a checkpoint after a small
/// edit appends only the tiles it changed. The box holds the open handle (and with it the
/// core's memo of the tiles already keyed) from one checkpoint to the next.
///
/// A record keeps one box. The handle is dropped on every outcome but a published manifest,
/// so an in-memory index never outlives a write that failed half way, and `clear` gives the
/// record a NEW box: a late worker that `clear` overtook stores into the abandoned one, never
/// into the record's. The next checkpoint reopens the pool from disk.
final class RecoveryPoolBox {
    static let folderName = "pool"
    private let lock = NSLock()
    private var pool: RasterTilePool?
    private var info: [String: Any]?

    /// The handle on `<recovery>/pool`, opened (and the folder made, one level) on first use.
    func open(recovery directory: URL) throws -> RasterTilePool {
        lock.lock(); defer { lock.unlock() }
        if let pool { return pool }
        let opened = try RasterTilePool(directory: directory.appendingPathComponent(Self.folderName, isDirectory: true))
        pool = opened
        return opened
    }

    /// Forgets the handle; the next `open` reads the pool from disk.
    func drop() {
        lock.lock(); defer { lock.unlock() }
        pool = nil
    }

    /// The pool's report after the last published write (`write_recovery`'s `pool`).
    var lastInfo: [String: Any]? {
        lock.lock(); defer { lock.unlock() }
        return info
    }

    func record(_ report: [String: Any]?) {
        lock.lock(); defer { lock.unlock() }
        info = report
    }

    /// Keeps only the payloads the kept generations name: every `image.drskel` under them
    /// (states and session snapshots alike, found by walking the folders, so none can be
    /// forgotten). An enumeration error skips the collection; a collection failure never fails
    /// the checkpoint it follows, but drops the handle.
    func collect(_ pool: RasterTilePool, recovery directory: URL, keeping folders: Set<String>) {
        var skeletons: [URL] = []
        var failed = false
        for folder in folders.sorted() {
            let root = directory.appendingPathComponent(folder, isDirectory: true)
            guard let walk = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil,
                                                            options: [], errorHandler: { _, _ in
                                                                failed = true
                                                                return false
                                                            }) else { return }
            for case let url as URL in walk where url.lastPathComponent == EditingStateArchive.skeletonName {
                skeletons.append(url)
            }
        }
        guard !failed else { return }
        do { try pool.collect(keeping: skeletons) } catch { drop() }
    }
}
