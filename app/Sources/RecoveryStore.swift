import Foundation
import Darwin

// `RecoveryStore.write` composes `DurableFile`'s primitives itself (`syncTree`, `writeTemporary`,
// `barrier`, `rename` under the gate, `persistDirectory`).

/// Recovery owns only this private tree. A completed immutable generation is
/// published by atomically replacing one small manifest. A failed replacement
/// leaves the prior checkpoint readable; at most two committed generations stay.
enum RecoveryStore {
    struct Generation: Codable {
        let folder: String
        let revision: UInt64
        let serial: UInt64
        let captured: Date
    }

    struct Manifest: Codable {
        let version: Int
        let id: String
        let title: String
        let sourceURL: String?
        let sourceType: String?
        let current: Generation
        let previous: Generation?
    }

    struct Candidate {
        let id: String
        let directory: URL
        let manifest: Manifest?
        let failure: String?

        var report: [String: Any] {
            var value: [String: Any] = ["recovery_id": id, "recoverable": manifest != nil]
            if let manifest {
                value["title"] = manifest.title
                value["revision"] = manifest.current.revision
                value["serial"] = manifest.current.serial
                value["captured_at"] = ISO8601DateFormatter().string(from: manifest.current.captured)
                value["generation"] = manifest.current.folder
                value["has_previous"] = manifest.previous != nil
                if let source = manifest.sourceURL {
                    value["source_url"] = source
                    if let url = URL(string: source), url.isFileURL {
                        value["source_exists"] = FileManager.default.fileExists(atPath: url.path)
                    }
                }
            }
            if let failure { value["error"] = failure }
            return value
        }
    }

    struct Captured {
        let id: String
        let title: String
        let sourceURL: String?
        let sourceType: String?
        let generation: Generation
        let previous: Manifest?
        let state: EditingStateArchive.Captured
        let snapshots: [NamedSnapshots.Archived]

        var scratchEstimate: UInt64 {
            snapshots.reduce(state.scratchEstimate) { max($0, $1.state.scratchEstimate) }
        }
    }

    struct Loaded {
        let manifest: Manifest
        let generation: Generation
        let state: EditingStateArchive.Decoded
        let snapshots: [NamedSnapshots.Loaded]
        let warning: String?
    }

    static func directory(root: URL, id: String) throws -> URL {
        guard UUID(uuidString: id) != nil else { throw failure("Invalid recovery identifier.") }
        return root.appendingPathComponent(id, isDirectory: true)
    }

    static func readManifest(at directory: URL) throws -> Manifest {
        let data = try EditingStateArchive.readFile(directory.appendingPathComponent("manifest.json"), maximum: 64 * 1024)
        let manifest = try JSONDecoder().decode(Manifest.self, from: data)
        guard manifest.version == 1, manifest.id == directory.lastPathComponent,
              UUID(uuidString: manifest.id) != nil,
              !manifest.title.isEmpty, manifest.title.utf8.count <= 4096 else {
            throw failure("Unsupported or damaged recovery manifest.")
        }
        for generation in [manifest.current, manifest.previous].compactMap({ $0 }) {
            guard UUID(uuidString: generation.folder) != nil,
                  generation.captured.timeIntervalSince1970.isFinite else {
                throw failure("Invalid recovery generation.")
            }
        }
        return manifest
    }

