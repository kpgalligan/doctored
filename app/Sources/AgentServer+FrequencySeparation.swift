import AppKit

/// Mirrors Filters ▸ Other ▸ Frequency Separation…: the same `FrequencySeparation` split the
/// sheet commits, as one undo step at 8 and 16 bits.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let frequencySeparationHandlers: [String: ToolHandler] = [
        "frequency_separation": { $0.frequencySeparation },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. 8 and 16 bits only: a
    /// 32-bit float document and the colour modes refuse it through their closed sets.
    static let frequencySeparationToolFacts: [String: ToolFacts] = [
        "frequency_separation": [.commonTool],
    ]

    /// Filters ▸ Other ▸ Frequency Separation… (`EditorViewController.commitFrequencySeparation`).
    func frequencySeparation(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let layer = try structuralLayerIndex(a, document)
        let radius = doubleArg(a, "radius") ?? 4
        guard FrequencySeparationParameters.radiusRange.contains(radius) else {
            throw ToolError(message: "radius must be from 0.5 to 64 pixels.")
        }
        let p = FrequencySeparationParameters(layer: layer, radius: radius)
        if let reason = FrequencySeparation.refusal(doc, layer: layer, quickMask: document.quickMaskActive) {
            throw ToolError(message: reason)
        }
        if doc.isHighDepth {
            let capture = try PrecisionEditorCapture(document)
            let result = try FrequencySeparation.nativeSplit(capture, p)
            try capture.publish(result, to: document, name: FrequencySeparation.actionName)
        } else {
            let low = try FrequencySeparation.blurredLow(doc, p, waitForSlot: false)
            try performGroupedEdit(document, FrequencySeparation.actionName) { current in
                guard current === doc else {
                    throw ToolError(message: "The document changed while the low layer was blurred. No result was applied.")
                }
                return try FrequencySeparation.split(current, p, low: low)
            }
        }
        return try jsonResult(["ok": true, "action": FrequencySeparation.actionName, "low_layer": layer,
                               "high_layer": layer + 1, "radius": radius])
    }
}
