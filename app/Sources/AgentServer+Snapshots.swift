import Foundation

/// Mirrors Window > Snapshots, including explicit portable Save/Load files.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let snapshotsHandlers: [String: ToolHandler] = [
        "list_snapshots": { $0.listSnapshots },
        "create_snapshot": { $0.createSnapshot },
        "rename_snapshot": { $0.renameSnapshot },
        "delete_snapshot": { $0.deleteSnapshot },
        "restore_snapshot": { $0.restoreSnapshot },
        "save_snapshot": { $0.saveSnapshot },
        "load_snapshot": { $0.loadSnapshot },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let snapshotsToolFacts: [String: ToolFacts] = [
        "list_snapshots": [.nonRecordable, .commonTool, .floatTool],
        "create_snapshot": [.nonRecordable, .commonTool, .floatTool],
        "rename_snapshot": [.nonRecordable, .commonTool, .floatTool],
        "delete_snapshot": [.nonRecordable, .commonTool, .floatTool],
        "restore_snapshot": [.nonRecordable, .commonTool, .floatTool],
        "save_snapshot": [.nonRecordable, .commonTool, .floatTool],
        "load_snapshot": [.nonRecordable, .commonTool, .floatTool],
    ]

    var snapshotCatalog: [[String: Any]] {
        let id: [String: Any] = ["type": "string", "description": "ID from list_snapshots."]
        let name: [String: Any] = ["type": "string", "description": "Snapshot name (1–256 UTF-8 bytes)."]
        let path: [String: Any] = ["type": "string", "description": "Absolute .drsnapshot package path."]
        let doc = Self.docIDProperty
        return [
            tool("list_snapshots", "Session snapshots and their memory budget. Ordinary .doctored saves do not embed this list.", ["document_id": doc]),
            tool("create_snapshot", "Capture the full editing state in the session list. Use save_snapshot to persist a portable file.", ["document_id": doc, "name": name], required: ["name"]),
            tool("rename_snapshot", "Rename a session snapshot.", ["document_id": doc, "snapshot_id": id, "name": name], required: ["snapshot_id", "name"]),
            tool("delete_snapshot", "Remove a session snapshot; saved snapshot files remain on disk.", ["document_id": doc, "snapshot_id": id], required: ["snapshot_id"]),
            tool("restore_snapshot", "Restore image, selection, layer target and export settings as one undoable edit.", ["document_id": doc, "snapshot_id": id], required: ["snapshot_id"]),
            tool("save_snapshot", "Atomically save a portable .drsnapshot package without changing the document's saved state.", ["document_id": doc, "snapshot_id": id, "path": path], required: ["snapshot_id", "path"]),
            tool("load_snapshot", "Load a portable snapshot into the session list. Restore separately to apply it.", ["document_id": doc, "path": path], required: ["path"]),
        ]
    }

    func listSnapshots(_ a: [String: Any]) throws -> String {
        try jsonResult(target(a).namedSnapshots.report())
    }

    func createSnapshot(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let entry = try document.namedSnapshots.create(name: snapshotName(a))
        return try jsonResult(["snapshot_id": entry.id, "name": entry.name])
    }

    func renameSnapshot(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try document.namedSnapshots.rename(id: snapshotID(a), name: snapshotName(a))
        return try jsonResult(document.namedSnapshots.report())
    }

    func deleteSnapshot(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try document.namedSnapshots.delete(id: snapshotID(a))
        return try jsonResult(document.namedSnapshots.report())
    }

    func restoreSnapshot(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try document.namedSnapshots.restore(id: snapshotID(a))
        return try jsonResult(["ok": true, "unsaved_changes": document.isDocumentEdited])
    }

    func saveSnapshot(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let url = try snapshotURL(a)
        try document.namedSnapshots.save(id: snapshotID(a), to: url)
        return try jsonResult(["ok": true, "path": url.path])
    }

    func loadSnapshot(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let entry = try document.namedSnapshots.load(from: snapshotURL(a))
        return try jsonResult(["snapshot_id": entry.id, "name": entry.name,
                               "note": "Loaded into the session list; use restore_snapshot to apply it."])
    }

    private func snapshotID(_ a: [String: Any]) throws -> String {
        guard let value = stringArg(a, "snapshot_id") else { throw ToolError(message: "snapshot_id is required.") }
        return value
    }
    private func snapshotName(_ a: [String: Any]) throws -> String {
        guard let value = stringArg(a, "name") else { throw ToolError(message: "name is required.") }
        return value
    }
    private func snapshotURL(_ a: [String: Any]) throws -> URL {
        guard let path = stringArg(a, "path"), path.hasPrefix("/") else {
            throw ToolError(message: "An absolute snapshot path is required.")
        }
        return URL(fileURLWithPath: path)
    }
}