    static func discover(root: URL) throws -> [Candidate] {
        guard FileManager.default.fileExists(atPath: root.path) else { return [] }
        let directories = try FileManager.default.contentsOfDirectory(
            at: root, includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey], options: [.skipsHiddenFiles])
        var found: [Candidate] = []
        for directory in directories where UUID(uuidString: directory.lastPathComponent) != nil {
            do {
                let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                guard values.isDirectory == true, values.isSymbolicLink != true else { continue }
                // Held by an open document in any Doctored instance. The OS
                // drops the lock on forced termination, without signalling PIDs.
                guard let lease = try RecoveryLease.acquire(directory: directory, create: false) else { continue }
                defer { lease.release() }
                found.append(Candidate(id: directory.lastPathComponent, directory: directory,
                                       manifest: try readManifest(at: directory), failure: nil))
            } catch {
                // A failed first checkpoint may leave a directory without its
                // lease or manifest. Report that entry without hiding complete
                // checkpoints beside it on a full or read-only volume.
                found.append(Candidate(id: directory.lastPathComponent, directory: directory,
                                       manifest: nil, failure: error.localizedDescription))
            }
        }
        return found.sorted { ($0.manifest?.current.captured ?? .distantPast) > ($1.manifest?.current.captured ?? .distantPast) }
    }

    static func write(_ captured: Captured, directory: URL, gate: RecoveryPublicationGate) throws -> Manifest? {
        guard gate.isOpen else { return nil }
        try RecoveryCapacity.validate(captured)
        let files = FileManager.default
        let staged = directory.appendingPathComponent(".staging-\(UUID().uuidString)", isDirectory: true)
        let generation = directory.appendingPathComponent(captured.generation.folder, isDirectory: true)
        let manifestURL = directory.appendingPathComponent("manifest.json")
        var temporary: URL?
        var published = false
        defer {
            try? files.removeItem(at: staged)
            if !published {
                if let temporary { try? files.removeItem(at: temporary) }
                try? files.removeItem(at: generation)
            }
        }
        try EditingStateArchive.write(captured.state, to: staged)
        try NamedSnapshots.writeArchive(captured.snapshots, to: staged)
        // The generation's bytes reach the disk before the manifest names
        // it; the barrier orders the move and the new manifest before the
        // publishing rename, and the directory flush makes that rename durable.
        try DurableFile.syncTree(staged)
        guard gate.isOpen else { return nil }
        try files.moveItem(at: staged, to: generation)
        let manifest = Manifest(version: 1, id: captured.id, title: captured.title,
            sourceURL: captured.sourceURL, sourceType: captured.sourceType,
            current: captured.generation, previous: captured.previous?.current)
        let data = try JSONEncoder().encode(manifest)
        let written = try DurableFile.writeTemporary(data, beside: manifestURL, prefix: ".manifest-")
        temporary = written
        try DurableFile.barrier(directory)
        // The gate's lock holds only rename(2), never a flush.
        guard try gate.publish({
            guard rename(written.path, manifestURL.path) == 0 else {
                throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
            }
        }) else { return nil }
        published = true
        DurableFile.persistDirectory(directory)
        if captured.previous == nil {
            // A first checkpoint also created this recovery's directory, and
            // possibly the recovery root itself.
            let root = directory.deletingLastPathComponent()
            DurableFile.persistDirectory(root)
            DurableFile.persistDirectory(root.deletingLastPathComponent())
        }
        let keep = Set([manifest.current.folder, manifest.previous?.folder].compactMap { $0 })
        // Cleanup follows the commit. Its failure cannot invalidate that commit.
        if let contents = try? files.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) {
            for url in contents {
                let name = url.lastPathComponent
                let staleGeneration = UUID(uuidString: name) != nil && !keep.contains(name)
                guard staleGeneration || name.hasPrefix(".staging-") || name.hasPrefix(".manifest-")
                else { continue }
                try? files.removeItem(at: url)
            }
        }
        return manifest
    }

    static func read(_ candidate: Candidate) throws -> Loaded {
        let manifest = try readManifest(at: candidate.directory)
        func decode(_ generation: Generation, warning: String?) throws -> Loaded {
            let directory = candidate.directory.appendingPathComponent(generation.folder, isDirectory: true)
            let state = try EditingStateArchive.read(from: directory)
            let snapshots = try NamedSnapshots.readArchive(from: directory)
            return Loaded(manifest: manifest, generation: generation, state: state,
                          snapshots: snapshots, warning: warning)
        }
        do { return try decode(manifest.current, warning: nil) }
        catch {
            guard let previous = manifest.previous else { throw error }
            return try decode(previous, warning: "The latest recovery copy could not be read. The previous checkpoint from \(ISO8601DateFormatter().string(from: previous.captured)) was recovered. \(error.localizedDescription)")
        }
    }

    static func discard(directory: URL) throws {
        if FileManager.default.fileExists(atPath: directory.path) {
            try FileManager.default.removeItem(at: directory)
        }
    }

    private static func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }
}

/// A process-owned lease prevents two app instances from claiming or discarding
/// one recovery. Opening a lease never truncates it; close/crash releases flock.
final class RecoveryLease {
    private var descriptor: Int32
    private init(_ descriptor: Int32) { self.descriptor = descriptor }
    deinit { release() }

    static func acquire(directory: URL, create: Bool) throws -> RecoveryLease? {
        let files = FileManager.default
        if create { try files.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700]) }
        let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard values.isDirectory == true, values.isSymbolicLink != true else {
            throw DoctoredCoreError(message: "Recovery location is not a private directory.")
        }
        let descriptor = open(directory.appendingPathComponent("lease").path, O_RDWR | O_CREAT | O_NOFOLLOW, 0o600)
        guard descriptor >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        if flock(descriptor, LOCK_EX | LOCK_NB) != 0 {
            let code = errno
            Darwin.close(descriptor)
            if code == EWOULDBLOCK { return nil }
            throw POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)
        }
        return RecoveryLease(descriptor)
    }

    func release() {
        guard descriptor >= 0 else { return }
        _ = flock(descriptor, LOCK_UN)
        Darwin.close(descriptor)
        descriptor = -1
    }
}

/// Invalidation serializes against the tiny manifest publication only. Closing
/// a document never waits for native encoding, and a late worker cannot recreate
/// a recovery the user has already saved or deliberately discarded.
final class RecoveryPublicationGate {
    private let lock = NSLock()
    private var open = true
    var isOpen: Bool { lock.lock(); defer { lock.unlock() }; return open }
    func invalidate() { lock.lock(); open = false; lock.unlock() }
    func publish(_ write: () throws -> Void) throws -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard open else { return false }
        try write()
        return true
    }
}
