import AppKit

// Named captures last for the document session; explicit `.drsnapshot` files persist independently
// of ordinary `.rz` saves. Recovery also stores the list. Shared allocation accounting covers the
// list; creation refuses excess retention, existing named captures stay pinned until
// removed/closed.

extension Notification.Name {
    static let imageDocumentSnapshotsDidChange = Notification.Name("ImageDocumentSnapshotsDidChange")
}

/// Named full editing states. Explicit snapshot packages are portable; ordinary
/// native saves do not embed this session list or a recursive history graph.
final class NamedSnapshots {
    struct Entry {
        let id: String
        var name: String
        let created: Date
        let state: EditingState
    }

    struct Header: Codable {
        let version: Int
        let name: String
        let created: Date
    }

    struct Archived {
        let header: Header
        let state: EditingStateArchive.Captured
    }

    struct Loaded {
        let header: Header
        let state: EditingStateArchive.Decoded
    }

    private weak var document: ImageDocument?
    private(set) var entries: [Entry] = []
    let limit = RasterResources.shared.workingLimit / 4
    static let maximumCount = 32

    init(document: ImageDocument) { self.document = document }

    @discardableResult
    func create(name: String) throws -> Entry {
        guard let document, !document.isLiveEditing, let state = document.captureEditingState() else {
            throw failure("Finish the current gesture before creating a snapshot.")
        }
        return try insert(name: name, created: Date(), state: state)
    }

    func rename(id: String, name: String) throws {
        let name = try validName(name)
        guard let index = entries.firstIndex(where: { $0.id == id }) else { throw failure("Unknown snapshot.") }
        entries[index].name = name
        changed()
    }

    func delete(id: String) throws {
        guard let index = entries.firstIndex(where: { $0.id == id }) else { throw failure("Unknown snapshot.") }
        entries.remove(at: index)
        changed()
    }

    func restore(id: String) throws {
        guard let document, !document.isLiveEditing, !ActionPlayer.isPlaying,
              let entry = entries.first(where: { $0.id == id }) else {
            throw failure("Choose a snapshot after finishing the current gesture.")
        }
        document.restoreEditingState(entry.state, actionName: "Restore “\(entry.name)”")
    }

    func save(id: String, to url: URL) throws {
        guard let entry = entries.first(where: { $0.id == id }) else { throw failure("Unknown snapshot.") }
        let snapshot = SideFileExtension.snapshot
        guard snapshot.matches(url) else {
            throw failure("Snapshot files use the .\(snapshot.current) extension "
                + "(or the legacy .\(snapshot.legacy)).")
        }
        let archive = Archived(header: Header(version: 1, name: entry.name, created: entry.created),
                               state: try EditingStateArchive.capture(entry.state))
        _ = try RasterResources.shared.admit(scratch: archive.state.scratchEstimate,
            own: archive.state.ownRetainedBytes, remedy: archive.state.refusalRemedy)
        let task = try WorkerTask(scratchEstimate: archive.state.scratchEstimate) {
            let staged = url.deletingLastPathComponent().appendingPathComponent(".snapshot-\(UUID().uuidString)")
            defer { try? FileManager.default.removeItem(at: staged) }
            try Self.write(archive, to: staged)
            try DurableFile.publish(staged, to: url)
        }
        try BackgroundWork.wait(task, title: "Saving Snapshot")
    }

    @discardableResult
    func load(from url: URL) throws -> Entry {
        guard let document, !document.isLiveEditing else { throw failure("Finish the current gesture first.") }
        let task = try WorkerTask { try Self.read(from: url) }
        let loaded = try BackgroundWork.wait(task, title: "Loading Snapshot")
        return try insert(name: loaded.header.name, created: loaded.header.created,
                          state: loaded.state.editingState())
    }

