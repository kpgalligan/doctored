import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let moveSelectedHandlers: [String: ToolHandler] = [
        "move_selected_pixels": { $0.moveSelectedPixels },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let moveSelectedToolFacts: [String: ToolFacts] = [
        // Move Selected Pixels rides the typed `move_selected` request.
        // A 32-bit float document admits a mask or channel target,
        // and a layer target too, through the capability table.
        "move_selected_pixels": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
    ]

    /// `move_selected_pixels` — mirrors the Move tool's drag and arrow nudge
    /// with a selection active (EditorViewController+MoveSelected.swift):
    /// the layer's pixels × the selection float by (dx, dy), the hole is
    /// transparent, and the selection moves with them, in ONE undo step.
    /// `duplicate` mirrors the ⌥-drag (the press latch in
    /// `MultiLayerEdit.moveDidBegin`): no hole, the copy lands and the
    /// selection moves with it, one "Duplicate" step.
    /// Described layers rasterize silently and say so (`pixelEditResult`).
    /// `target` "mask" or "channel:<name>" mirrors the drag and nudge
    /// with that plane as the edit target (`openMoveSelectedPlane`): the
    /// vacated area takes `background` (default #000000) as coverage gray,
    /// at 8, 16 and 32-bit float; nothing is rasterized. `target` "red",
    /// "green", "blue" or "alpha" mirrors the drag and nudge with that
    /// colour plane as the edit target: the layer path's lock (Pixels; Alpha
    /// a merge edit) and silent rasterize, and for R/G/B the vacated area
    /// takes `background`'s 30/59/11 luma rounded to a code; the reply is the
    /// layer path's.
    /// A call on the active layer and target, with the document and the
    /// selection as the last Move left them, continues that Move's float as
    /// a drag does (`continuing`): the float's source, coverage and latched
    /// background at its TOTAL, one "Move" step, and `background` ignored.
    func moveSelectedPixels(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "No image loaded.") }
        guard let selection = document.canvasSelection else {
            throw ToolError(message:
                "move_selected_pixels needs an active selection; use move_layers to move whole layers")
        }
        guard let dx = intArg(a, "dx"), let dy = intArg(a, "dy") else {
            throw ToolError(message: "move_selected_pixels requires dx and dy — whole canvas pixels "
                + "to move by (x right, y down).")
        }
        guard abs(dx) <= 1_000_000, abs(dy) <= 1_000_000 else {
            throw ToolError(message: "dx and dy must be within ±1,000,000 canvas pixels.")
        }
        let duplicate = boolArg(a, "duplicate") ?? false
        let name = duplicate ? MoveSelectedPixels.duplicateActionName : MoveSelectedPixels.actionName
        let planeTarget = try paintTarget(a, document)
        let explicit = intArg(a, "layer")
        if planeTarget == .layer, explicit == nil, document.layerSelection.isMultiple {
            throw ToolError(message: MoveSelectedPixels.multiLayerRefusal(
                verb: "move", count: document.layerSelection.count))
        }
        let layer = explicit ?? document.activeLayerIndex
        guard layer >= 0, layer < doc.layerCount else {
            throw ToolError(message: "Layer \(layer) is out of range (0..\(doc.layerCount - 1))")
        }
        var background: PlaneBackground?
        if planeTarget != .layer {
            try rejectPlaneLock(document, layer, planeTarget)
            if let reason = MoveSelectedPixels.planeRefusal(
                document, layer: layer, target: planeTarget, panelSelection: explicit == nil,
                verb: "move", subject: "Moving selected pixels") {
                throw ToolError(message: reason)
            }
            guard TransformSelectedPixels.planeBox(selection) != nil else {
                throw ToolError.selectedPixels(MoveSelectedPixels.emptyRefusal)
            }
            background = try planeBackgroundArg(a, document, target: planeTarget)
        } else {
            guard a["background"] == nil else {
                throw ToolError(message: Self.layerBackgroundRefusal)
            }
            try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
            if let reason = MoveSelectedPixels.refusal(document, layer: layer, target: .layer,
                                                       panelSelection: false) {
                throw ToolError(message: reason)
            }
        }
        // ⌥ (duplicate) over a live float stamps a fresh copy, as the drag does.
        let continuing = duplicate ? nil : document.liveMoveFloat(layer: layer, target: planeTarget)
        if let continuing { background = continuing.float.background }
        var fields: [String: Any] = ["layer": layer, "dx": dx, "dy": dy, "duplicate": duplicate]
        if planeTarget != .layer {
            fields["target"] = planeTarget.agentName(in: doc)
            if planeTarget.isChannel { fields["layer"] = nil }
            if let background, planeTarget != .plane(.alpha) {
                fields["background"] = background.hex
            }
        }
        guard dx != 0 || dy != 0 else {
            return try structureNoOp(fields, why: "dx and dy are both zero, so nothing would move.")
        }
        let history = document.selectionHistory
        let position = continuing.map { $0.float.positions[$0.at] }
        let total = (x: (position?.dx ?? 0) + dx, y: (position?.dy ?? 0) + dy)
        let resulting = continuing.map {
            $0.float.resultingSelection(from: history, dx: total.x, dy: total.y)
        } ?? history.moved(dx: dx, dy: dy)
        let coverage = continuing?.float.coverage ?? selection.maskSamples16()
        let bounds = selection.bounds
        let dropped = continuing == nil && (planeTarget == .layer || planeTarget.isPlane)
            ? Self.description(of: document, layer: layer) : nil
        let lift = { (source: RasterDocument) in
            MoveFloat(source: source, before: doc, selection: history, layer: layer,
                      target: planeTarget, background: background, duplicate: duplicate,
                      coverage: coverage, bounds: bounds)
        }
        do {
            if let continuing, total.x == 0, total.y == 0 {
                // Back at the lift: the float's source itself, one step.
                let source = continuing.float.source
                try performGroupedEdit(document, name, resultingSelection: resulting) {
                    $0 === doc ? source : nil
                }
                document.noteMoveStep(base: doc, continuing: continuing,
                                      lift: { continuing.float }, dx: 0, dy: 0)
            } else if doc.isHighDepth {
                let command = try continuing?.float.command(dx: total.x, dy: total.y)
                    ?? MoveSelectedPixels.command(
                        layer: layer, dx: dx, dy: dy, duplicate: duplicate, bounds: bounds,
                        target: planeTarget, background: background)
                let gesture = try PrecisionGeometryGesture(
                    document, workSource: continuing?.float.source)
                defer { gesture.cancel() }
                let changed = try gesture.finish(.moveSelected(command: command, coverage: coverage),
                                                 name: name, record: .notACommand,
                                                 resultingSelection: resulting)
                guard changed else {
                    return try structureNoOp(fields, why: "the move changed no pixel.")
                }
                document.noteMoveStep(base: doc, continuing: continuing, lift: { lift(doc) },
                                      dx: total.x, dy: total.y)
            } else {
                let command = try continuing?.float.command(dx: total.x, dy: total.y)
                    ?? MoveSelectedPixels.command(
                        layer: layer, dx: dx, dy: dy, duplicate: duplicate, bounds: bounds,
                        target: planeTarget, background: background)
                var source = continuing?.float.source ?? doc
                if dropped != nil {
                    guard let plain = doc.withLayerMeta(layer, nil) else {
                        throw ToolError(message: "The layer's description could not be dropped.")
                    }
                    source = plain
                }
                let lifted = source
                // Captured at entry: the helper refuses if another edit
                // replaced the document or the selection meanwhile.
                guard let prepared = try MoveSelectedPixels.prepared(
                    document, source: source, request: command.request, coverage: coverage,
                    title: name, base: doc, selection: history)
                else {
                    return try structureNoOp(fields, why: "the move changed no pixel.")
                }
                document.preparedEdit = prepared
                try performGroupedEdit(document, name,
                                       resultingSelection: resulting) { _ in prepared.document }
                document.noteMoveStep(base: doc, continuing: continuing, lift: { lift(lifted) },
                                      dx: total.x, dy: total.y)
            }
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError.selectedPixels(error.localizedDescription)
        }
        var result = fields
        result["ok"] = true
        if planeTarget == .mask || planeTarget.isChannel {
            return try planeTransformResult(result, document, layer: layer, target: planeTarget)
        }
        if let info = document.doc?.layerInfo(layer) {
            result["layer_bounds"] = ["x": info.offsetX, "y": info.offsetY,
                                      "width": info.width, "height": info.height]
        }
        if let bounds = document.canvasSelection?.bounds {
            result["selection_bounds"] = ["x": Int(bounds.minX), "y": Int(bounds.minY),
                                          "width": Int(bounds.width), "height": Int(bounds.height)]
        } else {
            result["selection_bounds"] = NSNull()
        }
        return try pixelEditResult(result, layer: layer, rasterized: dropped)
    }
}
