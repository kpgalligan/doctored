import AppKit

/// remove_stroke: the agent mirror of the Remove tool. The stroke
/// (or, with `use_selection`, the selection) is rasterised, clipped to the
/// shared selection and filled by `RemoveStroke.start`, the same call the
/// tool's own commit makes (EditorViewController+Remove.removeStrokeCommitted),
/// so a stroke driven from here and the same stroke painted by hand land
/// identical pixels: `mode` "model" (the default) runs the local inpainting
/// model, "content_aware" the Content-Aware Fill. `strokes` mirrors the
/// tool with "Remove after each stroke" off: the held strokes' Apply
/// (EditorViewController+Remove.applyPendingRemove), one hole and one step.
///
/// content_aware_move: the agent mirror of the Content-Aware Move tool.
/// The region is a polygon or the current selection, as `patch_region` takes
/// it, and the plan and the job are `ContentAwareMoveWork.start`'s, the call
/// the tool's commit makes
/// (EditorViewController+ContentAwareMove.contentAwareMoveCommitted).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let contentAwareToolsHandlers: [String: ToolHandler] = [
        "remove_stroke": { $0.removeStroke },
        "content_aware_move": { $0.contentAwareMove },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let contentAwareToolsToolFacts: [String: ToolFacts] = [
        // Both run on a float layer's pixels (`FloatRetouch`).
        "remove_stroke": [.typedTool, .rasterTool, .floatTool, .cmykTool],
        "content_aware_move": [.typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    /// remove_stroke — mirrors the Remove tool's drag
    /// (EditorViewController+Remove.removeStrokeCommitted).
    ///
    /// The stroke is confined to the live selection, as every stroke tool's
    /// is. Like `content_aware_fill`, whose job this is, it fills a layer's
    /// RGB only: with a mask, a colour plane, a channel or Quick Mask
    /// targeted it refuses rather than write somewhere the caller did not
    /// name.
    func removeStroke(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(a, ["size", "seed", "layer"], in: "remove_stroke")
        guard document.planeEditTarget == .layer, !document.quickMaskActive,
              a["target"] == nil || a["target"] as? String == "layer" else {
            throw ToolError(
                message: "remove_stroke fills layer RGB only. Scalar planes, masks, channels "
                    + "and Quick Mask are unavailable.")
        }
        let index = try paintLayerIndex(a, document)
        // The UI refuses the stroke on an adjustment layer with this sentence
        // (removeStrokeCommitted → refuseAdjustmentPixelEdit).
        try rejectAdjustmentPixelEdit(document, index)
        let mode: RemoveMode
        switch stringArg(a, "mode") ?? RemoveMode.model.rawValue {
        case RemoveMode.model.rawValue: mode = .model
        case RemoveMode.contentAware.rawValue: mode = .contentAware
        case let other:
            throw ToolError(message: "mode must be model or content_aware (got \"\(other)\")")
        }
        if a["use_selection"] != nil, boolArg(a, "use_selection") == nil {
            throw ToolError(message: "use_selection must be a boolean.")
        }
        let useSelection = boolArg(a, "use_selection") == true
        let sources = [a["points"] != nil, a["strokes"] != nil, useSelection].filter { $0 }.count
        if sources > 1 {
            throw ToolError(
                message: "remove_stroke takes one of points, strokes or use_selection: the hole "
                    + "is either the stroke, the strokes or the selection.")
        }
        let size = try removeStrokeSize(a, default: 40)
        let strokes: [RemoveSession.Result]
        if useSelection {
            strokes = []
        } else if let raw = a["strokes"] {
            strokes = try removeStrokes(raw, size: size)
        } else {
            strokes = [RemoveSession.Result(points: try parsePoints(a), size: CGFloat(size))]
        }
        let points = strokes.reduce(0) { $0 + $1.points.count }
        var seed: UInt64 = 0
        if let value = a["seed"] {
            guard !Self.isJSONBoolean(value), let parsed = intArg(a, "seed"), parsed >= 0,
                  doubleArg(a, "seed") == Double(parsed) else {
                throw ToolError(message: "seed must be a non-negative integer.")
            }
            seed = UInt64(parsed)
        }
        // Default `current`, Content-Aware Fill's: the tool's "Sample all
        // layers" checkbox is off until it is ticked.
        let choice = try retouchSampling(a, default: .current)
        let output = try retouchOutput(a)
        if a["background"] != nil, boolArg(a, "background") == nil {
            throw ToolError(message: "background must be a boolean.")
        }
        let selection = editor(document)?.agentSelection
        if useSelection, selection == nil {
            throw ToolError(
                message: "use_selection needs a selection: make one first with select_rect, "
                    + "select_ellipse, select_polygon, select_magic_wand or select_subject, or "
                    + "pass points.")
        }
        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        let job = try RemoveStroke.start(
            RemoveRequest(strokes: strokes, choice: choice, output: output, seed: seed,
                          mode: mode, useSelection: useSelection),
            document: document, layer: index, selection: selection,
            record: .notACommand, showProgress: false)
        var applied: [String: Any] = [
            "size": Self.transformNumber(size), "seed": Int(seed), "output": output.rawValue,
            "mode": mode.rawValue, "use_selection": useSelection,
        ]
        applied.merge(choice.report) { _, new in new }
        if a["strokes"] != nil { applied["strokes"] = strokes.count }
        job.details["points"] = points
        job.details["mode"] = mode.rawValue
        job.details["applied"] = applied
        return try finishJobCall(job, tool: "remove_stroke", arguments: a,
            unchanged: { try self.removeNoOpResult(layer: index) })
    }

    /// A stroke width in px, clamped to the tool's own range (its Size field and its `[` `]`
    /// keys clamp to it); `fallback` when absent.
    private func removeStrokeSize(_ a: [String: Any], default fallback: Double) throws -> Double {
        let raw = doubleArg(a, "size") ?? fallback
        guard raw.isFinite else { throw ToolError(message: "size must be a finite number") }
        return min(max(raw, EditorTool.remove.minimumBrushSize), EditorTool.remove.maximumBrushSize)
    }

    /// `strokes`: 1…64 items `{"points": [[x, y], …], "size": number?}`, each `points` read as
    /// the call's own (`parsePoints`), `size` defaulting to the call's.
    private func removeStrokes(_ raw: Any, size: Double) throws -> [RemoveSession.Result] {
        let cap = RemoveSession.maximumPendingStrokes
        guard let items = raw as? [Any], !items.isEmpty, items.count <= cap else {
            throw ToolError(
                message: "strokes must be an array of 1…\(cap) {\"points\": [[x, y], …], "
                    + "\"size\": number} objects")
        }
        return try items.enumerated().map { index, item in
            guard let object = item as? [String: Any] else {
                throw ToolError(message: "strokes[\(index)] must be an object with points")
            }
            do {
                let points = try parsePoints(object)
                let width = try removeStrokeSize(object, default: size)
                return RemoveSession.Result(points: points, size: CGFloat(width))
            } catch let error as ToolError {
                throw ToolError(message: "strokes[\(index)]: \(error.message)")
            }
        }
    }

    /// A stroke whose fill changed nothing: a no-op, not a failure.
    func removeNoOpResult(layer: Int) throws -> String {
        try jsonResult([
            "ok": true, "changed": false, "layer": layer,
            "note": "remove_stroke changed nothing: the stroke never reached layer "
                + "\(layer)'s pixels, or the fill reproduced the pixels already under it. "
                + "No undo step was added.",
        ])
    }

    /// content_aware_move — mirrors the Content-Aware Move tool's drag
    /// (EditorViewController+ContentAwareMove.contentAwareMoveCommitted).
    ///
    /// The region IS the confinement, as it is for `patch_region`: an
    /// explicit polygon replaces the current selection for this edit, and
    /// omitting `points` moves the selection's own region. Layer RGB only,
    /// as `remove_stroke` is.
    func contentAwareMove(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseNullOrBooleanNumbers(
            a, ["offset_x", "offset_y", "seed", "layer"], in: "content_aware_move")
        guard document.planeEditTarget == .layer, !document.quickMaskActive,
              a["target"] == nil || a["target"] as? String == "layer" else {
            throw ToolError(
                message: "content_aware_move edits layer RGB only. Scalar planes, masks, "
                    + "channels and Quick Mask are unavailable.")
        }
        let index = try paintLayerIndex(a, document)
        // The UI refuses the commit on an adjustment layer with this sentence
        // (contentAwareMoveCommitted → refuseAdjustmentPixelEdit).
        try rejectAdjustmentPixelEdit(document, index)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }

        guard let rawX = doubleArg(a, "offset_x"), let rawY = doubleArg(a, "offset_y"),
              rawX.isFinite, rawY.isFinite else {
            throw ToolError(
                message: "content_aware_move requires offset_x and offset_y — how far the "
                    + "region is moved, in canvas px.")
        }
        // `patch_region`'s wall, for its reason: far past any real canvas, and
        // a hard stop before a displacement no rectangle arithmetic can hold.
        guard abs(rawX) <= 100_000, abs(rawY) <= 100_000 else {
            throw ToolError(
                message: "offset_x and offset_y must be within ±100,000 canvas px — got "
                    + "(\(rawX), \(rawY)). A move displaces a region inside the picture.")
        }
        // Whole pixels, like the tool's drag (`PatchSession`'s WHOLE PIXELS note).
        let offset = CGVector(dx: rawX.rounded(), dy: rawY.rounded())
        guard offset.dx != 0 || offset.dy != 0 else {
            throw ToolError(
                message: "content_aware_move needs a non-zero offset: with no displacement "
                    + "there is nowhere to move the region to.")
        }
        let extend: Bool
        switch stringArg(a, "mode") ?? "move" {
        case "move": extend = false
        case "extend": extend = true
        case let other:
            throw ToolError(message: "mode must be move or extend (got \"\(other)\")")
        }
        // Default `all`, the tool's own and Patch's.
        let choice = try retouchSampling(a, default: .all)
        try rejectRetouchOutput(a, "content_aware_move")
        var seed: UInt64 = 0
        if let value = a["seed"] {
            guard !Self.isJSONBoolean(value), let parsed = intArg(a, "seed"), parsed >= 0,
                  doubleArg(a, "seed") == Double(parsed) else {
                throw ToolError(message: "seed must be a non-negative integer.")
            }
            seed = UInt64(parsed)
        }
        if a["background"] != nil, boolArg(a, "background") == nil {
            throw ToolError(message: "background must be a boolean.")
        }
        guard a["points"] != nil || editor(document)?.agentSelection != nil else {
            throw ToolError(
                message: "content_aware_move needs a region: pass points to outline one, or "
                    + "make a selection first with select_rect, select_ellipse, "
                    + "select_polygon, select_magic_wand or select_subject.")
        }
        let (region, source) = try patchRegionArg(
            a, document, width: doc.width, height: doc.height)

        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        // Latched before the edit, as `patch_region` latches it. Always a
        // Destination capture: the pixels land where the region is moved to.
        let captured: PatchEditCapture
        do {
            captured = try PatchEditCapture(document: document, layer: index,
                choice: choice, destination: true, preview: false)
        } catch { throw ContentAwareMoveWork.reworded(error) }
        let job: DocumentJob
        do {
            job = try ContentAwareMoveWork.start(
                capture: captured, result: PatchSession.Result(region: region, offset: offset),
                extend: extend, seed: seed, document: document, record: .notACommand,
                showProgress: false)
        } catch let error as DoctoredCoreError where error.message == PatchEditCapture.missesCanvas {
            throw ToolError(
                message: "The move falls outside the picture: displaced by "
                    + "(\(Int(offset.dx)), \(Int(offset.dy))) px, the region does not land on "
                    + "the canvas.")
        }
        var applied: [String: Any] = ["seed": Int(seed)]
        applied.merge(choice.report) { _, new in new }
        job.details = ["region": source, "mode": extend ? "extend" : "move",
            "offset": ["x": Int(offset.dx), "y": Int(offset.dy)],
            "moved_bounds": Self.patchBounds(region, offset), "applied": applied]
        return try finishJobCall(job, tool: "content_aware_move", arguments: a,
            unchanged: { try self.contentAwareMoveNoOpResult(layer: index) })
    }

    /// A move that changed nothing: a no-op, not a failure, at both depths.
    func contentAwareMoveNoOpResult(layer: Int) throws -> String {
        try jsonResult([
            "ok": true, "changed": false, "layer": layer,
            "note": "content_aware_move changed nothing: neither the region nor the place "
                + "it was moved to reached layer \(layer)'s pixels, or both already held "
                + "the pixels the move would write. No undo step was added.",
        ])
    }
}
