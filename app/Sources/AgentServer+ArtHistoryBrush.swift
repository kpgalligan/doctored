import AppKit

/// The agent mirror of the Art History Brush: `art_history_brush_stroke` hands the
/// stroke's DABS (canonical `native_stroke`, or the generated walk of `points` at the tip's
/// spacing, as `smudge_stroke` does) with the tool's options (`ArtHistoryOptions`) to the core,
/// exactly as the canvas stroke's final tick does. It paints from `state_id` (from
/// list_history), by default the History panel's source, else the oldest retained state, as
/// `history_brush_stroke` does; an Action records the call without `state_id`
/// (`ActionRecorder.recordAgentCall`), so a replay paints from that document's own source. The
/// typed twin runs as one `DocumentJobs` job on an 8-bit CMYK document; the core refuses every
/// other typed depth by name.
extension AgentServer {
    /// The refusal of a mask, channel or Quick Mask target.
    static let artHistoryTargetRefusal = "The Art History Brush paints a layer's pixels"

    /// Refuses a target that is not a layer's pixels, at every depth.
    private func refuseArtHistoryTarget(_ a: [String: Any], _ document: ImageDocument) throws {
        if let target = stringArg(a, "target"), target != "layer" {
            throw ToolError(message: Self.artHistoryTargetRefusal)
        }
        if document.quickMaskActive { throw ToolError(message: Self.artHistoryTargetRefusal) }
    }

    /// The source state for `layer`, latched as the History Brush latches it,
    /// its sentences naming this tool.
    private func artHistorySource(_ a: [String: Any], document: ImageDocument,
                                  layer: Int) throws -> HistoryBrushSource {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        do {
            return try HistoryBrushSource.latch(in: document, current: doc, layer: layer,
                stateID: stringArg(a, "state_id"),
                tool: EditorViewController.artHistoryToolName)
        } catch { throw ToolError(message: error.localizedDescription) }
    }

    /// art_history_brush_stroke — mirrors the Art History Brush and its
    /// options bar (Mode, Opacity, Style, Area, Tolerance): paints the source
    /// state as the style's seeded strokes, one cluster per click along the
    /// stroke. `seed` makes the strokes reproducible; the reply's `applied`
    /// echoes every option.
    func artHistoryBrushStroke(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseArtHistoryTarget(a, document)
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        let (size, _) = try retouchBrushAmounts(a, key: "opacity", fallback: 100)
        let art = try ArtHistoryOptions.argument(a)
        let source = try artHistorySource(a, document: document, layer: index)
        let stroke = try smudgeDabs(a, size: size)
        let request: String
        do { request = try stroke.request } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let selection = selectionMask(document)
        var applied = art.arguments
        applied["size"] = Double(size)
        // The core op answers nil for "nothing changed" and throws its own
        // sentence for a stroke it refuses; both are latched here because
        // `performPixelEdit`'s transform cannot throw.
        var opRefused = false
        var failure: String?
        let rasterized: DroppedDescription?
        do {
            rasterized = try performPixelEdit(document, "Art History Brush", pixelLayer: index) {
                current in
                do {
                    let out = try current.artHistoryBrushLayer(index, source: source.document,
                        sourceLayer: source.layer, request: art.request, brushRequest: request,
                        dabs: stroke.dabs, selection: selection)
                    if out == nil { opRefused = true }
                    return out
                } catch {
                    failure = error.localizedDescription
                    return nil
                }
            }
        } catch let error as ToolError {
            if let failure { throw ToolError(message: failure) }
            guard opRefused else { throw error }
            return try jsonResult([
                "ok": true, "changed": false, "layer": index, "applied": applied,
                "note": "The Art History Brush changed no pixel: every stroke was skipped by the "
                    + "tolerance, missed layer \(index)'s pixels, or painted what was there. "
                    + "No undo step was added.",
            ])
        }
        return try pixelEditResult(
            ["ok": true, "action": "Art History Brush", "layer": index,
             "dabs": stroke.dabs.count / 4, "applied": applied],
            layer: index, rasterized: rasterized)
    }

    /// The typed twin, reached through `dispatchPrecision`: one job on an
    /// 8-bit CMYK document (`PrecisionStrokeWork.Kind.artHistory`); the plan
    /// refuses a 16-bit, 32-bit or 16-bit CMYK document by name.
    func precisionArtHistoryBrush(_ a: [String: Any], document: ImageDocument) throws -> String {
        try refuseArtHistoryTarget(a, document)
        let capture = try PrecisionEditorCapture(document), layer = try paintLayerIndex(a, document)
        let storage = capture.source.isCmyk ? "cmyka16_unorm" : capture.facade.isFloat ? "rgba32_float" : "u16"
        let (size, _) = try retouchBrushAmounts(a, key: "opacity", fallback: 100)
        let art = try ArtHistoryOptions.argument(a)
        // The depth first, in the core's sentence, as at mouse-down.
        try EditorViewController.refuseArtHistoryDepth(capture.source, layer: layer,
                                                        request: art.request)
        let native = try smudgeDabs(a, size: size)
        let source = try artHistorySource(a, document: document, layer: layer)
        guard let state = source.document.precision else {
            throw ToolError(message: "That history state is not at this document's bit depth.")
        }
        let selected = capture.selection != nil
        // The request only names the stroke; `kind` carries the work.
        let command = try PrecisionPixelCommand(["op": "retouch_brush", "layer": layer,
            "kind": "blur", "strength": art.opacity, "selection": selected])
        let work = try PrecisionStrokeWork(
            kind: .artHistory(source: state, sourceLayer: source.layer, request: art.request),
            layer: layer, source: capture.source, stroke: native, command: command,
            selection: selected ? capture.selection?.maskSamples16() : nil,
            width: capture.facade.width, height: capture.facade.height)
        let job = try DocumentJobs.startPrecisionStroke(work, document: document, capture: capture,
            layer: layer, title: "Art History Brush", record: .notACommand, showProgress: false,
            rasterizesTarget: true, completion: Self.noteStrokeJobChange)
        var applied = art.arguments
        applied["size"] = Double(size)
        job.details = ["storage": storage, "applied": applied,
                       "working_memory_plan_bytes": work.scratchEstimate]
        return try finishJobCall(job, tool: "art_history_brush_stroke", arguments: a, unchanged: {
            try self.jsonResult(["ok": true, "changed": false, "storage": storage,
                                 "applied": applied])
        })
    }
}
