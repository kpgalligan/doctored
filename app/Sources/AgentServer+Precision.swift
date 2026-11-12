import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let precisionHandlers: [String: ToolHandler] = [
        "get_precision": { $0.getPrecision },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let precisionToolFacts: [String: ToolFacts] = [
        "get_precision": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
    ]

    /// Image > Document Precision and the same operation/target/depth table
    /// used by menu validation and the pre-dispatch Actions/MCP gate.
    func getPrecision(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // A Bitmap or Indexed Color document answers its own gate's list, its mode named
        // `bitmap1` or `indexed8` (`precisionName`).
        if doc.isBitmap || doc.isIndexed {
            var result = doc.isBitmap ? BitmapDocument.support(doc) : IndexedDocument.support(doc)
            result["document"] = summary(document)
            result["mode"] = doc.precisionName
            result["mode_description"] = doc.isBitmap ? "Bitmap, 1-bit"
                : "Indexed Color, 8-bit, \(doc.indexedColors ?? 0) colors"
            if let projection = try? doc.projectionScratchEstimate() {
                result["plans"] = ["projection_scratch_bytes": projection]
            }
            return try jsonResult(result)
        }
        // #1 phase 1c: a Grayscale document answers its own gate's list.
        if doc.isGray {
            var result = GrayDocument.support(doc)
            let depth = doc.grayDepth ?? 0
            result["document"] = summary(document)
            result["mode"] = doc.precisionName
            result["bits_per_channel"] = depth
            result["mode_description"] = "Grayscale, \(depth)-bit, \(doc.profileName)"
            if let projection = try? doc.projectionScratchEstimate() {
                result["plans"] = ["projection_scratch_bytes": projection]
            }
            return try jsonResult(result)
        }
        // #1 Lab phase 1c: a Lab Color document answers its own gate's list.
        if doc.isLab {
            var result = LabDocument.support(doc)
            let depth = doc.labDepth ?? 0
            result["document"] = summary(document)
            result["mode"] = doc.precisionName
            result["bits_per_channel"] = depth
            result["mode_description"] = "Lab Color, \(depth)-bit, D50"
            if let projection = try? doc.projectionScratchEstimate() {
                result["plans"] = ["projection_scratch_bytes": projection]
            }
            return try jsonResult(result)
        }
        var result = doc.isFloat ? FloatDocument.support : DocumentPrecision.support
        result["document"] = summary(document)
        result["mode"] = doc.isFloat ? "rgba32" : doc.isHighDepth ? "rgba16" : "rgba8"
        result["bits_per_channel"] = doc.isFloat ? 32 : doc.isHighDepth ? 16 : 8
        result["mode_description"] = doc.isFloat
            ? "32-bit float source, extended-range linear light, tone-mapped output"
            : doc.isHighDepth ? "16-bit integer source, float evaluation" : "8-bit source and output"
        // The view's charge (`projectionScratchEstimate`) at 16 and 32 bits, under the key a CMYK
        // document's `plans` use — what the RGB admission row reads.
        if doc.isHighDepth, let projection = try? doc.projectionScratchEstimate() {
            result["plans"] = ["projection_scratch_bytes": projection]
        }
        return try jsonResult(result)
    }

    /// A nil result means the normal handler has an explicit typed facade
    /// route. All unmapped tools have already been refused by the shared gate.
    func dispatchPrecision(_ tool: String, _ a: [String: Any]) throws -> String? {
        guard let document = try? target(a), document.doc?.isHighDepth == true else { return nil }
        // #1: a CMYK document's own routes first (AgentServer+CmykMode.swift).
        if let result = try dispatchCmyk(tool, a, document: document) { return result }
        // #1 phase 5a: a Grayscale document's profile and depth routes (AgentServer+GrayColour.swift).
        if let result = try dispatchGrayColour(tool, a, document: document) { return result }
        // #1 Lab phase 5a: a Lab document's depth route (AgentServer+LabColour.swift).
        if let result = try dispatchLabColour(tool, a, document: document) { return result }
        switch tool {
        case "get_document":
            return try typedDocumentReply(document)
        case "brush_stroke", "eraser_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "fill", "clear_selection":
            return try precisionPaint(tool, a, document: document)
        // In Quick Mask the ramp paints the Quick Mask coverage.
        case "gradient":
            return try quickMaskGradient(a, document) ?? precisionGradient(a, document: document)
        case "dodge_burn": return try precisionDodgeBurn(a, document: document)
        case "blur_stroke", "sharpen_stroke", "smudge_stroke", "sponge_stroke", "history_brush_stroke":
            return try precisionRetouchBrush(tool, a, document: document)
        case "art_history_brush_stroke": return try precisionArtHistoryBrush(a, document: document)
        case "sample_color": return try precisionSampleColor(a, document: document)
        case "sample_pixel": return try precisionSample(a, document: document)
        case "histogram": return try precisionHistogram(a, document: document)
        case "add_layer_mask", "remove_layer_mask", "set_layer_mask_enabled", "get_mask_properties",
             "set_mask_properties", "save_selection", "load_selection", "delete_channel", "add_channel":
            return try precisionScalarTool(tool, a, document: document)
        default: return nil
        }
    }

    /// 12E-2: a typed adjustment lands at the slot `position` names relative
    /// to `anchor` (the ACTIVE entry when the caller named none), not only at
    /// the top of the stack — the core owns the rule
    /// (`core/src/layer_insert.rs`) and `requireTopInsertion` is gone.
    func precisionAdjustment(_ document: ImageDocument, op: AdjustmentLayerOp,
                             params: [String: Any], index: Int?, destructive: Bool,
                             name: String? = nil, anchor: Int? = nil,
                             position: LayerInsertPosition = .above) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        if destructive, let index {
            return try selectedPrecisionFilter(document, filter: op.rawValue, params: params, index: index, target: .layer)
        }
        let target = anchor ?? capture.layer
        var landing = 0
        if index == nil {
            if let reason = position.refusal(in: capture.facade, anchor: target) {
                throw ToolError(message: reason)
            }
            landing = position.landingIndex(in: capture.facade, anchor: target)
        }
        let adjustment = try PrecisionAdjustment(op: op, params: params)
        let operation: PrecisionCapturedEdit.Operation
        if let index { operation = destructive ? .apply(index: index, adjustment: adjustment) : .set(index: index, adjustment: adjustment) }
        else {
            operation = .add(name: name ?? op.displayName, adjustment: adjustment, opacity: 1,
                             anchor: target, position: position)
        }
        var result = try PrecisionCapturedEdit(capture.source, operation: operation).run(title: "Applying 16-bit \(op.displayName)")
        if index == nil, let selection = capture.selection, let added = result {
            try capture.requireCurrent(document)
            let data = try PrecisionStrokeCoverage.selection(width: capture.facade.width,
                height: capture.facade.height, selection: selection)
            let info = try added.descriptor()
            let count = (info["layer_count"] as? NSNumber)?.intValue ?? 0
            guard count > landing else { throw ToolError(message: "The new adjustment has no valid layer index.") }
            // The mask goes onto the record that was INSERTED — the slot
            // rule's landing index, which is only `layer_count - 1` when the
            // position happened to be `top`.
            result = try PrecisionPixelEdit(added,
                command: .setMask(layer: landing, width: data.width, height: data.height), input: data).run()
        }
        let changed = try capture.publish(result, to: document, name: "\(op.displayName) (16-bit)")
        if index == nil, changed { document.activeLayerIndex = min(landing, document.doc.layerCount - 1) }
        return try jsonResult(["ok": true, "changed": changed, "layer": index ?? document.activeLayerIndex,
                               "op": op.rawValue, "position": position.rawValue,
                               "evaluation": PrecisionAdjustment.evaluation,
                               // #1: a CMYK document's Levels/Curves layer acts on ink.
                               "storage": capture.source.isCmyk ? "cmyka16_unorm" : "rgba16_unorm"])
    }
}
