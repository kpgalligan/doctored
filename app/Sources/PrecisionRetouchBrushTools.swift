import AppKit

/// The 16-bit twins of the five retouch-brush stroke tools (`AgentServer+RetouchBrushes.swift`),
/// reached through `dispatchPrecision`. Each commits as ONE `DocumentJobs` job, as the 16-bit canvas
/// stroke and `precisionDodgeBurn` do: Blur, Sharpen and Sponge as the core's `retouch_brush` edit
/// over the rasterized footprint, Smudge along the stroke's own dabs, and the History Brush as its
/// footprint painted from a history state (`PrecisionStrokeWork.Kind`).
///
/// A 32-bit float document takes the same requests for Blur, Sharpen, Sponge, Smudge and the
/// History Brush: the core's `float_retouch_brush` runs them on f32 in linear light with the range
/// kept, and the reply and the job's details say `storage: "rgba32_float"`. `sponge_stroke` aimed
/// at a mask or an alpha channel refuses at every depth with `FloatRetouch.spongeCoverageRefusal`.
/// `sponge_stroke`'s `vibrance` and Blur, Sharpen and Smudge's `sample_all_layers`
/// ride in the planned request (`retouch_brush`) or the Smudge kind, and the reply's `applied`.
extension AgentServer {
    func precisionRetouchBrush(_ tool: String, _ a: [String: Any],
                               document: ImageDocument) throws -> String {
        if tool != "history_brush_stroke" {
            try refuseMisplacedOption(a, sponge: tool == "sponge_stroke")
        }
        let sampleAllLayers = ["blur_stroke", "sharpen_stroke", "smudge_stroke"].contains(tool)
            ? try sampleAllLayersArg(a) : false
        let vibrance = tool == "sponge_stroke" && boolArg(a, "vibrance") == true
        // A mask or an alpha channel only; a colour plane keeps its scalar-target refusal.
        if tool == "sponge_stroke" {
            let target = try paintTarget(a, document)
            if target.isCoverage, !target.isPlane {
                throw ToolError(message: FloatRetouch.spongeCoverageRefusal)
            }
        }
        let capture = try PrecisionEditorCapture(document), layer = try paintLayerIndex(a, document)
        try refuseBlendMode(a, tool: tool)
        let storage = capture.source.isCmyk ? "cmyka16_unorm" : capture.facade.isFloat ? "rgba32_float" : "u16"
        let history = tool == "history_brush_stroke"
        let (size, percent) = try retouchBrushAmounts(
            a, key: history ? "opacity" : "strength", fallback: history ? 100 : 50)
        let native = try smudgeDabs(a, size: size)
        let selected = capture.selection != nil
        var kind = PrecisionStrokeWork.Kind.edit
        var brush = RetouchBrushKind.blur
        let title: String
        switch tool {
        case "blur_stroke":
            title = "Blur"
        case "sharpen_stroke":
            brush = .sharpen
            title = "Sharpen"
        case "sponge_stroke":
            brush = try spongeSaturates(a) ? .saturate : .desaturate
            title = "Sponge"
        case "smudge_stroke":
            kind = .smudge(strength: percent / 100, sampleAllLayers: sampleAllLayers)
            title = "Smudge"
        default:
            // `HistoryBrushSource.latch` has already matched the state's depth
            // to the document's (8, 16 or 32-bit float).
            let source = try historyBrushSource(a, document: document, layer: layer)
            guard let state = source.document.precision else {
                throw ToolError(message: "That history state is not at this document's bit depth.")
            }
            kind = .historyBrush(source: state, sourceLayer: source.layer, opacity: percent / 100)
            title = "History Brush"
        }
        // The planned request of Blur, Sharpen and Sponge; for the other two
        // it only names the stroke, and `kind` carries the work.
        var request: [String: Any] = ["op": "retouch_brush", "layer": layer,
            "kind": brush.requestName, "strength": percent / 100, "selection": selected]
        if vibrance { request["vibrance"] = true }
        if sampleAllLayers, tool != "smudge_stroke" { request["sample_all_layers"] = true }
        let command = try PrecisionPixelCommand(request)
        let work = try PrecisionStrokeWork(kind: kind, layer: layer, source: capture.source,
            stroke: native, command: command,
            selection: selected ? capture.selection?.maskSamples16() : nil,
            width: capture.facade.width, height: capture.facade.height)
        let job = try DocumentJobs.startPrecisionStroke(work, document: document, capture: capture,
            layer: layer, title: title, record: .notACommand, showProgress: false,
            rasterizesTarget: true, completion: Self.noteStrokeJobChange)
        var applied: [String: Any] = [:]
        if tool == "sponge_stroke" { applied["vibrance"] = vibrance }
        if tool != "sponge_stroke", !history { applied["sample_all_layers"] = sampleAllLayers }
        // The stroke's admitted plan (the footprint's and the edit's), which
        // `scripts/cmyk-admission/measure.py`'s Sample All Layers rows gate on.
        job.details = ["storage": storage, "applied": applied,
                       "working_memory_plan_bytes": work.scratchEstimate]
        return try finishJobCall(job, tool: tool, arguments: a, unchanged: {
            try self.jsonResult(["ok": true, "changed": false, "storage": storage,
                                 "applied": applied])
        })
    }
}
