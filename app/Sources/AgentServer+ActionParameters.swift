import AppKit

/// `edit_action_parameters` — the MCP mirror of the Actions window's Parameters… sheet
/// (`ActionParametersEditorSheetController`): add, rename, retype, give a default or a description
/// and remove an action's declared parameters without writing JSON. The edits run through
/// `Action.applying` (`ActionParameterEdits.swift`) all or nothing, and the result is saved through
/// `ActionLibrary.save`, so a refused list leaves the file's bytes as they were.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let actionParametersHandlers: [String: ToolHandler] = [
        "edit_action_parameters": { $0.editActionParameters },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. An action-library
    /// write, as `save_action` is: no document, never a step.
    static let actionParametersToolFacts: [String: ToolFacts] = [
        "edit_action_parameters": [.nonRecordable, .documentFree],
    ]

    /// Mirrors the Actions window's Parameters… sheet.
    func editActionParameters(_ a: [String: Any]) throws -> String {
        guard let name = stringArg(a, "name"), !name.isEmpty else {
            throw ToolError(message: "Missing required argument: name")
        }
        guard let rawEdits = a["edits"] as? [Any], !rawEdits.isEmpty else {
            throw ToolError(message: "edits must be a non-empty array of edit objects")
        }
        var edits: [ActionParameterEdit] = []
        for (index, raw) in rawEdits.enumerated() {
            switch ActionParameterEdit.parse(raw, at: index) {
            case .success(let edit): edits.append(edit)
            case .failure(let error): throw ToolError(message: error.message)
            }
        }
        let action: Action
        switch ActionLibrary.entry(named: name) {
        case .ok(let decoded)?: action = decoded
        case .broken(let file, let reason)?: throw ToolError(message: "\(file): \(reason)")
        case nil: throw ToolError(message: "No action named \"\(name)\". Call list_actions.")
        }
        let edited: Action
        switch action.applying(edits) {
        case .success(let result): edited = result
        case .failure(let error): throw ToolError(message: error.message)
        }
        do {
            _ = try ActionLibrary.save(edited)
        } catch let error as ActionLibrary.Failure {
            throw ToolError(message: error.message)
        }
        return try jsonResult([
            "ok": true, "name": edited.name,
            "parameters": edited.parameters.map { $0.jsonObject() },
        ])
    }

    @inline(never) func actionParametersCatalog() -> [[String: Any]] {
        [
            tool(
                "edit_action_parameters",
                "The Actions window's Parameters… sheet: edits an action's declared parameters "
                    + "in place, all or nothing. Each edit is {op, name, …}: \"add\" (type "
                    + "number|integer|string|boolean, optional default and description), "
                    + "\"rename\" (to) — every {\"$param\": name} in every step is rewritten — "
                    + "\"retype\" (type; a default that still matches is kept, integer and number "
                    + "convert when exact, any other default is refused unless the same edit "
                    + "gives a new one or null), \"set_default\" (default; null makes it "
                    + "required), \"set_description\" (description) and \"remove\" (refused while "
                    + "a step still uses it, naming the steps). A refused list changes nothing. "
                    + "Replies with the action's parameters.",
                [
                    "name": ["type": "string", "description": "The action to edit."],
                    "edits": [
                        "type": "array",
                        "items": ["type": "object"],
                        "description": "The edits, applied in order: {\"op\": \"add\"|\"rename\"|"
                            + "\"retype\"|\"set_default\"|\"set_description\"|\"remove\", "
                            + "\"name\", \"to\", \"type\", \"default\", \"description\"}.",
                    ],
                ], required: ["name", "edits"]),
        ]
    }
}
