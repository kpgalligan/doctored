import Foundation

/// Mirrors Window > History, including navigation and its per-document budget.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let historyHandlers: [String: ToolHandler] = [
        "list_history": { $0.listHistory },
        "restore_history": { $0.restoreHistory },
        "set_history_budget": { $0.setHistoryBudget },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let historyToolFacts: [String: ToolFacts] = [
        "list_history": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "restore_history": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "set_history_budget": [.nonRecordable, .commonTool, .floatTool],
    ]

    var historyCatalog: [[String: Any]] {
        let doc = Self.docIDProperty
        return [
            tool("list_history", "Retained full editing states, current position, saved markers and memory budget.",
                 ["document_id": doc]),
            tool("restore_history", "Navigate existing Undo/Redo history to a retained state. New edits discard the redo branch. A restore whose state's display goes over the working limit still proceeds and returns memory_warning naming the overage. While a job is still running on the document it refuses, naming the job_id: cancel_job it or wait for job_status, then restore_history.",
                 ["document_id": doc, "state_id": ["type": "string"]], required: ["state_id"]),
            tool("set_history_budget", "Set this document's incremental history byte budget. Old states are evicted; the nearest reversible transition is retained even if it alone exceeds the budget, and reported explicitly.",
                 ["document_id": doc, "bytes": ["type": "integer", "minimum": 1]], required: ["bytes"]),
        ]
    }

    func listHistory(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        if document.history.entries.isEmpty { document.history.reset(document.captureEditingState()) }
        return try jsonResult(document.history.report())
    }

    func restoreHistory(_ arguments: [String: Any]) throws -> String {
        guard let id = stringArg(arguments, "state_id") else {
            throw ToolError(message: "state_id from list_history is required.")
        }
        let document = try target(arguments)
        // MCP `undo`/`redo`'s rule: refuse by name while any job runs.
        if let refusal = DocumentJobs.historyRefusal(on: document, then: "restore_history") {
            throw ToolError(message: refusal)
        }
        try document.navigateHistory(toID: id)
        var report = document.history.report()
        // An over-limit restore proceeds and says so.
        if let warning = document.historyMemoryWarning { report["memory_warning"] = warning.detail }
        return try jsonResult(report)
    }

    func setHistoryBudget(_ arguments: [String: Any]) throws -> String {
        guard let bytes = intArg(arguments, "bytes"), bytes > 0 else {
            throw ToolError(message: "bytes must be a positive integer.")
        }
        let document = try target(arguments)
        try document.setHistoryBudget(UInt64(bytes))
        return try jsonResult(document.history.report())
    }
}
