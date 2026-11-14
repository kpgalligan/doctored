import AppKit

/// Mirrors Select ▸ Focus Area… (`EditorViewController.commitFocusArea`): the same capture and
/// core call, one selection step.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let focusAreaHandlers: [String: ToolHandler] = [
        "select_focus_area": { $0.selectFocusArea },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. RGB at 8, 16 and 32
    /// bits: a selection is canvas coverage, read off the capture's 8-bit analysis source (at
    /// 32 bits the R1 display codes, as Color Range reads them). The colour modes refuse
    /// through their closed sets (`designs/focus-area.md`, decision 7).
    static let focusAreaToolFacts: [String: ToolFacts] = [
        "select_focus_area": [.commonTool, .floatTool],
    ]

    /// Select ▸ Focus Area…
    func selectFocusArea(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = FocusArea.refusal(doc) { throw ToolError(message: reason) }
        var parameters = FocusAreaParameters()
        if let raw = a["range"], !(raw is NSNull) {
            let range = try selectionNumber(a, "range", default: 50)
            guard FocusAreaParameters.rangeRange.contains(range) else {
                throw ToolError(message: "range must be from 0 to 100.")
            }
            parameters.range = range
        }
        if a["soften_edge"] != nil {
            guard let soften = boolArg(a, "soften_edge") else {
                throw ToolError(message: "soften_edge must be true or false.")
            }
            parameters.softenEdge = soften
        }
        let mode = try selectionMode(a)
        let capture = try SelectionCapture(document: document)
        let (shape, range) = try capture.focusAreaShape(parameters, in: document)
        try capture.commit(shape, in: document, name: FocusArea.actionName, mode: mode)
        var reply = selectionReplyFields(document)
        reply["range"] = range
        return try jsonResult(reply)
    }
}
