import AppKit

// MCP `transform_selected_pixels` takes `transform_layer`'s arguments around the box, or `corners`.

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let transformSelectedHandlers: [String: ToolHandler] = [
        "transform_selected_pixels": { $0.transformSelectedPixels },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let transformSelectedToolFacts: [String: ToolFacts] = [
        // Free Transform of the selected pixels, `transform_selected`.
        // Free Transform of the selected area of a mask or alpha
        // channel — u16 planes at float. A layer target is admitted at float
        // through the core's float table (`floatCoreRefusal`).
        "transform_selected_pixels": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
    ]

    /// `transform_selected_pixels` — mirrors ⌘T with a selection active
    /// (EditorViewController+FloatingTransform.swift): the layer's pixels ×
    /// the selection resample through `transform_layer`'s named parameters
    /// (around the BOX, the float's opaque bounds as ⌘T opens it) or
    /// onto four corners,
    /// the hole is transparent, and the transformed coverage becomes the
    /// selection, in ONE undo step. Described layers rasterize silently and
    /// say so (`pixelEditResult`). `duplicate` mirrors ⌥⌘T, Layer ▸ Free
    /// Transform a Copy: no hole, one "Transform Copy" step. `target`
    /// "mask" or "channel:<name>" mirrors ⌘T with that plane as the
    /// edit target (`beginPlaneTransform`): the box is the selection's
    /// bounds and the vacated area takes `background` (default #000000) as
    /// coverage gray; nothing is rasterized. `target` "red", "green", "blue"
    /// or "alpha" mirrors ⌘T with that colour plane as the edit target:
    /// the selection's bounds as the box, the layer path's lock and silent
    /// rasterize, `background`'s rounded 30/59/11 luma as the R/G/B vacated value
    /// (Alpha's transparent), and the layer path's reply.
    func transformSelectedPixels(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "No image loaded.") }
        guard let selection = document.canvasSelection else {
            throw ToolError(message: "transform_selected_pixels needs an active selection; use "
                + "transform_layer to transform whole layers")
        }
        let planeTarget = try paintTarget(a, document)
        let explicit = intArg(a, "layer")
        if planeTarget == .layer, explicit == nil, document.layerSelection.isMultiple {
            throw ToolError(message: MoveSelectedPixels.multiLayerRefusal(
                verb: "transform", count: document.layerSelection.count))
        }
        let layer = explicit ?? document.activeLayerIndex
        guard layer >= 0, layer < doc.layerCount else {
            throw ToolError(message: "Layer \(layer) is out of range (0..\(doc.layerCount - 1))")
        }
        // Captured BEFORE the float-box wait, so the transform runs on
        // exactly what the box was measured from: the wait services MCP, and
        // an edit that lands during it refuses by name. The 8-bit helper and
        // the 16-bit gesture (captured after this check) refuse again if
        // another edit replaces the document or the selection later.
        let base = doc
        let history = document.selectionHistory
        let coverage = selection.maskSamples16()
        let box: CGRect
        var background: PlaneBackground?
        if planeTarget != .layer {
            let opening = try planeTransformOpening(
                a, document, layer: layer, target: planeTarget, selection: selection,
                panelSelection: explicit == nil)
            box = opening.box
            background = opening.background
        } else {
            guard a["background"] == nil else {
                throw ToolError(message: Self.layerBackgroundRefusal)
            }
            try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
            if let reason = TransformSelectedPixels.refusal(
                document, layer: layer, target: .layer, panelSelection: false) {
                throw ToolError(message: reason)
            }
            guard TransformSelectedPixels.box(doc, layer: layer, selection: selection) != nil else {
                throw ToolError.selectedPixels(TransformSelectedPixels.emptyRefusal)
            }
            let activeLayer = document.activeLayerIndex
            do {
                box = try TransformSelectedPixels.floatBox(
                    base, layer: layer, coverage: coverage,
                    title: TransformSelectedPixels.actionName)
            } catch {
                throw ToolError.selectedPixels(error.localizedDescription)
            }
            guard document.doc === base, document.selectionHistory == history,
                  document.activeLayerIndex == activeLayer else {
                throw ToolError(message: TransformSelectedPixels.staleRefusal)
            }
        }
        let samplerName = (stringArg(a, "sampler") ?? "bicubic").lowercased()
        guard let sampler = Self.transformSamplers[samplerName] else {
            throw ToolError(message: "sampler must be nearest, bilinear, bicubic or lanczos "
                + "(got \"\(samplerName)\")")
        }
        let matrixKeys = ["rotate", "scale", "scale_x", "scale_y", "translate_x", "translate_y",
                          "pivot_x", "pivot_y", "around"]
        let usesMatrix = matrixKeys.contains { a[$0] != nil }
        var transform = LayerTransform(pivot: CGPoint(x: box.midX, y: box.midY))
        var quad: [CGPoint]?
        var applied: [String: Any] = ["sampler": sampler.name]
        if a["corners"] != nil {
            guard !usesMatrix else {
                throw ToolError(message: "Pass either corners or the matrix arguments (rotate, "
                    + "scale, scale_x, scale_y, translate_x, translate_y, pivot_x, pivot_y, "
                    + "around), not both.")
            }
            let corners = try cornerPoints(a, subject: "the box's")
            let unchanged = zip(corners, LayerTransform.rectCorners(box)).allSatisfy {
                abs($0.x - $1.x) <= LayerTransform.cornerEpsilon
                    && abs($0.y - $1.y) <= LayerTransform.cornerEpsilon
            }
            applied["corners"] = corners.map {
                [Self.transformNumber(Double($0.x)), Self.transformNumber(Double($0.y))]
            }
            if unchanged {
                return try structureNoOp(["layer": layer, "applied": applied],
                                         why: "the corners match the box's own corners.")
            }
            guard LayerTransform.isUsableQuad(corners) else {
                throw ToolError(message: "The corners must form a convex quad in the order "
                    + "top-left, top-right, bottom-right, bottom-left: a concave, self-crossing "
                    + "or collapsed arrangement folds the mapping and the core refuses it.")
            }
            quad = corners
        } else {
            let anchor: CGPoint
            switch stringArg(a, "around") ?? "center" {
            case "center": anchor = CGPoint(x: box.midX, y: box.midY)
            case "top_left": anchor = CGPoint(x: box.minX, y: box.minY)
            case let other:
                throw ToolError(message: "around must be \"center\" or \"top_left\" (got "
                    + "\"\(other)\"); pass pivot_x / pivot_y for any other point.")
            }
            let pivot = CGPoint(x: CGFloat(try finiteArg(a, "pivot_x") ?? Double(anchor.x)),
                                y: CGFloat(try finiteArg(a, "pivot_y") ?? Double(anchor.y)))
            let rotate = try finiteArg(a, "rotate")
            let uniformScale = try scaleArg(a, "scale")
            let scaleX = try scaleArg(a, "scale_x")
            let scaleY = try scaleArg(a, "scale_y")
            let translateX = try finiteArg(a, "translate_x")
            let translateY = try finiteArg(a, "translate_y")
            guard rotate != nil || uniformScale != nil || scaleX != nil || scaleY != nil
                || translateX != nil || translateY != nil
            else {
                throw ToolError(message: "transform_selected_pixels: nothing to change — pass "
                    + "corners, or at least one of rotate, scale, scale_x, scale_y, "
                    + "translate_x, translate_y.")
            }
            transform = LayerTransform(pivot: pivot)
            transform.translation = CGVector(dx: CGFloat(translateX ?? 0),
                                             dy: CGFloat(translateY ?? 0))
            if let rotate { transform.degrees = rotate }
            transform.scaleX = scaleX ?? uniformScale ?? 1
            transform.scaleY = scaleY ?? uniformScale ?? 1
            guard transform.isFinite else {
                throw ToolError(message: "Those parameters do not compose a usable transform — "
                    + "every value must be a finite number.")
            }
            applied["rotate"] = Self.transformNumber(transform.degrees)
            applied["scale_x"] = Self.transformNumber(Double(transform.scaleX))
            applied["scale_y"] = Self.transformNumber(Double(transform.scaleY))
            applied["translate_x"] = Self.transformNumber(translateX ?? 0)
            applied["translate_y"] = Self.transformNumber(translateY ?? 0)
            applied["pivot_x"] = Self.transformNumber(Double(pivot.x))
            applied["pivot_y"] = Self.transformNumber(Double(pivot.y))
            if transform.isIdentity {
                return try structureNoOp(["layer": layer, "applied": applied],
                    why: "those parameters leave the selected pixels exactly where they are.")
            }
            let m = transform.matrix
            guard abs(m.a * m.d - m.b * m.c) >= 1e-9 else {
                throw ToolError(message: "The composed matrix is degenerate (its determinant is "
                    + "~0): a scale of 0 or very near it. Use scales away from 0.")
            }
        }
        let duplicate = boolArg(a, "duplicate") ?? false
        var fields: [String: Any] = [
            "layer": layer,
            "box": ["x": Double(box.minX), "y": Double(box.minY),
                    "width": Double(box.width), "height": Double(box.height)],
            "applied": applied,
        ]
        if duplicate { fields["duplicate"] = true }
        if planeTarget != .layer {
            fields["target"] = planeTarget.agentName(in: doc)
            if planeTarget.isChannel { fields["layer"] = nil }
            if let background, planeTarget != .plane(.alpha) {
                fields["background"] = background.hex
            }
        }
        let matrix = transform.matrix
        let name = duplicate
            ? TransformSelectedPixels.duplicateActionName : TransformSelectedPixels.actionName
        let dropped = planeTarget == .layer || planeTarget.isPlane
            ? Self.description(of: document, layer: layer) : nil
        guard try commitSelectedPixelsTransform(
            document, layer: layer, base: base, history: history, coverage: coverage,
            bounds: selection.bounds, box: box, matrix: matrix, quad: quad,
            sampler: sampler.filter, duplicate: duplicate, name: name, target: planeTarget,
            background: background)
        else { return try structureNoOp(fields, why: "the transform changed no pixel.") }
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

    /// The selected-pixels publication `transform_selected_pixels` and
    /// `transform_again` share: the shared `transform_selected` request
    /// over the captured `base`, `history` and `coverage` — through a 16-bit
    /// gesture or the 8-bit prepared edit (a described layer's description
    /// dropped) — as ONE `name` step. True when published (the Transform Again
    /// memory is then stored); false when no pixel changed.
    func commitSelectedPixelsTransform(
        _ document: ImageDocument, layer: Int, base: RasterDocument,
        history: SelectionHistoryState, coverage: [UInt16], bounds: CGRect?, box: CGRect,
        matrix: CGAffineTransform, quad: [CGPoint]?, sampler: DrResizeFilter, duplicate: Bool,
        name: String, target: PaintTarget = .layer, background: PlaneBackground? = nil
    ) throws -> Bool {
        do {
            let gesture = base.isHighDepth ? try PrecisionGeometryGesture(document) : nil
            defer { gesture?.cancel() }
            let command = try TransformSelectedPixels.command(
                layer: layer, box: box, matrix: matrix, quad: quad, sampler: sampler,
                bounds: bounds, duplicate: duplicate, target: target, background: background)
            let transformed = try TransformedSelection(
                source: base, command: command, captured: history, coverage: coverage, box: box,
                matrix: matrix, quad: quad, sampler: sampler)
            if let gesture {
                let changed = try gesture.finish(
                    .transformSelected(command: command, coverage: coverage),
                    name: name, record: .notACommand, transformedSelection: transformed)
                guard changed else { return false }
            } else {
                var source = base
                // A mask or channel transform leaves the layer's pixels and
                // description untouched; a colour plane
                // rewrites the pixels, so its description goes as the
                // layer's does.
                if target == .layer || target.isPlane,
                   Self.description(of: document, layer: layer) != nil {
                    guard let plain = base.withLayerMeta(layer, nil) else {
                        throw ToolError(message: "The layer's description could not be dropped.")
                    }
                    source = plain
                }
                guard let prepared = try MoveSelectedPixels.prepared(
                    document, source: source, request: command.request, coverage: coverage,
                    title: name, base: base, selection: history,
                    stale: TransformSelectedPixels.staleRefusal, transformedSelection: transformed)
                else { return false }
                document.preparedEdit = prepared.edit
                try performGroupedEdit(document, name, resultingSelection: prepared.selection) {
                    _ in prepared.edit.document
                }
            }
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError.selectedPixels(error.localizedDescription)
        }
        // Transform Again's memory: a true perspective stores its corners.
        document.lastFreeTransform = RepeatableTransform(
            matrix: matrix, quad: quad, sourceRect: box, sampler: sampler)
        return true
    }

    static let layerBackgroundRefusal =
        "background applies to a mask, channel, red, green, blue, cyan, magenta, "
        + "yellow or black target only"

    /// The plane target's lock gate: Mask for a mask, the core's kind for a
    /// colour plane (Pixels, Alpha a merge edit), none for a channel.
    func rejectPlaneLock(_ document: ImageDocument, _ layer: Int,
                         _ target: PaintTarget) throws {
        if target == .mask { try rejectLockedEdit(document, layer, DR_EDIT_MASK) }
        if let kind = MoveSelectedPixels.planeLockKind(target) {
            try rejectLockedEdit(document, layer, kind)
        }
    }

    /// A plane target's `background` hex (default #000000) as the vacated
    /// area's colour (`planeBackground`); refused with an Alpha target, whose
    /// hole is transparent.
    func planeBackgroundArg(_ a: [String: Any], _ document: ImageDocument,
                            target: PaintTarget) throws -> PlaneBackground {
        if target == .plane(.alpha), a["background"] != nil {
            throw ToolError(message: Self.layerBackgroundRefusal)
        }
        let hex = stringArg(a, "background") ?? "#000000"
        do {
            return try TransformSelectedPixels.planeBackground(
                hex: hex, target: target, in: document)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// A mask or channel target's opening, shared with
    /// `transform_again`: the lock gate (Mask for a mask, none for a
    /// channel; a colour plane's own kind), `refusal(planes:)`, the
    /// selection's bounds as the box and the `background` hex (default
    /// #000000) as the vacated colour.
    func planeTransformOpening(
        _ a: [String: Any], _ document: ImageDocument, layer: Int, target: PaintTarget,
        selection: CanvasSelection, panelSelection: Bool
    ) throws -> (box: CGRect, background: PlaneBackground) {
        try rejectPlaneLock(document, layer, target)
        if let reason = TransformSelectedPixels.refusal(
            document, layer: layer, target: target, panelSelection: panelSelection,
            planes: true) {
            throw ToolError(message: reason)
        }
        guard let box = TransformSelectedPixels.planeBox(selection) else {
            throw ToolError.selectedPixels(TransformSelectedPixels.emptyRefusal)
        }
        return (box, try planeBackgroundArg(a, document, target: target))
    }

    /// A plane transform's reply — the plane's agent name, the new
    /// selection bounds, and the mask's stored rect or the channel's name in
    /// place of the layer bounds. Nothing is rasterized.
    func planeTransformResult(_ fields: [String: Any], _ document: ImageDocument, layer: Int,
                              target: PaintTarget) throws -> String {
        var result = fields
        if case .channel(let index) = target {
            result["channel"] = document.doc?.channelInfo(index)?.name ?? NSNull()
        } else if let mask = document.doc?.layerMaskProperties(layer) {
            result["mask_bounds"] = ["x": mask.offsetX, "y": mask.offsetY,
                                     "width": mask.width, "height": mask.height]
        }
        if let bounds = document.canvasSelection?.bounds {
            result["selection_bounds"] = ["x": Int(bounds.minX), "y": Int(bounds.minY),
                                          "width": Int(bounds.width), "height": Int(bounds.height)]
        } else {
            result["selection_bounds"] = NSNull()
        }
        return try jsonResult(result)
    }
}
