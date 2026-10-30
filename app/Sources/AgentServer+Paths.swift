import AppKit

/// The path tools:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `listPaths` | the Paths panel's rows |
/// | `setPath` | drawing or editing a path with the Pen tool (one whole-path step per gesture) |
/// | `renamePath` | double-clicking a row in the Paths panel (renaming the work path saves it) |
/// | `duplicatePath` | Paths panel ▸ Duplicate Path |
/// | `deletePath` | Paths panel ▸ Delete Path |
/// | `pathToSelection` | Paths panel ▸ Make Selection… |
/// | `fillPath` | Paths panel ▸ Fill Path |
/// | `strokePath` | Paths panel ▸ Stroke Path |
/// | `selectionToPath` | Select ▸ Make Work Path… |
/// | `getVectorMask` | the Paths panel's vector-mask row |
/// | `setVectorMask` | Layer ▸ Vector Mask ▸ Current Path / Reveal All, and the Pen editing a vector mask |
/// | `setVectorMaskProperties` | Layer ▸ Vector Mask ▸ Enable / Link |
/// | `removeVectorMask` | Layer ▸ Vector Mask ▸ Delete |
///
/// A VECTOR MASK IS A LAYER MASK WHOSE COVERAGE IS A SHAPE'S FILL, kept
/// editable. The layer's ordinary mask is the shape baked together with the
/// raster mask the layer had or is later given (`core/src/doc_vector_mask.rs`),
/// so every other tool reads and composites it as the mask it is; the mask
/// tools edit the raster half, these four edit the shape and its two flags.
///
/// A PATH IS NAMED, NEVER INDEXED. `path` is a saved path's name or
/// "Work Path", the document's one unsaved scratch path; the list reorders
/// when a work path appears, and a name is what a recorded step can still
/// find on another document. Every operation tool takes `path` OR an explicit
/// shape (`subpaths` with an optional `fill_rule`), so a model can fill or
/// stroke an outline it computed without storing it first. There is no
/// `select_path`: which path the Pen is editing is view state.
///
/// COORDINATES ARE ABSOLUTE CANVAS PIXELS, y down from the top-left, handles
/// included, and a path may lie partly or wholly outside the canvas.
///
/// THE LIST EDITS ARE `applyPathEdit` INSIDE `withUndoGroupDiscardingEmpty` —
/// `AgentServer+Guides.editGuides`' shape, for its reasons: a path changes no
/// pixel, so the edit must not reach the reprojecting `applyEdit`, and an undo
/// registration off the event path needs an explicit group or three calls
/// collapse into one undo step. The core's answer is read FIRST, so its
/// refusal sentence (a duplicate name, a full list, too many anchors) is the
/// tool's error and an unchanged list opens no group.
///
/// `fill_path` AND `stroke_path` RUN ANOTHER TOOL'S COMMIT, and the dispatcher
/// asked the depth and retained-source gates under THEIR names only. So each
/// asks `DocumentPrecision.refusal` again under the tool it runs —
/// `stroke_selection` for the fill (its band branch, the band being the
/// path's coverage inside the selection), `brush_stroke` / `eraser_stroke`
/// for the stroke — before anything is painted.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let pathsHandlers: [String: ToolHandler] = [
        "list_paths": { $0.listPaths },
        "set_path": { $0.setPath },
        "rename_path": { $0.renamePath },
        "duplicate_path": { $0.duplicatePath },
        "delete_path": { $0.deletePath },
        "path_to_selection": { $0.pathToSelection },
        "fill_path": { $0.fillPath },
        "stroke_path": { $0.strokePath },
        "selection_to_path": { $0.selectionToPath },
        "get_vector_mask": { $0.getVectorMask },
        "set_vector_mask": { $0.setVectorMask },
        "set_vector_mask_properties": { $0.setVectorMaskProperties },
        "remove_vector_mask": { $0.removeVectorMask },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let pathsToolFacts: [String: ToolFacts] = [
        // Paths are document state with a typed twin (`precision_path`), read
        // and written through the same `DoctoredCore+Paths` wrappers at every
        // depth, 32-bit float included.
        "list_paths": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "set_path": [.commonTool, .floatTool, .cmykTool],
        "rename_path": [.commonTool, .floatTool, .cmykTool],
        "duplicate_path": [.commonTool, .floatTool, .cmykTool],
        "delete_path": [.commonTool, .floatTool, .cmykTool],
        "selection_to_path": [.commonTool, .floatTool, .cmykTool],
        "path_to_selection": [.selectionSensitive, .commonTool, .floatTool, .cmykTool],
        // Admitted at float here and asked again as `stroke_selection`
        // (`fillPath`), whose float rules are Fill… and Stroke…'s.
        // #1: on a CMYK document asked again as `fill_selection` (ink on a layer).
        "fill_path": [.selectionSensitive, .commonTool, .floatTool, .cmykTool],
        // Admitted at float here and asked again as `brush_stroke` /
        // `eraser_stroke` (`strokePath`), whose float rules are `FloatPaint`'s:
        // `rgba32` is the Brush's linear ink on a float layer.
        // #1: on a CMYK document asked again as the Brush's `brush_stroke` (ink on a layer).
        "stroke_path": [.selectionSensitive, .commonTool, .floatTool, .cmykTool],
        // Vector masks are baked u16 (or u8) mask planes at every depth, and
        // `DoctoredCore+Paths` reaches the byte or the typed core behind one
        // call, so one handler serves 8, 16 and 32 bits.
        "get_vector_mask": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "set_vector_mask": [.commonTool, .floatTool, .cmykTool],
        "set_vector_mask_properties": [.commonTool, .floatTool, .cmykTool],
        "remove_vector_mask": [.commonTool, .floatTool, .cmykTool],
    ]

    /// The flatness, in pixels, of the polyline `stroke_path` hands the
    /// brush: a quarter pixel is below what a 1 px brush can show.
    private static let strokeFlatness = 0.25

    // MARK: - The list

    func listPaths(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        return try jsonResult([
            "ok": true, "paths": Self.pathFields(doc.paths), "max_paths": RasterDocument.maxPaths,
        ])
    }

    /// One row per path, in the core's order (the work path first).
    static func pathFields(_ set: PathSet) -> [[String: Any]] {
        set.entries.map { entry in
            [
                "name": entry.name, "kind": entry.kind.rawValue,
                "fill_rule": entry.shape.fillRule.rawValue,
                "anchors": entry.shape.anchorCount,
                "subpaths": entry.shape.subpathsJSON,
            ]
        }
    }

    /// The Pen's commit: one path created or replaced whole. With no `path`
    /// and no `name` it is the work path; `path` (or `name` alone) names a
    /// saved path, created when the document has none of that name — which is
    /// what lets a recorded step replay on a fresh document; `name` beside
    /// `path` renames it in the same step. One undo step.
    func setPath(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard Self.present(a["subpaths"]) else {
            throw ToolError(
                message: "set_path requires subpaths: [{\"closed\": bool, \"anchors\": [[x, y] or "
                    + "{\"x\", \"y\", \"in\": [x, y], \"out\": [x, y], \"smooth\": bool}, …]}, …] "
                    + "in canvas pixels. Pass an empty array for an empty path.")
        }
        let shape = try explicitShape(a)
        let named = try nameArgument(a, "path")
        let renamed = try nameArgument(a, "name")
        let name = named ?? renamed ?? PathSet.workPathName
        var set = doc.paths
        let created: Bool
        if name == PathSet.workPathName {
            created = set.workPath == nil
            set = set.settingWorkPath(shape)
        } else if let index = set.entries.firstIndex(where: { $0.name == name }) {
            created = false
            set.entries[index].shape = shape
        } else {
            created = true
            set.entries.append(PathEntry(id: 0, name: name, kind: .saved, shape: shape))
        }
        var finalName = name
        if named != nil, let renamed, renamed != name,
           let index = set.entries.firstIndex(where: { $0.name == name }) {
            set.entries[index].name = renamed
            set.entries[index].kind = .saved
            finalName = renamed
        }
        let actionName = !created ? "Edit Path"
            : name == PathSet.workPathName ? "New Work Path" : "New Path"
        let fields: [String: Any] = [
            "path": finalName, "created": created, "anchors": shape.anchorCount,
            "subpaths": shape.subpaths.count,
        ]
        guard try editPaths(document, actionName, set) else {
            return try pathNoOp(fields, why: "\"\(finalName)\" already is that path.")
        }
        return try pathResult(document, fields)
    }

    /// Double-clicking a row. Renaming the work path saves it.
    func renamePath(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var set = doc.paths
        let index = try pathIndex(a, set, tool: "rename_path")
        guard let name = try nameArgument(a, "name") else {
            throw ToolError(message: "rename_path requires name — the path's new name.")
        }
        let old = set.entries[index].name
        let saved = set.entries[index].kind == .work
        set.entries[index].name = name
        set.entries[index].kind = .saved
        let fields: [String: Any] = ["path": name, "renamed_from": old, "saved": saved]
        guard try editPaths(document, saved ? "Save Path" : "Rename Path", set) else {
            return try pathNoOp(fields, why: "the path is already named \"\(name)\".")
        }
        return try pathResult(document, fields)
    }

    /// Paths panel ▸ Duplicate Path: a new saved path with the same shape,
    /// named `name` or "<name> copy" (numbered until it is free).
    func duplicatePath(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var set = doc.paths
        let index = try pathIndex(a, set, tool: "duplicate_path")
        let source = set.entries[index]
        var name = try nameArgument(a, "name") ?? "\(source.name) copy"
        if !Self.present(a["name"]) {
            var number = 2
            let base = name
            while set.entry(named: name) != nil {
                name = "\(base) \(number)"
                number += 1
            }
        }
        set.entries.append(PathEntry(id: 0, name: name, kind: .saved, shape: source.shape))
        let fields: [String: Any] = ["path": name, "duplicated_from": source.name]
        guard try editPaths(document, "Duplicate Path", set) else {
            return try pathNoOp(fields, why: "the path could not be duplicated.")
        }
        return try pathResult(document, fields)
    }

    /// Paths panel ▸ Delete Path.
    func deletePath(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var set = doc.paths
        let index = try pathIndex(a, set, tool: "delete_path")
        let removed = set.entries.remove(at: index)
        let fields: [String: Any] = ["deleted": removed.name]
        guard try editPaths(document, "Delete Path", set) else {
            return try pathNoOp(fields, why: "the path could not be deleted.")
        }
        return try pathResult(document, fields)
    }

    // MARK: - Selection

    /// Paths panel ▸ Make Selection…: the path's fill becomes the selection,
    /// combined by `mode`, feathered by `feather`, hard-edged with
    /// `anti_alias: false`. One selection undo step.
    func pathToSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["feather", "layer"], in: "path_to_selection")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let shape = try pathArgument(a, doc, tool: "path_to_selection")
        try refuseEmpty(shape, "select")
        let mode = try selectionMode(a)
        var feather = 0.0
        if Self.present(a["feather"]) {
            guard let value = doubleArg(a, "feather"), value.isFinite, (0...250).contains(value)
            else { throw ToolError(message: "feather must be a number from 0 to 250 (pixels).") }
            feather = value
        }
        try precisionToolErrors {
            let capture = try SelectionCapture(document: document, needsSource: false)
            let coverage = try RasterPathCore.coverage(
                shape, width: doc.width, height: doc.height,
                antiAlias: boolArg(a, "anti_alias") ?? true, clip: nil, control: nil)
            try capture.commit(
                .mask16(coverage), in: document, name: "Make Selection", mode: mode,
                feather: feather)
        }
        var result = selectionHistorySummary(document)
        if document.canvasSelection == nil {
            result["note"] = "The path's fill left nothing selected on this canvas."
        }
        return try jsonResult(result)
    }

    /// Select ▸ Make Work Path…: the selection's half-coverage outline traced
    /// into the work path, which it replaces. One undo step.
    func selectionToPath(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["tolerance"], in: "selection_to_path")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var tolerance = 2.0
        if Self.present(a["tolerance"]) {
            guard let value = doubleArg(a, "tolerance"), value.isFinite, (0.5...10).contains(value)
            else {
                throw ToolError(
                    message: "tolerance must be a number from 0.5 to 10 — how far, in pixels, the "
                        + "path may stray from the selection's edge (default 2).")
            }
            tolerance = value
        }
        let traced: PathShape = try precisionToolErrors {
            let capture = try SelectionCapture(document: document, needsSource: false)
            guard let selection = capture.selection.selection else {
                throw ToolError(
                    message: "selection_to_path needs a selection: nothing is selected.")
            }
            _ = try RasterResources.shared.admit(scratch: UInt64(doc.width * doc.height) * 2)
            let shape = try RasterPathCore.traced(
                selection.maskSamples16(), width: doc.width, height: doc.height,
                tolerance: tolerance, control: nil)
            try capture.requireCurrent(in: document)
            return shape
        }
        guard !traced.isEmpty else {
            throw ToolError(
                message: "The selection has no area at 50% coverage or more that is larger than "
                    + "the tolerance, so there is no outline to trace. Lower tolerance, or select "
                    + "more.")
        }
        let fields: [String: Any] = [
            "path": PathSet.workPathName, "tolerance": tolerance,
            "anchors": traced.anchorCount, "subpaths": traced.subpaths.count,
        ]
        guard try editPaths(document, "Make Work Path", doc.paths.settingWorkPath(traced)) else {
            return try pathNoOp(fields, why: "the work path already is that outline.")
        }
        return try pathResult(document, fields)
    }

    // MARK: - Fill and stroke

    /// Paths panel ▸ Fill Path: `stroke_selection`'s band commit with the
    /// path's anti-aliased fill, inside the selection when there is one, as
    /// the band. One undo step, "Fill Path".
    func fillPath(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["opacity", "layer"], in: "fill_path")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // The dispatcher asked the gates as `fill_path`; this runs
        // `stroke_selection`'s commit, so its gate answers too.
        if let reason = DocumentPrecision.refusal(
            tool: "stroke_selection", arguments: a, document: document) {
            throw ToolError(message: reason)
        }
        let shape = try pathArgument(a, doc, tool: "fill_path")
        try refuseEmpty(shape, "fill")
        let outside = { (layer: Int, target: String) in
            try self.jsonResult([
                "ok": true, "changed": false, "layer": layer, "target": target,
                "note": "Nothing was filled: the path covers no pixel of the canvas"
                    + (document.canvasSelection == nil ? "." : " inside the selection.")
                    + " No undo step was added.",
            ])
        }
        if doc.isHighDepth {
            let (index, planeTarget, capture) = try precisionSelectionPaintCapture(
                a, document, tool: "fill_path")
            // #1: a CMYK layer takes `ink` (C, M, Y, K percent) in place of `color`, as
            // `fill_selection` does (`precisionFillSelection`).
            let ink = capture.source.isCmyk && !planeTarget.isCoverage ? try cmykInkArgument(a, "ink") : nil
            var arguments = a
            if ink != nil, arguments["color"] == nil { arguments["color"] = "#000000" }
            let paint = try selectionPaintArguments(
                arguments, document, coverage: planeTarget.isCoverage, tool: "fill_path")
            let band = try precisionToolErrors {
                try RasterPathCore.coverage(
                    shape, width: doc.width, height: doc.height, antiAlias: true,
                    clip: capture.selection?.maskSamples16(), control: nil)
            }
            guard band.contains(where: { $0 != 0 }) else {
                return try outside(index, planeTarget.agentName(in: capture.facade))
            }
            var request = PrecisionFillStroke.Request(
                layer: index, target: planeTarget, color: paint.color, mode: paint.mode,
                opacity: paint.opacity, preserve: paint.preserve, linear: paint.linear)
            request.cmykInk = ink
            return try commitPrecisionPaintBand(
                document, capture: capture, request: request, band: band,
                actionName: "Fill Path", noOp: FillSettings.transparentNoOp, clippedNoOp: nil,
                extra: [:])
        }
        let index = try structuralLayerIndex(a, document)
        let planeTarget = try paintTarget(a, document)
        let paint = try selectionPaintArguments(
            a, document, coverage: planeTarget.isCoverage, tool: "fill_path")
        let capture = try operationCapture(
            document, operation: .fill, target: planeTarget, layer: index)
        // The byte selection widened to the path coverage's 16 bits, so the
        // product is rounded once, on the way back to bytes.
        let clip = capture.selection.map { $0.map { UInt16($0) * 257 } }
        let coverage = try precisionToolErrors {
            try RasterPathCore.coverage(
                shape, width: doc.width, height: doc.height, antiAlias: true, clip: clip,
                control: nil)
        }
        let band = coverage.map { UInt8((UInt32($0) * 255 + 32767) / 65535) }
        guard band.contains(where: { $0 != 0 }) else {
            return try outside(index, capture.target.agentName(in: capture.doc))
        }
        return try commitPaintBand(
            document, capture: capture, rgba: paint.rgba, mode: paint.mode,
            opacity: paint.opacity, preserve: paint.preserve, band: band,
            actionName: "Fill Path", noOp: FillSettings.transparentNoOp, clippedNoOp: nil,
            extra: [:])
    }

    /// The arguments `stroke_path` hands its inner stroke; the colour keys go
    /// to the brush only.
    private static let strokeKeys = [
        "size", "hardness", "flow", "spacing", "angle", "roundness", "opacity", "layer", "target",
        "document_id",
    ]
    /// `ink` is a CMYK layer's Brush colour (#1), C, M, Y, K percent.
    private static let brushOnlyKeys = ["color", "color_space", "blend_mode", "rgba32", "ink"]
    /// Stroke arguments that describe ONE recorded stroke's own geometry or
    /// timing; a path stroke takes its geometry from the path.
    private static let foreignStrokeKeys = [
        "points", "stroke_version", "native_stroke", "pressures", "times", "duration",
        "pressure_size", "pressure_opacity", "airbrush",
    ]

    /// Paths panel ▸ Stroke Path: the Brush or the Eraser run along every
    /// subpath, as ONE undo step, "Stroke Path". Nothing is painted until
    /// every check has passed, in this order: the inner tool's own depth and
    /// retained-source gate, then every subpath flattened and held to one
    /// stroke's limits (10,000 points, ±100,000 px) — so a path the model
    /// accepted but a stroke cannot carry is refused by subpath index before
    /// the first dab.
    func strokePath(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(
            a, ["size", "hardness", "flow", "spacing", "angle", "roundness", "opacity", "layer"],
            in: "stroke_path")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let erase: Bool
        switch stringArg(a, "tool") ?? "brush" {
        case "brush": erase = false
        case "eraser": erase = true
        default: throw ToolError(message: "tool must be \"brush\" (default) or \"eraser\".")
        }
        let inner = erase ? "eraser_stroke" : "brush_stroke"
        if let key = Self.foreignStrokeKeys.first(where: { Self.present(a[$0]) }) {
            throw ToolError(
                message: "stroke_path takes no \(key): the stroke follows the path's own outline "
                    + "at one pressure. Its stroke arguments are size, hardness, flow, spacing, "
                    + "angle, roundness, opacity, color, color_space, rgba32, ink, blend_mode, layer and "
                    + "target.")
        }
        if erase, let key = Self.brushOnlyKeys.first(where: { Self.present(a[$0]) }) {
            throw ToolError(
                message: "stroke_path with tool \"eraser\" takes no \(key): erasing removes "
                    + "alpha. Omit it, or use tool \"brush\".")
        }
        // (a) The inner call's arguments, minus the points.
        var arguments: [String: Any] = [:]
        for key in Self.strokeKeys + (erase ? [] : Self.brushOnlyKeys) where Self.present(a[key]) {
            arguments[key] = a[key]
        }
        // (b) The dispatcher asked the gates as `stroke_path`; the pixels are
        // the inner tool's to paint, so its gate answers too.
        if let reason = DocumentPrecision.refusal(
            tool: inner, arguments: arguments, document: document) {
            throw ToolError(message: reason)
        }
        // (c) Every subpath as one stroke's points, all checked before any.
        let shape = try pathArgument(a, doc, tool: "stroke_path")
        try refuseEmpty(shape, "stroke")
        let polylines = try precisionToolErrors {
            try RasterPathCore.flattened(shape, tolerance: Self.strokeFlatness)
        }
        var strokes: [[[Double]]] = []
        for (index, polyline) in polylines.enumerated() {
            var points = polyline.points.map { [Double($0.x), Double($0.y)] }
            // A closed subpath returns to where it began.
            if polyline.closed, points.count > 1, let first = points.first { points.append(first) }
            do {
                _ = try parsePoints(["points": points])
            } catch let error as ToolError {
                throw ToolError(message: "stroke_path, subpath \(index): \(error.message)")
            }
            strokes.append(points)
        }
        // (d) One inner call per subpath, as one undo step.
        let before = document.doc
        var replies: [String] = []
        var failure: Error?
        document.withReplayUndo("Stroke Path") {
            for points in strokes {
                var call = arguments
                call["points"] = points
                do {
                    if let reply = try dispatchPrecision(inner, call) {
                        replies.append(reply)
                    } else if let handler = Self.handlers[inner] {
                        replies.append(try handler(self)(call))
                    } else {
                        throw ToolError(message: "Unknown tool: \(inner)")
                    }
                } catch {
                    failure = error
                    return
                }
            }
        }
        if let failure { throw failure }
        // The first stroke's reply names the layer, the target and anything
        // rasterized on the way; the rest repeat it.
        var result = replies.first.map(Self.replyFields) ?? [:]
        result["ok"] = true
        result["action"] = "Stroke Path"
        result["changed"] = document.doc !== before
        result["tool"] = erase ? "eraser" : "brush"
        result["subpaths"] = strokes.count
        result["points"] = strokes.reduce(0) { $0 + $1.count }
        return try jsonResult(result)
    }

    // MARK: - Vector masks

    /// What `get_vector_mask` and the three mutators report of a layer's
    /// vector mask.
    static func vectorMaskFields(_ mask: VectorMaskInfo, layer: Int) -> [String: Any] {
        [
            "ok": true, "layer": layer, "vector_mask": true, "enabled": mask.enabled,
            "linked": mask.linked, "reveal_all": mask.shape.isEmpty,
            "has_raster_mask": mask.hasRaster, "raster_mask_enabled": mask.rasterEnabled,
            "fill_rule": mask.shape.fillRule.rawValue, "anchors": mask.shape.anchorCount,
            "subpaths": mask.shape.subpathsJSON,
        ]
    }

    private static func noVectorMask(_ index: Int) -> String {
        "Layer \(index) has no vector mask. Give it one with set_vector_mask."
    }

    /// The Paths panel's vector-mask row, and Layer ▸ Vector Mask's state.
    func getVectorMask(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "get_vector_mask")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try structuralLayerIndex(a, document)
        guard let mask = doc.vectorMask(index) else {
            return try jsonResult([
                "ok": true, "layer": index, "vector_mask": false,
                "note": Self.noVectorMask(index),
            ])
        }
        return try jsonResult(Self.vectorMaskFields(mask, layer: index))
    }

    /// Layer ▸ Vector Mask ▸ Current Path / Reveal All, and the Pen's commit
    /// while it edits a vector mask: the layer's vector mask created, or its
    /// shape replaced. One undo step.
    func setVectorMask(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "set_vector_mask")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // A GROUP and an adjustment layer take a vector mask as they take a
        // layer mask, so this is the structural lookup.
        let index = try structuralLayerIndex(a, document)
        try rejectLockedEdit(document, index, DR_EDIT_MASK)
        let shape: PathShape
        if Self.present(a["reveal_all"]) {
            guard let reveal = boolArg(a, "reveal_all") else {
                throw ToolError(message: "reveal_all must be a boolean.")
            }
            guard reveal, !Self.present(a["path"]), !Self.present(a["subpaths"]) else {
                throw ToolError(
                    message: "Pass exactly one of path, subpaths or reveal_all: true.")
            }
            shape = PathShape()
        } else {
            guard Self.present(a["path"]) || Self.present(a["subpaths"]) else {
                throw ToolError(
                    message: "set_vector_mask requires path — a saved path's name, or \"Work "
                        + "Path\" — or subpaths (with an optional fill_rule), or reveal_all: "
                        + "true for a vector mask that hides nothing yet.")
            }
            shape = try pathArgument(a, doc, tool: "set_vector_mask")
        }
        let existed = doc.vectorMask(index) != nil
        // Asked first, so the core's sentence (the anchor cap, a lock at high
        // depth) is the tool's error and an unchanged mask opens no group.
        guard let updated = try precisionToolErrors({
            try doc.settingVectorMask(index, shape: shape, control: nil)
        }) else {
            guard let mask = doc.vectorMask(index) else {
                throw ToolError(message: "Set Vector Mask failed — check the parameters")
            }
            return try jsonResult(
                Self.vectorMaskFields(mask, layer: index).merging([
                    "changed": false,
                    "note": "Nothing changed: layer \(index)'s vector mask already has that "
                        + "shape. No undo step was added.",
                ]) { _, new in new })
        }
        try performGroupedEdit(document, existed ? "Edit Vector Mask" : "Add Vector Mask") { _ in
            updated
        }
        guard let mask = document.doc?.vectorMask(index) else {
            throw ToolError(message: "Set Vector Mask failed — check the parameters")
        }
        return try jsonResult(
            Self.vectorMaskFields(mask, layer: index).merging([
                "changed": true, "created": !existed,
            ]) { _, new in new })
    }

    /// Layer ▸ Vector Mask ▸ Enable and ▸ Link. One undo step.
    func setVectorMaskProperties(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "set_vector_mask_properties")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try structuralLayerIndex(a, document)
        for key in ["enabled", "linked"] where Self.present(a[key]) && boolArg(a, key) == nil {
            throw ToolError(message: "\(key) must be a boolean.")
        }
        guard Self.present(a["enabled"]) || Self.present(a["linked"]) else {
            throw ToolError(
                message: "set_vector_mask_properties: nothing to change. Pass enabled, linked "
                    + "or both.")
        }
        guard let mask = doc.vectorMask(index) else {
            throw ToolError(message: Self.noVectorMask(index))
        }
        try rejectLockedEdit(document, index, DR_EDIT_MASK)
        let enabled = boolArg(a, "enabled") ?? mask.enabled
        let linked = boolArg(a, "linked") ?? mask.linked
        guard let updated = try precisionToolErrors({
            try doc.settingVectorMaskProperties(
                index, enabled: enabled, linked: linked, control: nil)
        }) else {
            return try jsonResult(
                Self.vectorMaskFields(mask, layer: index).merging([
                    "changed": false,
                    "note": "Nothing changed: the vector mask already has those flags. No undo "
                        + "step was added.",
                ]) { _, new in new })
        }
        try performGroupedEdit(document, "Vector Mask Properties") { _ in updated }
        guard let after = document.doc?.vectorMask(index) else {
            throw ToolError(message: "Vector Mask Properties failed — check the parameters")
        }
        return try jsonResult(
            Self.vectorMaskFields(after, layer: index).merging(["changed": true]) { _, new in new })
    }

    /// Layer ▸ Vector Mask ▸ Delete: the raster mask kept beside the shape,
    /// if any, is the layer's mask again. One undo step.
    func removeVectorMask(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "remove_vector_mask")
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let index = try structuralLayerIndex(a, document)
        guard let mask = doc.vectorMask(index) else {
            throw ToolError(message: Self.noVectorMask(index))
        }
        try rejectLockedEdit(document, index, DR_EDIT_MASK)
        guard let updated = try precisionToolErrors({ try doc.removingVectorMask(index) }) else {
            throw ToolError(message: "Delete Vector Mask failed — check the parameters")
        }
        try performGroupedEdit(document, "Delete Vector Mask") { _ in updated }
        return try jsonResult([
            "ok": true, "changed": true, "layer": index, "vector_mask": false,
            "raster_mask_kept": mask.hasRaster,
            "has_mask": document.doc?.layerHasMask(index) ?? false,
        ])
    }

    // MARK: - Shared pieces

    /// The fields of a handler's reply: `jsonResult` wraps them as the text
    /// of a tool result, and this takes them back out. Empty for anything
    /// else.
    private static func replyFields(_ reply: String) -> [String: Any] {
        guard let result = (try? JSONSerialization.jsonObject(with: Data(reply.utf8)))
                as? [String: Any],
              let content = result["content"] as? [[String: Any]],
              let text = content.first?["text"] as? String,
              let fields = (try? JSONSerialization.jsonObject(with: Data(text.utf8)))
                as? [String: Any]
        else { return [:] }
        return fields
    }

    /// One path-list edit as one undo step, off the event path. false when
    /// the core answered "the same list" — nothing changed, no group opened.
    /// The core's refusal sentence is thrown as the tool's error.
    private func editPaths(
        _ document: ImageDocument, _ actionName: String, _ next: PathSet
    ) throws -> Bool {
        guard let current = document.doc else {
            throw ToolError(message: "Document has no image")
        }
        // Asked first, so a refused or unchanged list never reaches
        // `applyGuideEdit`, whose own refusals beep at a user who is not
        // there and present the error in a sheet.
        guard try precisionToolErrors({ try current.withPaths(next) }) != nil else { return false }
        document.withUndoGroupDiscardingEmpty {
            document.applyPathEdit(actionName, record: .notACommand) { _ in next }
        }
        guard document.doc !== current else {
            throw ToolError(
                message: "A canvas gesture is in progress; try again when it ends.")
        }
        return true
    }

    /// A mutator's reply: its own fields, and the list's size after it.
    private func pathResult(_ document: ImageDocument, _ fields: [String: Any]) throws -> String {
        try jsonResult(
            fields.merging([
                "ok": true, "changed": true, "paths": document.doc?.paths.entries.count ?? 0,
            ]) { _, shared in shared })
    }

    /// The in-band "nothing changed" answer (`AgentServer+Guides.noOpResult`'s
    /// shape): never an error, no undo step.
    private func pathNoOp(_ fields: [String: Any], why: String) throws -> String {
        try jsonResult(
            fields.merging([
                "ok": true, "changed": false,
                "note": "Nothing changed: \(why) No undo step was added.",
            ]) { _, shared in shared })
    }

    /// Whether an argument was passed at all (JSON null is "not passed").
    private static func present(_ value: Any?) -> Bool {
        value != nil && !(value is NSNull)
    }

    /// A name argument: nil when absent, refused when it is not a string.
    private func nameArgument(_ a: [String: Any], _ key: String) throws -> String? {
        guard Self.present(a[key]) else { return nil }
        guard let name = a[key] as? String else {
            throw ToolError(message: "\(key) must be a string — a path's name.")
        }
        return name
    }

    /// The index of the path `path` names, which is required.
    private func pathIndex(_ a: [String: Any], _ set: PathSet, tool: String) throws -> Int {
        guard let name = try nameArgument(a, "path") else {
            throw ToolError(
                message: "\(tool) requires path — a saved path's name, or \"Work Path\".")
        }
        guard let index = set.entries.firstIndex(where: { $0.name == name }) else {
            throw ToolError(message: Self.noSuchPath(name, set))
        }
        return index
    }

    private static func noSuchPath(_ name: String, _ set: PathSet) -> String {
        guard !set.entries.isEmpty else {
            return "This document has no paths, so \"\(name)\" names nothing. Create one with "
                + "set_path or selection_to_path."
        }
        let names = set.entries.map { "\"\($0.name)\"" }.joined(separator: ", ")
        return "No path is named \"\(name)\". This document's paths: \(names)."
    }

    /// The `subpaths` and `fill_rule` arguments as a shape. The caps and the
    /// coordinate wall are the core's, refused with its sentence when the
    /// shape is used.
    private func explicitShape(_ a: [String: Any]) throws -> PathShape {
        do {
            return try PathShape(subpathsJSON: a["subpaths"], fillRuleJSON: a["fill_rule"])
        } catch let error as PathFormatError {
            throw ToolError(message: error.message)
        }
    }

    /// The shape an operation tool works on: `path` (a saved path's name or
    /// "Work Path") or an explicit `subpaths` with an optional `fill_rule` —
    /// exactly one of the two.
    private func pathArgument(
        _ a: [String: Any], _ doc: RasterDocument, tool: String
    ) throws -> PathShape {
        let named = try nameArgument(a, "path")
        switch (named, Self.present(a["subpaths"])) {
        case (.some, true):
            throw ToolError(message: "Pass path or subpaths, not both.")
        case (nil, false):
            throw ToolError(
                message: "\(tool) requires path — a saved path's name, or \"Work Path\" — or "
                    + "subpaths (with an optional fill_rule) for a shape that is not stored.")
        case (let name?, false):
            guard !Self.present(a["fill_rule"]) else {
                throw ToolError(
                    message: "fill_rule goes with subpaths; a stored path keeps its own rule.")
            }
            let set = doc.paths
            guard let entry = set.entry(named: name) else {
                throw ToolError(message: Self.noSuchPath(name, set))
            }
            return entry.shape
        case (nil, true):
            return try explicitShape(a)
        }
    }

    private func refuseEmpty(_ shape: PathShape, _ verb: String) throws {
        guard shape.isEmpty else { return }
        throw ToolError(message: "The path is empty: it has no subpath to \(verb).")
    }
}
