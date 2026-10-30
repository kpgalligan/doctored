import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let selectionHistoryHandlers: [String: ToolHandler] = [
        "reselect": { $0.reselect },
        "get_selection": { $0.getSelection },
        "set_quick_mask": { $0.setQuickMask },
        "quick_mask_stroke": { $0.quickMaskStroke },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let selectionHistoryToolFacts: [String: ToolFacts] = [
        // Parity with the float menu/tool whitelist in
        // `EditorViewController+Precision.swift`: a selection is canvas
        // coverage, not document pixels, so everything the UI lets a user do
        // with one the agent answers too (including Save/Load Selection,
        // whose `channel_set` family is admitted at depth 32).
        "reselect": [.commonTool, .floatTool, .cmykTool],
        "get_selection": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "set_quick_mask": [.commonTool, .floatTool, .cmykTool],
        "quick_mask_stroke": [.commonTool, .floatTool, .cmykTool],
    ]

    // Select ▸ Reselect. Transient history shared with the visible marquee.
    func reselect(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard document.canReselect else { throw ToolError(message: "There is no previous selection to restore.") }
        _ = document.reselect()
        return try jsonResult(selectionHistorySummary(document))
    }

    // Select ▸ Quick Mask Mode. Mode changes and strokes remain file-clean.
    func setQuickMask(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let active = boolArg(a, "active") else {
            throw ToolError(message: "set_quick_mask requires active (boolean).")
        }
        document.setQuickMaskActive(active)
        return try jsonResult(selectionHistorySummary(document))
    }

    // Brush/Eraser in Quick Mask. Captures the actual gray/alpha overlay,
    // preserving pressure, smoothing, opacity and airbrush timing during replay.
    func quickMaskStroke(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        if let native = try capturedNativeStroke(a) {
            guard let doc = document.doc, !doc.isFloat,
                  intArg(a, "width") == doc.width, intArg(a, "height") == doc.height,
                  a["coverage_rle"] == nil, a["coverage_rle16"] == nil else {
                throw ToolError(message: "Quick Mask needs matching dimensions and one canonical stroke encoding on a byte or 16-bit document.")
            }
            let opacity = doubleArg(a, "opacity") ?? 1
            guard opacity.isFinite, (0...1).contains(opacity),
                  a["erase"] == nil || NativeBrushStroke.jsonBoolean(a["erase"]) != nil else {
                throw ToolError(message: "Quick Mask opacity must be 0…1 and erase must be boolean.")
            }
            let erase = NativeBrushStroke.jsonBoolean(a["erase"]) ?? false
            let stroke: QuickMaskStroke
            if doc.isHighDepth {
                stroke = try QuickMaskStroke(nativeBrush: native, width: doc.width,
                    height: doc.height, opacity: opacity, erase: erase)
            } else {
                guard let size = NativeBrushStroke.jsonNumber(a["size"]),
                      size.isFinite,
                      (EditorTool.brush.minimumBrushSize...EditorTool.brush.maximumBrushSize).contains(size) else {
                    throw ToolError(message: "Byte canonical Quick Mask needs size between "
                        + "\(Int(EditorTool.brush.minimumBrushSize)) and \(Int(EditorTool.brush.maximumBrushSize)).")
                }
                stroke = try QuickMaskStroke(byteBrush: native, size: CGFloat(size),
                    width: doc.width, height: doc.height, opacity: opacity, erase: erase)
            }
            try document.applyQuickMaskStroke(stroke)
            return try jsonResult(selectionHistorySummary(document))
        }
        guard let doc = document.doc,
              let width = intArg(a, "width"), let height = intArg(a, "height"),
              width == doc.width, height == doc.height,
              let encoded = stringArg(a, "coverage_rle16") ?? stringArg(a, "coverage_rle")
        else { throw ToolError(message: "Quick Mask requires matching canvas width/height and coverage_rle.") }
        guard a["coverage_rle"] == nil || a["coverage_rle16"] == nil else { throw ToolError(message: "Choose one Quick Mask coverage encoding.") }
        let stroke = try QuickMaskStroke(width: width, height: height, base64: encoded, native: a["coverage_rle16"] != nil)
        try document.applyQuickMaskStroke(stroke)
        return try jsonResult(selectionHistorySummary(document))
    }

    /// Read-only coverage inspection, including while Quick Mask hides the marquee.
    func getSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        var result = selectionHistorySummary(document)
        if let points = a["points"] as? [[String: Any]], let doc = document.doc {
            guard points.count <= 4096 else { throw ToolError(message: "At most 4096 selection samples are allowed.") }
            let coverage = document.canvasSelection?.maskBytes()
            let native = document.canvasSelection?.maskSamples16()
            result["samples"] = try points.map { point -> [String: Any] in
                guard let x = intArg(point, "x"), let y = intArg(point, "y"),
                      x >= 0, y >= 0, x < doc.width, y < doc.height
                else { throw ToolError(message: "Selection sample lies outside the canvas.") }
                return ["x": x, "y": y, "coverage": coverage?[y * doc.width + x] ?? 0,
                    "coverage16": native?[y * doc.width + x] ?? 0]
            }
        }
        return try jsonResult(result)
    }

    func selectionHistorySummary(_ document: ImageDocument) -> [String: Any] {
        var result: [String: Any] = ["ok": true, "selection_empty": document.canvasSelection == nil,
                                    "can_reselect": document.canReselect,
                                    "quick_mask_active": document.quickMaskActive,
                                    "coverage_maximum": 65535,
                                    "coverage_storage": document.canvasSelection?.hasNativeCoverage == true ? "u16" : "u8"]
        if let bounds = document.canvasSelection?.bounds {
            result["bounds"] = ["x": Int(bounds.minX), "y": Int(bounds.minY),
                                "width": Int(bounds.width), "height": Int(bounds.height)]
        }
        if let selection = document.canvasSelection {
            result["outline_simplified"] = selection.contourIsSimplified
            if let notice = selection.outlineNotice { result["outline_note"] = notice }
        }
        return result
    }
}
