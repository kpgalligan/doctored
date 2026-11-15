import AppKit

/// The Background Eraser over MCP (`designs/background-eraser.md`, `BackgroundEraser.swift`).
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `backgroundEraserStroke` | the Background Eraser's canvas stroke (`EditorViewController.backgroundEraserTick` at 8 bits, `PrecisionStrokeWork.Kind.backgroundEraser` typed) |
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let backgroundEraserHandlers: [String: ToolHandler] = [
        "background_eraser_stroke": { $0.backgroundEraserStroke },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. A layer's pixels at
    /// every depth and in every colour mode but Bitmap (`BitmapDocument.disabledTools`), gated by
    /// the selection.
    static let backgroundEraserToolFacts: [String: ToolFacts] = [
        "background_eraser_stroke": [.selectionSensitive, .typedTool, .rasterTool, .floatTool,
                                     .cmykTool],
    ]

    /// The stroke keys the tool has no use for (its opacity and flow are pinned to 100).
    static let backgroundEraserTipKeys = ["flow", "opacity", "airbrush", "pressure_opacity",
                                          "aliased"]

    /// Refuses a key of `backgroundEraserTipKeys` by name.
    static func refuseBackgroundEraserTipKeys(_ a: [String: Any]) throws {
        if backgroundEraserTipKeys.contains(where: { a[$0] != nil }) {
            throw ToolError(message: BackgroundEraser.tipRefusal)
        }
    }

    /// Mirrors the Background Eraser's stroke at 8 bits: the stroke's dabs (canonical
    /// `native_stroke`, or `points` walked at the tip's spacing) erased on the layer in one
    /// core call, inside the selection, as the canvas stroke's final tick computes. A stroke
    /// that changes nothing answers `changed: false` with no undo step.
    func backgroundEraserStroke(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        try refuseBackgroundEraserCall(a, document)
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        try refuseBackgroundEraserLayer(doc, index)
        let options = try backgroundEraserOptions(a, document: document)
        let stroke = try smudgeDabs(a, size: try backgroundEraserSize(a))
        let request: String
        let brushRequest: String
        do {
            request = try options.request(in: doc)
            brushRequest = try stroke.request
        } catch { throw ToolError(message: error.localizedDescription) }
        let selection = selectionMask(document)
        // The core op answers nil for "nothing changed" and throws its own sentence for a
        // stroke it refuses; both are latched here because `performPixelEdit`'s transform
        // cannot throw.
        var opRefused = false
        var failure: String?
        let rasterized: DroppedDescription?
        do {
            rasterized = try performPixelEdit(document, BackgroundEraser.name, pixelLayer: index) {
                current in
                do {
                    let out = try current.backgroundEraserLayer(index, request: request,
                        brushRequest: brushRequest, dabs: stroke.dabs, selection: selection)
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
                "ok": true, "changed": false, "layer": index, "storage": "rgba8",
                "applied": options.arguments,
                "note": "The Background Eraser changed no pixel: no dab found a like colour on "
                    + "layer \(index) inside the selection. No undo step was added.",
            ])
        }
        return try pixelEditResult(
            ["ok": true, "changed": true, "action": BackgroundEraser.name, "layer": index,
             "storage": "rgba8", "dabs": stroke.dabs.count / 4, "applied": options.arguments],
            layer: index, rasterized: rasterized)
    }

    /// The typed twin, reached through `dispatchPrecision`: one job, the core's coverage plane
    /// erased by `erase_native` (`PrecisionStrokeWork.Kind.backgroundEraser`), as the canvas
    /// stroke commits. An Indexed document's erased pixels take its transparent index, else the
    /// entry nearest white.
    func precisionBackgroundEraser(_ a: [String: Any], document: ImageDocument) throws -> String {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        try refuseBackgroundEraserCall(a, document)
        let capture = try PrecisionEditorCapture(document), layer = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, layer)
        try refuseBackgroundEraserLayer(doc, layer)
        let storage = capture.source.isCmyk ? "cmyka16_unorm"
            : capture.facade.isFloat ? "rgba32_float" : "rgba16_unorm"
        let options = try backgroundEraserOptions(a, document: document)
        let native = try smudgeDabs(a, size: try backgroundEraserSize(a))
        let request: String
        do { request = try options.request(in: capture.facade) } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let selected = capture.selection != nil
        let command = try PrecisionPixelCommand(["op": "erase_native", "layer": layer,
                                                 "opacity": 1, "selection": selected])
        let work = try PrecisionStrokeWork(
            kind: .backgroundEraser(request: request), layer: layer, source: capture.source,
            stroke: native, command: command,
            selection: selected ? capture.selection?.maskSamples16() : nil,
            width: capture.facade.width, height: capture.facade.height)
        let job = try DocumentJobs.startPrecisionStroke(work, document: document, capture: capture,
            layer: layer, title: BackgroundEraser.name, record: .notACommand, showProgress: false,
            rasterizesTarget: true, completion: Self.noteStrokeJobChange)
        job.details = ["storage": storage, "applied": options.arguments,
                       "working_memory_plan_bytes": work.scratchEstimate]
        return try finishJobCall(job, tool: "background_eraser_stroke", arguments: a, unchanged: {
            try self.jsonResult(["ok": true, "changed": false, "layer": layer, "storage": storage,
                                 "applied": options.arguments])
        })
    }

    /// The call's own refusals, by name, at every depth: a `blend_mode`, the tip keys the tool
    /// has no use for, a target that is not the layer, the Quick Mask.
    private func refuseBackgroundEraserCall(_ a: [String: Any], _ document: ImageDocument) throws {
        guard a["blend_mode"] == nil else { throw ToolError(message: BackgroundEraser.blendRefusal) }
        try Self.refuseBackgroundEraserTipKeys(a)
        guard (stringArg(a, "target") ?? "layer") == "layer" else {
            throw ToolError(message: BackgroundEraser.targetRefusal)
        }
        guard !document.quickMaskActive else {
            throw ToolError(message: OperationScope.quickMaskRefusal)
        }
    }

    /// A retained source or a pixels lock (`scopeRefusal`), then the transparency lock.
    private func refuseBackgroundEraserLayer(_ doc: RasterDocument, _ index: Int) throws {
        if let reason = scopeRefusal(.pixelOnly, target: .layer, doc: doc, layer: index) {
            throw ToolError(message: reason)
        }
        if let refusal = BackgroundEraser.transparencyRefusal(doc, layer: index) {
            throw ToolError(message: refusal.message + Self.lockHint(owner: refusal.owner))
        }
    }

    /// `size`: 1…1000 px, default the tool's (`BackgroundEraser.defaultSize`).
    private func backgroundEraserSize(_ a: [String: Any]) throws -> CGFloat {
        let size = doubleArg(a, "size") ?? BackgroundEraser.defaultSize
        let tool = EditorTool.backgroundEraser
        guard size.isFinite, (tool.minimumBrushSize...tool.maximumBrushSize).contains(size) else {
            throw ToolError(message: "background_eraser_stroke takes size "
                + "\(Int(tool.minimumBrushSize))…\(Int(tool.maximumBrushSize)) px.")
        }
        return CGFloat(size)
    }

    /// tolerance (0…255, default 32), limits (default contiguous), sampling (default
    /// continuous), sample_color / sample_ink (background_swatch only, then required) and
    /// protect_color / protect_ink (Protect Foreground).
    private func backgroundEraserOptions(_ a: [String: Any],
                                         document: ImageDocument) throws -> BackgroundEraserOptions {
        let tolerance = doubleArg(a, "tolerance") ?? 32
        guard tolerance.isFinite, (0...255).contains(tolerance),
              let limits = BackgroundEraserOptions.Limits(
                  rawValue: stringArg(a, "limits") ?? "contiguous"),
              let sampling = BackgroundEraserOptions.Sampling(
                  rawValue: stringArg(a, "sampling") ?? "continuous")
        else { throw ToolError(message: BackgroundEraser.argumentRefusal) }
        let swatch = try backgroundEraserColor(a, "sample", document: document)
        if sampling == .swatch, swatch == nil { throw ToolError(message: BackgroundEraser.swatchMissing) }
        if sampling != .swatch, swatch != nil { throw ToolError(message: BackgroundEraser.swatchUnused) }
        return BackgroundEraserOptions(
            tolerance: tolerance, limits: limits, sampling: sampling, swatch: swatch,
            protect: try backgroundEraserColor(a, "protect", document: document))
    }

    /// `<key>_color` (an sRGB hex) and, on a CMYK document, `<key>_ink` (C, M, Y, K percent,
    /// kept exactly); nil when neither is passed. `<key>_ink` on another document refuses.
    private func backgroundEraserColor(_ a: [String: Any], _ key: String,
                                       document: ImageDocument) throws -> BackgroundEraserColor? {
        let ink = try cmykInkArgument(a, "\(key)_ink")
        guard a["\(key)_color"] != nil || ink != nil else { return nil }
        if ink != nil, document.doc?.precision?.isCmyk != true {
            throw ToolError(message: "\(key)_ink names a CMYK document's ink; pass \(key)_color.")
        }
        return BackgroundEraserColor(
            color: try parseColor(a, "\(key)_color", fallback: .white), ink: ink)
    }
}
