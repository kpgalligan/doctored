import AppKit

/// Image ▸ Adjustments ▸ Equalize (#547):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `equalize` | Image ▸ Adjustments ▸ Equalize (its prompt answered by `scope`) |
///
/// `Equalize.apply` is the one apply the menu item and this handler share; the dispatcher records
/// the call as its Action step.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let equalizeHandlers: [String: ToolHandler] = [
        "equalize": { $0.equalize },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let equalizeToolFacts: [String: ToolFacts] = [
        // 8, 16 and 32 bits on RGB and CMYK 8/16 (#547); the selection weighs and gates it.
        "equalize": [.repeatable, .selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    /// Image ▸ Adjustments ▸ Equalize on `layer`'s `target` (`paintTarget`'s: an 8-bit RGB document
    /// takes the layer, its mask, one colour plane or a saved channel; a typed one the layer), with
    /// `scope` the prompt's answer when a selection exists.
    func equalize(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let name = stringArg(a, "scope") ?? EqualizeScope.selected.rawValue
        guard let scope = EqualizeScope(rawValue: name) else {
            throw ToolError(message: "scope must be selected or entire (got \"\(name)\").")
        }
        let layer = try paintLayerIndex(a, document)
        let paint = try paintTarget(a, document)
        let reported = document.canvasSelection == nil ? "whole" : scope.rawValue
        // An 8-bit document: the scoped-filter capture and commit every byte filter's MCP twin
        // uses, so `layer` and `target` need not be the active ones.
        if document.doc?.isHighDepth != true {
            let capture = try operationCapture(document, operation: .filter, target: paint, layer: layer)
            let updated: RasterDocument?
            do {
                updated = try Equalize.byteResult(capture, scope: scope)
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
            return try commitScopedResult(document, capture: capture, updated: updated,
                                          actionName: "Equalize", extra: ["scope": reported])
        }
        let changed: Bool
        do {
            changed = try Equalize.apply(document, layer: layer, target: paint, scope: scope, record: false)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try jsonResult(["ok": true, "changed": changed, "layer": layer,
                               "target": paint.agentName(in: document.doc), "scope": reported])
    }
}
