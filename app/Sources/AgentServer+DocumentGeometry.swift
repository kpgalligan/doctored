import Foundation

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let documentGeometryHandlers: [String: ToolHandler] = [
        "flip": flip,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let documentGeometryToolFacts: [String: ToolFacts] = [
        // Phase 12F-1b: the EXACT canvas geometry. A zero-angle `crop` keeps
        // every raster and mask allocation; a quarter turn and a flip are a
        // Nearest PERMUTATION, so an extended-range component is only ever
        // copied. The FILTERED geometry is admitted too — a `crop`
        // with an angle (straighten), an arbitrary `rotate`, `image_size`,
        // the layer transforms and `transform_mask` — through the core's
        // premultiplied extended-range resampler, which clamps each RGB
        // component to the layer's own visible range (D3). `transform_again`
        // repeats a layer transform; with a selection it would transform the
        // selected PIXELS, which float does not admit, so
        // `DocumentPrecision.refusal` refuses that case by name.
        "flip": [.typedTool, .floatTool, .cmykTool],
    ]

    /// Publishes one checked exact canvas operation. Preparation happens once;
    /// a content-identical result returns a successful no-op before any undo
    /// group or document state is touched.
    func exactDocumentGeometry(_ document: ImageDocument, _ op: DocumentGeometry) throws -> String {
        guard let current = document.doc else { throw ToolError(message: "No image loaded") }
        let revision = document.revision
        let state = document.captureEditingState()
        // #547 Indexed phase 2c: an Indexed document's admitted plan, read before the edit
        // (`scripts/indexed-admission/measure.py`'s `rotate90-indexed`).
        let indexedPlan = IndexedDocument.geometryPlanBytes(current, op)
        let prepared: RasterDocument?
        do {
            prepared = try current.applyingDocumentGeometry(op)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        guard document.doc === current, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(state) else {
            throw ToolError(
                message: "The document or edit target changed while \(op.actionName) was running. No result was applied.")
        }
        guard let prepared else {
            return try structureNoOp(
                ["document": summary(document)],
                why: "the exact canvas operation produced identical stored document state.")
        }
        // The selection goes through the op's own map.
        try performGroupedEdit(document, op.actionName, changesCanvasGeometry: true,
                               resultingSelection: document.selectionCarried(through: op)) { _ in prepared }
        var reply: [String: Any] = ["ok": true, "document": summary(document)]
        if let indexedPlan { reply["working_memory_plan_bytes"] = indexedPlan }
        return try jsonResult(reply)
    }

    // The MCP `flip` tool mirrors Image ▸ Flip Horizontal / Flip Vertical.
    func flip(_ a: [String: Any]) throws -> String {
        let op: DocumentGeometry
        switch try requiredString(a, "axis") {
        case "horizontal": op = .flipHorizontal
        case "vertical": op = .flipVertical
        default: throw ToolError(message: "axis must be \"horizontal\" or \"vertical\"")
        }
        return try exactDocumentGeometry(try target(a), op)
    }

    /// True only for the successful changed:false result shape of the
    /// canvas geometry tools (`rotate`, `flip`, `crop`, `canvas_size`): a
    /// no-op records no step. It is intentionally stateless so suppression
    /// cannot leak to a later call.
    static func isUnchangedCanvasGeometryResult(tool: String, result: String) -> Bool {
        guard ["rotate", "flip", "crop", "canvas_size"].contains(tool),
              let data = result.data(using: .utf8),
              let call = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              call["isError"] as? Bool == false,
              let content = call["content"] as? [[String: Any]] else { return false }
        for item in content where item["type"] as? String == "text" {
            guard let text = item["text"] as? String,
                  let payloadData = text.data(using: .utf8),
                  let payload = try? JSONSerialization.jsonObject(with: payloadData)
                    as? [String: Any] else { continue }
            if payload["ok"] as? Bool == true, payload["changed"] as? Bool == false {
                return true
            }
        }
        return false
    }
}
