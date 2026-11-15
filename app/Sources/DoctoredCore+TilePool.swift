import Foundation

/// The core's content-addressed tile pool (13C-1, `doc_tile_pool`): a
/// directory of packs and an index that recovery writes every state into
/// (13C-3, `RecoveryPool.swift`). The handle owns files on disk and a memo of
/// the tiles it keyed; the core serializes calls on it, so it may be used from
/// the recovery worker.
final class RasterTilePool {
    let ptr: OpaquePointer
    /// The pool's own directory (`<recovery>/pool`).
    let directory: URL

    /// Opens the pool, making `directory` (one level, never its parents).
    init(directory: URL) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_tile_pool_open(directory.path, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not open the recovery tile pool."))
        }
        ptr = handle
        self.directory = directory
    }

    deinit { dr_tile_pool_free(ptr) }

    /// The fixed transient a pool write or rebuild holds beside the document's own plan.
    static var windowBytes: UInt64 { dr_tile_pool_window_bytes() }

    /// Writes `document` into the pool and its skeleton to `skeleton`, atomically, after the
    /// pool's new payloads are durable. A typed document is admitted like its native save.
    func write(_ document: RasterDocument, skeleton: URL, control: RasterJobControl? = nil) throws {
        var error: UnsafeMutablePointer<CChar>?
        let written: Bool
        if let precision = document.precision {
            let context = try PrecisionWorkContext(source: precision)
            written = dr_precision_doc_tile_pool_write(precision.ptr, ptr, skeleton.path, context.limit,
                                                       context.otherRetained, control?.ptr, &error)
        } else {
            written = dr_doc_tile_pool_write(document.ptr, ptr, skeleton.path, control?.ptr, &error)
        }
        guard written else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not write the recovery tile pool."))
        }
    }

    /// Writes the native file `skeleton` names to `native`, atomically.
    func rebuild(skeleton: URL, to native: URL, control: RasterJobControl? = nil) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_tile_pool_rebuild(ptr, skeleton.path, native.path, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not read the recovery tile pool."))
        }
    }

    /// Keeps only the payloads `skeletons` name; refuses, removing nothing, when one cannot be read.
    func collect(keeping skeletons: [URL]) throws {
        let copies = skeletons.map { strdup($0.path) }
        defer { copies.forEach { free($0) } }
        let pointers = copies.map { $0.map { UnsafePointer($0) } }
        var error: UnsafeMutablePointer<CChar>?
        let collected = pointers.withUnsafeBufferPointer {
            dr_tile_pool_collect(ptr, $0.baseAddress, $0.count, &error)
        }
        guard collected else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not collect the recovery tile pool."))
        }
    }

    /// The pool's counts and the last write's (`dr_tile_pool_info`).
    func info() throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        guard let text = dr_tile_pool_info(ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not read the recovery tile pool."))
        }
        defer { dr_string_free(text) }
        guard let value = try JSONSerialization.jsonObject(with: Data(String(cString: text).utf8)) as? [String: Any] else {
            throw DoctoredCoreError(message: "The recovery tile pool's report is not an object.")
        }
        return value
    }
}
