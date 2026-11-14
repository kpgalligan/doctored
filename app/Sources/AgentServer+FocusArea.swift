import AppKit

/// Mirrors Select ▸ Focus Area… (`EditorViewController.commitFocusArea`): the same capture and
/// core call, one selection step.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let focusAreaHandlers: [String: ToolHandler] = [
        "select_focus_area": { $0.selectFocusArea },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. A selection is canvas
    /// coverage, read off the capture's 8-bit analysis source (`ImageDocument.analysisComposite`):
    /// RGB at 8 and 16 bits its composite, at 32 bits the document's own tone-mapped display;
    /// Grayscale the same (R = G = B); Lab Color and CMYK their Display P3 view; Indexed Color
    /// the palette colours. Grayscale, Lab and Indexed admit it through their closed sets, CMYK
    /// by `.cmykTool`; Duotone and Bitmap refuse (`designs/focus-area.md`, decision 7).
    static let focusAreaToolFacts: [String: ToolFacts] = [
        "select_focus_area": [.commonTool, .floatTool, .cmykTool],
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
