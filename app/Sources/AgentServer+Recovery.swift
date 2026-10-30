import AppKit

/// Mirrors File > Recover Unsaved Work and its checkpoint/recover/discard choices.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let recoveryHandlers: [String: ToolHandler] = [
        "get_recovery_status": { $0.recoveryStatus },
        "write_recovery": { $0.writeRecovery },
        "recover_document": { $0.recoverDocument },
        "discard_recovery": { $0.discardRecovery },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let recoveryToolFacts: [String: ToolFacts] = [
        "get_recovery_status": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "write_recovery": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "recover_document": [.nonRecordable, .commonTool, .floatTool],
        "discard_recovery": [.nonRecordable, .commonTool, .floatTool],
    ]

    var recoveryCatalog: [[String: Any]] {
        let id: [String: Any] = ["type": "string", "description": "recovery_id from get_recovery_status."]
        return [
            tool("get_recovery_status", "Native recovery interval, open-document checkpoint status and abandoned copies. Recovery never overwrites an original and includes session snapshots. State failed means the document is not protected (the editor shows Recovery off); a working-memory refusal adds memory figures.", ["document_id": Self.docIDProperty]),
            tool("write_recovery", "Checkpoint the current complete editing state and session snapshots in private native recovery storage. Does not mark the document saved. Busy asynchronous requests retry automatically.", ["document_id": Self.docIDProperty, "wait": ["type": "boolean", "description": "Wait for completion; default true."]]),
            tool("recover_document", "Open an abandoned recovery as a dirty native untitled copy. Original or moved source files remain unchanged. Returns a warning if only the previous checkpoint is readable.", ["recovery_id": id, "show": ["type": "boolean", "description": "Show the recovered window; default true."]], required: ["recovery_id"]),
            tool("discard_recovery", "Permanently discard an abandoned recovery copy. Refuses copies leased by an open document in any app instance.", ["recovery_id": id], required: ["recovery_id"]),
        ]
    }

    func recoveryStatus(_ arguments: [String: Any]) throws -> String {
        let coordinator = RecoveryCoordinator.shared
        let documents = NSDocumentController.shared.documents.compactMap { $0 as? ImageDocument }
        let open = documents.map { document -> [String: Any] in
            var report = coordinator.status(for: document)
            report["document_id"] = documentID(for: document)
            return report
        }
        var result: [String: Any] = ["interval_seconds": coordinator.interval,
            "open_documents": open, "available": try coordinator.available().map(\.report),
            "root": coordinator.root.path,
            "policy": "Native checkpoints start within the interval plus one polling tick; active work and encoding can defer completion. Two generations are kept. Originals are never autosaved over."]
        if intArg(arguments, "document_id") != nil {
            let document = try target(arguments)
            var report = coordinator.status(for: document)
            report["document_id"] = documentID(for: document)
            result["document"] = report
        }
        return try jsonResult(result)
    }

    func writeRecovery(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        try RecoveryCoordinator.shared.checkpoint(document, wait: boolArg(arguments, "wait") ?? true)
        var report = RecoveryCoordinator.shared.status(for: document)
        report["document_id"] = documentID(for: document)
        return try jsonResult(report)
    }

    func recoverDocument(_ arguments: [String: Any]) throws -> String {
        guard let id = stringArg(arguments, "recovery_id") else { throw ToolError(message: "recovery_id is required.") }
        let (document, warning) = try RecoveryCoordinator.shared.recover(id: id, show: boolArg(arguments, "show") ?? true)
        var report: [String: Any] = ["document": summary(document), "recovery_id": id]
        if let warning { report["warning"] = warning }
        return try jsonResult(report)
    }

    func discardRecovery(_ arguments: [String: Any]) throws -> String {
        guard let id = stringArg(arguments, "recovery_id") else { throw ToolError(message: "recovery_id is required.") }
        try RecoveryCoordinator.shared.discard(id: id)
        return try jsonResult(["ok": true])
    }
}