    func report() throws -> [String: Any] {
        let states = entries.map(\.state)
        let baseline = document?.captureEditingState().map { [$0] } ?? []
        let total = try EditingState.retainedBytes(in: states + baseline)
        let current = try EditingState.retainedBytes(in: baseline)
        return [
            "snapshots": entries.map { ["snapshot_id": $0.id, "name": $0.name,
                "created": ISO8601DateFormatter().string(from: $0.created),
                "width": $0.state.document.width, "height": $0.state.document.height] as [String: Any] },
            "retained_bytes": try EditingState.retainedBytes(in: states),
            "incremental_bytes": total >= current ? total - current : 0,
            "over_budget": total > current && total - current > limit,
            "budget_bytes": limit, "maximum_count": Self.maximumCount,
            "budget_policy": "Existing named captures remain retained if later edits exceed the budget; new captures are refused until space is released.",
            "persistence": "Session snapshots; Save Snapshot writes a portable .drsnapshot package. Ordinary .doctored saves do not embed the list.",
        ]
    }

    func captureArchive() throws -> [Archived] {
        try entries.map { Archived(header: Header(version: 1, name: $0.name, created: $0.created),
                                   state: try EditingStateArchive.capture($0.state)) }
    }

    static func writeArchive(_ archives: [Archived], to directory: URL) throws {
        let root = directory.appendingPathComponent("snapshots")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        for (index, archive) in archives.enumerated() {
            try write(archive, to: root.appendingPathComponent(String(index)))
        }
        try JSONEncoder().encode(archives.count).write(to: root.appendingPathComponent("count.json"))
    }

    static func readArchive(from directory: URL) throws -> [Loaded] {
        let root = directory.appendingPathComponent("snapshots")
        let values = try root.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard values.isDirectory == true, values.isSymbolicLink != true else {
            throw failure("Recovery snapshot list is missing or damaged.")
        }
        let count = try JSONDecoder().decode(Int.self, from: EditingStateArchive.readFile(
            root.appendingPathComponent("count.json"), maximum: 128))
        guard (0...maximumCount).contains(count) else { throw failure("Invalid recovery snapshot count.") }
        return try (0..<count).map { try read(from: root.appendingPathComponent(String($0))) }
    }

    func installArchive(_ archives: [Loaded]) throws {
        // Validate the whole list before replacing anything. Recovery restores
        // the captured list even if a future machine has a smaller budget.
        let loaded = try archives.map { archive in
            Entry(id: UUID().uuidString, name: try validName(archive.header.name),
                  created: archive.header.created, state: try archive.state.editingState())
        }
        entries = loaded
        changed()
    }

    private func insert(name: String, created: Date, state: EditingState) throws -> Entry {
        let name = try validName(name)
        guard entries.count < Self.maximumCount else { throw failure("Keep at most 32 session snapshots. Save and remove one first.") }
        let baseline = document?.captureEditingState().map { [$0] } ?? []
        let union = try EditingState.retainedBytes(in: entries.map(\.state) + [state] + baseline)
        let current = try EditingState.retainedBytes(in: baseline)
        guard union <= current || union - current <= limit else {
            throw failure("The snapshot memory budget is full. Save and remove a snapshot first.")
        }
        let entry = Entry(id: UUID().uuidString, name: name, created: created, state: state)
        entries.append(entry)
        changed()
        return entry
    }

    private static func write(_ archive: Archived, to directory: URL) throws {
        try EditingStateArchive.write(archive.state, to: directory)
        do { try JSONEncoder().encode(archive.header).write(to: directory.appendingPathComponent("snapshot.json")) }
        catch { try? FileManager.default.removeItem(at: directory); throw error }
    }

    private static func read(from directory: URL) throws -> Loaded {
        let header = try JSONDecoder().decode(Header.self, from: EditingStateArchive.readFile(
            directory.appendingPathComponent("snapshot.json"), maximum: 4096))
        guard header.version == 1, header.created.timeIntervalSince1970.isFinite else {
            throw failure("Unsupported snapshot header.")
        }
        _ = try validName(header.name)
        return Loaded(header: header, state: try EditingStateArchive.read(from: directory))
    }

    private func changed() {
        NotificationCenter.default.post(name: .imageDocumentSnapshotsDidChange, object: document)
    }

    private func validName(_ name: String) throws -> String { try Self.validName(name) }
    private static func validName(_ name: String) throws -> String {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed.utf8.count <= 256,
              !trimmed.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
            throw failure("Use a snapshot name of 1–256 UTF-8 bytes without control characters.")
        }
        return trimmed
    }
    private func failure(_ message: String) -> DoctoredCoreError { Self.failure(message) }
    private static func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }
}
